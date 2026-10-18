# M9b: native Windows SITL spike (#67) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Find out whether the pinned Betaflight SITL (2026.6.2 plus our lockstep patch) builds and runs natively on Windows with MSYS2, with no WSL, record what is POSIX-only and what a port costs, and, if it runs, make the simulator launch it.

**Architecture:** A time-boxed spike with two decision gates. Stage A is research on the machine: install MSYS2 (user-approved winget install), build the stock and then the patched SITL in the Cygwin-based `msys` environment first (POSIX headers, sockets and `fcntl` exist there) and in `ucrt64` second (native Win32, needs Winsock shims), recording every error. Stage B runs only if a binary boots: a Windows patch on top of `ofs-sitl.patch`, a build script, a live run through `ofs-sim`, and the Rust pieces the Windows host lacks (a slot-scoped cleanup, since `default_cleanup` returns nothing off unix). Stage C writes the findings and the go/no-go either way.

**Tech Stack:** MSYS2 (pacman, gcc, make), Betaflight 2026.6.2 (`e0b7bb0`), C patch, Rust (`ofs-fc` `sitl::net` / `sitl::process`), PowerShell and bash scripts.

**Spec:** issue #67 ("what in the SITL target and the patch is POSIX-only (sockets, `pthread`, the timer)?"), `docs/research/sitl-interface.md` §6 ("Native Windows build: not attempted") and the M9a plan's slot design (`docs/superpowers/plans/2026-10-18-m9a-parallel-sitl.md`).

## Global Constraints

