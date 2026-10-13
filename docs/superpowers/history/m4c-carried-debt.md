> Retired at the v1 close-out (2026-10-13): every item here now lives in `docs/roadmap.md` or was fixed (see
> `CHANGELOG.md`); this file is kept as history.

# M4c carried debt

## Deferred features
- **The repro bundle** (parent spec §7) — post-v1; M4c provides its halves: recordings and scripted inputs.
- **MCAP in Foxglove** — recordings open and plot in app.foxglove.dev (checked by hand 2026-10-13, after the fix
  below: `battery.voltage_v` and `imu.gyro_frd_radps` of a sandbox flight); live tailing a recording still being
  written is untested.
- **The Rerun live view and Blackbox Explorer** — checked by hand 2026-10-13: `ofs.rerun.view` streams a realtime
  open-loop climb to the Rerun viewer, and the sandbox flight's `LOG*.BFL` opens in Blackbox Explorer.
- **`mcap` is a git pin** (`00e47a1b`, 0.25.1 unreleased) — switch the workspace dependency to the crates.io release
  that carries the summary-order fix (foxglove/mcap#1838) once it ships, and re-run `crates/ofs-log/tests/determinism.rs`.
- **The reader is pure Python** — `ofs.reader` decodes one JSON record at a time: about 3.7 s for three patterns of a
  one-minute default recording, 11.7 s for all of it. A faster path (columnar decode, or the Rust reader behind a
  binding) waits for a user who needs it.
- **Rates are whole tick divisors** — a requested rate becomes `base_hz / round(base_hz / rate)`: 240 Hz on the
  8 kHz quads records at 242.4 Hz (the channel's `rate_hz` metadata says so).
- **The live view follows the state stream** (≤ 240 Hz, Python-side); 8 kHz signals are only in the recording.
- **`rerun-sdk` is pinned to 0.39.x** by the extra; bump it deliberately (its Python API changes between minors).
- **A Blackbox log of a session that ends armed is cut short** — the session kills SITL; disarm first (documented).
- **A recording stopped by a write error is left unfinished** — no summary section, so indexed readers refuse it;
  the `recording_stopped` event names the error.
- **Published to PyPI** — `open-fpv-sim` 0.1.0 (wheel and sdist) from tag `v0.1.0`, 2026-10-13, through the
  trusted publisher (this repository, workflow `ci.yml`, environment `pypi`, which needs a `v*` tag rule and a
  reviewer's approval). Later releases: bump `python/pyproject.toml`'s version and push the matching `v*` tag.
- **The SITL port-bind flake reaches the sandbox test** — on the `v0.1.0` tag, the wheel step's
  `test_sitl_sandbox.py` failed with "could not bind its ports" 16 s after the same test passed in the step before
  it; a re-run passed. It holds back `publish` (which needs `sitl`), so a release tag may need a re-run.
- Inherited and still open: fcs/receivers/cameras part kinds (m4a); windmilling drag, battery capacity fade,
  vibration transmissibility (m4b); `test_sitl_link.py` not in the sitl job's list (m3e). (The flaky
  `runner::tests::a_panicking_step_poisons_the_session_and_keeps_the_runner_alive`, found while planning M4c, was
  fixed on main in ccb50c9 before M4c started.)

## Rulings worth remembering
- **Records are JSON objects** (`{"v"}`, `{"x","y","z"}`, `{"w","x","y","z"}`) with a JSON schema per shape, not the
  spec's bare arrays: Foxglove's message paths need object roots.
- **The per-message index records stay** — about 25 MB per simulated minute at the default rates instead of 9.9 MB.
  M4c first dropped them (the Python and Rust readers cope), and Foxglove refused every recording: its reader needs
  them (`docs/research/mcap-determinism.md`).
- **A record is stamped with the time after its tick**, `(tick + 1) / base_hz`: the state `GetState` reports then.
- **Scheduled faults and inputs rows apply right after the step that reaches their time**, so a `run` ending at
  that time already shows them (and its fault events); the physics is the same as applying them before the next.
- **Inputs rows are relative to the `PlayInputs` call**; columns a file leaves out hold their value; the last row
  holds after playback. The CSV travels in the request: the server reads no client paths, as it writes none.
- **Requests up to 64 MiB** (`ofs_sim::server::MAX_REQUEST_BYTES`, served through `sim_server`): tonic's 4 MiB
  default refused the CSV of a few minutes of recorded sticks.
- **A StopRecording after a write failure says why** (`RecordError::Io` with the cause, once), not "no recording".
- **Sticks have one owner**: `SetSticks` during playback is `InvalidArgument` (the spec's ruling); a pilot and a
  playing file exclude each other (`InvalidState` either way round).
- **Python records lockstep sessions by default**; `sim.log` stops the recording (an MCAP file is readable once
  finished) and only ever returns this session's recording — a new `load` finishes the previous session's file on
  the server, and it stays reachable as `sim.recording.last`.
- **A world name** (`world="flat"`) is `<quad dir>/../worlds/<name>.toml`, as parts resolve against `library/`.
- **The Blackbox copy is a `Session` field declared last**: fields drop in declaration order, so SITL is gone and
  its files closed before the copy runs, on every way a session ends.
- **A launched server stops exactly once** — at `close()`, when its `Sim` is garbage-collected, or at interpreter
  exit (`weakref.finalize`, holding the stub, channel and process but not the `Sim`), so the §6.2 script, which never
  calls `close()`, still finishes its recording and Blackbox logs, and a `Sim` dropped inside a function never
  orphans its server.
