# Post-audit fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close #185, #186 and #164: parse `Load`/`PlayInputs` off the async runtime, let DVR recordings pass 4 GiB in OpenDML segments, and pin the 5F's full-throttle current to the electrical model.

**Architecture:** Three independent fixes in one branch. #185 wraps the two parses in `tokio::task::spawn_blocking` and keeps the existing `self.blocking` session work. #186 adds `RIFF/AVIX` segments and a `dmlh` total to the AVI writer, and teaches the export reader to walk them. #164 is a diagnosis test plus the ruling from the issue, unless the test shows the clamp is binding at steady state.

**Tech Stack:** Rust (`ofs-sim`, `ofs-client`), Tokio, Tonic, Godot (one toast string).

**Spec:** `docs/superpowers/specs/2026-11-18-post-audit-fixes-design.md`

## Global Constraints

- Branch base: `main` at `2379a57`; work on `fix/post-audit-fixes`.
- No protocol or wire change; no new dependency.
- AVI 1.0 files still open; the first segment stays `RIFF/AVI `; later segments are `RIFF/AVIX`.
- The total DVR cap is 64 GiB and is still reported by `truncated`.
- #164's expected outcome is a ruling. If the test shows the clamp binds at steady state, stop and report; do not change the electrical model in this batch.
- One commit per issue, tests with the fix, `cargo fmt --all` before each commit.

## Review Focus

- A frame larger than a segment: the writer must start the segment and write the frame without looping or dropping it.
- A recording that reaches the 64 GiB total cap: `truncated` is still true and the file is valid.
- An unfinalized AVI (header sizes zero, no final `idx1`): export still walks to the complete frames.
- A `PlayInputs` CSV just under the 64 MiB request cap: no runtime stall and no session-mutex hold.
- A 5F at full throttle on a sagging pack: the steady current still follows `(V − Ke·ω)/R`; the startup clamp does not persist.

---

### Task 1: #185 — parse `Load` and `PlayInputs` off the async runtime

**Files:**
- Modify: `crates/ofs-sim/src/server.rs:291-300` (`load`), `crates/ofs-sim/src/server.rs:595-599` (`play_inputs`)
- Test: `crates/ofs-sim/tests/grpc.rs` (append one test)

**Interfaces:**
- Consumes: `ofs_config::load`, `ofs_config::world::load`, `inputs::parse` as today.
- Produces: no new public API; the same RPC results and error kinds.

- [ ] **Step 1: Write the failing test**

Append to `crates/ofs-sim/tests/grpc.rs`:

```rust
#[tokio::test]
async fn a_large_inputs_file_does_not_stall_other_calls() {
    let mut c = start().await;
    load_open_loop(&mut c).await;

    // Just under the 64 MiB request cap: 2 million rows with nine columns.
    let mut csv = String::with_capacity(64 * 1024 * 1024);
    csv.push_str("t_s,roll,pitch,yaw,throttle,aux1,aux2,aux3,aux4\n");
    for i in 0..2_000_000 {
        csv.push_str(&format!("{:.6},0,0,0,0.5,0,0,0,0\n", i as f64 * 1e-6));
    }

    let mut playing = c.clone();
    let started = std::time::Instant::now();
    let play = tokio::spawn(async move {
        playing.play_inputs(InputsFile { name: "big.csv".into(), csv }).await
    });
    // Let the request reach the server and start the parse before asking for state.
    tokio::time::sleep(Duration::from_millis(20)).await;
    c.get_state(Empty {}).await.unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "GetState waited {:?}: the parse ran on the runtime",
        started.elapsed()
    );
    play.await.unwrap().unwrap();
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ofs-sim --test grpc a_large_inputs_file_does_not_stall_other_calls -- --nocapture`

Expected: FAIL on the elapsed assertion (the parse blocks the current-thread test runtime).

- [ ] **Step 3: Implement the offload in `load`**

Replace the two parse calls (lines 294-300) with one blocking closure. Move only the paths in; `req` is still needed below:

