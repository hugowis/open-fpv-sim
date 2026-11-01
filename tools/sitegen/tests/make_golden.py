"""Builds the golden .glb the Rust engine test loads. Only IEEE-basic float ops here (no sin/cos), so the bytes match on every CI platform.

Usage: python tools/sitegen/tests/make_golden.py   (rewrites golden/fixture.glb)
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # also runs as a plain script

from sitegen.glb import write_glb
from sitegen.mesh import MeshBuilder

GOLDEN = Path(__file__).resolve().parent / "golden" / "fixture.glb"


def build() -> bytes:
    wall = MeshBuilder()
    wall.add_tri((100.0, 0.0, -5.0), (0.0, 0.0, 0.0), (0.0, 50.0, 0.0), "concrete")
    ground = MeshBuilder()
    ground.add_quad((0.0, 0.0, 0.0), (0.0, 20.0, 0.0), (10.0, 20.0, 0.0), (10.0, 0.0, 0.0), "grass")
    return write_glb({"wall": wall, "ground": ground}, {"grass": (0.2, 0.5, 0.1), "concrete": (0.6, 0.6, 0.6)})


if __name__ == "__main__":
    GOLDEN.write_bytes(build())
