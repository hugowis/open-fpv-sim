"""Client for the ofs-sim gRPC server."""
from __future__ import annotations

import collections
import collections.abc
import math
import os
import shutil
import socket
import subprocess
import threading
import time
from dataclasses import dataclass, field

import grpc

from ofs.v1 import sim_pb2 as pb
from ofs.v1 import sim_pb2_grpc as pbg

from .errors import ProtocolMismatch, from_rpc_error

PROTOCOL_VERSION = 5
UNLOAD_TIMEOUT_S = 5.0

_MODES = {"lockstep": pb.MODE_LOCKSTEP, "realtime": pb.MODE_REALTIME}
_POLICIES = {"warn": pb.OVERRUN_POLICY_WARN, "slow": pb.OVERRUN_POLICY_SLOW}


@dataclass(frozen=True)
class RadioLink:
    tx_enabled: bool = False
    link_up: bool = False
    lq_pct: float = 0.0
    rssi_dbm: float = 0.0  # at the antenna in use
    snr_db: float = 0.0  # may be negative: LoRa decodes below the noise
    active_antenna: str = ""
    downlink_lq_pct: float = 0.0  # what the handset receives


@dataclass(frozen=True)
class Vtx:
    """The video transmitter (all zero when the quad has none)."""
    present: bool = False
    band: int = 0  # 1..6 = A, B, E, F, R, L; 0 in user-frequency mode
    channel: int = 0  # 1..8; 0 in user-frequency mode
    freq_mhz: int = 0
    power_mw: int = 0
    pit_mode: bool = False

    @property
    def band_letter(self) -> str:
        return "ABEFRL"[self.band - 1] if 1 <= self.band <= 6 else ""


class AntennaRssi(collections.abc.Mapping):
    """Received signal in dBm by goggle antenna name, in the world file's order. A read-only mapping that, unlike a
    dict, can be hashed, so a `State` can be a set member or a dictionary key."""

    __slots__ = ("_items",)

    def __init__(self, items=()):
        self._items = tuple((str(k), float(v)) for k, v in (items.items() if isinstance(items, dict) else items))

    def __getitem__(self, name):
        for k, v in self._items:
            if k == name:
                return v
        raise KeyError(name)

    def __iter__(self):
        return (k for k, _ in self._items)

    def __len__(self):
        return len(self._items)

    def __hash__(self):
        return hash(frozenset(self._items))  # equal mappings (compared as dicts) hash alike, in any order

    def __repr__(self):
        return f"AntennaRssi({dict(self._items)!r})"


@dataclass(frozen=True)
class VideoLink:
    """The analog video link at the goggles (`present` is False when the quad has no VTX)."""
    present: bool = False
    snr_db: float = 0.0
    interference_dbm: float = 0.0
    rssi: AntennaRssi = field(default_factory=AntennaRssi)  # dBm by goggle antenna name, in the world file's order
    active_antenna: str = ""
    noise: float = 0.0  # grain, 0 (clean) to 1 (static)
    sparkles: float = 0.0
    chroma: float = 1.0  # colour, 1 (full) to 0 (black and white)
    sync: str = ""  # "locked", "unstable" (tearing), "lost" (rolling, static); "" without a VTX


@dataclass(frozen=True)
class ReceiverAntenna:
    name: str
    kind: str  # "omni" or "patch"
    gain_dbi: float
    beamwidth_deg: float  # patch only, else 0
    polarization: str  # "rhcp", "lhcp" or "linear"
    aim_az_deg: float
    aim_el_deg: float


@dataclass(frozen=True)
class WorldObject:
    name: str
    shape: str  # "box" or "cylinder"
    center_ned_m: tuple
    size_m: tuple  # box: (north, east, height)
    radius_m: float  # cylinder
    height_m: float  # cylinder
    color: tuple  # (r, g, b) in 0..1
    rf_loss_db: float


@dataclass(frozen=True)
class Emitter:
    name: str
    position_ned_m: tuple
    freq_mhz: float
    power_mw: float


