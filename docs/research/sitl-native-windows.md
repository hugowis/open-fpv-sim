# Betaflight SITL natively on Windows (no WSL), issue #67

Findings of the M9b spike (2026-10-18). Plan: `docs/superpowers/plans/2026-10-18-m9b-native-windows-sitl-spike.md`.
Background: `docs/research/sitl-interface.md` §6 (WSL 2 is the supported Windows path of v1) and the SITL patch
(`third_party/betaflight/ofs-sitl.patch`).

## Toolchain
Installed with `winget install --id MSYS2.MSYS2 -e` (MSYS2 installer 20260611, `C:\msys64`; about 1.1 GB for `/usr`
and 0.5 GB for `/ucrt64` after the packages below), then `pacman -Syu` twice (the first run closes the shell to
update the runtime; a retry was needed after a mirror download error) and
`pacman -S base-devel git patch diffutils python gcc make mingw-w64-ucrt-x86_64-gcc mingw-w64-ucrt-x86_64-make`.

| Environment | Compiler | What it builds | `uname -s` seen by Betaflight's `mk/system-id.mk` |
|---|---|---|---|
| `msys` (Cygwin-based POSIX layer) | gcc 15.3.0 | PE binaries that link `msys-2.0.dll`; POSIX sockets, `fcntl`, `pthread`, `clock_gettime` come from the layer | `MSYS_NT-10.0-26200`, so `OSFAMILY := windows`, `MINGW := 1` |
| `ucrt64` (native Win32, UCRT) | gcc 16.2.0 | PE binaries with no POSIX layer; sockets need Winsock, `fcntl` needs `ioctlsocket`; `winpthreads` supplies `pthread` | the `ucrt64` shell reports `MINGW64_NT-...` |

Betaflight's Makefile already knows these hosts (`mk/system-id.mk` maps `MINGW%`, `MSYS%`, `CYGWIN%` to
`OSFAMILY := windows`), and `src/platform/SIMULATOR/link/sitl.ld` already contains a Windows case (the PE section
must be page-aligned: `BLOCK(DEFINED(__section_alignment__) ? ...)`), so the parameter-group registry that the
SITL needs from the linker is meant to work on PE. The SITL sources themselves (`sitl.c`, `udplink.c`,
`drivers/serial_tcp.c`) have no `_WIN32` / `__CYGWIN__` case; only `lib/main/dyad/dyad.c` does.

## Build attempts
Scratch clones of the pinned tag (`2026.6.2`, `e0b7bb0`) under `C:\ofs-win`, built from an MSYS2 `msys` shell with
the flags of `scripts/build-sitl.sh`.

### The `msys` (Cygwin-based) environment builds and runs the SITL
The unmodified sources of the SITL target (`sitl.c`, `udplink.c`, `drivers/serial_tcp.c`, dyad) **compile as they are**:
the POSIX layer provides `pthread`, `clock_gettime`, `nanosleep`, BSD sockets and `fcntl`. What stopped the stock build
was the link and the run, in this order (each row is fixed in `third_party/betaflight/ofs-sitl-windows.patch`):

| # | Symptom | Cause | Fix |
|---|---|---|---|
| 1 | `ld: option « -z » non reconnue` | `SITL.mk` passes the ELF-only `-Wl,-z,noexecstack` | drop it |
| 2 | `lto1: internal compiler error: in get_token, at opts-common.cc:2175` at the link | gcc 15.3 LTO on the Cygwin target | build without LTO (`OPTIMISATION_BASE="-ffast-math -fmerge-all-constants"` on the make command line) |
| 3 | `ld: .fini_array not found for insert` | `sitl.ld` inserts the parameter-group registry `BEFORE .fini_array`, a section PE does not have | `INSERT AFTER .data` (the script already has its PE page-alignment case) |
| 4 | 11 to 16 undefined references (`IORead`, `dmaGetHandlerCount`, `hottTextmode*`, `audio*`, ...) | code that the Linux SITL link drops as unreachable stays in this link and refers to symbols no SITL object defines. `--gc-sections` removes **nothing** on this PE target (`--print-gc-sections` prints no line, with or without the linker script), and this build has no LTO (the Linux build has, which also drops unreferenced code). Which of the two matters is a hypothesis: a Linux link without LTO would tell | `src/platform/SIMULATOR/win_stubs.c` defines the symbols; a function stub that is ever called prints its name and aborts (none was in the runs below). This is the one brittle part of the port: the list depends on the build flags, and a Betaflight upgrade can add names |
| 5 | exit code 2048 (SIGFPE, `0xC0000094` integer divide by zero) while applying `serial 1 64 115200 57600 0 115200` | Betaflight's own `printf` (`tfp_format`) reads `%ld` as `unsigned long`; the CLI prints `uint32_t` baud rates with `%ld` (the MCU's `long` is 32 bits). The Windows x64 calling convention passes the 5th argument onward on the stack, and a 32-bit store leaves the upper half of the slot undefined, so `uli2a`'s `d *= base` overflows to 0 and divides by it. On Linux the arguments are in registers, which the compiler happens to zero-extend (the ABI does not promise it), and that hid the bug | `%ld` reads 32 bits under `__CYGWIN__` / `_WIN32` (`printf.c`): sign-extended for `d` (a negative `long` prints right), zero-extended for `u`, `x`, `X` |
| 6 | the TCP UARTs refuse `127.0.0.1` clients | dyad and `bindProbe` use `getaddrinfo` with `AF_UNSPEC` and the wildcard host, which gives an `AF_INET6` socket; on Windows it is IPv6-only (no dual stack by default), on Linux it also accepts IPv4 | `AF_INET` under `__CYGWIN__` (`dyad.c`, `serial_tcp.c`) |
| 7 | `a_held_serial_port_is_retried_then_refused` fails: a held port 5761 is not detected | with `SO_REUSEADDR` set (dyad and `bindProbe` do), a second socket could bind a port that is in use, the opposite of Linux, so the bind-failure check that stops a session talking to a stale SITL never fired. Without the option the bind is exclusive (observed; how Cygwin maps the option onto Winsock was not traced) | no `SO_REUSEADDR` on the TCP listeners under `__CYGWIN__` |
| 8 | (found by review, then measured) the listeners bind every interface | `AI_PASSIVE` and `INADDR_ANY`: Windows may show its firewall prompt for each new executable path, and "Allow" would expose Betaflight's unauthenticated MSP and CLI ports to the network | the UDP links bind loopback under `__CYGWIN__` (`udplink.c`). **The TCP listeners could not follow:** bound to `127.0.0.1` they failed with `errno 112` (address in use) when a session started right after another, because the closed session's connection stays in TIME_WAIT on `127.0.0.1:5761` and an exclusive bind to that address is refused, while a wildcard bind is not. The TCP ports therefore stay on the wildcard address; answer the firewall prompt with Cancel (loopback traffic is not filtered) |

