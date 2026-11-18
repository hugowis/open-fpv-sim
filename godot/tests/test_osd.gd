extends "res://tests/testing.gd"
## The OSD layer's logic (headless has no renderer, so these check state, not pixels; tests/shots.gd draws it).

const Osd = preload("res://ui/osd.gd")


func _osd() -> CanvasLayer:
	var osd := Osd.new()
	await add_to_tree(osd)
	return osd


## A 3x2 frame: row 0 is " A " with the A blinking, row 1 is "B C" with B addressed on font page 2 (a WARNING,
## drawn from the bold page and tinted).
func _frame(overrides := {}) -> Dictionary:
	var cells := PackedInt32Array([0x20, 0x41 | (1 << 10), 0x20, 0x42 | (2 << 8), 0x20, 0x43])
	var f := {"seq": 1, "time_s": 0.0, "present": true, "cols": 3, "rows": 2, "cells": cells}
	f.merge(overrides, true)
	return f


## A 3x2 frame whose first cell is HD system element `sys` (1 = element 0); the other five are spaces.
func _sys_frame(sys: int) -> Dictionary:
	var cells := PackedInt32Array([0x20, 0x20, 0x20, 0x20, 0x20, 0x20])
	cells[0] = 0x20 | (sys << 11)
	return {"seq": 1, "time_s": 0.0, "present": true, "cols": 3, "rows": 2, "cells": cells}


func test_the_font_atlas_has_the_expected_layout() -> void:
	eq(Osd.FONT.get_size(), Vector2(192, 576), "two pages of 16 x 16 glyphs of 12 x 18 pixels")
	eq(Osd.glyph_rect(0, 0), Rect2(0, 0, 12, 18), "glyph 0, page 0")
	eq(Osd.glyph_rect(0, 65), Rect2(12, 72, 12, 18), "glyph 'A' is column 1, row 4")
	eq(Osd.glyph_rect(1, 0), Rect2(0, 288, 12, 18), "page 1 starts one glyph-block down")
	eq(Osd.glyph_rect(1, 255), Rect2(180, 558, 12, 18), "the last glyph of page 1")
	eq(Osd.glyph_rect(2, 65), Osd.glyph_rect(1, 65), "page 2 (WARNING) uses the bold glyphs")
	eq(Osd.glyph_rect(3, 0), Osd.glyph_rect(1, 0), "so does page 3 (CRITICAL)")


func test_warnings_and_criticals_are_the_bold_glyphs_in_amber_and_red() -> void:
	eq([Osd.glyph_page(0), Osd.glyph_page(1), Osd.glyph_page(2), Osd.glyph_page(3)], [0, 1, 1, 1], "atlas page behind each wire page")
	eq(Osd.glyph_tint(0), Color.WHITE, "normal text is untinted")
	eq(Osd.glyph_tint(1), Color.WHITE, "so is INFO")
	var warning := Osd.glyph_tint(2)
	var critical := Osd.glyph_tint(3)
	ok(warning.r > 0.9 and warning.g > 0.5 and warning.g < 0.9 and warning.b < 0.3, "WARNING is amber: %s" % warning)
	ok(critical.r > 0.9 and critical.g < 0.4 and critical.b < 0.4, "CRITICAL is red: %s" % critical)
	ok(warning != critical, "and they differ")


func test_nothing_is_drawn_without_a_present_frame() -> void:
	var osd := await _osd()
	ok(not osd.is_drawing(), "no frame yet")
	osd.set_frame(_frame({"present": false}))
	ok(not osd.is_drawing(), "not present")
	osd.set_frame(_frame({"cols": 0, "rows": 0, "cells": PackedInt32Array()}))
	ok(not osd.is_drawing(), "no grid (a quad without an OSD)")
	osd.set_frame(_frame())
	ok(osd.is_drawing(), "present with a grid")
	osd.set_enabled(false)
	ok(not osd.is_drawing(), "hidden in the chase view")
	osd.set_enabled(true)
	ok(osd.is_drawing(), "and back")
	osd.queue_free()


func test_blank_cells_are_not_drawn_and_blinking_cells_follow_the_blink_phase() -> void:
	var osd := await _osd()
	osd.set_frame(_frame())
	ok(osd.blink_visible(), "blink starts visible")
	eq(osd.drawn_cell_count(), 3, "A (blinking, on), B (page 2, a warning) and C; the three blanks are skipped")
	osd.advance_blink(0.6)
	ok(not osd.blink_visible(), "the phase flips after half a period")
	eq(osd.drawn_cell_count(), 2, "the blinking A is off")
	osd.advance_blink(0.6)
	ok(osd.blink_visible(), "and on again")
	eq(osd.drawn_cell_count(), 3, "all three again")
	osd.queue_free()


func test_a_sys_cell_is_drawn_as_a_marker_not_text() -> void:
	var osd := await _osd()
	osd.set_frame(_sys_frame(1))
	eq(osd.drawn_cell_count(), 1, "the cell's character is a space, but its SYS marker is drawn")
	eq(osd.sys_text(1), "GVOLT --", "element 0 has no model yet")
	osd.queue_free()


func test_the_sys_values_come_from_the_environment() -> void:
	var osd := await _osd()
	osd.set_environment({"latency_ms": 42, "pilot": Vector3.ZERO, "position": Vector3(0.0, 0.0, 123.0), "lq_pct": 87.6, "dvr": true})
	eq(osd.sys_text(4), "DLY 42ms", "element 3 is the video delay")
	eq(osd.sys_text(5), "DIST 123m", "element 4 is the distance from the pilot")
	eq(osd.sys_text(6), "LQ 88%", "element 5 is the radio link quality")
	eq(osd.sys_text(7), "GDVR REC", "element 6 is the goggles' DVR")
	osd.set_environment({"dvr": false})
	eq(osd.sys_text(7), "GDVR OFF", "the DVR is off, not missing")
	osd.queue_free()


func test_the_unmodelled_sys_values_are_dashes() -> void:
	var osd := await _osd()
	osd.set_environment({})
	for sys in range(1, 12):
		ok(osd.sys_text(sys).ends_with("--"), "element %d has no model: %s" % [sys - 1, osd.sys_text(sys)])
	osd.set_environment({"latency_ms": 40})
	eq(osd.sys_text(4), "DLY 40ms", "a value present on its own renders")
	ok(osd.sys_text(6).ends_with("--"), "and the others stay dashes")
	osd.queue_free()


func test_a_53_by_20_grid_draws_its_last_cell() -> void:
	var osd := await _osd()
	var cells := PackedInt32Array()
	cells.resize(53 * 20)
	cells.fill(0x20)
	cells[19 * 53 + 52] = 0x20 | (11 << 11)
	osd.set_frame({"seq": 1, "time_s": 0.0, "present": true, "cols": 53, "rows": 20, "cells": cells})
	eq(osd.drawn_cell_count(), 1, "the bottom-right marker is inside the grid")
	eq(osd.sys_text(11), "FAN --", "the last element")
	osd.queue_free()


func test_a_malformed_frame_does_not_crash_the_layer() -> void:
	var osd := await _osd()
	osd.set_frame(_frame({"cells": PackedInt32Array([0x41])}))  # fewer cells than cols * rows
	osd._draw_grid()
	eq(osd.drawn_cell_count(), 1, "counts what is there")
	osd.set_frame({})
	osd._draw_grid()
	ok(not osd.is_drawing(), "an empty frame draws nothing")
	osd.queue_free()
