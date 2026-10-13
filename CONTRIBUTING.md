# Contributing

Thank you for helping. Setup for Windows and Linux (Rust, Python, Godot, Betaflight SITL, WSL) is in
`docs/dev-setup.md`; how the code fits together is in `docs/architecture.md`.

## Tests, from fastest to slowest
1. **Rust unit and integration tests:** `cargo test --workspace --locked`, and
   `cargo clippy --workspace --all-targets --locked -- -D warnings` (CI fails on any warning).
2. **Python, without firmware:** `cargo build -p ofs-sim`, `python -m pip install -e "python[dev]"`,
   `python -m pytest python/tests` (the tests that need Betaflight skip themselves).
3. **Live, with Betaflight SITL** (Linux or WSL; build it with `scripts/build-sitl.sh`): set `OFS_SITL_LAUNCH` to the
   SITL binary, then `cargo test -p ofs-fc -p ofs-sim --test sitl_live -- --ignored --test-threads=1` and the
   `python/tests/test_sitl_*.py` files. One SITL per machine: never run these in parallel.
4. **Godot:** `GODOT_BIN=<godot 4.7.2> bash scripts/run-godot-tests.sh unit` (or `e2e`, or `all`); the script builds
   what it needs.
5. **Performance:** `python scripts/benchmark.py --binary target/release/ofs-sim` (`docs/dev-setup.md`, Performance).

## CI (`.github/workflows/ci.yml`)
| Job | Runs | Gates |
|---|---|---|
| `core` (Linux) | clippy, the Rust tests, the Python tests | every push |
| `msrv` | `cargo check` on Rust 1.85 | every push |
| `windows` | the Rust and Python tests on Windows | every push |
| `godot` | the Godot unit and e2e suites | every push |
| `sitl` (Linux) | the live Rust and Python tests against real Betaflight, the built wheel in a clean venv, the benchmark (fails below 1× real time), the Godot e2e with Betaflight | every push |
| `publish` | builds and uploads to PyPI | `v*` tags only, after core, windows and sitl pass |

## Commits
One logical change per commit, written as `type(scope): what changed` (`fix(dvr): …`, `test(radio): …`,
`docs: …`). A fix comes with the test that failed without it.

## Releasing the Python package
1. Bump `version` in `python/pyproject.toml` and add the release to `CHANGELOG.md`.
2. Push a matching tag: `git tag v<version> && git push origin v<version>` (the publish job checks they match).
3. Approve the `pypi` deployment in the run (the environment requires a reviewer; it accepts `v*` tags only).
4. If the sitl job fails on a known flake (`docs/roadmap.md`), re-run the failed jobs; publish waits for it.
