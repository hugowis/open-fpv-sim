# OSD font

`osd_font.png` is a glyph atlas converted by `tools/make_osd_font.py` from Betaflight's analog OSD font
`resources/osd/2/default.mcm`, from the Betaflight Configurator repository
(https://github.com/betaflight/betaflight-configurator), commit `d85e797e8674e3059cc3172e38df831e46a5250c`, sha256 of the .mcm
`d3b122c5da7fe83fc10f70ee49817b9d4f4251fc2529146b8294424782b442f6`.

Betaflight Configurator is licensed under the GNU General Public License v3.0 (its repository reports GPL-3.0). This
project is GPL-3.0-or-later; the atlas is a derivative of the font and stays under the GPL-3.0 terms of its source.

Betaflight sends character codes, not pictures; the symbols (battery, units, arrows) only look right with this font.
