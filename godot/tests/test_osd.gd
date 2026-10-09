extends "res://tests/testing.gd"
## The OSD layer's logic (headless has no renderer, so these check state, not pixels; tests/shots.gd draws it).

const Osd = preload("res://ui/osd.gd")


func _osd() -> CanvasLayer:
	var osd := Osd.new()
	await add_to_tree(osd)
	return osd


## A 3x2 frame: row 0 is " A " with the A blinking, row 1 is "B C".
func _frame(overrides := {}) -> Dictionary:
	var cells := PackedInt32Array([0x20, 0x41 | (1 << 10), 0x20, 0x42, 0x20, 0x43])
	var f := {"seq": 1, "time_s": 0.0, "present": true, "cols": 3, "rows": 2, "cells": cells}
	f.merge(overrides, true)
	return f


func test_the_font_atlas_has_the_expected_layout() -> void:
	eq(Osd.FONT.get_size(), Vector2(192, 288), "16 x 16 glyphs of 12 x 18 pixels")
	eq(Osd.glyph_rect(0), Rect2(0, 0, 12, 18), "glyph 0")
	eq(Osd.glyph_rect(65), Rect2(12, 72, 12, 18), "glyph 'A' is column 1, row 4")
	eq(Osd.glyph_rect(255), Rect2(180, 270, 12, 18), "the last glyph")


func test_the_box_is_4_by_3_fitted_and_centred() -> void:
	eq(Osd.box_for(Vector2(1280, 720)), Rect2(160, 0, 960, 720), "a wide window: height-fitted")
	eq(Osd.box_for(Vector2(400, 800)), Rect2(0, 250, 400, 300), "a tall window: width-fitted")


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
	eq(osd.drawn_cell_count(), 3, "A (blinking, on), B and C; the three blanks are skipped")
	osd.advance_blink(0.6)
	ok(not osd.blink_visible(), "the phase flips after half a period")
	eq(osd.drawn_cell_count(), 2, "the blinking A is off")
	osd.advance_blink(0.6)
	ok(osd.blink_visible(), "and on again")
	eq(osd.drawn_cell_count(), 3, "all three again")
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
