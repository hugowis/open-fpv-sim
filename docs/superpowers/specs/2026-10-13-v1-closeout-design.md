# v1 close-out — carried debt and the v1 docs

**Status:** design approved in conversation 2026-10-13; this file is the written spec.
**Parent:** `2026-10-04-open-fpv-sim-design.md` (v1: M0–M4, all merged; `open-fpv-sim` 0.1.0 on PyPI 2026-10-13).

## 1. Intent

v1 is feature-complete, but nine milestone files (`docs/superpowers/m*-carried-debt.md`) still list about 75 open
items, and nothing tells a newcomer what v1 is, what it is not, and what comes next. This close-out:

- **fixes** every defect, test gap and hygiene item left open (group A below), and builds the spec §8.6 performance
  benchmark that was never built;
- **settles** the OSD font licence question (the maintainer's ruling: accept and document);
- **moves** everything else — features, blocked pins, by-design limitations, modelling estimates — into one
  consolidated `docs/roadmap.md`, and retires the nine carried-debt files to a history folder;
- **writes** the docs that close v1: README status, CHANGELOG, the parent spec marked done with evidence, an
  architecture guide and a contributor guide.

**Success:** every item of every carried-debt file has a disposition in §3 that the work then honours; CI is green
with `test_sitl_link.py` and the benchmark in it; the SITL port-bind flake is root-caused and fixed (not retried);
the docs in §6 exist and describe the final state.

**Out of scope:** any group-B feature (they go to the roadmap); a new PyPI release (the Python package barely
changes — tagging `v0.1.1` afterwards is the maintainer's call).

## 2. Rulings made while brainstorming

| Question | Ruling |
|---|---|
| How much debt to fix | Group A (defects, test gaps, hygiene) + the §8.6 benchmark; everything else to the roadmap. |
| Docs to deliver | Roadmap + limitations; README + CHANGELOG; the parent spec closed out; architecture + contributing guides. |
| OSD font atlas (GPL-3.0-only in a GPL-3.0-or-later project) | Accept and document: the combined work is distributable under GPL-3.0. |
| Benchmark | Tracked in CI (job summary table) with a floor gate at 1× real time; the ≥ 2× target is measured on the maintainer's PC and recorded in the docs. |
| The carried-debt files | Moved unchanged to `docs/superpowers/history/`, each with a one-line header pointing to the roadmap. |

## 3. Disposition of every carried item

Legend — **W1–W7**: fixed or written by that workstream (§4–§6). **Closed (Mx)**: a later milestone already did it;
the roadmap drops it. **Roadmap**: post-v1 backlog. **Limitation**: a known, deliberate v1 limitation in the roadmap's
limitations section. **Ruling**: kept in the roadmap's "rulings worth remembering".

### M1
| Item | Disposition |
|---|---|
| The scheduler is unusable after an error (poisoned session) | Limitation (by design) |
| Physics simplifications (no rotor gyroscopic coupling, viscous friction below 5 cm/s, `BODY_ACCEL_NED` includes gravity, pre-clamp `omega_dot`) | Limitation |
| Heading/quaternion mapping for the compass | Roadmap (the shipped quad has no compass) |
| One intermittent MSP timeout (M0) | Closed — not seen since M1; the MSP client rejects stale replies |

### M3a
| Item | Disposition |
|---|---|
| Tramp | Closed (M3e) |
| The analog link model and degradation shader | Closed (M3b) |
| Font page 1 | Closed (M3e); pages 2–3 → Roadmap (see M3e) |
| HD/digital OSD element set | Roadmap |
| OSD aspect-ratio setting | Closed (M3e) |
| RPM filter and per-motor eRPM | Closed (M3d) |
| CRSF telemetry back to the radio | Closed (M3d) |
| A thermal model (ESC and VTX temperature constant 25 °C) | Roadmap |
| VTX faults | Closed (M4b) |
| The OSD font atlas is GPL-3.0-only | **W5** |
| `serial_overflow` never triggered by a real overflow | **W3** |

### M3b
| Item | Disposition |
|---|---|
| Collision with world objects | Closed (M3c) |
| ELRS on the shared propagation | Closed (M3c) |
| The VTX-interference fault | Closed (M4b) |
| Non-flat terrain and rotated boxes | Roadmap |
| Configurable receiver thresholds | Roadmap |
| Video latency, frame drops and DVR | Closed (M3e) |
| Modelling simplifications (one knife edge per object, no object reflections, one body-shadow lobe, estimated constants) | Limitation |
| Rulings (VTX transmits in open loop, golden-section obstruction search, rooted objects, 5300–6000 MHz emitters, sync-loss test at 4 km) | Ruling |

### M3c
| Item | Disposition |
|---|---|
| Body sphere past the legs | Closed (maintainer's flight check 2026-10-11: accepted) |
| MSP_STATUS failsafe flags not decoded in the 50 km link test | **W3** (the decoder exists since M4b: `python/tests/msp_status.py`) |
| Open-loop sessions cannot steer; no scripted-FC circuit test | Roadmap |
| The spec's out-of-scope list (900 MHz, ELRS dynamic power/hopping, 2.4 GHz interference, crash damage, non-flat terrain, rotated boxes, frequency-dependent `rf_loss_db`, object–object collisions) | Roadmap (crash damage and CRSF telemetry: CRSF closed in M3d) |
| Modelling simplifications (active-antenna transmit, two contact mechanisms, deepest-contact correction, one frequency/one lobe) | Limitation |
| Rulings (2.0 cm prop spheres, last collision's speed, no `LINK_DOWN` edge on a never-up link, the `handset_position` guard) | Ruling |
| Flake: `test_moving_off_the_parked_quads_channel_lowers_the_interference` MSP 89 timeout | **W1** |

### M3d
| Item | Disposition |
|---|---|
| The RPM filter's spectral proof | Roadmap |
| DShot extended telemetry and MSP-over-CRSF | Roadmap |
| Config-less telemetry slot ratio | Roadmap |
| Modelling simplifications (1:4 slot ratio, no UART contention model) | Limitation |
| Pole pairs constant vs Betaflight's `motor_poles` (a mismatched diff silently scales the RPM filter) | **W2** |
| Rulings (PWM motor protocol, FC telemetry outside the digest, battery values verbatim, a stale SITL binary refused loudly) | Ruling |

### M3e
| Item | Disposition |
|---|---|
| Font pages 2–3 | Roadmap |
| The DVR stops on a mid-recording resize | Limitation (re-negotiating the header → Roadmap) |
| No clean-feed DVR | Roadmap |
| A thermal model (Tramp status 25 °C) | Roadmap (with M3a's) |
| `FIELD_PERIOD_S` duplicated in `ofs-client::fpv` | **W2** |
| DVR boundary dedup is floating-point-path dependent | **W2** |
| `test_sitl_link.py` not in the sitl job's list | **W1** |
| The spec's out-of-scope list (digital video, audio, image-sequence/ffmpeg export, full HD OSD, configurable thresholds, object reflections) | Roadmap (the VTX-interference fault: Closed M4b) |
| The controls-menu DVR toggle shipped keybind-only | Roadmap |
| The two undelivered spec tests (Godot e2e DVR `.avi` check; lockstep latency + drops bit-identical) and the headless JPEG pin | **W3** |
| A same-name DVR start truncates silently | **W2** |
| `VideoHandles::read` reports latency 0 without a VTX | **W2** |

### M4a
| Item | Disposition |
|---|---|
| imu/baro are not parts | Roadmap |
| fcs/, receivers/, cameras/ part kinds | Roadmap |
| Provenance is validated, not exposed | Roadmap |
| `OFS_LIBRARY` is one path | Roadmap |
| Rulings (no mixed pole pairs on SITL quads, per-motor eRPM pole pairs, per-motor rotor inertia) | Ruling |

### M4b
| Item | Disposition |
|---|---|
| Windmilling drag on a dead motor | Roadmap |
| Per-cell battery model, capacity fade | Roadmap |
| Vibration transmissibility | Roadmap |
| RadioLoss on the downlink | Roadmap |
| Vibration on mixed pole-pair open-loop builds | Limitation |
| `test_sitl_link.py` not in CI | **W1** (same item as M3e) |
| The radio-fault no-op test cannot prove stream provenance | **W3** |
| The burst validation message says `on_ms`/`off_ms` for seconds | **W2** |
| `msp_status.py`'s `_exchange` has no partial-frame buffering | **W3** |
| `status.close()` not in try/finally in `test_burst_loss_failsafes_and_betaflight_flags_it` | **W3** |
| Unused `import ofs` in `test_sitl_faults.py` | **W3** |
| The interference cache's exact-equality key needs a comment | **W2** |
| Rulings (dedicated `elrs-faults` RNG stream, failsafe decode from arming-disable flags, weak-cell index + 1, the interference cache key) | Ruling |

### M4c
| Item | Disposition |
|---|---|
| The repro bundle | Roadmap |
| Live tailing a recording still being written in Foxglove | Limitation (untested) |
| `mcap` is a git pin | Roadmap (blocked on the crates.io release with foxglove/mcap#1838) |
| The reader is pure Python | Roadmap |
| Rates are whole tick divisors | Limitation |
| The live view follows the state stream (≤ 240 Hz) | Limitation |
| `rerun-sdk` pinned to 0.39.x | Roadmap (tooling) |
| A Blackbox log of a session that ends armed is cut short | Limitation |
| A recording stopped by a write error is left unfinished | Limitation |
| The SITL port-bind flake reaches the sandbox test | **W1** |
| Rulings (JSON object records, message indexes kept, record stamping, scheduled-row timing, inputs semantics, 64 MiB requests, one stick owner, Python records lockstep by default, world names, the Blackbox copy last, the launched server stops once) | Ruling |

## 4. W1 — CI and flakes

**`test_sitl_link.py` in CI.** Add it to the sitl job's pytest list (`.github/workflows/ci.yml`). If it fails there,
that is a finding to fix under this workstream, not a reason to leave it out.

**The SITL port-bind flake.** Evidence: on tag `v0.1.0` (run 38437153244) the wheel step's
`test_sitl_sandbox.py` failed with "could not bind its ports" 16 s after the same test passed in the previous step;
the earlier WSL failures of `test_sitl_link` showed the same message.

Facts established while designing: Betaflight's TCP listeners set `SO_REUSEADDR` (dyad), so `TIME_WAIT` is not the
cause; SITL is stopped only by `SitlProcess::drop` (kill + wait), and nothing ties its lifetime to the server's;
the Python `_stop_server` escalates `Unload` (5 s timeout) → `terminate` → 5 s → `kill`.

**Lead hypothesis:** a server that is SIGKILLed (or dies) never runs `Drop`, so its SITL survives and holds ports
5760+ for the next session; the sandbox test's unload (finishing a ~5 MB recording, copying the Blackbox log) is
the slowest unload there is.

**Procedure (systematic debugging):**
1. Reproduce on WSL: launch a SITL session, SIGKILL the server, check whether SITL survives; time the sandbox test's
   unload on a cold run.
2. If confirmed, fix the root cause: SITL dies with its parent on every path — Linux: `prctl(PR_SET_PDEATHSIG,
   SIGKILL)` in a `pre_exec` hook on the SITL `Command`; Windows (via `wsl.exe`): verify the M0 finding that killing
   `wsl.exe` ends SITL, and fix it the same way if it does not. If the unload timing is near the 5 s limit, raise
   `UNLOAD_TIMEOUT_S` too.
3. A live regression test: SIGKILL a server running a SITL session, then load a SITL session in a new server at
   once — it must bind.
4. If step 1 disproves the hypothesis, keep investigating; do not ship the `prctl` change on speculation.

**The m3c VTX timeout** (`no MSP reply to command 89`). Search CI logs since 2026-10-10 for it. Never recurred:
close it in the roadmap with the run range searched. Recurred: capture the test output and Betaflight's stderr and
root-cause it like the flake above.

## 5. W2–W4 — fixes, test gaps, the benchmark

Every W2 and W3 item is a test-first change: a failing test, then the fix.

### W2 — Rust defects
- **Same-name DVR start** (`crates/ofs-client/src/dvr.rs`): open with `File::create_new`; a start whose file exists
  fails with an error naming the path instead of truncating it.
- **`FIELD_PERIOD_S`**: one constant in `ofs-core` (both `ofs-video` and `ofs-client` depend on it), used by
  `ofs-video::link` and `ofs-client::fpv`; a test pins `FIELD_PERIOD_S × FIELD_RATE_HZ == 1`.
- **DVR boundary dedup**: captures dedup on an integer field index (`round(t / FIELD_PERIOD_S)`) instead of
  comparing floating-point boundaries; a test over a long flight time shows no one-ulp stretch.
- **Latency without a VTX** (`crates/ofs-sim/src/vehicle.rs`, `VideoHandles::read`): report the world receiver's
  quantized latency whether or not a VTX is present.
- **`motor_poles`**: a SITL quad whose diff sets `motor_poles` to anything other than 2 × its motors' pole pairs is a
  config error at load; a diff with no `motor_poles` is checked against Betaflight's default, 14.
- **The burst validation message** says seconds (`on_s`/`off_s`), and its test greps the new words.
- **The interference cache key** gets the one-line comment (exact equality is intended; non-finite payloads cannot
  reach it).

### W3 — test gaps
- **The radio no-op provenance test**: fading on, at the sensitivity edge, varying `fault.radio.loss_fraction`; the
  model's own stream must not move. The overclaiming draw-site comment is softened to match.
- **`serial_overflow` for real**: an integration test that overflows a real capture/tap buffer through the actual
  wire code (a reduced capacity where the code allows one) and sees `fc.serial_dropped_bytes` rise and the event
  fire.
- **Lockstep latency + drops determinism**: two same-seed lockstep runs with video latency and drops configured
  produce byte-identical MCAP recordings.
- **Godot e2e DVR check**: the e2e records a few seconds and checks the `.avi` — RIFF/AVI header, a frame count
  matching the recording duration at about 50 fps, every frame a JPEG (`FF D8`). Risk: the headless dummy renderer
  may not produce viewport images; if so, the e2e drives the real `dvr.gd` → `dvr_write_frame` path with synthetic
  images, and the plan records the deviation.
- **Headless JPEG pin**: a Godot unit test runs `Image.create` → `save_jpg_to_buffer` → `dvr_write_frame` and
  checks the written frame, closing the JPEG-quality guard beyond the comment in `godot/ui/dvr.gd`.
- **The 50 km link test** asserts Betaflight's decoded failsafe flags (`msp_status.py`, arming-disable
  RX_FAILSAFE/FAILSAFE) besides "never arms" and the OSD LQ cell.
- **`msp_status.py`**: `_exchange` buffers until a whole `$M>` frame has arrived.
- **`test_sitl_faults.py`**: `status.close()` in try/finally; the unused `import ofs` removed.

### W4 — the benchmark (spec §8.6)
`scripts/benchmark.py` drives a server through the public Python API:
- For each configuration: load, warm up 1 s, time `sim.run(30)` on the wall clock; RTF = 30 / wall.
- Configurations: open loop with recording on and off; Betaflight SITL lockstep with recording on and off (skipped
  without `OFS_SITL_LAUNCH`).
- Output: a Markdown table (configuration, simulated s, wall s, RTF) on stdout; `--summary` appends it to
  `$GITHUB_STEP_SUMMARY`; `--floor X` exits non-zero when any row is below X.
- CI: one step in the sitl job after the live tests, against a release build of `ofs-sim`, with `--summary --floor 1`.
- The maintainer's PC: run it (Windows-native open loop, WSL SITL); record the numbers and the date in
  `docs/dev-setup.md` and as the §8.6 evidence. A row below the 2× target is stated plainly.

## 6. W5–W7 — licence and docs (written last, describing the final state)

**W5 — licence.** The README gains a Licence section: the project is GPL-3.0-or-later; the OSD font atlas
(`godot/ui/`, notice `godot/ui/OSD_FONT_LICENSE.md`) is GPL-3.0-only, derived from Betaflight Configurator's
`default.mcm`, so the combined work is distributed under GPL-3.0. The notice file states the ruling and its date.

**W6 — roadmap.** `docs/roadmap.md`:
- *Known limitations* by subsystem (physics, electrical, radio, video, FC/SITL, logging, Python): one line each,
  linking the research doc that explains it.
- *Post-v1 backlog* by theme: fidelity levels 2/3 (parent spec §1.5); modelling features; sandbox (C: plugin
  models, the repro bundle); tooling (fast reader, the `mcap` and `rerun-sdk` pins); validation reports (parent
  spec §8.5).
- *Rulings worth remembering*: the Ruling rows of §3, kept because they explain behaviour that would otherwise
  look like a bug.
The nine carried-debt files move unchanged to `docs/superpowers/history/`, each under a one-line header pointing to
the roadmap; links to them elsewhere in the repository are updated.

**W7 — v1 docs.**
- `README.md`: a *Status: v1* block — what works (the three §1.4 criteria), what does not (parent spec §2), `pip
  install open-fpv-sim`, links to the roadmap, CHANGELOG, architecture and contributing guides.
- `CHANGELOG.md` (Keep a Changelog): *0.1.0 — 2026-10-13* summarising M0–M4 by capability; *Unreleased* listing
  this close-out.
- The parent spec: a header line "v1 complete" with the date; §1.4's criteria each followed by their evidence (test
  names, manual checks and their dates); §8's tiers each with where they live, the benchmark included; §9's table
  gains a Status column with each milestone's merge commit.
- `docs/architecture.md`: the crate map (one line per crate, with its dependencies); the tick and the data flow
  (bus, scheduler, models, SITL bridge, clients); the client protocols (gRPC, the Godot extension); "how to add"
  recipes — a part kind, a model, a fault, a recorded signal — each naming the files to touch.
- `CONTRIBUTING.md`: toolchains (linking `docs/dev-setup.md`); the test tiers (unit, live SITL with `--ignored` and
  `OFS_SITL_LAUNCH`, Python, Godot unit and e2e); the CI jobs and what each gates; the commit style
  (`type(scope): sentence`); the release steps (bump `python/pyproject.toml`, push tag `v*`, approve the `pypi`
  deployment, re-run the sitl job on a known flake).

## 7. Order and testing

W1 first, so CI can be trusted while the rest lands; then W2 and W3; then W4; then W5–W7. Everything lands on the
branch `v1-closeout`. The full verification before merging is the usual set: Windows `cargo test` + clippy +
Python; WSL live Rust and Python; the Godot suites; then CI on the branch push, with the benchmark's job-summary
table.

## 8. Risks

| Risk | Answer |
|---|---|
| The flake's hypothesis is wrong | §4's procedure keeps investigating; nothing ships on speculation. |
| Headless Godot cannot render DVR frames | The e2e drives the real DVR path with synthetic images; the deviation is recorded. |
| CI runners fall below 1× on the SITL lockstep rows | The floor exists to catch a regression, not to measure; if the first CI run is below it, record the runner numbers and set the floor from them, stating why. |
| `motor_poles` validation rejects a shipped quad | The shipped diffs are checked first; a shipped quad that fails is a real mismatch and gets fixed in its diff. |
| The disposition table misses an item | The W6 step re-reads all nine files against §3 before moving them. |
