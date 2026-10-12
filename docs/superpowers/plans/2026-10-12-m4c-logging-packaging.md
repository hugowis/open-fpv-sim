# M4c: logging and packaging — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every session can be recorded and replayed. The signal bus records to deterministic MCAP files, which read back as DataFrames, CSV or Parquet, show live in Rerun, and sit next to Betaflight's own Blackbox logs. Scripts drive sticks from a CSV and schedule faults at a simulated time. The Python client ships to PyPI as `open-fpv-sim`, and the parent spec's §6.2 example runs as printed.

**Architecture:**
- A new `ofs-log` crate holds a recorder model, appended last to the scheduler. It only reads the bus. The session starts and stops it through a shared handle (the `OsdHandle` pattern).
- It writes through an MCAP sink pinned to the upstream `mcap` revision whose summary order is deterministic.
- The vehicle gains a per-tick hook that applies scheduled faults and the rows of a playing inputs file.
- Protocol 9 carries recording, inputs, `at_s`, world names and the logs directory.
- Python gains `ofs.reader`, `sim.recording`, `sim.log`, `sim.play_inputs` and `ofs.rerun`.
- Packaging renames the distribution and adds a CI job that publishes only on tags.

**Tech Stack:** Rust workspace (new `ofs-log`; `ofs-core`, `ofs-sim`, `ofs-proto`, `ofs-client`, `ofs-config`), `mcap` (git pin), `serde_json`, tonic/prost (protocol 9), Python ≥ 3.10 (`grpcio`, `mcap`, `pandas`, `pyarrow`, `rerun-sdk` 0.39.x, `build`), Betaflight SITL (pinned 2026.6.2, unchanged), GitHub Actions (PyPI trusted publishing).

**Spec:** `docs/superpowers/specs/2026-10-11-m4-sandbox-design.md`. The parts that apply: §4 (logging), §5 (`play_inputs`), §6 (packaging), §7 (protocol 9), §8's M4c row and failure handling, §9 (risks) and §10 (debt). Also from the parent spec (`2026-10-04-open-fpv-sim-design.md`): §5.5, §6.2, §6.3 and §8.4. The plan argues from the spec, so executors read both. The pre-M4c spike that settled the riskiest point is `docs/research/mcap-determinism.md`.

**How this plan was verified:**
- **The prototype:** the plan was implemented end to end on a scratch clone of the merged M4b tree (`174d85a` plus `docs/m4c-prep` `6c0430c`). Every code block below is that prototype's code, split by task.
- **The replay:** the replay applied the blocks in order to a fresh clone and regenerated `Cargo.lock` and the protoc stubs. It reproduced each task's tree.
- **Final results:**
  - Windows: `cargo test --workspace` 516 passed, clippy clean; Python 50 passed and 22 skipped (no SITL).
  - Linux (WSL): `cargo test --workspace -- --include-ignored --test-threads=1` 527 passed, the live SITL tests included; the Python suite 72 passed against live SITL.
  - Godot: the unit suites and the open-loop end-to-end passed.

