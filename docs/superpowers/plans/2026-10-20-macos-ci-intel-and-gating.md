# macOS CI Intel and gating Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the Intel side of the macOS survey (the Godot suites that were never reached, including the DVR frame-count check) and promote the Apple-silicon `macos` CI job from `continue-on-error` to a required check once its record is clean.

**Architecture:** Split the Godot test runner into named suites so a failing open-loop test no longer prevents the error and Betaflight suites from running on Intel. Replace the open-loop DVR's field-rate frame check with a check tied to the field boundaries the client actually rendered, which is what the DVR records. Then run the manual survey on both runners, record the results, and apply the gating change only when ten recent `main` runs have no failed `macos` step.

**Tech Stack:** bash, Godot 4.7.2 headless, GitHub Actions (`macos-latest`, `macos-15-intel`, `ubuntu-24.04`), `gh` CLI.

**Spec:** PR #101's "Not done / deferred minors" (Intel: the Godot DVR check, the unreached suites; the `continue-on-error` decision), `docs/research/macos.md` sections "Godot" and "Decision".

## Execution base

Start from a fresh worktree on `origin/main` (`341ad51`):

```bash
git fetch origin
git worktree add ../ofs-macos-ci -b worktree-macos-ci origin/main
```

## Global Constraints

- The Linux `godot` job's default (`scripts/run-godot-tests.sh all`) must keep running the same suites in the same order; existing users of `unit` and `e2e` keep their meaning.
- The DVR test must still prove that the recorder writes one frame per field boundary; it must not simply lower the bound until slow runners pass.
- The macOS `ci.yml` job stays `continue-on-error` until the record check in Task 4 passes. Do not promote it in the same commit as the survey changes.
- `macos-15-intel` remains survey-only; this plan does not add a second gating job.
- MSRV is 1.85; no new crates.
- Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` before the final commit.

## Review Focus

- **Suite selection.** `open-loop` must not silently imply `unit` or `errors`; `all` must remain the union. A typo in the argument must fail before downloading Godot.
- **DVR on a slow runner.** The recorder writes what the pilot sees; a 22 fps runner legitimately has fewer frames than a 50 Hz field clock. The assertion must compare frames to rendered boundaries, not to simulated seconds.
- **Intel-specific failures.** One failing suite must not hide the next; the survey must record all four Godot outcomes.
- **`continue-on-error` visibility.** The job conclusion is green even when a step fails. Any "ten green runs" claim must count failed steps, not job conclusions.
- **Windows/Linux unchanged.** The runner script changes must not alter the `all` path on the Linux `godot` job.

---

### Task 1: Named Godot suites

**Files:**
- Modify: `scripts/run-godot-tests.sh`
- Test: `scripts/run-godot-tests.sh` (shell behaviour; run on Linux or macOS)

**Interfaces:**
- Consumes: the existing `run_godot`, `GODOT_BIN`, import, and `OFS_SITL_LAUNCH` handling.
- Produces: `scripts/run-godot-tests.sh unit|open-loop|errors|betaflight|e2e|all`. `e2e` = `open-loop` + `errors` + `betaflight` (the old `e2e`), `all` = `unit` + `e2e`.

- [ ] **Step 1: Demonstrate the missing modes**

Run: `bash scripts/run-godot-tests.sh open-loop`
Expected: FAIL with `usage: scripts/run-godot-tests.sh [unit|e2e|all]` and exit 1.

- [ ] **Step 2: Update the usage and selection**

In `scripts/run-godot-tests.sh`, replace the header usage lines and the `suite` case with:

```bash
## Usage: scripts/run-godot-tests.sh [unit|open-loop|errors|betaflight|e2e|all]   (default: all)
##   e2e = open-loop + errors + betaflight (when OFS_SITL_LAUNCH is set); all = unit + e2e.
```

```bash
# The mode is selected here so an unknown one fails loudly before anything runs; it would
# otherwise run no suite at all and still print "Godot tests passed.".
case "$suite" in
    unit|open-loop|errors|betaflight|e2e|all) ;;
    *) echo "usage: scripts/run-godot-tests.sh [unit|open-loop|errors|betaflight|e2e|all]"; exit 1 ;;
