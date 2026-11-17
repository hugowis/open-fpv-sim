# Audit Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Status: done** — implemented in `c61277b` ("fix: address the 2026-11-17 audit findings (#187)"). The task boxes below were marked after the merge.

**Goal:** Fix the two data-loss bugs and the small correctness/doc drift found in the 2026-11-17 whole-repo audit.

**Architecture:** Small, isolated changes in the crates and files that own each behavior. No new dependencies, no format changes beyond a truthful `truncated` flag on the DVR summary. Each task is independently testable and committable.

**Tech Stack:** Rust 1.85+ (workspace crates `ofs-client`, `ofs-sim`, `ofs-config`, `ofs-log`, `ofs-godot`), Godot 4.7 GDScript, Markdown, Bash.

**Spec:** This document's "Findings" section (the audit of 2026-11-17). There is no separate spec file.

## Global Constraints

- Rust edition 2021, `rust-version = "1.85"`; the installed toolchain is 1.100.0.
- No new dependencies. Use the standard library and what the workspace already has.
- Keep AVI 1.0 (no OpenDML): cap the file cleanly at the RIFF limit instead of writing fields that wrap.
- `MAX_RUN_FOR_S = 86_400.0` (`crates/ofs-core/src/scheduler.rs`) is the cap for a scripted input's `t_s`.
- Rust gate: `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked`.
- Godot gate: `GODOT_BIN=build/godot-dl/Godot_v4.7.2-stable_win64_console.exe bash scripts/run-godot-tests.sh unit` on Windows (Git Bash), or the script's own download on Linux.
- Python gate: `.venv/Scripts/python.exe -m pytest python/tests -v` and `.venv/Scripts/python.exe -m pytest tools/sitegen/tests -q`.
- Live SITL tests need WSL2 or MSYS2; they are skipped without `OFS_SITL_LAUNCH` and are not part of this plan.

## Review Focus

The input classes and failure modes the audit implies but no existing test exercises, most likely to bite first:

1. A DVR recording that crosses 4 GiB must leave a playable AVI with every frame up to the cap and a truthful "truncated" state, not a file with wrapped RIFF/`movi`/`idx1` fields.
2. An inputs CSV with an absurd `t_s` (`1e300`, `86401`) must be refused with a clear error, not panic in debug or schedule at a wrapped tick in release.
3. A DVR start with 0×0, a dimension above `u16::MAX`, or `fps = 0` must fail without creating a file.
4. A relative `data_dir` must still list quads and worlds in the Godot menu.
5. A schema-4 quad must name the schema range this build actually reads (5–7), not a stale one.

Each line has a test in the task that owns the code.

## Findings

### Must fix

**F1 — DVR AVI fields wrap past 4 GiB (data loss).**
`crates/ofs-client/src/dvr.rs:155-185` writes the RIFF size, `movi` LIST size, `idx1` entry sizes and `idx1` offsets as `u32`. At 50 fps a 640×480 MJPEG flight reaches 4 GiB in roughly 30–40 minutes; past that the casts truncate and the AVI is corrupt. The index offsets (`offset += 8 + size + (size & 1)`) wrap first.

**F2 — `play_inputs` overflows on a huge `t_s` (panic/wrong schedule).**
`crates/ofs-sim/src/inputs.rs:171-187` accepts any finite `t_s`; `crates/ofs-sim/src/vehicle.rs:1486` computes `now + tick_at(*t, base_hz)`. `tick_at` saturates to `u64::MAX`, so the addition overflows: a debug build panics, a release build wraps and applies the row at the wrong tick. The file is client-supplied (up to 64 MiB), so this is a trust-boundary bug.

### Should fix

**F3 — README quad count and list are stale.**
`README.md:204-211` says "Eight quads ship", "The five 5\" freestyle ones" and "(and four variants)". The repo ships nine quads and six 5" freestyle variants; `quads/opendrone-5f-ibus.toml` is missing from the list at `README.md:198-199`.

**F4 — the Godot catalog ignores a relative `data_dir`.**
`godot/scripts/catalog.gd:18-20` uses `DirAccess.dir_exists_absolute` and `DirAccess.get_files_at`, which take absolute paths. A `--data-dir=relative` (or `OFS_DATA_DIR=relative`) therefore lists no quads or worlds in the menu.

