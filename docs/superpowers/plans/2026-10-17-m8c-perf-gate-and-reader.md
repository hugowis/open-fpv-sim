# M8c: Performance Gate and a Faster Reader Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** CI catches a real slowdown of the simulator (#61), and the Python recording reader is several times faster with the same public API (#45).

**Architecture:** (#61) `scripts/benchmark.py` learns to compare each measured row with a baseline recorded in the repository (`scripts/benchmark-baseline.json`, per runner class) and to fail below a generous ratio; best-of-N timing keeps shared runners from flaking; a visible override accepts a slowdown on purpose. (#45) `ofs.reader.read` decodes the MCAP chunks itself, using each chunk's per-channel message index to touch only the selected signals' records, and keeps the current `mcap`-library path as the fallback and as the reference the tests compare against.

**Tech Stack:** Python (`ofs.reader`, `scripts/`), `mcap` and `zstandard` (already installed with the `log` extra), GitHub Actions.

**Spec:** none beyond GitHub issues #61 and #45 (`gh issue view <n> --repo hugowis/open-fpv-sim`) and the "Design calls" below, which are binding.

## Global Constraints

- No Rust changes, no protocol change, no golden digest change. `cargo test --workspace` is not part of this milestone's gate beyond one confirming run at the end.
- **Line endings:** never rewrite a file through Python text mode on Windows (CRLF); use Edit/Write. Check new files with `git ls-files --eol` (all LF).
- Python tests: `PYTHONPATH=<worktree>/python python -m pytest python/tests -q` with a built `target/debug/ofs-sim` (`cargo build -p ofs-sim --locked`).
- Worktree-isolated sessions refuse heredocs and compound git commands: use Edit/Write; WSL through PowerShell with a script file in the scratchpad (`wsl -d Ubuntu -- bash <path>`), `$HOME/.cargo/bin` on PATH, Python from `~/ofs/venv/bin/python`.
- Commit trailers: a blank line, then `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` on its own line.

## Design calls (decided here; the executor ledgers any change)

1. **#61, what is compared.** Each measured benchmark row's real-time factor against a baseline recorded for the runner class it runs on. The baseline file `scripts/benchmark-baseline.json` is `{"min_ratio": 0.6, "runners": {"ci-linux": {"note": "...", "rows": {"<row name>": <real-time factor>}}}}`. The `ci-linux` rows are the median of the last five green CI runs on `main` (GitHub's `ubuntu-24.04`, release build, 30 simulated seconds; run ids 39017824619, 39020889413, 39026139379, 39030241182, 39041634879): open loop recording 47.9, open loop no recording 131.9, Betaflight SITL recording 10.0, Betaflight SITL no recording 11.8 (spread between runs: under 4 %). A row fails the gate when `rtf < baseline * min_ratio`. The existing `--floor` stays.
2. **#61, no flakes.** `--repeat N` runs each row N times and keeps the fastest wall time (noise only slows a run down); CI uses `--repeat 2`. Rows with no measurement (SITL not available) and rows with no baseline entry are never gated (a missing baseline row is reported, not failed). The gate applies only with `--baseline FILE --runner NAME`; a runner name the file lacks is an error naming the known runners. A developer machine is not a runner class: do not gate local runs.
3. **#61, the visible override.** A deliberate slowdown is accepted by one of two visible acts: updating the baseline in the PR (`python scripts/benchmark.py --print-baseline` prints the JSON `rows` object for a runner from a fresh run, or copy it from the job summary), or setting the repository variable `OFS_BENCH_ACCEPT_SLOWER` to a reason, which the CI step passes as the environment variable of that name: regressions are then listed as "ACCEPTED: <reason>" in the table and the log and do not fail the job. The summary always shows the baseline and the ratio per row.
4. **#45, which option.** Profiling a one-minute open-loop recording (27.6 MB, 1.04 million records, 148 signals) showed 3 patterns taking 2.8 s and all signals 9.7 s, and the time is in `mcap`'s pure-Python record parsing and heap merge of every message (`breakup_chunk`, `iter_messages`), not in decoding JSON: so a NumPy columnar decode of the JSON would not help, and a Rust reader behind a binding would turn the pure-Python wheel into per-platform wheels. A prototype that keeps `mcap` for the summary and uses each chunk's per-channel message index to slice only the wanted records out of the zstd-decompressed chunk read the same file in 0.32 s (3 patterns, 8.8x) and 2.5 s (all, 4.1x) with output identical to the current reader. That is the design.
5. **#45, the fast path and its fallbacks.** `ofs.reader.read(path, patterns)` keeps its signature, its return value and its error messages. The fast path is used when the summary has chunk indexes whose channels carry message indexes (`message_index_offsets` non-empty) and every chunk's compression is `zstd` or empty and `zstandard` imports. Anything else (a file written without message indexes, lz4 chunks, no `zstandard`) uses the previous `mcap`-iterator implementation, renamed `_read_with_mcap`, which is also the reference the parity tests compare against. Any exception inside the fast path becomes the same `damaged` `ValueError` as before (never a wrong answer, never a raw struct/zstd error).
6. **#45, the proof.** `scripts/bench_reader.py` records one simulated minute (needs `ofs-sim`), times the reference and the fast reader on 3 patterns and on every signal, asserts the outputs are equal and prints the table with the speed-up; the numbers go in `docs/dev-setup.md` and the roadmap. A parity test on a short recording and on synthetic files (written with the `mcap` writer: zstd, lz4 and uncompressed chunks, with and without message indexes, several chunks) asserts fast and reference agree.

## Review Focus

- A row that was skipped, or that has no baseline entry, never fails the gate; an unknown runner name is an error that lists the known ones (task 1).
- The override variable set to a reason makes a regressed run pass and says so in the table; unset or empty does not (task 1).
- `--repeat` keeps the minimum wall time, and the real-time factor of a repeated run is computed from it (task 1).
- A file with no message indexes, lz4 chunks or uncompressed chunks reads identically to the reference (task 2).
- The three error messages (`not an MCAP file`, `unfinished recording`, `damaged recording`) and the near-miss `KeyError` are unchanged (task 2).
- A signal recorded across several chunks, selected alone and with others, comes out in the same order and with the same values (task 2).
- The fast reader does not hold every decompressed chunk in memory at once (task 2).

---

### Task 1: The benchmark gate (#61)

**Files:**
- Modify: `scripts/benchmark.py`, `.github/workflows/ci.yml` (the benchmark step of the `sitl` job), `docs/dev-setup.md` (Performance section)
- Create: `scripts/benchmark-baseline.json`
- Test: `python/tests/test_benchmark.py`

**Interfaces:**
- Produces in `scripts/benchmark.py` (importable by the tests): `load_baseline(path, runner) -> tuple[float, dict[str, float]]` returning `(min_ratio, rows)` and raising `ValueError` naming the known runners for an unknown one; `regressions(rows: list[Row], baseline: dict[str, float], min_ratio: float) -> list[Row]` (measured rows with a baseline entry and `rtf < baseline * min_ratio`); `table(rows, baseline=None, accepted="")` (extra columns "Baseline" and "vs baseline" when a baseline is given; `accepted` marks regressed rows `ACCEPTED: <reason>`); `measure_best(sim, seconds, load, sticks, repeat) -> float` (minimum wall seconds over `repeat` runs); command line `--baseline PATH`, `--runner NAME`, `--min-ratio X` (overrides the file), `--repeat N` (default 1), `--print-baseline` (prints `{"rows": {...}}` of the measured rows and exits 0); environment variable `OFS_BENCH_ACCEPT_SLOWER` (the reason).

- [ ] **Step 1: Write the failing tests** in `python/tests/test_benchmark.py` (the module is loaded by path as today):
  - `a_row_below_its_baseline_ratio_is_a_regression`: baseline `{"a": 100.0, "b": 10.0}`, min_ratio 0.6; rows a at 59.0 (regressed), b at 6.0 (exactly 0.6: not regressed), c at 1.0 with no baseline entry (not gated), d skipped (`wall_s=None`, not gated): `[r.name for r in regressions(...)] == ["a"]`.
  - `the_table_shows_the_baseline_and_the_ratio`: with a baseline the text has `| Baseline | vs baseline |` columns and a row like `| open loop, recording | 30.0 | 0.65 | 46.2x | 47.9x | 96 % |`; a regressed row says `REGRESSED`; with `accepted="new tracing"` it says `ACCEPTED: new tracing` instead; without a baseline the table is exactly the old one (the existing table test still passes unchanged).
  - `an_unknown_runner_is_an_error_naming_the_known_ones`: `load_baseline(file, "laptop")` raises `ValueError` whose text contains `ci-linux`; the shipped file loads for `ci-linux` with `min_ratio == 0.6` and the four row names of `CONFIGS`.
  - `the_best_of_n_keeps_the_fastest_run`: `measure_best` with a fake `sim` whose `run` time is scripted (monkeypatch `time.perf_counter` or inject the clock) returns the minimum; with `repeat=1` it runs once.
  - `the_cli_gates_on_a_baseline_and_the_override_passes_it`: call `benchmark.main([...])` against a real open-loop server with `--seconds 0.5`, a temp baseline file giving the open-loop rows an absurd baseline (1e9) and `--runner t`: exit code 1 and stderr names the row; the same call with `OFS_BENCH_ACCEPT_SLOWER=testing` in the environment exits 0 and prints `ACCEPTED: testing`; `--print-baseline` prints parseable JSON with the measured rows.
- [ ] **Step 2: Run** `PYTHONPATH=python python -m pytest python/tests/test_benchmark.py -q` — Expected: FAIL (missing functions/options).
- [ ] **Step 3: Implement** per Design calls 1 to 3. Keep `below_floor`, `Row`, `measure` and the existing table behaviour intact. In `ci.yml`, the benchmark step becomes `python scripts/benchmark.py --binary target/release/ofs-sim --summary --floor 1 --repeat 2 --baseline scripts/benchmark-baseline.json --runner ci-linux` with `env: OFS_BENCH_ACCEPT_SLOWER: ${{ vars.OFS_BENCH_ACCEPT_SLOWER }}`, and a comment explaining the gate and the two ways to accept a slowdown. Create `scripts/benchmark-baseline.json` per Design call 1 (LF, 2-space indent, the run ids in `note`). Update the Performance section of `docs/dev-setup.md`: what the gate compares, the 0.6 ratio, `--repeat`, the baseline file, the two overrides, how to refresh the baseline, that local runs are not gated.
- [ ] **Step 4: Run** the same tests, then `python scripts/benchmark.py --binary target/debug/ofs-sim --seconds 1 --repeat 2 --print-baseline` once by hand — Expected: PASS and parseable JSON.
- [ ] **Step 5: Commit** `feat(ci): a benchmark gate against a recorded baseline (#61)`.

### Task 2: The faster reader (#45)

**Files:**
- Modify: `python/ofs/reader.py`
- Create: `scripts/bench_reader.py`
- Test: `python/tests/test_recording.py` (parity on a real recording), `python/tests/test_reader_formats.py` (new: synthetic files)

**Interfaces:**
- Consumes: the existing `read(path, patterns=None)`, `_select`, `_check_magic`, `_summary`, `_unfinished`, `_damaged`.
- Produces: `ofs.reader._read_with_mcap(path, patterns)` (today's body of `read`, unchanged) and `ofs.reader._read_indexed(f, path, summary, selected)` or equivalent returning the same `(columns, times_ns, rows)`; `read` chooses between them per Design call 5; `scripts/bench_reader.py` with `--binary PATH` and `--seconds 60`.

- [ ] **Step 1: Write the failing tests:**
  - in `test_recording.py`: `the_fast_reader_agrees_with_the_mcap_reader_on_a_real_recording`: a 2-second open-loop recording; for patterns `None`, `["imu.gyro_frd_radps"]`, `["motor.*.omega_radps", "battery.voltage_v"]`: `ofs.reader.read(path, p) == ofs.reader._read_with_mcap(path, p)`; and `ofs.reader._used_indexed_path` (a module-level counter or flag the fast path sets, test-only, documented as such) is true after a plain `read`, proving the fast path ran.
  - `test_reader_formats.py` (skips without `mcap`): a helper writes a small MCAP with the `mcap` writer — three channels (`a` scalar `{"v": ...}`, `b` vector `{"x","y","z"}`, `c` slow scalar), JSON messages, chunks of about 50 messages — in four variants: zstd + message indexes, lz4 + message indexes, uncompressed + message indexes, zstd without message indexes. For each: `read(path)` and `read(path, ["b"])` equal `_read_with_mcap`; the zstd+indexes variant uses the fast path, the other three the fallback (the flag). A signal that appears in several chunks, and two messages of one signal with equal `log_time`, are included.
  - the existing `unfinished`, `damaged`, `not an MCAP` tests and the near-miss `KeyError` test stay green unchanged.
- [ ] **Step 2: Run** them — Expected: FAIL (no `_read_with_mcap`).
- [ ] **Step 3: Implement.** Keep `mcap` for the footer/summary (`_summary`). Fast path: for each chunk index, read the selected channels' message indexes (`opcode u8, length u64, channel u16, array length u32, (log_time u64, offset u64)*`), then, if any, read the chunk record (`opcode, length, start u64, end u64, uncompressed_size u64, crc u32, compression string (u32 length + bytes), records length u64, records`), decompress zstd (or take the bytes as they are for an empty compression string), and for each indexed offset unpack the message header (`opcode u8, length u64, channel u16, sequence u32, log_time u64, publish_time u64`), `json.loads` the data after it and fill the row dict exactly as `_read_with_mcap` does (column names from the first record of a channel). One decompressed chunk lives at a time. Wrap the loop in the same `except Exception -> ValueError(_damaged(path))`. Mark the path taken in a module-level `_used_indexed_path` flag (reset at each `read`).
- [ ] **Step 4: Run** `PYTHONPATH=python python -m pytest python/tests/test_recording.py python/tests/test_reader_formats.py -q` and the whole Python suite — Expected: PASS.
- [ ] **Step 5: Write `scripts/bench_reader.py`** (record `--seconds` of open loop at throttle 0.5 with `ofs.launch`, time `_read_with_mcap` and `read` on `["imu.gyro_frd_radps", "motor.*.omega_radps", "battery.voltage_v"]` and on everything, best of 2, assert equal, print the table with the speed-up) and run it once; put the numbers in the ledger.
- [ ] **Step 6: Commit** `feat(python): a faster recording reader that uses the chunks' message indexes (#45)`.

### Task 3: Docs, changelog, roadmap

**Files:**
- Modify: `CHANGELOG.md` ("Unreleased — M8c"), `docs/roadmap.md` (the Python limitation about the reader's speed; the Tooling backlog "A faster reader"; the Performance section's mention of the 1x floor), `docs/dev-setup.md` (reader numbers next to the performance table), `python/README.md` if it mentions the reader's speed, `docs/research/mcap-determinism.md` only if it states the reader's cost

- [ ] **Step 1: Update** the changelog (Added: the benchmark gate with baseline and override; Changed: the reader is faster, with measured numbers and the fallback), the roadmap (replace "The reader decodes in pure Python: about 3.7 s ..." with the new measured numbers and what the fast path needs; remove the backlog line; the CI gate sentence), and `docs/dev-setup.md`.
- [ ] **Step 2: Check** every number against the ledger's measurements and the CI run ids against the baseline file; `grep -rn "3.7 s\|11.7 s\|fails only below" docs README.md python/README.md CHANGELOG.md` for stale figures outside `docs/superpowers`.
- [ ] **Step 3: Run** the whole gate: `PYTHONPATH=python python -m pytest python/tests -q`, `cargo test --workspace --locked --no-fail-fast` and `cargo clippy --workspace --all-targets --locked -- -D warnings` (nothing Rust changed: confirms the tree), and the WSL Python run of `test_recording.py`, `test_reader_formats.py` and `test_benchmark.py` against Linux — Expected: PASS.
- [ ] **Step 4: Commit** `docs: M8c changelog, roadmap and performance notes (#61, #45)`.
