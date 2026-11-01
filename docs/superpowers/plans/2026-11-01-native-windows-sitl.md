# The Native Windows SITL `ucrt64` Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The pinned Betaflight SITL builds as one DLL-free native Win32 executable (MSYS2 `ucrt64`), passes the live batteries from a plain PowerShell, a whole seeded flight is compared with the Linux SITL and recorded, and CI covers it — closing #105; #107 closes first, on the evidence already on `main`.

**Architecture:** All port work lives in `third_party/betaflight/ofs-sitl-windows.patch` (Winsock for the socket layer, a small string-compat header, a static link) behind `_WIN32`/`__MINGW32__` fences, plus an `OFS_SITL_ENV=msys|ucrt64` mode in `scripts/build-sitl-windows.sh` whose `ucrt64` tail checks the import table instead of copying Cygwin DLLs. CI gains a second, non-blocking `windows-sitl-ucrt64` lane beside the gating `msys` job; one later commit promotes it and deletes the `msys` lane. A committed by-hand script (`scripts/compare-sitl-builds.py`) measures the ucrt64 build against the WSL Linux SITL on the same seeded flight. No Rust, Python-client, wire or world surface changes.

**Tech Stack:** MSYS2 `ucrt64` (mingw-w64 gcc, Winsock, static winpthreads), the Betaflight 2026.6.2 SITL target, GitHub Actions (`ci.yml`), Python 3.12 (`ofs` client, pytest).

**Spec:** `docs/superpowers/specs/2026-11-01-native-windows-sitl-design.md` (the design calls are binding; this plan argues from it). Umbrella: `docs/superpowers/plans/2026-11-01-platform-followups.md` (task 1 — read its "rules this wave respects" first). Sizing record: `docs/research/sitl-native-windows.md` §"The `ucrt64` (native Win32) environment" (:52-60).

## Global Constraints