**F5 — the schema-4 error names the wrong range.**
`crates/ofs-config/src/lib.rs:769` says "this build reads schemas 5 and 6"; `SCHEMA_VERSION = 7` and `MIN_SCHEMA_VERSION = 5`.

**F6 — the `ofs-proto` description is stale.**
`crates/ofs-proto/Cargo.toml:7` says "Protocol 2 messages"; the protocol is 18. A generic description avoids this drift recurring.

**F7 — DVR start accepts invalid sizes and leaves a file on spawn failure.**
`crates/ofs-client/src/dvr.rs:202-225` accepts `width = 0`, `height = 0`, dimensions above `u16::MAX` (the `rcFrame` fields truncate) and `fps = 0` (the `strh` rate is 0). If the writer thread fails to spawn, the created file is left behind.

### Nice to have

**F8 — `ofs-log` resolves the recording spec twice.**
`crates/ofs-log/src/recorder.rs:204-213` calls `resolve` to validate, then `start_with` calls it again at line 229. Harmless CPU, but easy to remove without losing the "a refused request creates no file" guarantee.

**F9 — the Godot test script cannot download Godot on Windows.**
`scripts/run-godot-tests.sh:20-31` handles Darwin and Linux only. Under Git Bash `uname -s` is `MINGW64_NT-…`, so the script downloads the Linux binary and fails. The dev-setup tells Windows users to download the zip by hand; the script can do it with the pinned SHA-512 instead.

**Not planned — `Load`/`play_inputs` parse on the async runtime.**
`crates/ofs-sim/src/server.rs:294-300` and `:598` parse config/world/CSV on a tokio worker. Moving that work into `self.blocking` would hold the session mutex during a 64 MiB GLB or CSV parse, which is worse for concurrent RPCs; a correct fix needs a separate `spawn_blocking` and buys nothing for a local single-client server. Leave it.

**Not planned — OpenDML AVI.**
The cap is the smallest complete fix. OpenDML would allow recordings past 4 GiB but adds a new index format and much more code; nobody asked for it.

## File Structure

| File | Task | Responsibility |
|---|---|---|
| `crates/ofs-client/src/dvr.rs` | 1, 3 | AVI muxer: size cap, `truncated` flag, start validation |
| `crates/ofs-client/src/client.rs` | 1 | `Client::dvr_truncated`; existing DVR API |
| `crates/ofs-godot/src/lib.rs` | 1 | Expose `truncated` and `dvr_truncated` to GDScript |
| `godot/ui/dvr.gd` | 1 | `truncated()` over the active feeds; summary carries the flag |
| `godot/scripts/app.gd` | 1 | Poll the flag, stop the recording, toast the reason |
| `godot/tests/test_dvr.gd` | 1 | Mock client + auto-stop test |
| `crates/ofs-sim/src/inputs.rs` | 2 | Reject `t_s` past `MAX_RUN_FOR_S`; unit test |
| `crates/ofs-sim/src/vehicle.rs` | 2 | `saturating_add` when turning row times into ticks |
| `crates/ofs-sim/tests/grpc.rs` | 2 | End-to-end refusal test |
| `README.md` | 4 | Quad count, list and variant count |
| `godot/scripts/catalog.gd` | 5 | `DirAccess.open` so relative roots work |
| `godot/tests/test_catalog.gd` | 5 | Relative-root test |
| `crates/ofs-config/src/lib.rs` | 6 | Schema-4 message uses the constants |
| `crates/ofs-config/tests/config.rs` | 6 | Assert the current range |
| `crates/ofs-proto/Cargo.toml` | 7 | Generic protocol description |
| `scripts/run-godot-tests.sh` | 8 | Windows asset and SHA-512 |
| `crates/ofs-log/src/recorder.rs` | 9 | One `resolve` per start, file removed on failure |

---

### Task 1: Cap the DVR AVI at the RIFF limit and report truncation (F1)

**Files:**
- Modify: `crates/ofs-client/src/dvr.rs`
- Modify: `crates/ofs-client/src/client.rs`
- Modify: `crates/ofs-godot/src/lib.rs`
- Modify: `godot/ui/dvr.gd`
- Modify: `godot/scripts/app.gd`
- Test: `godot/tests/test_dvr.gd`

