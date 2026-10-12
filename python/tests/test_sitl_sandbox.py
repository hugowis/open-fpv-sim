"""Spec §6.2's sandbox example through real Betaflight (M4c): python/examples/sandbox.py as the README prints it,
run from a directory laid out like the repository, then its recording and Betaflight's own Blackbox log.
Needs OFS_SITL_LAUNCH."""
import os
import shutil

import pytest

from conftest import REPO

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")


@pytest.fixture
def sandbox(tmp_path, sim_bin, monkeypatch):
    """A working directory with the repository's quads, library and worlds, the example and its flips.csv."""
    for d in ("quads", "library", "worlds"):
        shutil.copytree(REPO / d, tmp_path / d)
    for f in ("sandbox.py", "flips.csv"):
        shutil.copy(REPO / "python" / "examples" / f, tmp_path / f)
    monkeypatch.chdir(tmp_path)
    monkeypatch.setenv("OFS_SIM_BIN", sim_bin)
    monkeypatch.delenv("OFS_LIBRARY", raising=False)  # the copied library/ beside quads/ must resolve on its own
    return tmp_path


def test_the_sandbox_example_runs_as_printed(sandbox):
    pytest.importorskip("mcap")
    pytest.importorskip("pandas")
    script = (sandbox / "sandbox.py").read_text()
    namespace = {}
    exec(compile(script, "sandbox.py", "exec"), namespace)
    sim, df = namespace["sim"], namespace["df"]
    try:
        assert list(df.columns) == ["imu.gyro_frd_radps.x", "imu.gyro_frd_radps.y", "imu.gyro_frd_radps.z",
                                    "motor.0.omega_radps", "motor.1.omega_radps", "motor.2.omega_radps",
                                    "motor.3.omega_radps", "battery.voltage_v"]
        assert df.index[-1] == pytest.approx(10.0)
        motor2 = df["motor.2.omega_radps"].dropna()
        flying = motor2.loc[2.8:2.95].mean()
        assert flying > 500, f"armed at 1 s and climbing: motor 3 at {flying:.0f} rad/s"
        assert motor2.loc[4.0:4.2].mean() < 0.5 * flying, "MotorFailure(motor=2, at=3.0) stopped it"
        assert df["imu.gyro_frd_radps.x"].loc[2.0:2.5].abs().max() > 3.0, "the flip shows on the gyro"
    finally:
        sim.close()
    logs = sim.blackbox_files()
    assert len(logs) == 1, logs
    with open(logs[0], "rb") as f:
        assert f.read(64).startswith(b"H Product:Blackbox flight data recorder"), "Betaflight's own log, readable"
