"""The downlink-loss fault (protocol 13): a handset that hears no telemetry while the uplink still flies."""
import time

import pytest

import ofs
from conftest import QUAD


def _events(sim, kinds, count=1, timeout_s=5.0):
    """The (kind, message) events of these kinds, waiting up to `timeout_s` for `count` of them."""
    found = []
    deadline = time.monotonic() + timeout_s
    while True:
        found += [(e.kind, e.message) for e in sim.events() if e.kind in kinds]
        if len(found) >= count or time.monotonic() >= deadline:
            return found
        time.sleep(0.01)


def test_downlink_loss_maps_to_field_ten_and_at_s():
    fault = ofs.faults.DownlinkLoss(0.25)
    pb = fault._to_pb()
    assert pb.WhichOneof("kind") == "downlink_loss"
    assert pb.downlink_loss.fraction == 0.25
    assert not pb.HasField("at_s")
    assert pb.DESCRIPTOR.fields_by_name["downlink_loss"].number == 10, "at_s already uses field 9"
    assert pb.DESCRIPTOR.fields_by_name["at_s"].number == 9

    scheduled = ofs.faults.DownlinkLoss(0.5, at=1.25)._to_pb()
    assert scheduled.WhichOneof("kind") == "downlink_loss"
    assert scheduled.at_s == 1.25


def test_downlink_loss_drops_handset_telemetry_but_flies_the_same_quad(sim):
    sim.load(QUAD, seed=3, open_loop_fc=True)
    sim.set_sticks(throttle=0.5)
    clean = sim.run(0.5)
    assert clean.radio.link_up and clean.radio.downlink_lq_pct == 100.0

    sim.load(QUAD, seed=3, open_loop_fc=True)
    sim.set_sticks(throttle=0.5)
    sim.inject(ofs.faults.DownlinkLoss(1.0))
    lossy = sim.run(0.5)

    assert lossy.radio.downlink_lq_pct == 0.0, "the handset hears no telemetry"
    assert lossy.radio.lq_pct == clean.radio.lq_pct == 100.0, "the uplink is untouched"
    assert lossy.radio.link_up and lossy.radio.tx_enabled, "the control link never failsafes"
    assert lossy.motor_cmd == clean.motor_cmd, "the sticks still fly the quad"
    assert lossy.position_ned_m == clean.position_ned_m


def test_the_active_fault_list_names_downlink_loss(sim):
    sim.load(QUAD, open_loop_fc=True)
    sim.inject(ofs.faults.DownlinkLoss(1.0))
    sim.run(0.05)
    assert _events(sim, {"fault_injected"}) == [("fault_injected", "downlink_loss")]

    sim.clear_faults()
    sim.run(0.05)
    assert _events(sim, {"fault_cleared"}) == [("fault_cleared", "downlink_loss")]


def test_an_out_of_range_downlink_loss_fraction_is_invalid_argument(sim):
    sim.load(QUAD, open_loop_fc=True)
    with pytest.raises(ofs.InvalidArgument, match="downlink loss fraction"):
        sim.inject(ofs.faults.DownlinkLoss(1.5))
    with pytest.raises(ofs.InvalidArgument, match="downlink loss fraction"):
        sim.inject(ofs.faults.DownlinkLoss(-0.1))
    sim.run(0.05)
    assert _events(sim, {"fault_injected"}, count=0) == []
