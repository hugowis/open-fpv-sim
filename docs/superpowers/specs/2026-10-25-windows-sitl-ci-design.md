# CI for the native Windows SITL (#107) — design

The native Windows SITL (M9b, `scripts/build-sitl-windows.sh`, `third_party/betaflight/ofs-sitl-windows.patch`) is built by
nobody in CI, so a new pinned Betaflight tag or a change to the Linux patch can break it unnoticed.
`docs/research/sitl-native-windows.md` lists a CI job as the first thing needed before it is more than experimental.

## Goal
A `windows-sitl` job in `.github/workflows/ci.yml` that builds the SITL natively (MSYS2 `msys`), runs the live SITL tests
against the `.exe`, and tells us loudly when `win_stubs.c` no longer covers the link. It starts non-blocking.

## Non-goals
- The DLL-free `ucrt64` build (#105), shipping a prebuilt zip (a licensing decision), the Godot e2e against the native SITL
  (not in #107's "done when"; the research note still lists it as unchecked).
- Changing the `windows` job (Rust tests + Python tests without SITL): it stays the fast, required check.

## Decisions (my calls)
1. **Separate job `windows-sitl`, `continue-on-error: true`, not in `publish.needs`** — the macOS job is the precedent;
   promote after a clean record (about ten green runs).
2. **MSYS2 via `msys2/setup-msys2@v2`**, `msystem: MSYS`, `release: false` (the runner's preinstalled `C:\msys64`, no
   second download), `update: true` (the runner image's MSYS2 can lag the repos; M9b needed `pacman -Syu` too), packages
   `base-devel git patch diffutils python gcc make`. Only the build step uses the `msys2` shell; everything else is the
   runner's default PowerShell, which also proves the exe runs without MSYS2 on `PATH`.
3. **Build in `C:/ofs/betaflight`** (`BF_DIR`; Betaflight's make breaks on spaces, and one fixed path avoids the two
   path flavours of the workspace).
4. **Cache the three outputs** (`betaflight_SITL.exe`, `msys-2.0.dll`, `msys-gcc_s-seh-1.dll`), keyed on
   `hashFiles('scripts/build-sitl-windows.sh', 'third_party/betaflight/**')` as the Linux/macOS jobs do. On a hit, MSYS2
   is not even installed. Not in the key: the runner's MSYS2 package versions (the DLLs come from there); any patch change
   refreshes them, and an unused cache entry expires after 7 days.
5. **Tests**: the same four commands as the `sitl` job (`sitl_live` for `ofs-fc` and `ofs-sim`, `vtx_live`, the Python
   SITL files), `--test-threads=1`, `OFS_SITL_LAUNCH` = the `.exe`. The `ofs-sim` live tests that need `pkill` are
   Linux-only already. Plus the headless benchmark (release `ofs-sim`, `--floor 1`, no baseline yet, table in the job
   summary) so the native real-time factor is recorded on every run.
6. **Artifact**: the `.exe` and the two DLLs, `upload-artifact@v7`, 14 days, for a manual run.
7. **The stub check (`win_stubs.c`)**: a link with an undefined symbol already fails, but buried in make's output and
   dependent on the locale. `build-sitl-windows.sh` now runs make with `LC_ALL=C`, keeps the log, and on failure prints the
   undefined symbols, names `win_stubs.c`, and (under Actions) emits an `::error` annotation listing them. A stub that is
   *called* at run time already aborts with its name (`STUB_CALLED`), which the live tests surface.
8. **Docs**: `docs/dev-setup.md` ("nothing builds this in CI yet" goes) and `docs/research/sitl-native-windows.md` (a CI
   section; the duration / real-time factor / flakiness record reads "not yet measured" until runs exist). No CHANGELOG
   entry: not user-visible.

## What cannot be checked locally
The workflow itself (setup-msys2, the cache round trip, the runner's MSYS2) only runs on GitHub. Locally: the YAML parses,
the script runs in the local MSYS2 (`C:\msys64`) for a fresh clone and for a deliberately broken stub file, and the live
tests run against the resulting exe. The branch is not pushed until the user says so.
