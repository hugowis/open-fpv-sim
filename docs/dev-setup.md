# Developer setup

## Requirements
- Rust stable ≥ 1.80 (`rustup`), Python ≥ 3.10.
- Betaflight SITL: Linux, or WSL2 (Ubuntu) on Windows. Build with `bash scripts/build-sitl.sh` (see the script for env options).

## Everyday commands
| What | Command |
|---|---|
| All Rust tests | `cargo test --workspace` |
| Live SITL bridge test | `OFS_SITL_LAUNCH=<cmd> cargo test -p ofs-fc --test sitl_live -- --ignored` |
| Server | `cargo run -p ofs-sim -- --listen 127.0.0.1:50051 --data-dir .ofs-data` |
| Python tests | `cargo build -p ofs-sim && python -m pytest python/tests -v` |
| Regenerate Python stubs | `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto` |

## Environment variables
- `OFS_SIM_BIN` — path to `ofs-sim` used by `ofs.launch()`.
- `OFS_SITL_LAUNCH` — SITL argv (space-separated), overrides `fc.launch` in quad files.
  Windows: `wsl.exe -d Ubuntu -e /home/<user>/ofs/betaflight/obj/main/betaflight_SITL.elf`. Works with WSL's default NAT networking: the bridge discovers the WSL VM and host IPs and passes `--ip` (see `docs/research/sitl-interface.md` §6).
- `OFS_SITL_CLEANUP` — argv run before launch and after stop to kill stray SITL processes. Under WSL it defaults to `<wsl prefix> pkill -x betaflight_SITL` (required: a stale SITL would otherwise answer instead of the new one).
- `OFS_SITL_HOST`, `OFS_SITL_REPLY_IP` — override the address state datagrams go to, and the address SITL replies to (`--ip`, motor socket bind).

## Firmware state
Each quad gets `<data_dir>/<quad file stem>/` holding `eeprom.bin`, `betaflight.diff` and `sitl.log`.
The diff is applied only on first boot; delete `eeprom.bin` to re-apply it. Betaflight Configurator can connect to `tcp://127.0.0.1:5761` while a session runs.

## Ports
SITL uses fixed ports (UDP 9001–9004, TCP 5760+), so only one SITL session can run per machine and SITL tests must not run in parallel.
