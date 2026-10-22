# Audit follow-ups: the smaller fixes (2026-10-22)

From the whole-repo audit of 2026-10-22. The two major findings are issues #108 (replay runs a bundle's programs) and #109 (a launched Python `Sim` is never freed after `load()`); they are not part of this plan.

Each step is small, has its own check, and is independent of the others except where noted.

| # | Finding | Change | Check |
|---|---|---|---|
| 1 | Scratch files and the graph cache show up as untracked | `.gitignore`: `/graphify-out/`, `/.zcodeignore`, `/sticks-sites.txt` | `git status` no longer lists them |
| 2 | Two simulators sharing a data folder can pick the same log name | `RecordError::Taken`; `ofs_log::recorder::create_numbered` retries with the next number when `create_new` says the name exists. Used by the MCAP recording, the inputs CSV and the repro bundle folder | `ofs-log` test: a name taken between choosing and creating gives the next number |
| 3 | SITL output lines have no length cap | `pipe_lines` reads at most 64 KiB per line | test: 200 KB without a newline arrives in capped pieces |
| 4 | `launch()` picks a free port, releases it, starts the server later | `ofs-sim --port-file FILE`: the server binds first and writes the port it got; `launch()` starts it on `127.0.0.1:0` and reads the file | `python/tests/test_client.py` launch tests; server integration test |
| 5 | Python stubs can drift from `sim.proto` | a pytest that compiles `proto/ofs/v1/sim.proto` to a descriptor and compares it with the committed `sim_pb2` | the test passes now; it fails after a proto edit with no regeneration |
| 6 | The publish job uses moving action refs | pin its three actions to commit SHAs; add `.github/dependabot.yml` for `github-actions` | workflow YAML parses |
| 7 | `vehicle.rs` mixes jobs; `Load` mixes three | move `env_argv`, `split_argv`, `env_ip` to `ofs-fc/src/sitl/net.rs` with their test; move the mode and policy parsing out of `Load` | `cargo test --workspace`, no behaviour change |

Order: 1, 2, 3, 6, 5, 7, then 4 (it touches the server binary and the Python launcher). Run `cargo clippy --workspace --all-targets -- -D warnings` and the tests that need no Betaflight at the end.
