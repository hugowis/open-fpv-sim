"""The Rerun live view (spec §4): what it logs, against a stand-in recording stream, then against Rerun itself."""
import math

import pytest

import ofs
from conftest import QUAD

rr = pytest.importorskip("rerun")


class FakeStream:
    """Records what the view logs: (entity path, archetype class name, static) per call, and the times set."""

    def __init__(self):
        self.logged = []
        self.times = []

    def set_time(self, timeline, duration):
        self.times.append((timeline, duration))

    def log(self, path, archetype, static=False):
        self.logged.append((path, type(archetype).__name__, static))


def _state(**kw):
    base = dict(time_s=1.5, position_ned_m=(1.0, 2.0, -3.0), velocity_ned_mps=(0.0, 0.0, 0.0),
                attitude_wxyz=(1.0, 0.0, 0.0, 0.0), rate_frd_radps=(0.0, 0.0, 0.0), battery_voltage_v=24.0,
                battery_current_a=10.0, motor_rpm=(1000.0, 1001.0, 1002.0, 1003.0), motor_cmd=(0.5,) * 4)
    base.update(kw)
    return ofs.State(**base)


def test_a_state_logs_the_pose_and_the_curves_at_its_time():
    rec = FakeStream()
    ofs.rerun.log_state(rec, _state())
    assert rec.times == [("sim", 1.5)]
    assert [(p, k) for p, k, _ in rec.logged] == [
        ("world/quad", "Transform3D"), ("battery/voltage_v", "Scalars"), ("battery/current_a", "Scalars"),
        ("motors/rpm", "Scalars"), ("radio/lq_pct", "Scalars"), ("radio/rssi_dbm", "Scalars")]
    rec = FakeStream()
    ofs.rerun.log_state(rec, _state(video=ofs.VideoLink(present=True, snr_db=20.0)))
    assert ("video/snr_db", "Scalars", False) in rec.logged


def test_events_are_text_lines_and_a_timeless_one_keeps_the_clock():
    rec = FakeStream()
    ofs.rerun.log_event(rec, ofs.Event(time_s=2.0, kind="fault_injected", message="motor_failure"))
    ofs.rerun.log_event(rec, ofs.Event(time_s=math.nan, kind="watch_ended", message="gone"))
    assert rec.times == [("sim", 2.0)]
    assert [(p, k) for p, k, _ in rec.logged] == [("events", "TextLog"), ("events", "TextLog")]


def test_the_world_is_logged_once_as_static_entities(sim):
    sim.load(QUAD, open_loop_fc=True, world="flat")
    rec = FakeStream()
    ofs.rerun.log_world(rec, sim.get_world())
    assert all(static for _, _, static in rec.logged)
    assert [p for p, _, _ in rec.logged] == ["world", "world/objects", "world/poles", "world/pilot", "world/quad/frame"]


def test_view_streams_a_real_time_session_into_a_rerun_file(sim, tmp_path):
    sim.load(QUAD, open_loop_fc=True, mode="realtime")
    sim.start()
    rec = rr.RecordingStream("open-fpv-sim-test")
    path = tmp_path / "view.rrd"
    rec.save(str(path))
    returned = ofs.rerun.view(sim, rate_hz=60, spawn=False, recording=rec, until_s=0.3)
    assert returned is rec
    rec.disconnect()
    assert path.stat().st_size > 1000