esac
```

Then replace the suite dispatch at the end with:

```bash
if [ "$suite" = unit ] || [ "$suite" = all ]; then
    run_godot 120 --headless --path godot -s res://tests/run_tests.gd
fi

if [ "$suite" = open-loop ] || [ "$suite" = e2e ] || [ "$suite" = all ]; then
    run_godot 180 --headless --path godot -s res://tests/e2e_open_loop.gd
fi

if [ "$suite" = errors ] || [ "$suite" = e2e ] || [ "$suite" = all ]; then
    run_godot 180 --headless --path godot -s res://tests/e2e_errors.gd
fi

if [ "$suite" = betaflight ] || [ "$suite" = e2e ] || [ "$suite" = all ]; then
    if [ -n "${OFS_SITL_LAUNCH:-}" ]; then
        run_godot 300 --headless --path godot -s res://tests/e2e_betaflight.gd
    else
        echo "SKIP: OFS_SITL_LAUNCH is not set, so there is no Betaflight SITL to fly"
    fi
fi
```

- [ ] **Step 3: Check the shell parses and the unknown mode fails**

```bash
bash -n scripts/run-godot-tests.sh
bash scripts/run-godot-tests.sh nope; test $? -eq 1
```

Expected: `bash -n` silent; `nope` prints the new usage and exits 1.

- [ ] **Step 4: Run the unit suite on Linux**

Run: `bash scripts/run-godot-tests.sh unit`
Expected: PASS (downloads Godot if needed). This proves the `all` path still starts with the unit suite.

- [ ] **Step 5: Commit**

```bash
git add scripts/run-godot-tests.sh
git commit -m "test(godot): name the suites so the survey can keep going (#101)"
```

---

### Task 2: Make the DVR check measure what the DVR records

**Files:**
- Modify: `godot/tests/e2e_open_loop.gd`
- Test: `godot/tests/e2e_open_loop.gd` (run on the macOS survey; local Linux run optional)

**Interfaces:**
- Consumes: `app.client.get_telemetry()["time_s"]`, `app.client.get_fpv_view()["target_field"]`, `app.dvr.recording`.
- Produces: the open-loop DVR check compares `frames.size()` with the number of distinct `target_field` values seen between DVR start and stop, allowing one boundary of start/stop skew.

- [ ] **Step 1: Capture the current failure on the slow runner**

Run the existing survey on Intel once (the workflow from Task 3 can be used, or the current `macos-survey.yml`), then read the open-loop log:

```bash
gh run download <run-id> -n survey-macos-15-intel -D /tmp/ofs-intel-survey
rg -n "frames for|FAIL" /tmp/ofs-intel-survey/godot-all.log
```

Expected: the recorded `44 frames for 2.01 simulated seconds ... (about 100)` failure, or a newer value with the same shape. Record the run id for the findings doc.

- [ ] **Step 2: Replace the field-rate block**

In `godot/tests/e2e_open_loop.gd`, replace the block from `var t0: float = ...` through the `every frame is a JPEG` check with:

```gdscript
	var t0: float = app.client.get_telemetry()["time_s"]
	var seen_targets := {}
	var deadline := Time.get_ticks_msec() + 20_000
	_press(app, KEY_D)
	_check(app.dvr.recording, "D starts the DVR")
	while float(app.client.get_telemetry()["time_s"]) < t0 + 2.0:
		var target := int(app.client.get_fpv_view().get("target_field", -1))
		if target >= 0:
			seen_targets[target] = true
		await process_frame
		if Time.get_ticks_msec() >= deadline:
			break
	var t1: float = app.client.get_telemetry()["time_s"]
	_press(app, KEY_D)
	_check(t1 >= t0 + 2.0, "the client advanced two simulated seconds (%.2f)" % (t1 - t0))
	_check(not app.dvr.recording and started.size() == 1, "D again stops it")
	if started.size() == 1:
		var frames := _avi_frames(ProjectSettings.globalize_path(started[0]))
		# The DVR is a screen recorder: it writes one frame per field boundary the client actually renders, not one
		# per simulated 20 ms. A runner slower than 50 Hz renders fewer boundaries; that is the pilot's picture.
		var expected := seen_targets.size()
		_check(expected > 0, "the client rendered at least one field boundary")
		_check(frames.size() >= expected - 1 and frames.size() <= expected + 3,
			"%d frames for %d rendered field boundaries (%.2f simulated seconds)" % [frames.size(), expected, t1 - t0])
		_check(frames.all(func(f: PackedByteArray) -> bool: return f.size() > 2 and f[0] == 0xFF and f[1] == 0xD8),
			"every frame is a JPEG")