@dataclass(frozen=True)
class Handset:
    """The pilot's handset: where it is and what transmits from it (the ELRS uplink)."""
    position_ned_m: tuple
    antennas: tuple


@dataclass(frozen=True)
class World:
    """The field a session flies in: where the pilot stands, the goggles' antennas, the objects, other transmitters."""
    name: str
    pilot_position_ned_m: tuple
    pilot_facing_deg: float
    antennas: tuple
    objects: tuple
    emitters: tuple
    handset: Handset = field(default_factory=lambda: Handset((), ()))


@dataclass(frozen=True)
class Osd:
    """Betaflight's OSD as a character grid. `cells` are row-major, each `char | page << 8 | blink << 10`."""
    seq: int
    time_s: float
    present: bool
    cols: int
    rows: int
    cells: tuple

    def char(self, row: int, col: int) -> int:
        return self.cells[row * self.cols + col] & 0xFF

    def page(self, row: int, col: int) -> int:
        return (self.cells[row * self.cols + col] >> 8) & 0x3

    def blink(self, row: int, col: int) -> bool:
        return bool((self.cells[row * self.cols + col] >> 10) & 1)

    def rows_text(self) -> list:
        """The rows as text. Printable ASCII is shown as is; Betaflight's symbols (battery icon, units) as `?`."""
        out = []
        for r in range(self.rows):
            out.append("".join(chr(c) if 0x20 <= c <= 0x7E else "?" for c in (self.char(r, k) for k in range(self.cols))))
        return out

    @property
    def text(self) -> str:
        return "\n".join(self.rows_text())


@dataclass(frozen=True)
class State:
    time_s: float
    position_ned_m: tuple
    velocity_ned_mps: tuple
    attitude_wxyz: tuple
    rate_frd_radps: tuple
    battery_voltage_v: float
    battery_current_a: float
    motor_rpm: tuple
    motor_cmd: tuple
    radio: RadioLink = field(default_factory=RadioLink)
    running: bool = False
    overruns: int = 0
    fc_restarts: int = 0
    vtx: Vtx = field(default_factory=Vtx)
    serial_dropped_bytes: int = 0
    video: VideoLink = field(default_factory=VideoLink)
    collision_speed_mps: float = 0.0  # the last collision's inward speed (0 until one happens)

    @property
    def altitude_m(self) -> float:
        return -self.position_ned_m[2]

    def euler_deg(self) -> tuple:
        """(roll, pitch, yaw) in degrees, aerospace Z-Y-X convention, FRD body / NED world."""
        w, x, y, z = self.attitude_wxyz
        roll = math.degrees(math.atan2(2 * (w * x + y * z), 1 - 2 * (x * x + y * y)))
        pitch = math.degrees(math.asin(max(-1.0, min(1.0, 2 * (w * y - z * x)))))
        yaw = math.degrees(math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z)))
        return roll, pitch, yaw


@dataclass(frozen=True)
class Event:
    time_s: float
    kind: str  # e.g. "link_down", "firmware_restarted", "overrun", "session_ended", "vtx_changed", "video_lost";
    # "watch_ended" is the client's own (see Sim.events)
    message: str


def _v(m) -> tuple:
    return (m.x, m.y, m.z)


_SYNC_NAMES = {pb.VIDEO_SYNC_LOCKED: "locked", pb.VIDEO_SYNC_UNSTABLE: "unstable", pb.VIDEO_SYNC_LOST: "lost"}


def _video(m) -> VideoLink:
    return VideoLink(present=m.present, snr_db=m.snr_db, interference_dbm=m.interference_dbm,
                     rssi=AntennaRssi((r.name, r.rssi_dbm) for r in m.rssi), active_antenna=m.active_antenna, noise=m.noise,
                     sparkles=m.sparkles, chroma=m.chroma, sync=_SYNC_NAMES.get(m.sync, ""))


