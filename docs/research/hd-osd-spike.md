# HD OSD spike (issue #32): the FC-sent SYS path

Findings of the milestone spike (2026-11-18). Plan: `docs/superpowers/plans/2026-11-18-hd-osd-milestone.md` (Task 1).
Design: `docs/superpowers/specs/2026-11-18-hd-osd-milestone-design.md`.

Question: does the pinned Betaflight SITL, built with `USE_OSD_HD`, emit the HD SYS markers (`MSP_DP_SYS`, DisplayPort
subcommand 6) on a 53×20 canvas, and what does the client need for the font?

**Verdict: GO.** The SITL emits subcommand 6 with the configured positions on a 53×20 canvas, and the values are not on
the wire (the FC sends markers, the goggles render them), so the client needs no new glyph art.

## Build

`third_party/betaflight/tools/add_serial_out_datagram.py` and `third_party/betaflight/ofs-sitl.patch` now define both
`USE_OSD_SD` and `USE_OSD_HD` beside `USE_OSD` in the SITL `target.h` hunk. The patch hunk still applies cleanly; the
Windows build (`scripts/build-sitl-windows.sh`, MSYS2 `msys`, BF `2026.6.2`, `e0b7bb01`) produced a binary whose build
tree has:

```
#define USE_OSD
#define USE_OSD_SD
#define USE_OSD_HD
```

`ofs-fc`'s `sitl_live` suite still passes 8/8 with this binary, including `displayport_frames_come_back_in_the_reply`,
which runs a PAL diff: the SD path is unchanged.

Reproduce with `OFS_SITL_LAUNCH=<the exe> cargo test -p ofs-fc --test sitl_live -- --ignored --test-threads=1`.

## Capture

A throwaway ignored test (since removed) ran a level, still quad on the default bridge and tapped UART4 (serial index
3), like `displayport_frames_come_back_in_the_reply`. The diff was the minimal HD variant of
`crates/ofs-fc/tests/data/osd.betaflight.diff`:

```
feature OSD
serial 3 131073 115200 57600 0 115200
set osd_displayport_device = MSP
set vcd_video_system = HD
# profile 1 (2048) + col 40 (XHD bit 1024) + row 0
set osd_sys_goggle_voltage_pos = 3080
# ... elements 1..9 one row apart, and fan speed at row 19, col 52
set osd_sys_fan_speed_pos = 3700
```

After 6 s of simulated time, `MSP_OSD_CANVAS` (MSP 189) answered **[53, 20]**, and the UART4 stream held 5119 bytes:

| subcommand | frames |
|---|---|
| 0 heartbeat | 73 |
| 2 clear screen | 28 |
| 3 write string | 100 |
| 4 draw screen | 75 |
| 6 SYS | 275 |

Subcommand 6 arrived as 25 redraws of 11 markers, exactly the eleven configured SYS elements:

```
row 0..9, col 40, element 0..9   (goggle voltage .. VTX temperature)
row 19, col 52, element 10       (fan speed, bottom-right corner)
```

The corner marker proves both ends of the canvas: `osdCompleteInitialization` clamps an element to the last column and
row of the display, so on a 30×16 SD port column 40 would come back as 29 and row 19 as 15. Seeing 40 and 19 means the
port really is at least 53×20.

One decoded frame, the first SYS marker:

```
24 4D 3E 04 B6 06 00 28 00 9C
$M> len=4 cmd=182 (MSP_DISPLAYPORT) payload=[6, 0, 40, 0] checksum=0x9C
```

Payload `[6, row, col, systemElement]`: row 0, col 40, element 0 = `DISPLAYPORT_SYS_GOGGLE_VOLTAGE`. The checksum is
`len ^ cmd ^ payload[0..]` = `0x9C`.

The four-byte payload carries no value. The goggle fills in the numbers from its own link; this is why the client must
supply delay, distance and LQ itself and show `--` where no model exists.

## Positions

Betaflight's element position is `2048 (profile 1) | (row << 5) | (col & 31) | (col >= 32 ? 1024 : 0)`; rows and columns
are 5 bits each. `OSD_X()`/`OSD_Y()` read it back, so all eleven `osd_sys_*_pos` values fit the existing `uint16`
settings. An element is only drawn when its profile bits are set (`VISIBLE()`); a bare `(row << 5) | col` stays hidden.

The temporary diff used the minimal OSD configuration. Copying the shipped 5F diff instead made the SITL exit during
the bridge handshake on this Windows build before the first reply; the minimal diff isolates the HD OSD path and is the
spike's baseline. The shipped quad's own feature set is covered by the normal simulator tests.

## Font and licence

The SYS elements are markers, not characters: the FC sends `[6, row, col, element]`, and the picture is rendered by the
goggle. There is no SYS glyph in the DisplayPort stream and no font download on this path. The client therefore keeps
its analog atlas (`godot/ui/osd_font.png`, two pages, 12×18 glyphs) untouched and draws the SYS labels and values with
Godot's built-in fallback font. No new art, and no use of the GPL-3.0 Configurator `digital.mcm` (a goggle font, one
page, and not what this path needs). The fallback font ships with the engine (Noto Sans, SIL OFL 1.1), so the client
adds no asset and no new licence obligation.

Decision for Tasks 2-5: draw SYS cells through a small fallback-font path in `godot/ui/osd.gd`; do not add atlas pages.

## What Tasks 2-5 can rely on

- `USE_OSD_HD` in the SITL build is enough; no goggle handshake and no `MSP_SET_OSD_CANVAS` are needed.
- The canvas is 53×20 (not the issue's 50×18) on this pin.
- `MSP_DP_SYS` frames are `[6, row, col, systemElement]` in an MSP v1 `$M>` frame, command 182, on the same UART as
  the rest of the DisplayPort traffic.
- Element ids are `displayPortSystemElement_e` (0 goggle voltage through 10 fan speed), which the plan maps to packed
  cell bits 11-15 as `id + 1`.
- The client fills DLY from `FpvView.latency_ms` (the analog receiver's link budget; the design names that source, and
  there is no digital-link delay model until #33) and DIST from the latest fetched pose to the world's pilot, not from
  the video-delayed pose.
- Subcommand 6 currently counts in `OsdDecoder.unknown_frames` (275 in the 6 s capture); that counter is the test hook
  for Task 2.
