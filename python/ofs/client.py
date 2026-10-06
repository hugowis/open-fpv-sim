"""Client for the ofs-sim gRPC server."""
from __future__ import annotations

import collections
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

PROTOCOL_VERSION = 2
UNLOAD_TIMEOUT_S = 5.0

_MODES = {"lockstep": pb.MODE_LOCKSTEP, "realtime": pb.MODE_REALTIME}
_POLICIES = {"warn": pb.OVERRUN_POLICY_WARN, "slow": pb.OVERRUN_POLICY_SLOW}


@dataclass(frozen=True)
class RadioLink:
    tx_enabled: bool = False
    link_up: bool = False
    lq_pct: float = 0.0
    rssi_dbm: float = 0.0


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
    kind: str  # e.g. "link_down", "firmware_restarted", "overrun", "session_ended"
    message: str


def _v(m) -> tuple:
    return (m.x, m.y, m.z)


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
        radio=RadioLink(m.radio.tx_enabled, m.radio.link_up, m.radio.lq_pct, m.radio.rssi_dbm),
        running=m.running,
        overruns=m.overruns,
        fc_restarts=m.fc_restarts,
    )


def _event(m) -> Event:
    name = pb.EventKind.Name(m.kind).removeprefix("EVENT_KIND_").lower()
    return Event(time_s=m.time_s, kind=name, message=m.message)


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
        """Opens the event stream (once). It also tells the server this client is alive."""
        if self._watch is not None:
            return
        self._watch = self._stub.Watch(pb.Empty())

        def pump(stream=self._watch, events=self._events):
            try:
                for m in stream:
                    events.append(_event(m))
            except grpc.RpcError:
                pass  # cancelled by close(), or the server went away

        threading.Thread(target=pump, name="ofs-watch", daemon=True).start()

    def load(self, quad_path: str, seed: int = 0, mode: str = "lockstep", open_loop_fc: bool = False,
             overrun_policy: str = "warn", keep_alive: bool = False) -> str:
        """Loads a quad. `mode` is "lockstep" (advance with `run`) or "realtime" (loads paused; `start` paces it
        to the wall clock). Returns the quad's name; `configurator_address` is set when it runs Betaflight."""
        if mode not in _MODES:
            raise ValueError(f"mode must be one of {sorted(_MODES)}")
        if overrun_policy not in _POLICIES:
            raise ValueError(f"overrun_policy must be one of {sorted(_POLICIES)}")
        self._ensure_watch()
        reply = self._call(self._stub.Load, pb.LoadRequest(
            quad_path=str(quad_path), seed=seed, mode=_MODES[mode], open_loop_fc=open_loop_fc,
            overrun_policy=_POLICIES[overrun_policy], keep_alive=keep_alive))
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

    def inject(self, fault) -> None:
        """Activates a fault from `ofs.faults` until `clear_faults()`."""
        self._call(self._stub.InjectFault, fault._to_pb())

    def clear_faults(self) -> None:
        self._call(self._stub.ClearFaults, pb.Empty())

    def events(self) -> list:
        """Events received since the last call (radio link up/down, firmware restarts, overruns, ...)."""
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
        if self._watch is not None:
            self._watch.cancel()
            self._watch = None
        if self._process is not None:
            try:
                self._stub.Unload(pb.Empty(), timeout=UNLOAD_TIMEOUT_S)
            except Exception:  # best effort: the server may already be gone or failing
                pass
        self._channel.close()
        if self._process is not None:
            self._process.terminate()
            try:
                self._process.wait(5)
            except subprocess.TimeoutExpired:
                self._process.kill()
            self._process = None

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
