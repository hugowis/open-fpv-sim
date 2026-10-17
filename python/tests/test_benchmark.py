"""scripts/benchmark.py (parent spec §8.6): its table, its floor, and one real open-loop row."""
import importlib.util
import json

import pytest

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


def test_a_row_below_its_baseline_ratio_is_a_regression():
    rows = [benchmark.Row("a", 30.0, 30.0 / 59.0, ""), benchmark.Row("b", 30.0, 30.0 / 6.0, ""),
            benchmark.Row("c", 30.0, 30.0, ""), benchmark.Row("d", 30.0, None, "no SITL")]
    baseline = {"a": 100.0, "b": 10.0, "d": 100.0}
    assert [r.name for r in benchmark.regressions(rows, baseline, 0.6)] == ["a"], "b sits exactly on 0.6, c has no baseline, d was skipped"


def test_the_table_shows_the_baseline_and_the_ratio():
    rows = [benchmark.Row("open loop, recording", 30.0, 0.65, ""), benchmark.Row("slow", 30.0, 3.0, "")]
    baseline = {"open loop, recording": 47.9, "slow": 100.0}
    text = benchmark.table(rows, baseline, 0.6)
    assert "| Baseline | vs baseline |" in text
    assert "| open loop, recording | 30.0 | 0.65 | 46.2x | 47.9x | 96 % |" in text
    slow = next(line for line in text.splitlines() if line.startswith("| slow"))
    assert "10.0x | 100.0x | 10 % REGRESSED |" in slow, slow
    accepted = benchmark.table(rows, baseline, 0.6, accepted="new tracing")
    assert "ACCEPTED: new tracing" in accepted and "REGRESSED" not in accepted
    assert benchmark.table(rows[:1]) == benchmark.table(rows[:1], None), "without a baseline: the old table"
    assert "Baseline" not in benchmark.table(rows[:1])


def test_an_unknown_runner_is_an_error_naming_the_known_ones(tmp_path):
    shipped = REPO / "scripts" / "benchmark-baseline.json"
    min_ratio, rows = benchmark.load_baseline(shipped, "ci-linux")
    assert min_ratio == 0.6
    assert set(rows) == {name for name, *_ in benchmark.CONFIGS}
    with pytest.raises(ValueError, match="ci-linux"):
        benchmark.load_baseline(shipped, "laptop")


def test_the_best_of_n_keeps_the_fastest_run(monkeypatch):
    def scripted(*walls):
        it = iter(walls)
        monkeypatch.setattr(benchmark, "measure", lambda sim, seconds, load, sticks: next(it))

    scripted(5.0, 3.0, 4.0)
    assert benchmark.measure_best(None, 30.0, {}, {}, 3) == 3.0
    scripted(7.0, 1.0)
    assert benchmark.measure_best(None, 30.0, {}, {}, 1) == 7.0, "repeat=1 measures once"


def test_the_cli_gates_on_a_baseline_and_the_override_passes_it(sim_bin, tmp_path, monkeypatch, capsys):
    monkeypatch.delenv("OFS_SITL_LAUNCH", raising=False)
    monkeypatch.delenv("OFS_BENCH_ACCEPT_SLOWER", raising=False)
    names = ["open loop, recording", "open loop, no recording"]
    path = tmp_path / "baseline.json"
    path.write_text(json.dumps({"min_ratio": 0.6, "runners": {"t": {"rows": {n: 1e9 for n in names}}}}))
    args = ["--binary", sim_bin, "--seconds", "0.5", "--baseline", str(path), "--runner", "t"]
    assert benchmark.main(args) == 1
    err = capsys.readouterr().err
    assert all(n in err for n in names), err
    monkeypatch.setenv("OFS_BENCH_ACCEPT_SLOWER", "testing")
    assert benchmark.main(args) == 0
    assert "ACCEPTED: testing" in capsys.readouterr().out
    monkeypatch.delenv("OFS_BENCH_ACCEPT_SLOWER")
    assert benchmark.main(["--binary", sim_bin, "--seconds", "0.5", "--print-baseline"]) == 0
    printed = json.loads(capsys.readouterr().out)
    assert set(printed["rows"]) == set(names) and all(v > 0 for v in printed["rows"].values())


def test_one_open_loop_row_runs_against_a_real_server(sim):
    wall = benchmark.measure(sim, 0.5, {"open_loop_fc": True, "record": False}, {"throttle": 0.5})
    assert 0.0 < wall < 60.0
