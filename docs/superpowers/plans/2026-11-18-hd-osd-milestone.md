# HD/digital OSD element set (#32) — milestone plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the HD/digital half of #32 — an HD SITL build, one HD quad with the `OSD_SYS_*` positions and the 53×20 canvas, the `MSP_DP_SYS` decoder, and the Godot SYS cells — without changing the wire or the SD quads.

**Architecture:** Keep `USE_OSD_SD` and add `USE_OSD_HD` to the generated SITL `target.h`; an HD quad diff selects `vcd_video_system = HD` and the SYS positions. The DisplayPort decoder stores subcommand 6 in spare bits of the existing packed `Cell`; Godot draws the markers with the simulator's values where they exist and `--` where they do not. The existing SD quads stay PAL 30×16.

**Tech Stack:** Rust (`ofs-video`, `ofs-sim`, `ofs-config`), the Betaflight SITL patch/build, Godot 4.7.2, the Python client.

**Spec:** `docs/superpowers/specs/2026-11-18-hd-osd-milestone-design.md`

## Global Constraints

- Pinned SITL: `BF_TAG=2026.6.2`, `BF_SHA=e0b7bb01b17b21351057e9ead2d1ab39dd44fa16`; `USE_OSD_SD` lives in
  `third_party/betaflight/tools/add_serial_out_datagram.py` and the committed `ofs-sitl.patch`. Add `USE_OSD_HD` to
  both; keep SD; rebuild with `scripts/build-sitl.sh`.
- The six shipped SD quads keep `vcd_video_system = PAL`, 30×16, and their OSD tests unchanged. The HD quad is new.
- The pinned SITL's HD canvas is **53×20**, not the issue's 50×18. Do not add `MSP_SET_OSD_CANVAS` or
  `OSD_CANVAS_SIZE_DEBUG`.
- No protocol bump if the SYS id uses spare bits in the packed `uint32`; update the Python `Osd` accessor and the
  proto comment.
- No new glyph art: the SYS markers are not glyphs; use the analog atlas plus the severity tints and Godot's fallback
  font. The `digital.mcm` face is a separate visual decision; vendor fonts are unlicensed.
- The SYS values that do not exist in the simulator (goggle voltage, VTX voltage, bitrate, VTX DVR, VTX temperature,
  fan speed) are `--`, documented; do not invent models for them in this milestone.
- Gates: `cargo test --workspace --locked`, workspace clippy, the Python tests, the Godot unit and e2e suites, and a
  live SITL run with the HD quad.

## Review Focus

- An SD quad still draws 30×16 and its OSD tests are byte-for-byte unchanged.
- A SYS cell outside the grid, with an unknown system id, or with no value is clipped/blanked/`--`, never a crash.
- The 53×20 canvas reaches the client; a 50×18 assumption does not creep back in.
- The SYS positions with a column at 32 or above set Betaflight's extra X bit (1024).
- No new font art enters the atlas; the licence note stays true.

---

### Task 1: The spike — prove the FC-sent SYS path

**Files:**
- Modify (spike): `third_party/betaflight/tools/add_serial_out_datagram.py`, regenerate
  `third_party/betaflight/ofs-sitl.patch`
- Temporary: `quads/hd-spike.toml` and `quads/hd-spike.betaflight.diff` (removed after the spike)
- Create: `docs/research/hd-osd-spike.md`

**Interfaces:**
- Produces: the measured `MSP_DP_SYS` bytes, the canvas size, the SYS positions, and the font/licence decision that
  Tasks 2–5 rely on. No product API.

- [ ] **Step 1: Add `USE_OSD_HD`.** In the generator's `target.h` hunk add `#define USE_OSD_HD` beside
  `#define USE_OSD_SD`; regenerate the patch with the generator's documented command and verify both defines are in
  `ofs-sitl.patch`.
- [ ] **Step 2: Rebuild the SITL.** Run `scripts/build-sitl.sh`; confirm the build still defines `USE_OSD_SD` and adds
  `USE_OSD_HD` (grep the generated `target.h` in the build tree).
- [ ] **Step 3: Make the temporary HD quad.** Copy the 5F diff; set `set vcd_video_system = HD` and
  `set osd_displayport_device = MSP`; add the eleven `osd_sys_*_pos` values using `2048 + (row << 5) + col` (plus 1024
  when the column is 32 or more); set `[osd] cols = 53, rows = 20` in the temporary quad TOML.
- [ ] **Step 4: Run a live session.** Start `ofs-sim` with the temporary quad and a real SITL; capture UART4's
  DisplayPort bytes (the existing serial tap); confirm the grid is 53×20 and the frame stream contains subcommand 6
  `[6, row, col, systemElement]` for the visible SYS markers. Decode one marker by hand.
- [ ] **Step 5: Settle the font/licence.** Decide whether the SYS labels/values can use Godot's fallback font (the
  expectation) or whether the one-page `resources/osd/2/digital.mcm` face is needed; record the licence finding.
- [ ] **Step 6: Write the spike report** in `docs/research/hd-osd-spike.md`: the build/defines, the measured canvas,
  the positions, the raw frame shape, the decoded marker, the font/licence, and the go/no-go for Tasks 2–5. Remove the
  temporary quad/diff.
- [ ] **Step 7: Commit** `docs(research): the HD OSD spike (#32)`.

**Stop if:** the SITL does not emit subcommand 6 or a 53×20 grid with the expected positions; re-plan before writing
product code.

---

### Task 2: Decode `MSP_DP_SYS` into the packed cell

**Files:**
- Modify: `crates/ofs-video/src/osd.rs` (`SUB_SYS`, `Cell`, packing, `write`)
- Modify: `crates/ofs-sim/src/session.rs` (cell packing), `proto/ofs/v1/sim.proto` (the comment), `python/ofs/client.py`
  (the `Osd` accessor)