**Interfaces:**
- Produces: `DvrSummary { frames: u64, dropped_frames: u64, bytes: u64, truncated: bool }`
- Produces: `DvrWriter::truncated(&self) -> bool`
- Produces: `Client::dvr_truncated(&self, feed: DvrFeed) -> bool`
- Produces: GDScript `OfsClient.dvr_truncated(feed: String) -> bool` and the `dvr_stop` dictionary key `"truncated": bool`
- Produces: GDScript `Dvr.truncated() -> bool` and `Dvr.stop()` summary key `"truncated"`

- [x] **Step 1: Write the failing Rust test for the cap**

Add a `#[cfg(test)] mod tests` at the end of `crates/ofs-client/src/dvr.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_stops_at_the_avi_limit_and_finalizes_a_valid_file() {
        let dir = std::env::temp_dir().join(format!("ofs-dvr-limit-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("limit.avi");
        let file = File::create_new(&path).unwrap();
        let header = Header { microsec_per_frame: 20_000, fps: 50, width: 4, height: 2 };
        let (tx, rx) = sync_channel(4);
        let truncated = std::sync::atomic::AtomicBool::new(false);
        let handle = std::thread::spawn(move || {
            writer_with_limit(file, header, rx, 1024, &truncated)
        });
        for _ in 0..50 {
            let _ = tx.send(vec![0xFF; 100]);
        }
        drop(tx);
        let summary = handle.join().unwrap().unwrap();

        assert!(summary.truncated, "the writer reports the cap");
        assert!(summary.frames > 0 && summary.frames < 50, "some frames, not all: {}", summary.frames);
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.len() as u64 <= 1024, "the file stays within the cap: {}", bytes.len());
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as u64, bytes.len() as u64 - 8, "RIFF size");
        let idx = bytes.len() - (8 + 16 * summary.frames as usize);
        assert_eq!(&bytes[idx..idx + 4], b"idx1", "the index is the last block");
        assert_eq!(u32::from_le_bytes(bytes[idx + 4..idx + 8].try_into().unwrap()) as u64 / 16, summary.frames, "one index entry per frame");
        std::fs::remove_dir_all(&dir).ok();
    }
}
```

- [x] **Step 2: Run it to verify it fails**

Run: `cargo test -p ofs-client dvr::tests::a_recording_stops_at_the_avi_limit -- --nocapture`
Expected: FAIL to compile — `writer_with_limit` and `summary.truncated` do not exist.

- [x] **Step 3: Implement the cap in `dvr.rs`**

Extend the existing `use std::sync::atomic::{AtomicU64, Ordering};` with `AtomicBool`, and add the constant near `DVR_FPS`:

```rust
/// The largest file AVI 1.0 can describe: the RIFF size field is `u32` and counts the bytes after it.
const MAX_AVI_FILE_BYTES: u64 = u32::MAX as u64 + 8;
```

Add `truncated: bool` to `DvrSummary`.

Split `writer` into a wrapper and a limit-aware function:

```rust
fn writer(file: File, header: Header, rx: Receiver<Vec<u8>>) -> Result<DvrSummary, String> {
    writer_with_limit(file, header, rx, MAX_AVI_FILE_BYTES, &AtomicBool::new(false))
}

fn writer_with_limit(
    mut file: File,
    header: Header,
    rx: Receiver<Vec<u8>>,
    max_bytes: u64,
    truncated: &AtomicBool,
) -> Result<DvrSummary, String> {
    // ... existing header write ...
    let mut data_end = file.stream_position().expect("the DVR position failed");
    // ... existing loop ...
    while let Ok(jpeg) = rx.recv() {
        let size = jpeg.len() as u32;
        let chunk = 8 + u64::from(size) + u64::from(size & 1);
        // The final file includes this frame plus one more 16-byte index entry; stop before any field would wrap.
        if data_end + chunk + 8 + 16 * (frames + 1) > max_bytes {
            truncated.store(true, Ordering::Relaxed);
            break;
        }
        // ... existing write, data_end += chunk, index.push(size), frames += 1 ...
    }
    // ... existing idx1 write and patches ...
    Ok(DvrSummary { frames, dropped_frames: 0, bytes: end, truncated: truncated.load(Ordering::Relaxed) })
}
```

`DvrWriter` gains `truncated: Arc<AtomicBool>`; `start` creates it, clones it into the thread, and the thread passes `&truncated_in_thread` to `writer_with_limit`. Add:

