"""Client for the ofs-sim gRPC server."""
from __future__ import annotations

import collections
import collections.abc
import math
import os
import pathlib
import shutil
import socket
import subprocess
import threading
import time
import weakref
from dataclasses import dataclass, field

import grpc

from ofs.v1 import sim_pb2 as pb
from ofs.v1 import sim_pb2_grpc as pbg

from .errors import InvalidState, ProtocolMismatch, from_rpc_error
from .reader import Log
from .recording import Recording

PROTOCOL_VERSION = 11
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
class FcTelemetry:
    """Betaflight's own telemetry as the handset received it over the ELRS downlink.

    telemetry_age_s is INFINITY until the first frame arrives (and always INFINITY without SITL).
    """
    battery_v: float = 0.0
    battery_a: float = 0.0
    battery_mah: float = 0.0
    roll_deg: float = 0.0
    pitch_deg: float = 0.0
    yaw_deg: float = 0.0
    flight_mode: str = ""
    frame_count: int = 0
    telemetry_age_s: float = float("inf")


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
    field_valid: bool = False  # false: the receiver dropped this field (marginal signal)
    latency_ms: int = 0  # the goggles' glass-to-glass latency, quantized down to whole fields


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
class Gate:
    """A race gate: a vertical opening flown through in the direction of `heading_deg` (0 = north, clockwise)."""
    name: str
    center_ned_m: tuple
    heading_deg: float
    width_m: float
    height_m: float


@dataclass(frozen=True)
class World:
    """The field a session flies in: where the pilot stands, the goggles' antennas, the objects, other transmitters
    and the race gates (in course order; gate 0 is the start and finish)."""
    name: str
    pilot_position_ned_m: tuple
    pilot_facing_deg: float
    antennas: tuple
    objects: tuple
    emitters: tuple
    handset: Handset = field(default_factory=lambda: Handset((), ()))
    gates: tuple = ()


@dataclass(frozen=True)
class Race:
    """The lap clock. `present` is False in a world without gates (everything else is then zero). `last_lap_s` and
    `best_lap_s` are 0 until a lap completes (`laps > 0`)."""
    present: bool = False
    running: bool = False  # gate 0 has started the clock
    next_gate: int = 0  # the gate to fly through next
    gate_count: int = 0
    laps: int = 0  # completed
    current_lap_s: float = 0.0  # time since the current lap started
    last_lap_s: float = 0.0
    best_lap_s: float = 0.0


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
    fc_telemetry: FcTelemetry = field(default_factory=FcTelemetry)
    running: bool = False
    overruns: int = 0
    fc_restarts: int = 0
    vtx: Vtx = field(default_factory=Vtx)
    serial_dropped_bytes: int = 0
    video: VideoLink = field(default_factory=VideoLink)
    collision_speed_mps: float = 0.0  # the last collision's inward speed (0 until one happens)
    battery_cell_voltages: tuple = ()  # per-cell voltage (each cell's own state; the weak cell droops)
    race: Race = field(default_factory=Race)

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
                     sparkles=m.sparkles, chroma=m.chroma, sync=_SYNC_NAMES.get(m.sync, ""),
                     field_valid=m.field_valid, latency_ms=m.latency_ms)


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
        gates=tuple(Gate(g.name, _v(g.center_ned_m), g.heading_deg, g.width_m, g.height_m) for g in m.gates),
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
        fc_telemetry=FcTelemetry(m.fc_telemetry.battery_v, m.fc_telemetry.battery_a, m.fc_telemetry.battery_mah,
                                 m.fc_telemetry.roll_deg, m.fc_telemetry.pitch_deg, m.fc_telemetry.yaw_deg,
                                 m.fc_telemetry.flight_mode, m.fc_telemetry.frame_count,
                                 m.fc_telemetry.telemetry_age_s),
        running=m.running,
        overruns=m.overruns,
        fc_restarts=m.fc_restarts,
        vtx=Vtx(m.vtx.present, m.vtx.band, m.vtx.channel, m.vtx.freq_mhz, m.vtx.power_mw, m.vtx.pit_mode),
        serial_dropped_bytes=m.serial_dropped_bytes,
        video=_video(m.video),
        collision_speed_mps=m.collision_speed_mps,
        battery_cell_voltages=tuple(m.battery_cell_voltages),
        race=Race(m.race.present, m.race.running, m.race.next_gate, m.race.gate_count, m.race.laps,
                  m.race.current_lap_s, m.race.last_lap_s, m.race.best_lap_s),
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
        self.recording = Recording(self)
        try:
            reply = self._call(self._stub.Handshake, pb.HandshakeRequest(protocol_version=PROTOCOL_VERSION))
            if reply.protocol_version != PROTOCOL_VERSION:
                raise ProtocolMismatch(f"server speaks protocol {reply.protocol_version}, client {PROTOCOL_VERSION}")
        except Exception:
            self._channel.close()
            raise
        self.logs_dir = reply.logs_dir  # where the server writes recordings and copies Blackbox logs
        # A launched server stops exactly once: at close(), when this Sim is collected, or at interpreter exit,
        # whichever comes first (a script need not close it). The finalizer holds no reference to the Sim.
        self._stop_server = weakref.finalize(self, _stop_server, self._stub, self._channel, process) if process else None

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
             overrun_policy: str = "warn", keep_alive: bool = False, world: str | None = None,
             record: bool | None = None) -> str:
        """Loads a quad. `mode` is "lockstep" (advance with `run`) or "realtime" (loads paused; `start` paces it
        to the wall clock). `world` is a world file (e.g. "worlds/flat.toml") or a world name ("flat":
        worlds/flat.toml beside the quad's directory); without one the quad flies in the open field. `record`
        starts a recording of every signal at the default rates (`sim.log` reads it); it defaults to on for
        lockstep sessions and off for real-time ones. Returns the quad's name; `configurator_address` is set when
        it runs Betaflight."""
        if mode not in _MODES:
            raise ValueError(f"mode must be one of {sorted(_MODES)}")
        if overrun_policy not in _POLICIES:
            raise ValueError(f"overrun_policy must be one of {sorted(_POLICIES)}")
        self._ensure_watch()
        reply = self._call(self._stub.Load, pb.LoadRequest(
            quad_path=str(quad_path), seed=seed, mode=_MODES[mode], open_loop_fc=open_loop_fc,
            overrun_policy=_POLICIES[overrun_policy], keep_alive=keep_alive, world_path=str(world or "")))
        self.configurator_address = reply.configurator_address
        self.recording._session_ended()
        if (mode == "lockstep") if record is None else record:
            self.recording.start()
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
        """Clears every active fault, and the ones scheduled with `at` that have not activated yet."""
        self._call(self._stub.ClearFaults, pb.Empty())

    def play_inputs(self, path) -> None:
        """Plays a stick-inputs CSV from now (spec §5): a `t_s` column (seconds after this call, increasing) and any
        of `roll`, `pitch`, `yaw`, `throttle`, `aux1`..`aux4`. Each row holds until the next; columns the file
        leaves out keep their value; the last row holds after it ends. `set_sticks` is refused while it plays. A
        malformed file raises `ConfigError` naming every problem with its line."""
        p = pathlib.Path(path)
        self._call(self._stub.PlayInputs, pb.InputsFile(name=p.name, csv=p.read_text(encoding="utf-8")))

    def stop_inputs(self) -> None:
        """Stops the playing inputs file, if any (the sticks keep their last values)."""
        self._call(self._stub.StopInputs, pb.Empty())

    def record_inputs(self) -> str:
        """Starts recording this session's sticks to a CSV the server names under `logs_dir` (one row whenever a
        stick changes, in `play_inputs`' format) and returns its path. Fly the session (the Godot game's sticks, or
        `set_sticks` calls) and finish with `stop_inputs_recording()`; `play_inputs(path)` then replays the sticks in
        a lockstep session. One recording at a time; refused while an inputs file plays, and `play_inputs` is refused
        while it runs."""
        return self._call(self._stub.StartInputsRecording, pb.Empty()).path

    def stop_inputs_recording(self) -> str:
        """Finishes the inputs recording and returns its path (the file is complete from then on)."""
        return self._call(self._stub.StopInputsRecording, pb.Empty()).path

    @property
    def log(self) -> Log:
        """The session's recording, as a `reader.Log` (`sim.log.to_dataframe(["imu.*"])`). A running recording is
        stopped first: an MCAP file is readable once it is finished."""
        if self.recording.active:
            return self.recording.stop()
        if self.recording.session_last is None:
            raise InvalidState("this session has no recording: a lockstep load records by default, or call "
                               "sim.recording.start() (an earlier session's file is sim.recording.last)")
        return self.recording.session_last

    def blackbox_files(self) -> list:
        """Betaflight's own Blackbox logs the server copied into its logs directory, sorted. A session's logs
        arrive when it ends (`close()`, or the next `load`); disarm before then for a complete log."""
        return sorted(str(p) for p in pathlib.Path(self.logs_dir).glob("*.BFL"))

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
        finally:
            if self._stop_server is not None:
                self._stop_server()  # unloads, then stops the launched server; a no-op the second time
            else:
                self._channel.close()

    def __enter__(self) -> "Sim":
        return self

    def __exit__(self, *exc) -> None:
        self.close()


