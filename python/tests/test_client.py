import subprocess

import pytest

import ofs
from conftest import QUAD, WORLD


def test_open_loop_rest_then_climb(sim):
    assert sim.load(QUAD, seed=1, open_loop_fc=True).startswith("OpenDrone")
    s = sim.run(1.0)
    assert abs(s.time_s - 1.0) < 1e-9
    # The generated default body sphere (radius 4 cm) carries the rest height (M3c task 6's ruling).
    assert abs(s.altitude_m - 0.04) < 2e-3
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


def test_events_tolerate_an_unknown_kind_from_a_newer_server():
    from ofs.client import _event
    from ofs.v1 import sim_pb2 as pb

    known = _event(pb.Event(time_s=1.0, kind=pb.EVENT_KIND_LINK_DOWN, message="m"))
    assert known.kind == "link_down"
    unknown = _event(pb.Event(time_s=2.0, kind=99, message="new"))
    assert (unknown.kind, unknown.time_s, unknown.message) == ("unknown_99", 2.0, "new")


def test_pilot_busy_and_internal_are_typed():
    import grpc

    from ofs.errors import from_rpc_error

    class FakeRpcError(grpc.RpcError):
        def __init__(self, kind, message):
            super().__init__(message)
            self._md = (("ofs-error-kind", kind),)
            self._message = message

        def trailing_metadata(self):
            return self._md

        def details(self):
            return self._message

        def code(self):
            return grpc.StatusCode.UNKNOWN

    assert isinstance(from_rpc_error(FakeRpcError("pilot_busy", "another pilot is flying")), ofs.PilotBusy)
    assert isinstance(from_rpc_error(FakeRpcError("internal", "boom")), ofs.InternalError)


def test_without_firmware_the_osd_is_absent_but_the_vtx_transmits(sim):
    sim.load(QUAD, open_loop_fc=True)
    osd = sim.get_osd()
    assert (osd.present, osd.cols, osd.rows) == (False, 0, 0)
    assert osd.rows_text() == [] and osd.text == ""
    state = sim.run(0.1)
    assert state.vtx == ofs.Vtx(present=True, band=5, channel=1, freq_mhz=5658, power_mw=200, pit_mode=False)
    assert state.serial_dropped_bytes == 0
    assert state.video.present and state.video.sync == "locked", state.video
    assert list(state.video.rssi) == ["omni"], "the open field's single antenna"


def test_a_session_flies_in_its_world(sim):
    sim.load(QUAD, open_loop_fc=True, world=WORLD)
    world = sim.get_world()
    assert world.name == "Flat field"
    assert world.pilot_position_ned_m == (-3.0, 2.0, -1.7)
    assert [a.name for a in world.antennas] == ["omni", "patch"]
    assert len(world.objects) == 30
    b = next(o for o in world.objects if o.name == "BuildingB")
    assert (b.shape, b.size_m, b.rf_loss_db) == ("box", (10.0, 10.0, 22.0), 30.0)
    assert [(e.name, e.freq_mhz) for e in world.emitters] == [("parked-quad", 5695.0)]
    video = sim.run(0.5).video
    assert video.present and video.sync == "locked" and video.snr_db > 40.0, video
    assert list(video.rssi) == ["omni", "patch"] and video.active_antenna in video.rssi
    assert 0.0 <= video.noise <= 1.0 and video.chroma == 1.0
    sim.load(QUAD, open_loop_fc=True)
    assert sim.get_world().name == "open field"


def test_a_bad_world_path_is_a_config_error(sim):
    with pytest.raises(ofs.ConfigError):
        sim.load(QUAD, open_loop_fc=True, world="no/such/world.toml")
    with pytest.raises(ofs.NotLoaded):
        sim.get_world()


def test_video_messages_convert():
    from ofs.client import _event, _video
    from ofs.v1 import sim_pb2 as pb

    m = pb.VideoLink(present=True, snr_db=2.5, sync=pb.VIDEO_SYNC_LOST, active_antenna="patch",
                     rssi=[pb.AntennaRssi(name="omni", rssi_dbm=-90.0), pb.AntennaRssi(name="patch", rssi_dbm=-85.0)])
    v = _video(m)
    assert (v.sync, v.active_antenna, v.rssi) == ("lost", "patch", {"omni": -90.0, "patch": -85.0})
    assert _video(pb.VideoLink()).sync == "" and not _video(pb.VideoLink()).present
    assert _event(pb.Event(kind=pb.EVENT_KIND_VIDEO_LOST)).kind == "video_lost"
    assert _event(pb.Event(kind=pb.EVENT_KIND_VIDEO_RESTORED)).kind == "video_restored"


def test_the_osd_helpers_read_packed_cells():
    cells = tuple([0x20] * 6)
    cells = cells[:1] + (ord("A") | 1 << 8 | 1 << 10,) + cells[2:]
    osd = ofs.Osd(seq=1, time_s=0.0, present=True, cols=3, rows=2, cells=cells)
    assert osd.char(0, 1) == ord("A")
    assert osd.blink(0, 1) and osd.page(0, 1) == 1
    assert osd.rows_text() == [" A ", "   "]
    assert osd.text == " A \n   "
    assert ofs.Vtx(band=5).band_letter == "R" and ofs.Vtx(band=0).band_letter == ""