Runtime: the executable imports `msys-2.0.dll` and `msys-gcc_s-seh-1.dll` (`objdump -p`); without them it exits with
`0xC0000135` and no message, from any shell that does not have `C:\msys64\usr\bin` on PATH. `scripts/build-sitl-windows.sh`
copies both next to `betaflight_SITL.exe` (about 3.5 MB), and the result runs from a plain PowerShell.

Result with `scripts/build-sitl-windows.sh` (a fresh clone, 3 to 4 minutes): `ofs-fc`'s `sitl_live` suite, started
from Windows with `OFS_SITL_LAUNCH=<exe>`, passes **7 of 7** (lockstep exchange, CRSF in, DisplayPort and ESC
telemetry out, the shifted ports of `--port-offset`, the held-port refusal).

### The `ucrt64` (native Win32) environment: not ported, sized instead
Compiling the patched sources with the UCRT gcc 16.2 (`make -k`, nothing else changed) fails in 7 files:
`udplink.c`, `serial_tcp.c` and `serial_tcp.h` need `sys/socket.h` and `netinet/in.h` (Winsock instead: `WSAStartup`,
`closesocket`, `ioctlsocket` for the non-blocking flag, `SO_EXCLUSIVEADDRUSE`), and `cli.c`, `runtime_config.c`,
`core.c` and `olc.c` use `strsep`, `strcasestr` and `ffs`, which the UCRT lacks (a small compat header). Past the compile
come the same link and ABI questions as above (section order, garbage collection, the `%ld` bug applies to this x64
ABI as well), plus the Winsock semantics of the dyad event loop (`select` on sockets only) and of `SO_REUSEADDR`.
The gain is an executable with no Cygwin runtime DLL (about 3.5 MB less to ship). It is a multi-day port that touches
Betaflight's own sources; it was not attempted once the `msys` build worked.

## Running it from the simulator
The simulator needs no new code to launch a native Windows SITL (`fc.launch` / `OFS_SITL_LAUNCH` is the executable, the
network is loopback, `--port-offset` and the slot logic of M9a apply unchanged). Two things were added:

- **A slot-scoped cleanup on a plain Windows host** (`net::default_cleanup`, `net::WINDOWS_SLOT_KILL_SCRIPT`). Windows
  has no `pkill`, and a hard-killed simulator leaves its `betaflight_SITL.exe` running. The default cleanup is now a
  PowerShell command that terminates the `betaflight_SITL.exe` processes whose last `--port-offset` is the session's
  slot (none means slot 0, the rule of M9a's `is_victim`) and waits up to 2 s for them to be gone; the offset is an
  argument, never part of the script text. It returns at once when no `betaflight_SITL` process exists (the usual
  case: measured 196 ms per call, a cold `powershell.exe` start), asks WMI only for that process name with a 5 s
  operation timeout, ignores a process whose command line it cannot read, and `run_cleanup` gives up on any cleanup
  command after 10 s (`CLEANUP_TIMEOUT`) so a stuck WMI query cannot hang a load. Two calls per session. An
  unscoped launch kills by name with `taskkill` and does not wait (hand-made launches in the live tests only).
  `await_cleanup` reports nothing on Windows, because the wait is inside the command: the "waited for the cleanup's
  victims" diagnostic line is a Linux and WSL feature. Tests: `crates/ofs-fc/tests/sitl_windows.rs` (decoys: a
  copy of `cmd.exe` named `betaflight_SITL.exe`; slot isolation, no-option is slot 0, spaces in the path).