def _world(m) -> World:
    return World(
        name=m.name,
        pilot_position_ned_m=_v(m.pilot_position_ned_m),
        pilot_facing_deg=m.pilot_facing_deg,
        antennas=tuple(ReceiverAntenna(a.name, a.kind, a.gain_dbi, a.beamwidth_deg, a.polarization, a.aim_az_deg,
                                       a.aim_el_deg) for a in m.antennas),
        objects=tuple(WorldObject(o.name, o.shape, _v(o.center_ned_m), _v(o.size_m), o.radius_m, o.height_m, _v(o.color),
                                  o.rf_loss_db) for o in m.objects),
        emitters=tuple(Emitter(e.name, _v(e.position_ned_m), e.freq_mhz, e.power_mw) for e in m.emitters),
        handset=Handset(
            position_ned_m=_v(m.handset.position_ned_m),
            antennas=tuple(ReceiverAntenna(a.name, a.kind, a.gain_dbi, a.beamwidth_deg, a.polarization, a.aim_az_deg,
                                           a.aim_el_deg) for a in m.handset.antennas),
        ) if m.HasField("handset") else Handset((), ()),
    )


def _state(m) -> State:
    return State(
        time_s=m.time_s,
        position_ned_m=_v(m.position_ned_m),
        velocity_ned_mps=_v(m.velocity_ned_mps),
        attitude_wxyz=(m.attitude.w, m.attitude.x, m.attitude.y, m.attitude.z),
        rate_frd_radps=_v(m.rate_frd_radps),
        battery_voltage_v=m.battery_voltage_v,
        battery_current_a=m.battery_current_a,
        motor_rpm=tuple(m.motor_rpm),
        motor_cmd=tuple(m.motor_cmd),
        radio=RadioLink(m.radio.tx_enabled, m.radio.link_up, m.radio.lq_pct, m.radio.rssi_dbm,
                        m.radio.snr_db, m.radio.active_antenna, m.radio.downlink_lq_pct),
        running=m.running,
        overruns=m.overruns,
        fc_restarts=m.fc_restarts,
        vtx=Vtx(m.vtx.present, m.vtx.band, m.vtx.channel, m.vtx.freq_mhz, m.vtx.power_mw, m.vtx.pit_mode),
        serial_dropped_bytes=m.serial_dropped_bytes,
        video=_video(m.video),
        collision_speed_mps=m.collision_speed_mps,
    )


def _osd(m) -> Osd:
    return Osd(seq=m.seq, time_s=m.time_s, present=m.present, cols=m.cols, rows=m.rows, cells=tuple(m.cells))


def _kind_name(kind: int) -> str:
    """The event kind as a lower-case name; a kind this client does not know (a newer server) is `unknown_<n>`."""
    try:
        return pb.EventKind.Name(kind).removeprefix("EVENT_KIND_").lower()
    except ValueError:
        return f"unknown_{kind}"


def _event(m) -> Event:
    return Event(time_s=m.time_s, kind=_kind_name(m.kind), message=m.message)


