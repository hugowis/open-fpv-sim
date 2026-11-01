# Native Windows SITL: the DLL-free `ucrt64` build (#105) — design

The native Windows SITL of M9b builds in MSYS2's `msys` (Cygwin) environment and needs `msys-2.0.dll` and
`msys-gcc_s-seh-1.dll` beside the executable (about 3.5 MB). The `ucrt64` environment would produce a native Win32
executable with no POSIX layer and no runtime DLL; M9b sized that port — seven failing files, then the same link and
ABI questions the `msys` build hit — and stopped (`docs/research/sitl-native-windows.md`, "The `ucrt64` (native Win32)
environment: not ported, sized instead"). This spec rules the port's open decisions. The plan is
`docs/superpowers/plans/2026-11-01-native-windows-sitl.md`; its first task closes **#107** on the evidence already on
`main` (the wave umbrella's ruling, `docs/superpowers/plans/2026-11-01-platform-followups.md`, task 1).

## Goal
The pinned Betaflight SITL (2026.6.2, `e0b7bb0`) builds in the `ucrt64` environment as **one PE executable with no
runtime DLL beside it**, starts from a plain PowerShell with no MSYS2 directory on `PATH`, passes the same live
batteries the `msys` build passes (lockstep exchange, Configurator/MSP port, `sitl_live`, `vtx_live`, the Python SITL
files), a **whole seeded flight** is compared against the Linux SITL with the differences recorded, and
`scripts/build-sitl-windows.sh` produces it behind a flag while CI keeps a native lane green — with the Linux and `msys`
builds byte-for-byte untouched in behaviour (all new code behind `_WIN32`/`__MINGW32__` fences, so neither of the other
two builds even recompiles differently).

## Non-goals
- Displacing WSL as the supported Windows path of the simulator (M9b's decision stands; `ucrt64` becomes the supported
  *native* build, WSL stays the supported path — #105 asks the docs to say which build is supported, not to demote WSL).
- Shipping a prebuilt zip (the licence question, research doc item 3).
- LTO on the `ucrt64` link (gcc 15's `lto1` crashed on the `msys` link; the port matches the `msys` build's
  `OPTIMISATION_BASE` exactly, one variable fewer in the determinism story; trying LTO later is a free experiment once
  the lane is green — record it as not attempted).
- Binding the TCP UART listeners to loopback (would remove the firewall-prompt exposure): a Cygwin TIME_WAIT
  observation blocked it on `msys` (research doc row 8); Winsock may allow it, but the parallel-session tests must
  speak first. The port mirrors the `msys` decisions (wildcard TCP, loopback UDP); loopback TCP is a follow-up.
- The stub-list generation question (research doc item 2, why `--gc-sections` removes nothing on PE): `win_stubs.c`
  stays hand-grown and the build script's undefined-symbol check stays the guard; the `ucrt64` link may grow the list.
- The Godot game against the native SITL (still "not checked" in the research doc; the by-hand rows belong to #163).
- The Rust workspace, the Python client, the wire: untouched (the wave umbrella's rule — a `ucrt64` build is the same
  lockstep SITL or the differences are documented).

## The port's shape (what the seven files get)

From the sizing (`docs/research/sitl-native-windows.md:52-60`, verified there against the pinned tag with
`make -k`; the executor re-runs the compile and treats its error list as the map — a tag- or gcc-version drift can
move a file either way):

- **`src/platform/SIMULATOR/udplink.c`, `src/main/drivers/serial_tcp.c`, `serial_tcp.h`** — the POSIX socket layer.
  Winsock instead of `<sys/socket.h>`/`<netinet/in.h>`/`<netdb.h>`/`<arpa/inet.h>`: `<winsock2.h>` + `<ws2tcpip.h>`
  under `defined(_WIN32) && !defined(__CYGWIN__)`, the POSIX includes otherwise. `WSAStartup` called once at the top
  of `systemInit` (ref-counted, so dyad's own `_WIN32` init is safe whichever runs first); `closesocket` for `close`;
  `ioctlsocket(fd, FIONBIO, ...)` for the `fcntl` non-blocking flag; socket errno through one macro (Winsock sets
  `WSAGetLastError`, not `errno` — `bindProbe`'s retry loop only tests zero/non-zero, so the printed number differing
  per OS is fine and gets documented). The three behaviour fences the `msys` build added under `__CYGWIN__` — `AF_INET`
  for the wildcard listeners, loopback bind for the UDP servers, no `SO_REUSEADDR` on the TCP listeners — extend to
  `defined(__CYGWIN__) || defined(_WIN32)`: the reasons (an IPv6-only wildcard socket; a second bind allowed on a busy
  port) are Windows semantics, not Cygwin's. `SO_EXCLUSIVEADDRUSE` is deliberately **not** set (it would make the
  back-to-back session churn worse; `test_sitl_parallel.py` and the bind-retry loop are the evidence the chosen
  semantics hold — re-verify at execution).
- **`lib/main/dyad/dyad.c`** — the only SITL file with an upstream `_WIN32` case (research doc :22), so `select` on
  sockets, `closesocket` and its own non-blocking handling come with it. The patch's two `__CYGWIN__` fences in
  `dyad_listenEx` (AF_INET, no `SO_REUSEADDR`) extend to `_WIN32` like the others.
- **`src/main/cli/cli.c`, `src/main/runtime_config.c`, `src/main/core.c`, `src/main/osd/olc.c`** — `strsep`,
  `strcasestr` and `ffs`, which the UCRT lacks (Cygwin has all three, which is why the `msys` build never tripped). One
  small compat header in the patch (self-fenced `#ifdef __MINGW32__`: static-inline `strsep`/`strcasestr`, `ffs` as
  `__builtin_ffs`), included next to `<string.h>` in exactly the files the compile names.
- **Already PE-correct, no change expected:** `printf.c`'s `%ld` fix and `win_stubs.c` are fenced
  `defined(__CYGWIN__) || defined(_WIN32)` in `ofs-sitl-windows.patch` today (the `%ld` bug is this same x64 ABI);
  `sitl.ld`'s `INSERT AFTER .data` and the dropped `-z,noexecstack` are PE-generic. The `ucrt64` link may need **more
  stubs** (different codegen, different libm): the build script's symbol check names them and the patch's list grows.
- **The static link.** `mingw` gcc links `libgcc_s_seh-1.dll` and `libwinpthread-1.dll` by default; the `ucrt64` mode
  links `-static` so both go into the executable. The channel is whatever `SITL.mk` exposes to the script (a make
  variable the windows patch adds, fenced so the `msys` mode is unaffected) — the contract is the check, not the
  channel: the script runs `objdump -p` on the result and **fails** if the import table names anything but OS
  components (`kernel32.dll`, `ws2_32.dll`, `ucrtbase.dll` / `api-ms-win-crt-*`, and their kin). That check is the
  `ucrt64` lane's analogue of the stub check — "DLL-free" must fail loudly in CI if a flag or tag change reintroduces
  a runtime DLL.

## Design calls (my calls)

1. **CI: a second, non-blocking lane, then promote-and-replace in one commit.** A `windows-sitl-ucrt64` job
   (`continue-on-error: true`, not in `publish.needs`) runs beside the gating `windows-sitl` (`msys`) job; after a
   clean record (at least 10 green runs, branch runs counting as they did in B6's 13) one commit promotes it into
   `publish.needs` and **deletes the `msys` lane**, flipping `OFS_SITL_ENV`'s default. Rationale: the green record is
   the currency this house pays for gating (the #145→#167 promotion; the macOS job is the one still `continue-on-error`), so an unproven-on-
   runners build must not gate `publish` on day one — and the doubling is temporary and free (a public repository's
   Actions minutes), while a direct swap would orphan the record and a permanent second lane would tax every run to
   protect a build the promotion demotes to fallback. End state: one gating lane, one artifact.
2. **One script, one flag: `OFS_SITL_ENV=msys|ucrt64`** (default `msys` until the promotion flips it). The clone,
   pin-check, patch, submodule and stub-check logic is shared — a second script would drift. The mode changes the
   environment guard (`MSYS_NT*`/`CYGWIN_NT*` for `msys`; `MINGW64_NT*` for `ucrt64`, refused with a message naming
   the right shell), the make flags (the static-link variable), and the tail: `msys` copies the Cygwin DLLs, `ucrt64`
   runs the import-table check instead. Rationale: the issue allows either shape; one script keeps the two modes from
   diverging and makes the promotion a one-word change.
3. **One patch: `ofs-sitl-windows.patch` grows the `ucrt64` hunks; no third patch file.** The fences above are the
   whole difference between the environments inside the same sources; a stacked `ofs-sitl-ucrt64.patch` would turn one
   matrix (two environments × one patch) into a patch-combination matrix for no isolation gain, and the CI cache key
   already hashes `third_party/betaflight/**`. The `msys` build must keep building from the same file — that is what
   the `msys` lane proves on the promotion-eve record, and the fences are additive (no unconditional line the `msys`
   build compiles changes).
4. **The cross-build comparison runs by hand, once, from a committed script; it is not a CI gate.**
   `scripts/compare-sitl-builds.py` takes two or more labelled launch commands, flies the same seeded flight against
   each (one **30 s feedback hover**, `python/examples/hover.py`'s `fly_hover` imported the way `test_sitl_hover.py`
   does, and one **30 s open-loop hold** — fixed sticks, no feedback amplification, so the number measures the
   firmware+physics chain), and reports, per label pair: max `|Δmotor_cmd|` per motor, max `|Δaltitude_m|`, max
   `|Δeuler_deg|`, final pose distance, state count, first divergent index at 1e-3, and both recordings' SHA-256
   (`test_sitl_hover.py:44`'s pattern). Each label gets its own data dir and a warm-up flight (its build applies its
   own diff on first boot); the compared flights are the second of each. Expected outcome, recorded honestly: motor
   commands and pose agree to a small tolerance, the **digests differ** (different compiler and libm on the firmware
   side make cross-build byte equality a non-goal — same-build determinism is what
   `test_betaflight_sitl_hover_is_deterministic` gates, and it runs per-OS in both lanes), and the tests that already
   carry the native-Windows tolerance are named (the resampling `test_per_motor_erpm_reaches_betaflight`, the pumping
   MSP readers `msp_status.py`/`test_sitl_telemetry.py`). Rationale: the question is a one-time research question, not
   a recurring regression; in CI it would cost a WSL install and a Linux SITL build on the Windows runner plus the NAT
   dance to answer nothing the per-OS lanes don't already gate. The research doc records the command and the numbers,
   so anyone can re-run it.
5. **Docs say: WSL is the supported Windows path; `ucrt64` is the supported native build; `msys` is the fallback.**
   `docs/dev-setup.md`'s native section grows the `ucrt64` steps (a UCRT64 shell, `OFS_SITL_ENV=ucrt64`, no DLLs to
   copy) ahead of the `msys` ones; `docs/roadmap.md`'s paragraph (:76-82) and `docs/research/sitl-native-windows.md`'s
   decision list (item 6, :135) are rewritten from what is true. The by-hand plain-PowerShell and Configurator-port
   rows land in #163's checklist per the umbrella's Manual QA section — not duplicated here.
6. **#107 closes on this pair's first task, not at the port's end.** Its five done-when bullets are satisfied on
   `main` today (the plan's task 1 carries the line-by-line evidence and the SHAs); nothing in #105's port is a
   precondition, and the umbrella rules it closed with the evidence.

## What cannot be checked locally
The CI lane itself (the runner's `ucrt64` packages and their gcc version, the cache round trip, the two lanes'
interference-free coexistence on `ci.yml`) only runs on GitHub. Locally: the patch applies to a fresh clone of the
pinned tag and builds in the local MSYS2 (`C:\msys64`) in both modes; the import-table check fails on demand (drop the
static flag and it must name `libgcc_s_seh-1.dll`); the exe starts from a plain PowerShell and passes the live
batteries; the `msys` rebuild after the patch change passes the `ofs-fc` `sitl_live` suite; the comparison script runs
against the WSL Linux SITL on this machine. The promotion's ≥10-run record exists only after the lane has run on
`main` — task 6 of the plan is a recipe with an evidence gate, not a same-session promise.
