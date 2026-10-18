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