- Test: `crates/ofs-video/tests/osd.rs`, `python/tests/test_client.py` (or the Osd test file)

**Interfaces:**
- Produces: `Cell.sys: u8` (0 = none, 1..=11 = the `displayPortSystemElement_e` value + 1), packed at bits 11–15 of the
  existing cell `uint32`; `text_rows` stays spaces for a sys cell; the Python `Osd.sys(cell)` accessor.

- [ ] **Step 1: Write the failing Rust tests:** a `dp(6, row, col, sys)` frame sets `Cell.sys`; a sys cell does not
  contribute text; an out-of-grid sys cell is clipped; an unknown subcommand is still counted; the packed round trip
  keeps `sys`/`page`/`blink`; the existing `dp(6)` "ignored" test changes to the new behavior.
- [ ] **Step 2: Run** `cargo test -p ofs-video --test osd` — expected FAIL.
- [ ] **Step 3: Implement** `SUB_SYS = 6`; `Cell.sys`; the pack/unpack at bits 11–15; `write` parses
  `[6, row, col, systemElement]` (a short frame is ignored and counted); `text_rows` spaces the sys cell; update the
  proto comment and the Python accessor.
- [ ] **Step 4: Run** the Rust test, `cargo test --workspace --locked`, and the Python Osd test — expected PASS.
- [ ] **Step 5: Commit** `feat(video): decode the MSP DisplayPort SYS cells (#32)`.

---

### Task 3: Draw the SYS cells in Godot

**Files:**
- Modify: `godot/ui/osd.gd` (the cell read, `_is_drawn`, `_draw_grid`, an environment setter)
- Modify: `godot/scripts/app.gd` (pass `latency_ms`, distance, LQ, the DVR state)
- Test: `godot/tests/test_osd.gd`

**Interfaces:**
- Consumes: the packed `sys` bits and the environment dictionary
  `{latency_ms, pilot: Vector3, position: Vector3, lq_pct: float, dvr: bool}`.
- Produces: `Osd.set_environment(values: Dictionary)`; each sys cell draws its label and value (or `--`) at its cell;
  the 53×20 grid draws without clipping; a sys cell outside the grid is ignored.

- [ ] **Step 1: Write the failing tests:** a sys cell is counted/drawn (not text); the delay/distance/LQ values render
  from the environment; a missing value renders `--`; a 53×20 grid's last cell is drawable; the malformed-frame test
  still passes.
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` — expected FAIL.
- [ ] **Step 3: Implement** the sys branch (the label/value via the fallback font), the environment setter, and the
  app wiring; keep the existing page/tint logic untouched.
- [ ] **Step 4: Run** the unit suite — expected PASS.
- [ ] **Step 5: Commit** `feat(godot): draw the HD OSD SYS cells (#32)`.

---

### Task 4: The shipped HD quad and diff

**Files:**
- Create: `quads/opendrone-5f-hd.toml`, `quads/opendrone-5f-hd.betaflight.diff`
- Modify: `docs/research/sitl-interface.md` (the HD build), `docs/dev-setup.md` (the HD quad), the quad/catalog tests
- Test: `crates/ofs-config/tests/quads.rs` (or the existing quad tests), `godot/tests/e2e_betaflight.gd` (a new HD
  block)

**Interfaces:**
- Consumes: Tasks 1–3' build/decoder/renderer.
- Produces: an HD quad with the HD diff, the eleven SYS positions, `[osd] cols = 53, rows = 20`; the six SD quads and
  their tests unchanged.

- [ ] **Step 1: Write the failing tests:** the HD quad loads with 53×20 and the HD diff; the SD quads still load at
  30×16; the diff carries `vcd_video_system = HD`, the UART, and the SYS positions; the e2e block waits for a 53×20
  frame with a sys cell.
- [ ] **Step 2: Run** the config tests and the Godot unit — expected FAIL.
- [ ] **Step 3: Create the HD quad/diff** from the spike's measured values; place the readable SYS markers (the
  spike's decision); set the positions with the column-X bit; update the docs/catalog.
- [ ] **Step 4: Run** the config tests, the Godot unit, and the live e2e with the real SITL — expected PASS (the SD
  e2e unchanged).
- [ ] **Step 5: Commit** `feat(quads): the HD OSD quad and diff (#32)`.

---

### Task 5: The e2e, docs, licence and gates

**Files:**
- Modify: `docs/research/sitl-interface.md`, `docs/dev-setup.md`, `docs/roadmap.md`, `CHANGELOG.md`,
  `godot/ui/OSD_FONT_LICENSE.md` (the no-new-art note)
- Test: the live SITL HD e2e; the Godot `all`; the full Rust/Python gates

- [ ] **Step 1: Write the HD e2e checks** (53×20, the sys cells, an SD quad still 30×16) and the manual checklist in
  `docs/human-check.md`.
- [ ] **Step 2: Update the docs** with the HD build, the quad, the decoder, the SYS values (`--` set), the font/licence,
  and the roadmap line; state the 53×20 correction to the issue.
- [ ] **Step 3: Run the gates:** `cargo test --workspace --locked`, workspace clippy, the Python tests, the Godot unit
  and `all`, and the live SITL e2e with the HD quad.
- [ ] **Step 4: Commit** `docs: the HD OSD build, quad and values (#32)`.

## Manual QA (the owner)

- Fly the HD quad with the real SITL: the goggles' OSD shows the SYS markers at their placed cells; the delay/distance/
  LQ values track; the `--` markers stay `--`; the analog noise/breakup still owns the picture.
- An SD quad still shows its 30×16 OSD exactly as before.
- Check the SYS labels/values by eye at 53×20; the fallback font is the visual decision.