```rust
/// True once the writer hit the AVI size limit and stopped accepting frames.
pub fn truncated(&self) -> bool {
    self.truncated.load(Ordering::Relaxed)
}
```

- [x] **Step 4: Run the test to verify it passes**

Run: `cargo test -p ofs-client dvr::tests::a_recording_stops_at_the_avi_limit -- --nocapture`
Expected: PASS.

- [x] **Step 5: Expose the flag through the Rust client and the Godot binding**

In `crates/ofs-client/src/client.rs`:

```rust
/// Whether this feed's writer hit the AVI size limit (the recording is no longer growing).
pub fn dvr_truncated(&self, feed: DvrFeed) -> bool {
    lock(&self.dvr)[feed as usize].as_ref().is_some_and(|(writer, _)| writer.truncated())
}
```

In `crates/ofs-godot/src/lib.rs`, add the `dvr_truncated` method beside `dvr_stop`:

```rust
#[func]
fn dvr_truncated(&self, feed: GString) -> bool {
    let Some(client) = self.client.as_ref() else { return false };
    let Some(feed) = DvrFeed::parse(&feed.to_string()) else { return false };
    client.dvr_truncated(feed)
}
```

Add `d.set("truncated", summary.truncated);` to `dvr_stop`'s dictionary.

- [x] **Step 6: Stop the recording from GDScript and say why**

In `godot/ui/dvr.gd`, add:

```gdscript
## True once any feed's writer hit the 4 GiB AVI limit: the recording must be stopped.
func truncated() -> bool:
	for entry in _feeds:
		if _client.dvr_truncated(entry["feed"]):
			return true
	return false
```

In `stop()`, collect the flag across feeds:

```gdscript
var truncated := false
for i in _feeds.size():
	var one: Dictionary = _client.dvr_stop(_feeds[i]["feed"])
	truncated = truncated or bool(one.get("truncated", false))
	# ... existing failed/dropped/summary lines ...
summary["truncated"] = truncated
```

In `godot/scripts/app.gd`, add a testable poll helper and call it from `_process` right after `_poll_dvr_exports()`:

```gdscript
func _poll_dvr_limit() -> void:
	if dvr.recording and dvr.truncated():
		_stop_dvr(false)
		_sync_dvr()
```

In `_stop_dvr`, append the reason to both toasts:

```gdscript
var limit := " (stopped at the 4 GiB AVI limit)" if summary.get("truncated", false) else ""
```

Append `limit` as a third `%s` in both toast format strings.

- [x] **Step 7: Extend the Godot mock and write the failing auto-stop test**

In `godot/tests/test_dvr.gd`, add to `MockClient`:

```gdscript
var truncated := {}  # feed -> bool

func dvr_truncated(feed: String) -> bool:
	return truncated.get(feed, false)
```

Return `"truncated": truncated.get(feed, false)` from `dvr_stop`.

Add the test:

```gdscript
func test_a_truncated_recording_stops_itself_and_says_why() -> void:
	var s := await _app_shell()
	var app = s["app"]
	var client = s["client"]
	s["menu"]._dvr.button_pressed = true
	_feed_field(s["dvr"], 1)
	client.truncated["goggles"] = true
	app._poll_dvr_limit()
	ok(not s["dvr"].recording, "the cap stops the recording")
	ok(not s["menu"].dvr_on(), "the checkbox follows")
	ok(not s["hud"]._rec.visible, "REC hides")
	ok("4 GiB" in " ".join(s["hud"].toast_texts()), "the toast says why: %s" % str(s["hud"].toast_texts()))
	app.free()
	s["menu"].queue_free()
	s["hud"].queue_free()
	s["dvr"].queue_free()
```

- [x] **Step 8: Run the Godot unit suite**

Run: `GODOT_BIN=build/godot-dl/Godot_v4.7.2-stable_win64_console.exe bash scripts/run-godot-tests.sh unit`
Expected: PASS, including the new test.

- [x] **Step 9: Commit**

```bash
git add crates/ofs-client/src/dvr.rs crates/ofs-client/src/client.rs crates/ofs-godot/src/lib.rs godot/ui/dvr.gd godot/scripts/app.gd godot/tests/test_dvr.gd
git commit -m "fix(dvr): stop at the AVI 1.0 size limit and report truncation"
```

