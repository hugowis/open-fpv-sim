"""Client for the ofs-sim gRPC server."""
from __future__ import annotations

import math
import os
import shutil
import socket
import subprocess
import time
from dataclasses import dataclass

import grpc

from ofs.v1 import sim_pb2 as pb
from ofs.v1 import sim_pb2_grpc as pbg

from .errors import ProtocolMismatch, from_rpc_error

PROTOCOL_VERSION = 1


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
    )


class Sim:
    """A connection to one ofs-sim server (and the process, if launched by `launch`)."""

    def __init__(self, address: str, process: subprocess.Popen | None = None):
        self.address = address
        self._process = process
        self._channel = grpc.insecure_channel(address)
        self._stub = pbg.SimStub(self._channel)
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

    def load(self, quad_path: str, seed: int = 0, mode: str = "lockstep", open_loop_fc: bool = False) -> str:
        if mode != "lockstep":
            raise ValueError("only mode='lockstep' is available in this version")
        reply = self._call(self._stub.Load, pb.LoadRequest(quad_path=str(quad_path), seed=seed,
                                                           mode=pb.MODE_LOCKSTEP, open_loop_fc=open_loop_fc))
        return reply.quad_name

    def set_sticks(self, roll=0.0, pitch=0.0, yaw=0.0, throttle=0.0, aux=(-1.0, -1.0, -1.0, -1.0)) -> None:
        self._call(self._stub.SetSticks, pb.Sticks(roll=roll, pitch=pitch, yaw=yaw, throttle=throttle, aux=list(aux)))

    def run(self, seconds: float) -> State:
        return _state(self._call(self._stub.Run, pb.RunRequest(seconds=seconds)))

    def state(self) -> State:
        return _state(self._call(self._stub.GetState, pb.Empty()))

    def close(self) -> None:
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
        raise NotImplementedError("the Godot pilot client arrives in M2; only headless=True is available")
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
    return Sim(address, proc)
