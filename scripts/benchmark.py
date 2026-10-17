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
import json
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


def table(rows: list[Row], baseline: dict | None = None, min_ratio: float = 0.0, accepted: str = "") -> str:
    """The results as a Markdown table. With a `baseline` (row name -> real-time factor) it adds the baseline and the
    ratio to it; a row under `min_ratio` of its baseline says REGRESSED, or ACCEPTED: <reason> when `accepted` is set."""
    if baseline is None:
        lines = ["| Configuration | Simulated (s) | Wall (s) | Real-time factor |", "|---|---|---|---|"]
    else:
        lines = ["| Configuration | Simulated (s) | Wall (s) | Real-time factor | Baseline | vs baseline |", "|---|---|---|---|---|---|"]
    for r in rows:
        if r.wall_s is None:
            lines.append(f"| {r.name} | {r.simulated_s:.1f} | - | skipped: {r.note} |" + ("" if baseline is None else " - | - |"))
            continue
        line = f"| {r.name} | {r.simulated_s:.1f} | {r.wall_s:.2f} | {r.rtf:.1f}x |"
        if baseline is not None:
            base = baseline.get(r.name)
            if base is None:
                line += " - | no baseline |"
            else:
                verdict = ""
                if r.rtf < base * min_ratio:
                    verdict = f" ACCEPTED: {accepted}" if accepted else " REGRESSED"
                line += f" {base:.1f}x | {100.0 * r.rtf / base:.0f} %{verdict} |"
        lines.append(line)
    return "\n".join(lines)


def below_floor(rows: list[Row], floor: float) -> list[Row]:
    return [r for r in rows if r.rtf is not None and r.rtf < floor]


def regressions(rows: list[Row], baseline: dict, min_ratio: float) -> list[Row]:
    """Measured rows whose real-time factor is under `min_ratio` of their baseline. Skipped rows and rows without a
    baseline entry are never regressions."""
    return [r for r in rows if r.rtf is not None and r.name in baseline and r.rtf < baseline[r.name] * min_ratio]


def load_baseline(path, runner: str) -> tuple:
    """(min_ratio, rows) of one runner class from the baseline file; a ValueError names the known runners."""
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    runners = data.get("runners", {})
    if runner not in runners:
        raise ValueError(f"{path}: no baseline for runner '{runner}' (known: {', '.join(sorted(runners)) or 'none'})")
    return float(data["min_ratio"]), {name: float(v) for name, v in runners[runner]["rows"].items()}


def measure(sim, seconds: float, load: dict, sticks: dict) -> float:
    """Loads, warms up 1 simulated second, and returns the wall seconds `run(seconds)` took."""
    sim.load(QUAD, seed=1, mode="lockstep", **load)
    if sticks:
        sim.set_sticks(**sticks)
    sim.run(1.0)
    start = time.perf_counter()
    sim.run(seconds)
    return time.perf_counter() - start


def measure_best(sim, seconds: float, load: dict, sticks: dict, repeat: int = 1) -> float:
    """The fastest of `repeat` measurements: noise on a shared machine only ever makes a run slower."""
    return min(measure(sim, seconds, load, sticks) for _ in range(max(1, repeat)))


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", help="the ofs-sim to measure (default: OFS_SIM_BIN or ofs-sim on PATH)")
    parser.add_argument("--seconds", type=float, default=30.0)
    parser.add_argument("--summary", action="store_true", help="append the table to $GITHUB_STEP_SUMMARY")
    parser.add_argument("--floor", type=float, help="exit 1 if a measured row is below this real-time factor")
    parser.add_argument("--repeat", type=int, default=1, help="measure each row this many times and keep the fastest")
    parser.add_argument("--baseline", help="a baseline file (scripts/benchmark-baseline.json); needs --runner")
    parser.add_argument("--runner", help="the runner class to take the baseline of (e.g. ci-linux)")
    parser.add_argument("--min-ratio", type=float, help="fail a row under this fraction of its baseline (default: the file's)")
    parser.add_argument("--print-baseline", action="store_true", help="print the measured rows as baseline JSON and exit")
    args = parser.parse_args(argv)

    baseline, min_ratio = None, 0.0
    if args.baseline:
        if not args.runner:
            parser.error("--baseline needs --runner")
        file_ratio, baseline = load_baseline(args.baseline, args.runner)
        min_ratio = args.min_ratio if args.min_ratio is not None else file_ratio

    import ofs

    rows = []
    with tempfile.TemporaryDirectory() as data_dir:
        sim = ofs.launch(binary=args.binary, data_dir=data_dir)
        try:
            for name, load, sticks, needs_sitl in CONFIGS:
                if needs_sitl and not os.environ.get("OFS_SITL_LAUNCH"):
                    rows.append(Row(name, args.seconds, None, "OFS_SITL_LAUNCH not set"))
                    continue
                rows.append(Row(name, args.seconds, measure_best(sim, args.seconds, load, sticks, args.repeat), ""))
        finally:
            sim.close()
    if args.print_baseline:
        print(json.dumps({"rows": {r.name: round(r.rtf, 1) for r in rows if r.rtf is not None}}, indent=2))
        return 0
    accepted = os.environ.get("OFS_BENCH_ACCEPT_SLOWER", "").strip()
    text = table(rows, baseline, min_ratio, accepted)
    print(text)
    if args.summary and os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as f:
            f.write("### Real-time factor (parent spec §8.6)\n\n" + text + "\n")
    failed = False
    if baseline is not None:
        for r in regressions(rows, baseline, min_ratio):
            what = f"{r.name} at {r.rtf:.1f}x is under {min_ratio:.0%} of its baseline {baseline[r.name]:.1f}x"
            if accepted:
                print(f"ACCEPTED: {accepted}: {what}")
            else:
                print(f"slower than the baseline: {what}", file=sys.stderr)
                failed = True
        for r in rows:
            if r.rtf is not None and r.name not in baseline:
                print(f"no baseline for '{r.name}' on runner '{args.runner}': not gated", file=sys.stderr)
    if args.floor is not None:
        slow = below_floor(rows, args.floor)
        for r in slow:
            print(f"below the floor of {args.floor}x: {r.name} at {r.rtf:.2f}x", file=sys.stderr)
        failed = failed or bool(slow)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