class Sim:
    """A connection to one ofs-sim server (and the process, if launched by `launch`).

    A session this client loads ends when the client disconnects (`close()`, or the script exits), unless it
    was loaded with `keep_alive=True`.
    """

    def __init__(self, address: str, process: subprocess.Popen | None = None):
        self.address = address
        self.configurator_address = ""
        self._process = process
        self._channel = grpc.insecure_channel(address)
        self._stub = pbg.SimStub(self._channel)
        self._watch = None
        self._events = collections.deque(maxlen=10_000)
        try:
            reply = self._call(self._stub.Handshake, pb.HandshakeRequest(protocol_version=PROTOCOL_VERSION))
            if reply.protocol_version != PROTOCOL_VERSION:
                raise ProtocolMismatch(f"server speaks protocol {reply.protocol_version}, client {PROTOCOL_VERSION}")
        except Exception:
            self._channel.close()
            raise

    @staticmethod
    def _call(method, request):
        try:
            return method(request)
        except grpc.RpcError as e:
            raise from_rpc_error(e) from None

    def _ensure_watch(self) -> None:
        """Opens the event stream, unless it is open. It also tells the server this client is alive.

        A stream that ends while the client is open (the server dropped it) is reported as a `watch_ended` event and
        opened again by the next `load`."""
        if self._watch is not None:
            return
        stream = self._stub.Watch(pb.Empty())
        self._watch = stream

        def pump(events=self._events):
            reason = "the server closed the event stream"
            try:
                for m in stream:
                    events.append(_event(m))
            except grpc.RpcError as e:
                reason = f"the event stream failed: {e.code().name}"
            if self._watch is stream:  # not closed by close()
                self._watch = None
                events.append(Event(time_s=math.nan, kind="watch_ended", message=f"{reason}; events are missed until "
                                                                                   "the next load()"))

        threading.Thread(target=pump, name="ofs-watch", daemon=True).start()

    def load(self, quad_path: str, seed: int = 0, mode: str = "lockstep", open_loop_fc: bool = False,
             overrun_policy: str = "warn", keep_alive: bool = False, world: str | None = None) -> str:
        """Loads a quad. `mode` is "lockstep" (advance with `run`) or "realtime" (loads paused; `start` paces it
        to the wall clock). `world` is a world file (e.g. "worlds/flat.toml"); without one the quad flies in the
        open field. Returns the quad's name; `configurator_address` is set when it runs Betaflight."""
        if mode not in _MODES:
            raise ValueError(f"mode must be one of {sorted(_MODES)}")
        if overrun_policy not in _POLICIES:
            raise ValueError(f"overrun_policy must be one of {sorted(_POLICIES)}")
        self._ensure_watch()
        reply = self._call(self._stub.Load, pb.LoadRequest(
            quad_path=str(quad_path), seed=seed, mode=_MODES[mode], open_loop_fc=open_loop_fc,
            overrun_policy=_POLICIES[overrun_policy], keep_alive=keep_alive, world_path=str(world or "")))
        self.configurator_address = reply.configurator_address
        return reply.quad_name

    def set_sticks(self, roll=0.0, pitch=0.0, yaw=0.0, throttle=0.0, aux=(-1.0, -1.0, -1.0, -1.0)) -> None:
        """Replaces the *whole* stick state: every argument left out goes back to its default.

        Sticks are in [-1, 1] (throttle in [0, 1]); `aux` holds up to 4 channels, missing ones read -1.
        The defaults include `aux=(-1, -1, -1, -1)`, so after arming, `set_sticks(throttle=0.6)` drops the
        arm switch and **disarms** the quad. Pass the aux channels on every call, e.g.
        `sim.set_sticks(throttle=0.6, aux=(1.0, 1.0, -1.0, -1.0))`.
        """
        self._call(self._stub.SetSticks, pb.Sticks(roll=roll, pitch=pitch, yaw=yaw, throttle=throttle, aux=list(aux)))

    def run(self, seconds: float) -> State:
        """Steps a lockstep session (or a paused real-time one) by `seconds` of simulated time."""
        return _state(self._call(self._stub.Run, pb.RunRequest(seconds=seconds)))

    def start(self) -> None:
        """Real-time sessions: run paced to the wall clock."""
        self._call(self._stub.Start, pb.Empty())

    def pause(self) -> None:
        self._call(self._stub.Pause, pb.Empty())

    def state(self) -> State:
        return _state(self._call(self._stub.GetState, pb.Empty()))

    def get_osd(self) -> Osd:
        """The current OSD frame (`present` is False without an OSD)."""
        return _osd(self._call(self._stub.GetOsd, pb.Empty()))

    def get_world(self) -> World:
        """The world the session flies in, as the server loaded it (the open field when `load` named none)."""
        return _world(self._call(self._stub.GetWorld, pb.Empty()))

    def stream_states(self, rate_hz: int = 60):
        """Yields the state at `rate_hz` (1..240) until you stop iterating or the session ends."""
        call = self._stub.StreamState(pb.StreamRequest(rate_hz=rate_hz))
        try:
            for m in call:
                yield _state(m)
        except grpc.RpcError as e:
            if e.code() != grpc.StatusCode.CANCELLED:
                raise from_rpc_error(e) from None
        finally:
            call.cancel()

    def stream_osd(self, rate_hz: int = 60):
        """Yields the OSD when it changes (and once at the start), checked at up to `rate_hz` (1..60)."""
        call = self._stub.StreamOsd(pb.StreamRequest(rate_hz=rate_hz))
        try:
            for m in call:
                yield _osd(m)
        except grpc.RpcError as e:
            if e.code() != grpc.StatusCode.CANCELLED:
                raise from_rpc_error(e) from None
        finally:
            call.cancel()

    def inject(self, fault) -> None:
        """Activates a fault from `ofs.faults` until `clear_faults()`."""
        self._call(self._stub.InjectFault, fault._to_pb())

    def clear_faults(self) -> None:
        self._call(self._stub.ClearFaults, pb.Empty())

    def events(self) -> list:
        """Events received since the last call (radio link up/down, firmware restarts, overruns, ...).

        Each event is returned once: a second caller (another thread) does not see the events the first one took.
        `watch_ended` (time_s NaN) means the client stopped receiving events until its next `load`."""
        out = []
        while self._events:
            out.append(self._events.popleft())
        return out

    def close(self) -> None:
        """Disconnects (ending this client's session unless it was loaded with keep_alive). For a server started
        by `launch`, first unloads the quad and then stops the server.

        Unloading stops Betaflight SITL cleanly; terminating the server alone would orphan it on Windows
        (TerminateProcess skips the server's cleanup).
        """
        try:
            if self._watch is not None:
                watch, self._watch = self._watch, None
                watch.cancel()
            if self._process is not None:
                try:
                    self._stub.Unload(pb.Empty(), timeout=UNLOAD_TIMEOUT_S)
                except Exception:  # best effort: the server may already be gone or failing
                    pass
        finally:
            try:
                self._channel.close()
            finally:
                if self._process is not None:
                    process, self._process = self._process, None
                    process.terminate()
                    try:
                        process.wait(5)
                    except subprocess.TimeoutExpired:
                        process.kill()

    def __enter__(self) -> "Sim":
        return self

    def __exit__(self, *exc) -> None:
        self.close()


