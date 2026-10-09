import os
import pathlib
import sys

import pytest

REPO = pathlib.Path(__file__).resolve().parents[2]
QUAD = str(REPO / "quads" / "opendrone-5f-freestyle.toml")
WORLD = str(REPO / "worlds" / "flat.toml")


@pytest.fixture(scope="session")
def sim_bin():
    env = os.environ.get("OFS_SIM_BIN")
    if env:
        return env
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