---

### Task 2: Reject an inputs `t_s` past the run limit (F2)

**Files:**
- Modify: `crates/ofs-sim/src/inputs.rs`
- Modify: `crates/ofs-sim/src/vehicle.rs:1486`
- Test: `crates/ofs-sim/src/inputs.rs`, `crates/ofs-sim/tests/grpc.rs`

**Interfaces:**
- Consumes: `ofs_core::scheduler::MAX_RUN_FOR_S`
- Produces: `inputs::parse` refuses `t_s > MAX_RUN_FOR_S` with a message containing `past the 86400 s limit`

- [x] **Step 1: Write the failing unit test**

In `crates/ofs-sim/src/inputs.rs`'s test module:

```rust
#[test]
fn a_time_past_the_simulators_run_limit_is_refused() {
    let ok = parse("long.csv", "t_s,yaw\n86400,0\n").unwrap();
    assert_eq!(ok.duration_s(), 86_400.0, "the limit itself is allowed");
    let err = parse("long.csv", "t_s,yaw\n86401,0\n").unwrap_err();
    assert!(err.contains("past the 86400 s limit"), "{err}");
}
```

- [x] **Step 2: Run it to verify it fails**

Run: `cargo test -p ofs-sim inputs::tests::a_time_past_the_simulators_run_limit_is_refused`
Expected: FAIL — `parse` accepts 86401.

- [x] **Step 3: Implement the cap**

At the top of `crates/ofs-sim/src/inputs.rs`:

```rust
use ofs_core::scheduler::MAX_RUN_FOR_S;
```

In `parse`, after the negative check:

```rust
if t > MAX_RUN_FOR_S {
    problems.push(format!("{name} line {line}: t_s {t} is past the {MAX_RUN_FOR_S} s limit"));
}
```

In `crates/ofs-sim/src/vehicle.rs:1486`, make the addition saturating as defense in depth:

```rust
let ticks = inputs.rows.iter().map(|(t, _)| now.saturating_add(tick_at(*t, base_hz))).collect();
```

- [x] **Step 4: Run the unit test to verify it passes**

Run: `cargo test -p ofs-sim inputs::tests::a_time_past_the_simulators_run_limit_is_refused`
Expected: PASS.

- [x] **Step 5: Write the failing gRPC test**

Add beside the other `play_inputs` tests in `crates/ofs-sim/tests/grpc.rs`:

```rust
#[tokio::test]
async fn an_inputs_time_past_the_run_limit_is_refused_not_wrapped() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let err = c
        .play_inputs(InputsFile { name: "long.csv".into(), csv: "t_s,throttle\n86401,1\n".into() })
        .await
        .unwrap_err();
    assert_eq!(kind(&err), "config");
    assert!(err.message().contains("past the 86400 s limit"), "{}", err.message());
}
```

- [x] **Step 6: Run it to verify it passes**

Run: `cargo test -p ofs-sim --test grpc an_inputs_time_past_the_run_limit_is_refused_not_wrapped`
Expected: PASS.

- [x] **Step 7: Commit**

```bash
git add crates/ofs-sim/src/inputs.rs crates/ofs-sim/src/vehicle.rs crates/ofs-sim/tests/grpc.rs
git commit -m "fix(inputs): refuse t_s past the simulator's run limit"
```

---

### Task 3: Validate DVR start arguments and clean up a failed start (F7)

**Files:**
- Modify: `crates/ofs-client/src/dvr.rs`
- Test: `crates/ofs-client/src/dvr.rs`

**Interfaces:**
- Produces: `DvrWriter::start` returns `Err` and creates no file for `fps = 0`, `width = 0`, `height = 0`, or a dimension above `u16::MAX`.

- [x] **Step 1: Write the failing test**

Add to the `dvr.rs` test module:

```rust
#[test]
fn a_start_with_a_bad_size_or_rate_creates_no_file() {
    let dir = std::env::temp_dir().join(format!("ofs-dvr-start-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (fps, width, height) in [(50, 0, 2), (50, 4, 0), (0, 4, 2), (50, 65_536, 2), (50, 4, 65_536)] {
        let path = dir.join(format!("bad-{fps}-{width}-{height}.avi"));
        let _ = std::fs::remove_file(&path);
        assert!(DvrWriter::start(path.clone(), fps, width, height).is_err(), "{fps} {width} {height}");
        assert!(!path.exists(), "no file for {fps} {width} {height}");
    }
    std::fs::remove_dir_all(&dir).ok();
}
```

