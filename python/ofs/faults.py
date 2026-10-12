"""Faults to inject with `Sim.inject` (spec §6.2): the v1 catalog, radio to video."""
from dataclasses import dataclass

from ofs.v1 import sim_pb2 as pb


@dataclass(frozen=True)
class RadioLinkLoss:
    """Every radio uplink packet is lost while active: the receiver goes silent and Betaflight fails safe."""

    def _to_pb(self):
        return pb.Fault(radio_link_loss=pb.RadioLinkLoss())


@dataclass(frozen=True)
class RadioLoss:
    """Extra per-packet uplink loss while active: `fraction` of packets are dropped on top of the link model's own."""

    fraction: float

    def _to_pb(self):
        return pb.Fault(radio_loss=pb.RadioLoss(fraction=self.fraction))


@dataclass(frozen=True)
class BurstLoss:
    """Every uplink packet is lost for `on_ms`, then the link delivers normally for `off_ms`, repeating from t=0."""

    on_ms: int
    off_ms: int

    def _to_pb(self):
        return pb.Fault(burst_loss=pb.BurstLoss(on_ms=self.on_ms, off_ms=self.off_ms))


@dataclass(frozen=True)
class MotorFailure:
    """Motor `motor` (0-based) produces `scale` of its torque and current; 0 = dead motor."""

    motor: int
    scale: float = 0.0

    def _to_pb(self):
        return pb.Fault(motor_failure=pb.MotorFailure(motor=self.motor, scale=self.scale))


@dataclass(frozen=True)
class PropDamage:
    """Propeller `prop` (0-based) produces `thrust_scale` of its thrust and torque, with `imbalance_kgm` of vibration."""

    prop: int
    thrust_scale: float = 1.0
    imbalance_kgm: float = 0.0

    def _to_pb(self):
        return pb.Fault(prop_damage=pb.PropDamage(prop=self.prop, thrust_scale=self.thrust_scale,
                                                  imbalance_kgm=self.imbalance_kgm))


@dataclass(frozen=True)
class WeakCell:
    """Cell `cell` (0-based) carries `r_offset_ohm` of extra series resistance and `v_offset_v` of extra droop."""

    cell: int
    r_offset_ohm: float = 0.01
    v_offset_v: float = 0.05

    def _to_pb(self):
        return pb.Fault(weak_cell=pb.WeakCell(cell=self.cell, r_offset_ohm=self.r_offset_ohm,
                                              v_offset_v=self.v_offset_v))


@dataclass(frozen=True)
class GyroFault:
    """The IMU's gyro reads `bias_radps` of extra bias and `noise_scale` times its normal noise."""

    bias_radps: tuple = (0.0, 0.0, 0.0)
    noise_scale: float = 1.0

    def _to_pb(self):
        x, y, z = self.bias_radps
        return pb.Fault(gyro_fault=pb.GyroFault(bias_radps=pb.Vec3(x=x, y=y, z=z), noise_scale=self.noise_scale))


@dataclass(frozen=True)
class VtxInterference:
    """A transmitter at `position_ned_m` on `freq_mhz` at `power_mw` interferes with the video link while active."""

    freq_mhz: float
    power_mw: float
    position_ned_m: tuple

    def _to_pb(self):
        x, y, z = self.position_ned_m
        return pb.Fault(vtx_interference=pb.VtxInterference(freq_mhz=self.freq_mhz, power_mw=self.power_mw,
                                                            position_ned_m=pb.Vec3(x=x, y=y, z=z)))
