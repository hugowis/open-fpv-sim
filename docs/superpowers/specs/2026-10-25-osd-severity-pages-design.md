# OSD font pages 2–3: warnings and criticals (part of #32)

A Godot-only change (`godot/ui/osd.gd`, `godot/tests/test_osd.gd`); no protocol, server or Rust change, and **no new glyph
art**. Whether it closes #32 is the maintainer's call: the HD/digital half of the issue is not done, see "What is left".

## Why

Betaflight's MSP DisplayPort sends each character's *severity* as its font page (`displayport_msp.c`: `fontSelection[severity]`,
identity by default; `displayPortSeverity_e`: NORMAL 0, INFO 1, WARNING 2, CRITICAL 3). Warnings and criticals
(`osd_elements.c`: low battery, the warnings line, RSSI and link-quality alarms) therefore arrive on pages 2 and 3. The
atlas ships pages 0 (default face) and 1 (bold, INFO); pages 2 and 3 fall back to page 0, so a pilot sees a warning as
ordinary text. The brief is the issue's three lines; no design conversation took place, so every ruling below is the
author's call and is open to change on review.

## Goals

1. A WARNING cell (page 2) and a CRITICAL cell (page 3) are visibly different from normal text.
2. No new glyph art and no licence change: the atlas, `tools/make_osd_font.py` and `OSD_FONT_LICENSE.md`'s sources stay.

## Rulings

**R1. Severity is drawn by tint, from the bold page.** Pages 2 and 3 use the glyphs of page 1 (the bold face of
`resources/osd/2/bold.mcm`, already in the atlas) multiplied by amber (page 2) or red (page 3). The atlas is white glyphs
with a black outline, so the tint colours the glyph and keeps the outline. Rejected: *more atlas pages*: the only
GPL-3.0-compatible glyph source in hand, the Configurator's `resources/osd/{1,2}/*.mcm`, holds 256-glyph *alternative
faces* (default, bold, clarity, impact, vision, …), none of which is a warning or a critical face, so a copied page would
be the same shapes under another name; the HD fonts that do have four pages belong to the goggle vendors and carry
licences not checked against GPL-3.0-or-later.

**R2. The tint lives in `osd.gd`, beside `glyph_rect`.** `Osd.glyph_page(page)` maps the wire page to the atlas page
(0 → 0, 1 → 1, 2 → 1, 3 → 1), `Osd.glyph_tint(page)` returns the colour (white for 0 and 1). `glyph_rect` keeps its
signature and uses `glyph_page`. Pages beyond 3 cannot occur (`& 0x3` in the decoder); an atlas that ships fewer pages
than a mapping names still falls back to page 0, as today.

**R3. Colours.** Amber `Color(1.0, 0.72, 0.1)` for WARNING, red `Color(1.0, 0.25, 0.25)` for CRITICAL, chosen to read on
the analog video's grain and on a bright sky; they carry on through the analog video layer (layer 3) like the white text.
The DVR's goggles feed records them as drawn; the clean feed has no OSD (unchanged).

## What is left (not in this change)

The "full HD/digital OSD element set": the OSD_SYS elements (goggle voltage, VTX voltage, bitrate, delay, distance, LQ,
DVR, warnings, VTX temperature, fan speed) that Betaflight emits only when built with `USE_OSD_HD` and the goggles render
themselves (`osd_elements.c`, `writeSys` in `displayport_msp.c`), plus the 50 × 18 HD canvas and an HD font. This
simulator's SITL build defines `USE_OSD_SD` (`docs/research/sitl-interface.md`), so none of it is ever sent; drawing it
needs an HD SITL build, a decoder for the system-element sub-command, a source for each value in the simulator, and a
font with a verified licence. That is a milestone, not a good-first-issue, and the roadmap line keeps it.

## Tests

`godot/tests/test_osd.gd` (headless, state not pixels): the atlas layout test now asserts pages 2 and 3 map to the bold
page's rectangles; a new test asserts the four tints (white, white, amber, red) and that the amber and red are not white;
the frame fixture's page-2 cell is drawn (still counted). Pixels are by eye with `tests/shots.gd`'s `osd_fpv.png`, which
gains a warning and a critical word.