```rust
let quad_path = req.quad_path.clone();
let world_path = req.world_path.clone();
let (cfg, world) = tokio::task::spawn_blocking(move || {
    let cfg = ofs_config::load(Path::new(&quad_path))
        .map_err(|e| error("config", Code::InvalidArgument, e.to_string()))?;
    let world = if world_path.is_empty() {
        WorldConfig::open_field()
    } else {
        let path = ofs_config::world::resolve(&world_path, Path::new(&quad_path));
        ofs_config::world::load(&path)
            .map_err(|e| error("config", Code::InvalidArgument, e.to_string()))?
    };
    Ok::<_, Status>((cfg, world))
})
.await
.map_err(|e| Status::internal(format!("the config parse panicked: {e}")))??;
```

- [ ] **Step 4: Implement the offload in `play_inputs`**

Replace line 598. Clone the name for the message used below:

```rust
let csv = file.csv;
let parse_name = name.clone();
let inputs = tokio::task::spawn_blocking(move || inputs::parse(&parse_name, &csv))
    .await
    .map_err(|e| Status::internal(format!("the inputs parse panicked: {e}")))?
    .map_err(|m| error("config", Code::InvalidArgument, m))?;
```

- [ ] **Step 5: Run the test and the suite**

Run: `cargo test -p ofs-sim --test grpc`