def connect(address: str = "127.0.0.1:50051") -> Sim:
    return Sim(address)


def _stop_server(stub, channel, process) -> None:
    """Unloads (stopping Betaflight SITL cleanly; terminating the server alone would orphan it on Windows, where
    TerminateProcess skips its cleanup), then stops a server `launch` started."""
    try:
        stub.Unload(pb.Empty(), timeout=UNLOAD_TIMEOUT_S)
    except Exception:  # best effort: the server may already be gone or failing
        pass
    finally:
        try:
            channel.close()
        finally:
            process.terminate()
            try:
                process.wait(5)
            except subprocess.TimeoutExpired:
                process.kill()


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def find_binary(binary: str | None = None) -> str:
    """The `ofs-sim` to run: `binary`, else `OFS_SIM_BIN`, else the one on PATH."""
    binary = binary or os.environ.get("OFS_SIM_BIN") or shutil.which("ofs-sim")
    if not binary:
        raise FileNotFoundError("ofs-sim binary not found: set OFS_SIM_BIN or put ofs-sim on PATH")
    if "/" in binary or os.sep in binary:
        # A path, not a name to look up: absolute, with native separators. Windows' CreateProcess refuses a
        # relative path written with forward slashes (`target/debug/ofs-sim`).
        binary = os.path.abspath(binary)
    return binary


def launch(binary: str | None = None, headless: bool = True, data_dir: str = ".ofs-data",
           timeout_s: float = 15.0) -> Sim:
    """Starts an ofs-sim server on a free local port and connects to it. The server stops with `close()`, when the
    returned Sim is garbage-collected, or when the script exits (a script need not close it: its session's
    recording and Blackbox logs are finished either way)."""
    if not headless:
        raise NotImplementedError("the Godot pilot client arrives with M2b; only headless=True is available")
    binary = find_binary(binary)
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
        sim = Sim(address, proc)
    except BaseException:  # e.g. ProtocolMismatch: don't leave the server running
        proc.kill()
        proc.wait()
        raise
    return sim
