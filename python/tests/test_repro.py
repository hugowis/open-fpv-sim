"""Repro bundles from a script (parent spec §7): a failed session leaves a folder, and the folder replays."""
import pytest

import ofs
from conftest import QUAD


def _fail(sim):
    """Open loop, a finite fault that overflows the vibration model once the rotors spin: a numerical error."""
    sim.load(QUAD, seed=9, open_loop_fc=True, record=False)
    sim.set_sticks(throttle=0.6)
    sim.run(0.1)
    sim.set_sticks(throttle=0.9, roll=0.1)
    sim.inject(ofs.faults.MotorFailure(motor=1, scale=0.5, at=0.2))
    sim.inject(ofs.faults.PropDamage(prop=0, thrust_scale=1.0, imbalance_kgm=1e305, at=0.5))
    with pytest.raises(ofs.NumericalError):
        sim.run(1.0)


def _sim_error_text(sim):
    import time
    deadline = time.monotonic() + 5.0
    while time.monotonic() < deadline:
        for e in sim.events():
            if e.kind == "sim_error":
                return e.message
        time.sleep(0.01)
    raise AssertionError("no sim_error event arrived")


def test_a_failed_session_leaves_a_bundle_a_script_replays(sim, sim_bin):
    _fail(sim)
    message = _sim_error_text(sim)
    bundles = ofs.repro.find_bundles(sim.logs_dir)
    assert len(bundles) == 1, bundles
    assert str(bundles[0]) in message, (bundles, message)
    result = ofs.repro.replay_bundle(bundles[0], binary=sim_bin)
    assert result.reproduced and result.exact, result
    assert result.reached_tick == result.expected_tick == 4000
    assert result.digest == result.expected_digest
    assert result.failure == result.expected_failure
    assert result.failure.startswith("non-finite value in signal")


def test_a_changed_journal_replays_as_not_reproduced(sim, sim_bin):
    _fail(sim)
    bundle = ofs.repro.find_bundles(sim.logs_dir)[0]
    journal = bundle / "journal.txt"
    lines = journal.read_text().splitlines()
    first_sticks = next(i for i, l in enumerate(lines) if " sticks " in l)
    lines[first_sticks] = lines[first_sticks].replace(" 0.6 ", " 0.7 ")
    journal.write_text("\n".join(lines) + "\n")
    result = ofs.repro.replay_bundle(bundle, binary=sim_bin)
    assert not result.reproduced
    assert result.digest != result.expected_digest


def test_a_betaflight_bundle_does_not_run_its_own_launch_program(sim, sim_bin, monkeypatch):
    """A bundle is whatever someone attached to an issue: its quad file's `fc.launch` is not run by a replay."""
    _fail(sim)
    bundle = ofs.repro.find_bundles(sim.logs_dir)[0]
    toml_path = bundle / "bundle.toml"
    toml_path.write_text(toml_path.read_text().replace("open_loop_fc = true", "open_loop_fc = false"))
    monkeypatch.delenv("OFS_SITL_LAUNCH", raising=False)
    with pytest.raises(ofs.ConfigError, match="trust-bundle"):
        ofs.repro.replay_bundle(bundle, binary=sim_bin)


def test_replay_of_a_missing_folder_is_a_config_error(tmp_path, sim_bin):
    with pytest.raises(ofs.ConfigError, match="bundle.toml"):
        ofs.repro.replay_bundle(tmp_path / "nowhere.repro", binary=sim_bin)


def test_a_replay_error_with_non_ascii_text_reads_back(tmp_path, sim_bin):
    with pytest.raises(ofs.ConfigError, match="défaut"):
        ofs.repro.replay_bundle(tmp_path / "défaut-1.repro", binary=sim_bin)


def test_find_bundles_skips_a_folder_without_bundle_toml(tmp_path):
    for name in ["a-1.repro", "a-2.repro"]:
        (tmp_path / name).mkdir()
    (tmp_path / "a-1.repro" / "bundle.toml").write_text("format = 1\n")
    assert [p.name for p in ofs.repro.find_bundles(tmp_path)] == ["a-1.repro"]


def test_find_bundles_lists_only_bundle_folders_in_order(tmp_path):
    for name in ["quad-2.repro", "quad-10.repro", "quad-1.repro"]:
        (tmp_path / name).mkdir()
        (tmp_path / name / "bundle.toml").write_text("format = 1\n")
    (tmp_path / "quad-3.mcap").write_text("not a bundle")
    (tmp_path / "quad-4.repro").write_text("a file, not a folder")
    assert [p.name for p in ofs.repro.find_bundles(tmp_path)] == ["quad-1.repro", "quad-2.repro", "quad-10.repro"]
    assert ofs.repro.find_bundles(tmp_path / "missing") == []
