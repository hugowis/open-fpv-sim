"""Faults to inject with `Sim.inject` (spec §6.2). M2 ships the radio link loss; the v1 catalog completes in M4."""
from dataclasses import dataclass

from ofs.v1 import sim_pb2 as pb


@dataclass(frozen=True)
class RadioLinkLoss:
    """Every radio uplink packet is lost while active: the receiver goes silent and Betaflight fails safe."""

    def _to_pb(self):
        return pb.Fault(radio_link_loss=pb.RadioLinkLoss())