def connect(address: str = "127.0.0.1:50051") -> Sim:
    return Sim(address)


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def launch(binary: str | None = None, headless: bool = True, data_dir: str = ".ofs-data",
           timeout_s: float = 15.0) -> Sim:
    """Starts an ofs-sim server on a free local port and connects to it."""
    if not headless:
        raise NotImplementedError("the Godot pilot client arrives with M2b; only headless=True is available")
    binary = binary or os.environ.get("OFS_SIM_BIN") or shutil.which("ofs-sim")
    if not binary:
        raise FileNotFoundError("ofs-sim binary not found: set OFS_SIM_BIN or put ofs-sim on PATH")
    address = f"127.0.0.1:{_free_port()}"
    proc = subprocess.Popen([binary, "--listen", address, "--data-dir", data_dir])
    deadline = time.monotonic() + timeout_s
    channel = grpc.insecure_channel(address)
    try:
        while True:
            if proc.poll() is not None:
                raise RuntimeError(f"ofs-sim exited with code {proc.returncode} during startup")
            try:
                grpc.channel_ready_future(channel).result(timeout=0.2)
                break
            except grpc.FutureTimeoutError:
                if time.monotonic() > deadline:
                    proc.kill()
                    raise RuntimeError(f"ofs-sim did not accept connections on {address} within {timeout_s} s")
    finally:
        channel.close()
    try:
        return Sim(address, proc)
    except BaseException:  # e.g. ProtocolMismatch: don't leave the server running
        proc.kill()
        proc.wait()
        raise
