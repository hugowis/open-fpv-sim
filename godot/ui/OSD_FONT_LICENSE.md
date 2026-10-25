# OSD font

`osd_font.png` is a glyph atlas converted by `tools/make_osd_font.py` from Betaflight's analog OSD font
`resources/osd/2/default.mcm` (the atlas's page 0), from the Betaflight Configurator repository
(https://github.com/betaflight/betaflight-configurator), commit `d85e797e8674e3059cc3172e38df831e46a5250c`, sha256 of the .mcm
`d3b122c5da7fe83fc10f70ee49817b9d4f4251fc2529146b8294424782b442f6`.

The atlas's second page (page 1, the bold face Betaflight uses for its INFO-severity elements) is converted from
`resources/osd/2/bold.mcm`, from the same repository, commit `74d0d917cbe11f5712618cbc2491fdc757ef56f3`, 147463 bytes,
sha256 `fa2291ade88ef76912ae7f2bdf0eb23223b8d9c78b4b586501fe5ec779058614`, under the same GNU General Public License
v3.0 (not "or later") as default.mcm.

Betaflight sends a cell's severity as its font page, so a WARNING arrives on page 2 and a CRITICAL on page 3. The atlas
has no pages for them: `godot/ui/osd.gd` draws both from page 1's bold glyphs, tinted amber and red at draw time. No glyph
art from any other source is in this repository.

Betaflight Configurator is licensed under the GNU General Public License v3.0 (its repository reports GPL-3.0). This
project is GPL-3.0-or-later; the atlas is a derivative of the font and stays under the GPL-3.0 terms of its source.

Betaflight sends character codes, not pictures; the symbols (battery, units, arrows) only look right with this font.

**Maintainer's ruling (2026-10-13):** accepted. The atlas stays under GPL-3.0 (not "or later") inside this
GPL-3.0-or-later project; GPL-3.0 is compatible with GPL-3.0-or-later, so the combined work is distributed under
GPL-3.0. Replacing the atlas with a font under GPL-3.0-or-later (or a more permissive licence) would lift that.