```

- [ ] **Step 3: Run the open-loop suite on the fast runner**

Run: `bash scripts/run-godot-tests.sh open-loop`
Expected: PASS on Linux/Apple silicon, with a new `%d frames for %d rendered field boundaries` line. If the count is more than one below `expected`, the DVR is dropping a field and the bound should not be widened; investigate `dvr.capture` instead.

- [ ] **Step 4: Run the open-loop suite on Intel**

Trigger the survey from Task 3 (or run the current workflow) and read the Intel open-loop log:

```bash
gh run download <run-id> -n survey-macos-15-intel -D /tmp/ofs-intel-survey
rg -n "frames for|FAIL" /tmp/ofs-intel-survey/godot-open-loop.log
```

Expected: PASS with a much smaller `expected` than 100 (for example 44 rendered boundaries for 44 frames). This is the acceptance that the old check was measuring the runner's speed, not a DVR defect.

- [ ] **Step 5: Commit**

```bash
git add godot/tests/e2e_open_loop.gd
git commit -m "test(godot): the DVR check counts rendered fields, not wall time (#101)"
```

---

### Task 3: Let the Intel survey reach every Godot suite

**Files:**
- Modify: `.github/workflows/macos-survey.yml`
- Modify: `docs/research/macos.md`

**Interfaces:**
- Consumes: Task 1's suite names; `scripts/macos-survey.sh`, which records a failed step and continues.
- Produces: separate survey rows `godot-unit`, `godot-open-loop`, `godot-errors`, `godot-betaflight`; on a runner without a SITL, the first three still run and the fourth is marked skipped.

- [ ] **Step 1: Replace the single Godot step**

In `.github/workflows/macos-survey.yml`, in the `if [ -x "$bin" ]` block, replace

```yaml
            bash scripts/macos-survey.sh godot-all bash scripts/run-godot-tests.sh all
```

with:

```yaml
            bash scripts/macos-survey.sh godot-unit bash scripts/run-godot-tests.sh unit
            bash scripts/macos-survey.sh godot-open-loop bash scripts/run-godot-tests.sh open-loop
            bash scripts/macos-survey.sh godot-errors bash scripts/run-godot-tests.sh errors
            bash scripts/macos-survey.sh godot-betaflight bash scripts/run-godot-tests.sh betaflight
```

and replace the `else` branch's single `godot-all` line with:

```yaml
            bash scripts/macos-survey.sh godot-unit bash scripts/run-godot-tests.sh unit
            bash scripts/macos-survey.sh godot-open-loop bash scripts/run-godot-tests.sh open-loop
            bash scripts/macos-survey.sh godot-errors bash scripts/run-godot-tests.sh errors
            echo "| godot-betaflight | skipped | | no patched SITL binary |" >> "$GITHUB_STEP_SUMMARY"
```

- [ ] **Step 2: Run the survey on both runners**

```bash
git push -u origin worktree-macos-ci
gh workflow run macos-survey.yml
gh run list --workflow=macos-survey.yml --limit 3
```

Wait for completion, then download both artifacts:

```bash
gh run download <run-id> -n survey-macos-latest -D /tmp/ofs-arm-survey
gh run download <run-id> -n survey-macos-15-intel -D /tmp/ofs-intel-survey
Get-ChildItem /tmp/ofs-arm-survey,/tmp/ofs-intel-survey
```

Expected: four `godot-*.log` files per runner. The Intel run now reaches `godot-errors` and `godot-betaflight` even if `godot-open-loop` fails.

- [ ] **Step 3: Record the results**

In `docs/research/macos.md`:

1. In the "Godot" section, replace "the script stops at the first failing step, so the error and Betaflight end-to-end tests have **never run on Intel**" with the survey run id and the outcome of each of `unit`, `open-loop`, `errors`, `betaflight` on Intel.
2. If a suite fails, record the first distinct error and leave the wording "not yet passing"; do not claim a pass from a missing log.
3. Update the table's Godot row for Intel with the four outcomes.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/macos-survey.yml docs/research/macos.md
git commit -m "ci(macos): the survey reaches every Godot suite on Intel (#101)"
```

