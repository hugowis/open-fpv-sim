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
| 4 | 11 to 16 undefined references (`IORead`, `dmaGetHandlerCount`, `hottTextmode*`, `audio*`, ...) | `--gc-sections` removes **nothing** on this PE target (`--print-gc-sections` prints no line even without the linker script), so hardware-only code that the Linux link discards stays and refers to symbols no SITL object defines | `src/platform/SIMULATOR/win_stubs.c` defines them (never called). This is the one brittle part of the port: the list depends on the build flags, and a Betaflight upgrade can add names. Finding out why garbage collection does nothing on PE would replace the file |
| 5 | exit code 2048 (SIGFPE, `0xC0000094` integer divide by zero) while applying `serial 1 64 115200 57600 0 115200` | Betaflight's own `printf` (`tfp_format`) reads `%ld` as `unsigned long`; the CLI prints `uint32_t` baud rates with `%ld` (the MCU's `long` is 32 bits). The Windows x64 calling convention passes the 5th argument onward on the stack and a 32-bit store leaves the upper half undefined, so `uli2a`'s `d *= base` overflows to 0 and divides by it. The SysV ABI (Linux) zero-extends registers, which hid the bug | `%ld` reads 32 bits under `__CYGWIN__` / `_WIN32` (`printf.c`) |
| 6 | the TCP UARTs refuse `127.0.0.1` clients | dyad and `bindProbe` use `getaddrinfo` with `AF_UNSPEC` and the wildcard host, which gives an `AF_INET6` socket; on Windows it is IPv6-only (no dual stack by default), on Linux it also accepts IPv4 | `AF_INET` under `__CYGWIN__` (`dyad.c`, `serial_tcp.c`) |
| 7 | `a_held_serial_port_is_retried_then_refused` fails: a held port 5761 is not detected | on Windows `SO_REUSEADDR` lets a second socket bind a port that is in use (the opposite of Linux), so the bind-failure check that stops a session talking to a stale SITL never fires | no `SO_REUSEADDR` on the TCP listeners under `__CYGWIN__` |

Runtime: the executable imports `msys-2.0.dll` and `msys-gcc_s-seh-1.dll` (`objdump -p`); without them it exits with
`0xC0000135` and no message, from any shell that does not have `C:\msys64\usr\bin` on PATH. `scripts/build-sitl-windows.sh`
copies both next to `betaflight_SITL.exe` (about 3.5 MB), and the result runs from a plain PowerShell.

Result with `scripts/build-sitl-windows.sh` (a fresh clone, 3 to 4 minutes): `ofs-fc`'s `sitl_live` suite, started
from Windows with `OFS_SITL_LAUNCH=<exe>`, passes **7 of 7** (lockstep exchange, CRSF in, DisplayPort and ESC
telemetry out, the shifted ports of `--port-offset`, the held-port refusal).
