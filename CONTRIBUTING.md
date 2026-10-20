# Contributing

Thank you for helping. Setup for Windows and Linux (Rust, Python, Godot, Betaflight SITL, WSL) is in
`docs/dev-setup.md`; how the code fits together is in `docs/architecture.md`.

## Picking something to work on
Long-term work is tracked as issues under six axes (fidelity, modelling, video, sandbox, tooling, validation); each axis
has an umbrella issue labelled `epic` that lists its items, and `docs/roadmap.md` stays the ledger of known
limitations. Filter by `good first issue` for a gentle start, by `area:*` for your part of the stack, and by
`needs-data` if you have hardware or logs rather than code to offer.

- **Small and clear** (a bug, a doc fix, a `good first issue`): comment to claim it and open a PR.
- **Large or open-ended** (`research`, or anything spanning crates): comment first with a short plan or proposal. Large
  items are agreed on the issue, then written up as a spec and a plan in `docs/superpowers/` before the code.
- **A model change** carries a unit test against an analytic result, a provenance (`datasheet`, `measured` or
  `estimated`) for every new constant, and the matching `docs/research/` page updated. Seeded runs stay deterministic.

## Reporting a crash
If the simulator stops with an error (a Betaflight crash, a non-finite value, a panic) it has written a repro bundle
and the `sim_error` event names it: a folder under the logs directory (`.ofs-data/logs/<quad>-<n>.repro`). Zip it and
attach it to the issue; a maintainer replays it with `ofs-sim --replay <folder>`. What it holds and how exact the
replay is: `docs/repro-bundle.md`.

## Offering measurements
Real data is what turns an `estimated` constant into a `measured` one. A thrust-stand log (or a Blackbox step response)
goes in a submission folder under `validation/`, in the format of `docs/validation.md`; run
`cargo run -p ofs-validate -- <folder>` on it, and open a PR or an issue labelled `needs-data` with the folder and the
report. Data comes under CC-BY-SA-4.0, like the component library.

## Tests, from fastest to slowest
1. **Rust unit and integration tests:** `cargo test --workspace --locked`, and
   `cargo clippy --workspace --all-targets --locked -- -D warnings` (CI fails on any warning).
2. **Python, without firmware:** `cargo build -p ofs-sim`, `python -m pip install -e "python[dev]"`,
   `python -m pytest python/tests` (the tests that need Betaflight skip themselves).
3. **Live, with Betaflight SITL** (Linux or WSL; build it with `scripts/build-sitl.sh`): set `OFS_SITL_LAUNCH` to the
   SITL binary, then `cargo test -p ofs-fc -p ofs-sim --test sitl_live -- --ignored --test-threads=1` and the
   `python/tests/test_sitl_*.py` files. Run the live test files one at a time (several kill every SITL by name on
   purpose); only `test_sitl_parallel.py` and the two-session test in `ofs-sim`'s `sitl_live` run sessions side by
   side (`docs/dev-setup.md`, Ports and parallel sessions).
4. **Godot:** `GODOT_BIN=<godot 4.7.2> bash scripts/run-godot-tests.sh unit` (or `e2e`, or `all`); the script builds
   what it needs.
5. **Performance:** `python scripts/benchmark.py --binary target/release/ofs-sim` (`docs/dev-setup.md`, Performance).

## CI (`.github/workflows/ci.yml`)
| Job | Runs | Gates |
|---|---|---|
| `core` (Linux) | clippy, the Rust tests, the Python tests | every push |
| `msrv` | `cargo check` on Rust 1.85 | every push |
| `windows` | the Rust and Python tests on Windows | every push |
| `macos` (Apple silicon) | the Rust tests, the live Betaflight tests, all Python tests with SITL, the Godot suites with Betaflight | every push; non-blocking for now (`continue-on-error`) and not needed by `publish` |
| `godot` | the Godot unit and e2e suites | every push |
| `sitl` (Linux) | the live Rust and Python tests against real Betaflight, the built wheel in a clean venv, the benchmark (fails below 1× real time), the Godot e2e with Betaflight | every push |
| `publish` | builds and uploads to PyPI | `v*` tags only, after core, windows and sitl pass |

## Commits
One logical change per commit, written as `type(scope): what changed` (`fix(dvr): …`, `test(radio): …`,
`docs: …`). A fix comes with the test that failed without it.

## Releasing the Python package
1. In one PR, bump `version` in `python/pyproject.toml` **and** `[workspace.package]` in `Cargo.toml` (the server
   reports it), update `Cargo.lock` (`cargo build --workspace`, without `--locked`; CI builds `--locked`), and fold the
   changelog's "Unreleased" sections under a dated `## <version> — <date>` heading with an upgrade list (re-date it if
   the tag is pushed on another day).
   `python/tests/test_release.py` fails when the two versions, the lock or the changelog heading disagree.
2. Before tagging, check the package: `python -m build python --outdir <dir>`, `twine check <dir>/*`, install the wheel
   in a clean venv and load a quad against a freshly built `ofs-sim`, and confirm the version is not on PyPI
   (`pip index versions open-fpv-sim`). A PyPI release cannot be replaced, only yanked.
3. After the PR is merged, tag the merge commit on `main` (not whatever your checkout has) and push the tag:
   `git fetch origin && git tag v<version> origin/main && git push origin v<version>` (the publish job checks the tag
   against `pyproject.toml`).
   Pushing the tag is the maintainer's explicit step; nothing else publishes.
4. Approve the `pypi` deployment in the run (the environment requires a reviewer). Its deployment rule must be of
   type **Tag** with the pattern `v*` (Settings, Environments, `pypi`): a rule of type Branch named `v*` never matches a
   tag, and `publish` then fails with "Tag is not allowed to deploy to pypi" (this stopped 0.2.0 until the rule was
   fixed). After fixing the rule, re-run only the failed `publish` job.
5. If the sitl job fails on a known flake (`docs/roadmap.md`), re-run the failed jobs; publish waits for it.
