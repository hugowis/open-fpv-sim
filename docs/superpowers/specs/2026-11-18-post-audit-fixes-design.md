# Post-audit fixes: async parsing, OpenDML DVR, pack current

## Intent

Three fix-class issues remain after the 2026-11-17 audit: `Load` and `PlayInputs` parse on a tokio worker
(#185), the DVR stops at AVI 1.0's 4 GiB (#186), and the 5F reports exactly 4× its ESC current limit at full
throttle (#164). This batch closes them in one branch and one PR, one commit per issue.

## Goals

- **#185**: no config, world or CSV parse runs on an async runtime thread; the session mutex is not held during
  the parse; a large inputs file still lets another RPC be served.
- **#186**: a recording past 4 GiB plays and exports. The first segment stays `RIFF/AVI ` with its `idx1`; later
  segments are `RIFF/AVIX`; `dmlh` carries the total frame count; the export reads every segment.
- **#164**: the pack current at full throttle is explained, and the startup clamp and the steady draw are both
  pinned by a test. If the clamp is binding at steady state, stop before changing the electrical model.

## Non-goals

- No protocol, wire or Godot–Rust API change.
- No new codec, resolution, frame rate or audio behaviour.
- No new electrical or propeller model: #164 changes the clamp's role only.

## Constraints

- A physics change must not alter seeded runs silently: #164's expected outcome is a ruling (no change); any
  actual model change regenerates the affected goldens in the same commit.
- AVI 1.0 files written by earlier versions still open; the export reader still reads an unfinalized file.
- Branch base is `main` at `2379a57`. `photoreal-chernobyl` also touches `server.rs`; rebase if it lands first.
- Gates: `cargo test --workspace --locked` and
  `cargo clippy --workspace --all-targets --locked -- -D warnings`; the Godot unit suite for the toast wording.

## Approaches

- **#185**: move each parse into a separate `tokio::task::spawn_blocking`, await it, then keep the existing
  `self.blocking` for the session work — chosen. Moving the whole handler into `self.blocking` was rejected
  because it holds the session mutex through the parse; leaving it was the audit finding.
- **#186**: OpenDML multi-segment AVI with a per-segment `idx1` and a `dmlh` total — chosen. Splitting at 4 GiB
  into numbered files was rejected because it changes the user-facing recording, not the container; raising the
  cap is impossible with AVI 1.0's 32-bit fields.
- **#164**: diagnose with an end-to-end test, then either remove the phase clamp if it binds at steady state, or
  keep it as the ESC's startup protection and write the ruling — chosen. Clamping only the reported telemetry
  was rejected because it hides the model; changing the ESC part was rejected because it is not the issue.

## Decisions

- **Segment size**: the AVI 1.0 maximum (`u32::MAX + 8` bytes); the total cap is 64 GiB, still reported by
  `truncated`.
- **Index**: one `idx1` per segment, no `indx`/`ix00`. VLC and ffmpeg read segments; if the manual check fails,
  add the standard index rather than abandoning the format.
- **Header**: a `dmlh` chunk is written from the first segment (AVI 1.0 readers ignore it). The first segment's
  `avih` and `strh` lengths count that segment's frames; `dmlh` counts all of them.
- **#164 expected outcome**: the 180 A is four motors at the 45 A startup clamp; the steady draw is the
  electrical model's. The test decides. If the steady state is clamped, stop and report instead of changing the
  model in this batch.

## What is left to the builder

- The 5F's steady draw is computed from the state and the motor constants in the test, not hardcoded.
- The >4 GiB playback in VLC/ffmpeg is the owner's manual check (a 30+ minute recording); the automated tests
  check the segment structure, the `dmlh` total and the export reader.
