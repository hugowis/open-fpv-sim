import os
import pathlib
import sys

import pytest

REPO = pathlib.Path(__file__).resolve().parents[2]
QUAD = str(REPO / "quads" / "opendrone-5f-freestyle.toml")
WORLD = str(REPO / "worlds" / "flat.toml")

# Schema-5 quads reference library/ parts. Tests that copy a quad into a bare temp dir would not resolve
# them; default to the repo's library (an explicit OFS_LIBRARY wins).
os.environ.setdefault("OFS_LIBRARY", str(REPO / "library"))


@pytest.fixture(scope="session")
def sim_bin():
    env = os.environ.get("OFS_SIM_BIN")
    if env:
        # Absolute: a test that changes directory (the sandbox example's) must still find a relative path (CI's).
        return str(pathlib.Path(env).resolve())
    exe = REPO / "target" / "debug" / ("ofs-sim.exe" if sys.platform == "win32" else "ofs-sim")
    if not exe.exists():
        pytest.fail("ofs-sim not built: run `cargo build -p ofs-sim` or set OFS_SIM_BIN")
    return str(exe)


@pytest.fixture
def sim(sim_bin, tmp_path):
    import ofs

    s = ofs.launch(binary=sim_bin, data_dir=str(tmp_path))
    yield s
    s.close()
