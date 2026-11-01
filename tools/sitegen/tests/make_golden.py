"""Builds the golden files the Rust engine tests load: fixture.glb (only IEEE-basic float ops, no sin/cos, so the bytes
match on every platform) and the demo site's demo.glb and demo.toml (built with trigonometry, checked on Linux and Windows).

Usage: python tools/sitegen/tests/make_golden.py   (rewrites golden/*)
"""
import shutil
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # also runs as a plain script

from sitegen.build import build_site
from sitegen.glb import write_glb
from sitegen.mesh import MeshBuilder

GOLDEN_DIR = Path(__file__).resolve().parent / "golden"
GOLDEN = GOLDEN_DIR / "fixture.glb"
DEMO = Path(__file__).resolve().parent / "fixtures" / "demo" / "recipe.toml"


def build() -> bytes:
    wall = MeshBuilder()
    wall.add_tri((100.0, 0.0, -5.0), (0.0, 0.0, 0.0), (0.0, 50.0, 0.0), "concrete")
    ground = MeshBuilder()
    ground.add_quad((0.0, 0.0, 0.0), (0.0, 20.0, 0.0), (10.0, 20.0, 0.0), (10.0, 0.0, 0.0), "grass")
    return write_glb({"wall": wall, "ground": ground}, {"grass": (0.2, 0.5, 0.1), "concrete": (0.6, 0.6, 0.6)})


def build_demo(out: Path) -> None:
    """Copies the demo site's world file and glb to out/demo.toml and out/demo.glb (the engine test re-creates assets/)."""
    with tempfile.TemporaryDirectory() as tmp:
        rep = build_site(DEMO, Path(tmp))
        shutil.copyfile(rep.world_path, out / "demo.toml")
        shutil.copyfile(rep.glb_path, out / "demo.glb")


if __name__ == "__main__":
    GOLDEN.write_bytes(build())
    build_demo(GOLDEN_DIR)
