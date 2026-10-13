"""scripts/benchmark.py (parent spec §8.6): its table, its floor, and one real open-loop row."""
import importlib.util

from conftest import REPO

spec = importlib.util.spec_from_file_location("benchmark", REPO / "scripts" / "benchmark.py")
benchmark = importlib.util.module_from_spec(spec)
spec.loader.exec_module(benchmark)


def test_the_table_reports_the_real_time_factor_and_skips():
    rows = [benchmark.Row("open loop, recording", 30.0, 3.0, ""),
            benchmark.Row("Betaflight SITL, recording", 30.0, None, "OFS_SITL_LAUNCH not set")]
    text = benchmark.table(rows)
    assert "| open loop, recording | 30.0 | 3.00 | 10.0x |" in text
    assert "| Betaflight SITL, recording | 30.0 | - | skipped: OFS_SITL_LAUNCH not set |" in text


def test_only_measured_rows_below_the_floor_fail_it():
    rows = [benchmark.Row("fast", 30.0, 3.0, ""), benchmark.Row("slow", 30.0, 60.0, ""),
            benchmark.Row("skipped", 30.0, None, "no SITL")]
    assert [r.name for r in benchmark.below_floor(rows, 1.0)] == ["slow"]


def test_one_open_loop_row_runs_against_a_real_server(sim):
    wall = benchmark.measure(sim, 0.5, {"open_loop_fc": True, "record": False}, {"throttle": 0.5})
    assert 0.0 < wall < 60.0