- The user approved installing MSYS2 with winget (2026-10-18); nothing else is installed on the machine without asking. Everything built goes outside the repo (scratch clone under `C:\ofs-win`), never into the working tree.
- WSL stays the supported path and must keep working: `scripts/build-sitl.sh` and `third_party/betaflight/ofs-sitl.patch` are not changed by Stage B except where the Linux build still passes (the CI `sitl` job rebuilds from the patch).
- Per-OS golden digests do not move; no model, protocol or Python API changes.
- MSRV 1.85 for the crates touched; `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- Findings go in `docs/research/sitl-native-windows.md` (every error with its fix or its blocker), linked from `docs/research/sitl-interface.md` §6, `docs/roadmap.md` and `docs/dev-setup.md`.
- The pinned tag and SHA do not change (`BF_TAG=2026.6.2`, `e0b7bb01b17b21351057e9ead2d1ab39dd44fa16`).

## Review Focus

- **A stray `betaflight_SITL.exe` holding the ports.** Windows has no `pkill`: a hard-killed simulator leaves its SITL running, and the next session in that slot must remove it, and only it (slot-scoped by `--port-offset`, as M9a does on Linux), or the bind fails.
- **Paths with spaces and non-ASCII** (`C:\Users\First Last\...`, `C:\Program Files\...`) in the SITL binary path, the firmware directory and `--config betaflight.diff`.
- **Line endings and drive paths**: a CRLF `betaflight.diff`, `eeprom.bin` written by a Cygwin-linked binary read by the Windows simulator, a Cygwin path (`/cygdrive/c/...`) vs a Windows path in `--config` and the working directory.
- **Timer granularity**: Windows sleeps and condition waits round to ~1 to 15 ms; the 1000 Hz lockstep exchange and external-time scheduler may run far slower than real time or hang. Measure with the benchmark, do not assume.
- **A missing runtime DLL** (`msys-2.0.dll`, `libwinpthread-1.dll`) when the simulator launches the exe from a normal shell or from the Godot game: the exe must either be self-contained or found with its DLL directory on PATH.

---

### Task 1: Install MSYS2 and record the toolchain

**Files:**
- Create: `docs/research/sitl-native-windows.md` (heading, "Toolchain" section)

**Interfaces:**
- Produces: `C:\msys64` with `gcc`, `make`, `git`, `patch`, `diffutils`, `python` for the `msys` environment and `mingw-w64-ucrt-x86_64-gcc` for `ucrt64`; the findings document that Tasks 2 to 5 append to.

- [ ] **Step 1: Install.** `winget install --id MSYS2.MSYS2 -e --silent --accept-package-agreements --accept-source-agreements`, then `C:\msys64\usr\bin\bash.exe -lc "pacman -Syu --noconfirm"` (run twice; the first updates the core and may close the shell), then `pacman -S --noconfirm --needed base-devel git patch diffutils python gcc make mingw-w64-ucrt-x86_64-gcc mingw-w64-ucrt-x86_64-make`.
- [ ] **Step 2: Verify.** Expected: `C:\msys64\usr\bin\bash.exe -lc "gcc --version && make --version && uname -s"` prints GCC, GNU Make and `MSYS_NT-...`; `C:\msys64\ucrt64\bin\gcc.exe --version` prints GCC.
- [ ] **Step 3: Write the "Toolchain" section** of the findings document: package versions, install size, the `uname` values the Betaflight Makefile sees (it maps `MSYS%` to `MINGW := 1`, `CYGWIN%` to `CYGWIN := 1`).
- [ ] **Step 4: Commit** (`docs: native Windows SITL findings, toolchain (#67)`).

### Task 2: Build the SITL natively, stock first, and record every error (Stage A)

**Files:**
- Modify: `docs/research/sitl-native-windows.md` ("Build attempts" section)
- Scratch (not committed): `C:\ofs-win\bf-msys\`, `C:\ofs-win\bf-ucrt\` (clones of the pinned tag)

**Interfaces:**
- Consumes: Task 1's toolchain.
- Produces: a verdict per environment (`msys`, `ucrt64`): does the stock SITL build, does it start (`--help`, then `--config` of a diff)? Stage B only starts if at least one environment builds and starts.

- [ ] **Step 1: Clone** the pinned tag into each scratch directory (`git clone --branch 2026.6.2 --depth 1`, `git submodule update --init --recursive --depth 1`), check `git rev-parse HEAD` against the pinned SHA.
- [ ] **Step 2: Build stock in `msys`**: `make TARGET=SITL -j$(nproc)` from an MSYS2 `msys` login shell (the flags of `build-sitl.sh` without the patch: stock needs none). Capture the log to `C:\ofs-win\bf-msys\build.log`. Expected on the first run: it fails or succeeds; either way record the first 10 distinct errors with file:line in the document (sockets, `fcntl`, `clock_gettime`, linker script, the `exe` output name).
- [ ] **Step 3: Same in `ucrt64`** (`C:\msys64\ucrt64\bin` first on PATH, `mingw32-make` or MSYS `make`).
- [ ] **Step 4: Try to start what built**: `betaflight_SITL.exe --help` exits 0; from a plain PowerShell (not an MSYS shell) to prove the DLL question.
- [ ] **Step 5: Gate A.** Neither environment builds: skip to Task 5 and write the blocker analysis with an effort estimate per blocker. Otherwise continue.
- [ ] **Step 6: Commit** the document.

### Task 3: A Windows patch and build script for the patched SITL (Stage B)

**Files:**
- Create: `third_party/betaflight/ofs-sitl-windows.patch` (applied after `ofs-sitl.patch`), `scripts/build-sitl-windows.sh` (run from an MSYS2 shell; mirrors `build-sitl.sh`: clone, pin check, both patches, build clean)
- Modify: `docs/research/sitl-native-windows.md`

**Interfaces:**
- Consumes: Task 2's working environment and its list of errors.
- Produces: `C:\ofs-win\betaflight\obj\main\betaflight_SITL.exe` built from tag + `ofs-sitl.patch` + `ofs-sitl-windows.patch`; the Linux build from `ofs-sitl.patch` alone is unchanged (the Windows patch only touches what compiles on Windows, behind `#ifdef _WIN32 / __CYGWIN__`).

- [ ] **Step 1: Apply `ofs-sitl.patch`** to the working clone; note which hunks conflict with a Windows source (none expected).
- [ ] **Step 2: Port until it links**, smallest change first, each change recorded in the document with the error it fixes: socket and `fcntl` calls (`udplink.c`, `serial_tcp.c`, `sitl.c`), `SO_REUSEADDR` semantics (on Windows it allows a second bind to the same port: the opposite of Linux, so the stale-SITL bind-failure check does not fire; record how the slot lock behaves), the timer, `pthread` condition waits on `CLOCK_MONOTONIC`.
- [ ] **Step 3: Regenerate** `ofs-sitl-windows.patch` with `git diff` against the state after `ofs-sitl.patch` (LF line endings; check the patch applies with `git apply --check` in a fresh WSL clone after `ofs-sitl.patch`, and that `scripts/build-sitl.sh` in WSL still builds and the existing Linux live tests pass).
- [ ] **Step 4: Write `scripts/build-sitl-windows.sh`** and run it from an MSYS2 shell on a clean directory: it must end with the exe listed.
- [ ] **Step 5: Commit** (`feat(sitl): Windows port of the SITL patch and build script (#67)`).

### Task 4: Fly it from the simulator on Windows (Stage B)

**Files:**
- Modify: `crates/ofs-fc/src/sitl/net.rs` (`default_cleanup`, a Windows branch), `crates/ofs-fc/src/sitl/process.rs` (`await_cleanup` for Windows), `crates/ofs-fc/tests/sitl_net.rs`
- Create: `crates/ofs-fc/tests/sitl_windows.rs` (Windows-only decoy tests: two fake `betaflight_SITL.exe` processes in two slots, a cleanup for one leaves the other)
- Modify: `crates/ofs-sim/tests/sitl_live.rs` or `python/tests/test_sitl_parallel.py` as needed to run with a native exe.

**Interfaces:**
- Consumes: Task 3's exe; M9a's `net::port_offset_in`, `net::is_victim(args, offset)`, `net::default_cleanup(launch) -> Vec<String>`, `process::await_cleanup(launch) -> AwaitCleanup`.
- Produces: on Windows, `default_cleanup` returns a PowerShell argv that terminates the processes named `betaflight_SITL.exe` whose command line carries the slot's `--port-offset` (or none for slot 0), the offset passed as an argument, never part of the script text; `await_cleanup` waits for them on Windows as it does on Linux and reports the same `AwaitCleanup`.

- [ ] **Step 1: Run the lockstep exchange natively.** `OFS_SITL_LAUNCH=C:\ofs-win\betaflight\obj\main\betaflight_SITL.exe` with `OFS_SITL_CLEANUP` set to a plain `taskkill /F /IM betaflight_SITL.exe`: `cargo test -p ofs-fc --test sitl_live -- --ignored --test-threads=1`. Record which tests pass and which fail and why (the timer finding goes in the document with numbers).
- [ ] **Step 2: Write the failing tests** (Windows only): `a_cleanup_for_one_slot_leaves_another_slots_sitl_alive_on_windows` and `a_slots_sitl_is_found_when_its_path_has_spaces_on_windows`, using a copy of a tiny console exe under the SITL name (a Rust test helper binary or `ping`/`timeout` copied to `betaflight_SITL.exe`) launched with `--port-offset N`. Run them: they fail (`default_cleanup` returns an empty argv).
- [ ] **Step 3: Implement** `net::default_cleanup` for non-WSL, non-unix hosts and the Windows wait in `process::await_cleanup` with `powershell.exe -NoProfile -Command` and `Get-CimInstance Win32_Process` filtered by name and command line, the offset an argument (`$args[0]`). Keep `is_victim` the single rule for what a slot's command line means.
- [ ] **Step 4: Run** the new tests (green), `cargo test --workspace --locked`, clippy, `cargo +1.85 check` on `ofs-fc`, and `python -m pytest python/tests/test_sitl_parallel.py` with the native exe (two sessions side by side, one hard-killed).
- [ ] **Step 5: Benchmark** with the native exe (`scripts/benchmark.py`) against WSL and record real-time ratios in the document.
- [ ] **Step 6: Commit** (`feat(fc): slot-scoped SITL cleanup on Windows (#67)`).

### Task 5: Findings, decision and docs (Stage C)

**Files:**
- Modify: `docs/research/sitl-native-windows.md`, `docs/research/sitl-interface.md` (§6 "Native Windows build"), `docs/roadmap.md`, `docs/dev-setup.md`, `CHANGELOG.md`, `CONTRIBUTING.md` (only if a test command changes)

- [ ] **Step 1: Write the decision**: go / no-go for shipping native Windows as a supported path, what remains (CI on `windows-latest`, distributing the exe and its DLLs, the Godot game's launch setting, Defender prompts), and the estimated size of each remaining piece. If Stage B did not run, the blocker analysis replaces it.
- [ ] **Step 2: Update** the §6 paragraph of `sitl-interface.md` ("not attempted" is no longer true), the roadmap line for #67, the Windows part of `docs/dev-setup.md` (native option with the build script, or why not), and the changelog `Unreleased` section.
- [ ] **Step 3: Check** `python -m pytest python/tests/test_release.py` and the doc links; commit (`docs: native Windows SITL findings and decision (#67)`).
