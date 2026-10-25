extends "res://tests/testing.gd"

const CycleButton = preload("res://ui/cycle_button.gd")


func _key(action: String) -> InputEventAction:
	var e := InputEventAction.new()
	e.action = action
	e.pressed = true
	return e


func test_it_cycles_forward_and_back_and_says_what_changed() -> void:
	var b := CycleButton.new()
	b.setup("Picture", ["4:3", "16:9", "window"])
	var got := []
	b.changed.connect(func(v): got.append(v))
	eq(b.text, "Picture: 4:3", "the first value is shown")
	b._pressed()
	eq(b.value(), "16:9", "press: next")
	b._gui_input(_key("ui_right"))
	eq(b.value(), "window", "right: next")
	b._gui_input(_key("ui_right"))
	eq(b.value(), "4:3", "wraps around")
	b._gui_input(_key("ui_left"))
	eq(b.value(), "window", "left: previous")
	eq(got, ["16:9", "window", "4:3", "window"], "each change is emitted")
	b.free()


func test_set_value_shows_without_emitting_and_unknown_values_fall_back() -> void:
	var b := CycleButton.new()
	b.setup("", ["a", "b"], ["Alpha", "Beta"])
	var got := []
	b.changed.connect(func(v): got.append(v))
	b.set_value("b")
	eq(b.text, "Beta", "no title: just the label")
	b.set_value("zzz")
	eq(b.value(), "a", "unknown: the first value")
	eq(got, [], "nothing emitted")
	var one := CycleButton.new()
	one.setup("Only", ["x"])
	one.step(1)
	eq(one.value(), "x", "a single value does not change")
	var empty := CycleButton.new()
	empty.setup("None", [])
	eq(empty.value(), null, "no values, no value")
	b.free()
	one.free()
	empty.free()