- [x] **Step 2: Run it to verify it fails**

Run: `cargo test -p ofs-client dvr::tests::a_start_with_a_bad_size_or_rate_creates_no_file`
Expected: FAIL — the 0×0 and oversize starts succeed and create files.

- [x] **Step 3: Implement the validation and cleanup**

At the top of `DvrWriter::start`, before `create_dir_all`:

```rust
if fps == 0 {
    return Err("fps must be greater than 0".into());
}
if width == 0 || height == 0 {
    return Err("width and height must be greater than 0".into());
}
if width > u32::from(u16::MAX) || height > u32::from(u16::MAX) {
    return Err(format!("width and height must be at most {} (the AVI frame rectangle is 16-bit)", u16::MAX));
}
if u64::from(width) * u64::from(height) * 3 > u64::from(u32::MAX) {
    return Err(format!("{width}x{height} is too large for a 32-bit AVI frame size"));
}
```

On the thread spawn, remove the created file before returning the error:

```rust
.map_err(|e| {
    let _ = std::fs::remove_file(&path);
    e.to_string()
})?;
```

- [x] **Step 4: Run the test to verify it passes**

Run: `cargo test -p ofs-client dvr::tests::a_start_with_a_bad_size_or_rate_creates_no_file`
Expected: PASS.

- [x] **Step 5: Commit**

```bash
git add crates/ofs-client/src/dvr.rs
git commit -m "fix(dvr): validate start arguments and remove a file when the writer fails to spawn"
```

---

### Task 4: Correct the README's quad count and list (F3)

**Files:**
- Modify: `README.md:198-211`

**Interfaces:** none.

- [x] **Step 1: Edit the list and the counts**

Add `quads/opendrone-5f-ibus.toml` to the file list at `README.md:198-199`.

Change the paragraph at line 204:

- "Eight quads ship." → "Nine quads ship."
- "The five 5\" freestyle ones" → "The six 5\" freestyle ones"
- "(and four variants)" → "(and five variants)"

- [x] **Step 2: Verify the numbers against the directory**

Run:

```bash
ls quads/*.toml | wc -l
grep -n "Nine quads\|six 5\|five variants\|opendrone-5f-ibus" README.md
```

Expected: `9`; the grep shows the new text and the ibus file.

- [x] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: list all nine shipped quads"
```

---

### Task 5: List a relative `data_dir` in the Godot catalog (F4)

**Files:**
- Modify: `godot/scripts/catalog.gd:14-26`
- Test: `godot/tests/test_catalog.gd`

**Interfaces:**
- Produces: `Catalog.list` accepts absolute, `user://` and plain relative roots.

- [x] **Step 1: Write the failing test**

Add to `godot/tests/test_catalog.gd`:

```gdscript
func test_a_relative_root_lists_its_files() -> void:
	var rel := "build/catalog-relative-test"
	var base := DirAccess.open(".")
	ok(base != null, "the working directory opens")
	base.make_dir_recursive(rel + "/worlds")
	var file := FileAccess.open(rel + "/worlds/rel.toml", FileAccess.WRITE)
	file.store_string("name = \"Relative\"\n")
	file.close()
	var items := Catalog.list([rel + "/worlds"])
	eq(items.size(), 1, "a relative root lists its file")
	if items.size() == 1:
		eq(items[0]["label"], "Relative", "with its label")
```

`build/` is already a scratch directory; the test leaves the file there and rewrites it on the next run.

- [x] **Step 2: Run it to verify it fails**

Run: `GODOT_BIN=build/godot-dl/Godot_v4.7.2-stable_win64_console.exe bash scripts/run-godot-tests.sh unit`
Expected: FAIL — `Catalog.list` returns an empty array for the relative root.

- [x] **Step 3: Use `DirAccess.open`**

Replace the static calls in `Catalog.list`:

```gdscript
for dir in dirs:
	var access := DirAccess.open(dir)
	if access == null:
		continue
	for file in access.get_files():
		# ... existing body ...
```

`DirAccess.open` returns `null` for a missing directory and accepts absolute, `user://` and relative paths.