def test_states_are_hashable_values():
    from ofs.client import _state
    from ofs.v1 import sim_pb2 as pb

    m = pb.State(video=pb.VideoLink(present=True, rssi=[pb.AntennaRssi(name="omni", rssi_dbm=-60.0)]))
    a, b = _state(m), _state(m)
    assert hash(a) == hash(b) and a == b and len({a, b}) == 1
    assert a.video.rssi["omni"] == -60.0 and a.video.rssi == {"omni": -60.0}
    assert dict(a.video.rssi) == {"omni": -60.0} and list(a.video.rssi) == ["omni"]


def test_a_watch_that_ends_is_reported_and_reopened(sim):
    import time

    sim.load(QUAD, open_loop_fc=True)
    first = sim._watch
    first.cancel()  # as if the server had dropped the event stream
    deadline = time.monotonic() + 3.0
    kinds = []
    while time.monotonic() < deadline and "watch_ended" not in kinds:
        kinds.extend(e.kind for e in sim.events())
        time.sleep(0.05)
    assert "watch_ended" in kinds, kinds
    sim.load(QUAD, open_loop_fc=True)
    assert sim._watch is not None and sim._watch is not first, "the next load reopens the event stream"


def test_equal_rssi_mappings_hash_alike_whatever_their_order():
    from ofs.client import AntennaRssi

    a, b = AntennaRssi([("omni", -60.0), ("patch", -70.0)]), AntennaRssi([("patch", -70.0), ("omni", -60.0)])
    assert a == b and hash(a) == hash(b)
    assert list(a) == ["omni", "patch"], "the order is still the world file's"


def test_the_radio_reports_snr_antenna_and_downlink(sim):
    sim.load(QUAD, seed=1, open_loop_fc=True)
    s = sim.run(0.5)
    assert s.radio.link_up and s.radio.lq_pct == 100.0, s.radio
    assert s.radio.snr_db > 20.0, f"1.7 m from the handset: strong ({s.radio.snr_db})"
    assert s.radio.active_antenna == "antenna", s.radio
    assert s.radio.downlink_lq_pct == 100.0, s.radio


def test_the_state_starts_without_a_collision(sim):
    sim.load(QUAD, seed=1, open_loop_fc=True)
    s = sim.run(0.5)
    assert s.collision_speed_mps == 0.0


def test_the_world_includes_the_handset(sim):
    sim.load(QUAD, seed=1, open_loop_fc=True, world=WORLD)
    w = sim.get_world()
    assert w.handset.position_ned_m == (-3.0, 2.0, -1.2), "0.5 m below the goggles by default"
    assert w.handset.antennas[0].name == "handset"
    assert w.handset.antennas[0].polarization == "linear"


def test_cutting_the_radio_raises_link_down(sim):
    sim.load(QUAD, seed=1, open_loop_fc=True)
    s = sim.run(0.5)
    assert s.radio.link_up, s.radio
    sim.inject(ofs.faults.RadioLinkLoss())
    s = sim.run(0.5)
    assert not s.radio.link_up
    assert "link_down" in [e.kind for e in sim.events()], [e.kind for e in sim.events()]


def test_two_lockstep_runs_give_identical_body_and_radio_signals(sim):
    runs = []
    for _ in range(2):
        sim.load(QUAD, seed=5, open_loop_fc=True)  # loading again replaces the session (there is no unload)
        s = sim.run(0.25)
        runs.append((s.time_s, tuple(s.motor_rpm), s.position_ned_m, s.radio))
    assert runs[0] == runs[1], "lockstep is deterministic, radio included"


def test_a_collision_event_names_the_object(sim, tmp_path):
    import pathlib
    import shutil
    # 0.55 m above the roof: hard enough for the 1 m/s event threshold, while the 0.3 default restitution's
    # rebound stays under it, so the one touch raises exactly one event (a 1.5 m drop bounces and raises two).
    text = pathlib.Path(QUAD).read_text().replace(
        "position_ned_m = [0.0, 0.0, -0.03]", "position_ned_m = [110.0, -45.0, -14.55]")
    quad = tmp_path / "onto-building-a.toml"
    quad.write_text(text)
    # fc.betaflight_diff resolves next to the quad file, so the copy needs the diff beside it.
    shutil.copy(pathlib.Path(QUAD).with_name("opendrone-5f-freestyle.betaflight.diff"), tmp_path)
    sim.load(str(quad), seed=1, open_loop_fc=True, world=WORLD)
    s = sim.run(5.0)
    collisions = [e for e in sim.events() if e.kind == "collision"]
    assert len(collisions) == 1, collisions
    assert "BuildingA" in collisions[0].message and "m/s" in collisions[0].message, collisions[0].message
    assert s.collision_speed_mps > 3.0, "the fall onto the roof was hard"
