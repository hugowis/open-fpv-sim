import subprocess

import pytest

import ofs
from conftest import QUAD


def test_open_loop_rest_then_climb(sim):
    assert sim.load(QUAD, seed=1, open_loop_fc=True).startswith("OpenDrone")
    s = sim.run(1.0)
    assert abs(s.time_s - 1.0) < 1e-9
    assert abs(s.altitude_m - 0.0295) < 1e-3
    sim.set_sticks(throttle=1.0)
    s = sim.run(1.0)
    assert s.altitude_m > 1.0
    assert s.battery_voltage_v < 24.5
    assert len(s.motor_rpm) == 4


def test_errors_are_typed(sim):
    with pytest.raises(ofs.NotLoaded):
        sim.run(0.1)
    with pytest.raises(ofs.ConfigError):
        sim.load("does/not/exist.toml")
    sim.load(QUAD, open_loop_fc=True)
    with pytest.raises(ofs.InvalidArgument):
        sim.run(-1.0)
    with pytest.raises(ofs.InvalidArgument):
        sim.run(float("nan"))
    sim.run(0.1)


def test_protocol_mismatch_is_typed(sim, monkeypatch):
    import ofs.client

    monkeypatch.setattr(ofs.client, "PROTOCOL_VERSION", 999)
    with pytest.raises(ofs.ProtocolMismatch):
        ofs.connect(sim.address)


def test_euler_of_identity_is_zero():
    s = ofs.State(time_s=0.0, position_ned_m=(0.0, 0.0, 0.0), velocity_ned_mps=(0.0, 0.0, 0.0),
                  attitude_wxyz=(1.0, 0.0, 0.0, 0.0), rate_frd_radps=(0.0, 0.0, 0.0),
                  battery_voltage_v=0.0, battery_current_a=0.0, motor_rpm=(), motor_cmd=())
    assert s.euler_deg() == (0.0, 0.0, 0.0)


def test_close_unloads_before_stopping_a_launched_server(sim_bin, tmp_path):
    # Terminating ofs-sim skips its cleanup (TerminateProcess on Windows, unhandled SIGTERM on Linux),
    # which would orphan Betaflight SITL: close() must unload while the server is still running.
    s = ofs.launch(binary=sim_bin, data_dir=str(tmp_path))
    proc, real_unload, calls = s._process, s._stub.Unload, []

    def unload(request, timeout=None):
        calls.append((timeout, proc.poll()))
        return real_unload(request, timeout=timeout)

    s._stub.Unload = unload
    s.load(QUAD, open_loop_fc=True)
    s.close()
    assert len(calls) == 1, calls
    timeout, returncode_at_unload = calls[0]
    assert returncode_at_unload is None, "server was already stopped when Unload was sent"
    assert timeout is not None and timeout <= 10.0, "Unload needs a short deadline"
    assert proc.poll() is not None


def test_close_tolerates_a_dead_server(sim_bin, tmp_path):
    s = ofs.launch(binary=sim_bin, data_dir=str(tmp_path))
    s._process.kill()
    s._process.wait()
    s.close()  # Unload fails (server gone); close() must still succeed


def test_launch_stops_the_server_when_the_handshake_fails(sim_bin, tmp_path, monkeypatch):
    import ofs.client

    procs, real_popen = [], subprocess.Popen

    def popen(*args, **kwargs):
        procs.append(real_popen(*args, **kwargs))
        return procs[-1]

    monkeypatch.setattr(ofs.client.subprocess, "Popen", popen)
    monkeypatch.setattr(ofs.client, "PROTOCOL_VERSION", 999)
    with pytest.raises(ofs.ProtocolMismatch):
        ofs.launch(binary=sim_bin, data_dir=str(tmp_path))
    assert len(procs) == 1
    alive = procs[0].poll() is None
    if alive:  # don't leak it from the test either
        procs[0].kill()
        procs[0].wait()
    assert not alive, "ofs-sim was left running after the failed handshake"
