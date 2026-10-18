#!/usr/bin/env python3
"""The recording reader's speed (issue #45): `ofs.reader.read` against the `mcap`-iterator reference it replaced.

    python scripts/bench_reader.py [--binary PATH] [--seconds 60] [--repeat 2]

Records `--seconds` of an open-loop flight with the shipped quad (the default signal table), then reads it three
patterns at a time and in full with both readers, keeping the fastest of `--repeat` runs, checks that they return the
same thing, and prints a table with the speed-up. Run it from the repository root (the quad path is relative to it).
"""
import argparse
import os
import sys
import tempfile
import time

QUAD = "quads/opendrone-5f-freestyle.toml"
THREE = ["imu.gyro_frd_radps", "motor.*.omega_radps", "battery.voltage_v"]


def best_of(function, repeat: int) -> float:
    best = float("inf")
    for _ in range(max(1, repeat)):
        start = time.perf_counter()
        function()
        best = min(best, time.perf_counter() - start)
    return best


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", help="the ofs-sim to record with (default: OFS_SIM_BIN or ofs-sim on PATH)")
    parser.add_argument("--seconds", type=float, default=60.0)
    parser.add_argument("--repeat", type=int, default=2)
    args = parser.parse_args(argv)

    import ofs
    from ofs import reader

    with tempfile.TemporaryDirectory() as data_dir:
        with ofs.launch(binary=args.binary, data_dir=data_dir) as sim:
            sim.load(QUAD, seed=1, open_loop_fc=True)
            sim.set_sticks(throttle=0.5)
            sim.run(args.seconds)
            path = sim.log.path
        megabytes = os.path.getsize(path) / 1e6
        lines = [f"A {args.seconds:.0f} s recording, {megabytes:.1f} MB.", "",
                 "| Read | mcap iterator (s) | message indexes (s) | Speed-up |", "|---|---|---|---|"]
        for label, patterns in (("3 patterns", THREE), ("every signal", None)):
            if reader._read_with_mcap(path, patterns) != reader.read(path, patterns):
                print(f"the readers disagree on {label}", file=sys.stderr)
                return 1
            if not reader._used_indexed_path:
                print("the fast path did not run: is this recording written with message indexes?", file=sys.stderr)
                return 1
            slow = best_of(lambda: reader._read_with_mcap(path, patterns), args.repeat)
            fast = best_of(lambda: reader.read(path, patterns), args.repeat)
            lines.append(f"| {label} | {slow:.2f} | {fast:.2f} | {slow / fast:.1f}x |")
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    sys.exit(main())