---

### Task 4: Promote the macOS job when the record is clean

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/research/macos.md`

**Interfaces:**
- Consumes: the `macos` job's step conclusions in recent `ci.yml` runs.
- Produces: `macos` without `continue-on-error` and listed in `publish.needs`, or a recorded "not yet" with the evidence.

- [ ] **Step 1: Count failed macOS steps in the last ten main runs**

```bash
for id in $(gh run list --workflow=ci.yml --branch main --limit 10 --json databaseId --jq '.[].databaseId'); do
  echo "run $id"
  gh run view "$id" --json jobs --jq '.jobs[] | select(.name=="macos") | .steps[] | select(.conclusion=="failure") | "  FAILED: " + .name'
done
```

Expected: either no `FAILED:` lines (the record is clean) or a list of the steps that failed. The job conclusion cannot be used: `continue-on-error` keeps it green.

- [ ] **Step 2: If the record is clean, remove the waiver**

In `.github/workflows/ci.yml`, delete:

```yaml
    # Shown as passing even when it fails, until it has a track record (docs/research/macos.md: about ten green runs); the
    # failure is still listed in the run and its annotations.
    continue-on-error: true
```

and add `macos` to the publish job:

```yaml
  publish:
    if: startsWith(github.ref, 'refs/tags/v')
    needs: [core, windows, sitl, macos]
```

Update the comment above the `macos` job so it no longer says "Not in `publish`'s needs" or "best effort for now"; say the job gates releases as of the first clean run id.

- [ ] **Step 3: If the record is not clean, record it and stop**

Do not remove `continue-on-error`. Append to `docs/research/macos.md` under "Decision" a line with the last ten run ids and the failed step names, and leave the job as it is. The next attempt is the next ten runs.

- [ ] **Step 4: Prove the gate locally**

```bash
cargo test --workspace --locked -- --test-threads=1
cargo clippy --workspace --all-targets --locked -- -D warnings
python -m pytest python/tests -v
```

Expected: PASS on the local OS; the macOS step cache is unchanged.

- [ ] **Step 5: Commit**

If promoted:

```bash
git add .github/workflows/ci.yml docs/research/macos.md
git commit -m "ci: the macOS job gates releases (#101)"
```

If not yet:

```bash
git add docs/research/macos.md
git commit -m "docs(macos): the job's record is not yet clean (#101)"
```

---

### Task 5: Verify on a pull request

**Files:**
- No source changes; acceptance.

- [ ] **Step 1: Push and open a PR**

```bash
git push -u origin worktree-macos-ci
gh pr create --title "M9c follow-up: Intel survey and the macOS gate" --body-file - <<'EOF'
Finishes the deferred Intel survey and promotes the macOS job when its record allows.
EOF
```

- [ ] **Step 2: Check the run**

```bash
gh pr checks
gh run view <run-id> --json jobs --jq '.jobs[] | [.name,.conclusion] | @tsv'
```

Expected: `core`, `msrv`, `windows`, `godot`, `sitl` green; `macos` green with no failed steps (inspect with the Task 4 command). If the macOS job is still `continue-on-error`, its job conclusion is green; the failed-step query is the real gate.

## Self-review notes

- Coverage: the Intel DVR check and the unreached error/Betaflight suites are covered; the gating decision has explicit criteria.
- Placeholder scan: run ids are placeholders in commands only; the plan says exactly which value to record.
- Type consistency: suite names (`unit`, `open-loop`, `errors`, `betaflight`, `e2e`, `all`) are the same in the script, workflow, and text.