**Prerequisite (before Task 1):** `main` must contain `docs/m4c-prep` (`6c0430c`, which brings the research note and the spec's resolved rulings). Then branch off `main`:

```bash
git switch -c m4c-logging-packaging main
```

## Global Constraints

- **Protocol 9.** `PROTOCOL_VERSION` goes from 8 to 9 in three places together: `crates/ofs-proto/src/lib.rs`, `python/ofs/client.py:22` and the pin in `crates/ofs-proto/tests/messages.rs`. The wire changes are exactly:
  - four RPCs: `StartRecording`, `StopRecording`, `PlayInputs`, `StopInputs`;
  - five messages: `RecordingSignal`, `StartRecordingRequest`, `RecordingStarted`, `RecordingSummary`, `InputsFile`;
  - `optional double at_s = 9` on `Fault`;
  - `logs_dir = 3` on `HandshakeReply`;
  - four `EventKind` values, 16 to 19: `inputs_started`, `inputs_ended`, `recording_started`, `recording_stopped`.

  `LoadRequest.world_path` keeps its type; only its comment changes, because it now accepts names. Nothing else changes on the wire. The spec names three of the messages; `StartRecordingRequest` and `RecordingStarted` are the request and reply it leaves unnamed.
- **The `mcap` pin.** Exactly `mcap = { git = "https://github.com/foxglove/mcap.git", rev = "00e47a1b", default-features = false, features = ["zstd"] }`, the only version that writes deterministic files (see `docs/research/mcap-determinism.md`). Never use crates.io 0.25.0. Its highest dependency MSRV is 1.85, the workspace's own.
- **Recording never changes a run.** The recorder registers no signal and writes nothing to the bus: a run's digest is the same with and without it. Two lockstep runs with the same seed and inputs write byte-identical files.
- **Existing behaviour unchanged.** Every existing test passes unchanged, except the edits this plan makes:
  - `Fault { kind, .. }` literals gain `at_s: None`;
  - the protocol pin goes to 9;
  - the shipped diffs gain a line, so an existing EEPROM in your own `.ofs-data` refuses to load until you delete its `eeprom.bin` (the error says so).
- **Rust style.** Never run bare `rustfmt`; hand-indent new code to match. Run `cargo clippy --workspace --all-targets --locked -- -D warnings` after every task that touches Rust; it is CI's first gate.
- **New dependencies.** The first cargo command after adding a dependency must run without `--locked`, so `Cargo.lock` updates (Task 1). Commit `Cargo.lock`.
- **Python file edits** keep LF line endings. Write files from scripts in binary mode, or with an editor set to LF.
- **Commands:**
  - Windows commands run from the repository root in Git Bash.
  - Live tests run inside WSL against a Linux-built `ofs-sim`, from `/mnt/c/dev/open-fpv-sim` (or your worktree's path).
  - In WSL, use `CARGO_TARGET_DIR=$HOME/ofs/m4c-target`. Two checkouts must never share one target directory: it mixes same-named crates.
  - WSL's system Python has no grpc, so run Python there through `uv run --no-project --with ...`. `--no-project` keeps uv from writing a `python/uv.lock`.
- **Known flake, not yours:** `runner::tests::a_panicking_step_poisons_the_session_and_keeps_the_runner_alive` fails about half the time under WSL when tests run in parallel, on `main` too: an `overrun` event can arrive before the `sim_error` it reads. Re-run it or use `--test-threads=1`. A separate task fixes it.
- **Commits:** one per task, conventional style, after the task's tests pass. The protocol bump is `feat(proto)!:`.
- **Drone failures are behaviour, simulator failures are loud** (spec §7):
  - a malformed inputs file is a `config` error naming every problem with its line;
  - a recording pattern that matches nothing is `invalid_argument`, naming the signals it probably meant;
  - a disk error stops the recording with a `recording_stopped` event while the session flies on.

## Rulings and facts verified in advance

All verified in the prototype. They are the contract where the spec is silent.

1. **The writer's options** (`crates/ofs-log/src/sink.rs`):
   - Library string pinned to `"ofs-log"`; the default embeds the crate version, which would change the bytes on every `mcap` bump.
   - zstd chunks with `compression_threads(0)`: single-threaded, so the output can't depend on core count.
   - `emit_message_indexes(false)`. Measured over 60 simulated seconds at the default rates (105 signals, 15,456 records/s): **24.9 MB with indexes, 9.9 MB without**. The Python indexed reader and the Rust `MessageStream` read the files without them.
   - The summary keeps repeated channels and schemas. The Python indexed reader needs them: with them off it raises `KeyError`.
2. **Records are JSON objects** (`{"v":x}`, `{"x","y","z"}`, `{"w","x","y","z"}`), refining the spec's bare arrays: Foxglove's message paths need an object root. Each shape has a `jsonschema` schema (`ofs.Scalar`, `ofs.Vec3`, `ofs.Quat`). Numbers print through serde_json (shortest round trip; a non-finite value would be `null`). Channel metadata is `{"rate_hz": "<actual rate>"}`.
3. **Each record is stamped with the time *after* its tick:** `log_time = publish_time = (tick + 1) · 10⁹ / base_hz` ns, which is the state `GetState` reports at that time. A channel with divisor `d` records when `(tick + 1) % d == 0`, so channels align to one global grid whenever recording starts.
4. **The default table** (`ofs_log::DEFAULT_RATES`; the first matching row wins): `body.*` 240 Hz, `imu.*` 1000, `motor.*` 240, `esc.*` 240, `prop.*` 240, `battery.*` 10, `radio.*`, `video.*`, `rc.*` and `fault.*` 50, `*` 50.
   - `prop.*` joins the spec's motor/ESC class, because thrust and torque are the motor's output.
   - A rate becomes `divisor = max(1, round(base_hz / rate))`: 240 Hz on the 8 kHz quads records at 242.4 Hz, and the metadata says so.
   - Rates must be finite, greater than 0 and at most `base_hz`.
5. **Patterns:** `*` matches any run of characters, dots included; `?` matches one character; anything else matches itself. Python follows the same rules (`fnmatch` with `[` escaped).
   - No requests means every signal at its default rate.
   - Otherwise each signal takes the first request that matches it; `rate_hz: None` (0 on the wire) takes the default.
   - Every request must match at least one signal. Problems are collected and joined with `; `.
   - **Near misses** are names sharing a literal piece of the pattern, ranked by pieces shared, then by starting with the first piece, then by name; at most five. The same ranking is in Rust (`spec::near_misses`) and Python (`reader.near_misses`).
6. **The recording lifecycle:**
   - The server names files `<data_dir>/logs/<quad stem>-<n>.mcap`, with `n` one more than the largest existing number. Creation uses `create_new`, so an existing file is never overwritten. A refused request creates no file.
   - One recording at a time: starting a second is `invalid_state`, and so is stopping when none runs.
   - A write error stops the recording, keeps the message for the session's `recording_stopped` event, and makes the next `StopRecording` return that error (`internal`, naming the cause) instead of "no recording".
   - Dropping the vehicle while it records finishes the file: `mcap::Writer`'s `Drop` writes the summary. So Unload, a new Load and server shutdown all leave readable files.
7. **Scheduled faults and inputs share one tick rule** (`inputs::tick_at`): an event at time `t` lands on the first tick at or after `t`, and a product within 1e-6 of a whole tick is that tick (0.3 s at 8 kHz is tick 2400, not 2401). Both apply **right after the step that reaches their tick** (`Vehicle::step_ticks`), so a `run` that ends there already shows them and their events. The physics is identical to applying them before the next step.
   - `Vehicle::run_for` now goes through `step_ticks` (via the new `Scheduler::ticks_for`); it used to bypass any hook.
8. **Faults with `at`:**
   - The time must be finite and at least 0.
   - The fault is validated when scheduled (`check_fault`, split out of `set_fault` with every message unchanged).
   - A tick already passed is `InvalidArgument`: `fault time 0.2 s has passed (the session is at 1 s)`. The current tick applies at once.
   - Faults due on the same tick apply in scheduling order.
   - `clear_faults` also drops the pending ones.
   - Pending faults are not "active": `fault_injected` fires when they apply.
9. **The inputs CSV** (`crates/ofs-sim/src/inputs.rs`):
   - A UTF-8 BOM and CRLF line endings are accepted. Blank lines and lines starting with `#` are skipped.
   - The header is `t_s` plus any non-empty subset of `roll, pitch, yaw, throttle, aux1..aux4`, with no duplicates.
   - Every value must be a finite number. `t_s` must be at least 0 and strictly increasing.
   - Throttle must be in [0, 1], every other column in [-1, 1].
   - Problems are collected, each as `<name> line <n>: ...`, capped at 20 plus `... and N more`. At least one row is required.
   - The client sends the file's text (`InputsFile{name, csv}`): the server reads no client paths, as it writes none.
10. **Playback:**
    - Rows' `t_s` count from the `PlayInputs` call, and rows at `t_s = 0` apply during the call.
    - Columns the file leaves out keep their values, and the last row holds after playback ends.
    - `SetSticks` during playback is `InvalidArgument` (the spec's ruling).
    - `PlayInputs` while another file plays is `InvalidState`.
    - A pilot connected and a playing file exclude each other: either order gives `InvalidState`.
    - `StopInputs` with nothing playing does nothing.
    - Events: `inputs_started` (`<name>: <rows> rows over <duration> s`), and `inputs_ended` with `<name>: finished` (raised by `Session::changes`) or `<name>: stopped`.
11. **Requests up to 64 MiB** (`ofs_sim::server::MAX_REQUEST_BYTES`, served through `sim_server(...)` by `main` and the tests). tonic's default of 4 MiB refuses the CSV of a few minutes of recorded sticks.
12. **World names:** `world="flat"` (no `/` or `\`, no `.toml`) resolves to `<quad dir>/../worlds/flat.toml`, the way parts resolve against `library/`. Anything else is a path (`ofs_config::world::resolve`). A missing world is the usual `config` error naming the path.
13. **The Blackbox** (pinned against the checkout, `~/ofs/betaflight` @ 2026.6.2):
    - `blackbox.c` defaults the device to `BLACKBOX_DEVICE_NONE`, so the shipped diffs gain `set blackbox_device = VIRTUAL`.
    - `blackbox_virtual.c` writes `LOG%05u.BFL` into SITL's working directory (the quad's firmware directory). It picks the next number by scanning that directory at boot, and closes the file on disarm.
    - The OSD crash on that device (`storageTotal == 0`) is already guarded by the SITL patch.
    - `SitlProcess::drop` kills SITL, so a log still open while armed is cut short. Disarm before the session ends.
    - SITL arms 1.0 s after load, measured with the deterministic-boot patch (arming at 1.0, 1.5, … 3.5 s all spun the motors), so `flips.csv` arms at 1 s.
14. **Copying Blackbox logs:**
    - `BlackboxCopy` is the **last field declared on `Session`**: Rust drops fields in declaration order, so the vehicle (and SITL) is gone before the copy runs, however the session ends.
    - It copies each `LOGnnnnn.BFL` modified since 2 s before the load to `<logs dir>/<firmware dir name>-LOGnnnnn.BFL`.
    - An identical copy already there is skipped; a different file under that name gets a `-2`, `-3`, ... suffix.
    - Only Betaflight sessions get one.
15. **The Python client:**
    - A lockstep `load` starts a default recording (`record=None` means on for lockstep, off for real time; `record=False` opts out).
    - `sim.log` stops a running recording (an MCAP file is readable once finished) and returns `reader.Log`. Without any recording it raises `InvalidState`.
    - A new `load` turns the previous session's recording into `recording.last`: the server finished it as the session ended.
    - `sim.logs_dir` comes from `HandshakeReply.logs_dir`, and `blackbox_files()` lists `*.BFL` there.
    - Faults take `at` as a keyword-only dataclass field (`field(kw_only=True)`, Python ≥ 3.10).
16. **`ofs.reader`:**
    - The table is wide, indexed by `time_s`, with one column per signal component: `name` for a scalar, `name.x` and so on otherwise.
    - Columns come in **pattern order** (each pattern's matches in name order, no repeats), with NaN where a slower signal has no record.
    - It needs only `mcap`, plus `pandas` for DataFrames (Parquet also needs `pyarrow`). It imports lazily, with an `ImportError` naming the extra.
    - A pattern that matches no recorded signal raises `KeyError` with near misses.
    - Speed: about 3.7 s for three patterns of a one-minute recording, 11.7 s for all of it (debt).
17. **`ofs.rerun`** (rerun-sdk 0.39, checked against the installed API: `RecordingStream`, `set_time(timeline, duration=)`, `Transform3D(translation=, quaternion=Quaternion(xyzw=))`, `Scalars`, `TextLog(level=TextLogLevel.*)`, `Boxes3D`, `Points3D`, `ViewCoordinates.FRD`):
    - It logs the world statically, with NED drawn as FRD.
    - Each state from the stream logs the pose and the curves; each event logs as text.
    - `view` drains the `Sim`'s events, so document viewing through a second connection.
    - The extra pins `rerun-sdk>=0.39,<0.40`, because its API changes between minor releases. 0.39 has no reader module (`rerun.dataframe` is gone), so the tests use a stand-in stream plus an `.rrd` smoke test.
18. **Packaging:**
    - Distribution `open-fpv-sim` 0.1.0, import `ofs`, pure Python (`py3-none-any`), with `.pyi` stubs as package data.
    - `license = "GPL-3.0-or-later"` as an SPDX string, with no license classifier, and `setuptools>=77`. The old table form and the classifier draw deprecation warnings that become errors in 2027.
    - Extras: `log` = `mcap>=1.5`, `pandas>=2`; `rerun` = `rerun-sdk>=0.39,<0.40`; `dev` adds `pyarrow`, `build` and both extras' packages.
    - `python/README.md` is the PyPI page.
    - `launch()` registers an `atexit` close through a weak reference, so a script that never calls `close()` (the §6.2 one) still stops its server and finishes its recording and Blackbox logs.

## Review Focus

These are the inputs the spec implies that are most likely to bite a user, most likely first. Each is pinned by the named test:

1. **A session that ends while recording** (Unload, a new Load, a server shutdown, a script that never stops the recording) must leave a finished, readable file: `ending_the_session_mid_recording_still_finishes_the_file` (Task 2) and `test_a_new_load_finishes_the_previous_recording` (Task 5).
2. **An inputs CSV saved by a spreadsheet** (a UTF-8 BOM, CRLF line endings) must parse exactly like a plain one: `a_spreadsheet_export_with_a_bom_and_crlf_lines_parses_like_a_plain_file` (Task 3).
3. **An inputs file larger than gRPC's 4 MB default message** (a few minutes of recorded sticks) must play: `an_inputs_file_larger_than_grpcs_default_limit_plays` (Task 4) and `test_a_long_inputs_file_plays` (Task 5).
4. **Stopping after a disk error** must say what failed, not "no recording": `a_failed_write_stops_the_recording_once_and_the_run_flies_on` (Task 1).
5. **Recording a real-time session** (the runner thread steps the vehicle) must record like lockstep: `test_a_real_time_session_records_when_asked` (Task 5).

## File Structure

```
Cargo.toml                                  + mcap (git pin), serde_json, ofs-log workspace deps (T1)
crates/ofs-core/src/bus.rs                  + Bus::signals() (T1)
crates/ofs-core/src/scheduler.rs            + Scheduler::ticks_for() (T3)
crates/ofs-log/                             new crate (T1)
  src/glob.rs                                 pattern matching
  src/spec.rs                                 default table, request resolution, near misses
  src/sink.rs                                 the deterministic MCAP writer and the JSON records
  src/recorder.rs                             RecorderHandle, the Recorder model, next_log_path
  tests/determinism.rs, tests/recorder.rs
crates/ofs-sim/src/vehicle.rs               + recorder (T2); check_fault split, schedule_fault, playback, apply_due (T3)
crates/ofs-sim/src/inputs.rs                new: the inputs CSV parser, tick_at (T3)
crates/ofs-sim/src/blackbox.rs              new: BlackboxCopy (T4)
crates/ofs-sim/src/{server,session,streams,main,lib}.rs   protocol 9 handlers, events, guards (T4)
crates/ofs-sim/tests/{recording,scripted}.rs  new (T2, T3); grpc.rs + protocol 9 tests (T4)
crates/ofs-config/src/world.rs              + world::resolve (T4)
crates/ofs-proto/src/lib.rs, proto/ofs/v1/sim.proto   protocol 9 (T4)
crates/ofs-client/src/{model,worker}.rs     four event kinds; at_s: None (T4)
python/ofs/v1/sim_pb2*.py(i)                regenerated (T4)
python/ofs/{reader,recording}.py            new (T5); rerun.py new (T6)
python/ofs/{client,faults,__init__}.py      protocol 9 (T4); API (T5); atexit (T7)
python/pyproject.toml, python/README.md     open-fpv-sim, extras (T5, T6)
python/tests/test_{recording,scripting}.py  new (T5); test_rerun.py (T6); test_sitl_sandbox.py (T7)
python/examples/{sandbox.py,flips.csv}      new: the §6.2 example (T7)
quads/*.betaflight.diff                     + blackbox_device = VIRTUAL (T7)
.github/workflows/ci.yml, .gitignore        wheel test, publish job (T7)
README.md, docs/...                         (T8)
```

---

### Task 1: `ofs-log`: the deterministic MCAP recorder (byte-identity first)

The spec's riskiest point comes first. The crate is self-contained (it depends only on `ofs-core`), and its byte-identity test is written before the writer. Nothing in the simulator uses it yet.

**Files:**
- Create: `crates/ofs-log/Cargo.toml`
- Create: `crates/ofs-log/src/glob.rs`
- Create: `crates/ofs-log/src/lib.rs`
- Create: `crates/ofs-log/src/recorder.rs`
- Create: `crates/ofs-log/src/sink.rs`
- Create: `crates/ofs-log/src/spec.rs`
- Create: `crates/ofs-log/tests/determinism.rs`
- Create: `crates/ofs-log/tests/recorder.rs`
- Modify: `Cargo.toml`
- Modify: `crates/ofs-core/src/bus.rs`
- Modify: `crates/ofs-core/tests/bus.rs`
- Regenerate (never hand-edit): `Cargo.lock`

**Interfaces:**
- Produces for Task 2 and later:
  - `ofs_core::Bus::signals(&self) -> impl Iterator<Item = (&str, SignalKind)>`, in name order.
  - `ofs_log::{SignalRate { pattern: String, rate_hz: Option<f64> }, Channel { name, kind, divisor: u64, rate_hz: f64 }, DEFAULT_RATES}`.
  - `ofs_log::spec::{resolve(&[SignalRate], &[(String, SignalKind)], base_hz) -> Result<Vec<Channel>, String>, default_rate, near_misses}`.
  - `ofs_log::glob::matches(pattern, name) -> bool`.
  - `ofs_log::sink::{McapSink::new(w, &[Channel]), write(index, time_ns, &[u8]), finish() -> SinkSummary, LIBRARY, scalar_json, vec3_json, quat_json}`.
  - `ofs_log::{RecorderHandle, Recorder, RecordError, RecordingSummary}`:
    - `RecorderHandle` is `Clone + Default`, with `start(&Bus, base_hz, &[SignalRate], &Path) -> Result<Vec<String>, RecordError>`, `start_with(.., Box<dyn WriteSeek>, &Path)`, `stop() -> Result<RecordingSummary, RecordError>`, `is_recording()` and `take_failure() -> Option<String>`.
    - `Recorder::new(handle, base_hz)` is a `Model` named `"recorder"`, divisor 1.
    - `RecordError` has the variants `AlreadyRecording`, `NotRecording`, `Spec(String)` and `Io(String)`.
    - `RecordingSummary { path: PathBuf, messages: u64, first_s: f64, last_s: f64 }`.
  - `ofs_log::recorder::next_log_path(dir, stem) -> io::Result<PathBuf>`.

- [ ] **Step 1: The bus lists its signals: the test**

**Modify `crates/ofs-core/tests/bus.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-core/tests/bus.rs b/crates/ofs-core/tests/bus.rs
index 2739fef..0753860 100644
--- a/crates/ofs-core/tests/bus.rs
+++ b/crates/ofs-core/tests/bus.rs
@@ -138,3 +138,16 @@ fn tables_must_be_non_empty_and_strictly_increasing() {
     assert!(interp::check_table(&[(1.0, 1.0), (1.0, 2.0)], "t").unwrap_err().contains("strictly increasing"));
     assert!(interp::check_table(&[(0.0, f64::NAN)], "t").unwrap_err().contains("finite"));
 }
+
+#[test]
+fn lists_every_signal_with_its_kind_in_name_order() {
+    let mut bus = Bus::new();
+    let _q: Signal<DQuat> = bus.signal("b.att");
+    let _s: Signal<f64> = bus.signal("a.x");
+    let _v: Signal<DVec3> = bus.signal("c.pos");
+    let listed: Vec<(&str, ofs_core::SignalKind)> = bus.signals().collect();
+    assert_eq!(
+        listed,
+        [("a.x", ofs_core::SignalKind::Scalar), ("b.att", ofs_core::SignalKind::Quat), ("c.pos", ofs_core::SignalKind::Vec3)]
+    );
+}
````

Run: `cargo test -p ofs-core --test bus lists_every_signal_with_its_kind_in_name_order`
Expected: compile error, `no method named signals found for struct Bus`.

- [ ] **Step 2: The bus lists its signals: the method**

**Modify `crates/ofs-core/src/bus.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-core/src/bus.rs b/crates/ofs-core/src/bus.rs
index 99a6530..b5cd870 100644
--- a/crates/ofs-core/src/bus.rs
+++ b/crates/ofs-core/src/bus.rs
@@ -110,6 +110,11 @@ impl Bus {
         T::arena_mut(self)[s.index] = value;
     }
 
+    /// Every registered signal's name and kind, in name order (the recorder resolves its patterns against this).
+    pub fn signals(&self) -> impl Iterator<Item = (&str, SignalKind)> + '_ {
+        self.names.iter().map(|(name, &(kind, _))| (name.as_str(), kind))
+    }
+
     /// Hash of every signal's name and exact bit pattern, in name order.
     pub fn digest(&self) -> u64 {
         let mut h = Fnv1a::new();
````

Run: `cargo test -p ofs-core`
Expected: PASS, the new test included.

- [ ] **Step 3: The workspace dependencies and the crate's manifest**

The pin comment is part of the contract: it says why this is not crates.io.

**Modify `Cargo.toml`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/Cargo.toml b/Cargo.toml
index 1554d5d..c887abf 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -27,6 +27,11 @@ tracing = "0.1"
 tracing-subscriber = "0.3"
 tempfile = "3"
 proptest = "1"
+# Pinned upstream, not crates.io: mcap 0.25.0 writes the summary's channel and schema records in HashMap order,
+# so identical runs give different files. 00e47a1b (0.25.1, unreleased) writes them in id order
+# (foxglove/mcap#1838). See docs/research/mcap-determinism.md; switch to the release once it ships.
+mcap = { git = "https://github.com/foxglove/mcap.git", rev = "00e47a1b", default-features = false, features = ["zstd"] }
+serde_json = "1"
 ofs-core = { path = "crates/ofs-core" }
 ofs-physics = { path = "crates/ofs-physics" }
 ofs-electrical = { path = "crates/ofs-electrical" }
@@ -38,3 +43,4 @@ ofs-rf = { path = "crates/ofs-rf" }
 ofs-video = { path = "crates/ofs-video" }
 ofs-proto = { path = "crates/ofs-proto" }
 ofs-client = { path = "crates/ofs-client" }
+ofs-log = { path = "crates/ofs-log" }
````

**Create `crates/ofs-log/Cargo.toml`:**

````toml
[package]
name = "ofs-log"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
ofs-core.workspace = true
glam.workspace = true
mcap.workspace = true
serde_json.workspace = true

[dev-dependencies]
tempfile.workspace = true
````

- [ ] **Step 4: The byte-identity tests, before the writer**

`recording(n)` writes `n` channels at mixed divisors for 2 simulated seconds. The values use only IEEE + − × ÷ and `floor`, because libm's `sin` differs between MSVC and glibc in the last bits (see the research note). The 24-channel test is the regression guard for the pin: on crates.io 0.25.0 both tests fail.

**Create `crates/ofs-log/tests/determinism.rs`:**

````rust
//! Spec §9's riskiest point, tested first: identical input writes byte-identical MCAP.
use std::io::Cursor;

use ofs_core::SignalKind;
use ofs_log::sink::{scalar_json, vec3_json, McapSink};
use ofs_log::Channel;

/// `n` channels at mixed divisors, 2 simulated seconds on an 8 kHz tick, values from IEEE + − × ÷ only (libm's
/// `sin` differs between platforms in the last bits).
fn recording(n: usize) -> Vec<u8> {
    let channels: Vec<Channel> = (0..n)
        .map(|i| Channel {
            name: format!("sig.{i:02}"),
            kind: if i % 3 == 0 { SignalKind::Vec3 } else { SignalKind::Scalar },
            divisor: [8, 33, 160, 800][i % 4],
            rate_hz: 8000.0 / [8.0, 33.0, 160.0, 800.0][i % 4],
        })
        .collect();
    let mut out = Cursor::new(Vec::new());
    let mut sink = McapSink::new(&mut out, &channels).unwrap();
    let mut record = Vec::new();
    for end in 1..=16_000u64 {
        for (i, c) in channels.iter().enumerate() {
            if end % c.divisor != 0 {
                continue;
            }
            let p = end as f64 / 8000.0 * (i as f64 + 1.0) / 7.0;
            let x = (p - p.floor()) * 3.7 / 1.3;
            record.clear();
            match c.kind {
                SignalKind::Vec3 => vec3_json(&mut record, glam::DVec3::new(x, -x, x * 0.5)),
                _ => scalar_json(&mut record, x),
            }
            sink.write(i, end * 125_000, &record).unwrap();
        }
    }
    assert!(sink.finish().unwrap().messages > 0);
    out.into_inner()
}

#[test]
fn a_two_signal_recording_is_byte_identical() {
    let (a, b) = (recording(2), recording(2));
    assert!(a.len() > 1000);
    assert_eq!(a, b);
}

/// The regression guard for the mcap pin: crates.io 0.25.0 writes the summary's channels in HashMap order, which
/// differs between two writers in one process as soon as there are a few channels.
#[test]
fn a_many_channel_recording_is_byte_identical() {
    let first = recording(24);
    for _ in 0..4 {
        assert_eq!(recording(24), first, "the summary section's order must not depend on hashing");
    }
}

#[test]
fn records_are_compact_json_objects() {
    let mut out = Vec::new();
    scalar_json(&mut out, 1.0);
    assert_eq!(out, br#"{"v":1.0}"#);
    out.clear();
    vec3_json(&mut out, glam::DVec3::new(0.1, -2.0, 3e-7));
    assert_eq!(std::str::from_utf8(&out).unwrap(), r#"{"x":0.1,"y":-2.0,"z":3e-7}"#);
    out.clear();
    ofs_log::sink::quat_json(&mut out, glam::DQuat::IDENTITY);
    assert_eq!(std::str::from_utf8(&out).unwrap(), r#"{"w":1.0,"x":0.0,"y":0.0,"z":0.0}"#);
    out.clear();
    scalar_json(&mut out, f64::NAN);
    assert_eq!(out, br#"{"v":null}"#, "a non-finite value cannot be JSON; the session fails on it anyway");
}
````

Run: `cargo test -p ofs-log --test determinism`
Expected: compile errors (`ofs_log::sink` unresolved, and no `src/lib.rs` yet).

- [ ] **Step 5: The crate: lib, patterns, the table, the sink and the recorder**

`glob.rs`, `spec.rs` and `sink.rs` carry their unit tests. `recorder.rs` is here because `lib.rs` re-exports it. Its behaviour tests are the next step.

**Create `crates/ofs-log/src/lib.rs`:**

````rust
//! Recording the signal bus to MCAP (M4c). The recorder is a model appended last to the scheduler: it only reads
//! the bus, so a run's digest is the same with and without it, and two lockstep runs with the same seed and inputs
//! write byte-identical files.
pub mod glob;
pub mod recorder;
pub mod sink;
pub mod spec;

pub use recorder::{RecordError, Recorder, RecorderHandle, RecordingSummary};
pub use spec::{Channel, SignalRate, DEFAULT_RATES};
````

**Create `crates/ofs-log/src/glob.rs`:**

````rust
//! Signal-name patterns: `*` matches any run of characters (dots included), `?` exactly one, anything else
//! itself. The Python reader (`ofs.reader`) uses the same rules (`fnmatch.fnmatchcase` without brackets).

/// Whether `name` matches `pattern` as a whole.
pub fn matches(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let (mut pi, mut ni) = (0, 0);
    // The last `*` seen and the name position it was tried at: on a mismatch, let it swallow one more character.
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ni));
            pi += 1;
        } else if let Some((sp, sn)) = star {
            pi = sp + 1;
            ni = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn star_spans_dots_and_question_mark_is_one_character() {
        assert!(matches("imu.*", "imu.gyro_frd_radps"));
        assert!(matches("motor.*.omega_radps", "motor.3.omega_radps"));
        assert!(matches("*", "anything.at.all"));
        assert!(matches("motor.?.current_a", "motor.0.current_a"));
        assert!(!matches("motor.?.current_a", "motor.10.current_a"));
        assert!(matches("motor.*.current_a", "motor.10.current_a"));
        assert!(!matches("imu.*", "fault.imu.gyro_bias_radps"), "anchored at the start");
        assert!(!matches("battery.voltage", "battery.voltage_v"), "anchored at the end");
        assert!(matches("battery.voltage_v", "battery.voltage_v"));
        assert!(matches("*.theta_rad", "motor.2.theta_rad"));
        assert!(matches("a*b*c", "aXXbYYc"));
        assert!(!matches("a*b*c", "aXXbYY"));
        assert!(matches("", "") && !matches("", "x"));
    }
}
````

**Create `crates/ofs-log/src/spec.rs`:**

````rust
//! Which signals a recording carries, at what rate: patterns resolved once against the bus's names at start.
use ofs_core::SignalKind;

use crate::glob;

/// The default table (spec §4): every signal class has a rate; the first matching row wins.
pub const DEFAULT_RATES: &[(&str, f64)] = &[
    ("body.*", 240.0),
    ("imu.*", 1000.0),
    ("motor.*", 240.0),
    ("esc.*", 240.0),
    ("prop.*", 240.0),
    ("battery.*", 10.0),
    ("radio.*", 50.0),
    ("video.*", 50.0),
    ("rc.*", 50.0),
    ("fault.*", 50.0),
    ("*", 50.0),
];

/// One requested pattern. `rate_hz: None` takes each matched signal's rate from [`DEFAULT_RATES`].
#[derive(Debug, Clone, PartialEq)]
pub struct SignalRate {
    pub pattern: String,
    pub rate_hz: Option<f64>,
}

/// A recorded signal: its bus name and kind, and the base-tick divisor its rate became.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub name: String,
    pub kind: SignalKind,
    /// Recorded on ticks whose end time is a multiple of `divisor` base ticks.
    pub divisor: u64,
    /// The rate actually recorded: `base_hz / divisor`.
    pub rate_hz: f64,
}

/// The default table's rate for `name`.
pub fn default_rate(name: &str) -> f64 {
    DEFAULT_RATES.iter().find(|(p, _)| glob::matches(p, name)).map_or(50.0, |&(_, r)| r)
}

/// Up to five signal names that share a literal piece of `pattern` (its dot-separated parts with the wildcards
/// removed): the likely intended signals when a pattern matches nothing. Names sharing more pieces come first, then
/// names that start with the pattern's first piece, then name order.
pub fn near_misses<'a>(pattern: &str, names: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let parts: Vec<String> =
        pattern.split('.').map(|p| p.replace(['*', '?'], "")).filter(|p| !p.is_empty()).collect();
    let mut scored: Vec<(usize, bool, &str)> = names
        .map(|n| (parts.iter().filter(|p| n.contains(p.as_str())).count(), parts.first().is_some_and(|p| n.starts_with(p.as_str())), n))
        .filter(|&(shared, _, _)| shared > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(b.2)));
    scored.into_iter().take(5).map(|(_, _, n)| n).collect()
}

/// Resolves `requests` against the bus's `signals` (name order). No requests: every signal at its default rate.
/// Otherwise each signal takes the first request that matches it, and signals no request matches are not
/// recorded. Every request must match at least one signal; every rate must be finite, > 0 and at most `base_hz`.
/// All problems are collected into one message.
pub fn resolve(requests: &[SignalRate], signals: &[(String, SignalKind)], base_hz: u32) -> Result<Vec<Channel>, String> {
    let mut problems = Vec::new();
    for r in requests {
        if let Some(rate) = r.rate_hz {
            if !rate.is_finite() || rate <= 0.0 || rate > f64::from(base_hz) {
                problems.push(format!("pattern '{}': rate_hz must be in (0, {base_hz}] (got {rate})", r.pattern));
            }
        }
        if !signals.iter().any(|(n, _)| glob::matches(&r.pattern, n)) {
            let near = near_misses(&r.pattern, signals.iter().map(|(n, _)| n.as_str()));
            let hint = if near.is_empty() { String::new() } else { format!(" (similar: {})", near.join(", ")) };
            problems.push(format!("pattern '{}' matches no signal{hint}", r.pattern));
        }
    }
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    let channels = signals
        .iter()
        .filter_map(|(name, kind)| {
            let rate = if requests.is_empty() {
                default_rate(name)
            } else {
                let r = requests.iter().find(|r| glob::matches(&r.pattern, name))?;
                r.rate_hz.unwrap_or_else(|| default_rate(name))
            };
            let divisor = ((f64::from(base_hz) / rate).round() as u64).max(1);
            Some(Channel { name: name.clone(), kind: *kind, divisor, rate_hz: f64::from(base_hz) / divisor as f64 })
        })
        .collect();
    Ok(channels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bus_names() -> Vec<(String, SignalKind)> {
        [
            ("battery.voltage_v", SignalKind::Scalar),
            ("body.att_q", SignalKind::Quat),
            ("body.pos_ned_m", SignalKind::Vec3),
            ("fault.imu.gyro_bias_radps", SignalKind::Vec3),
            ("fc.motor.0.cmd", SignalKind::Scalar),
            ("imu.gyro_frd_radps", SignalKind::Vec3),
            ("motor.0.omega_radps", SignalKind::Scalar),
            ("motor.1.omega_radps", SignalKind::Scalar),
        ]
        .into_iter()
        .map(|(n, k)| (n.to_string(), k))
        .collect()
    }

    fn req(pattern: &str, rate_hz: Option<f64>) -> SignalRate {
        SignalRate { pattern: pattern.into(), rate_hz }
    }

    #[test]
    fn no_requests_record_everything_at_the_default_table_rates() {
        let ch = resolve(&[], &bus_names(), 8000).unwrap();
        let rates: Vec<(&str, u64)> = ch.iter().map(|c| (c.name.as_str(), c.divisor)).collect();
        assert_eq!(
            rates,
            [
                ("battery.voltage_v", 800),
                ("body.att_q", 33),
                ("body.pos_ned_m", 33),
                ("fault.imu.gyro_bias_radps", 160),
                ("fc.motor.0.cmd", 160),
                ("imu.gyro_frd_radps", 8),
                ("motor.0.omega_radps", 33),
                ("motor.1.omega_radps", 33),
            ]
        );
        assert!((ch[1].rate_hz - 8000.0 / 33.0).abs() < 1e-12, "the actual rate is base / divisor");
    }

    #[test]
    fn requests_select_signals_and_the_first_match_sets_the_rate() {
        let ch = resolve(&[req("motor.0.*", Some(1000.0)), req("motor.*", None)], &bus_names(), 8000).unwrap();
        let rates: Vec<(&str, u64)> = ch.iter().map(|c| (c.name.as_str(), c.divisor)).collect();
        assert_eq!(rates, [("motor.0.omega_radps", 8), ("motor.1.omega_radps", 33)]);
    }

    #[test]
    fn a_pattern_matching_nothing_names_its_near_misses_and_all_problems_are_collected() {
        let err = resolve(&[req("gyro.*", None), req("battery.voltage", None), req("imu.*", Some(0.0))], &bus_names(), 8000)
            .unwrap_err();
        assert!(err.contains("pattern 'gyro.*' matches no signal (similar: fault.imu.gyro_bias_radps, imu.gyro_frd_radps)"), "{err}");
        assert!(err.contains("pattern 'battery.voltage' matches no signal (similar: battery.voltage_v)"), "{err}");
        assert!(err.contains("pattern 'imu.*': rate_hz must be in (0, 8000] (got 0)"), "{err}");
        let rpm = resolve(&[req("motor.*.rpm", None)], &bus_names(), 8000).unwrap_err();
        assert!(rpm.contains("(similar: motor.0.omega_radps, motor.1.omega_radps, fc.motor.0.cmd)"), "the first piece's names come first: {rpm}");
        let err = resolve(&[req("zzz", None)], &bus_names(), 8000).unwrap_err();
        assert_eq!(err, "pattern 'zzz' matches no signal");
    }

    #[test]
    fn a_rate_above_the_base_rate_is_refused_and_a_fast_rate_records_every_tick() {
        assert!(resolve(&[req("imu.*", Some(8001.0))], &bus_names(), 8000).is_err());
        let ch = resolve(&[req("imu.*", Some(8000.0))], &bus_names(), 8000).unwrap();
        assert_eq!(ch[0].divisor, 1);
    }
}
````

**Create `crates/ofs-log/src/sink.rs`:**

````rust
//! The deterministic MCAP writer: same channels and records in, same bytes out (docs/research/mcap-determinism.md).
use std::collections::BTreeMap;
use std::io::{Seek, Write};

use glam::{DQuat, DVec3};
use mcap::records::MessageHeader;
use mcap::{Compression, WriteOptions};
use ofs_core::SignalKind;

use crate::spec::Channel;

/// The header's library string, pinned: the crate default carries its version, which would change the bytes on
/// every mcap bump.
pub const LIBRARY: &str = "ofs-log";

/// JSON schemas of the three record shapes, so viewers (Foxglove) can address the fields.
const SCALAR_SCHEMA: &str = r#"{"type":"object","properties":{"v":{"type":"number"}}}"#;
const VEC3_SCHEMA: &str = r#"{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"},"z":{"type":"number"}}}"#;
const QUAT_SCHEMA: &str =
    r#"{"type":"object","properties":{"w":{"type":"number"},"x":{"type":"number"},"y":{"type":"number"},"z":{"type":"number"}}}"#;

/// `{"v":x}` for a scalar. Numbers print with serde_json (shortest round-trip; a non-finite value is `null`).
pub fn scalar_json(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(b"{\"v\":");
    number(out, v);
    out.push(b'}');
}

/// `{"x":..,"y":..,"z":..}` for a vec3.
pub fn vec3_json(out: &mut Vec<u8>, v: DVec3) {
    fields(out, &[("x", v.x), ("y", v.y), ("z", v.z)]);
}

/// `{"w":..,"x":..,"y":..,"z":..}` for a quaternion.
pub fn quat_json(out: &mut Vec<u8>, q: DQuat) {
    fields(out, &[("w", q.w), ("x", q.x), ("y", q.y), ("z", q.z)]);
}

fn fields(out: &mut Vec<u8>, fields: &[(&str, f64)]) {
    out.push(b'{');
    for (i, (k, v)) in fields.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.push(b'"');
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(b"\":");
        number(out, *v);
    }
    out.push(b'}');
}

fn number(out: &mut Vec<u8>, v: f64) {
    serde_json::to_writer(&mut *out, &v).expect("writing to a Vec cannot fail");
}

/// The error and its causes: mcap's own message ("I/O error from writing...") hides the cause ("disk full").
fn describe(e: mcap::McapError) -> String {
    let mut message = e.to_string();
    let mut source = std::error::Error::source(&e);
    while let Some(cause) = source {
        message = format!("{message}: {cause}");
        source = cause.source();
    }
    message
}

/// Counts of what a finished file holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SinkSummary {
    pub messages: u64,
    /// Log time of the first and last message, ns (both 0 when there are none).
    pub first_ns: u64,
    pub last_ns: u64,
}

/// One MCAP file: a channel per recorded signal (topic = bus name, JSON records with a JSON schema), zstd chunks,
/// every option that could vary between machines pinned.
pub struct McapSink<W: Write + Seek> {
    writer: mcap::Writer<W>,
    ids: Vec<u16>,
    sequence: Vec<u32>,
    summary: SinkSummary,
}

impl<W: Write + Seek> McapSink<W> {
    pub fn new(w: W, channels: &[Channel]) -> Result<Self, String> {
        let opts = WriteOptions::new()
            .library(LIBRARY)
            .compression(Some(Compression::Zstd))
            // Single-threaded: identical bytes whatever the machine's core count, and no worker threads.
            .compression_threads(0)
            .emit_message_indexes(false);
        let mut writer = opts.create(w).map_err(describe)?;
        let mut schema = |name: &str, data: &str| writer.add_schema(name, "jsonschema", data.as_bytes());
        let schemas = [
            schema("ofs.Scalar", SCALAR_SCHEMA).map_err(describe)?,
            schema("ofs.Vec3", VEC3_SCHEMA).map_err(describe)?,
            schema("ofs.Quat", QUAT_SCHEMA).map_err(describe)?,
        ];
        let mut ids = Vec::with_capacity(channels.len());
        for c in channels {
            let schema_id = match c.kind {
                SignalKind::Scalar => schemas[0],
                SignalKind::Vec3 => schemas[1],
                SignalKind::Quat => schemas[2],
            };
            let metadata = BTreeMap::from([("rate_hz".to_string(), format!("{}", c.rate_hz))]);
            ids.push(writer.add_channel(schema_id, &c.name, "json", &metadata).map_err(describe)?);
        }
        let sequence = vec![0; ids.len()];
        Ok(Self { writer, ids, sequence, summary: SinkSummary { messages: 0, first_ns: 0, last_ns: 0 } })
    }

    /// Appends one record to channel `index` (the order `new` was given) at `time_ns`.
    pub fn write(&mut self, index: usize, time_ns: u64, data: &[u8]) -> Result<(), String> {
        let header = MessageHeader { channel_id: self.ids[index], sequence: self.sequence[index], log_time: time_ns, publish_time: time_ns };
        self.writer.write_to_known_channel(&header, data).map_err(describe)?;
        self.sequence[index] = self.sequence[index].wrapping_add(1);
        if self.summary.messages == 0 {
            self.summary.first_ns = time_ns;
        }
        self.summary.messages += 1;
        self.summary.last_ns = time_ns;
        Ok(())
    }

    /// Writes the summary section and footer. The file is readable (and indexed) only after this.
    pub fn finish(mut self) -> Result<SinkSummary, String> {
        self.writer.finish().map_err(describe)?;
        Ok(self.summary)
    }
}
````

**Create `crates/ofs-log/src/recorder.rs`:**

````rust
//! The recorder model and the handle the session starts and stops recordings through (the OsdHandle pattern:
//! shared state the vehicle writes and the model reads).
use std::fs::File;
use std::io::{BufWriter, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use glam::{DQuat, DVec3};
use ofs_core::{Bus, Model, Signal, SignalKind, SimError, StepCtx};

use crate::sink::{quat_json, scalar_json, vec3_json, McapSink};
use crate::spec::{resolve, SignalRate};

/// Anything a recording can be written to: a file in the server, a failing writer in tests.
pub trait WriteSeek: Write + Seek + Send {}
impl<T: Write + Seek + Send> WriteSeek for T {}

#[derive(Debug, Clone, PartialEq)]
pub enum RecordError {
    /// StartRecording while one runs.
    AlreadyRecording,
    /// StopRecording with none running.
    NotRecording,
    /// A pattern or rate the bus cannot satisfy (the message lists every problem).
    Spec(String),
    /// Creating, writing or finishing the file failed.
    Io(String),
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordError::AlreadyRecording => write!(f, "a recording is already running; stop it first"),
            RecordError::NotRecording => write!(f, "no recording is running"),
            RecordError::Spec(m) => write!(f, "{m}"),
            RecordError::Io(m) => write!(f, "recording I/O failed: {m}"),
        }
    }
}

/// What a finished recording holds.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordingSummary {
    pub path: PathBuf,
    pub messages: u64,
    /// Simulated time of the first and last record, s (both 0 when nothing was recorded).
    pub first_s: f64,
    pub last_s: f64,
}

#[derive(Clone, Copy)]
enum Source {
    Scalar(Signal<f64>),
    Vec3(Signal<DVec3>),
    Quat(Signal<DQuat>),
}

struct Active {
    sink: McapSink<Box<dyn WriteSeek>>,
    path: PathBuf,
    /// Per channel, in the sink's channel order: where its value lives and its base-tick divisor.
    sources: Vec<(Source, u64)>,
    record: Vec<u8>,
}

#[derive(Default)]
struct State {
    active: Option<Active>,
    /// Why the last recording stopped on its own (a write failed), until the session takes it for its event.
    failure: Option<String>,
    /// The same message, until the next `stop` or `start`: a StopRecording after the failure says why.
    stopped_by: Option<String>,
}

/// Shared between the vehicle (start/stop) and the recorder model (writes).
#[derive(Clone, Default)]
pub struct RecorderHandle(Arc<Mutex<State>>);

/// `<dir>/<stem>-<n>.mcap` with `n` one more than the largest already there (1 in an empty directory). Creates
/// `dir` as needed.
pub fn next_log_path(dir: &Path, stem: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let prefix = format!("{stem}-");
    let mut largest = 0u64;
    for entry in std::fs::read_dir(dir)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if let Some(n) = name.strip_prefix(&prefix).and_then(|rest| rest.strip_suffix(".mcap")).and_then(|n| n.parse::<u64>().ok()) {
            largest = largest.max(n);
        }
    }
    Ok(dir.join(format!("{stem}-{}.mcap", largest + 1)))
}

fn time_ns(tick_end: u64, base_hz: u32) -> u64 {
    (u128::from(tick_end) * 1_000_000_000 / u128::from(base_hz)) as u64
}

impl RecorderHandle {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Starts recording to a new file at `path` (refused if it exists). Returns the recorded signals' names.
    pub fn start(&self, bus: &Bus, base_hz: u32, requests: &[SignalRate], path: &Path) -> Result<Vec<String>, RecordError> {
        if self.is_recording() {
            return Err(RecordError::AlreadyRecording);
        }
        // Validate before creating the file: a refused request leaves nothing behind.
        let signals: Vec<(String, SignalKind)> = bus.signals().map(|(n, k)| (n.to_string(), k)).collect();
        resolve(requests, &signals, base_hz).map_err(RecordError::Spec)?;
        let file = File::create_new(path).map_err(|e| RecordError::Io(format!("{}: {e}", path.display())))?;
        self.start_with(bus, base_hz, requests, Box::new(BufWriter::new(file)), path)
    }

    /// `start` with the writer given (tests use a failing one). `path` is only reported back.
    pub fn start_with(
        &self,
        bus: &Bus,
        base_hz: u32,
        requests: &[SignalRate],
        writer: Box<dyn WriteSeek>,
        path: &Path,
    ) -> Result<Vec<String>, RecordError> {
        let mut state = self.lock();
        if state.active.is_some() {
            return Err(RecordError::AlreadyRecording);
        }
        let signals: Vec<(String, SignalKind)> = bus.signals().map(|(n, k)| (n.to_string(), k)).collect();
        let channels = resolve(requests, &signals, base_hz).map_err(RecordError::Spec)?;
        let sources = channels
            .iter()
            .map(|c| {
                let source = match c.kind {
                    SignalKind::Scalar => Source::Scalar(bus.lookup(&c.name).expect("listed by the bus")),
                    SignalKind::Vec3 => Source::Vec3(bus.lookup(&c.name).expect("listed by the bus")),
                    SignalKind::Quat => Source::Quat(bus.lookup(&c.name).expect("listed by the bus")),
                };
                (source, c.divisor)
            })
            .collect();
        let sink = McapSink::new(writer, &channels).map_err(RecordError::Io)?;
        state.active = Some(Active { sink, path: path.to_path_buf(), sources, record: Vec::with_capacity(96) });
        state.failure = None;
        state.stopped_by = None;
        Ok(channels.into_iter().map(|c| c.name).collect())
    }

    /// Finishes the running recording's file and returns what it holds. After a recording stopped on its own, the
    /// first `stop` reports its write error instead of `NotRecording`.
    pub fn stop(&self) -> Result<RecordingSummary, RecordError> {
        let mut state = self.lock();
        let Some(active) = state.active.take() else {
            return Err(state.stopped_by.take().map_or(RecordError::NotRecording, RecordError::Io));
        };
        drop(state);
        let s = active.sink.finish().map_err(RecordError::Io)?;
        let secs = |ns: u64| ns as f64 / 1e9;
        Ok(RecordingSummary { path: active.path, messages: s.messages, first_s: secs(s.first_ns), last_s: secs(s.last_ns) })
    }

    pub fn is_recording(&self) -> bool {
        self.lock().active.is_some()
    }

    /// The message of a recording that stopped because a write failed (once; `None` afterwards).
    pub fn take_failure(&self) -> Option<String> {
        self.lock().failure.take()
    }

    /// Writes the channels due at the end of tick `tick` (records carry that time: the state `GetState` reports
    /// after the tick). A failed write stops the recording and is kept for `take_failure`; it never fails the step.
    fn record(&self, tick: u64, base_hz: u32, bus: &Bus) {
        let mut state = self.lock();
        let Some(active) = state.active.as_mut() else { return };
        let end = tick + 1;
        let t = time_ns(end, base_hz);
        let mut failed = None;
        for (i, (source, divisor)) in active.sources.iter().enumerate() {
            if end % divisor != 0 {
                continue;
            }
            active.record.clear();
            match *source {
                Source::Scalar(s) => scalar_json(&mut active.record, bus.get(s)),
                Source::Vec3(s) => vec3_json(&mut active.record, bus.get(s)),
                Source::Quat(s) => quat_json(&mut active.record, bus.get(s)),
            }
            if let Err(e) = active.sink.write(i, t, &active.record) {
                failed = Some(format!("{}: {e}", active.path.display()));
                break;
            }
        }
        if let Some(message) = failed {
            state.active = None; // dropping the sink tries to finish the file; the error is already reported
            state.failure = Some(message.clone());
            state.stopped_by = Some(message);
        }
    }
}

/// The recorder model: last in the tick, it reads every recorded signal and writes nothing to the bus.
pub struct Recorder {
    handle: RecorderHandle,
    base_hz: u32,
}

impl Recorder {
    pub fn new(handle: RecorderHandle, base_hz: u32) -> Self {
        Self { handle, base_hz }
    }
}

impl Model for Recorder {
    fn name(&self) -> &str {
        "recorder"
    }

    fn rate_divisor(&self) -> u32 {
        1
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        self.handle.record(ctx.tick, self.base_hz, bus);
        Ok(())
    }
}
````

Run (without `--locked`, so `Cargo.lock` takes the new crates): `cargo test -p ofs-log --lib --test determinism`
Expected: PASS: `glob`, `spec` and `determinism` (3 tests).

- [ ] **Step 6: The recorder's behaviour tests**

These cover state-time stamping, the digest staying unchanged, refusals that create no file, a failing writer, and file naming.

**Create `crates/ofs-log/tests/recorder.rs`:**

````rust
use std::io::{Cursor, Seek, SeekFrom, Write};

use glam::DVec3;
use ofs_core::{Bus, Model, Scheduler, Signal, SimError, StepCtx};
use ofs_log::recorder::next_log_path;
use ofs_log::{RecordError, Recorder, RecorderHandle, SignalRate};

/// Writes `t` (simulated seconds after the step) into `x` and `t·(1, 2, 3)` into `v` every tick.
struct Clock {
    x: Signal<f64>,
    v: Signal<DVec3>,
}

impl Model for Clock {
    fn name(&self) -> &str {
        "clock"
    }
    fn rate_divisor(&self) -> u32 {
        1
    }
    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let t = ctx.time_s + ctx.dt_s;
        bus.set(self.x, t);
        bus.set(self.v, DVec3::new(t, 2.0 * t, 3.0 * t));
        Ok(())
    }
}

/// An 8 kHz scheduler with the clock and, last, a recorder.
fn rig() -> (Scheduler, RecorderHandle) {
    let mut bus = Bus::new();
    let clock = Clock { x: bus.signal("clock.x"), v: bus.signal("clock.v") };
    let handle = RecorderHandle::default();
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(clock));
    s.add(Box::new(Recorder::new(handle.clone(), 8000)));
    (s, handle)
}

fn req(pattern: &str, rate_hz: Option<f64>) -> SignalRate {
    SignalRate { pattern: pattern.into(), rate_hz }
}

#[test]
fn a_recording_carries_each_channel_at_its_rate_stamped_with_the_state_time() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, h) = rig();
    let path = dir.path().join("r.mcap");
    let names = h.start(s.bus(), 8000, &[req("clock.x", Some(1000.0)), req("clock.v", Some(100.0))], &path).unwrap();
    assert_eq!(names, ["clock.v", "clock.x"]);
    s.run_for(0.1).unwrap();
    let summary = h.stop().unwrap();
    assert_eq!(summary.path, path);
    assert_eq!(summary.messages, 100 + 10, "0.1 s at 1 kHz and at 100 Hz");
    assert!((summary.first_s - 0.001).abs() < 1e-12, "{summary:?}");
    assert!((summary.last_s - 0.1).abs() < 1e-12, "{summary:?}");
    assert!(!h.is_recording());
    // The bytes are an MCAP file: magic at both ends.
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..8], b"\x89MCAP0\r\n");
    assert_eq!(&bytes[bytes.len() - 8..], b"\x89MCAP0\r\n");
}

#[test]
fn recording_never_changes_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let (mut with, h) = rig();
    h.start(with.bus(), 8000, &[], &dir.path().join("r.mcap")).unwrap();
    with.run_for(0.05).unwrap();
    let (mut without, _) = rig();
    without.run_for(0.05).unwrap();
    assert_eq!(with.bus().digest(), without.bus().digest());
}

#[test]
fn start_and_stop_refuse_the_wrong_state_and_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let (s, h) = rig();
    assert_eq!(h.stop().unwrap_err(), RecordError::NotRecording);
    let path = dir.path().join("r.mcap");
    h.start(s.bus(), 8000, &[], &path).unwrap();
    assert_eq!(h.start(s.bus(), 8000, &[], &dir.path().join("other.mcap")).unwrap_err(), RecordError::AlreadyRecording);
    h.stop().unwrap();
    assert!(matches!(h.start(s.bus(), 8000, &[], &path), Err(RecordError::Io(m)) if m.contains("r.mcap")), "never overwrites");
    match h.start(s.bus(), 8000, &[req("gyro.*", None)], &dir.path().join("x.mcap")) {
        Err(RecordError::Spec(m)) => assert_eq!(m, "pattern 'gyro.*' matches no signal"),
        other => panic!("{other:?}"),
    }
    assert!(!dir.path().join("x.mcap").exists(), "a refused request creates no file");
}

/// Accepts `limit` bytes, then fails every write.
struct Failing {
    inner: Cursor<Vec<u8>>,
    limit: usize,
}

impl Write for Failing {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.inner.get_ref().len() + buf.len() > self.limit {
            return Err(std::io::Error::other("disk full"));
        }
        self.inner.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Seek for Failing {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

#[test]
fn a_failed_write_stops_the_recording_once_and_the_run_flies_on() {
    let (mut s, h) = rig();
    let writer = Box::new(Failing { inner: Cursor::new(Vec::new()), limit: 4096 });
    h.start_with(s.bus(), 8000, &[req("clock.*", Some(8000.0))], writer, std::path::Path::new("disk.mcap")).unwrap();
    // A chunk reaches the writer when it fills (1 MiB of records): about 1.3 simulated seconds at these rates.
    s.run_for(5.0).unwrap();
    assert!(!h.is_recording(), "the recording stopped itself");
    let failure = h.take_failure().expect("the failure is kept for the session");
    assert!(failure.contains("disk.mcap") && failure.contains("disk full"), "{failure}");
    assert_eq!(h.take_failure(), None, "reported once");
    match h.stop() {
        Err(RecordError::Io(m)) => assert_eq!(m, failure, "a StopRecording after the failure says why"),
        other => panic!("{other:?}"),
    }
    assert_eq!(h.stop().unwrap_err(), RecordError::NotRecording, "then there is nothing to stop");
}

#[test]
fn log_paths_count_up_per_quad() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    assert_eq!(next_log_path(&logs, "quad").unwrap(), logs.join("quad-1.mcap"));
    std::fs::write(logs.join("quad-1.mcap"), b"").unwrap();
    std::fs::write(logs.join("quad-7.mcap"), b"").unwrap();
    std::fs::write(logs.join("other-9.mcap"), b"").unwrap();
    std::fs::write(logs.join("quad-x.mcap"), b"").unwrap();
    assert_eq!(next_log_path(&logs, "quad").unwrap(), logs.join("quad-8.mcap"));
    assert_eq!(next_log_path(&logs, "other").unwrap(), logs.join("other-10.mcap"));
}
````

Run: `cargo test -p ofs-log`
Expected: PASS, 5 unit + 3 determinism + 5 recorder tests.

Prove the pin matters. This is a temporary edit, reverted at once:
```bash
cp Cargo.toml Cargo.toml.bak && cp Cargo.lock Cargo.lock.bak
sed -i 's|^mcap = { git = .*|mcap = { version = "=0.25.0", default-features = false, features = ["zstd"] }|' Cargo.toml
CARGO_TARGET_DIR=target/mcap-0250 cargo test -p ofs-log --test determinism   # expected: both byte-identity tests FAIL
mv Cargo.toml.bak Cargo.toml && mv Cargo.lock.bak Cargo.lock   # never `git checkout` here: it would drop this task's edits
```

- [ ] **Step 7: Lint and commit**

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`. Expected: no output.

```bash
git add Cargo.toml Cargo.lock crates/ofs-core crates/ofs-log
git commit -m "feat(log): ofs-log - the deterministic MCAP recorder (pinned upstream mcap, byte-identity tests first)"
```

---

### Task 2: The recorder in the vehicle: last in the tick, byte-identical lockstep recordings

The vehicle appends the recorder after every other model and exposes start and stop. This upgrades the §8.4 determinism test from the bus digest to the file's bytes; the digest test stays as the faster unit.

**Files:**
- Create: `crates/ofs-sim/tests/recording.rs`
- Modify: `crates/ofs-sim/Cargo.toml`
- Modify: `crates/ofs-sim/src/vehicle.rs`
- Regenerate (never hand-edit): `Cargo.lock`

**Interfaces:**
- Consumes from Task 1: `RecorderHandle`, `Recorder`, `SignalRate`, `RecordError`, `RecordingSummary`.
- Produces for Task 4: `Vehicle::start_recording(&mut self, &[SignalRate], &Path) -> Result<Vec<String>, RecordError>`, `stop_recording() -> Result<RecordingSummary, RecordError>`, `is_recording() -> bool` and `take_recording_failure() -> Option<String>`.

- [ ] **Step 1: The tests**

`recording.rs` reads records back with the `mcap` crate, so `ofs-sim` gains it as a dev-dependency. That edit is in the same `Cargo.toml` hunk as the runtime dependency, which Step 2 needs anyway.

**Create `crates/ofs-sim/tests/recording.rs`:**

````rust
//! The bus recorder in a real vehicle (spec §4, §8.4): recording never changes a run, two lockstep runs with the
//! same seed and inputs write byte-identical files, and a record carries the state at its time.
use std::path::{Path, PathBuf};

use ofs_config::{load, FcKind, WorldConfig};
use ofs_log::SignalRate;
use ofs_sim::vehicle::{build, BuildOptions, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn vehicle(seed: u64) -> Vehicle {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    build(&cfg, &opts).unwrap()
}

/// The scripted flight of the digest test (throttle steps), recorded with the default table into `path`.
fn recorded_flight(seed: u64, path: &Path) -> Vehicle {
    let mut v = vehicle(seed);
    v.start_recording(&[], path).unwrap();
    for (t, secs) in [(0.0, 0.5), (0.6, 1.0), (0.2, 0.5)] {
        v.set_sticks(&Sticks { throttle: t, ..Sticks::default() });
        v.run_for(secs).unwrap();
    }
    v.stop_recording().unwrap();
    v
}

#[test]
fn two_lockstep_runs_write_byte_identical_recordings() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b, c) = (dir.path().join("a.mcap"), dir.path().join("b.mcap"), dir.path().join("c.mcap"));
    recorded_flight(1, &a);
    recorded_flight(1, &b);
    recorded_flight(2, &c);
    let (a, b, c) = (std::fs::read(a).unwrap(), std::fs::read(b).unwrap(), std::fs::read(c).unwrap());
    assert!(a.len() > 10_000, "{} bytes", a.len());
    assert!(a == b, "same seed and inputs: the files differ");
    assert!(a != c, "another seed's noise must show in the file");
}

#[test]
fn recording_leaves_the_digest_alone() {
    let dir = tempfile::tempdir().unwrap();
    let recorded = recorded_flight(1, &dir.path().join("r.mcap"));
    let mut plain = vehicle(1);
    for (t, secs) in [(0.0, 0.5), (0.6, 1.0), (0.2, 0.5)] {
        plain.set_sticks(&Sticks { throttle: t, ..Sticks::default() });
        plain.run_for(secs).unwrap();
    }
    assert_eq!(recorded.digest(), plain.digest());
}

/// Every record of `topic` in the file as (log time ns, JSON text).
fn records(path: &PathBuf, topic: &str) -> Vec<(u64, String)> {
    let bytes = std::fs::read(path).unwrap();
    mcap::MessageStream::new(&bytes)
        .unwrap()
        .map(|m| m.unwrap())
        .filter(|m| m.channel.topic == topic)
        .map(|m| (m.log_time, String::from_utf8(m.data.into_owned()).unwrap()))
        .collect()
}

#[test]
fn a_record_carries_the_state_reported_at_its_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.mcap");
    let mut v = vehicle(1);
    let names = v.start_recording(&[SignalRate { pattern: "battery.voltage_v".into(), rate_hz: Some(1000.0) }], &path).unwrap();
    assert_eq!(names, ["battery.voltage_v"]);
    v.set_sticks(&Sticks { throttle: 0.8, ..Sticks::default() });
    v.run_for(0.25).unwrap();
    let state = v.state();
    let summary = v.stop_recording().unwrap();
    assert_eq!(summary.messages, 250);
    assert!((summary.last_s - state.time_s).abs() < 1e-12, "{summary:?} vs {}", state.time_s);
    let all = records(&path, "battery.voltage_v");
    let (t, json) = all.last().unwrap();
    assert_eq!(*t, 250_000_000);
    let v: f64 = json.strip_prefix(r#"{"v":"#).unwrap().strip_suffix('}').unwrap().parse().unwrap();
    assert_eq!(v, state.battery_voltage_v, "the last record is the state GetState reports at 0.25 s");
}

#[test]
fn ending_the_session_mid_recording_still_finishes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.mcap");
    let mut v = vehicle(1);
    v.start_recording(&[], &path).unwrap();
    v.run_for(0.2).unwrap();
    drop(v); // Unload, a new Load or a server shutdown: the vehicle goes, and its recording with it
    let bytes = std::fs::read(&path).unwrap();
    let summary = mcap::Summary::read(&bytes).unwrap().expect("a summary section: the file was finished");
    assert!(summary.stats.unwrap().message_count > 1000);
}
````

**Modify `crates/ofs-sim/Cargo.toml`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-sim/Cargo.toml b/crates/ofs-sim/Cargo.toml
index 8bbff6d..300997c 100644
--- a/crates/ofs-sim/Cargo.toml
+++ b/crates/ofs-sim/Cargo.toml
@@ -16,6 +16,7 @@ ofs-radio.workspace = true
 ofs-rf.workspace = true
 ofs-proto.workspace = true
 ofs-video.workspace = true
+ofs-log.workspace = true
 glam.workspace = true
 tonic.workspace = true
 tokio.workspace = true
@@ -25,5 +26,6 @@ tracing.workspace = true
 tracing-subscriber.workspace = true
 
 [dev-dependencies]
+mcap.workspace = true
 ofs-client = { path = "../ofs-client" }
 tempfile.workspace = true
````

Run: `cargo test -p ofs-sim --test recording`
Expected: compile errors, `no method named start_recording`.

- [ ] **Step 2: The vehicle owns a recorder**

**Modify `crates/ofs-sim/src/vehicle.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-sim/src/vehicle.rs b/crates/ofs-sim/src/vehicle.rs
index 68f33e3..e995cd0 100644
--- a/crates/ofs-sim/src/vehicle.rs
+++ b/crates/ofs-sim/src/vehicle.rs
@@ -12,6 +12,7 @@ use ofs_core::{names, Bus, Model, Scheduler, Signal, SimError, Wire};
 use ofs_electrical::battery::{Battery, BatteryParams};
 use ofs_electrical::esc_motor::{EscMotor, EscParams, MotorParams};
 use ofs_fc::open_loop::OpenLoopFc;
+use ofs_log::{RecordError, Recorder, RecorderHandle, RecordingSummary, SignalRate};
 use ofs_fc::sitl::bridge::{BridgeConfig, SerialLink, SerialTap, SitlBridge};
 use ofs_fc::sitl::codec::MSP_TCP_PORT;
 use ofs_fc::sitl::esc_telemetry::EscTelemetry;
@@ -440,6 +441,8 @@ pub struct Vehicle {
     has_vtx: bool,
     /// The active faults' names in injection order; `active_fault_names` sorts them for the events.
     active_faults: Vec<String>,
+    /// The recorder model's handle: StartRecording/StopRecording go through it.
+    recorder: RecorderHandle,
 }
 
 /// Per-quad firmware directory: `<data_dir>/<quad file stem>-<hash of the quad file's path>`, so quads with
@@ -814,6 +817,10 @@ pub fn build(cfg: &QuadConfig, opts: &BuildOptions) -> Result<Vehicle, SimError>
         models.push(Box::new(VideoLink::new(params, base_hz / FIELD_RATE_HZ, opts.seed, vtx_interference.clone(), &mut bus)));
     }
 
+    // Last of all: the recorder reads the tick's final values and writes nothing to the bus.
+    let recorder = RecorderHandle::default();
+    models.push(Box::new(Recorder::new(recorder.clone(), base_hz)));
+
     let h = Handles::register(&mut bus, n, &opts.world);
     let mut scheduler = Scheduler::new(base_hz, bus);
     for m in models {
@@ -833,6 +840,7 @@ pub fn build(cfg: &QuadConfig, opts: &BuildOptions) -> Result<Vehicle, SimError>
         battery_cells: cfg.battery.cells,
         has_vtx: cfg.vtx.is_some(),
         active_faults: Vec::new(),
+        recorder,
     };
     // The bus starts every signal at zero; aux 0.0 would reach Betaflight as 1500 us until the first SetSticks.
     vehicle.set_sticks(&Sticks::default());
@@ -1104,6 +1112,26 @@ impl Vehicle {
     pub fn digest(&self) -> u64 {
         self.scheduler.bus().digest()
     }
+
+    /// Starts recording the bus to a new MCAP file at `path` (the server names it). No requests: every signal at
+    /// the default table's rate. Returns the recorded signals' names.
+    pub fn start_recording(&mut self, requests: &[SignalRate], path: &Path) -> Result<Vec<String>, RecordError> {
+        self.recorder.start(self.scheduler.bus(), self.scheduler.base_hz(), requests, path)
+    }
+
+    /// Finishes the running recording's file.
+    pub fn stop_recording(&mut self) -> Result<RecordingSummary, RecordError> {
+        self.recorder.stop()
+    }
+
+    pub fn is_recording(&self) -> bool {
+        self.recorder.is_recording()
+    }
+
+    /// Why the last recording stopped on its own (a write failed), reported once.
+    pub fn take_recording_failure(&mut self) -> Option<String> {
+        self.recorder.take_failure()
+    }
 }
 
 /// Space-separated argv from an environment variable, if set and non-empty.
````

Run: `cargo test -p ofs-sim`
Expected: PASS: every existing test unchanged, plus the 4 recording tests.

- [ ] **Step 3: Lint and commit**

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`. Expected: no output.

```bash
git add crates/ofs-sim
git commit -m "feat(sim): the recorder in the vehicle - last in the tick, byte-identical lockstep recordings"
```

---

### Task 3: Scheduled faults and scripted inputs: applied on their tick, whatever the `Run` chunking

This task is pure simulator: no protocol yet. Three changes:
- `set_fault`'s validation moves into `check_fault`, every message unchanged, so a fault can be validated when it is scheduled.
- The vehicle gains pending faults and a playing inputs file, applied right after each step that reaches their tick.
- `run_for` routes through `step_ticks` so the hook can't be skipped.

**Files:**
- Create: `crates/ofs-sim/src/inputs.rs`
- Create: `crates/ofs-sim/tests/scripted.rs`
- Modify: `crates/ofs-core/src/scheduler.rs`
- Modify: `crates/ofs-sim/src/lib.rs`
- Modify: `crates/ofs-sim/src/vehicle.rs`

**Interfaces:**
- Produces for Task 4:
  - `ofs_core::Scheduler::ticks_for(&self, seconds) -> Result<u64, SimError>`.
  - `ofs_sim::inputs::{parse(name, text) -> Result<Inputs, String>, Inputs { name, columns: Vec<Stick>, rows: Vec<(f64, Vec<f64>)> }, Inputs::duration_s(), Stick, COLUMNS, tick_at(t_s, base_hz) -> u64}`.
  - `Vehicle::schedule_fault(Fault, at_s) -> Result<(), SimError>`, `play_inputs(Inputs) -> Result<(), SimError>`, `stop_inputs() -> Option<String>`, `inputs_playing() -> Option<&str>` and `take_finished_inputs() -> Option<String>`.

- [ ] **Step 1: The vehicle-level tests**

**Create `crates/ofs-sim/tests/scripted.rs`:**

````rust
//! Scheduled faults (`at=`) and scripted inputs (spec §5, §6.2): applied on their tick, whatever the Run chunking.
use std::path::Path;

use ofs_config::{load, FcKind, WorldConfig};
use ofs_core::SimError;
use ofs_sim::inputs;
use ofs_sim::vehicle::{build, BuildOptions, Fault, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn vehicle() -> Vehicle {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    build(&cfg, &opts).unwrap()
}

fn dead_motor_2() -> Fault {
    Fault::MotorFailure { motor: 2, scale: 0.0 }
}

/// Full throttle, motor 2 scheduled to die at 0.5 s, run for 1 s in the given chunks.
fn chunked(chunks: &[f64]) -> Vehicle {
    let mut v = vehicle();
    v.set_sticks(&Sticks { throttle: 1.0, ..Sticks::default() });
    v.schedule_fault(dead_motor_2(), 0.5).unwrap();
    for c in chunks {
        v.run_for(*c).unwrap();
    }
    v
}

#[test]
fn a_scheduled_fault_lands_on_its_tick_however_the_run_is_cut() {
    let whole = chunked(&[1.0]);
    let cut = chunked(&[0.3, 0.2, 0.05, 0.45]);
    assert_eq!(whole.digest(), cut.digest());
    let mut never = vehicle();
    never.set_sticks(&Sticks { throttle: 1.0, ..Sticks::default() });
    never.run_for(1.0).unwrap();
    assert_ne!(whole.digest(), never.digest(), "the fault changed the flight");

    let mut v = vehicle();
    v.set_sticks(&Sticks { throttle: 1.0, ..Sticks::default() });
    v.schedule_fault(dead_motor_2(), 0.5).unwrap();
    v.run_for(0.499_875).unwrap(); // tick 3999
    assert!(v.active_fault_names().is_empty(), "not yet");
    v.step_ticks(1).unwrap(); // 0.5 s: the state at 0.5 s shows the fault
    assert_eq!(v.active_fault_names(), ["motor_failure"]);
}

#[test]
fn a_scheduled_fault_is_validated_now_and_a_passed_time_is_refused() {
    let mut v = vehicle();
    v.run_for(1.0).unwrap();
    let passed = v.schedule_fault(dead_motor_2(), 0.5).unwrap_err();
    assert_eq!(passed, SimError::InvalidArgument("fault time 0.5 s has passed (the session is at 1 s)".into()));
    let bad = v.schedule_fault(Fault::MotorFailure { motor: 9, scale: 0.0 }, 2.0).unwrap_err();
    assert_eq!(bad, SimError::InvalidArgument("motor 9 does not exist (the quad has 4)".into()));
    assert!(matches!(v.schedule_fault(dead_motor_2(), f64::NAN), Err(SimError::InvalidArgument(_))));
    v.schedule_fault(dead_motor_2(), 1.0).unwrap();
    assert_eq!(v.active_fault_names(), ["motor_failure"], "now applies at once");
}

#[test]
fn clearing_faults_drops_the_scheduled_ones() {
    let mut v = vehicle();
    v.schedule_fault(dead_motor_2(), 0.2).unwrap();
    v.clear_faults();
    v.run_for(0.5).unwrap();
    assert!(v.active_fault_names().is_empty());
}

fn csv(text: &str) -> inputs::Inputs {
    inputs::parse("test.csv", text).unwrap()
}

#[test]
fn inputs_rows_apply_on_their_tick_counted_from_the_start_of_playback() {
    let mut v = vehicle();
    v.run_for(0.2).unwrap();
    v.play_inputs(csv("t_s,throttle\n0,0\n0.5,1\n")).unwrap();
    assert_eq!(v.inputs_playing(), Some("test.csv"));
    v.run_for(0.49).unwrap();
    assert_eq!(v.state().motor_cmd, [0.0; 4], "row 0.5 is due at 0.7 s");
    v.run_for(0.02).unwrap();
    assert_eq!(v.state().motor_cmd, [1.0; 4]);
    assert_eq!(v.inputs_playing(), None, "the last row applied: playback is over");
    assert_eq!(v.take_finished_inputs().as_deref(), Some("test.csv"));
    assert_eq!(v.take_finished_inputs(), None, "reported once");
    v.run_for(0.2).unwrap();
    assert_eq!(v.state().motor_cmd, [1.0; 4], "the last row holds");
}

#[test]
fn playback_is_the_same_however_the_run_is_cut_and_equals_setting_the_sticks() {
    let text = "t_s,throttle,roll\n0,0.4,0\n0.25,0.7,0.2\n0.6,0.5,-0.1\n";
    let mut whole = vehicle();
    whole.play_inputs(csv(text)).unwrap();
    whole.run_for(1.0).unwrap();
    let mut cut = vehicle();
    cut.play_inputs(csv(text)).unwrap();
    for _ in 0..10 {
        cut.run_for(0.1).unwrap();
    }
    assert_eq!(whole.digest(), cut.digest());

    let mut by_hand = vehicle();
    for (sticks, secs) in [((0.4, 0.0), 0.25), ((0.7, 0.2), 0.35), ((0.5, -0.1), 0.4)] {
        by_hand.set_sticks(&Sticks { throttle: sticks.0, roll: sticks.1, ..Sticks::default() });
        by_hand.run_for(secs).unwrap();
    }
    assert_eq!(whole.digest(), by_hand.digest(), "a file is the same as the SetSticks calls it stands for");
}

#[test]
fn columns_the_file_leaves_out_keep_their_value() {
    let armed = Sticks { aux: [1.0, 1.0, -1.0, -1.0], ..Sticks::default() };
    let mut played = vehicle();
    played.set_sticks(&armed);
    played.play_inputs(csv("t_s,throttle\n0,0.5\n")).unwrap();
    played.run_for(0.3).unwrap();
    let mut by_hand = vehicle();
    by_hand.set_sticks(&Sticks { throttle: 0.5, ..armed });
    by_hand.run_for(0.3).unwrap();
    assert_eq!(played.digest(), by_hand.digest());
}

#[test]
fn a_second_file_waits_for_the_first_and_stop_keeps_the_sticks() {
    let mut v = vehicle();
    v.play_inputs(csv("t_s,throttle\n0,1\n5,0\n")).unwrap();
    let busy = v.play_inputs(csv("t_s,yaw\n0,0\n")).unwrap_err();
    assert_eq!(busy, SimError::InvalidArgument("test.csv is still playing; stop it first".into()));
    v.run_for(0.1).unwrap();
    assert_eq!(v.stop_inputs().as_deref(), Some("test.csv"));
    assert_eq!(v.stop_inputs(), None);
    assert_eq!(v.take_finished_inputs(), None, "stopped, not finished");
    v.run_for(0.1).unwrap();
    assert_eq!(v.state().motor_cmd, [1.0; 4], "the sticks keep the last applied row");
}
````

Run: `cargo test -p ofs-sim --test scripted`
Expected: compile errors (`ofs_sim::inputs` and `schedule_fault` don't exist yet).

- [ ] **Step 2: The parser and the tick rule, with their unit tests**

**Create `crates/ofs-sim/src/inputs.rs`:**

````rust
//! Scripted stick inputs (spec §5): a CSV of `t_s` and stick columns, validated once, played back
//! sample-and-hold at tick boundaries.
use ofs_core::names::RC_AUX_COUNT;

/// The columns an inputs file may have; `t_s` is required, the sticks are any non-empty subset.
pub const COLUMNS: [&str; 9] = ["t_s", "roll", "pitch", "yaw", "throttle", "aux1", "aux2", "aux3", "aux4"];

/// Problems listed in one error before the rest are counted.
const MAX_PROBLEMS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stick {
    Roll,
    Pitch,
    Yaw,
    Throttle,
    /// 0-based aux channel (`aux1` is `Aux(0)`).
    Aux(usize),
}

impl Stick {
    fn from_column(name: &str) -> Option<Stick> {
        Some(match name {
            "roll" => Stick::Roll,
            "pitch" => Stick::Pitch,
            "yaw" => Stick::Yaw,
            "throttle" => Stick::Throttle,
            _ => {
                let n: usize = name.strip_prefix("aux")?.parse().ok()?;
                if !(1..=RC_AUX_COUNT).contains(&n) {
                    return None;
                }
                Stick::Aux(n - 1)
            }
        })
    }

    /// The documented range: throttle [0, 1], every other stick and switch [-1, 1].
    fn range(self) -> (f64, f64) {
        match self {
            Stick::Throttle => (0.0, 1.0),
            _ => (-1.0, 1.0),
        }
    }
}

/// A validated inputs file: the stick columns it sets and, per row, the time (s after playback starts) and one
/// value per column.
#[derive(Debug, Clone, PartialEq)]
pub struct Inputs {
    pub name: String,
    pub columns: Vec<Stick>,
    pub rows: Vec<(f64, Vec<f64>)>,
}

impl Inputs {
    /// When the last row applies, s after playback starts.
    pub fn duration_s(&self) -> f64 {
        self.rows.last().map_or(0.0, |r| r.0)
    }
}

/// Parses and validates an inputs file. Blank lines and lines starting with `#` are skipped; the first other
/// line is the header. A spreadsheet's UTF-8 byte-order mark and CRLF line endings are accepted. Every problem is
/// collected, each naming `name` and its line (1-based).
pub fn parse(name: &str, text: &str) -> Result<Inputs, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut problems = Vec::new();
    let mut lines = text.lines().enumerate().map(|(i, l)| (i + 1, l.trim())).filter(|(_, l)| !l.is_empty() && !l.starts_with('#'));
    let Some((header_line, header)) = lines.next() else {
        return Err(format!("{name}: no header line (expected t_s and stick columns from {})", COLUMNS.join(", ")));
    };
    let fields: Vec<&str> = header.split(',').map(str::trim).collect();
    let mut time_index = None;
    let mut columns: Vec<(usize, Stick)> = Vec::new();
    for (i, f) in fields.iter().enumerate() {
        if *f == "t_s" {
            if time_index.replace(i).is_some() {
                problems.push(format!("{name} line {header_line}: column t_s appears twice"));
            }
        } else if let Some(stick) = Stick::from_column(f) {
            if columns.iter().any(|(_, s)| *s == stick) {
                problems.push(format!("{name} line {header_line}: column {f} appears twice"));
            }
            columns.push((i, stick));
        } else {
            problems.push(format!("{name} line {header_line}: unknown column '{f}' (expected {})", COLUMNS.join(", ")));
        }
    }
    if time_index.is_none() {
        problems.push(format!("{name} line {header_line}: the header needs a t_s column"));
    }
    if columns.is_empty() {
        problems.push(format!("{name} line {header_line}: the header needs at least one stick column"));
    }
    let mut rows = Vec::new();
    let mut last_t: Option<f64> = None;
    for (line, l) in lines {
        let values: Vec<&str> = l.split(',').map(str::trim).collect();
        if values.len() != fields.len() {
            problems.push(format!("{name} line {line}: {} values for {} columns", values.len(), fields.len()));
            continue;
        }
        let parsed: Vec<Option<f64>> = values.iter().map(|v| v.parse::<f64>().ok().filter(|x| x.is_finite())).collect();
        for (v, p) in values.iter().zip(&parsed) {
            if p.is_none() {
                problems.push(format!("{name} line {line}: '{v}' is not a finite number"));
            }
        }
        let Some(ti) = time_index else { continue };
        let Some(t) = parsed[ti] else { continue };
        if t < 0.0 {
            problems.push(format!("{name} line {line}: t_s {t} is negative"));
        }
        if let Some(prev) = last_t {
            if t <= prev {
                problems.push(format!("{name} line {line}: t_s {t} does not increase (previous row {prev})"));
            }
        }
        last_t = Some(t);
        let mut row = Vec::with_capacity(columns.len());
        for (i, stick) in &columns {
            let Some(v) = parsed[*i] else { continue };
            let (lo, hi) = stick.range();
            if !(lo..=hi).contains(&v) {
                problems.push(format!("{name} line {line}: {} {v} is outside [{lo}, {hi}]", fields[*i]));
            }
            row.push(v);
        }
        rows.push((t, row));
    }
    if rows.is_empty() && problems.is_empty() {
        problems.push(format!("{name}: no rows after the header"));
    }
    if !problems.is_empty() {
        let extra = problems.len().saturating_sub(MAX_PROBLEMS);
        problems.truncate(MAX_PROBLEMS);
        if extra > 0 {
            problems.push(format!("... and {extra} more"));
        }
        return Err(problems.join("\n"));
    }
    Ok(Inputs { name: name.to_string(), columns: columns.into_iter().map(|(_, s)| s).collect(), rows })
}

/// The first tick whose time is at or after `t_s` (a time that is a whole number of ticks up to rounding is that
/// tick: 0.3 s at 8 kHz is tick 2400, not 2401).
pub fn tick_at(t_s: f64, base_hz: u32) -> u64 {
    let x = t_s * f64::from(base_hz);
    let r = x.round();
    if (x - r).abs() < 1e-6 {
        r as u64
    } else {
        x.ceil() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subset_of_columns_parses_in_file_order() {
        let text = "# flips\n\nt_s, throttle, aux1\n0.0, 0.0, -1\n1.0, 0.0, 1\n\n1.5, 0.6, 1\n";
        let inputs = parse("flips.csv", text).unwrap();
        assert_eq!(inputs.columns, [Stick::Throttle, Stick::Aux(0)]);
        assert_eq!(inputs.rows, [(0.0, vec![0.0, -1.0]), (1.0, vec![0.0, 1.0]), (1.5, vec![0.6, 1.0])]);
        assert_eq!(inputs.duration_s(), 1.5);
    }

    #[test]
    fn every_problem_is_reported_with_its_line() {
        let text = "t_s,throttle,roll,wobble\n0,0.5,0,1\n0.5,1.5,0\n0.5,x,0,0\n0.2,0.5,-2,0\n";
        let err = parse("bad.csv", text).unwrap_err();
        assert_eq!(
            err.lines().collect::<Vec<_>>(),
            [
                "bad.csv line 1: unknown column 'wobble' (expected t_s, roll, pitch, yaw, throttle, aux1, aux2, aux3, aux4)",
                "bad.csv line 3: 3 values for 4 columns",
                "bad.csv line 4: 'x' is not a finite number",
                "bad.csv line 5: t_s 0.2 does not increase (previous row 0.5)",
                "bad.csv line 5: roll -2 is outside [-1, 1]",
            ]
        );
    }

    #[test]
    fn the_header_needs_time_and_a_stick_and_the_file_needs_rows() {
        assert!(parse("e.csv", "").unwrap_err().contains("e.csv: no header line"));
        assert!(parse("e.csv", "throttle\n0.5\n").unwrap_err().contains("the header needs a t_s column"));
        assert!(parse("e.csv", "t_s\n0\n").unwrap_err().contains("at least one stick column"));
        assert_eq!(parse("e.csv", "t_s,yaw\n").unwrap_err(), "e.csv: no rows after the header");
        assert!(parse("e.csv", "t_s,aux5\n0,0\n").unwrap_err().contains("unknown column 'aux5'"));
        assert!(parse("e.csv", "t_s,yaw,yaw\n0,0,0\n").unwrap_err().contains("column yaw appears twice"));
        assert!(parse("e.csv", "t_s,yaw\n-1,0\n").unwrap_err().contains("t_s -1 is negative"));
        assert!(parse("e.csv", "t_s,yaw\n0,inf\n").unwrap_err().contains("'inf' is not a finite number"));
    }

    #[test]
    fn a_spreadsheet_export_with_a_bom_and_crlf_lines_parses_like_a_plain_file() {
        let plain = parse("f.csv", "t_s,throttle\n0,0.5\n1,0.2\n").unwrap();
        let excel = parse("f.csv", "\u{feff}t_s,throttle\r\n0,0.5\r\n1,0.2\r\n").unwrap();
        assert_eq!(excel, plain);
    }

    #[test]
    fn long_error_lists_are_capped() {
        let mut text = String::from("t_s,throttle\n");
        for i in 0..30 {
            text.push_str(&format!("{i},2\n"));
        }
        let err = parse("big.csv", &text).unwrap_err();
        assert_eq!(err.lines().count(), 21);
        assert_eq!(err.lines().last(), Some("... and 10 more"));
    }

    #[test]
    fn times_land_on_the_first_tick_at_or_after_them() {
        assert_eq!(tick_at(0.0, 8000), 0);
        assert_eq!(tick_at(0.3, 8000), 2400, "0.3 * 8000 is 2400.0000000000005 in floating point");
        assert_eq!(tick_at(0.00001, 8000), 1);
        assert_eq!(tick_at(1.0 / 3.0, 8000), 2667);
    }
}
````

**Modify `crates/ofs-sim/src/lib.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-sim/src/lib.rs b/crates/ofs-sim/src/lib.rs
index 77d3b25..a09ab7d 100644
--- a/crates/ofs-sim/src/lib.rs
+++ b/crates/ofs-sim/src/lib.rs
@@ -2,6 +2,7 @@
 // tonic's `Status` is the error type of every gRPC call (the generated traits fix it); boxing it would only move
 // the size elsewhere.
 #![allow(clippy::result_large_err)]
+pub mod inputs;
 pub mod listen;
 pub mod pacer;
 pub mod runner;
````

Run: `cargo test -p ofs-sim --lib inputs`
Expected: PASS (6 tests). `scripted` still fails to compile.

- [ ] **Step 3: `ticks_for`, `check_fault`, scheduling, playback, the per-tick hook**

In `vehicle.rs`, the `check_fault` hunk moves each arm's validation verbatim; read its messages against the old `set_fault` while applying it.

**Modify `crates/ofs-core/src/scheduler.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-core/src/scheduler.rs b/crates/ofs-core/src/scheduler.rs
index c692835..a25fa25 100644
--- a/crates/ofs-core/src/scheduler.rs
+++ b/crates/ofs-core/src/scheduler.rs
@@ -68,15 +68,20 @@ impl Scheduler {
         Ok(())
     }
 
-    /// Steps `seconds` of simulated time, rounded to the nearest whole tick (at most [`MAX_RUN_FOR_S`]).
-    pub fn run_for(&mut self, seconds: f64) -> Result<(), SimError> {
+    /// The whole ticks in `seconds` of simulated time (rounded to the nearest; at most [`MAX_RUN_FOR_S`]).
+    pub fn ticks_for(&self, seconds: f64) -> Result<u64, SimError> {
         if !seconds.is_finite() || seconds < 0.0 {
             return Err(SimError::InvalidArgument(format!("seconds must be finite and >= 0 (got {seconds})")));
         }
         if seconds > MAX_RUN_FOR_S {
             return Err(SimError::InvalidArgument(format!("seconds must be <= {MAX_RUN_FOR_S} (got {seconds})")));
         }
-        let ticks = (seconds * f64::from(self.base_hz)).round() as u64;
+        Ok((seconds * f64::from(self.base_hz)).round() as u64)
+    }
+
+    /// Steps `seconds` of simulated time, rounded to the nearest whole tick (at most [`MAX_RUN_FOR_S`]).
+    pub fn run_for(&mut self, seconds: f64) -> Result<(), SimError> {
+        let ticks = self.ticks_for(seconds)?;
         for _ in 0..ticks {
             self.step()?;
         }
````

**Modify `crates/ofs-sim/src/vehicle.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-sim/src/vehicle.rs b/crates/ofs-sim/src/vehicle.rs
index e995cd0..4a92ff0 100644
--- a/crates/ofs-sim/src/vehicle.rs
+++ b/crates/ofs-sim/src/vehicle.rs
@@ -13,6 +13,7 @@ use ofs_electrical::battery::{Battery, BatteryParams};
 use ofs_electrical::esc_motor::{EscMotor, EscParams, MotorParams};
 use ofs_fc::open_loop::OpenLoopFc;
 use ofs_log::{RecordError, Recorder, RecorderHandle, RecordingSummary, SignalRate};
+use crate::inputs::{tick_at, Inputs, Stick};
 use ofs_fc::sitl::bridge::{BridgeConfig, SerialLink, SerialTap, SitlBridge};
 use ofs_fc::sitl::codec::MSP_TCP_PORT;
 use ofs_fc::sitl::esc_telemetry::EscTelemetry;
@@ -443,6 +444,19 @@ pub struct Vehicle {
     active_faults: Vec<String>,
     /// The recorder model's handle: StartRecording/StopRecording go through it.
     recorder: RecorderHandle,
+    /// Faults scheduled with `at`, by due tick (ties in scheduling order).
+    pending_faults: Vec<(u64, Fault)>,
+    /// The inputs file playing now.
+    playback: Option<Playback>,
+    /// An inputs file that finished on its own, until the session raises its event.
+    finished_inputs: Option<String>,
+}
+
+/// An inputs file being played: its rows' due ticks and the next row to apply.
+struct Playback {
+    inputs: Inputs,
+    ticks: Vec<u64>,
+    next: usize,
 }
 
 /// Per-quad firmware directory: `<data_dir>/<quad file stem>-<hash of the quad file's path>`, so quads with
@@ -841,6 +855,9 @@ pub fn build(cfg: &QuadConfig, opts: &BuildOptions) -> Result<Vehicle, SimError>
         has_vtx: cfg.vtx.is_some(),
         active_faults: Vec::new(),
         recorder,
+        pending_faults: Vec::new(),
+        playback: None,
+        finished_inputs: None,
     };
     // The bus starts every signal at zero; aux 0.0 would reach Betaflight as 1500 us until the first SetSticks.
     vehicle.set_sticks(&Sticks::default());
@@ -881,17 +898,14 @@ impl Vehicle {
         self.scheduler.bus_mut().set(s, if on { 1.0 } else { 0.0 });
     }
 
-    /// Activates (`active = true`) or deactivates a fault. A fault that fails validation leaves nothing
-    /// behind: no signal moves and it is not recorded as active.
-    pub fn set_fault(&mut self, fault: Fault, active: bool) -> Result<(), SimError> {
-        let b = self.scheduler.bus_mut();
-        match &fault {
-            Fault::RadioLinkLoss => b.set(self.h.fault_radio_loss, if active { 1.0 } else { 0.0 }),
+    /// Validates a fault against this quad without applying it: what `set_fault` and `schedule_fault` refuse.
+    fn check_fault(&self, fault: &Fault) -> Result<(), SimError> {
+        match fault {
+            Fault::RadioLinkLoss => {}
             Fault::RadioLoss { fraction } => {
                 if !(0.0..=1.0).contains(fraction) {
                     return Err(SimError::InvalidArgument(format!("radio loss fraction must be in [0, 1], got {fraction}")));
                 }
-                b.set(self.h.fault_radio_loss_fraction, if active { *fraction } else { 0.0 });
             }
             Fault::BurstLoss { on_s, off_s } => {
                 if !on_s.is_finite() || !off_s.is_finite() || *on_s <= 0.0 || *off_s <= 0.0 {
@@ -899,9 +913,6 @@ impl Vehicle {
                         "burst on_ms and off_ms must both be finite and > 0, got on {on_s} s / off {off_s} s"
                     )));
                 }
-                let (on, off) = if active { (*on_s, *off_s) } else { (0.0, 0.0) };
-                b.set(self.h.fault_burst_on, on);
-                b.set(self.h.fault_burst_off, off);
             }
             Fault::MotorFailure { motor, scale } => {
                 if *motor >= self.h.fault_motor_scale.len() {
@@ -913,7 +924,6 @@ impl Vehicle {
                 if !(0.0..=1.0).contains(scale) {
                     return Err(SimError::InvalidArgument(format!("motor failure scale must be in [0, 1], got {scale}")));
                 }
-                b.set(self.h.fault_motor_scale[*motor], if active { *scale } else { 1.0 });
             }
             Fault::PropDamage { prop, thrust_scale, imbalance_kgm } => {
                 if *prop >= self.h.fault_prop_thrust_scale.len() {
@@ -927,8 +937,6 @@ impl Vehicle {
                         "prop damage thrust_scale and imbalance_kgm must be finite and >= 0, got {thrust_scale} / {imbalance_kgm}"
                     )));
                 }
-                b.set(self.h.fault_prop_thrust_scale[*prop], if active { *thrust_scale } else { 1.0 });
-                b.set(self.h.prop_imbalance[*prop], if active { *imbalance_kgm } else { self.prop_imbalance_baseline[*prop] });
             }
             Fault::WeakCell { cell, r_offset_ohm, v_offset_v } => {
                 if *cell as u32 >= self.battery_cells {
@@ -940,16 +948,11 @@ impl Vehicle {
                 if !r_offset_ohm.is_finite() || !v_offset_v.is_finite() || *r_offset_ohm < 0.0 || *v_offset_v < 0.0 {
                     return Err(SimError::InvalidArgument("weak-cell offsets must be finite and >= 0".into()));
                 }
-                b.set(self.h.fault_battery_weak_cell, if active { *cell as f64 + 1.0 } else { 0.0 });
-                b.set(self.h.fault_battery_r_offset, if active { *r_offset_ohm } else { 0.0 });
-                b.set(self.h.fault_battery_v_offset, if active { *v_offset_v } else { 0.0 });
             }
             Fault::GyroFault { bias_radps, noise_scale } => {
                 if !noise_scale.is_finite() || *noise_scale < 0.0 || !bias_radps.is_finite() {
                     return Err(SimError::InvalidArgument("gyro fault noise_scale must be >= 0 and bias finite".into()));
                 }
-                b.set(self.h.fault_imu_gyro_bias, if active { *bias_radps } else { DVec3::ZERO });
-                b.set(self.h.fault_imu_noise_scale, if active { *noise_scale } else { 1.0 });
             }
             Fault::VtxInterference { freq_mhz, power_mw, position_ned_m } => {
                 if !self.has_vtx {
@@ -960,6 +963,39 @@ impl Vehicle {
                         "vtx interference needs a positive frequency and power and a finite position, got {freq_mhz} MHz / {power_mw} mW / {position_ned_m:?}"
                     )));
                 }
+            }
+        }
+        Ok(())
+    }
+
+    /// Activates (`active = true`) or deactivates a fault. A fault that fails validation leaves nothing
+    /// behind: no signal moves and it is not recorded as active.
+    pub fn set_fault(&mut self, fault: Fault, active: bool) -> Result<(), SimError> {
+        self.check_fault(&fault)?;
+        let b = self.scheduler.bus_mut();
+        match &fault {
+            Fault::RadioLinkLoss => b.set(self.h.fault_radio_loss, if active { 1.0 } else { 0.0 }),
+            Fault::RadioLoss { fraction } => b.set(self.h.fault_radio_loss_fraction, if active { *fraction } else { 0.0 }),
+            Fault::BurstLoss { on_s, off_s } => {
+                let (on, off) = if active { (*on_s, *off_s) } else { (0.0, 0.0) };
+                b.set(self.h.fault_burst_on, on);
+                b.set(self.h.fault_burst_off, off);
+            }
+            Fault::MotorFailure { motor, scale } => b.set(self.h.fault_motor_scale[*motor], if active { *scale } else { 1.0 }),
+            Fault::PropDamage { prop, thrust_scale, imbalance_kgm } => {
+                b.set(self.h.fault_prop_thrust_scale[*prop], if active { *thrust_scale } else { 1.0 });
+                b.set(self.h.prop_imbalance[*prop], if active { *imbalance_kgm } else { self.prop_imbalance_baseline[*prop] });
+            }
+            Fault::WeakCell { cell, r_offset_ohm, v_offset_v } => {
+                b.set(self.h.fault_battery_weak_cell, if active { *cell as f64 + 1.0 } else { 0.0 });
+                b.set(self.h.fault_battery_r_offset, if active { *r_offset_ohm } else { 0.0 });
+                b.set(self.h.fault_battery_v_offset, if active { *v_offset_v } else { 0.0 });
+            }
+            Fault::GyroFault { bias_radps, noise_scale } => {
+                b.set(self.h.fault_imu_gyro_bias, if active { *bias_radps } else { DVec3::ZERO });
+                b.set(self.h.fault_imu_noise_scale, if active { *noise_scale } else { 1.0 });
+            }
+            Fault::VtxInterference { freq_mhz, power_mw, position_ned_m } => {
                 self.vtx_interference.set(active.then_some(Emitter {
                     position: *position_ned_m,
                     freq_mhz: *freq_mhz,
@@ -977,6 +1013,85 @@ impl Vehicle {
         Ok(())
     }
 
+    /// Activates `fault` at simulated time `at_s`, on the first tick at or after it (spec §6.2's `at=`), so a
+    /// lockstep run is the same however its `Run` calls are cut. Validated now; a time already passed is refused;
+    /// the current time applies at once. `clear_faults` also drops the scheduled faults.
+    pub fn schedule_fault(&mut self, fault: Fault, at_s: f64) -> Result<(), SimError> {
+        if !at_s.is_finite() || at_s < 0.0 {
+            return Err(SimError::InvalidArgument(format!("fault time must be finite and >= 0 s, got {at_s}")));
+        }
+        self.check_fault(&fault)?;
+        let at = tick_at(at_s, self.base_hz());
+        let now = self.scheduler.tick();
+        if at < now {
+            return Err(SimError::InvalidArgument(format!("fault time {at_s} s has passed (the session is at {} s)", self.time_s())));
+        }
+        if at == now {
+            return self.set_fault(fault, true);
+        }
+        // Stable: faults due on one tick apply in the order they were scheduled.
+        let index = self.pending_faults.partition_point(|(t, _)| *t <= at);
+        self.pending_faults.insert(index, (at, fault));
+        Ok(())
+    }
+
+    /// Starts playing `inputs` from now: row `t_s` applies on the first tick at or after now + `t_s`, columns the
+    /// file leaves out keep their current value, and the last row's values hold after playback ends. Refused while
+    /// another file plays.
+    pub fn play_inputs(&mut self, inputs: Inputs) -> Result<(), SimError> {
+        if let Some(playing) = &self.playback {
+            return Err(SimError::InvalidArgument(format!("{} is still playing; stop it first", playing.inputs.name)));
+        }
+        let now = self.scheduler.tick();
+        let base_hz = self.base_hz();
+        let ticks = inputs.rows.iter().map(|(t, _)| now + tick_at(*t, base_hz)).collect();
+        self.playback = Some(Playback { inputs, ticks, next: 0 });
+        self.apply_due(); // rows at t_s = 0 apply now
+        Ok(())
+    }
+
+    /// Stops the playing inputs file, if any (the sticks keep their last values). Returns its name.
+    pub fn stop_inputs(&mut self) -> Option<String> {
+        self.playback.take().map(|p| p.inputs.name)
+    }
+
+    /// The name of the inputs file playing now.
+    pub fn inputs_playing(&self) -> Option<&str> {
+        self.playback.as_ref().map(|p| p.inputs.name.as_str())
+    }
+
+    /// The inputs file that finished on its own since the last call (its last row applied), once.
+    pub fn take_finished_inputs(&mut self) -> Option<String> {
+        self.finished_inputs.take()
+    }
+
+    /// Applies what is due at the current tick: scheduled faults, then inputs rows.
+    fn apply_due(&mut self) {
+        let now = self.scheduler.tick();
+        while self.pending_faults.first().is_some_and(|(t, _)| *t <= now) {
+            let (_, fault) = self.pending_faults.remove(0);
+            self.set_fault(fault, true).expect("validated when scheduled");
+        }
+        let Some(p) = self.playback.as_mut() else { return };
+        let bus = self.scheduler.bus_mut();
+        while p.next < p.ticks.len() && p.ticks[p.next] <= now {
+            for (stick, v) in p.inputs.columns.iter().zip(&p.inputs.rows[p.next].1) {
+                let signal = match stick {
+                    Stick::Roll => self.h.roll,
+                    Stick::Pitch => self.h.pitch,
+                    Stick::Yaw => self.h.yaw,
+                    Stick::Throttle => self.h.throttle,
+                    Stick::Aux(i) => self.h.aux[*i],
+                };
+                bus.set(signal, *v);
+            }
+            p.next += 1;
+        }
+        if p.next == p.ticks.len() {
+            self.finished_inputs = self.playback.take().map(|p| p.inputs.name);
+        }
+    }
+
     /// Remembers the fault as active (replacing an existing same-name entry, so a re-inject updates the
     /// parameters without duplicating) or forgets it.
     fn record_fault_active(&mut self, fault: &Fault, active: bool) {
@@ -1014,16 +1129,22 @@ impl Vehicle {
         bus.set(h.fault_imu_gyro_bias, DVec3::ZERO);
         self.vtx_interference.set(None);
         self.active_faults.clear();
+        self.pending_faults.clear();
     }
 
     pub fn run_for(&mut self, seconds: f64) -> Result<(), SimError> {
-        self.scheduler.run_for(seconds)
+        let ticks = self.scheduler.ticks_for(seconds)?;
+        self.step_ticks(ticks)
     }
 
-    /// Steps `ticks` base ticks. On error the failing tick is not counted.
+    /// Steps `ticks` base ticks. Scheduled faults and inputs rows apply as soon as the simulated time reaches
+    /// theirs, so the state at that time already shows them. On error the failing tick is not counted.
     pub fn step_ticks(&mut self, ticks: u64) -> Result<(), SimError> {
         for _ in 0..ticks {
             self.scheduler.step()?;
+            if !self.pending_faults.is_empty() || self.playback.is_some() {
+                self.apply_due();
+            }
         }
         Ok(())
     }
````

Run: `cargo test -p ofs-core -p ofs-sim`
Expected: PASS: the 7 `scripted` tests plus every existing test, M4b's fault-validation messages included.

- [ ] **Step 4: Lint and commit**

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`. Expected: no output.

```bash
git add crates/ofs-core crates/ofs-sim
git commit -m "feat(sim): scheduled faults and scripted inputs - applied on their tick, whatever the Run chunking"
```

---

### Task 4: Protocol 9: recording, scripted inputs, faults at a time, world names, Blackbox copies

The wire and the server move together. Every client moves in this commit: the Rust client through `ofs_proto`, and Python's version constant and regenerated stubs. Python's new API is Task 5.

**Files:**
- Create: `crates/ofs-sim/src/blackbox.rs`
- Modify: `crates/ofs-client/src/model.rs`
- Modify: `crates/ofs-client/src/worker.rs`
- Modify: `crates/ofs-config/src/world.rs`
- Modify: `crates/ofs-config/tests/world.rs`
- Modify: `crates/ofs-proto/src/lib.rs`
- Modify: `crates/ofs-proto/tests/messages.rs`
- Modify: `crates/ofs-sim/src/lib.rs`
- Modify: `crates/ofs-sim/src/main.rs`
- Modify: `crates/ofs-sim/src/server.rs`
- Modify: `crates/ofs-sim/src/session.rs`
- Modify: `crates/ofs-sim/src/streams.rs`
- Modify: `crates/ofs-sim/tests/grpc.rs`
- Modify: `proto/ofs/v1/sim.proto`
- Modify: `python/ofs/client.py`
- Regenerate (never hand-edit): `python/ofs/v1/sim_pb2.py`
- Regenerate (never hand-edit): `python/ofs/v1/sim_pb2.pyi`
- Regenerate (never hand-edit): `python/ofs/v1/sim_pb2_grpc.py`

**Interfaces:**
- Consumes from Tasks 2 and 3: the vehicle methods named there.
- Produces:
  - The protocol 9 messages (Global Constraints).
  - `ofs_sim::server::{sim_server(SimService) -> SimServer<SimService>, MAX_REQUEST_BYTES, SimService::logs_dir() -> PathBuf}`.
  - `ofs_sim::blackbox::BlackboxCopy::{new(firmware_dir, logs_dir), copy() -> io::Result<Vec<PathBuf>>}`, plus a `Drop` that copies.
  - `Session.quad_stem: String` and `Session.blackbox: Option<BlackboxCopy>`, declared last.
  - `ofs_config::world::resolve(&str, &Path) -> PathBuf`.
  - `ofs_client::model::EventKind::{InputsStarted, InputsEnded, RecordingStarted, RecordingStopped}`.
  - Python `PROTOCOL_VERSION = 9` and regenerated stubs.

- [ ] **Step 1: The tests: world names, the protocol pin, the gRPC behaviour**

The `grpc.rs` hunks also add `at_s: None` to its two existing `Fault` literals, because the struct gains a field.

**Modify `crates/ofs-config/tests/world.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-config/tests/world.rs b/crates/ofs-config/tests/world.rs
index a868a01..ebd96fc 100644
--- a/crates/ofs-config/tests/world.rs
+++ b/crates/ofs-config/tests/world.rs
@@ -321,3 +321,15 @@ position_ned_m = [-3.0, 2.0, -1.2]
     .unwrap_err();
     assert!(bare.to_string().contains("handset.antennas"), "{bare}");
 }
+
+#[test]
+fn a_world_name_resolves_beside_the_quads_directory_and_a_path_stays_a_path() {
+    assert_eq!(world::resolve("flat", Path::new("quads/x.toml")), Path::new("worlds").join("flat.toml"));
+    assert_eq!(world::resolve("flat", Path::new("/data/sim/quads/x.toml")), Path::new("/data/sim/worlds").join("flat.toml"));
+    assert_eq!(world::resolve("flat", Path::new("x.toml")), Path::new(".").join("worlds").join("flat.toml"));
+    assert_eq!(world::resolve("worlds/flat.toml", Path::new("quads/x.toml")), Path::new("worlds/flat.toml"));
+    assert_eq!(world::resolve("flat.toml", Path::new("quads/x.toml")), Path::new("flat.toml"), "a file name is a path");
+    assert_eq!(world::resolve(r"C:\sim\flat", Path::new("quads/x.toml")), Path::new(r"C:\sim\flat"));
+    let shipped = world::resolve("flat", Path::new(QUAD));
+    assert_eq!(world::load(&shipped).unwrap().name, "Flat field");
+}
````

**Modify `crates/ofs-proto/tests/messages.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-proto/tests/messages.rs b/crates/ofs-proto/tests/messages.rs
index 6b1599f..77f202c 100644
--- a/crates/ofs-proto/tests/messages.rs
+++ b/crates/ofs-proto/tests/messages.rs
@@ -5,7 +5,7 @@ use prost::Message;
 #[test]
 fn the_protocol_version_matches_the_python_client() {
     // python/ofs/client.py PROTOCOL_VERSION must equal this; bump both together.
-    assert_eq!(PROTOCOL_VERSION, 8);
+    assert_eq!(PROTOCOL_VERSION, 9);
     let python = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../python/ofs/client.py")).unwrap();
     assert!(python.contains(&format!("PROTOCOL_VERSION = {PROTOCOL_VERSION}")), "python client speaks another protocol");
 }
````

**Modify `crates/ofs-sim/tests/grpc.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-sim/tests/grpc.rs b/crates/ofs-sim/tests/grpc.rs
index a657d1b..dff514f 100644
--- a/crates/ofs-sim/tests/grpc.rs
+++ b/crates/ofs-sim/tests/grpc.rs
@@ -1,12 +1,11 @@
 use std::time::Duration;
 
 use ofs_sim::pb::sim_client::SimClient;
-use ofs_sim::pb::sim_server::SimServer;
 use ofs_sim::pb::{
-    fault, Empty, EventKind, Fault, HandshakeRequest, LoadRequest, Mode, PilotInput, RadioLinkLoss, RunRequest, Sticks,
-    StreamRequest, VideoSync,
+    fault, Empty, EventKind, Fault, HandshakeRequest, InputsFile, LoadRequest, Mode, PilotInput, RadioLinkLoss, RecordingSignal,
+    RunRequest, StartRecordingRequest, Sticks, StreamRequest, VideoSync,
 };
-use ofs_sim::server::{SimService, PROTOCOL_VERSION};
+use ofs_sim::server::{sim_server, SimService, PROTOCOL_VERSION};
 use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
 use tokio_stream::StreamExt;
 use tonic::transport::Channel;
@@ -24,7 +23,7 @@ async fn start_with_service() -> (SimClient<Channel>, SimService) {
     let service = SimService::new(data_dir);
     tokio::spawn(
         tonic::transport::Server::builder()
-            .add_service(SimServer::new(service.clone()))
+            .add_service(sim_server(service.clone()))
             .serve_with_incoming(TcpListenerStream::new(listener)),
     );
     (SimClient::connect(format!("http://{addr}")).await.unwrap(), service)
@@ -199,7 +198,7 @@ async fn faults_drop_the_radio_link_and_events_report_it() {
     let mut events = service.subscribe();
     load_open_loop(&mut c).await;
     c.run(RunRequest { seconds: 0.5 }).await.unwrap();
-    let fault = Fault { kind: Some(fault::Kind::RadioLinkLoss(RadioLinkLoss {})) };
+    let fault = Fault { kind: Some(fault::Kind::RadioLinkLoss(RadioLinkLoss {})), at_s: None };
     c.inject_fault(fault).await.unwrap();
     let s = c.run(RunRequest { seconds: 0.5 }).await.unwrap().into_inner();
     assert!(!s.radio.unwrap().link_up);
@@ -213,7 +212,7 @@ async fn faults_drop_the_radio_link_and_events_report_it() {
     // The fault injection and the clear are events now (M4b): injected before the link drops, cleared as the
     // link comes back.
     assert_eq!(kinds, vec![EventKind::LinkUp, EventKind::FaultInjected, EventKind::LinkDown, EventKind::LinkUp, EventKind::FaultCleared]);
-    let err = c.inject_fault(Fault { kind: None }).await.unwrap_err();
+    let err = c.inject_fault(Fault { kind: None, at_s: None }).await.unwrap_err();
     assert_eq!(kind(&err), "invalid_argument");
 }
 
@@ -407,3 +406,154 @@ async fn a_pilot_that_never_reads_its_states_frees_the_slot_when_its_session_end
     assert!(states.next().await.unwrap().unwrap().radio.unwrap().tx_enabled);
     drop(tx);
 }
+
+/// The events published since `events` subscribed, as (kind, message), keeping only `wanted` kinds.
+fn drain(events: &mut tokio::sync::broadcast::Receiver<ofs_sim::pb::Event>, wanted: &[EventKind]) -> Vec<(EventKind, String)> {
+    let mut out = Vec::new();
+    while let Ok(e) = events.try_recv() {
+        if wanted.contains(&e.kind()) {
+            out.push((e.kind(), e.message));
+        }
+    }
+    out
+}
+
+#[tokio::test]
+async fn the_handshake_names_the_logs_directory() {
+    let (mut c, service) = start_with_service().await;
+    let reply = c.handshake(HandshakeRequest { protocol_version: PROTOCOL_VERSION }).await.unwrap().into_inner();
+    assert_eq!(std::path::PathBuf::from(&reply.logs_dir), service.logs_dir());
+    assert!(std::path::Path::new(&reply.logs_dir).is_absolute(), "{}", reply.logs_dir);
+    assert!(reply.logs_dir.ends_with("logs"), "{}", reply.logs_dir);
+}
+
+#[tokio::test]
+async fn the_server_names_recordings_and_reports_them_as_events() {
+    let (mut c, service) = start_with_service().await;
+    let mut events = service.subscribe();
+    load_open_loop(&mut c).await;
+    let signals = vec![
+        RecordingSignal { pattern: "battery.*".into(), rate_hz: 0.0 },
+        RecordingSignal { pattern: "imu.gyro_frd_radps".into(), rate_hz: 500.0 },
+    ];
+    let started = c.start_recording(StartRecordingRequest { signals }).await.unwrap().into_inner();
+    assert_eq!(std::path::PathBuf::from(&started.path), service.logs_dir().join("opendrone-5f-freestyle-1.mcap"));
+    assert_eq!(started.signals, ["battery.consumed_mah", "battery.current_a", "battery.soc", "battery.voltage_v", "imu.gyro_frd_radps"]);
+    assert_eq!(kind(&c.start_recording(StartRecordingRequest::default()).await.unwrap_err()), "invalid_state", "one at a time");
+    c.run(RunRequest { seconds: 1.0 }).await.unwrap();
+    let summary = c.stop_recording(Empty {}).await.unwrap().into_inner();
+    assert_eq!(summary.path, started.path);
+    assert_eq!(summary.messages, 4 * 10 + 500, "battery at the default 10 Hz, the gyro at 500 Hz");
+    assert_eq!((summary.first_s, summary.last_s), (0.002, 1.0));
+    assert_eq!(kind(&c.stop_recording(Empty {}).await.unwrap_err()), "invalid_state");
+    let next = c.start_recording(StartRecordingRequest::default()).await.unwrap().into_inner();
+    assert!(next.path.ends_with("opendrone-5f-freestyle-2.mcap"), "{}", next.path);
+    let got = drain(&mut events, &[EventKind::RecordingStarted, EventKind::RecordingStopped]);
+    assert_eq!(
+        got,
+        [
+            (EventKind::RecordingStarted, started.path.clone()),
+            (EventKind::RecordingStopped, format!("{}: 540 messages", started.path)),
+            (EventKind::RecordingStarted, next.path.clone()),
+        ]
+    );
+}
+
+#[tokio::test]
+async fn a_recording_pattern_matching_nothing_is_refused_with_its_near_misses() {
+    let mut c = start().await;
+    load_open_loop(&mut c).await;
+    let signals = vec![RecordingSignal { pattern: "gyro.*".into(), rate_hz: 0.0 }];
+    let err = c.start_recording(StartRecordingRequest { signals }).await.unwrap_err();
+    assert_eq!(kind(&err), "invalid_argument");
+    assert_eq!(err.message(), "pattern 'gyro.*' matches no signal (similar: fault.imu.gyro_bias_radps, imu.gyro_frd_radps)");
+}
+
+#[tokio::test]
+async fn inputs_play_refuse_set_sticks_and_report_events() {
+    let (mut c, service) = start_with_service().await;
+    let mut events = service.subscribe();
+    load_open_loop(&mut c).await;
+    let file = |name: &str, csv: &str| InputsFile { name: name.into(), csv: csv.into() };
+    let bad = c.play_inputs(file("bad.csv", "t_s,throttle\n0,2\n")).await.unwrap_err();
+    assert_eq!(kind(&bad), "config");
+    assert_eq!(bad.message(), "bad.csv line 2: throttle 2 is outside [0, 1]");
+    c.play_inputs(file("climb.csv", "t_s,throttle\n0,0.3\n0.5,1\n")).await.unwrap();
+    let refused = c.set_sticks(Sticks::default()).await.unwrap_err();
+    assert_eq!(kind(&refused), "invalid_argument");
+    assert!(refused.message().contains("climb.csv is playing"), "{}", refused.message());
+    assert_eq!(kind(&c.play_inputs(file("other.csv", "t_s,yaw\n0,0\n")).await.unwrap_err()), "invalid_state");
+    let s = c.run(RunRequest { seconds: 0.6 }).await.unwrap().into_inner();
+    assert_eq!(s.motor_cmd, [1.0; 4]);
+    c.set_sticks(Sticks::default()).await.unwrap(); // finished: the sticks are the script's again
+    c.play_inputs(file("hold.csv", "t_s,throttle\n0,0.2\n9,0.2\n")).await.unwrap();
+    c.stop_inputs(Empty {}).await.unwrap();
+    c.stop_inputs(Empty {}).await.unwrap(); // nothing playing: nothing to do
+    c.set_sticks(Sticks::default()).await.unwrap();
+    let got = drain(&mut events, &[EventKind::InputsStarted, EventKind::InputsEnded]);
+    assert_eq!(
+        got,
+        [
+            (EventKind::InputsStarted, "climb.csv: 2 rows over 0.5 s".to_string()),
+            (EventKind::InputsEnded, "climb.csv: finished".to_string()),
+            (EventKind::InputsStarted, "hold.csv: 2 rows over 9 s".to_string()),
+            (EventKind::InputsEnded, "hold.csv: stopped".to_string()),
+        ]
+    );
+}
+
+#[tokio::test]
+async fn an_inputs_file_larger_than_grpcs_default_limit_plays() {
+    let mut c = start().await;
+    load_open_loop(&mut c).await;
+    let mut csv = String::from("t_s,throttle,roll,pitch,yaw\n");
+    for i in 0..200_000 {
+        csv.push_str(&format!("{:.3},0.5,0.1,-0.1,0.0\n", f64::from(i) * 0.001));
+    }
+    assert!(csv.len() > 4 * 1024 * 1024, "{} bytes", csv.len());
+    c.play_inputs(InputsFile { name: "long.csv".into(), csv }).await.unwrap();
+    let s = c.run(RunRequest { seconds: 0.1 }).await.unwrap().into_inner();
+    assert_eq!(s.motor_cmd, [0.5; 4]);
+}
+
+#[tokio::test]
+async fn scripted_inputs_and_a_pilot_exclude_each_other() {
+    let mut c = start().await;
+    c.load(open_loop(Mode::Realtime)).await.unwrap();
+    c.play_inputs(InputsFile { name: "hold.csv".into(), csv: "t_s,throttle\n0,0.2\n9,0.2\n".into() }).await.unwrap();
+    let (tx, rx) = tokio::sync::mpsc::channel(8);
+    tx.send(PilotInput { sticks: Some(Sticks::default()), state_rate_hz: 50 }).await.unwrap();
+    assert_eq!(kind(&c.pilot(ReceiverStream::new(rx)).await.unwrap_err()), "invalid_state");
+    c.stop_inputs(Empty {}).await.unwrap();
+    let (tx, rx) = tokio::sync::mpsc::channel(8);
+    tx.send(PilotInput { sticks: Some(Sticks::default()), state_rate_hz: 50 }).await.unwrap();
+    let _states = c.pilot(ReceiverStream::new(rx)).await.unwrap().into_inner();
+    let err = c.play_inputs(InputsFile { name: "hold.csv".into(), csv: "t_s,throttle\n0,0.2\n".into() }).await.unwrap_err();
+    assert_eq!(kind(&err), "invalid_state");
+    drop(tx);
+}
+
+#[tokio::test]
+async fn a_fault_can_wait_for_its_time_but_not_for_a_passed_one() {
+    let mut c = start().await;
+    load_open_loop(&mut c).await;
+    let at = |at_s: f64| Fault { kind: Some(fault::Kind::RadioLinkLoss(RadioLinkLoss {})), at_s: Some(at_s) };
+    c.inject_fault(at(0.5)).await.unwrap();
+    let s = c.run(RunRequest { seconds: 0.4 }).await.unwrap().into_inner();
+    assert!(s.radio.unwrap().link_up, "not yet");
+    let s = c.run(RunRequest { seconds: 0.6 }).await.unwrap().into_inner();
+    assert!(!s.radio.unwrap().link_up, "lost since 0.5 s");
+    let err = c.inject_fault(at(0.2)).await.unwrap_err();
+    assert_eq!(kind(&err), "invalid_argument");
+    assert_eq!(err.message(), "fault time 0.2 s has passed (the session is at 1 s)");
+}
+
+#[tokio::test]
+async fn a_world_can_be_named() {
+    let mut c = start().await;
+    c.load(LoadRequest { world_path: "flat".into(), ..open_loop(Mode::Lockstep) }).await.unwrap();
+    assert_eq!(c.get_world(Empty {}).await.unwrap().into_inner().name, "Flat field");
+    let err = c.load(LoadRequest { world_path: "nowhere".into(), ..open_loop(Mode::Lockstep) }).await.unwrap_err();
+    assert_eq!(kind(&err), "config");
+    assert!(err.message().contains("nowhere.toml"), "{}", err.message());
+}
````

Run: `cargo test -p ofs-config -p ofs-proto -p ofs-sim`
Expected: compile errors: no `world::resolve`, no `StartRecordingRequest`, no `sim_server`, no `at_s` field.

- [ ] **Step 2: The proto, the version and the stubs**

**Modify `proto/ofs/v1/sim.proto`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/proto/ofs/v1/sim.proto b/proto/ofs/v1/sim.proto
index b893a71..32aba3c 100644
--- a/proto/ofs/v1/sim.proto
+++ b/proto/ofs/v1/sim.proto
@@ -2,10 +2,10 @@ syntax = "proto3";
 
 package ofs.v1;
 
-// Simulator control, protocol version 8: protocol 7 plus the v1 fault catalog's wire shape, events and python
-// dataclasses (M4b).
-// Still: lockstep and real-time sessions, state and event streams, the pilot link, fault injection, OSD and VTX
-// state, and the world with its analog video link. One session (loaded quad) per server.
+// Simulator control, protocol version 9: protocol 8 plus recording the bus to MCAP, scripted stick inputs,
+// faults scheduled at a simulated time and world names (M4c).
+// Still: lockstep and real-time sessions, state and event streams, the pilot link, the v1 fault catalog, OSD and
+// VTX state, and the world with its analog video link. One session (loaded quad) per server.
 service Sim {
   rpc Handshake(HandshakeRequest) returns (HandshakeReply);
   rpc Load(LoadRequest) returns (LoadReply);
@@ -33,6 +33,15 @@ service Sim {
   rpc StreamOsd(StreamRequest) returns (stream OsdFrame);
   // The world the session flies in, as loaded and validated (the built-in open field when Load named none).
   rpc GetWorld(Empty) returns (World);
+  // Records the bus to a new MCAP file the server names (<data_dir>/logs/<quad>-<n>.mcap); one at a time.
+  rpc StartRecording(StartRecordingRequest) returns (RecordingStarted);
+  // Finishes the running recording's file (it is readable from then on).
+  rpc StopRecording(Empty) returns (RecordingSummary);
+  // Plays a stick-inputs CSV sample-and-hold from now; SetSticks is refused while it plays. The file travels in the
+  // request: the server reads no client paths.
+  rpc PlayInputs(InputsFile) returns (Empty);
+  // Stops the playing inputs file, if any (the sticks keep their last values).
+  rpc StopInputs(Empty) returns (Empty);
 }
 
 message Empty {}
@@ -41,6 +50,7 @@ message HandshakeRequest { uint32 protocol_version = 1; }
 message HandshakeReply {
   uint32 protocol_version = 1;
   string server_version = 2;
+  string logs_dir = 3;  // absolute: where recordings and copied Blackbox logs land
 }
 
 enum Mode {
@@ -63,7 +73,9 @@ message LoadRequest {
   bool open_loop_fc = 4;  // testing aid: replace the configured FC with throttle-only open loop
   OverrunPolicy overrun_policy = 5;
   bool keep_alive = 6;  // keep the session when its last watcher disconnects
-  string world_path = 7;  // a world file (worlds/flat.toml); empty: the built-in open field
+  // A world file (worlds/flat.toml), or a world name (flat: worlds/flat.toml beside the quad's directory, as
+  // parts resolve against library/); empty: the built-in open field.
+  string world_path = 7;
 }
 message LoadReply {
   string quad_name = 1;
@@ -213,6 +225,10 @@ enum EventKind {
   EVENT_KIND_COLLISION = 13;       // the quad hit something: the message names the object and the speed
   EVENT_KIND_FAULT_INJECTED = 14;  // a fault became active: the message names it
   EVENT_KIND_FAULT_CLEARED = 15;   // a fault stopped being active: the message names it
+  EVENT_KIND_INPUTS_STARTED = 16;  // an inputs file started playing: the message names it
+  EVENT_KIND_INPUTS_ENDED = 17;    // it finished or was stopped (the message says which)
+  EVENT_KIND_RECORDING_STARTED = 18;  // the message is the file's path
+  EVENT_KIND_RECORDING_STOPPED = 19;  // stopped, or failed (a disk error): the message says which
 }
 
 message Event {
@@ -271,6 +287,35 @@ message Fault {
     GyroFault gyro_fault = 7;
     VtxInterference vtx_interference = 8;
   }
+  optional double at_s = 9;  // activate at this simulated time (absent: now); refused once that time has passed
+}
+
+// A recorded signal pattern: `*` matches any run of characters (dots included), `?` one character.
+message RecordingSignal {
+  string pattern = 1;
+  double rate_hz = 2;  // 0: each matched signal's rate from the default table
+}
+
+message StartRecordingRequest {
+  repeated RecordingSignal signals = 1;  // empty: every signal at the default table's rates
+}
+
+message RecordingStarted {
+  string path = 1;
+  repeated string signals = 2;  // the recorded signals' bus names
+}
+
+message RecordingSummary {
+  string path = 1;
+  uint64 messages = 2;
+  double first_s = 3;  // simulated time of the first and last record (0 when there are none)
+  double last_s = 4;
+}
+
+// A stick-inputs CSV: header t_s plus any of roll, pitch, yaw, throttle, aux1..aux4; t_s increasing from 0.
+message InputsFile {
+  string name = 1;  // shown in errors and events (the client sends the file's name)
+  string csv = 2;
 }
 
 // The world file, NED metres from home (up is negative). Strings name the kinds: antenna "omni" or "patch";
````

**Modify `crates/ofs-proto/src/lib.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-proto/src/lib.rs b/crates/ofs-proto/src/lib.rs
index cf8917e..cb2353c 100644
--- a/crates/ofs-proto/src/lib.rs
+++ b/crates/ofs-proto/src/lib.rs
@@ -1,4 +1,4 @@
-//! Protocol 8 messages and gRPC stubs, generated from `proto/ofs/v1/sim.proto`. The simulator server
+//! Protocol 9 messages and gRPC stubs, generated from `proto/ofs/v1/sim.proto`. The simulator server
 //! (`ofs-sim`) and every Rust client (`ofs-client`, the Godot extension) build against this one crate.
 // tonic's `Status` is the error type of every gRPC call (the generated traits fix it); boxing it would only move
 // the size elsewhere.
@@ -9,7 +9,7 @@ pub mod pb {
 }
 
 /// The protocol version this build speaks; the `Handshake` RPC compares it on both sides.
-pub const PROTOCOL_VERSION: u32 = 8;
+pub const PROTOCOL_VERSION: u32 = 9;
 
 /// The unit vector (north, east, down) a heading and an elevation point along: how the server builds an antenna's
 /// aim and how clients draw it (`ReceiverAntenna.aim_az_deg` is added to the pilot's facing to give the heading).
````

**Modify `python/ofs/client.py`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/python/ofs/client.py b/python/ofs/client.py
index f736578..e13fccb 100644
--- a/python/ofs/client.py
+++ b/python/ofs/client.py
@@ -19,7 +19,7 @@ from ofs.v1 import sim_pb2_grpc as pbg
 
 from .errors import ProtocolMismatch, from_rpc_error
 
-PROTOCOL_VERSION = 8
+PROTOCOL_VERSION = 9
 UNLOAD_TIMEOUT_S = 5.0
 
 _MODES = {"lockstep": pb.MODE_LOCKSTEP, "realtime": pb.MODE_REALTIME}
````

Regenerate the Python stubs (`grpcio-tools` comes with `python[dev]`):
```bash
python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto
grep -c $'\r' python/ofs/v1/sim_pb2.py   # expected: 0 (LF)
```

- [ ] **Step 3: The client's event kinds and `Fault` literal**

**Modify `crates/ofs-client/src/model.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-client/src/model.rs b/crates/ofs-client/src/model.rs
index 1672b46..5ca6295 100644
--- a/crates/ofs-client/src/model.rs
+++ b/crates/ofs-client/src/model.rs
@@ -434,6 +434,14 @@ pub enum EventKind {
     FaultInjected,
     /// A fault stopped being active (the message names it).
     FaultCleared,
+    /// An inputs file started playing (the message names it).
+    InputsStarted,
+    /// An inputs file finished or was stopped.
+    InputsEnded,
+    /// A recording started (the message is its path).
+    RecordingStarted,
+    /// A recording stopped or failed.
+    RecordingStopped,
     /// A kind this client does not know (a newer server).
     Unknown,
 }
@@ -456,6 +464,10 @@ impl EventKind {
             EventKind::Collision => "collision",
             EventKind::FaultInjected => "fault_injected",
             EventKind::FaultCleared => "fault_cleared",
+            EventKind::InputsStarted => "inputs_started",
+            EventKind::InputsEnded => "inputs_ended",
+            EventKind::RecordingStarted => "recording_started",
+            EventKind::RecordingStopped => "recording_stopped",
             EventKind::Unknown => "unknown",
         }
     }
@@ -487,6 +499,10 @@ impl Event {
             Ok(pb::EventKind::Collision) => EventKind::Collision,
             Ok(pb::EventKind::FaultInjected) => EventKind::FaultInjected,
             Ok(pb::EventKind::FaultCleared) => EventKind::FaultCleared,
+            Ok(pb::EventKind::InputsStarted) => EventKind::InputsStarted,
+            Ok(pb::EventKind::InputsEnded) => EventKind::InputsEnded,
+            Ok(pb::EventKind::RecordingStarted) => EventKind::RecordingStarted,
+            Ok(pb::EventKind::RecordingStopped) => EventKind::RecordingStopped,
             Ok(pb::EventKind::Unspecified) | Err(_) => EventKind::Unknown,
         };
         Event { time_s: e.time_s, kind, message: e.message }
````

**Modify `crates/ofs-client/src/worker.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-client/src/worker.rs b/crates/ofs-client/src/worker.rs
index afe846f..1a232ce 100644
--- a/crates/ofs-client/src/worker.rs
+++ b/crates/ofs-client/src/worker.rs
@@ -317,7 +317,7 @@ async fn fly(client: &mut SimClient<Channel>, shared: &Arc<Shared>, settings: &S
                 },
                 Some(Command::SetRadioLoss(on)) => {
                     let result = if on {
-                        let fault = pb::Fault { kind: Some(pb::fault::Kind::RadioLinkLoss(pb::RadioLinkLoss {})) };
+                        let fault = pb::Fault { kind: Some(pb::fault::Kind::RadioLinkLoss(pb::RadioLinkLoss {})), at_s: None };
                         client.inject_fault(fault).await.map(|_| ())
                     } else {
                         client.clear_faults(pb::Empty {}).await.map(|_| ())
````

- [ ] **Step 4: World names**

**Modify `crates/ofs-config/src/world.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-config/src/world.rs b/crates/ofs-config/src/world.rs
index 7ec3f34..1a8d299 100644
--- a/crates/ofs-config/src/world.rs
+++ b/crates/ofs-config/src/world.rs
@@ -344,6 +344,16 @@ pub(crate) fn check_beamwidth(c: &mut Checker, kind: AntennaKind, beamwidth_deg:
     }
 }
 
+/// The file a session's world names. A name (`flat`: no path separator, no `.toml`) is `worlds/<name>.toml` beside
+/// the quad's directory (`<quad dir>/../worlds`, as parts resolve against `library/`); anything else is a path.
+pub fn resolve(world: &str, quad_path: &Path) -> PathBuf {
+    if world.contains(['/', '\\']) || world.ends_with(".toml") {
+        return PathBuf::from(world);
+    }
+    let quads = quad_path.parent().unwrap_or(Path::new("."));
+    quads.parent().unwrap_or(Path::new(".")).join("worlds").join(format!("{world}.toml"))
+}
+
 pub fn load(path: &Path) -> Result<WorldConfig, ConfigError> {
     let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io { path: path.to_path_buf(), source })?;
     let parse_err = |message: String| ConfigError::Parse { path: path.to_path_buf(), message };
````

- [ ] **Step 5: The Blackbox copy, with its unit tests**

**Create `crates/ofs-sim/src/blackbox.rs`:**

````rust
//! Betaflight's own Blackbox logs (spec §4). With `set blackbox_device = VIRTUAL` in the quad's diff, SITL writes
//! `LOGnnnnn.BFL` into the quad's firmware directory each time it is armed; when the session ends, the logs it
//! wrote are copied into the logs directory, beside the recordings.
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Copies the session's Blackbox logs when dropped. A `Session` declares it last: fields drop in declaration
/// order, so the vehicle (and Betaflight SITL with it) is gone, and its files closed, before the copy runs.
pub struct BlackboxCopy {
    firmware_dir: PathBuf,
    logs_dir: PathBuf,
    /// Logs modified since then belong to this session (with a little slack for coarse file times).
    since: SystemTime,
}

fn is_blackbox_log(name: &str) -> bool {
    // LOG00001.BFL: what SITL's VIRTUAL device names its files.
    name.len() == 12 && name.starts_with("LOG") && name.ends_with(".BFL") && name[3..8].bytes().all(|b| b.is_ascii_digit())
}

impl BlackboxCopy {
    pub fn new(firmware_dir: PathBuf, logs_dir: PathBuf) -> Self {
        let since = SystemTime::now() - Duration::from_secs(2);
        Self { firmware_dir, logs_dir, since }
    }

    /// Copies each `LOGnnnnn.BFL` the session wrote to `<logs dir>/<firmware dir name>-LOGnnnnn.BFL`. An identical
    /// copy already there is left alone; a different file under that name gets a `-2`, `-3`, ... before `.BFL`.
    /// Returns the copies made.
    pub fn copy(&self) -> std::io::Result<Vec<PathBuf>> {
        let Ok(entries) = std::fs::read_dir(&self.firmware_dir) else { return Ok(Vec::new()) };
        let prefix = self.firmware_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut logs: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| is_blackbox_log(&e.file_name().to_string_lossy()))
            .filter(|e| e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t >= self.since))
            .map(|e| e.path())
            .collect();
        logs.sort();
        let mut copies = Vec::new();
        for log in logs {
            std::fs::create_dir_all(&self.logs_dir)?;
            let name = log.file_name().expect("listed files have names").to_string_lossy().into_owned();
            if let Some(dest) = destination(&self.logs_dir, &prefix, &name, &std::fs::read(&log)?)? {
                std::fs::copy(&log, &dest)?;
                copies.push(dest);
            }
        }
        Ok(copies)
    }
}

/// Where `name` from firmware directory `prefix` goes: `None` when an identical copy is already there.
fn destination(logs_dir: &Path, prefix: &str, name: &str, bytes: &[u8]) -> std::io::Result<Option<PathBuf>> {
    let stem = name.trim_end_matches(".BFL");
    for n in 1.. {
        let candidate = if n == 1 { format!("{prefix}-{stem}.BFL") } else { format!("{prefix}-{stem}-{n}.BFL") };
        let path = logs_dir.join(candidate);
        match std::fs::read(&path) {
            Ok(existing) if existing == bytes => return Ok(None),
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Some(path)),
            Err(e) => return Err(e),
        }
    }
    unreachable!("the loop returns")
}

impl Drop for BlackboxCopy {
    fn drop(&mut self) {
        match self.copy() {
            Ok(copies) if !copies.is_empty() => tracing::info!("copied {} Blackbox log(s) to {}", copies.len(), self.logs_dir.display()),
            Ok(_) => {}
            Err(e) => tracing::warn!("copying Blackbox logs from {} failed: {e}", self.firmware_dir.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sessions_logs_are_copied_once_under_their_quads_name() {
        let tmp = tempfile::tempdir().unwrap();
        let fw = tmp.path().join("quad-0badf00d");
        let logs = tmp.path().join("logs");
        std::fs::create_dir_all(&fw).unwrap();
        std::fs::write(fw.join("LOG00001.BFL"), b"H Product:Blackbox flight data recorder").unwrap();
        std::fs::write(fw.join("eeprom.bin"), b"not a log").unwrap();
        std::fs::write(fw.join("LOG1.BFL"), b"not SITL's naming").unwrap();
        let copy = BlackboxCopy::new(fw.clone(), logs.clone());
        assert_eq!(copy.copy().unwrap(), [logs.join("quad-0badf00d-LOG00001.BFL")]);
        assert!(copy.copy().unwrap().is_empty(), "an identical copy is not made twice");
        // A different log under the same number (the firmware directory was reset) is kept beside the first.
        std::fs::write(fw.join("LOG00001.BFL"), b"H Product:Blackbox flight data recorder, another flight").unwrap();
        assert_eq!(copy.copy().unwrap(), [logs.join("quad-0badf00d-LOG00001-2.BFL")]);
        assert_eq!(std::fs::read(logs.join("quad-0badf00d-LOG00001.BFL")).unwrap(), b"H Product:Blackbox flight data recorder");
    }

    #[test]
    fn logs_from_before_the_session_and_a_missing_directory_copy_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let fw = tmp.path().join("fw");
        std::fs::create_dir_all(&fw).unwrap();
        std::fs::write(fw.join("LOG00001.BFL"), b"old").unwrap();
        let later = BlackboxCopy { firmware_dir: fw, logs_dir: tmp.path().join("logs"), since: SystemTime::now() + Duration::from_secs(60) };
        assert!(later.copy().unwrap().is_empty());
        let missing = BlackboxCopy::new(tmp.path().join("nope"), tmp.path().join("logs"));
        assert!(missing.copy().unwrap().is_empty());
    }

    #[test]
    fn dropping_copies() {
        let tmp = tempfile::tempdir().unwrap();
        let fw = tmp.path().join("q-1");
        std::fs::create_dir_all(&fw).unwrap();
        std::fs::write(fw.join("LOG00002.BFL"), b"x").unwrap();
        drop(BlackboxCopy::new(fw, tmp.path().join("logs")));
        assert!(tmp.path().join("logs").join("q-1-LOG00002.BFL").exists());
    }
}
````

**Modify `crates/ofs-sim/src/lib.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-sim/src/lib.rs b/crates/ofs-sim/src/lib.rs
index a09ab7d..382abf8 100644
--- a/crates/ofs-sim/src/lib.rs
+++ b/crates/ofs-sim/src/lib.rs
@@ -2,6 +2,7 @@
 // tonic's `Status` is the error type of every gRPC call (the generated traits fix it); boxing it would only move
 // the size elsewhere.
 #![allow(clippy::result_large_err)]
+pub mod blackbox;
 pub mod inputs;
 pub mod listen;
 pub mod pacer;
````

- [ ] **Step 6: The session, the server, the pilot guard and `main`**

Four parts here:
- `session.rs` declares `blackbox` last. Keep it last: the copy must run after the vehicle drops.
- `server.rs` adds the four handlers, the `at_s` dispatch, world resolution, `logs_dir`, the `SetSticks` guard and `sim_server`.
- `streams.rs` refuses a pilot while a file plays.
- `main.rs` serves `sim_server`.

**Modify `crates/ofs-sim/src/session.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-sim/src/session.rs b/crates/ofs-sim/src/session.rs
index 120540d..de18874 100644
--- a/crates/ofs-sim/src/session.rs
+++ b/crates/ofs-sim/src/session.rs
@@ -11,6 +11,7 @@ use ofs_video::link::VideoSync;
 use ofs_video::osd::OsdFrame;
 use tokio::sync::broadcast;
 
+use crate::blackbox::BlackboxCopy;
 use crate::pacer::{OverrunPolicy, Pacer};
 use crate::pb;
 use crate::vehicle::{VideoInfo, Vehicle, VehicleState, VtxInfo};
@@ -49,6 +50,11 @@ pub struct Session {
     /// Unit tests only: the next `step` panics (exercises panic containment).
     #[cfg(test)]
     pub panic_on_step: bool,
+    /// The quad file's stem: recordings are `<logs dir>/<stem>-<n>.mcap`.
+    pub quad_stem: String,
+    /// Betaflight sessions: copies SITL's Blackbox logs when the session ends. Declared last on purpose: fields
+    /// drop in order, so the vehicle (and SITL) is gone before the copy runs.
+    pub blackbox: Option<BlackboxCopy>,
 }
 
 pub type Slot = Option<Session>;
@@ -200,6 +206,8 @@ impl Session {
             last_faults: Vec::new(),
             #[cfg(test)]
             panic_on_step: false,
+            quad_stem: "quad".into(),
+            blackbox: None,
         }
     }
 
@@ -229,11 +237,19 @@ impl Session {
     }
 
     /// Events implied by the vehicle since the last call: radio link up or down, Betaflight restarts,
-    /// VTX changes, dropped UART bytes, video sync lost or regained, faults activating or clearing.
+    /// VTX changes, dropped UART bytes, video sync lost or regained, faults activating or clearing, an inputs file
+    /// finishing, a recording failing.
     pub fn changes(&mut self) -> Vec<pb::Event> {
         let faults = self.vehicle.active_fault_names();
         let s = self.vehicle.state();
-        self.events_for(&s, faults)
+        let mut out = self.events_for(&s, faults);
+        if let Some(name) = self.vehicle.take_finished_inputs() {
+            out.push(event(s.time_s, pb::EventKind::InputsEnded, format!("{name}: finished")));
+        }
+        if let Some(message) = self.vehicle.take_recording_failure() {
+            out.push(event(s.time_s, pb::EventKind::RecordingStopped, format!("recording failed: {message}")));
+        }
+        out
     }
 
     /// The events a vehicle state implies, compared against the remembered previous state; `faults` is the
````

**Modify `crates/ofs-sim/src/server.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-sim/src/server.rs b/crates/ofs-sim/src/server.rs
index a167560..3cb3a16 100644
--- a/crates/ofs-sim/src/server.rs
+++ b/crates/ofs-sim/src/server.rs
@@ -13,15 +13,28 @@ use tokio_stream::wrappers::ReceiverStream;
 use tonic::metadata::MetadataMap;
 use tonic::{Code, Request, Response, Status, Streaming};
 
+use crate::blackbox::BlackboxCopy;
+use crate::inputs;
 use crate::pacer::OverrunPolicy;
-use crate::pb::{self, sim_server::Sim};
+use crate::pb::{self, sim_server::Sim, sim_server::SimServer};
 use crate::runner;
 use crate::session::{event, panic_message, world_msg, RunMode, Session, Shared, Slot};
 use crate::streams;
 use crate::vehicle::{self, BuildOptions, Fault, Sticks};
+use ofs_log::recorder::next_log_path;
+use ofs_log::{RecordError, SignalRate};
 
 pub use ofs_proto::PROTOCOL_VERSION;
 
+/// The largest request the server accepts (tonic's default is 4 MiB): a scripted-inputs CSV of a long flight is
+/// bigger than that.
+pub const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;
+
+/// The gRPC service with this server's limits: what `main` and the tests serve.
+pub fn sim_server(service: SimService) -> SimServer<SimService> {
+    SimServer::new(service).max_decoding_message_size(MAX_REQUEST_BYTES)
+}
+
 #[derive(Clone)]
 pub struct SimService {
     shared: Arc<Shared>,
@@ -58,6 +71,14 @@ fn invalid_state(message: &str) -> Status {
     error("invalid_state", Code::FailedPrecondition, message)
 }
 
+fn record_error(e: RecordError) -> Status {
+    match e {
+        RecordError::AlreadyRecording | RecordError::NotRecording => invalid_state(&e.to_string()),
+        RecordError::Spec(m) => error("invalid_argument", Code::InvalidArgument, m),
+        RecordError::Io(_) => error("internal", Code::Internal, e.to_string()),
+    }
+}
+
 /// NED metres from the proto's optional Vec3 (absent = the origin).
 fn dvec3(v: Option<pb::Vec3>) -> DVec3 {
     let v = v.unwrap_or_default();
@@ -112,6 +133,12 @@ impl SimService {
         *self.shared.lock() = None;
     }
 
+    /// Where recordings and copied Blackbox logs land: `<data_dir>/logs`.
+    pub fn logs_dir(&self) -> PathBuf {
+        let logs = self.data_dir.join("logs");
+        std::path::absolute(&logs).unwrap_or(logs)
+    }
+
     #[cfg(test)]
     fn runner_finished(&self) -> bool {
         self.runner.lock().unwrap().as_ref().is_none_or(|h| h.is_finished())
@@ -180,7 +207,11 @@ impl Sim for SimService {
         if v != PROTOCOL_VERSION {
             return Err(error("protocol", Code::FailedPrecondition, format!("client speaks protocol {v}, server speaks {PROTOCOL_VERSION}")));
         }
-        Ok(Response::new(pb::HandshakeReply { protocol_version: PROTOCOL_VERSION, server_version: env!("CARGO_PKG_VERSION").into() }))
+        Ok(Response::new(pb::HandshakeReply {
+            protocol_version: PROTOCOL_VERSION,
+            server_version: env!("CARGO_PKG_VERSION").into(),
+            logs_dir: self.logs_dir().display().to_string(),
+        }))
     }
 
     async fn load(&self, req: Request<pb::LoadRequest>) -> Result<Response<pb::LoadReply>, Status> {
@@ -202,16 +233,22 @@ impl Sim for SimService {
         let world = if req.world_path.is_empty() {
             WorldConfig::open_field()
         } else {
-            ofs_config::world::load(Path::new(&req.world_path)).map_err(|e| error("config", Code::InvalidArgument, e.to_string()))?
+            let path = ofs_config::world::resolve(&req.world_path, Path::new(&req.quad_path));
+            ofs_config::world::load(&path).map_err(|e| error("config", Code::InvalidArgument, e.to_string()))?
         };
         let opts = BuildOptions { seed: req.seed, data_dir: self.data_dir.clone(), fc_override: req.open_loop_fc.then_some(FcKind::OpenLoop), world };
         let keep_alive = req.keep_alive;
+        let quad_stem = cfg.source_path.file_stem().map_or_else(|| "quad".into(), |s| s.to_string_lossy().into_owned());
+        let blackbox = (opts.fc_override.unwrap_or(cfg.fc.kind) == FcKind::Sitl)
+            .then(|| BlackboxCopy::new(vehicle::firmware_dir(&self.data_dir, &cfg.source_path), self.logs_dir()));
         self.blocking(move |shared, slot| {
             *slot = None; // stop the previous vehicle (and its SITL) before the new one binds the ports
             let vehicle = vehicle::build(&cfg, &opts).map_err(|e| sim_error(&e))?;
             let configurator_address = vehicle.configurator_address().unwrap_or_default();
             let mut session = Session::new(vehicle, mode, policy, keep_alive);
             session.watched = shared.watchers.load(Ordering::Acquire) > 0;
+            session.quad_stem = quad_stem;
+            session.blackbox = blackbox;
             *slot = Some(session);
             Ok(pb::LoadReply { quad_name: cfg.name.clone(), base_hz: cfg.sim.base_hz, configurator_address })
         })
@@ -221,7 +258,12 @@ impl Sim for SimService {
     async fn set_sticks(&self, req: Request<pb::Sticks>) -> Result<Response<pb::Empty>, Status> {
         let sticks = parse_sticks(req.into_inner())?;
         self.blocking(move |_, slot| {
-            loaded(slot)?.vehicle.set_sticks(&sticks);
+            let s = loaded(slot)?;
+            if let Some(playing) = s.vehicle.inputs_playing() {
+                let message = format!("{playing} is playing: SetSticks would be overwritten (call StopInputs first)");
+                return Err(error("invalid_argument", Code::InvalidArgument, message));
+            }
+            s.vehicle.set_sticks(&sticks);
             Ok(pb::Empty {})
         })
         .await
@@ -360,7 +402,9 @@ impl Sim for SimService {
     }
 
     async fn inject_fault(&self, req: Request<pb::Fault>) -> Result<Response<pb::Empty>, Status> {
-        let fault = match req.into_inner().kind {
+        let req = req.into_inner();
+        let at_s = req.at_s;
+        let fault = match req.kind {
             Some(pb::fault::Kind::RadioLinkLoss(_)) => Fault::RadioLinkLoss,
             Some(pb::fault::Kind::RadioLoss(m)) => Fault::RadioLoss { fraction: m.fraction },
             Some(pb::fault::Kind::BurstLoss(m)) => {
@@ -380,7 +424,12 @@ impl Sim for SimService {
             None => return Err(error("invalid_argument", Code::InvalidArgument, "the fault has no kind")),
         };
         self.blocking(move |_, slot| {
-            loaded(slot)?.vehicle.set_fault(fault, true).map_err(|e| sim_error(&e))?;
+            let vehicle = &mut loaded(slot)?.vehicle;
+            match at_s {
+                Some(at_s) => vehicle.schedule_fault(fault, at_s),
+                None => vehicle.set_fault(fault, true),
+            }
+            .map_err(|e| sim_error(&e))?;
             Ok(pb::Empty {})
         })
         .await
@@ -393,6 +442,72 @@ impl Sim for SimService {
         })
         .await
     }
+
+    async fn start_recording(&self, req: Request<pb::StartRecordingRequest>) -> Result<Response<pb::RecordingStarted>, Status> {
+        let requests: Vec<SignalRate> = req
+            .into_inner()
+            .signals
+            .into_iter()
+            .map(|s| SignalRate { pattern: s.pattern, rate_hz: (s.rate_hz != 0.0).then_some(s.rate_hz) })
+            .collect();
+        let logs = self.logs_dir();
+        self.blocking(move |shared, slot| {
+            let s = loaded(slot)?;
+            if s.vehicle.is_recording() {
+                return Err(record_error(RecordError::AlreadyRecording));
+            }
+            let path = next_log_path(&logs, &s.quad_stem).map_err(|e| record_error(RecordError::Io(format!("{}: {e}", logs.display()))))?;
+            let signals = s.vehicle.start_recording(&requests, &path).map_err(record_error)?;
+            let path = path.display().to_string();
+            shared.publish([event(s.vehicle.time_s(), pb::EventKind::RecordingStarted, path.clone())]);
+            Ok(pb::RecordingStarted { path, signals })
+        })
+        .await
+    }
+
+    async fn stop_recording(&self, _req: Request<pb::Empty>) -> Result<Response<pb::RecordingSummary>, Status> {
+        self.blocking(|shared, slot| {
+            let s = loaded(slot)?;
+            let summary = s.vehicle.stop_recording().map_err(record_error)?;
+            let path = summary.path.display().to_string();
+            let message = format!("{path}: {} messages", summary.messages);
+            shared.publish([event(s.vehicle.time_s(), pb::EventKind::RecordingStopped, message)]);
+            Ok(pb::RecordingSummary { path, messages: summary.messages, first_s: summary.first_s, last_s: summary.last_s })
+        })
+        .await
+    }
+
+    async fn play_inputs(&self, req: Request<pb::InputsFile>) -> Result<Response<pb::Empty>, Status> {
+        let file = req.into_inner();
+        let name = if file.name.is_empty() { "inputs".to_string() } else { file.name };
+        let inputs = inputs::parse(&name, &file.csv).map_err(|m| error("config", Code::InvalidArgument, m))?;
+        self.blocking(move |shared, slot| {
+            let s = loaded(slot)?;
+            if shared.pilot.load(Ordering::Acquire) {
+                return Err(invalid_state("a pilot is connected: scripted inputs would fight the pilot's sticks"));
+            }
+            if let Some(playing) = s.vehicle.inputs_playing() {
+                return Err(invalid_state(&format!("{playing} is still playing; call StopInputs first")));
+            }
+            let message = format!("{name}: {} rows over {} s", inputs.rows.len(), inputs.duration_s());
+            s.vehicle.play_inputs(inputs).map_err(|e| sim_error(&e))?;
+            shared.publish([event(s.vehicle.time_s(), pb::EventKind::InputsStarted, message)]);
+            shared.publish(s.changes()); // a file whose rows are all at t_s = 0 has already finished
+            Ok(pb::Empty {})
+        })
+        .await
+    }
+
+    async fn stop_inputs(&self, _req: Request<pb::Empty>) -> Result<Response<pb::Empty>, Status> {
+        self.blocking(|shared, slot| {
+            let s = loaded(slot)?;
+            if let Some(name) = s.vehicle.stop_inputs() {
+                shared.publish([event(s.vehicle.time_s(), pb::EventKind::InputsEnded, format!("{name}: stopped"))]);
+            }
+            Ok(pb::Empty {})
+        })
+        .await
+    }
 }
 
 #[cfg(test)]
````

**Modify `crates/ofs-sim/src/streams.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-sim/src/streams.rs b/crates/ofs-sim/src/streams.rs
index 82a14ed..8e557f6 100644
--- a/crates/ofs-sim/src/streams.rs
+++ b/crates/ofs-sim/src/streams.rs
@@ -192,6 +192,10 @@ pub async fn pilot(
         if !setup_link.alive.load(Ordering::Acquire) {
             return Err(Status::cancelled("the pilot went away before connecting"));
         }
+        if let Some(playing) = session.vehicle.inputs_playing() {
+            let message = format!("{playing} is playing scripted inputs; StopInputs before piloting");
+            return Err(error("invalid_state", Code::FailedPrecondition, message));
+        }
         session.vehicle.set_sticks(&first_sticks);
         session.vehicle.set_transmitter(true);
         setup_link.session.store(session.id, Ordering::Release);
````

**Modify `crates/ofs-sim/src/main.rs`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/crates/ofs-sim/src/main.rs b/crates/ofs-sim/src/main.rs
index 22c8ec9..4b89c90 100644
--- a/crates/ofs-sim/src/main.rs
+++ b/crates/ofs-sim/src/main.rs
@@ -3,8 +3,7 @@ use std::path::PathBuf;
 use std::time::Duration;
 
 use clap::Parser;
-use ofs_sim::pb::sim_server::SimServer;
-use ofs_sim::server::SimService;
+use ofs_sim::server::{sim_server, SimService};
 
 #[derive(Parser)]
 #[command(about = "Open FPV Sim headless server")]
@@ -55,7 +54,7 @@ async fn main() -> Result<(), Box<dyn std::error::Error>> {
     eprintln!("ofs-sim {} listening on {}", env!("CARGO_PKG_VERSION"), args.listen);
     let service = SimService::new(args.data_dir);
     let (signalled, on_signal) = tokio::sync::oneshot::channel::<()>();
-    let server = tonic::transport::Server::builder().add_service(SimServer::new(service.clone())).serve_with_shutdown(
+    let server = tonic::transport::Server::builder().add_service(sim_server(service.clone())).serve_with_shutdown(
         args.listen,
         async move {
             shutdown_signal().await;
````

Run: `cargo test --workspace --locked`
Expected: PASS: every crate, including the 8 new `grpc.rs` tests, the world test and the 3 `blackbox` unit tests. If the runner flake appears, re-run.

Run the existing Python suite against the new server (it must still pass: the version moved together):
```bash
cargo build -p ofs-sim && python -m pytest python/tests -q
```
Expected: PASS, with the SITL tests skipped unless `OFS_SITL_LAUNCH` is set.

- [ ] **Step 7: Lint and commit**

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`. Expected: no output.

```bash
git add proto crates python/ofs/client.py python/ofs/v1
git commit -m "feat(proto)!: protocol 9 - recording, scripted inputs, faults at a time, world names, Blackbox copies"
```

---

### Task 5: Python: recordings, the reader, scripted inputs, faults `at=`, the `open-fpv-sim` package

The Python API over protocol 9, and the distribution's new name. Re-install the package first: the old editable `ofs` distribution would shadow the renamed one.

```bash
python -m pip uninstall -y ofs
python -m pip install -e "python[dev]"     # after this task's pyproject change; brings mcap, pandas, pyarrow, build
```

**Files:**
- Create: `python/README.md`
- Create: `python/ofs/reader.py`
- Create: `python/ofs/recording.py`
- Create: `python/tests/test_recording.py`
- Create: `python/tests/test_scripting.py`
- Modify: `python/ofs/__init__.py`
- Modify: `python/ofs/client.py`
- Modify: `python/ofs/faults.py`
- Modify: `python/pyproject.toml`

**Interfaces:**
- Consumes from Task 4: the protocol 9 stubs.
- Produces:
  - `ofs.reader.{read, to_dataframe, to_csv, to_parquet, near_misses, Log}`.
  - `ofs.recording.Recording` as `sim.recording`: `start(patterns=None) -> list[str]`, `stop() -> Log`, `active`, `path`, `last`.
  - `Sim.load(..., record=None)`, `Sim.log`, `play_inputs(path)`, `stop_inputs()`, `blackbox_files()` and `logs_dir`.
  - Every fault's keyword-only `at`.

- [ ] **Step 1: The tests**

**Create `python/tests/test_recording.py`:**

````python
"""Recording the bus and reading it back (spec §4), open loop: no Betaflight needed."""
import hashlib
import math
import pathlib
import time

import pytest

import ofs
from conftest import QUAD

pytest.importorskip("mcap")
pd = pytest.importorskip("pandas")


def _climb(sim, seed=1):
    sim.load(QUAD, seed=seed, open_loop_fc=True)
    sim.set_sticks(throttle=0.6)
    sim.run(1.0)


def test_a_lockstep_load_records_and_sim_log_reads_it(sim):
    _climb(sim)
    assert sim.recording.active
    log = sim.log
    assert not sim.recording.active, "reading the log finished the file"
    assert pathlib.Path(log.path).parent == pathlib.Path(sim.logs_dir)
    assert pathlib.Path(log.path).name == "opendrone-5f-freestyle-1.mcap"
    assert log.messages > 10_000 and log.last_s == pytest.approx(1.0)
    df = log.to_dataframe(["imu.gyro_frd_radps", "motor.*.omega_radps", "battery.voltage_v"])
    assert df.index.name == "time_s"
    assert list(df.columns) == ["imu.gyro_frd_radps.x", "imu.gyro_frd_radps.y", "imu.gyro_frd_radps.z",
                                "motor.0.omega_radps", "motor.1.omega_radps", "motor.2.omega_radps",
                                "motor.3.omega_radps", "battery.voltage_v"]
    assert df.index[-1] == pytest.approx(1.0)
    gyro = df["imu.gyro_frd_radps.x"].dropna()
    assert len(gyro) == 1000, "the gyro at the default 1 kHz"
    battery = df["battery.voltage_v"].dropna()
    assert len(battery) == 10, "the battery at the default 10 Hz; NaN between its records"
    omega = df["motor.0.omega_radps"].dropna()
    assert omega.iloc[-1] > omega.iloc[0] + 500, "the motor spun up"
    assert sim.log.path == log.path, "a second read returns the same finished recording"


def test_a_real_time_or_unrecorded_session_has_no_log(sim):
    sim.load(QUAD, open_loop_fc=True, record=False)
    assert not sim.recording.active
    with pytest.raises(ofs.InvalidState):
        sim.log
    sim.load(QUAD, open_loop_fc=True, mode="realtime")
    assert not sim.recording.active, "real-time sessions record only when asked"


def test_a_real_time_session_records_when_asked(sim):
    sim.load(QUAD, open_loop_fc=True, mode="realtime")
    sim.recording.start(["battery.voltage_v"])
    sim.start()
    time.sleep(0.5)
    sim.pause()
    log = sim.recording.stop()
    assert log.messages >= 3, "10 Hz over about half a second of wall-paced simulated time"
    assert log.to_dataframe().shape[0] == log.messages


def test_a_recording_can_choose_its_signals_and_rates(sim):
    sim.load(QUAD, open_loop_fc=True, record=False)
    names = sim.recording.start(["battery.voltage_v", ("motor.*.omega_radps", 100)])
    assert names == ["battery.voltage_v", "motor.0.omega_radps", "motor.1.omega_radps", "motor.2.omega_radps",
                     "motor.3.omega_radps"]
    sim.run(0.5)
    log = sim.recording.stop()
    assert log.messages == 5 + 4 * 50
    assert log.signals() == names
    with pytest.raises(ofs.InvalidState):
        sim.recording.stop()
    with pytest.raises(ofs.InvalidArgument, match=r"similar: fault.imu.gyro_bias_radps, imu.gyro_frd_radps"):
        sim.recording.start(["gyro.*"])


def test_reading_a_pattern_the_file_lacks_names_the_near_misses(sim):
    _climb(sim)
    with pytest.raises(KeyError, match=r"pattern 'battery.voltage' matches no recorded signal \(similar: "
                                       r"battery.voltage_v"):
        sim.log.to_dataframe(["battery.voltage"])


def test_csv_and_parquet_exports(sim, tmp_path):
    pytest.importorskip("pyarrow")
    _climb(sim)
    log = sim.log
    out = log.to_csv(tmp_path / "battery.csv", ["battery.voltage_v", "battery.current_a"])
    lines = pathlib.Path(out).read_text().splitlines()
    assert lines[0] == "time_s,battery.voltage_v,battery.current_a", "columns in pattern order"
    assert len(lines) == 1 + 10
    t, voltage, current = lines[1].split(",")
    assert float(t) == pytest.approx(0.1) and float(voltage) > 20.0
    parquet = log.to_parquet(tmp_path / "imu.parquet", "imu.*")
    back = pd.read_parquet(parquet)
    assert back.shape == log.to_dataframe("imu.*").shape


def test_a_new_load_finishes_the_previous_recording(sim):
    _climb(sim)
    first = sim.recording.path
    _climb(sim)
    assert sim.recording.path != first
    assert sim.recording.last.path == first
    assert ofs.reader.Log(first).to_dataframe("battery.voltage_v").shape[0] == 10, "the server finished it"


def _sha(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


def test_two_identical_lockstep_sessions_write_identical_files(sim):
    """Spec §5.5 and §8.4: same quad, seed and inputs in lockstep give identical MCAP files."""
    _climb(sim, seed=3)
    a = sim.log.path
    _climb(sim, seed=3)
    b = sim.log.path
    _climb(sim, seed=4)
    c = sim.log.path
    assert a != b
    assert _sha(a) == _sha(b)
    assert _sha(a) != _sha(c)
    assert not math.isnan(ofs.reader.to_dataframe(a, "body.pos_ned_m").iloc[-1, 2])
````

**Create `python/tests/test_scripting.py`:**

````python
"""Scripted inputs, scheduled faults and world names (spec §5, §6.2), open loop: no Betaflight needed."""
import pytest

import ofs
from conftest import QUAD


def _events(sim, kinds):
    return [(e.kind, e.message) for e in sim.events() if e.kind in kinds]


def test_an_inputs_file_plays_from_now_and_refuses_set_sticks(sim, tmp_path):
    sim.load(QUAD, open_loop_fc=True)
    sim.run(0.2)
    path = tmp_path / "climb.csv"
    path.write_text("# a throttle step\nt_s,throttle\n0,0.3\n0.5,1\n")
    sim.play_inputs(path)
    with pytest.raises(ofs.InvalidArgument, match="climb.csv is playing"):
        sim.set_sticks(throttle=0.0)
    assert sim.run(0.49).motor_cmd == (0.3,) * 4
    assert sim.run(0.02).motor_cmd == (1.0,) * 4, "the 0.5 s row applied at 0.7 s"
    sim.set_sticks(throttle=0.0)  # the file finished
    sim.run(0.05)
    assert _events(sim, {"inputs_started", "inputs_ended"}) == [
        ("inputs_started", "climb.csv: 2 rows over 0.5 s"), ("inputs_ended", "climb.csv: finished")]


def test_a_malformed_inputs_file_names_every_problem_with_its_line(sim, tmp_path):
    sim.load(QUAD, open_loop_fc=True)
    path = tmp_path / "bad.csv"
    path.write_text("t_s,throttle,wobble\n0,0.5,1\n0.5,150,0\n")
    with pytest.raises(ofs.ConfigError) as e:
        sim.play_inputs(path)
    assert str(e.value).splitlines() == [
        "bad.csv line 1: unknown column 'wobble' (expected t_s, roll, pitch, yaw, throttle, aux1, aux2, aux3, aux4)",
        "bad.csv line 3: throttle 150 is outside [0, 1]",
    ]


def test_stop_inputs_keeps_the_last_sticks(sim, tmp_path):
    sim.load(QUAD, open_loop_fc=True)
    path = tmp_path / "long.csv"
    path.write_text("t_s,throttle\n0,0.4\n30,0\n")
    sim.play_inputs(path)
    sim.run(0.1)
    sim.stop_inputs()
    sim.stop_inputs()  # nothing playing: fine
    assert sim.run(0.1).motor_cmd == (0.4,) * 4
    sim.set_sticks(throttle=0.0)


def test_a_long_inputs_file_plays(sim, tmp_path):
    """Beyond gRPC's default 4 MB message: the server accepts requests up to 64 MiB."""
    sim.load(QUAD, open_loop_fc=True)
    path = tmp_path / "long.csv"
    path.write_text("t_s,throttle\n" + "".join(f"{i * 0.001:.3f},0.5\n" for i in range(400_000)))
    assert path.stat().st_size > 4 * 1024 * 1024
    sim.play_inputs(path)
    assert sim.run(0.1).motor_cmd == (0.5,) * 4
    sim.stop_inputs()


def test_a_fault_waits_for_its_time(sim):
    sim.load(QUAD, open_loop_fc=True)
    sim.set_sticks(throttle=1.0)
    sim.inject(ofs.faults.MotorFailure(motor=2, at=0.5))
    s = sim.run(0.5)
    assert _events(sim, {"fault_injected"}) == [("fault_injected", "motor_failure")], "active from 0.5 s"
    before = s.motor_rpm
    s = sim.run(0.5)
    assert s.motor_rpm[2] < before[2] - 1000, "motor 2 died at 0.5 s and coasts down"
    with pytest.raises(ofs.InvalidArgument, match=r"fault time 0.2 s has passed \(the session is at 1 s\)"):
        sim.inject(ofs.faults.RadioLinkLoss(at=0.2))


def test_clear_faults_drops_a_fault_still_waiting(sim):
    sim.load(QUAD, open_loop_fc=True)
    sim.inject(ofs.faults.RadioLinkLoss(at=0.3))
    sim.clear_faults()
    assert sim.run(1.0).radio.link_up


def test_at_is_keyword_only_and_optional():
    assert not ofs.faults.MotorFailure(2)._to_pb().HasField("at_s")
    assert ofs.faults.MotorFailure(2, 0.5, at=3.0)._to_pb().at_s == 3.0
    with pytest.raises(TypeError):
        ofs.faults.RadioLoss(0.1, 2.0)  # at is not positional


def test_a_world_can_be_named(sim):
    sim.load(QUAD, open_loop_fc=True, world="flat")
    assert sim.get_world().name == "Flat field"
    with pytest.raises(ofs.ConfigError, match="nowhere.toml"):
        sim.load(QUAD, open_loop_fc=True, world="nowhere")


def test_an_open_loop_session_leaves_no_blackbox_logs(sim):
    sim.load(QUAD, open_loop_fc=True)
    sim.run(0.1)
    assert sim.blackbox_files() == []
    assert sim.logs_dir.endswith("logs")
````

Run: `cargo build -p ofs-sim && python -m pytest python/tests/test_recording.py python/tests/test_scripting.py -q`
Expected: FAIL (no `sim.recording`, no `play_inputs`, no `at`).

- [ ] **Step 2: The reader and the recording controller**

**Create `python/ofs/reader.py`:**

````python
"""Reading the simulator's MCAP recordings: a wide table, one row per recorded time and one column per signal
component (`imu.gyro_frd_radps.x`, `battery.voltage_v`, `body.att_q.w`, ...), indexed by `time_s`.

Needs the `log` extra (`pip install open-fpv-sim[log]`): `mcap` for every function, plus `pandas` for
`to_dataframe` and `to_parquet` (Parquet also needs `pyarrow`). Patterns follow the server's rules: `*` matches
any run of characters, dots included, and `?` exactly one character.
"""
from __future__ import annotations

import csv
import fnmatch
import json
import os
import re

_INSTALL = "install the log extra: pip install open-fpv-sim[log]"


def _mcap_reader():
    try:
        from mcap.reader import make_reader
    except ImportError as e:
        raise ImportError(f"reading recordings needs the mcap package; {_INSTALL}") from e
    return make_reader


def _pattern_re(pattern: str):
    # fnmatch's `[...]` classes are not part of the pattern language: escape them before translating.
    return re.compile(fnmatch.translate(pattern.replace("[", "[[]")))


def near_misses(pattern: str, names) -> list:
    """Up to five names sharing a literal piece of `pattern` (its dot-separated parts without wildcards): more
    shared pieces first, then names starting with the first piece, then name order (the server's ranking)."""
    parts = [p.replace("*", "").replace("?", "") for p in pattern.split(".")]
    parts = [p for p in parts if p]
    scored = [(sum(p in n for p in parts), bool(parts) and n.startswith(parts[0]), n) for n in names]
    scored = [s for s in scored if s[0] > 0]
    scored.sort(key=lambda s: (-s[0], not s[1], s[2]))
    return [n for _, _, n in scored[:5]]


def _select(patterns, topics) -> list:
    """The topics `patterns` select, in pattern order (each pattern's matches in name order, no repeats); every
    topic for None. A pattern matching none raises KeyError naming its near misses."""
    if patterns is None:
        return list(topics)
    if isinstance(patterns, str):
        patterns = [patterns]
    problems, chosen = [], []
    for p in patterns:
        rx = _pattern_re(p)
        hits = [t for t in topics if rx.fullmatch(t)]
        if not hits:
            near = near_misses(p, topics)
            problems.append(f"pattern '{p}' matches no recorded signal" + (f" (similar: {', '.join(near)})" if near else ""))
        chosen.extend(t for t in hits if t not in chosen)
    if problems:
        raise KeyError("; ".join(problems))
    return chosen


def read(path, patterns=None):
    """The selected signals as (columns, times_ns, rows): `rows[i]` maps column -> value at `times_ns[i]`. A scalar
    signal is one column named after it; a vector or quaternion is one column per component (`<name>.x`)."""
    make_reader = _mcap_reader()
    with open(path, "rb") as f:
        reader = make_reader(f)
        summary = reader.get_summary()
        if summary is None:
            raise ValueError(f"{path}: not a finished recording (stop the recording first)")
        topics = sorted(c.topic for c in summary.channels.values())
        selected = _select(patterns, topics)
        rows, columns = {}, {}
        for _, channel, message in reader.iter_messages(topics=selected, log_time_order=True):
            record = json.loads(message.data)
            names = columns.get(channel.topic)
            if names is None:
                names = columns[channel.topic] = [channel.topic] if list(record) == ["v"] else [
                    f"{channel.topic}.{k}" for k in record]
            row = rows.setdefault(message.log_time, {})
            for name, value in zip(names, record.values()):
                row[name] = value
    times = sorted(rows)
    return [c for t in selected for c in columns.get(t, [])], times, [rows[t] for t in times]


def to_dataframe(path, patterns=None):
    """A pandas DataFrame indexed by `time_s`, one column per signal component; NaN where a slower signal has no
    record at that time."""
    try:
        import pandas as pd
    except ImportError as e:
        raise ImportError(f"to_dataframe needs pandas; {_INSTALL}") from e
    columns, times, rows = read(path, patterns)
    index = pd.Index([t / 1e9 for t in times], name="time_s")
    return pd.DataFrame.from_records(rows, index=index, columns=columns)


def to_csv(path, out, patterns=None) -> str:
    """Writes the selected signals as CSV (`time_s` first, empty cells where a signal has no record). Needs only
    `mcap`. Returns `out`."""
    columns, times, rows = read(path, patterns)
    with open(out, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["time_s", *columns])
        for t, row in zip(times, rows):
            w.writerow([repr(t / 1e9), *(repr(row[c]) if c in row else "" for c in columns)])
    return os.fspath(out)


def to_parquet(path, out, patterns=None) -> str:
    """Writes the selected signals as Parquet (`df.to_parquet`; needs pandas and pyarrow). Returns `out`."""
    to_dataframe(path, patterns).to_parquet(out)
    return os.fspath(out)


class Log:
    """One finished recording on disk (what `Sim.log` and `Sim.recording.stop()` hand back)."""

    def __init__(self, path, messages: int = 0, first_s: float = 0.0, last_s: float = 0.0):
        self.path = os.fspath(path)
        self.messages = messages
        self.first_s = first_s
        self.last_s = last_s

    def signals(self) -> list:
        """The recorded signals' names (bus names), sorted."""
        make_reader = _mcap_reader()
        with open(self.path, "rb") as f:
            return sorted(c.topic for c in make_reader(f).get_summary().channels.values())

    def to_dataframe(self, patterns=None):
        return to_dataframe(self.path, patterns)

    def to_csv(self, out, patterns=None) -> str:
        return to_csv(self.path, out, patterns)

    def to_parquet(self, out, patterns=None) -> str:
        return to_parquet(self.path, out, patterns)

    def __repr__(self):
        return f"Log({self.path!r}, messages={self.messages}, first_s={self.first_s}, last_s={self.last_s})"
````

**Create `python/ofs/recording.py`:**

````python
"""Recording the simulator's signal bus to MCAP (`Sim.recording`): one recording at a time, in a file the server
names under its logs directory (`Sim.logs_dir`)."""
from __future__ import annotations

from ofs.v1 import sim_pb2 as pb

from .reader import Log


def _signal(p) -> pb.RecordingSignal:
    if isinstance(p, str):
        return pb.RecordingSignal(pattern=p)
    pattern, rate_hz = p
    return pb.RecordingSignal(pattern=pattern, rate_hz=float(rate_hz))


class Recording:
    """`sim.recording`: start and stop the session's recording. A lockstep `load` starts one with the default
    rates (`load(record=False)` does not); `sim.log` stops the running one and reads it."""

    def __init__(self, sim):
        self._sim = sim
        self.path = None  # the running recording's file, None when none runs
        self.last = None  # the last finished recording, as a reader.Log

    @property
    def active(self) -> bool:
        return self.path is not None

    def start(self, patterns=None) -> list:
        """Starts recording. `patterns`: None for every signal at the default table's rates, or a list of patterns
        (`"imu.*"`, at the default rate for each signal) and `(pattern, rate_hz)` pairs; only matching signals are
        recorded. Returns the recorded signals' names."""
        signals = [_signal(p) for p in ([] if patterns is None else ([patterns] if isinstance(patterns, str) else patterns))]
        reply = self._sim._call(self._sim._stub.StartRecording, pb.StartRecordingRequest(signals=signals))
        self.path = reply.path
        return list(reply.signals)

    def stop(self) -> Log:
        """Finishes the running recording's file and returns it as a `reader.Log`."""
        m = self._sim._call(self._sim._stub.StopRecording, pb.Empty())
        self.path = None
        self.last = Log(m.path, messages=m.messages, first_s=m.first_s, last_s=m.last_s)
        return self.last

    def _session_ended(self) -> None:
        """A new session replaced the old one: its running recording was finished by the server as it ended."""
        if self.path is not None:
            self.last = Log(self.path)
            self.path = None
````

- [ ] **Step 3: Faults with `at`, the client API, the package's exports**

**Modify `python/ofs/faults.py`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/python/ofs/faults.py b/python/ofs/faults.py
index afd5fcc..d99c6a8 100644
--- a/python/ofs/faults.py
+++ b/python/ofs/faults.py
@@ -1,15 +1,28 @@
-"""Faults to inject with `Sim.inject` (spec §6.2): the v1 catalog, radio to video."""
-from dataclasses import dataclass
+"""Faults to inject with `Sim.inject` (spec §6.2): the v1 catalog, radio to video.
+
+Every fault takes a keyword-only `at`: the simulated time (s) it activates at, on the first tick at or after it.
+Without `at` it activates at once. A time the session has already passed is refused (`InvalidArgument`).
+"""
+from dataclasses import dataclass, field
 
 from ofs.v1 import sim_pb2 as pb
 
 
+def _fault(at, **kind):
+    f = pb.Fault(**kind)
+    if at is not None:
+        f.at_s = float(at)
+    return f
+
+
 @dataclass(frozen=True)
 class RadioLinkLoss:
     """Every radio uplink packet is lost while active: the receiver goes silent and Betaflight fails safe."""
 
+    at: float | None = field(default=None, kw_only=True)
+
     def _to_pb(self):
-        return pb.Fault(radio_link_loss=pb.RadioLinkLoss())
+        return _fault(self.at, radio_link_loss=pb.RadioLinkLoss())
 
 
 @dataclass(frozen=True)
@@ -17,9 +30,10 @@ class RadioLoss:
     """Extra per-packet uplink loss while active: `fraction` of packets are dropped on top of the link model's own."""
 
     fraction: float
+    at: float | None = field(default=None, kw_only=True)
 
     def _to_pb(self):
-        return pb.Fault(radio_loss=pb.RadioLoss(fraction=self.fraction))
+        return _fault(self.at, radio_loss=pb.RadioLoss(fraction=self.fraction))
 
 
 @dataclass(frozen=True)
@@ -28,9 +42,10 @@ class BurstLoss:
 
     on_ms: int
     off_ms: int
+    at: float | None = field(default=None, kw_only=True)
 
     def _to_pb(self):
-        return pb.Fault(burst_loss=pb.BurstLoss(on_ms=self.on_ms, off_ms=self.off_ms))
+        return _fault(self.at, burst_loss=pb.BurstLoss(on_ms=self.on_ms, off_ms=self.off_ms))
 
 
 @dataclass(frozen=True)
@@ -39,9 +54,10 @@ class MotorFailure:
 
     motor: int
     scale: float = 0.0
+    at: float | None = field(default=None, kw_only=True)
 
     def _to_pb(self):
-        return pb.Fault(motor_failure=pb.MotorFailure(motor=self.motor, scale=self.scale))
+        return _fault(self.at, motor_failure=pb.MotorFailure(motor=self.motor, scale=self.scale))
 
 
 @dataclass(frozen=True)
@@ -51,10 +67,11 @@ class PropDamage:
     prop: int
     thrust_scale: float = 1.0
     imbalance_kgm: float = 0.0
+    at: float | None = field(default=None, kw_only=True)
 
     def _to_pb(self):
-        return pb.Fault(prop_damage=pb.PropDamage(prop=self.prop, thrust_scale=self.thrust_scale,
-                                                  imbalance_kgm=self.imbalance_kgm))
+        return _fault(self.at, prop_damage=pb.PropDamage(prop=self.prop, thrust_scale=self.thrust_scale,
+                                                         imbalance_kgm=self.imbalance_kgm))
 
 
 @dataclass(frozen=True)
@@ -64,10 +81,11 @@ class WeakCell:
     cell: int
     r_offset_ohm: float = 0.01
     v_offset_v: float = 0.05
+    at: float | None = field(default=None, kw_only=True)
 
     def _to_pb(self):
-        return pb.Fault(weak_cell=pb.WeakCell(cell=self.cell, r_offset_ohm=self.r_offset_ohm,
-                                              v_offset_v=self.v_offset_v))
+        return _fault(self.at, weak_cell=pb.WeakCell(cell=self.cell, r_offset_ohm=self.r_offset_ohm,
+                                                     v_offset_v=self.v_offset_v))
 
 
 @dataclass(frozen=True)
@@ -76,10 +94,11 @@ class GyroFault:
 
     bias_radps: tuple = (0.0, 0.0, 0.0)
     noise_scale: float = 1.0
+    at: float | None = field(default=None, kw_only=True)
 
     def _to_pb(self):
         x, y, z = self.bias_radps
-        return pb.Fault(gyro_fault=pb.GyroFault(bias_radps=pb.Vec3(x=x, y=y, z=z), noise_scale=self.noise_scale))
+        return _fault(self.at, gyro_fault=pb.GyroFault(bias_radps=pb.Vec3(x=x, y=y, z=z), noise_scale=self.noise_scale))
 
 
 @dataclass(frozen=True)
@@ -89,8 +108,9 @@ class VtxInterference:
     freq_mhz: float
     power_mw: float
     position_ned_m: tuple
+    at: float | None = field(default=None, kw_only=True)
 
     def _to_pb(self):
         x, y, z = self.position_ned_m
-        return pb.Fault(vtx_interference=pb.VtxInterference(freq_mhz=self.freq_mhz, power_mw=self.power_mw,
-                                                            position_ned_m=pb.Vec3(x=x, y=y, z=z)))
+        return _fault(self.at, vtx_interference=pb.VtxInterference(freq_mhz=self.freq_mhz, power_mw=self.power_mw,
+                                                                   position_ned_m=pb.Vec3(x=x, y=y, z=z)))
````

**Modify `python/ofs/client.py`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/python/ofs/client.py b/python/ofs/client.py
index e13fccb..bdff324 100644
--- a/python/ofs/client.py
+++ b/python/ofs/client.py
@@ -5,6 +5,7 @@ import collections
 import collections.abc
 import math
 import os
+import pathlib
 import shutil
 import socket
 import subprocess
@@ -17,7 +18,9 @@ import grpc
 from ofs.v1 import sim_pb2 as pb
 from ofs.v1 import sim_pb2_grpc as pbg
 
-from .errors import ProtocolMismatch, from_rpc_error
+from .errors import InvalidState, ProtocolMismatch, from_rpc_error
+from .reader import Log
+from .recording import Recording
 
 PROTOCOL_VERSION = 9
 UNLOAD_TIMEOUT_S = 5.0
@@ -328,6 +331,7 @@ class Sim:
         self._stub = pbg.SimStub(self._channel)
         self._watch = None
         self._events = collections.deque(maxlen=10_000)
+        self.recording = Recording(self)
         try:
             reply = self._call(self._stub.Handshake, pb.HandshakeRequest(protocol_version=PROTOCOL_VERSION))
             if reply.protocol_version != PROTOCOL_VERSION:
@@ -335,6 +339,7 @@ class Sim:
         except Exception:
             self._channel.close()
             raise
+        self.logs_dir = reply.logs_dir  # where the server writes recordings and copies Blackbox logs
 
     @staticmethod
     def _call(method, request):
@@ -368,10 +373,14 @@ class Sim:
         threading.Thread(target=pump, name="ofs-watch", daemon=True).start()
 
     def load(self, quad_path: str, seed: int = 0, mode: str = "lockstep", open_loop_fc: bool = False,
-             overrun_policy: str = "warn", keep_alive: bool = False, world: str | None = None) -> str:
+             overrun_policy: str = "warn", keep_alive: bool = False, world: str | None = None,
+             record: bool | None = None) -> str:
         """Loads a quad. `mode` is "lockstep" (advance with `run`) or "realtime" (loads paused; `start` paces it
-        to the wall clock). `world` is a world file (e.g. "worlds/flat.toml"); without one the quad flies in the
-        open field. Returns the quad's name; `configurator_address` is set when it runs Betaflight."""
+        to the wall clock). `world` is a world file (e.g. "worlds/flat.toml") or a world name ("flat":
+        worlds/flat.toml beside the quad's directory); without one the quad flies in the open field. `record`
+        starts a recording of every signal at the default rates (`sim.log` reads it); it defaults to on for
+        lockstep sessions and off for real-time ones. Returns the quad's name; `configurator_address` is set when
+        it runs Betaflight."""
         if mode not in _MODES:
             raise ValueError(f"mode must be one of {sorted(_MODES)}")
         if overrun_policy not in _POLICIES:
@@ -381,6 +390,9 @@ class Sim:
             quad_path=str(quad_path), seed=seed, mode=_MODES[mode], open_loop_fc=open_loop_fc,
             overrun_policy=_POLICIES[overrun_policy], keep_alive=keep_alive, world_path=str(world or "")))
         self.configurator_address = reply.configurator_address
+        self.recording._session_ended()
+        if (mode == "lockstep") if record is None else record:
+            self.recording.start()
         return reply.quad_name
 
     def set_sticks(self, roll=0.0, pitch=0.0, yaw=0.0, throttle=0.0, aux=(-1.0, -1.0, -1.0, -1.0)) -> None:
@@ -444,8 +456,36 @@ class Sim:
         self._call(self._stub.InjectFault, fault._to_pb())
 
     def clear_faults(self) -> None:
+        """Clears every active fault, and the ones scheduled with `at` that have not activated yet."""
         self._call(self._stub.ClearFaults, pb.Empty())
 
+    def play_inputs(self, path) -> None:
+        """Plays a stick-inputs CSV from now (spec §5): a `t_s` column (seconds after this call, increasing) and any
+        of `roll`, `pitch`, `yaw`, `throttle`, `aux1`..`aux4`. Each row holds until the next; columns the file
+        leaves out keep their value; the last row holds after it ends. `set_sticks` is refused while it plays. A
+        malformed file raises `ConfigError` naming every problem with its line."""
+        p = pathlib.Path(path)
+        self._call(self._stub.PlayInputs, pb.InputsFile(name=p.name, csv=p.read_text(encoding="utf-8")))
+
+    def stop_inputs(self) -> None:
+        """Stops the playing inputs file, if any (the sticks keep their last values)."""
+        self._call(self._stub.StopInputs, pb.Empty())
+
+    @property
+    def log(self) -> Log:
+        """The session's recording, as a `reader.Log` (`sim.log.to_dataframe(["imu.*"])`). A running recording is
+        stopped first: an MCAP file is readable once it is finished."""
+        if self.recording.active:
+            return self.recording.stop()
+        if self.recording.last is None:
+            raise InvalidState("no recording: a lockstep load records by default, or call sim.recording.start()")
+        return self.recording.last
+
+    def blackbox_files(self) -> list:
+        """Betaflight's own Blackbox logs the server copied into its logs directory, sorted. A session's logs
+        arrive when it ends (`close()`, or the next `load`); disarm before then for a complete log."""
+        return sorted(str(p) for p in pathlib.Path(self.logs_dir).glob("*.BFL"))
+
     def events(self) -> list:
         """Events received since the last call (radio link up/down, firmware restarts, overruns, ...).
 
````

**Modify `python/ofs/__init__.py`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/python/ofs/__init__.py b/python/ofs/__init__.py
index 851d9ea..f0b528c 100644
--- a/python/ofs/__init__.py
+++ b/python/ofs/__init__.py
@@ -1,5 +1,5 @@
 """Python client for Open FPV Sim."""
-from . import faults
+from . import faults, reader
 from .client import (Emitter, Event, Handset, Osd, RadioLink, ReceiverAntenna, Sim, State, VideoLink, Vtx, World,
                      WorldObject, connect, launch)
 from .errors import (ConfigError, FirmwareCrashed, InternalError, InvalidArgument, InvalidState, NotLoaded,
@@ -8,7 +8,7 @@ from .errors import (ConfigError, FirmwareCrashed, InternalError, InvalidArgumen
 __all__ = [
     "Sim", "State", "RadioLink", "Event", "Handset", "Osd", "Vtx", "VideoLink", "World", "WorldObject",
     "ReceiverAntenna", "Emitter",
-    "connect", "launch", "faults",
+    "connect", "launch", "faults", "reader",
     "OfsError", "ConfigError", "FirmwareCrashed", "NumericalError", "ProtocolMismatch", "NotLoaded",
     "InvalidArgument", "InvalidState", "ServerUnavailable", "PilotBusy", "InternalError",
 ]
````

- [ ] **Step 4: The distribution**

The `readme` the pyproject names is the PyPI page.

**Modify `python/pyproject.toml`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/python/pyproject.toml b/python/pyproject.toml
index 7ed1566..7a5c846 100644
--- a/python/pyproject.toml
+++ b/python/pyproject.toml
@@ -1,17 +1,30 @@
 [build-system]
-requires = ["setuptools>=68"]
+requires = ["setuptools>=77"]
 build-backend = "setuptools.build_meta"
 
 [project]
-name = "ofs"
+name = "open-fpv-sim"
 version = "0.1.0"
-description = "Python client for Open FPV Sim"
-license = { text = "GPL-3.0-or-later" }
+description = "Python client for Open FPV Sim: an FPV drone simulator that runs real Betaflight"
+readme = "README.md"
+license = "GPL-3.0-or-later"
 requires-python = ">=3.10"
 dependencies = ["grpcio>=1.84", "protobuf>=7.35.1"]
+classifiers = [
+    "Programming Language :: Python :: 3",
+    "Topic :: Scientific/Engineering",
+]
+
+[project.urls]
+Homepage = "https://github.com/hugowis/open-fpv-sim"
 
 [project.optional-dependencies]
-dev = ["grpcio-tools>=1.84", "pytest>=8"]
+# Reading recordings: mcap for every reader function, pandas for DataFrames (Parquet also needs pyarrow).
+log = ["mcap>=1.5", "pandas>=2"]
+dev = ["grpcio-tools>=1.84", "pytest>=8", "mcap>=1.5", "pandas>=2", "pyarrow>=15", "build>=1"]
 
 [tool.setuptools.packages.find]
 include = ["ofs*"]
+
+[tool.setuptools.package-data]
+"ofs.v1" = ["*.pyi"]
````

**Create `python/README.md`:**

````markdown
# open-fpv-sim

The Python client of [Open FPV Sim](https://github.com/hugowis/open-fpv-sim), an open-source FPV drone simulator
that runs **real Betaflight** (SITL) against physics, electrical, sensor, radio and video models.

    pip install open-fpv-sim[log]

The package is the client (`import ofs`). It drives an `ofs-sim` server, which you build from the repository
(`cargo build -p ofs-sim`) and point at with `OFS_SIM_BIN`; flying real Betaflight also needs the SITL build
described in the repository's `docs/dev-setup.md`.

    import ofs

    sim = ofs.launch(headless=True)
    sim.load("quads/opendrone-5f-freestyle.toml", world="flat", seed=42, mode="lockstep")
    sim.inject(ofs.faults.MotorFailure(motor=2, at=3.0))
    sim.play_inputs("flips.csv")
    sim.run(seconds=10)
    df = sim.log.to_dataframe(["imu.gyro_frd_radps", "motor.*.omega_radps", "battery.voltage_v"])

The `log` extra reads the session recordings (`mcap`, `pandas`; Parquet export also needs `pyarrow`).
````

Re-install (see the task intro), then:
Run: `python -m pytest python/tests -q`
Expected: PASS: 44 tests on Windows without SITL, the SITL ones skipped.

Check the package builds cleanly:
```bash
python -m build python --outdir dist 2>&1 | grep -c '!!'    # expected: 0 (no setuptools deprecation warnings)
python -c "import zipfile,glob; n=zipfile.ZipFile(glob.glob('dist/*.whl')[0]).namelist(); assert 'ofs/v1/sim_pb2.pyi' in n, n"
rm -rf dist python/build python/open_fpv_sim.egg-info
```

- [ ] **Step 5: Commit**

```bash
git add python
git commit -m "feat(python): recordings, the reader, scripted inputs and faults at= - the open-fpv-sim package"
```

---

### Task 6: Python: the Rerun live view

`ofs.rerun` is thin on purpose (spec §9: Rerun's SDK churns): three logging functions that tests can call directly, and `view`, which wires them to the state stream.

**Files:**
- Create: `python/ofs/rerun.py`
- Create: `python/tests/test_rerun.py`
- Modify: `python/README.md`
- Modify: `python/ofs/__init__.py`
- Modify: `python/pyproject.toml`

**Interfaces:**
- Consumes: `Sim.get_world()`, `Sim.stream_states(rate_hz)`, `Sim.events()`, `Sim._ensure_watch()`, `ofs.State`, `ofs.Event`, `ofs.VideoLink`.
- Produces: `ofs.rerun.{log_world(rec, world), log_state(rec, state), log_event(rec, event), view(sim, rate_hz=30, spawn=True, recording=None, until_s=None) -> recording stream}`.

- [ ] **Step 1: The tests**

**Create `python/tests/test_rerun.py`:**

````python
"""The Rerun live view (spec §4): what it logs, against a stand-in recording stream, then against Rerun itself."""
import math

import pytest

import ofs
from conftest import QUAD

rr = pytest.importorskip("rerun")


class FakeStream:
    """Records what the view logs: (entity path, archetype class name, static) per call, and the times set."""

    def __init__(self):
        self.logged = []
        self.times = []

    def set_time(self, timeline, duration):
        self.times.append((timeline, duration))

    def log(self, path, archetype, static=False):
        self.logged.append((path, type(archetype).__name__, static))


def _state(**kw):
    base = dict(time_s=1.5, position_ned_m=(1.0, 2.0, -3.0), velocity_ned_mps=(0.0, 0.0, 0.0),
                attitude_wxyz=(1.0, 0.0, 0.0, 0.0), rate_frd_radps=(0.0, 0.0, 0.0), battery_voltage_v=24.0,
                battery_current_a=10.0, motor_rpm=(1000.0, 1001.0, 1002.0, 1003.0), motor_cmd=(0.5,) * 4)
    base.update(kw)
    return ofs.State(**base)


def test_a_state_logs_the_pose_and_the_curves_at_its_time():
    rec = FakeStream()
    ofs.rerun.log_state(rec, _state())
    assert rec.times == [("sim", 1.5)]
    assert [(p, k) for p, k, _ in rec.logged] == [
        ("world/quad", "Transform3D"), ("battery/voltage_v", "Scalars"), ("battery/current_a", "Scalars"),
        ("motors/rpm", "Scalars"), ("radio/lq_pct", "Scalars"), ("radio/rssi_dbm", "Scalars")]
    rec = FakeStream()
    ofs.rerun.log_state(rec, _state(video=ofs.VideoLink(present=True, snr_db=20.0)))
    assert ("video/snr_db", "Scalars", False) in rec.logged


def test_events_are_text_lines_and_a_timeless_one_keeps_the_clock():
    rec = FakeStream()
    ofs.rerun.log_event(rec, ofs.Event(time_s=2.0, kind="fault_injected", message="motor_failure"))
    ofs.rerun.log_event(rec, ofs.Event(time_s=math.nan, kind="watch_ended", message="gone"))
    assert rec.times == [("sim", 2.0)]
    assert [(p, k) for p, k, _ in rec.logged] == [("events", "TextLog"), ("events", "TextLog")]


def test_the_world_is_logged_once_as_static_entities(sim):
    sim.load(QUAD, open_loop_fc=True, world="flat")
    rec = FakeStream()
    ofs.rerun.log_world(rec, sim.get_world())
    assert all(static for _, _, static in rec.logged)
    assert [p for p, _, _ in rec.logged] == ["world", "world/objects", "world/poles", "world/pilot", "world/quad/frame"]


def test_view_streams_a_real_time_session_into_a_rerun_file(sim, tmp_path):
    sim.load(QUAD, open_loop_fc=True, mode="realtime")
    sim.start()
    rec = rr.RecordingStream("open-fpv-sim-test")
    path = tmp_path / "view.rrd"
    rec.save(str(path))
    returned = ofs.rerun.view(sim, rate_hz=60, spawn=False, recording=rec, until_s=0.3)
    assert returned is rec
    rec.disconnect()
    assert path.stat().st_size > 1000
````

Run: `python -m pip install "rerun-sdk>=0.39,<0.40" && python -m pytest python/tests/test_rerun.py -q`
Expected: FAIL (`ofs.rerun` has no attribute `log_state`).

- [ ] **Step 2: The module, its export, the extra**

**Create `python/ofs/rerun.py`:**

````python
"""A live view of a session in the Rerun viewer (spec §4): the quad's pose in the world, battery, motor, link and
video curves, and the session's events as text.

Needs the `rerun` extra (`pip install open-fpv-sim[rerun]`). It follows the state stream (at most 240 Hz), not the
full bus: for the 8 kHz signals, read the session's MCAP recording.
"""
from __future__ import annotations

_WARN_KINDS = {"link_down", "video_lost", "collision", "fault_injected", "overrun", "serial_overflow"}
_ERROR_KINDS = {"sim_error", "session_ended", "watch_ended"}


def _rr():
    try:
        import rerun as rr
    except ImportError as e:
        raise ImportError("the live view needs the rerun-sdk package; pip install open-fpv-sim[rerun]") from e
    return rr


def log_world(rec, world) -> None:
    """The world as static entities: NED axes (north forward, east right, down), its objects as boxes, the pilot."""
    rr = _rr()
    rec.log("world", rr.ViewCoordinates.FRD, static=True)
    boxes = [o for o in world.objects if o.shape == "box"]
    if boxes:
        # A box's center is mid-height; size is (north, east, height).
        rec.log("world/objects", rr.Boxes3D(centers=[o.center_ned_m for o in boxes],
                                            half_sizes=[[s / 2 for s in o.size_m] for o in boxes],
                                            colors=[o.color for o in boxes], labels=[o.name for o in boxes]), static=True)
    cylinders = [o for o in world.objects if o.shape == "cylinder"]
    if cylinders:
        rec.log("world/poles", rr.Boxes3D(centers=[o.center_ned_m for o in cylinders],
                                          half_sizes=[[o.radius_m, o.radius_m, o.height_m / 2] for o in cylinders],
                                          colors=[o.color for o in cylinders], labels=[o.name for o in cylinders]),
                static=True)
    rec.log("world/pilot", rr.Points3D([world.pilot_position_ned_m], radii=0.3, labels=["pilot"]), static=True)
    rec.log("world/quad/frame", rr.Boxes3D(half_sizes=[[0.12, 0.12, 0.03]]), static=True)


def log_state(rec, state) -> None:
    """One state: the quad's pose (`world/quad`) and its curves, at the state's simulated time."""
    rr = _rr()
    rec.set_time("sim", duration=state.time_s)
    w, x, y, z = state.attitude_wxyz
    rec.log("world/quad", rr.Transform3D(translation=list(state.position_ned_m), quaternion=rr.Quaternion(xyzw=[x, y, z, w])))
    rec.log("battery/voltage_v", rr.Scalars(state.battery_voltage_v))
    rec.log("battery/current_a", rr.Scalars(state.battery_current_a))
    if state.motor_rpm:
        rec.log("motors/rpm", rr.Scalars(list(state.motor_rpm)))
    rec.log("radio/lq_pct", rr.Scalars(state.radio.lq_pct))
    rec.log("radio/rssi_dbm", rr.Scalars(state.radio.rssi_dbm))
    if state.video.present:
        rec.log("video/snr_db", rr.Scalars(state.video.snr_db))


def log_event(rec, event) -> None:
    """One event as a line of text, at its simulated time (events without one, like `watch_ended`, keep the last)."""
    rr = _rr()
    if event.time_s == event.time_s:  # not NaN
        rec.set_time("sim", duration=event.time_s)
    level = rr.TextLogLevel.ERROR if event.kind in _ERROR_KINDS else (
        rr.TextLogLevel.WARN if event.kind in _WARN_KINDS else rr.TextLogLevel.INFO)
    rec.log("events", rr.TextLog(f"{event.kind}: {event.message}", level=level))


def view(sim, rate_hz: int = 30, spawn: bool = True, recording=None, until_s: float | None = None):
    """Streams `sim`'s session into Rerun until the session ends (or its simulated time reaches `until_s`).

    `spawn` starts a viewer; pass `recording` (a `rerun.RecordingStream`, e.g. one that saves to a file) to send
    elsewhere. It takes this `Sim`'s events (`sim.events()`): to keep yours, view through a second connection,
    `ofs.rerun.view(ofs.connect(sim.address))`. Returns the recording stream.
    """
    rr = _rr()
    rec = recording if recording is not None else rr.RecordingStream("open-fpv-sim")
    if spawn:
        rec.spawn()
    log_world(rec, sim.get_world())
    sim._ensure_watch()
    for state in sim.stream_states(rate_hz):
        log_state(rec, state)
        for event in sim.events():
            log_event(rec, event)
        if until_s is not None and state.time_s >= until_s:
            break
    rec.flush()
    return rec
````

**Modify `python/ofs/__init__.py`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/python/ofs/__init__.py b/python/ofs/__init__.py
index f0b528c..1d0d168 100644
--- a/python/ofs/__init__.py
+++ b/python/ofs/__init__.py
@@ -1,5 +1,5 @@
 """Python client for Open FPV Sim."""
-from . import faults, reader
+from . import faults, reader, rerun
 from .client import (Emitter, Event, Handset, Osd, RadioLink, ReceiverAntenna, Sim, State, VideoLink, Vtx, World,
                      WorldObject, connect, launch)
 from .errors import (ConfigError, FirmwareCrashed, InternalError, InvalidArgument, InvalidState, NotLoaded,
@@ -8,7 +8,7 @@ from .errors import (ConfigError, FirmwareCrashed, InternalError, InvalidArgumen
 __all__ = [
     "Sim", "State", "RadioLink", "Event", "Handset", "Osd", "Vtx", "VideoLink", "World", "WorldObject",
     "ReceiverAntenna", "Emitter",
-    "connect", "launch", "faults", "reader",
+    "connect", "launch", "faults", "reader", "rerun",
     "OfsError", "ConfigError", "FirmwareCrashed", "NumericalError", "ProtocolMismatch", "NotLoaded",
     "InvalidArgument", "InvalidState", "ServerUnavailable", "PilotBusy", "InternalError",
 ]
````

**Modify `python/pyproject.toml`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/python/pyproject.toml b/python/pyproject.toml
index 7a5c846..e7e0b6d 100644
--- a/python/pyproject.toml
+++ b/python/pyproject.toml
@@ -21,7 +21,9 @@ Homepage = "https://github.com/hugowis/open-fpv-sim"
 [project.optional-dependencies]
 # Reading recordings: mcap for every reader function, pandas for DataFrames (Parquet also needs pyarrow).
 log = ["mcap>=1.5", "pandas>=2"]
-dev = ["grpcio-tools>=1.84", "pytest>=8", "mcap>=1.5", "pandas>=2", "pyarrow>=15", "build>=1"]
+# The live view; Rerun's SDK changes between minor releases, so the extra pins one.
+rerun = ["rerun-sdk>=0.39,<0.40"]
+dev = ["grpcio-tools>=1.84", "pytest>=8", "mcap>=1.5", "pandas>=2", "pyarrow>=15", "rerun-sdk>=0.39,<0.40", "build>=1"]
 
 [tool.setuptools.packages.find]
 include = ["ofs*"]
````

**Modify `python/README.md`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/python/README.md b/python/README.md
index fde0226..326ea58 100644
--- a/python/README.md
+++ b/python/README.md
@@ -3,7 +3,7 @@
 The Python client of [Open FPV Sim](https://github.com/hugowis/open-fpv-sim), an open-source FPV drone simulator
 that runs **real Betaflight** (SITL) against physics, electrical, sensor, radio and video models.
 
-    pip install open-fpv-sim[log]
+    pip install open-fpv-sim[log,rerun]
 
 The package is the client (`import ofs`). It drives an `ofs-sim` server, which you build from the repository
 (`cargo build -p ofs-sim`) and point at with `OFS_SIM_BIN`; flying real Betaflight also needs the SITL build
@@ -18,4 +18,5 @@ described in the repository's `docs/dev-setup.md`.
     sim.run(seconds=10)
     df = sim.log.to_dataframe(["imu.gyro_frd_radps", "motor.*.omega_radps", "battery.voltage_v"])
 
-The `log` extra reads the session recordings (`mcap`, `pandas`; Parquet export also needs `pyarrow`).
+Extras: `log` reads the session recordings (`mcap`, `pandas`; Parquet export also needs `pyarrow`), `rerun` adds the
+live view (`ofs.rerun.view(sim)`).
````

Run: `python -m pip install -e "python[dev]" && python -m pytest python/tests -q`
Expected: PASS, the 4 Rerun tests included.

Manual check (needs a display): `python -c "import ofs; s = ofs.launch(); s.load('quads/opendrone-5f-freestyle.toml', open_loop_fc=True, mode='realtime'); s.set_sticks(throttle=0.6); s.start(); ofs.rerun.view(s, until_s=10)"`. The viewer opens with the quad climbing, and the battery and rpm curves move.

- [ ] **Step 3: Commit**

```bash
git add python
git commit -m "feat(python): the Rerun live view"
```

---

### Task 7: The sandbox example live, Blackbox on, the wheel in CI, publishing on tags

The parent §6.2 script runs as printed (with the bus's own signal names) against real Betaflight, with three effects:
- The shipped quads log to SITL's virtual Blackbox.
- The SITL determinism test also compares the two flights' recordings byte for byte.
- CI installs the built wheel alone into a clean venv and runs the example. A `publish` job uploads to PyPI on `v*` tags only.

After this task, delete the `eeprom.bin` of each quad in your own `.ofs-data/` once: the diffs changed (Global Constraints).

**Files:**
- Create: `python/examples/flips.csv`
- Create: `python/examples/sandbox.py`
- Create: `python/tests/test_sitl_sandbox.py`
- Modify: `.github/workflows/ci.yml`
- Modify: `.gitignore`
- Modify: `python/ofs/client.py`
- Modify: `python/tests/test_sitl_hover.py`
- Modify: `quads/opendrone-5f-freestyle.betaflight.diff`
- Modify: `quads/opendrone-5f-tramp.betaflight.diff`

**Interfaces:**
- Consumes from Tasks 4 and 5: everything the example calls.
- Produces: `python/examples/sandbox.py` and `flips.csv`, and `launch()`'s `atexit` close.

- [ ] **Step 1: The live tests**

`test_sitl_sandbox.py` copies `quads/`, `library/`, `worlds/`, the example and its CSV into a temp directory, then executes the example's text unchanged.

**Create `python/tests/test_sitl_sandbox.py`:**

````python
"""Spec §6.2's sandbox example through real Betaflight (M4c): python/examples/sandbox.py as the README prints it,
run from a directory laid out like the repository, then its recording and Betaflight's own Blackbox log.
Needs OFS_SITL_LAUNCH."""
import os
import shutil

import pytest

from conftest import REPO

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")


@pytest.fixture
def sandbox(tmp_path, sim_bin, monkeypatch):
    """A working directory with the repository's quads, library and worlds, the example and its flips.csv."""
    for d in ("quads", "library", "worlds"):
        shutil.copytree(REPO / d, tmp_path / d)
    for f in ("sandbox.py", "flips.csv"):
        shutil.copy(REPO / "python" / "examples" / f, tmp_path / f)
    monkeypatch.chdir(tmp_path)
    monkeypatch.setenv("OFS_SIM_BIN", sim_bin)
    monkeypatch.delenv("OFS_LIBRARY", raising=False)  # the copied library/ beside quads/ must resolve on its own
    return tmp_path


def test_the_sandbox_example_runs_as_printed(sandbox):
    pytest.importorskip("mcap")
    pytest.importorskip("pandas")
    script = (sandbox / "sandbox.py").read_text()
    namespace = {}
    exec(compile(script, "sandbox.py", "exec"), namespace)
    sim, df = namespace["sim"], namespace["df"]
    try:
        assert list(df.columns) == ["imu.gyro_frd_radps.x", "imu.gyro_frd_radps.y", "imu.gyro_frd_radps.z",
                                    "motor.0.omega_radps", "motor.1.omega_radps", "motor.2.omega_radps",
                                    "motor.3.omega_radps", "battery.voltage_v"]
        assert df.index[-1] == pytest.approx(10.0)
        motor2 = df["motor.2.omega_radps"].dropna()
        flying = motor2.loc[2.8:2.95].mean()
        assert flying > 500, f"armed at 1 s and climbing: motor 3 at {flying:.0f} rad/s"
        assert motor2.loc[4.0:4.2].mean() < 0.5 * flying, "MotorFailure(motor=2, at=3.0) stopped it"
        assert df["imu.gyro_frd_radps.x"].loc[2.0:2.5].abs().max() > 3.0, "the flip shows on the gyro"
    finally:
        sim.close()
    logs = sim.blackbox_files()
    assert len(logs) == 1, logs
    with open(logs[0], "rb") as f:
        assert f.read(64).startswith(b"H Product:Blackbox flight data recorder"), "Betaflight's own log, readable"
````

**Modify `python/tests/test_sitl_hover.py`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/python/tests/test_sitl_hover.py b/python/tests/test_sitl_hover.py
index f61a3cf..9f90f94 100644
--- a/python/tests/test_sitl_hover.py
+++ b/python/tests/test_sitl_hover.py
@@ -1,6 +1,8 @@
 """M1 exit criteria. Needs Betaflight SITL: set OFS_SITL_LAUNCH (cleanup defaults automatically on Linux and under WSL)."""
+import hashlib
 import importlib.util
 import os
+import pathlib
 
 import pytest
 
@@ -26,7 +28,8 @@ def test_betaflight_sitl_hover(sim, tmp_path):
 
 
 def test_betaflight_sitl_hover_is_deterministic(sim):
-    """Spec §5.5/§9: same quad + seed + inputs in lockstep give identical runs, Betaflight SITL included.
+    """Spec §5.5/§9: same quad + seed + inputs in lockstep give identical runs, Betaflight SITL included, down to
+    the bytes of their MCAP recordings (§8.4; each lockstep load records).
 
     Every load() relaunches SITL from <data_dir>/<quad stem>-<hash>/eeprom.bin. The first-ever load also applies
     betaflight.diff, so a warm-up flight runs first and both compared flights boot from the same EEPROM.
@@ -38,9 +41,11 @@ def test_betaflight_sitl_hover_is_deterministic(sim):
         sim.load(QUAD, seed=1)
         flights.append(hover.fly_hover(sim))
         flights[-1]["final"] = sim.state()
+        flights[-1]["recording"] = hashlib.sha256(pathlib.Path(sim.log.path).read_bytes()).hexdigest()
     a, b = flights[1], flights[2]
     assert a["armed"], "motors never spun"
     assert len(a["states"]) == len(b["states"])
     diverged = next((i for i, (x, y) in enumerate(zip(a["states"], b["states"])) if x != y), None)
     assert diverged is None, f"runs diverge at state {diverged}:\n{a['states'][diverged]}\n{b['states'][diverged]}"
     assert a["final"] == b["final"]
+    assert a["recording"] == b["recording"], "the two flights' recordings differ"
````

- [ ] **Step 2: The example and its flight**

`flips.csv` arms in acro at 1 s (Ruling 13), climbs, flips right, holds, and disarms at 7 s, so the Blackbox log closes.

**Create `python/examples/sandbox.py`:**

````python
import ofs
sim = ofs.launch(headless=True)
sim.load("quads/opendrone-5f-freestyle.toml", world="flat", seed=42, mode="lockstep")
sim.inject(ofs.faults.MotorFailure(motor=2, at=3.0))
sim.play_inputs("flips.csv")
sim.run(seconds=10)
df = sim.log.to_dataframe(["imu.gyro_frd_radps", "motor.*.omega_radps", "battery.voltage_v"])
````

**Create `python/examples/flips.csv`:**

````csv
# The spec §6.2 sandbox flight (python/examples/sandbox.py): arm in acro, climb, flip right, lose motor 3 (index 2)
# at 3 s by the script's MotorFailure, disarm. AUX1 = arm, AUX2 = angle mode (off here); see the quad's betaflight.diff.
t_s,throttle,roll,aux1,aux2
0.0,0.0,0.0,-1,-1
1.0,0.0,0.0,1,-1
1.3,0.6,0.0,1,-1
2.0,0.45,1.0,1,-1
2.3,0.5,0.0,1,-1
7.0,0.0,0.0,-1,-1
````

- [ ] **Step 3: Blackbox on in the shipped diffs; `launch()` closes at exit**

**Modify `quads/opendrone-5f-freestyle.betaflight.diff`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/quads/opendrone-5f-freestyle.betaflight.diff b/quads/opendrone-5f-freestyle.betaflight.diff
index 582933a..45a5efb 100644
--- a/quads/opendrone-5f-freestyle.betaflight.diff
+++ b/quads/opendrone-5f-freestyle.betaflight.diff
@@ -41,3 +41,6 @@ serial 4 2048 115200 57600 0 115200
 set vtx_band = 5
 set vtx_channel = 1
 set vtx_power = 2
+# M4c Blackbox: SITL's VIRTUAL device writes LOGnnnnn.BFL into the quad's firmware directory each time it is armed;
+# the server copies a session's logs into <data_dir>/logs when the session ends.
+set blackbox_device = VIRTUAL
````

**Modify `quads/opendrone-5f-tramp.betaflight.diff`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/quads/opendrone-5f-tramp.betaflight.diff b/quads/opendrone-5f-tramp.betaflight.diff
index b664393..5053088 100644
--- a/quads/opendrone-5f-tramp.betaflight.diff
+++ b/quads/opendrone-5f-tramp.betaflight.diff
@@ -42,3 +42,6 @@ serial 5 8192 115200 57600 0 115200
 set vtx_band = 5
 set vtx_channel = 1
 set vtx_power = 2
+# M4c Blackbox: SITL's VIRTUAL device writes LOGnnnnn.BFL into the quad's firmware directory each time it is armed;
+# the server copies a session's logs into <data_dir>/logs when the session ends.
+set blackbox_device = VIRTUAL
````

**Modify `python/ofs/client.py`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/python/ofs/client.py b/python/ofs/client.py
index bdff324..47d1cd1 100644
--- a/python/ofs/client.py
+++ b/python/ofs/client.py
@@ -1,6 +1,7 @@
 """Client for the ofs-sim gRPC server."""
 from __future__ import annotations
 
+import atexit
 import collections
 import collections.abc
 import math
@@ -11,6 +12,7 @@ import socket
 import subprocess
 import threading
 import time
+import weakref
 from dataclasses import dataclass, field
 
 import grpc
@@ -535,6 +537,12 @@ def connect(address: str = "127.0.0.1:50051") -> Sim:
     return Sim(address)
 
 
+def _close_at_exit(ref) -> None:
+    sim = ref()
+    if sim is not None:
+        sim.close()
+
+
 def _free_port() -> int:
     with socket.socket() as s:
         s.bind(("127.0.0.1", 0))
@@ -543,7 +551,9 @@ def _free_port() -> int:
 
 def launch(binary: str | None = None, headless: bool = True, data_dir: str = ".ofs-data",
            timeout_s: float = 15.0) -> Sim:
-    """Starts an ofs-sim server on a free local port and connects to it."""
+    """Starts an ofs-sim server on a free local port and connects to it. The server stops with `close()`, or when
+    the script exits (a script need not close it: its session's recording and Blackbox logs are finished either
+    way)."""
     if not headless:
         raise NotImplementedError("the Godot pilot client arrives with M2b; only headless=True is available")
     binary = binary or os.environ.get("OFS_SIM_BIN") or shutil.which("ofs-sim")
@@ -567,8 +577,10 @@ def launch(binary: str | None = None, headless: bool = True, data_dir: str = ".o
     finally:
         channel.close()
     try:
-        return Sim(address, proc)
+        sim = Sim(address, proc)
     except BaseException:  # e.g. ProtocolMismatch: don't leave the server running
         proc.kill()
         proc.wait()
         raise
+    atexit.register(_close_at_exit, weakref.ref(sim))
+    return sim
````

Run the live suites inside WSL (Linux-built server; see Global Constraints):
```bash
cd /mnt/c/dev/open-fpv-sim
export CARGO_TARGET_DIR=$HOME/ofs/m4c-target OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf
cargo build -p ofs-sim
cargo test --workspace --locked -- --include-ignored --test-threads=1
cd python && OFS_SIM_BIN=$CARGO_TARGET_DIR/debug/ofs-sim PYTHONPATH=. uv run --no-project --with grpcio,protobuf,pytest,mcap,pandas,pyarrow,'rerun-sdk>=0.39,<0.40' python -m pytest tests -v
```
Expected: every Rust test passes, the live ones included. The Python suite passes against live SITL: `test_the_sandbox_example_runs_as_printed` and the hover determinism test (now with equal recording hashes) included. In the prototype: 527 Rust, 72 Python.

- [ ] **Step 4: CI: the wheel in a clean venv, the publish job**

The clean-venv step runs from the repository root, so `import ofs` finds the installed wheel, not `python/`. The step asserts this.

**Modify `.github/workflows/ci.yml`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/.github/workflows/ci.yml b/.github/workflows/ci.yml
index 288eb0b..dfec30d 100644
--- a/.github/workflows/ci.yml
+++ b/.github/workflows/ci.yml
@@ -96,7 +96,15 @@ jobs:
       - run: cargo test -p ofs-video --test vtx_live --locked -- --ignored --test-threads=1
       - run: cargo build -p ofs-sim --locked
       - run: python -m pip install -e "python[dev]"
-      - run: python -m pytest python/tests/test_sitl_hover.py python/tests/test_sitl_radio.py python/tests/test_sitl_telemetry.py python/tests/test_sitl_tramp.py python/tests/test_sitl_video.py python/tests/test_sitl_faults.py -v
+      - run: python -m pytest python/tests/test_sitl_hover.py python/tests/test_sitl_radio.py python/tests/test_sitl_telemetry.py python/tests/test_sitl_tramp.py python/tests/test_sitl_video.py python/tests/test_sitl_faults.py python/tests/test_sitl_sandbox.py -v
+      # The package as PyPI would ship it (spec §8): the built wheel alone in a clean venv runs the §6.2 example
+      # against real Betaflight. Run from the repository root, `import ofs` finds the installed wheel, not python/.
+      - run: python -m build python --outdir dist
+      - run: |
+          python -m venv "$RUNNER_TEMP/wheel-venv"
+          "$RUNNER_TEMP/wheel-venv/bin/pip" install "$(ls dist/*.whl)[log]" pytest
+          "$RUNNER_TEMP/wheel-venv/bin/python" -c "import ofs; assert 'site-packages' in ofs.__file__, ofs.__file__"
+          OFS_SIM_BIN=target/debug/ofs-sim "$RUNNER_TEMP/wheel-venv/bin/python" -m pytest python/tests/test_sitl_sandbox.py -v
       - uses: actions/cache@v4
         with:
           path: build/godot-dl
@@ -104,3 +112,27 @@ jobs:
       # The game against real Betaflight (OFS_SITL_LAUNCH is set above): arm, climb, failsafe on a radio cut, reload.
       # The runner builds ofs-sim and ofs-godot and downloads the pinned Godot itself.
       - run: bash scripts/run-godot-tests.sh e2e
+
+  # PyPI, on version tags only (v*): nothing publishes from merges or branch pushes. Trusted publishing: the PyPI
+  # project trusts this workflow's `pypi` environment, so no token is stored. The first tag that publishes is the
+  # maintainer's call.
+  publish:
+    if: startsWith(github.ref, 'refs/tags/v')
+    needs: [core, windows, sitl]
+    runs-on: ubuntu-24.04
+    environment: pypi
+    permissions:
+      contents: read
+      id-token: write
+    steps:
+      - uses: actions/checkout@v5
+      - uses: actions/setup-python@v6
+        with:
+          python-version: "3.12"
+      - name: The tag names the package version
+        run: |
+          version=$(python -c "import tomllib; print(tomllib.load(open('python/pyproject.toml', 'rb'))['project']['version'])")
+          test "$GITHUB_REF_NAME" = "v$version" || { echo "tag $GITHUB_REF_NAME does not match the package version $version"; exit 1; }
+      - run: python -m pip install build
+      - run: python -m build python --outdir dist
+      - uses: pypa/gh-action-pypi-publish@release/v1
````

**Modify `.gitignore`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/.gitignore b/.gitignore
index 5c22047..f2c75a6 100644
--- a/.gitignore
+++ b/.gitignore
@@ -8,3 +8,5 @@ __pycache__/
 /godot/.godot/
 /build/
 /.worktrees/
+/python/build/
+/dist/
````

Check the YAML parses, and replay the wheel step locally in WSL:
```bash
python -c "import yaml; d = yaml.safe_load(open('.github/workflows/ci.yml')); assert 'publish' in d['jobs'] and d['jobs']['publish']['if'] == \"startsWith(github.ref, 'refs/tags/v')\""
# in WSL, from the repository root, with OFS_SITL_LAUNCH and CARGO_TARGET_DIR as above:
T=$(mktemp -d)
uv run --no-project --with build python -m build python --outdir $T/dist
uv venv -q $T/venv && uv pip install -q --python $T/venv/bin/python "$(ls $T/dist/*.whl)[log]" pytest
$T/venv/bin/python -c "import ofs; assert 'site-packages' in ofs.__file__, ofs.__file__"
OFS_SIM_BIN=$CARGO_TARGET_DIR/debug/ofs-sim $T/venv/bin/python -m pytest -q python/tests/test_sitl_sandbox.py
rm -rf python/build python/open_fpv_sim.egg-info
```
Expected: `1 passed`.

- [ ] **Step 5: Commit**

```bash
git add .github .gitignore quads python
git commit -m "feat(sitl): the sandbox example live - Blackbox on, recordings byte-identical with Betaflight, the wheel in CI, tag-only publish"
```

---

### Task 8: Docs: the README, dev-setup, the specs' errata, the research note, the carried debt

The README's status and install section, and the sandbox example with what each part does. dev-setup's protocol 9 section, the WSL Python recipe and the EEPROM note. The parent spec's example names, the M4 spec's two errata (object records; the risk row resolved), the research note's size measurement, and `m4c-carried-debt.md`.

**Files:**
- Create: `docs/superpowers/m4c-carried-debt.md`
- Modify: `README.md`
- Modify: `docs/dev-setup.md`
- Modify: `docs/research/mcap-determinism.md`
- Modify: `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`
- Modify: `docs/superpowers/specs/2026-10-11-m4-sandbox-design.md`

**Interfaces:**
- Consumes: the behaviour of Tasks 1–7, as built.

- [ ] **Step 1: The README**

**Modify `README.md`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/README.md b/README.md
index d1332dd..7dd3464 100644
--- a/README.md
+++ b/README.md
@@ -3,7 +3,7 @@
 An open-source FPV drone simulator that runs **real Betaflight** (SITL) against physics, electrical, sensor and radio models,
 replicating real protocols so real tools work against it. Inspired by the [OpenDrone](https://opendrone.be/) open-hardware initiative.
 
-Status: **M3e — video extras.**
+Status: **M4 — the sandbox (v1).**
 - Betaflight flies through a simulated ExpressLRS/CRSF link and fails safe on link loss.
 - Sessions run in lockstep (deterministic, Betaflight included) or paced to the wall clock.
 - Betaflight Configurator should connect to the running simulator (the manual check with the desktop app is pending). A reboot sent to its port makes the simulator relaunch SITL from its EEPROM (verified live).
@@ -20,11 +20,24 @@ Status: **M3e — video extras.**
   (whole PAL fields; dropped fields in the marginal band hold the picture instead of smoothly degrading), the
   **D** key records exactly what the pilot sees as an MJPEG AVI, and a Tramp-configured quad answers
   Betaflight's VTX commands like the SmartAudio one (M3e, `docs/research/video-link.md`).
+- The sandbox: parts are data files with recorded provenance (M4a), scripts break the aircraft with the v1 fault
+  catalog (M4b), and sessions are recordable and replayable — the signal bus to MCAP (two identical lockstep runs
+  write byte-identical files, Betaflight included), pandas/CSV/Parquet export, a live Rerun view, scripted stick
+  inputs, faults at a simulated time, and Betaflight's own Blackbox logs (M4c, `docs/research/mcap-determinism.md`).
 
 - Design: `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`
 - SITL interface findings: `docs/research/sitl-interface.md`
 - Developer setup: `docs/dev-setup.md`
 
+## Install the Python package
+
+    pip install open-fpv-sim[log,rerun]
+
+The package is the client (`import ofs`); it drives the `ofs-sim` server you build here (`cargo build -p ofs-sim`,
+then `OFS_SIM_BIN` or `PATH`). The `log` extra reads recordings (`mcap`, `pandas`; Parquet export also needs
+`pyarrow`), the `rerun` extra adds the live view. From a checkout, `python -m pip install -e "python[dev]"` installs
+it editable with every extra and the test tools.
+
 ## Quick start (open loop, no firmware)
 
     cargo build -p ofs-sim
@@ -124,20 +137,42 @@ mechanisms, constants and their estimate status are in `docs/research/faults.md`
 | `GyroFault(bias_radps, noise_scale)` | the gyro reads an extra bias and a multiple of its normal noise |
 | `VtxInterference(freq_mhz, power_mw, position_ned_m)` | a transmitter next door breaks up the picture |
 
-The §6.2 shape of the spec's scripting example, against the shipped API:
+Every fault takes a keyword-only `at=`: the simulated time it activates at (on that tick, however the script cuts its
+`run` calls); without it, now. `clear_faults()` also drops faults still waiting for their time. The same faults fly
+from the game: the **K** key injects and clears `RadioLinkLoss`.
 
-    import ofs
+## Scripting, recording and the live view
+
+The spec's sandbox example (`python/examples/sandbox.py`): run it from the repository root with `OFS_SIM_BIN` and
+`OFS_SITL_LAUNCH` set and `python/examples/flips.csv` copied beside it (`cp python/examples/flips.csv .`):
 
-    sim = ofs.launch(binary="target/debug/ofs-sim")
-    sim.load("quads/opendrone-5f-freestyle.toml", open_loop_fc=True)
-    sim.set_sticks(throttle=0.6)
-    sim.run(1.0)
-    sim.inject(ofs.faults.MotorFailure(motor=2, scale=0.0))   # motor 3 dies in flight
-    sim.run(2.0)
-    print(sim.state().motor_rpm)                              # the dead one falls behind
-    sim.clear_faults()
-    sim.close()
-
-The same faults fly from the game: the **K** key injects and clears `RadioLinkLoss`.
+    import ofs
+    sim = ofs.launch(headless=True)
+    sim.load("quads/opendrone-5f-freestyle.toml", world="flat", seed=42, mode="lockstep")
+    sim.inject(ofs.faults.MotorFailure(motor=2, at=3.0))
+    sim.play_inputs("flips.csv")
+    sim.run(seconds=10)
+    df = sim.log.to_dataframe(["imu.gyro_frd_radps", "motor.*.omega_radps", "battery.voltage_v"])
+
+- **Recording.** A lockstep `load` records the whole signal bus at the default rates (body 240 Hz, IMU 1 kHz,
+  motors/ESCs/props 240 Hz, battery 10 Hz, everything else 50 Hz) into `<data_dir>/logs/<quad>-<n>.mcap`;
+  `load(record=False)` does not, and real-time sessions record only when asked. `sim.recording.start(["imu.*",
+  ("motor.*.omega_radps", 1000)])` picks signals and rates, `sim.recording.stop()` finishes the file. Recordings
+  open in Foxglove unchanged. Two lockstep runs with the same seed and inputs write byte-identical files.
+- **Reading.** `sim.log` stops the running recording and returns it; `.to_dataframe(patterns)` gives one row per
+  recorded time and one column per signal component (`imu.gyro_frd_radps.x`), indexed by `time_s`, NaN where a
+  slower signal has no record. `.to_csv(path)` and `.to_parquet(path)` export; `ofs.reader.to_dataframe(path)`
+  reads any recording. Patterns: `*` matches any run of characters, `?` one; a pattern matching nothing names
+  the signals it probably meant.
+- **Scripted sticks.** `sim.play_inputs("flips.csv")` plays a CSV with a `t_s` column (seconds from the call) and any
+  of `roll`, `pitch`, `yaw`, `throttle`, `aux1`..`aux4`, sample-and-hold on the simulator's ticks; `set_sticks`
+  is refused while it plays, and a malformed file raises `ConfigError` naming each problem's line.
+- **Betaflight's Blackbox.** The shipped quads log to SITL's virtual Blackbox; when a session ends its logs are
+  copied into the logs directory (`sim.blackbox_files()`), ready for Blackbox Explorer. Disarm before the session
+  ends for a complete log. (Enabling it changed the shipped diffs: an EEPROM made before M4c is refused at load
+  until you delete it, see `docs/dev-setup.md`.)
+- **Live view.** `ofs.rerun.view(sim)` opens the Rerun viewer on a running session: the quad's pose in the world,
+  battery, motor, link and video curves, the events as text. It follows the state stream (up to 240 Hz); the 8 kHz
+  signals are in the recording.
 
 License: GPL-3.0-or-later.
````

- [ ] **Step 2: dev-setup, the specs, the research note**

**Modify `docs/dev-setup.md`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/docs/dev-setup.md b/docs/dev-setup.md
index f3a4093..058b969 100644
--- a/docs/dev-setup.md
+++ b/docs/dev-setup.md
@@ -11,7 +11,9 @@
 | Live SITL tests | `OFS_SITL_LAUNCH=<cmd> cargo test -p ofs-fc -p ofs-sim --test sitl_live -- --ignored --test-threads=1` |
 | Live video tests | `OFS_SITL_LAUNCH=<cmd> cargo test -p ofs-video --test vtx_live -- --ignored --test-threads=1`, and `python -m pytest python/tests/test_sitl_video.py` |
 | Server | `cargo run -p ofs-sim -- --listen 127.0.0.1:50051 --data-dir .ofs-data` (Ctrl-C stops it and its SITL) |
-| Python tests | `cargo build -p ofs-sim && python -m pytest python/tests -v` |
+| Python tests | `cargo build -p ofs-sim && python -m pytest python/tests -v` (after `python -m pip install -e "python[dev]"`, which brings the recording, Parquet and Rerun extras the tests use) |
+| Live Python tests in WSL | from `python/`: `OFS_SIM_BIN=$HOME/ofs/target/debug/ofs-sim OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf PYTHONPATH=. uv run --no-project --with grpcio,protobuf,pytest,mcap,pandas,pyarrow,'rerun-sdk>=0.39,<0.40' python -m pytest tests` (a Linux-built `ofs-sim`) |
+| Build the Python package | `python -m build python --outdir dist` (wheel and sdist; CI publishes them on `v*` tags only) |
 | Godot client tests | `bash scripts/run-godot-tests.sh` (see "Godot pilot client" below) |
 | Real-time session (for Configurator) | `OFS_SITL_LAUNCH=<cmd> python python/examples/serve_realtime.py` |
 | Regenerate Python stubs | `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto` |
@@ -207,6 +209,34 @@ The client and server speak **protocol 8**: the faults' wire shapes live in the
 `sim.clear_faults()`), and the models, constants and their estimate status in `docs/research/faults.md` —
 regenerate the stubs with the table's protoc recipe after changing the proto.
 
+## Recording, scripted inputs and the package (M4c)
+
+The client and server speak **protocol 9**: `StartRecording`/`StopRecording`, `PlayInputs`/`StopInputs`, a fault's
+optional `at_s`, world names in `LoadRequest.world_path`, and `HandshakeReply.logs_dir`.
+
+- **Recordings** land in `<data_dir>/logs/<quad stem>-<n>.mcap`; the server names them and never writes a
+  client-supplied path. The recorder (`crates/ofs-log`) is the last model of each tick and only reads the bus. A
+  record carries the state `GetState` reports at its time (`{"v": x}`, `{"x", "y", "z"}` or `{"w", "x", "y",
+  "z"}` JSON with a JSON schema); the default rates are `ofs_log::DEFAULT_RATES`. The files are deterministic:
+  the `mcap` crate is pinned to an upstream revision that writes its summary in id order
+  (`docs/research/mcap-determinism.md`), and the writer pins its library string, single-threaded zstd and no
+  per-message index records (smaller files; readers use the chunk index).
+- **Python:** a lockstep `load` starts a recording (`record=False` does not), `sim.log` stops and returns it,
+  `ofs.reader` reads any recording (`mcap` + `pandas`; Parquet also `pyarrow`), `ofs.rerun.view(sim)` is the live
+  view (`rerun-sdk` 0.39.x, pinned by the extra).
+- **Scripted inputs** (`sim.play_inputs(path)`): the client sends the CSV's text (the server accepts requests up
+  to 64 MiB, `MAX_REQUEST_BYTES`, so a long flight's file fits); the server validates it once
+  (`crates/ofs-sim/src/inputs.rs`) and applies each row on the first tick at or after its time. Faults with `at=`
+  use the same tick rule. Both apply right after the step that reaches their time, so a `run` that ends there
+  already shows them.
+- **Worlds by name:** `load(world="flat")` reads `worlds/flat.toml` beside the quad's directory, as parts resolve
+  against `library/`.
+- **Blackbox:** the shipped diffs set `blackbox_device = VIRTUAL`; SITL writes `LOGnnnnn.BFL` into the quad's
+  firmware directory, and the session copies the logs it wrote into the logs directory when it ends.
+- **Packaging:** the PyPI distribution is `open-fpv-sim` (import `ofs`), pure Python; `launch()` still needs an
+  `ofs-sim` binary. The `publish` CI job runs on `v*` tags only, checks the tag equals `python/pyproject.toml`'s
+  version and uploads through PyPI trusted publishing (the `pypi` environment); no token is stored.
+
 ## Troubleshooting (Windows)
 Live SITL tests run from Windows against SITL in WSL, and SITL's datagrams reach the Windows host through the Windows Firewall.
 - **Symptom:** SITL looks silent and runs fail with "no motor output within 5000 ms" or a first-exchange timeout.
@@ -219,7 +249,8 @@ Live SITL tests run from Windows against SITL in WSL, and SITL's datagrams reach
   CARGO_TARGET_DIR=$HOME/ofs/target OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf cargo test --workspace --locked -- --include-ignored --test-threads=1
   ```
   The second command runs every Rust test, the live ones (`sitl_live`, `vtx_live`) included.
-  WSL has no Python grpc module, so the Python tests need Windows.
+  WSL's system Python has no grpc module: run the Python tests there through `uv` (the "Live Python tests in
+  WSL" command above), or from Windows.
 
 ## Sessions
 - **Lockstep** (default): simulated time advances only through `Run`. Runs are deterministic, Betaflight included.
@@ -233,7 +264,7 @@ Live SITL tests run from Windows against SITL in WSL, and SITL's datagrams reach
 
 ## Firmware state
 - **Directory:** each quad file gets `<data_dir>/<quad file stem>-<8 hex digits>/` (the digits hash the quad file's path), holding `eeprom.bin`, `betaflight.diff` (the copy applied on first boot), `sitl.log` and any Blackbox logs.
-- **Diff changes:** the quad's diff is applied only on first boot, so Configurator changes persist. If the quad's diff changes later, loading fails until you delete `eeprom.bin` (which also discards Configurator changes).
+- **Diff changes:** the quad's diff is applied only on first boot, so Configurator changes persist. If the quad's diff changes later, loading fails until you delete `eeprom.bin` (which also discards Configurator changes). M4c changed both shipped diffs (`set blackbox_device = VIRTUAL`): delete the `eeprom.bin` of each quad you flew before it, once.
 - **Reboots:** a Betaflight reboot (Configurator "Save", MSP_REBOOT) relaunches SITL from its EEPROM; `State.fc_restarts` counts them.
 
 ## Betaflight Configurator
````

**Modify `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md b/docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md
index 08b7809..a6111dd 100644
--- a/docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md
+++ b/docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md
@@ -161,8 +161,11 @@ sim.load("quads/opendrone-5f-freestyle.toml", world="flat", seed=42, mode="locks
 sim.inject(ofs.faults.MotorFailure(motor=2, at=3.0))
 sim.play_inputs("flips.csv")
 sim.run(seconds=10)
-df = sim.log.to_dataframe(["gyro.*", "motor.*.rpm", "battery.voltage"])
+df = sim.log.to_dataframe(["imu.gyro_frd_radps", "motor.*.omega_radps", "battery.voltage_v"])
 ```
+(M4c: the signal names are the bus's own; this example predates them. It ships as `python/examples/sandbox.py` and
+runs as printed.)
+
 **v1 fault catalog:** motor failure/degradation; prop damage (thrust loss + imbalance vibration); weak battery cell; packet loss and burst loss (→ failsafe); gyro noise and bias; VTX interference.
 
 ### 6.3 (D) Logging and inspection
````

**Modify `docs/superpowers/specs/2026-10-11-m4-sandbox-design.md`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/docs/superpowers/specs/2026-10-11-m4-sandbox-design.md b/docs/superpowers/specs/2026-10-11-m4-sandbox-design.md
index 533f17f..f390267 100644
--- a/docs/superpowers/specs/2026-10-11-m4-sandbox-design.md
+++ b/docs/superpowers/specs/2026-10-11-m4-sandbox-design.md
@@ -113,7 +113,7 @@ The v1 catalog is the parent's §6.2 list. Every fault: a python dataclass in `o
 New crate `ofs-log` (the parent §4.2's missing row). A **`Recorder`** is a scheduler model, appended last, that only **reads** the bus (recording never changes a run: the digest is identical with and without it):
 
 - **Spec:** a list of `(glob, rate_hz)` resolved against bus signal names at build — e.g. `("imu.*", 1000)`, `("motor.*", 240)`. A **default table** covers every signal class (body 240 Hz, imu 1 kHz, motor/esc 240 Hz, battery 10 Hz, radio/video/rc/fault 50 Hz, catch-all 50 Hz); callers override per pattern. Unmatched patterns are an error listing near-misses.
-- **Writer:** the `mcap` crate; one channel per signal (name = bus name); records are JSON (`{"v": x}` / `[x,y,z]` / `[w,x,y,z]` for vec3/quat); `publish_time = log_time` = simulated time in ns. **Determinism:** the writer pins all metadata (fixed library string, no wall-clock anywhere); two lockstep runs with the same seed and inputs produce **byte-identical files** — the §8.4 determinism test upgrades from bus digest to MCAP file hash (the digest test stays as the faster unit).
+- **Writer:** the `mcap` crate; one channel per signal (name = bus name); records are JSON (`{"v": x}` / `[x,y,z]` / `[w,x,y,z]` for vec3/quat — M4c plan ruling: objects `{"x","y","z"}` / `{"w","x","y","z"}` with a JSON schema per shape, so Foxglove can address the fields); `publish_time = log_time` = simulated time in ns. **Determinism:** the writer pins all metadata (fixed library string, no wall-clock anywhere); two lockstep runs with the same seed and inputs produce **byte-identical files** — the §8.4 determinism test upgrades from bus digest to MCAP file hash (the digest test stays as the faster unit).
 - **API:** `StartRecording(signals, …)` / `StopRecording()` → summary `{path, messages, first_s, last_s}`. Files land under `<data_dir>/logs/<quad-stem>-<n>.mcap`; the server names them (client-supplied paths are refused — no server-side arbitrary writes).
 - **Python:** `sim.recording.start(patterns=None)` / `.stop()`; `ofs.reader.to_dataframe(path, patterns)` with pandas (extra `ofs[log]`, with `mcap`), plus CSV (stdlib) and Parquet (`df.to_parquet`) export helpers.
 - **Blackbox surfacing:** on `Unload`, the session copies SITL's `LOG*.BFL` from the firmware dir into `<data_dir>/logs/` (created as needed; `sim.blackbox_files()` lists them) — the parent §6.3's "Blackbox for Blackbox Explorer" stops being a buried directory.
@@ -152,7 +152,7 @@ New crate `ofs-log` (the parent §4.2's missing row). A **`Recorder`** is a sche
 
 | Risk | Mitigation |
 |---|---|
-| The `mcap` crate embeds nondeterministic bytes (chunk offsets, CRCs, library string), breaking byte-identity. | Pinned metadata and a fixed write order; the byte-identity test is written **first** in M4c against a throwaway two-signal recording; fallback of last resort is hashing the message records only, recorded as debt if taken. |
+| The `mcap` crate embeds nondeterministic bytes (chunk offsets, CRCs, library string), breaking byte-identity. | Pinned metadata and a fixed write order; the byte-identity test is written **first** in M4c against a throwaway two-signal recording; fallback of last resort is hashing the message records only, recorded as debt if taken. **Resolved before M4c (2026-10-12):** crates.io `mcap` 0.25.0 orders its summary by `HashMap`; the pinned upstream revision `00e47a1b` writes it in id order and is byte-identical across runs and platforms (`docs/research/mcap-determinism.md`); no fallback taken. |
 | Per-fault seams touch every model crate at once (M4b is wide, not deep). | One seam per model with its own bit-identity test before any fault lands; the faults themselves are then independent tasks. |
 | M3e merge conflicts (vehicle.rs, ofs-config, quads/, proto). | M4a is written after M3e merges (the M3d→M3e rule); the overlap regions are disjoint (Handles/Fault vs VideoHandles; quad parts vs VtxKind/world). |
 | Provenance-mandatory makes quick local experiments tedious. | `OFS_LIBRARY` points at a user dir; a one-screen example part ships; the strictness is the milestone's deliverable. |
````

**Modify `docs/research/mcap-determinism.md`** (apply with `git apply`, or edit by hand: the hunks show exact context):

````diff
diff --git a/docs/research/mcap-determinism.md b/docs/research/mcap-determinism.md
index 975247e..d98a439 100644
--- a/docs/research/mcap-determinism.md
+++ b/docs/research/mcap-determinism.md
@@ -59,6 +59,16 @@ The bytes also match a hand-patched 0.25.0 that sorts the summary by id, so #184
 output. The Python reader reads every variant fully: indexed, filtered by topic, by time window and as a
 stream.
 
+## File size: no per-message index records
+
+Measured in M4c with the shipped quad, open loop, 60 simulated seconds at the default rates (105 signals, 15 456
+records per simulated second): **24.9 MB** with the crate's defaults, **9.9 MB** with `emit_message_indexes(false)`.
+A message index record is 16 uncompressed bytes per message, written after each chunk, so it outweighed the
+zstd-compressed records themselves. Readers do not need it: the Python reader's indexed, topic-filtered and
+time-window reads and the Rust `MessageStream` all read the files without it (they use the chunk index and decompress
+whole chunks). `ofs-log` writes without them. Reading back costs about 3.7 s for three patterns of a one-minute
+recording in pure Python (`ofs.reader`), 11.7 s for every signal.
+
 ## Test data must avoid platform libm
 
 The Windows and Linux files first differed at byte 40 — the first chunk's length. The spike's test signal
````

- [ ] **Step 3: The carried debt**

**Create `docs/superpowers/m4c-carried-debt.md`:**

````markdown
# M4c carried debt

## Deferred features
- **The repro bundle** (parent spec §7) — post-v1; M4c provides its halves: recordings and scripted inputs.
- **MCAP in Foxglove** — the files are standard MCAP with a JSON schema per record shape, but opening them in
  Foxglove (and live tailing a recording still being written) is untested; manual check.
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
- **The first PyPI tag needs setup on pypi.org** — the `open-fpv-sim` project and its trusted publisher (this
  repository, workflow `ci.yml`, environment `pypi`) must exist before the first `v*` tag; which tag publishes first
  is the maintainer's call.
- Inherited and still open: fcs/receivers/cameras part kinds (m4a); windmilling drag, battery capacity fade,
  vibration transmissibility (m4b); `test_sitl_link.py` not in the sitl job's list (m3e); the flaky
  `runner::tests::a_panicking_step_poisons_the_session_and_keeps_the_runner_alive` (an `overrun` can precede the
  `sim_error` it looks for; fails about half the time under WSL, on main too).

## Rulings worth remembering
- **Records are JSON objects** (`{"v"}`, `{"x","y","z"}`, `{"w","x","y","z"}`) with a JSON schema per shape, not the
  spec's bare arrays: Foxglove's message paths need object roots.
- **No per-message index records** — 9.9 MB instead of 24.9 MB per simulated minute at the default rates; readers
  use the chunk index (`docs/research/mcap-determinism.md`).
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
  finished); a new `load` finishes the previous session's file on the server.
- **A world name** (`world="flat"`) is `<quad dir>/../worlds/<name>.toml`, as parts resolve against `library/`.
- **The Blackbox copy is a `Session` field declared last**: fields drop in declaration order, so SITL is gone and
  its files closed before the copy runs, on every way a session ends.
- **`launch()` closes its server at interpreter exit** (a weak reference through `atexit`), so the §6.2 script,
  which never calls `close()`, still finishes its recording and Blackbox logs.
````

```bash
git add README.md docs
git commit -m "docs: M4c - recording and the package documented, the 6.2 example's names, the carried debt"
```

---

## Final verification (before merge)

- [ ] **Windows** (repository root):
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`: no output.
  - `cargo test --workspace --locked`: all pass (the prototype counted 516).
  - `python -m pytest python/tests -q`: all pass, SITL skipped (prototype: 50 passed, 22 skipped).
- [ ] **WSL**:
  - Task 7's two commands: Rust 527 passed with `--include-ignored --test-threads=1`, and Python 72 passed against live SITL.
  - The clean-venv wheel check: 1 passed.
- [ ] **Godot**: `GODOT_BIN=<console exe> bash scripts/run-godot-tests.sh all` passes. The event switch's default branch shows the four new event kinds, so no GDScript changes.
- [ ] **Manual:**
  - Open a recording from `.ofs-data/logs/` in Foxglove. Plot `/battery.voltage_v.v` and `/imu.gyro_frd_radps.x` (debt if it fails).
  - Open a copied `.BFL` in Blackbox Explorer.
  - Open the Rerun viewer (Task 6's manual check).
- [ ] **Before the first `v*` tag** (the maintainer's call):
  - Create the `open-fpv-sim` project's trusted publisher on pypi.org (this repository, workflow `ci.yml`, environment `pypi`).
  - Create the `pypi` environment in the repository's settings.
  - Re-check that the name is still free.
