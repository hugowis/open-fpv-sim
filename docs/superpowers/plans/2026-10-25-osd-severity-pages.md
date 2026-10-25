# OSD font pages 2–3 as severity colours: implementation plan

Spec: `docs/superpowers/specs/2026-10-25-osd-severity-pages-design.md`. Test first. Gate: in WSL,
`GODOT_BIN=<godot 4.7.2> bash scripts/run-godot-tests.sh unit`, and grep its output for `SCRIPT ERROR` (the runner prints
`0 failed` even when a test aborts). No Windows cargo at the same time (shared `target/`).

1. **Tests** (`godot/tests/test_osd.gd`): change the atlas-layout assertions for pages 2 and 3 to the bold rectangles; add
   `test_severity_pages_are_tinted_bold_glyphs` for `glyph_page` and `glyph_tint`; update the fixture comment.
2. **Code** (`godot/ui/osd.gd`): `SEVERITY_TINTS`, `glyph_page`, `glyph_tint`; `glyph_rect` uses `glyph_page`; `_draw_grid`
   passes the tint as the modulate of `draw_texture_rect_region`.
3. **Shots** (`godot/tests/shots.gd`): a warning and a critical word in the `osd_fpv` grid, to look at once by eye.
4. **Docs**: `godot/ui/OSD_FONT_LICENSE.md` (pages 2–3 are page 1's glyphs tinted, no new source), `docs/dev-setup.md` line
   73, `docs/roadmap.md` (the pages half of the line is done, the HD half stays), `CHANGELOG.md` Unreleased.
5. Gate, commit `feat(godot): the OSD draws warnings and criticals - font pages 2 and 3 are the bold glyphs in amber and red (#32)`.