Expected: all tests pass, including the new one.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/ofs-sim/src/server.rs crates/ofs-sim/tests/grpc.rs
git commit -m "fix(sim): parse Load and PlayInputs off the async runtime"
```

---

### Task 2: #186 — make the AVI writer roll into OpenDML segments

**Files:**
- Modify: `crates/ofs-client/src/dvr.rs:16-23` (constants), `:69-205` (`write_header`, `writer_with_limit`), `:246-251` (`DvrWriter::start`)
- Test: `crates/ofs-client/src/dvr.rs` (`mod tests`)

**Interfaces:**
- Produces: `writer_with_limit(file, header, rx, segment_bytes: u64, max_bytes: u64, truncated: &AtomicBool) -> Result<DvrSummary, String>`; `write_header(...) -> io::Result<[usize; 5]>`; `write_segment_header(file) -> io::Result<[usize; 2]>`; `AVI_SEGMENT_BYTES`, `MAX_DVR_BYTES`.
- `DvrSummary` is unchanged.

- [ ] **Step 1: Write the failing tests**

In `crates/ofs-client/src/dvr.rs`'s `mod tests`, rename the existing cap test and change its call to
`writer_with_limit(file, header, rx, 1024, 1024, &truncated)` (one 1 KiB segment, the same total cap). Then add:

```rust
#[test]
fn a_recording_rolls_to_avix_segments_and_keeps_every_frame() {
    let dir = std::env::temp_dir().join(format!("ofs-dvr-roll-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("roll.avi");
    let file = File::create_new(&path).unwrap();
    let header = Header { microsec_per_frame: 20_000, fps: 50, width: 4, height: 2 };
    let (tx, rx) = sync_channel(64);
    let truncated = std::sync::atomic::AtomicBool::new(false);
    let handle = std::thread::spawn(move || {
        writer_with_limit(file, header, rx, 1024, MAX_DVR_BYTES, &truncated)
    });
    for i in 0..50u8 {
        tx.send(vec![i; 100]).unwrap();
    }
    tx.send(vec![0xFF; 2048]).unwrap(); // larger than a whole segment
    drop(tx);
    let summary = handle.join().unwrap().unwrap();

    assert_eq!(summary.frames, 51, "every frame is kept, including the oversized one");
    assert!(!summary.truncated);

    let bytes = std::fs::read(&path).unwrap();
    let mut pos = 0usize;
    let mut segments = 0usize;
    let mut total = 0usize;
    while pos + 12 <= bytes.len() {
        assert_eq!(&bytes[pos..pos + 4], b"RIFF", "segment {segments}");
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let form = &bytes[pos + 8..pos + 12];
        assert_eq!(form, if segments == 0 { b"AVI " } else { b"AVIX" }, "segment {segments} form");
        let end = pos + 8 + size;
        assert!(end <= bytes.len(), "segment {segments} fits");
        let segment = &bytes[pos..end];
        let idx = segment.windows(4).position(|w| w == b"idx1").expect("each segment has an index");
        total += u32::from_le_bytes(segment[idx + 4..idx + 8].try_into().unwrap()) as usize / 16;
        segments += 1;
        pos = end;
    }
    assert!(segments >= 2, "the tiny limit rolls at least once");
    assert_eq!(total, 51, "the segments' indexes add up");
    let dmlh = bytes.windows(4).position(|w| w == b"dmlh").expect("the OpenDML total");
    assert_eq!(u32::from_le_bytes(bytes[dmlh + 8..dmlh + 12].try_into().unwrap()), 51, "dmlh total frames");
    std::fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ofs-client --lib dvr`

Expected: compile error on the extra `writer_with_limit` argument; the new rollover assertions fail once it compiles.

- [ ] **Step 3: Add the `dmlh` header and the new constants**

Replace `MAX_AVI_FILE_BYTES` with:

```rust
/// AVI 1.0's 32-bit sizes cap one RIFF segment; a longer recording starts a `RIFF/AVIX` segment.
const AVI_SEGMENT_BYTES: u64 = u32::MAX as u64 + 8;
/// The total cap: about 70 hours at 50 fps and typical 640x480 frames.
const MAX_DVR_BYTES: u64 = 64 * 1024 * 1024 * 1024;
```

In `write_header`, keep the existing chunks and append `dmlh` after `strl`: a 248-byte data field (a patched `dwTotalFrames` plus 61 reserved DWORDs), growing the `hdrl` size from 192 to 448. Return `[riff_size_at, movi_size_at, avih_frames_at, strh_length_at, dmlh_frames_at]`; `movi_size_at` moves from 216 to 472 and `dmlh_frames_at` is 220. Update the `debug_assert_eq!` to 468 and the doc comment.

Add:

```rust
/// Starts a `RIFF/AVIX` segment: a `movi` LIST for the next frames. Returns the absolute offsets of the
/// segment's RIFF size and `movi` size fields.
fn write_segment_header(file: &mut File) -> std::io::Result<[usize; 2]> {
    let start = file.stream_position()? as usize;
    let mut b = Vec::with_capacity(24);
    put(&mut b, b"RIFF");
    put32(&mut b, 0);
    put(&mut b, b"AVIX");
    put(&mut b, b"LIST");
    put32(&mut b, 0);
    put(&mut b, b"movi");
    file.write_all(&b)?;
    Ok([start + 4, start + 16])
}
```

- [ ] **Step 4: Teach `writer_with_limit` to roll**

Change the signature to take `segment_bytes` and `max_bytes`. Keep the header write and the frame loop, with the index and frame counter per segment:

- Track `segment_start`, `segment_frames`, `total_frames`, the current segment's `[riff_size_at, movi_size_at]`, and the first header's `avih_frames_at`, `strh_length_at`, `dmlh_frames_at`.
- Before a frame: if `data_end + chunk + 8 + 16 * (segment_frames + 1) > max_bytes`, set `truncated` and break.
- If `segment_frames > 0` and the same sum exceeds `segment_start + segment_bytes`, finish the current segment (write its `idx1`, patch its RIFF and `movi` sizes), then call `write_segment_header`, reset the segment index and frame count, and continue with the same frame.
- Write the frame, push its size, and advance the counters.
- After the queue drains: finish the current segment, patch the first header's `avih`/`strh` to the first segment's frames and `dmlh` to `total_frames`, and return `DvrSummary { frames: total_frames, bytes: end, truncated }`.

Update `DvrWriter::start` to pass `AVI_SEGMENT_BYTES, MAX_DVR_BYTES`.

- [ ] **Step 5: Run the writer tests**

Run: `cargo test -p ofs-client --lib dvr`

Expected: all pass, including the rollover test and the unchanged total-cap test.

- [ ] **Step 6: Add the test-only fixture helper**

In `dvr.rs`, outside `mod tests`:

```rust
/// A tiny multi-segment recording for the export tests.
#[cfg(test)]
pub(crate) fn write_test_recording(path: &std::path::Path, frames: &[Vec<u8>], segment_bytes: u64) -> DvrSummary {
    let file = File::create_new(path).expect("the fixture file");
    let header = Header { microsec_per_frame: 20_000, fps: 50, width: 4, height: 2 };
    let (tx, rx) = sync_channel(64);
    let truncated = std::sync::atomic::AtomicBool::new(false);
    let handle = std::thread::spawn(move || writer_with_limit(file, header, rx, segment_bytes, MAX_DVR_BYTES, &truncated));
    for frame in frames {
        tx.send(frame.clone()).expect("the fixture frame");
    }
    drop(tx);
    handle.join().expect("the fixture writer").expect("the fixture summary")
}
```

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add crates/ofs-client/src/dvr.rs
git commit -m "fix(dvr): roll recordings into OpenDML AVIX segments"
```

---

### Task 3: #186 — read every segment in the export, and fix the 4 GiB wording

**Files:**
- Modify: `crates/ofs-client/src/dvr_export.rs:17-91` (`export_frames_with`)
- Modify: `crates/ofs-client/tests/dvr_export.rs:80-105` (offsets), append one test
- Modify: `godot/scripts/app.gd:377,391`, `godot/ui/dvr.gd:173`, `godot/tests/test_dvr.gd:53,974`

**Interfaces:**
- Consumes: `crate::dvr::write_test_recording(path, frames, segment_bytes) -> DvrSummary`.
- Produces: `export_frames` with unchanged signature and return type.

- [ ] **Step 1: Write the failing export test**

Append to `crates/ofs-client/tests/dvr_export.rs`:

```rust
#[test]
fn a_multi_segment_recording_exports_every_frame() {
    let dir = fresh_dir("multisegment");
    let avi = dir.join("rec.avi");
    let frames: Vec<Vec<u8>> = (0..7u8).map(|i| vec![i; 50]).collect();
    crate::dvr::write_test_recording(&avi, &frames, 512);
    let (out, count) = export_frames(&avi).unwrap();
    assert_eq!(count, 7, "every segment's frames are exported");
    for (i, frame) in frames.iter().enumerate() {
        let path = out.join(format!("frame_{:06}.jpg", i + 1));
        assert_eq!(&std::fs::read(path).unwrap(), frame, "frame {}", i + 1);
    }
    std::fs::remove_dir_all(&dir).ok();
}
```

Update `an_unfinalized_avi_still_exports` for the new 480-byte header: the comment says `movi` is at 476, the zeroed offsets are `[4, 472, 48, 140]`, and `movi_end` is `480 + 3 * 18`.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ofs-client --test dvr_export a_multi_segment_recording_exports_every_frame -- --nocapture`

Expected: FAIL after the first segment's frames (the old loop stops at the first `idx1`).

- [ ] **Step 3: Teach the scan loop about segments**

In `export_frames_with`, after reading each 8-byte chunk head:

- `RIFF`: read the 4-byte form. If it is `AVI ` or `AVIX`, skip the form and continue inside the segment; otherwise skip the rest of the chunk.
- `LIST`: read the 4-byte list type. If it is `movi`, continue at its payload; otherwise skip the rest of the list.
- `idx1`: skip it and continue (do not stop). That is how the reader reaches the next `RIFF/AVIX`.
- `00dc`: unchanged.
- Anything else: skip its payload as today.

Handle `RIFF` and `LIST` before the `pos + 8 + size > len` check, so the zero sizes in an unfinalized file still let the reader continue. Keep the first-`movi` search and `HEADER_SCAN` as they are (the first `movi` is at 476, still inside 1024).

- [ ] **Step 4: Update the Godot wording**

- `godot/scripts/app.gd:377`: `" (recording stopped at the size limit)"`.
- `godot/scripts/app.gd:391`: "Stops a recording whose writer hit the recording size limit".
- `godot/ui/dvr.gd:173`: "True once any feed's writer hit the recording size limit: the recording must be stopped."
- `godot/tests/test_dvr.gd:53`: comment "the writer hit the recording size limit".
- `godot/tests/test_dvr.gd:974`: assert `"size limit" in ...` instead of `"4 GiB"`.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ofs-client --test dvr_export`

Expected: all pass, including the unfinalized and multi-segment cases.

Run: `GODOT_BIN=<godot 4.7.2> bash scripts/run-godot-tests.sh unit`

Expected: pass, including `test_a_truncated_recording_stops_itself_and_says_why`.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/ofs-client/src/dvr_export.rs crates/ofs-client/tests/dvr_export.rs godot/scripts/app.gd godot/ui/dvr.gd godot/tests/test_dvr.gd
git commit -m "fix(dvr): export and name OpenDML segments"
```

---

### Task 4: #164 — pin the 5F's startup clamp and steady current

**Files:**
- Modify: `crates/ofs-sim/tests/open_loop_vehicle.rs` (append one test)
- Modify: `docs/roadmap.md` (the ruling)

**Interfaces:**
- Consumes: `ofs_config::load`, `Vehicle::state()` (`battery_current_a`, `battery_voltage_v`, `motor_rpm`), `cfg.motors[0]` (`kv_rpm_per_v`, `resistance_ohm`).
- Produces: no API change.

- [ ] **Step 1: Write the diagnosis test**

Append to `crates/ofs-sim/tests/open_loop_vehicle.rs`:

```rust
#[test]
fn full_throttle_current_is_the_startup_clamp_then_the_electrical_model() {
    let cfg = load(Path::new(QUAD)).unwrap();
    let motor = &cfg.motors[0];
    let ke = 60.0 / (2.0 * std::f64::consts::PI * motor.kv_rpm_per_v);
    let mut v = vehicle(1);
    v.set_sticks(&throttle(1.0)).unwrap();

    v.run_for(0.05).unwrap();
    let early = v.state();
    assert!((early.battery_current_a - 4.0 * 45.0).abs() < 1.0, "startup clamp: {}", early.battery_current_a);

    v.run_for(1.0).unwrap();
    let s = v.state();
    let expected: f64 = s.motor_rpm.iter().map(|rpm| {
        let omega = rpm * 2.0 * std::f64::consts::PI / 60.0;
        (s.battery_voltage_v - ke * omega) / motor.resistance_ohm
    }).sum();
    assert!(
        (s.battery_current_a - expected).abs() < 2.0,
        "steady current {} should follow the model {} (clamp is 180 A)",
        s.battery_current_a,
        expected
    );
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p ofs-sim --test open_loop_vehicle full_throttle_current_is_the_startup_clamp_then_the_electrical_model -- --nocapture`

Expected: PASS. If it FAILS on the steady-current assertion, the clamp is binding at steady state: **stop and report**; changing the electrical model is a separate decision, not this batch.

- [ ] **Step 3: Record the ruling**

Under `## Rulings worth remembering` in `docs/roadmap.md`, add:

```markdown
- **The 5F's full-throttle pack current starts at the four-motor clamp**: each motor's phase current is clamped
  to its ESC's 45 A while the prop spools up, and the pack current is the sum of the four ESC input currents.
  It settles to the electrical model's draw well below 180 A; the 180 A is the startup clamp, not a steady draw.
```

- [ ] **Step 4: Run the test again**

Run: `cargo test -p ofs-sim --test open_loop_vehicle full_throttle_current_is_the_startup_clamp_then_the_electrical_model`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/ofs-sim/tests/open_loop_vehicle.rs docs/roadmap.md
git commit -m "test(sim): pin the 5F's startup clamp and steady current"
```

---

### Task 5: changelog and the full gates

**Files:**
- Modify: `CHANGELOG.md` (`## Unreleased`)
- Modify: `docs/roadmap.md` (the DVR known limitation)

- [ ] **Step 1: Write the changelog**

Add a `### Simulator` section under `## Unreleased` (before `### Pilot client`):

```markdown
### Simulator
#### Fixed
- **`Load` and `PlayInputs` no longer parse on the async runtime**: a large world or inputs file is read on a
  blocking thread, so another RPC is served while it parses (#185).
- **The 5F's 180 A full-throttle reading is the four-motor startup clamp, not the steady draw**: the pack settles
  to the electrical model's current, and a test pins both (the ruling is in `docs/roadmap.md`, #164).

#### Changed
- **DVR recordings continue past 4 GiB** in OpenDML `RIFF/AVIX` segments with a `dmlh` total-frame field; the
  first segment stays AVI 1.0, the export reads every segment, and the size cap is 64 GiB (#186).
```

- [ ] **Step 2: Update the DVR limitation**

In `docs/roadmap.md`'s DVR bullet, add: "A recording that passes 4 GiB continues in OpenDML `AVIX` segments; the first segment stays AVI 1.0 and the export reads every segment."

- [ ] **Step 3: Run the gates**

Run: `cargo fmt --all --check`
Expected: no output.

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: no warnings.

Run: `cargo test --workspace --locked`
Expected: all pass.

Run: `GODOT_BIN=<godot 4.7.2> bash scripts/run-godot-tests.sh unit`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md docs/roadmap.md
git commit -m "docs: the post-audit fixes"
```

---

## Manual check left to the owner

The issue's acceptance also asks for a recording longer than 4 GiB to play in VLC/ffmpeg. The automated tests
cover the segment structure, the `dmlh` total and the export reader; the 30+ minute file is a manual check.
