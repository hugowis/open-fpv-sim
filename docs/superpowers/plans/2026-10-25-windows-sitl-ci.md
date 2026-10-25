# Windows SITL CI Implementation Plan

**Goal:** A non-blocking `windows-sitl` CI job that builds and live-tests the native Windows SITL (#107).
**Spec:** `docs/superpowers/specs/2026-10-25-windows-sitl-ci-design.md`

## Constraints
- Existing jobs untouched; `publish.needs` unchanged.
- Do not push or open a PR until the user says so.
- Gates: `cargo test --workspace --locked`, clippy `-D warnings` (no Rust changes expected; run once at the end).

## Tasks
- [ ] **1. Stub check in the build script.** `scripts/build-sitl-windows.sh`: `LC_ALL=C make` with a log; on failure list
  the `undefined reference` symbols, name `win_stubs.c`, emit `::error` under `GITHUB_ACTIONS`. Verify in local MSYS2: a
  fresh build succeeds; a copy of the Windows patch without the `IORead` stub fails naming `IORead`.
- [ ] **2. The `windows-sitl` job** in `.github/workflows/ci.yml` (cache, setup-msys2, build, live tests, Python SITL
  tests, benchmark, artifact). Verify: YAML parses; the test commands run locally against the task-1 exe.
- [ ] **3. Docs.** `docs/dev-setup.md`, `docs/research/sitl-native-windows.md` (CI section + the record table).
- [ ] **4. Hand over**: gates, then ask the user to approve a push to prove the job.

One commit per task.
