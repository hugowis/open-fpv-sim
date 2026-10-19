# M9c: macOS survey (#68) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Report what builds and what runs on macOS (the first contribution #68 asks for): the Rust workspace, the Python package and tests, the Godot client and its test runner, and Betaflight SITL, on Apple silicon and Intel, then fix what is cheap and write down what is not.

**Architecture:** There is no Mac on the maintainer's machine, so GitHub's macOS runners are the Mac (the repository is public: the minutes are free). A separate, non-gating workflow `.github/workflows/macos-survey.yml` runs every step with `continue-on-error`, records each step's outcome and the first lines of its error in the job summary, and uploads the logs. The survey runs on this branch until it is understood, then the workflow keeps only `workflow_dispatch` (a button, not a trigger on every push). Stage B fixes what the survey shows is small; Stage C writes `docs/research/macos.md` and the roadmap/dev-setup entries, and decides whether a macOS CI job, a SITL port or a Linux-VM route is the next piece.

**Tech Stack:** GitHub Actions (`macos-latest` = arm64, `macos-15-intel` = x86_64), Rust, Python, bash, Betaflight 2026.6.2 SITL (`scripts/build-sitl.sh`, `third_party/betaflight/ofs-sitl.patch`).

**Spec:** issue #68 ("start by reporting what builds and what does not; that list is the first contribution"), `docs/research/sitl-native-windows.md` (the same kind of survey for Windows) and the M9a/M9b plans.

## Global Constraints

- Nothing on the maintainer's machine changes; every macOS step runs on a runner. Pushing this branch to run the survey is the only outward effect until the PR.
- The existing jobs of `ci.yml` do not change in Stage A; no gate is added until Stage C says what is stable.
- Per-OS golden digests: macOS gets none in this plan; a golden test that has no file for the OS is a finding to report (and, if cheap, to make skip with a clear message rather than panic), not a digest to generate from a runner without a decision.
- WSL/Linux and native Windows behaviour stay byte-for-byte unchanged (`cfg`-gate anything macOS specific); MSRV 1.85.
- `docs/research/macos.md` states only what a runner showed, with the run id; no claim about hardware the survey did not touch.

## Review Focus

- **The macOS process-name rule.** A macOS process is named by its whole executable name (`betaflight_SITL.elf`), not Linux's 15-character `comm` (`betaflight_SITL`), and there is no `/proc`: the M9a slot cleanup (`pgrep -x betaflight_SITL` plus `/proc/<pid>/cmdline`) and the native wait are blind on macOS. A stray SITL then survives and the next session fails to bind.
- **Apple silicon vs Intel.** Float behaviour (`-Ofast`, libm) and pointer/`long` ABI differences between the two; the radio goldens are per-OS and must not silently pass or panic on a third OS.
- **Dual-stack and port semantics.** BSD sockets: `SO_REUSEADDR`, IPv6 wildcard dual-stack, `localhost` resolution; the stale-SITL bind check depends on them (Windows broke it).
- **Missing POSIX pieces.** `pthread_condattr_setclock`, `clock_gettime`, `nproc`, `-lrt`, GNU `sed`/`grep`/`date` flags in the scripts.
- **Silent skips.** A suite that "passes" because the SITL tests skip (no `OFS_SITL_LAUNCH`) must be reported as skipped, not as green.

---

### Task 1: The survey workflow

**Files:**
- Create: `.github/workflows/macos-survey.yml`, `scripts/macos-survey.sh`

**Interfaces:**
- Produces: a workflow with a matrix over `macos-latest` and `macos-15-intel`; `scripts/macos-survey.sh <step-name> <command...>` runs a command with its output in `survey/<step-name>.log`, never fails the job, and appends `| <step-name> | ok/FAILED (<exit>) | <first error line> |` to `$GITHUB_STEP_SUMMARY`.