- [x] **Step 4: Run the Godot unit suite**

Run: `GODOT_BIN=build/godot-dl/Godot_v4.7.2-stable_win64_console.exe bash scripts/run-godot-tests.sh unit`
Expected: PASS, including the existing catalog tests.

- [x] **Step 5: Commit**

```bash
git add godot/scripts/catalog.gd godot/tests/test_catalog.gd
git commit -m "fix(godot): list quads and worlds from a relative data dir"
```

---

### Task 6: Make the schema-4 error name the current range (F5)

**Files:**
- Modify: `crates/ofs-config/src/lib.rs:769`
- Test: `crates/ofs-config/tests/config.rs:160-169`

**Interfaces:** none.

- [x] **Step 1: Write the failing assertion**

In `a_schema_4_quad_file_is_refused_with_the_migration_message`, add:

```rust
assert!(
    message.contains(&format!("reads schemas {MIN_SCHEMA_VERSION} to {SCHEMA_VERSION}")),
    "{message}"
);
```

Import the constants at the top of the test file if they are not already in scope:

```rust
use ofs_config::{MIN_SCHEMA_VERSION, SCHEMA_VERSION};
```

- [x] **Step 2: Run it to verify it fails**

Run: `cargo test -p ofs-config --test config a_schema_4_quad_file_is_refused_with_the_migration_message`
Expected: FAIL — the message says "reads schemas 5 and 6".

- [x] **Step 3: Use the constants in the message**

In `crates/ofs-config/src/lib.rs:769`:

```rust
let message = format!(
    "unsupported schema_version 4 (this build reads schemas {MIN_SCHEMA_VERSION} to {SCHEMA_VERSION}); schema 5 moves the [frame], \
     [ground], [motor], [prop], [esc], [battery] and [vtx] hardware sections into library/ part \
     files with per-parameter provenance and the quad references them by id — see the \
     \"Component library\" section of README.md"
);
```

- [x] **Step 4: Run the test to verify it passes**

Run: `cargo test -p ofs-config --test config a_schema_4_quad_file_is_refused_with_the_migration_message`
Expected: PASS.

- [x] **Step 5: Commit**

```bash
git add crates/ofs-config/src/lib.rs crates/ofs-config/tests/config.rs
git commit -m "fix(config): name the current schema range in the schema-4 error"
```

---

### Task 7: Make the `ofs-proto` description version-neutral (F6)

**Files:**
- Modify: `crates/ofs-proto/Cargo.toml:7`

**Interfaces:** none.

- [x] **Step 1: Replace the description**

```toml
description = "The simulator's gRPC protocol messages and stubs shared by the server and its Rust clients"
```

- [x] **Step 2: Verify the crate still checks**

Run: `cargo check -p ofs-proto --locked`
Expected: PASS.

- [x] **Step 3: Commit**

```bash
git add crates/ofs-proto/Cargo.toml
git commit -m "docs(proto): describe the crate without a protocol number"
```

---

### Task 8: Let the Godot test script download Godot on Windows (F9, nice to have)

**Files:**
- Modify: `scripts/run-godot-tests.sh:20-31`

**Interfaces:** none.

- [x] **Step 1: Add the Windows case**

In the `case "$(uname -s)" in` block:

```bash
MINGW*|MSYS*|CYGWIN*)
    GODOT_ASSET="Godot_v${GODOT_VERSION}-stable_win64.exe.zip"
    GODOT_SHA512="83decd58fdf67b9d657958a1ae6bf1929c20785315a81effe245874cdc57acb709bf868e00778a96984338c1b29dafdb453c6847747694621c6ecf5da2259993"
    GODOT_REL_BIN="Godot_v${GODOT_VERSION}-stable_win64_console.exe"
    ;;
```

The SHA-512 is the one verified against the 4.7.2 release's `SHA512-SUMS.txt` on 2026-11-17.

- [x] **Step 2: Check the script syntax**

Run: `bash -n scripts/run-godot-tests.sh`
Expected: exit 0.

- [x] **Step 3: Run the unit suite with no `GODOT_BIN`**

Run: `bash scripts/run-godot-tests.sh unit`
Expected: the script finds `build/godot-dl/Godot_v4.7.2-stable_win64_console.exe` (already downloaded), prints `Godot: …`, builds, and passes the unit suites.

