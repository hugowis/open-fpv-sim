#!/usr/bin/env python3
"""The real-time factor of headless runs (parent spec §8.6: the target is >= 2x real time on a mid-range PC).

    python scripts/benchmark.py [--binary PATH] [--seconds 30] [--summary] [--floor X]

For each configuration: load the shipped quad in lockstep, warm up 1 simulated second, then time `run(seconds)` on
the wall clock; the real-time factor is simulated seconds / wall seconds. The Betaflight rows need OFS_SITL_LAUNCH
and are skipped without it. `--summary` appends the table to $GITHUB_STEP_SUMMARY; `--floor X` exits 1 when a
measured row is below X (CI uses 1: shared runners are noisy, so the floor catches regressions, not the target).
Run it from the repository root (the quad path is relative to it).
"""
import argparse
import os
import sys
import tempfile
import time
from dataclasses import dataclass

QUAD = "quads/opendrone-5f-freestyle.toml"
# (name, load() arguments, sticks during the run, needs Betaflight)
CONFIGS = [
    ("open loop, recording", {"open_loop_fc": True, "record": True}, {"throttle": 0.5}, False),
    ("open loop, no recording", {"open_loop_fc": True, "record": False}, {"throttle": 0.5}, False),
    ("Betaflight SITL, recording", {"record": True}, {}, True),
    ("Betaflight SITL, no recording", {"record": False}, {}, True),
]


@dataclass
class Row:
    name: str
    simulated_s: float
    wall_s: float | None  # None: skipped
    note: str

    @property
    def rtf(self) -> float | None:
        return None if self.wall_s is None else self.simulated_s / self.wall_s


def table(rows: list[Row]) -> str:
    lines = ["| Configuration | Simulated (s) | Wall (s) | Real-time factor |", "|---|---|---|---|"]
    for r in rows:
        if r.wall_s is None:
            lines.append(f"| {r.name} | {r.simulated_s:.1f} | - | skipped: {r.note} |")
        else:
            lines.append(f"| {r.name} | {r.simulated_s:.1f} | {r.wall_s:.2f} | {r.rtf:.1f}x |")
    return "\n".join(lines)


def below_floor(rows: list[Row], floor: float) -> list[Row]:
    return [r for r in rows if r.rtf is not None and r.rtf < floor]


def measure(sim, seconds: float, load: dict, sticks: dict) -> float:
    """Loads, warms up 1 simulated second, and returns the wall seconds `run(seconds)` took."""
    sim.load(QUAD, seed=1, mode="lockstep", **load)
    if sticks:
        sim.set_sticks(**sticks)
    sim.run(1.0)
    start = time.perf_counter()
    sim.run(seconds)
    return time.perf_counter() - start


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", help="the ofs-sim to measure (default: OFS_SIM_BIN or ofs-sim on PATH)")
    parser.add_argument("--seconds", type=float, default=30.0)
    parser.add_argument("--summary", action="store_true", help="append the table to $GITHUB_STEP_SUMMARY")
    parser.add_argument("--floor", type=float, help="exit 1 if a measured row is below this real-time factor")
    args = parser.parse_args(argv)

    import ofs

    rows = []
    with tempfile.TemporaryDirectory() as data_dir:
        sim = ofs.launch(binary=args.binary, data_dir=data_dir)
        try:
            for name, load, sticks, needs_sitl in CONFIGS:
                if needs_sitl and not os.environ.get("OFS_SITL_LAUNCH"):
                    rows.append(Row(name, args.seconds, None, "OFS_SITL_LAUNCH not set"))
                    continue
                rows.append(Row(name, args.seconds, measure(sim, args.seconds, load, sticks), ""))
        finally:
            sim.close()
    text = table(rows)
    print(text)
    if args.summary and os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as f:
            f.write("### Real-time factor (parent spec §8.6)\n\n" + text + "\n")
    if args.floor is not None:
        slow = below_floor(rows, args.floor)
        for r in slow:
            print(f"below the floor of {args.floor}x: {r.name} at {r.rtf:.2f}x", file=sys.stderr)
        return 1 if slow else 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