- [ ] **Step 1: Write `scripts/macos-survey.sh`** (bash 3.2 compatible: macOS ships it).
- [ ] **Step 2: Write the workflow.** Triggers: `workflow_dispatch` and `push` on branch `worktree-m9c-macos-survey` only. Steps (each through the script): toolchain versions (`sw_vers`, `uname -m`, `rustc -V`, `python3 -V`, `clang -V`, `make -v`); `cargo build --workspace --locked`; `cargo build -p ofs-sim --locked`; `cargo test --workspace --locked --no-fail-fast`; `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo check --workspace --all-targets --locked --exclude ofs-godot` on Rust 1.85; Python: `pip install -e "python[dev]"` then `pytest python/tests`; `bash scripts/run-godot-tests.sh unit`; `bash scripts/build-sitl.sh` with `OFS_SITL_PATCH=none` (stock) and then with the patch; if a SITL binary exists, the live tests (`sitl_live`, `vtx_live`, the Python SITL files). Upload `survey/` as an artifact.
- [ ] **Step 3: Commit and push the branch.** Expected: the workflow starts on both runners (`gh run list --branch worktree-m9c-macos-survey`).

### Task 2: Read the first results

**Files:**
- Modify: `docs/research/macos.md` (new; the results table, one row per step and runner)

**Interfaces:**
- Consumes: Task 1's run (`gh run view <id> --log`, the artifact logs).
- Produces: for every step that failed, the first distinct errors with file:line, and a verdict per area: Rust workspace, Python package, tests (skipped vs passed counts), Godot, SITL (stock build, patched build, run). Gate: Stage B starts only for items the table shows as small.

- [ ] **Step 1: Download the logs** (`gh run download`) into the scratchpad; read the summary tables.
- [ ] **Step 2: Write the results table** and the list of distinct errors, with run ids.
- [ ] **Step 3: Commit** (`docs: macOS survey, first results (#68)`).

### Task 3: Fix what is cheap (Stage B)

**Files:** decided by Task 2; candidates named by the Review Focus: `crates/ofs-fc/src/sitl/net.rs` (`SLOT_KILL_SCRIPT` and `slot_pids_sh!`: match the macOS process name, read the command line with `ps`/`pgrep -f` where `/proc` is missing), `crates/ofs-fc/src/sitl/process.rs` (`await_cleanup` on macOS), the golden tests (no digest file for the OS), `scripts/build-sitl.sh` (`nproc`), `scripts/run-godot-tests.sh` (the Linux-only Godot asset), `python/tests`.

**Interfaces:**
- Consumes: Task 2's table. Produces: a rerun of the survey where each fixed step turns green, each fix behind `cfg(target_os = "macos")` or a portable shell form, with a test that fails first where a test can run on Linux (for example the slot rule as a pure function over a process list).

- [ ] **Step 1: For each fix, write the failing test or the failing survey step first**, watch it fail, implement, rerun the survey.
- [ ] **Step 2: Linux and Windows gates unchanged:** `cargo test --workspace --locked` and clippy on Windows; the WSL workspace and live suites.
- [ ] **Step 3: SITL port decision.** If the patched SITL fails to build for at most three distinct reasons, fix them in a `third_party/betaflight/ofs-sitl-macos.patch` and `scripts/build-sitl.sh` (as M9b did for Windows); otherwise record the list and stop at the survey.
- [ ] **Step 4: Commit** per fix (`fix(...): ... on macOS (#68)`).

### Task 4: Findings, decision and docs (Stage C)

**Files:**
- Modify: `docs/research/macos.md`, `docs/dev-setup.md` (macOS section), `docs/roadmap.md`, `CHANGELOG.md`, `.github/workflows/macos-survey.yml` (drop the push trigger; keep `workflow_dispatch`)

- [ ] **Step 1: Write the verdicts and the decision**: what works on macOS today, what does not and why, what a supported macOS path costs (a CI job, a SITL port or a Linux VM such as Lima/OrbStack for SITL), in order of value.
- [ ] **Step 2: Update** the dev-setup guide (the macOS paragraph with the commands that worked), the roadmap, the changelog; remove the push trigger from the survey workflow.
- [ ] **Step 3: Check** `python -m pytest python/tests/test_release.py` and the doc links; commit (`docs: macOS findings and decision (#68)`).