- The Linux and `msys` builds change nothing: `third_party/betaflight/ofs-sitl.patch` is not touched; every `ofs-sitl-windows.patch` hunk is additive behind a fence (`defined(_WIN32)`, `defined(__MINGW32__)`, or `defined(__CYGWIN__) || defined(_WIN32)`); no unconditional line the `msys` build compiles changes.
- `cargo test --workspace --locked` and `cargo clippy --workspace --all-targets --locked -- -D warnings` stay green (nothing in the workspace changes; run once after the last code task); `PYTHONPATH=python python -m pytest python/tests -q` green (task 3 adds one SITL-free test file).
- `actionlint` on every `ci.yml` edit; `cargo fmt` is not a gate.
- The patches and scripts need LF endings (`.gitattributes` enforces it; keep it true for new files).
- Worktree-isolated sessions refuse heredocs and WSL through Bash: use Write/Edit for files, PowerShell for WSL, and for multi-line WSL commands write a script file in the scratchpad and run `wsl -d Ubuntu -- bash <path>`.
- Local shells (M9b's record): MSYS2 at `C:\msys64` ("MSYS2 MSYS" and "MSYS2 UCRT64" launchers), WSL Ubuntu for the Linux SITL. Betaflight checkouts live outside the repo (default `BF_DIR`; the script refuses a `BF_DIR` with spaces).
- Every number written into `docs/research/sitl-native-windows.md` comes from a run that happened, with its date and machine; nothing aspirational. "Re-verify at execution" points in the spec (the seven-file list, dyad's upstream `_WIN32` path, the static-flag channel, the Winsock bind semantics) are re-derived from the actual compile/run, not copied blindly.
- Commit trailers: none prescribed; a session that adds one names its own identity. One commit per task below (task 1 commits nothing).

## Review Focus

- The `msys` build quietly changed behaviour: fences must be additive-only, and the PR's `windows-sitl` run is the gate — its cache key hashes `third_party/betaflight/**`, so the patch change forces a real rebuild (task 2, step 8).
- A runtime DLL sneaks back in (a tag bump, a flag change): the `ucrt64` build's import-table check fails naming the DLL — prove it by dropping the static flag once locally (task 2, step 6).
- Winsock's bind/TIME_WAIT semantics differ from Cygwin's on the TCP listeners: back-to-back sessions (`test_sitl_parallel.py`) must pass against the ucrt64 exe; a failure there is a port bug, not a test to relax (task 2, step 7).
- A stale EEPROM or a first-boot diff masquerading as build divergence in the comparison: per-label data dirs and warm-up flights, the `test_sitl_hover.py` pattern (task 3, step 3).
- The research doc's determinism item 5 gets a number nobody measured: the comparison's maxes, first-divergence index and digests come from the script's output pasted as-is (task 3, step 5).

---

### Task 1: Close #107 with the evidence

The umbrella's ruling (`docs/superpowers/plans/2026-11-01-platform-followups.md`, tasks 1 and the dispositions): #107's done-when is satisfied on `main`; the pair's plan carries the close evidence. Nothing in #105 is a precondition. Run this task first, as soon as the wave's planning PR has merged.

**Files:** none (a GitHub comment; no commit).

**Interfaces:** none.

- [ ] **Step 1: Re-verify the five lines on `main` at execution time** (the anchors below were verified against `fdc4917` on 2026-11-01; line numbers drift, the SHAs do not). For each bullet: `gh issue view 107 --repo hugowis/open-fpv-sim` for the text; `git grep -n` / `git log` on `main` for the evidence.

| #107's done-when bullet | Satisfaction on `main` (verified 2026-11-01) |
|---|---|
| `windows-latest` job with `msys2/setup-msys2`, runs the build script, caches, sensible time | `.github/workflows/ci.yml`: the `windows-sitl` job (`:178`), `msys2/setup-msys2@v2` `msystem: MSYS` (`:202-208`), the cache of the exe + both DLLs keyed `hashFiles('scripts/build-sitl-windows.sh', 'third_party/betaflight/**')` (`:193-201`); durations recorded 14 m 42 s cold / 9 m 15 s cache hit (`docs/research/sitl-native-windows.md:156-157`). Landed `d71d001` (#145) |
| Live tests + Python SITL tests, one at a time, `OFS_SITL_LAUNCH` = the `.exe` | `ci.yml:183` (`OFS_SITL_LAUNCH` env), `:222-226` (`sitl_live` for `ofs-fc`+`ofs-sim` and `vtx_live`, `--test-threads=1`; the ten Python SITL files, `test_sitl_hover.py` included). `d71d001` |
| Non-blocking start, promoted into `publish.needs` after a clean record | Landed `continue-on-error` (`d71d001`); promoted 2026-10-31 after 13 consecutive green runs — `b37aae5` (#167; branch commit `e28006d`) — and `publish.needs: [core, windows, sitl, windows-sitl]` (`ci.yml:237`) |
| Binary + two Cygwin DLLs uploaded as artifact; duration/RTF/flaky recorded | `upload-artifact` `betaflight-sitl-windows`, exe + `msys-2.0.dll` + `msys-gcc_s-seh-1.dll`, 14 days (`ci.yml:213-221`); the record table and the promotion paragraph (`sitl-native-windows.md:152-166`), `cb5882c` (#148) |
| `win_stubs.c` checked; a new tag's undefined symbol fails the job named | `scripts/build-sitl-windows.sh:54-62`: `LC_ALL=C make` with a log, on failure the undefined symbols are listed, `win_stubs.c` named, `::error` annotation under Actions (`ci.yml:209`); verified by building with a stub-less `IORead` (`sitl-native-windows.md:147-150`); a *called* stub aborts with its name. `b8bae20`, squashed into `d71d001` |

- [ ] **Step 2: Close the issue** with `gh issue close 107 --repo hugowis/open-fpv-sim --comment "..."`, the comment body (adapt the anchors to what step 1 found):

> Closing on the evidence — all five done-when lines are satisfied on `main` (verified 2026-11-01 at `fdc4917`):
> 1. The `windows-sitl` job (windows-latest, `msys2/setup-msys2`, cached on the build script + `third_party/betaflight/**`) builds and live-tests it; 14 m 42 s cold, 9 m 15 s cached — landed in #145 (`d71d001`).
> 2. It runs the `sitl_live` (ofs-fc, ofs-sim) and `vtx_live` suites `--test-threads=1` and the ten Python SITL files against the `.exe` via `OFS_SITL_LAUNCH`, from the runner's PowerShell.
> 3. It started non-blocking and was promoted into `publish.needs` on 2026-10-31 after 13 consecutive green runs (#167, `b37aae5`); the macOS job is the one still waiting on a record (#118).
> 4. The exe and its two Cygwin DLLs are the `betaflight-sitl-windows` artifact (14 days); duration, real-time factor and flakiness are recorded in `docs/research/sitl-native-windows.md` (#148, `cb5882c`).
> 5. `win_stubs.c` is guarded: a link the stub file does not cover fails with the symbols named (`scripts/build-sitl-windows.sh`, tested with a stub-less `IORead`).
>
> The follow-ups this issue's research note lists — the DLL-free `ucrt64` build, distribution/licensing, the Godot check — live on in #105 (the `ucrt64` port) and #163 (the by-hand checklist).

### Task 2: The port — the script's `ucrt64` mode and the patch

The core. The failing test already exists and is documented: `make -k` in `ucrt64` fails in seven files (`docs/research/sitl-native-windows.md:52-56`). This task makes it compile, link statically, and pass the batteries — with the `msys` and Linux builds provably untouched.

**Files:**
- Modify: `scripts/build-sitl-windows.sh` (the `OFS_SITL_ENV` mode, the environment guard `:25-28`, the `ucrt64` tail)
- Modify: `third_party/betaflight/ofs-sitl-windows.patch` (Winsock and compat hunks; new file `src/main/common/win_compat.h` inside the patch; `win_stubs.c` list growth if the ucrt64 link needs it)
- No repository source outside those two (the Betaflight checkout is where the patch's content is developed; only the patch is committed)

**Interfaces:**
- Produces: `OFS_SITL_ENV` (`msys` default | `ucrt64`) consumed by the script, CI (task 4) and the docs (task 5); `scripts/build-sitl-windows.sh` in `ucrt64` mode emits `<BF_DIR>/obj/main/betaflight_SITL.exe` with **no** DLL copies and fails (exit 1, symbols/DLLs named) if the link is not DLL-free; the patch applies cleanly on top of `ofs-sitl.patch` at the pinned tag in both environments.

- [ ] **Step 1: Add the script mode first.** In `scripts/build-sitl-windows.sh`: read `OFS_SITL_ENV="${OFS_SITL_ENV:-msys}"`; guard `msys` mode to `MSYS_NT*|CYGWIN_NT*` and `ucrt64` mode to `MINGW64_NT*` (either mismatch: "error: run this from an MSYS2 <msys|UCRT64> shell, or set OFS_SITL_ENV accordingly"); refuse unknown values. In `ucrt64` mode: pass the static-link make variable (step 4), and replace the DLL discovery/copy (`:64-68`) with the import-table check (step 6). Keep `msys` mode byte-identical in behaviour.
- [ ] **Step 2: Baseline the compile in a scratch checkout.** `BF_DIR=/c/ofs-win/ucrt bash scripts/build-sitl-windows.sh` from the UCRT64 shell — expected: the link fails after the compile errors. Record the failing files; they must be the research doc's seven (`udplink.c`, `serial_tcp.c`, `serial_tcp.h`, `cli.c`, `runtime_config.c`, `core.c`, `olc.c`) or the difference is written into the doc in task 5 (gcc/tag drift).
- [ ] **Step 3: Write the Winsock hunks** (in a working Betaflight checkout, then regenerated into the patch — `git diff > …` the way the existing patch was made): includes fenced `#if defined(_WIN32) && !defined(__CYGWIN__)` → `<winsock2.h>` + `<ws2tcpip.h>`, else the POSIX headers, in `udplink.c` and `serial_tcp.c`/`serial_tcp.h`; `closesocket` for `close`, `ioctlsocket(fd, FIONBIO, &one)` for the `fcntl` non-blocking set, one `socket_errno()` macro (`WSAGetLastError()` under the fence, `errno` otherwise) at `bindProbe`'s and the listeners' error reads; `WSAStartup` once at the top of `systemInit` in `sitl.c` (under the same fence; Winsock ref-counts, so dyad's own `_WIN32` init stays safe); extend the five `__CYGWIN__`-only behaviour fences (`dyad.c` ×2, `serial_tcp.c` ×2, `udplink.c` ×1 — AF_INET for wildcard listeners, loopback bind for the UDP servers, no `SO_REUSEADDR` on TCP listeners) to `defined(__CYGWIN__) || defined(_WIN32)`. Do **not** set `SO_EXCLUSIVEADDRUSE` (spec: it would worsen session churn; the parallel tests are the arbiter).
- [ ] **Step 4: Write the compat header and the static link.** New `src/main/common/win_compat.h` in the patch: `#ifdef __MINGW32__` … `static inline char *strsep(char **sp, const char *delim)`, `static inline char *strcasestr(const char *h, const char *n)`, `#define ffs(i) __builtin_ffs(i)` … `#endif`, included next to `<string.h>` in exactly the files step 2 named. The static link: add `-static` through the channel `SITL.mk` exposes (a make variable the windows patch adds — e.g. `OFS_WIN_STATIC=1` guarded so the `msys` build's link line is unchanged), passed by the script in `ucrt64` mode only.
- [ ] **Step 5: Iterate the build to green**, letting the script's existing stub check name any `win_stubs.c` additions (a different codegen can grow the list — add symbols to the patch's `win_stubs.c`, keeping the `STUB_CALLED` shape). LTO stays off (spec: match the `msys` flags; `OPTIMISATION_BASE="-ffast-math -fmerge-all-constants"`).
- [ ] **Step 6: The import-table check.** In `ucrt64` mode the script runs `objdump -p obj/main/betaflight_SITL.exe | sed -n "s/^\s*DLL Name: //p"` and fails if any name is not an OS component (`kernel32.dll`, `ws2_32.dll`, `ucrtbase.dll`, `api-ms-win-crt-*`, `ntdll.dll` and kin; the allow-list is written in the script with a comment). Prove it: build once without `-static` — the check must fail naming `libgcc_s_seh-1.dll` (or `libwinpthread-1.dll`); then with `-static` it passes.
- [ ] **Step 7: Run the batteries against the ucrt64 exe from a plain PowerShell** (no MSYS2 on `PATH`; `$env:OFS_SITL_LAUNCH` to the exe): `cargo test -p ofs-fc -p ofs-sim --test sitl_live --locked -- --ignored --test-threads=1`, `cargo test -p ofs-video --test vtx_live --locked -- --ignored --test-threads=1`, `cargo build -p ofs-sim --locked` then `python -m pytest python/tests/test_sitl_hover.py python/tests/test_sitl_link.py python/tests/test_sitl_radio.py python/tests/test_sitl_telemetry.py python/tests/test_sitl_tramp.py python/tests/test_sitl_video.py python/tests/test_sitl_faults.py python/tests/test_sitl_sandbox.py python/tests/test_sitl_circuit.py python/tests/test_sitl_parallel.py -v` — the CI list. Expected: all green (the `msys` build's record is the baseline; a bind/TIME_WAIT failure in `test_sitl_parallel.py` is a port bug — spec's Review Focus).
- [ ] **Step 8: Prove `msys` untouched.** Rebuild in `msys` mode against the same patch (`OFS_SITL_ENV=msys` — default — from the MSYS shell) and run `cargo test -p ofs-fc --test sitl_live --locked -- --ignored --test-threads=1`; the patch hunks it compiles are the old ones plus new fences it cannot see. Verify by reading the patch: no unconditional line outside a fence changed. The Linux build needs no local proof (`ofs-sitl.patch` untouched; the `sitl` CI job builds it per run).
- [ ] **Step 9: Commit** `feat(sitl): the DLL-free ucrt64 (native Win32) build (#105)` — the script and the patch only; the docs ride task 5.

### Task 3: The cross-build comparison — script, measurement, record

`docs/research/sitl-native-windows.md`'s item 5 (":132-134") asks whether whole runs agree and whether any test needs a per-OS tolerance. The M9b probe was one second; #105 asks for a whole flight.

**Files:**
- Create: `scripts/compare-sitl-builds.py`
- Test: `python/tests/test_compare_sitl.py` (SITL-free: the differ over synthetic state lists)
- Modify: `docs/research/sitl-native-windows.md` (the comparison section; the numbers from step 5)

**Interfaces:**
- Consumes: `python/examples/hover.py`'s `fly_hover(sim, target_alt=1.0, seconds=…)` (imported via `importlib` the way `python/tests/test_sitl_hover.py:15-19` does), the `ofs.launch()`/`sim.load(quad, seed=…)`/`sim.run`/`sim.state()` API, `sim.log.path` for the recording.
- Produces: `compare_flights(a: list, b: list, tol: float = 1e-3) -> dict` (pure: `{"steps": n, "max_d_motor": [f32×4], "max_d_alt_m": f, "max_d_euler_deg": f, "final_pose_dist_m": f, "first_divergence": int|None}`); `main(argv)` with `--label NAME --launch CMD` (repeatable, ≥2), `--quad` (default `quads/opendrone-5f-freestyle.toml`), `--seed` (default 1), `--seconds` (default 30), `--out` (default `build/sitl-compare`); flights per label: a feedback hover (`fly_hover`, `seconds=--seconds`) and an open-loop hold (boot 4 s still, arm+angle aux, `throttle=0.55` for `--seconds`, `sim.run(0.02)` steps); each label gets its own data dir and one warm-up hover first (its build applies its own `betaflight.diff` on its own first boot); the report prints a per-pair table and writes `<out>/summary.md`.

- [ ] **Step 1: Write the failing test** in `python/tests/test_compare_sitl.py`: two identical 3-state synthetic flights → all maxes 0.0, `first_divergence` `None`; a known perturbation (motor 2 +0.01 at index 1, altitude +0.05 at index 2) → `max_d_motor[2] == 0.01`, `max_d_alt_m == 0.05`, `first_divergence == 1` at `tol=1e-3`; unequal lengths → `ValueError` (lockstep means equal counts; anything else is a bug to see, not to pad). Import the differ by path (`importlib.util.spec_from_file_location("compare_sitl_builds", REPO / "scripts" / "compare-sitl-builds.py")`, the conftest `REPO` pattern).
- [ ] **Step 2: Run** `PYTHONPATH=python python -m pytest python/tests/test_compare_sitl.py -q` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement the script** per the Interfaces block: set `os.environ["OFS_SITL_LAUNCH"]` per label before each `ofs.launch(...)`; record per state `time_s`, `motor_cmd` (4), `altitude_m`, `euler_deg()`, position for the final pose distance; digest `hashlib.sha256(pathlib.Path(sim.log.path).read_bytes()).hexdigest()` per flight, printed and diffed (expected unequal across builds — that is a finding, not a failure; report-only, exit 0 unless a flight fails to arm).
- [ ] **Step 4: Run** the test — Expected: PASS. Then `PYTHONPATH=python python -m pytest python/tests -q` — Expected: PASS (nothing else changed).
- [ ] **Step 5: Measure.** Build the Linux SITL in WSL (`wsl -d Ubuntu -- bash scripts/build-sitl.sh` from the repo, or the dev-setup WSL recipe) and run, from Windows against one `ofs-sim` build: `python scripts/compare-sitl-builds.py --label ucrt64 --launch C:\ofs-win\ucrt\obj\main\betaflight_SITL.exe --label linux-wsl --launch "wsl.exe -d Ubuntu -e /home/<user>/ofs/betaflight/obj/main/betaflight_SITL.elf" --seconds 30`. Paste the table into `docs/research/sitl-native-windows.md` as a new section ("A whole flight against the Linux build"), with the digests, the date, the machine and the exact command; answer item 5 in the decision list by naming the tests that already carry the native tolerance (`test_per_motor_erpm_reaches_betaflight`'s resampling, the pumping MSP readers `msp_status.py`/`test_sitl_telemetry.py`) or the new ones the numbers demand.
- [ ] **Step 6: Commit** `test(sitl): the ucrt64-vs-Linux seeded-flight comparison (#105)`.

### Task 4: The `windows-sitl-ucrt64` CI lane (non-blocking)

Spec call 1: the lane starts beside the gating `msys` job; `publish.needs` is untouched (promotion is task 6, on a record).

**Files:**
- Modify: `.github/workflows/ci.yml` (a new job after `windows-sitl`, before `publish`; `publish.needs` unchanged)

**Interfaces:**
- Consumes: task 2's `OFS_SITL_ENV=ucrt64` script mode.
- Produces: the `windows-sitl-ucrt64` job (`continue-on-error: true`), the `betaflight-sitl-windows-ucrt64` artifact (the exe only), the cache key `sitl-${{ runner.os }}-ucrt64-${{ hashFiles('scripts/build-sitl-windows.sh', 'third_party/betaflight/**') }}`.

- [ ] **Step 1: Write the job** mirroring `windows-sitl` (`ci.yml:178-230`) with these deltas only: `env` adds `OFS_SITL_ENV: ucrt64`; `continue-on-error: true`; `setup-msys2` with `msystem: UCRT64` and `install: base-devel git patch diffutils python mingw-w64-ucrt-x86_64-gcc mingw-w64-ucrt-x86_64-make` (M9b's package list); the cache path and artifact path are the exe alone (no DLLs); cache key and artifact name per the Interfaces block; the test and benchmark steps identical (`:222-230`). A comment block above the job in the house voice: what it builds, that it is non-blocking pending a record (the `windows-sitl` promotion is the precedent), and that promotion deletes the `msys` lane in the same commit.
- [ ] **Step 2: Check** `actionlint .github/workflows/ci.yml` — clean — and that `publish.needs` is byte-identical to before.
- [ ] **Step 3: Commit** `ci: build and live-test the ucrt64 SITL (non-blocking lane) (#105)`; the PR's own runs of both lanes are the proof (the `msys` lane's cache key changed with the patch — it rebuilds and re-passes; the `ucrt64` lane's first runs are its record's start).

### Task 5: Docs truth and the changelog

#105's fourth bullet: the research doc, `docs/dev-setup.md` and the roadmap say which build is supported.

**Files:**
- Modify: `docs/dev-setup.md` ("Native Windows SITL (experimental, no WSL)", `:385-412`), `docs/roadmap.md` (`:76-82`), `docs/research/sitl-native-windows.md` (the toolchain table `:13-16`, the `ucrt64` section `:52-60`, the decision list `:124-135`), `CHANGELOG.md` ("Unreleased")

**Interfaces:** none.

- [ ] **Step 1: dev-setup.** Lead the native section with the `ucrt64` build (an "MSYS2 UCRT64" shell, `pacman -S --needed base-devel git patch diffutils python mingw-w64-ucrt-x86_64-gcc mingw-w64-ucrt-x86_64-make`, `OFS_SITL_ENV=ucrt64 bash scripts/build-sitl-windows.sh`, the PowerShell line unchanged, **no DLLs to copy**); keep the `msys` steps as the fallback (one flag difference, its DLL copy); name CI (`windows-sitl-ucrt64`, non-blocking) and the firewall note (unchanged: the TCP listeners stay on the wildcard; answer Cancel).
- [ ] **Step 2: roadmap.** Rewrite the `:76-82` paragraph from what is true: the native build has two environments — `ucrt64` (DLL-free, the supported native build, CI lane non-blocking pending a record) and `msys` (the fallback, gating since 2026-10-31) — and the whole-flight comparison's one-line verdict with a pointer to the research doc; drop the "sized at several days and not started (#105)" sentence.
- [ ] **Step 3: research doc.** The toolchain table's `ucrt64` row moves from "not ported" to what shipped (compiler, static winpthreads, the import-table check); the `ucrt64` section records what each of the seven files needed (or why the list differed), the static-link channel, the stub deltas, and pointers to task 3's comparison section and the CI lane; the decision list's items 5 and 6 are marked answered (the numbers; the port).
- [ ] **Step 4: CHANGELOG "Unreleased"**, one Added line: the DLL-free native Windows SITL build (`ucrt64`; `OFS_SITL_ENV=ucrt64`), the cross-build comparison script, and the non-blocking CI lane.
- [ ] **Step 5: Run the gates** (`cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `PYTHONPATH=python python -m pytest python/tests -q`) — Expected: PASS (no workspace change; proof of no drift).
- [ ] **Step 6: Commit** `docs: the ucrt64 native SITL - dev setup, roadmap and the research record (#105)`.

### Task 6: Promotion — `ucrt64` gates, `msys` lane deleted (evidence-gated; may run in a later session)

Spec call 1's second half. **Precondition:** at least 10 green runs of `windows-sitl-ucrt64` — the PR's branch runs count, as they did in B6's 13. If the record is not there when this task is reached, leave the recipe as a comment on #105's PR and stop; the task is the recipe, not a same-session promise.

**Files:**
- Modify: `.github/workflows/ci.yml` (delete the `windows-sitl` job `:175-230`; `windows-sitl-ucrt64` loses `continue-on-error` and its comment's "non-blocking"; `publish.needs` at `:237`: `windows-sitl` → `windows-sitl-ucrt64`)
- Modify: `scripts/build-sitl-windows.sh` (`OFS_SITL_ENV` default → `ucrt64`; the `msys` mode stays available as the fallback)
- Modify: `docs/dev-setup.md`, `docs/roadmap.md`, `docs/research/sitl-native-windows.md` (the supported-build sentences; the record table row: date, cold/cache-hit build, whole job, RTF, flaky tests; the promotion line with the green-run count)

**Interfaces:** none.

- [ ] **Step 1: Count the record**: the `windows-sitl-ucrt64` runs on the merged PR's branch plus `main` since it landed (`gh run list --workflow ci.yml --job …` or the runs page), all green, ≥10. Write the count into the commit message and the research doc's promotion line.
- [ ] **Step 2: Make the swap** per the Files block: one commit, `ci: the ucrt64 SITL gates publish; the msys lane steps down (#105)`. The default flip makes the docs' plain `bash scripts/build-sitl-windows.sh` copy-paste the supported build; `actionlint` clean; `publish.needs` still names four jobs.
- [ ] **Step 3: Watch the first `main` run**: cold cache (the script changed), then green. A failure here reverts by re-adding `OFS_SITL_ENV: msys` — the fallback is one flag away, which is the point of keeping it.

## Ledger

- 2026-11-01 (planning): the seven-file list, dyad's upstream `_WIN32` path, the `-static` channel and the Winsock bind semantics are cited from `docs/research/sitl-native-windows.md` and the current patch, not from a fresh ucrt64 compile — task 2 steps 2-5 re-derive each and any difference is recorded (task 5 step 3), not papered over.