- [x] **Step 4: Commit**

```bash
git add scripts/run-godot-tests.sh
git commit -m "test(godot): download the Windows editor on Windows"
```

---

### Task 9: Resolve the recording spec once (F8, nice to have)

**Files:**
- Modify: `crates/ofs-log/src/recorder.rs:204-213`
- Test: `crates/ofs-log/tests/recorder.rs:90-105` (existing regression check)

**Interfaces:** none.

- [x] **Step 1: Run the existing regression test first**

Run: `cargo test -p ofs-log --test recorder start_and_stop_refuse_the_wrong_state_and_an_existing_file`
Expected: PASS. It asserts that a refused request creates no file.

- [x] **Step 2: Remove the duplicate resolve**

In `RecorderHandle::start`, delete the pre-create `resolve` line and the `signals` binding used only by it:

```rust
// Validate before creating the file: a refused request leaves nothing behind.
let signals = recorded_signals(bus, requests);
resolve(requests, &signals, base_hz).map_err(RecordError::Spec)?;
```

Keep the file creation, then remove the file when `start_with` refuses:

```rust
let file = File::create_new(path).map_err(|e| RecordError::from_create(path, &e))?;
match self.start_with(bus, base_hz, requests, Box::new(BufWriter::new(file)), path) {
    Ok(names) => Ok(names),
    Err(e) => {
        let _ = std::fs::remove_file(path);
        Err(e)
    }
}
```

`start_with` still resolves once, so a bad spec now creates the file, fails, and removes it. The existing test pins that outcome.

- [x] **Step 3: Run the regression test again**

Run: `cargo test -p ofs-log --test recorder start_and_stop_refuse_the_wrong_state_and_an_existing_file`
Expected: PASS.

- [x] **Step 4: Commit**

```bash
git add crates/ofs-log/src/recorder.rs
git commit -m "refactor(log): resolve the recording spec once"
```

---

### Task 10: Full verification

**Files:** none.

- [x] **Step 1: Format and lint**

Run:

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Expected: exit 0, no warnings.

- [x] **Step 2: Rust tests**

Run:

```bash
cargo test --workspace --locked
```

Expected: all tests pass. Live SITL tests are skipped without `OFS_SITL_LAUNCH`.

- [x] **Step 3: Build the server and the Godot extension**

Run:

```bash
cargo build -p ofs-sim -p ofs-godot --locked
```

Expected: exit 0.

- [x] **Step 4: Godot unit and open-loop suites**

Run:

```bash
GODOT_BIN=build/godot-dl/Godot_v4.7.2-stable_win64_console.exe bash scripts/run-godot-tests.sh all
```

Expected: the unit, open-loop, errors and terrain suites pass; the Betaflight suite is skipped without `OFS_SITL_LAUNCH`.

- [x] **Step 5: Python suites**

Run:

```bash
.venv/Scripts/python.exe -m pytest python/tests -v
.venv/Scripts/python.exe -m pytest tools/sitegen/tests -q
```

Expected: all tests pass (live SITL tests skip).

- [x] **Step 6: Commit any leftover generated files**

Run `git status`. If the Godot import or the tests wrote tracked files, restore them; do not commit `build/` or `godot/.godot/`.

---

## Self-Review

**Spec coverage:** F1 → Task 1; F2 → Task 2; F3 → Task 4; F4 → Task 5; F5 → Task 6; F6 → Task 7; F7 → Task 3; F8 → Task 9; F9 → Task 8. The two "not planned" findings are explicitly out of scope with reasons.

**Step scan:** Every step has one action and a checkable result. The test steps carry the assertions; the implementation steps carry the signatures and the exact values.

**Type consistency:** `DvrSummary.truncated` is added once and used by `Client::dvr_truncated`, the Godot dictionary, `Dvr.truncated()` and the toasts. `MAX_AVI_FILE_BYTES` is `u64`; all projections use `u64`. `MAX_RUN_FOR_S` is `f64`; `parse` compares `f64` to `f64`.

**Review Focus:** Each of the five lines has a test in the owning task: Task 1 (4 GiB cap), Task 2 (`t_s`), Task 3 (start args), Task 5 (relative data dir), Task 6 (schema range).

**Proportion:** The plan is longer than the findings because each task carries its test and its exact values; no task transcribes a function body the signature and test already determine.
