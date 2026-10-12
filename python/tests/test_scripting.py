"""Scripted inputs, scheduled faults and world names (spec §5, §6.2), open loop: no Betaflight needed."""
import pytest

import ofs
from conftest import QUAD


def _events(sim, kinds):
    return [(e.kind, e.message) for e in sim.events() if e.kind in kinds]


def test_an_inputs_file_plays_from_now_and_refuses_set_sticks(sim, tmp_path):
    sim.load(QUAD, open_loop_fc=True)
    sim.run(0.2)
    path = tmp_path / "climb.csv"
    path.write_text("# a throttle step\nt_s,throttle\n0,0.3\n0.5,1\n")
    sim.play_inputs(path)
    with pytest.raises(ofs.InvalidArgument, match="climb.csv is playing"):
        sim.set_sticks(throttle=0.0)
    assert sim.run(0.49).motor_cmd == (0.3,) * 4
    assert sim.run(0.02).motor_cmd == (1.0,) * 4, "the 0.5 s row applied at 0.7 s"
    sim.set_sticks(throttle=0.0)  # the file finished
    sim.run(0.05)
    assert _events(sim, {"inputs_started", "inputs_ended"}) == [
        ("inputs_started", "climb.csv: 2 rows over 0.5 s"), ("inputs_ended", "climb.csv: finished")]


def test_a_malformed_inputs_file_names_every_problem_with_its_line(sim, tmp_path):
    sim.load(QUAD, open_loop_fc=True)
    path = tmp_path / "bad.csv"
    path.write_text("t_s,throttle,wobble\n0,0.5,1\n0.5,150,0\n")
    with pytest.raises(ofs.ConfigError) as e:
        sim.play_inputs(path)
    assert str(e.value).splitlines() == [
        "bad.csv line 1: unknown column 'wobble' (expected t_s, roll, pitch, yaw, throttle, aux1, aux2, aux3, aux4)",
        "bad.csv line 3: throttle 150 is outside [0, 1]",
    ]


def test_stop_inputs_keeps_the_last_sticks(sim, tmp_path):
    sim.load(QUAD, open_loop_fc=True)
    path = tmp_path / "long.csv"
    path.write_text("t_s,throttle\n0,0.4\n30,0\n")
    sim.play_inputs(path)
    sim.run(0.1)
    sim.stop_inputs()
    sim.stop_inputs()  # nothing playing: fine
    assert sim.run(0.1).motor_cmd == (0.4,) * 4
    sim.set_sticks(throttle=0.0)


def test_a_long_inputs_file_plays(sim, tmp_path):
    """Beyond gRPC's default 4 MB message: the server accepts requests up to 64 MiB."""
    sim.load(QUAD, open_loop_fc=True)
    path = tmp_path / "long.csv"
    path.write_text("t_s,throttle\n" + "".join(f"{i * 0.001:.3f},0.5\n" for i in range(400_000)))
    assert path.stat().st_size > 4 * 1024 * 1024
    sim.play_inputs(path)
    assert sim.run(0.1).motor_cmd == (0.5,) * 4
    sim.stop_inputs()


def test_a_fault_waits_for_its_time(sim):
    sim.load(QUAD, open_loop_fc=True)
    sim.set_sticks(throttle=1.0)
    sim.inject(ofs.faults.MotorFailure(motor=2, at=0.5))
    s = sim.run(0.5)
    assert _events(sim, {"fault_injected"}) == [("fault_injected", "motor_failure")], "active from 0.5 s"
    before = s.motor_rpm
    s = sim.run(0.5)
    assert s.motor_rpm[2] < before[2] - 1000, "motor 2 died at 0.5 s and coasts down"
    with pytest.raises(ofs.InvalidArgument, match=r"fault time 0.2 s has passed \(the session is at 1 s\)"):
        sim.inject(ofs.faults.RadioLinkLoss(at=0.2))


def test_clear_faults_drops_a_fault_still_waiting(sim):
    sim.load(QUAD, open_loop_fc=True)
    sim.inject(ofs.faults.RadioLinkLoss(at=0.3))
    sim.clear_faults()
    assert sim.run(1.0).radio.link_up


def test_at_is_keyword_only_and_optional():
    assert not ofs.faults.MotorFailure(2)._to_pb().HasField("at_s")
    assert ofs.faults.MotorFailure(2, 0.5, at=3.0)._to_pb().at_s == 3.0
    with pytest.raises(TypeError):
        ofs.faults.RadioLoss(0.1, 2.0)  # at is not positional


def test_a_world_can_be_named(sim):
    sim.load(QUAD, open_loop_fc=True, world="flat")
    assert sim.get_world().name == "Flat field"
    with pytest.raises(ofs.ConfigError, match="nowhere.toml"):
        sim.load(QUAD, open_loop_fc=True, world="nowhere")


def test_an_open_loop_session_leaves_no_blackbox_logs(sim):
    sim.load(QUAD, open_loop_fc=True)
    sim.run(0.1)
    assert sim.blackbox_files() == []
    assert sim.logs_dir.endswith("logs")
