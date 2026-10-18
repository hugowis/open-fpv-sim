# B3: M8c Benchmark-Gate Truth and Reader Robustness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (native run, chosen by the user). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the M8c deferred minors: the baseline numbers tell the truth, the CLI refuses misuse instead of ignoring it, the fast reader either reads a corrupted index correctly or falls back — never returns wrong data — and the wording matches the code.

**Architecture:** Four tasks, each one commit. Task 1 is data/docs; task 2 is `scripts/benchmark.py` CLI guards; task 3 is `python/ofs/reader.py` index validation; task 4 is a wording sweep.

**Tech Stack:** Python `ofs` and its test suite; no Rust changes (goldens untouched by construction).

**Spec:** none. The scope is the "Deferred minors" list in the body of PR #86 (`gh pr view 86`).

## Global Constraints

- `PYTHONPATH=python python -m pytest python/tests -q` green; `python -m pytest python/tests/test_benchmark.py python/tests/test_reader*.py -q` green.
- The baseline file stays LF, 2-space indented JSON (its existing style).
- Ruling (baseline): only the "open loop, recording" row was wrong (median 47.1, not 47.9) and the agreement note (about 6 %, not 4 %); the gate moves 28.7 → 28.3, negligible — no re-measurement.
- Ruling (corruption): any inconsistency detected inside the indexed path falls back to the slow path (which owns the "damaged" error); the index path itself never returns wrong data silently.

## Review Focus

