# HD/digital OSD element set (#32) — milestone design

## Intent

Finish the HD/digital half of #32. The SD element set and the severity pages are done; the HD `OSD_SYS_*` markers never
reach the client because the SITL is built with `USE_OSD_SD` only and the DisplayPort decoder drops `MSP_DP_SYS`
(subcommand 6). The faithful route is FC-sent: an HD SITL build, a quad diff with the HD video system and the SYS
positions, the decoder, and a client that draws the markers with the values the simulator has and `--` for the rest.
The wire does not grow.

## Goals

- A SITL build with both `USE_OSD_SD` and `USE_OSD_HD`, so the SD quads stay PAL 30×16 and an HD quad uses 53×20.
- One shipped HD quad/diff: `vcd_video_system = HD`, the DisplayPort UART, the eleven `osd_sys_*_pos`, and
  `[osd] cols = 53, rows = 20`.
- The decoder stores `[6, row, col, systemElement]` in the existing packed `Cell` (spare bits; no protocol bump), with
  the Python accessor.
- The Godot OSD draws the SYS cells: delay (`FpvView.latency_ms`), distance (pose/pilot), LQ (radio telemetry) and the
  goggle DVR state (`dvr.gd`); goggle/VTX voltage, bitrate, VTX DVR, VTX temperature and fan speed stay `--` until a
  model exists.
- Tests: Rust for the sys decode and packing, Godot for the sys cells and the `--`, the HD quad's 53×20, and an e2e
  against the HD quad; the existing SD tests and quads are unchanged.
- Docs: the roadmap line, the SITL/OSD research, the dev setup, the licence note, the changelog.

## Non-goals

- New goggle, VTX, digital-link bitrate or thermal models; the missing SYS values are `--` and documented.
- A four-page goggle font: the only checked source is the GPL-3.0 Configurator `digital.mcm` (one page); the vendor
  fonts are unlicensed. No new glyph art.
- `MSP_DP_OPTIONS` (unused by Betaflight) and `MSP_SET_OSD_CANVAS` (a goggle handshake the sim does not send); the
  pinned SITL's HD canvas is 53×20, not the issue's 50×18.
- Digital video systems (#33), the analog chain, new worlds.

## Constraints

- Pinned SITL: `BF_TAG=2026.6.2`, `BF_SHA=e0b7bb01b17b21351057e9ead2d1ab39dd44fa16`. `USE_OSD_SD` lives only in the
  generated `target.h` patch (`ofs-sitl.patch` and `tools/add_serial_out_datagram.py`); add `USE_OSD_HD` in both and
  keep SD.
- The six shipped SD quads set `vcd_video_system = PAL` (30×16) and must keep their OSD tests byte-for-byte.
- No protocol version bump if the SYS id uses spare bits in the packed `uint32`; the Python `Osd` helper gains the
  accessor.
- The font stays the analog atlas plus the severity tints; the SYS markers are not glyphs and use Godot's fallback font.
- The HD canvas is 53×20 (the pinned build); the issue's 50×18 is a goggle-announced size, not what this SITL emits.

## Approaches

- **Faithful, FC-sent (chosen)** vs client-only synthesis: the latter cannot match Betaflight's OSD or the quad diff.
- **Spare bits in the packed cell (chosen)** vs a new repeated proto field: no schema/wire change for eleven markers.
- **53×20 (chosen)** vs 50×18: forcing the latter needs `OSD_CANVAS_SIZE_DEBUG` or a canvas handshake.
- **A new HD quad (chosen)** vs converting a shipped SD quad: the SD defaults and tests stay untouched.
- **Fallback font first (chosen)** vs a licensed HD face now: a visual decision for later; no blocker to drawing the
  markers.

## The spike

The first work is a throwaway spike: add `USE_OSD_HD`, rebuild the SITL, add a temporary HD quad/diff, run one live
session, capture the DisplayPort bytes, confirm subcommand 6 and the 53×20 grid, decode one system element, and settle
the font/licence. Its report (`docs/research/hd-osd-spike.md`) decides the later tasks. If the FC does not emit the
expected bytes, stop and re-plan before writing the product code.

## Decisions for the builder

- Which SYS markers the shipped HD quad places (all eleven or the readable subset); the positions follow Betaflight's
  `osd_sys_*_pos` encoding (`2048 + (row << 5) + col`, plus 1024 when the column is 32 or more).
- The layout and typography of each SYS cell (fallback font; label plus value); the minimap/waypoint elements are out.
- The e2e's world and quad; the lockstep capture; the by-eye frame check.

## What is left to the builder

The spike's measured canvas/positions/bytes; the value sources that do not exist yet (the `--` set); the HD font
decision; the manual visual check through the analog chain.