- **Hints**: a SITL that cannot load its Cygwin DLLs exits with `0xC0000135` and says nothing; the startup error now
  names the DLLs. The "SITL not found" hint on Windows names the native route as well as WSL.

Two Python test helpers read the MSP reply with a blocking `recv` after a fixed slice of simulated time. On Windows
the request reaches Betaflight's TCP thread a few milliseconds of wall time after the slice has run, so the reply never
came and the read timed out. They now read with a short timeout and keep pumping simulated time
(`python/tests/msp_status.py`, `test_sitl_telemetry.py`); the Rust `MspClient` and the video/Tramp helpers already did.

## Results (2026-10-18, Windows 11, MSYS2 20260611, Betaflight 2026.6.2 + both patches)
Everything below ran from Windows with `OFS_SITL_LAUNCH=C:\ofs-win\bf-fresh\obj\main\betaflight_SITL.exe`, started
from a PowerShell with no MSYS2 on PATH.

| Check | Result |
|---|---|
| `scripts/build-sitl-windows.sh` on a fresh clone | builds, 3 to 4 minutes |
| `cargo test -p ofs-fc --test sitl_live -- --ignored` | 7 of 7 |
| `cargo test -p ofs-sim --test sitl_live -- --ignored` | 9 of 9 (the other 5 are Linux-only: they kill SITL with `pkill`) |
| `cargo test -p ofs-video --test vtx_live -- --ignored` | 1 of 1 |
| `pytest python/tests -k sitl` (26 tests, the CI list plus the circuit and parallel files) | 26 passed, after one test fix (below) |
| Two sessions side by side (`test_sitl_parallel.py`, the slot tests) | pass; slots 0 and 1 (5761, 5771) |
| Hard-kill one simulator, start the next | the orphan in slot 0 is replaced, slot 1's SITL keeps flying, `fc_restarts` 0 |
| `scripts/benchmark.py --seconds 20`, release `ofs-sim` | native SITL 10.9x (recording) and 13.2x (no recording) real time; the same run through WSL 2: 5.1x and 5.2x |

The native SITL measured 2.1 to 2.5 times faster than the WSL one (one 20 s run per row, on one machine), and the
lockstep exchange shows no sign of the Windows timer granularity (the 1 kHz exchange is a socket round trip, not a
sleep).

**Same flight as the Linux SITL, at least for a second.** Hovering at half throttle for one simulated second after the
boot, the native and the WSL runs of the same seeded session agree to the printed precision on the motor commands
(0.501, 0.501, 0.501, 0.502), the motor rpm and the altitude (7.993 m); only the instant at which an MSP reply is
sampled differs (6.04 s against 6.02 s of simulated time), because the reply takes a few milliseconds of wall time
more on Windows. That is one probe, not a comparison of whole flights (the build differs: no LTO, another compiler and
libm, `-Ofast`).

One Python test needed a fix for that sampling difference: `test_per_motor_erpm_reaches_betaflight` asserted that a
single `MSP_MOTOR_TELEMETRY` sample shows two distinct rpm values among four motors that differ by a few rpm, with
Betaflight's value quantized in steps of about 14 rpm. Against the native SITL it failed in 5 runs out of 8 (never in 8
through WSL); it now samples again, up to 20 times, until the values differ (0 failures in 8 native runs).

Not checked: the Godot game against the native SITL, and a Windows CI job.

## Decision
**Go, as an experimental second path; WSL stays the supported one.** The question of #67 is answered: nothing in the
SITL target is a blocker. Sockets, `pthread`, the monotonic clock and `nanosleep` all come from the Cygwin-based
runtime of MSYS2, and the port items above are small (about 50 added lines in 6 files, and the 23-line stub file).

What has to happen before native Windows can be the default path (not done here):
1. **A CI job** that builds it (`windows-latest` with `msys2/setup-msys2`, about 4 minutes, cacheable on the two
   patches and the pinned SHA) and runs `sitl_live` plus the Python SITL files against it. Until then the port can rot
   when the patch or the pinned Betaflight changes.
2. **The stub file** (`win_stubs.c`): find why `--gc-sections` removes nothing on this PE target, or generate the list
   in the build script; a new Betaflight tag will add names.
3. **Distribution**: the executable and two DLLs are built by each user; shipping a prebuilt zip is a separate decision that
   needs a licence check (Betaflight is GPL-3.0; the Cygwin runtime has its own terms).
4. **The Godot game and `quads/*.toml`**: `fc.launch` is `betaflight_SITL.elf` in every quad file, so a native launch
   goes through `OFS_SITL_LAUNCH` (or a per-quad override), as WSL does today.
5. **Determinism**: compare whole native and Linux runs of the same seeded flight (one second agreed exactly, see
   above), and decide whether the golden-free SITL tests need a per-OS tolerance (the open-loop goldens are not
   affected).
6. **`ucrt64`**, if a DLL-free executable matters (the multi-day port above).