- A recording whose message index is corrupted but whose chunks are intact must read correctly (through the fallback), not raise "damaged" (task 3).
- A recording whose index entry lies about its offset or time must not yield wrong rows silently (task 3's new checks).
- `--runner x` without `--baseline` must be an argument error, not silently ignored (task 2).
- `--print-baseline` on a machine without `OFS_SITL_LAUNCH` must not produce JSON whose paste-over would silently un-gate the Betaflight rows (task 2).

---

### Task 1: The baseline file and the docs carry the real numbers

**Files:**
- Modify: `scripts/benchmark-baseline.json` (the "open loop, recording" row 47.9 → 47.1; the note's "agreed within 4 %" → "agreed within about 6 %")
- Modify: `docs/dev-setup.md:327–328` (47.9× → 47.1×; "agreed within 4 %" → "about 6 %"; keep the surrounding sentences)
- Modify: `CHANGELOG.md` (only if the Unreleased section from batch 1 already mentions the numbers; otherwise nothing — the 0.2.0 text is released history)

- [ ] **Step 1: Edit the three sites.**
- [ ] **Step 2: Sanity-check the gate**: `python -c` computing `0.6 * 47.1` and noting the old `0.6 * 47.9` in the commit message (28.3 vs 28.7 — the gate's effect is unchanged).
- [ ] **Step 3: Commit** `fix(bench): the ci-linux baseline row is 47.1 and the runs agreed within about 6 %`.

---

### Task 2: The benchmark CLI refuses misuse

**Files:**
- Modify: `scripts/benchmark.py` (arg parsing at lines 108–120; `load_baseline` at line 77; the `--print-baseline` block at lines 139–141)
- Test: `python/tests/test_benchmark.py`

**Interfaces:**
- Produces: `--runner`/`--min-ratio` without `--baseline` are `parser.error` (exit 2). An unknown runner prints `…known: …` to stderr and exits 2 (no traceback). `--print-baseline` prints every row, `null` for the skipped ones, plus a stderr warning naming `OFS_SITL_LAUNCH`; `load_baseline` skips `null` rows (they become "no baseline for row: not gated" at gate time, the existing message).

- [ ] **Step 1: Write the failing tests** (following the file's existing patterns — it launches a real open-loop server with `--seconds 0.5`):
  - `the_cli_refuses_runner_or_ratio_without_a_baseline`: `main(["--runner", "x"])` and `main(["--min-ratio", "0.5"])` raise SystemExit(2) (argparse's error exit).
  - `an_unknown_runner_is_a_clean_error`: a temp baseline file + `--runner nope` → main returns 2 and stderr contains `known:` (capture with capsys).
  - `print_baseline_names_the_rows_it_could_not_measure`: with `OFS_SITL_LAUNCH` unset, `--print-baseline` output parses as JSON and has `null` for the Betaflight rows and numbers for the open-loop rows; stderr says `OFS_SITL_LAUNCH`.
  - `load_baseline_skips_null_rows`: a baseline file with `"Betaflight SITL, recording": null` loads without it in the rows dict.
- [ ] **Step 2: Run** `python -m pytest python/tests/test_benchmark.py -q` — Expected: the new tests FAIL (traceback for the unknown runner; JSON without the null rows).
- [ ] **Step 3: Implement** the four pieces. Wrap the baseline-loading block in `try/except ValueError` → stderr + `return 2`. In `load_baseline`, skip `None` values. In the `--print-baseline` block, build the dict with `None` for `r.rtf is None` rows and warn when any was skipped.
- [ ] **Step 4: Run** the whole test file, then `python scripts/benchmark.py --help` once by eye — Expected: pass; help unchanged.
- [ ] **Step 5: Commit** `fix(bench): the CLI refuses a stray --runner/--min-ratio and prints what --print-baseline could not measure`.

---

### Task 3: The indexed reader validates its index and falls back

**Files:**
- Modify: `python/ofs/reader.py` (`_collect_by_index`, lines 154–201; `read`, lines 217–238)
- Test: the reader test file (find it: `grep -rln "_used_indexed_path\|damaged" python/tests/`)

**Interfaces:**
- Produces: a module-private `_IndexCorrupt(Exception)` raised for every inconsistency the index path detects: an index record whose opcode is not a message index or whose channel id does not match, an entry whose offset does not hold that channel's message header, an entry whose index time differs from the message header's time, a cut-short index or chunk, the existing opcode/channel-header mismatch, the counts mismatch. `read()` catches `_IndexCorrupt` and takes the slow path (`_used_indexed_path = False`); the slow path's errors surface as before (`_damaged`).

- [ ] **Step 1: Write the failing tests** (the file's fixtures already build recordings; corrupt one):
  - `a_corrupted_message_index_falls_back_to_the_slow_path`: flip a byte inside a chunk's message-index region (locate it via the summary offsets, as `_collect_by_index` does); `read()` returns the same rows as `reader._read_with_mcap(path, None)` and `_used_indexed_path` is False; no exception.
  - `an_index_entry_pointing_at_the_wrong_record_is_detected`: harder to synthesize byte-exactly — instead unit-test the guard directly: call `_collect_by_index` against a summary whose `message_index_offsets` were perturbed by ±1 on one entry (mutate the in-memory structures the same way the existing tests do, if they do; otherwise perturb the file bytes) and assert `_IndexCorrupt` (or the fallback result through `read`).
  - `an_entry_whose_time_disagrees_with_its_header_falls_back`: same mechanism, one byte of a `log_time` in an index entry.
  - Existing damaged-file tests must keep passing (the true damage message now comes only from the slow path).
- [ ] **Step 2: Run** the reader tests — Expected: the new ones FAIL (today a flipped index byte either raises `_damaged` or, worse, silently returns shifted rows).
- [ ] **Step 3: Implement**: raise `_IndexCorrupt` at each guard (convert the existing `raise ValueError(...)` sites in `_collect_by_index` and the counts mismatch); in `read`, wrap the `_collect_by_index` call in `except _IndexCorrupt: collected = None`.
- [ ] **Step 4: Run** the reader tests, then `python -m pytest python/tests -q` — Expected: pass. Note in the commit message that wrong-data-on-corruption is now impossible by construction (every index-derived value is cross-checked against the message header or the statistics counts).
- [ ] **Step 5: Commit** `fix(python): the indexed reader cross-checks its index and falls back instead of trusting it`.

---

### Task 4: The wording sweep

**Files (each: modify + one-line reason):**
- `scripts/bench_reader.py:48` — label `"3 signals"` → `"3 patterns"` (it is `THREE`, three patterns over six signals).
- `docs/dev-setup.md:362` — "three signals" → "three patterns (six signals)".
- `CHANGELOG.md:91` — the sentence credits `--repeat 2` for what the 0.6 ratio does; rewrite to: each row is the best of two runs because noise only ever slows a run, and the 60 % ratio leaves room for the ~4 % difference between CI runs (released text is history — fix it only if an Unreleased section repeats it; otherwise note it in the commit and leave 0.2.0's text alone).
- `docs/roadmap.md:96` and `docs/dev-setup.md:357` — the 12.4× figures (run 38453432260) are superseded by the baseline; reword to the current figures (Betaflight rows 10.0×/11.8×, open loop 47.1×/131.9×, see the Performance section) without pretending to be a fresh measurement.
- `docs/dev-setup.md:335` — append: delete the `OFS_BENCH_ACCEPT_SLOWER` repository variable once the baseline PR has merged (it accepts every future regression while set).
- `python/ofs/reader.py` docstring or the reader's `read` doc-comment — one line: on foreign files whose log times run backwards within a topic or whose JSON keys vary, column names/order can differ from the mcap-iterator path.

- [ ] **Step 1: Make the edits** (each one line, plus the doc note).
- [ ] **Step 2: Run** `python -m pytest python/tests/test_benchmark.py -q` and grep `12.4×|3 signals|three signals` outside `docs/superpowers` — Expected: pass; no stale wording left outside history docs.
- [ ] **Step 3: Commit** `docs: the reader and benchmark wording matches what the code does`.

---

## Not in this plan

- Re-measuring the baseline: the runs are named in the file; the correction is arithmetic on them (the PR body's finding).
- The Blackbox step-response comparator and thrust-stand work: open issues #51/#50.
- The benchmark-gate SITL flake in CI: the sitl job's own known flakiness, already re-run-and-report.

## Self-Review

- **Coverage:** PR #86's six bullets — baseline provenance (1), accept-slower lifecycle (4 via dev-setup:335), `--print-baseline` rows (2), silent flags/traceback (2), fast-path robustness (3), wording (4). The 5c column-name difference is ruled a documentation item (task 4), matching the PR's own framing.
- **Placeholders:** task 3 leaves the corruption-synthesis mechanism to the implementer with two named options and the rule (the file's existing fixtures decide); everything else is exact.
- **Interfaces:** `load_baseline` keeps its signature; `regressions` already skips `None` rows; `_IndexCorrupt` is module-private.
