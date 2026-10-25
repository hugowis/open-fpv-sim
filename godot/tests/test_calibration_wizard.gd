extends "res://tests/testing.gd"

const Wizard = preload("res://ui/calibration_wizard.gd")


## Stands in for the `Input` singleton.
class FakeInput:
	extends RefCounted
	var pads: Array = [3]
	var axes: Dictionary = {}  # "device/axis" -> value
	var guids: Dictionary = {3: "guid-3", 5: "guid-5"}
	var names: Dictionary = {3: "Radio A", 5: "Pad B"}

	func get_connected_joypads() -> Array:
		return pads

	func get_joy_axis(device: int, axis: int) -> float:
		return axes.get("%d/%d" % [device, axis], 0.0)

	func get_joy_guid(device: int) -> String:
		return guids.get(device, "")

	func get_joy_name(device: int) -> String:
		return names.get(device, "")


func _wizard(input) -> Control:
	var w := Wizard.new()
	w.input = input
	await add_to_tree(w)
	return w


func _rest(w, input, seconds := 1.2) -> void:
	for i in int(round(seconds * 10.0)):
		w.poll(0.1)


func test_it_does_not_open_without_a_controller() -> void:
	var input := FakeInput.new()
	input.pads = []
	var w = await _wizard(input)
	ok(not w.start(), "nothing to calibrate")
	ok(not w.is_open(), "stays closed")
	w.queue_free()


func test_the_two_steps_end_in_a_result_for_the_controller_key() -> void:
	var input := FakeInput.new()
	input.axes = {"3/0": 0.04, "3/2": -0.8}
	var w = await _wizard(input)
	var seen := []
	w.finished.connect(func(key: String, device_name: String, result: Dictionary) -> void: seen.append([key, device_name, result]))
	ok(w.start(), "opens")
	eq(w.current_device(), 3, "the only pad")
	eq(w.step(), "rest", "starts with the centres")
	ok(w._done.disabled, "Done waits for the sweep")
	_rest(w, input)
	eq(w.step(), "sweep", "then the end points")
	for v in [0.9, -0.7]:
		input.axes["3/0"] = v
		input.axes["3/2"] = -0.8 + (v + 0.7)
		w.poll(0.1)
	ok(not w._done.disabled, "Done is available")
	ok(w._bars.has(0) and w._bars.has(2), "a bar for each axis that moved")
	ok(not w._bars.has(1), "none for a quiet axis")
	w._done.pressed.emit()
	ok(not w.is_open(), "closed")
	eq(seen.size(), 1, "finished once")
	eq(seen[0][0], "guid-3", "the controller key")
	eq(seen[0][1], "Radio A", "and its name")
	near(seen[0][2]["0"]["max"], 0.9, "axis 0 max")
	near(seen[0][2]["0"]["center"], 0.04, "axis 0 centre")
	w.queue_free()


func test_done_with_nothing_moved_keeps_the_wizard_open() -> void:
	var input := FakeInput.new()
	var w = await _wizard(input)
	var seen := []
	w.finished.connect(func(_k: String, _n: String, _r: Dictionary) -> void: seen.append(true))
	w.start()
	_rest(w, input)
	w.finish()
	eq(seen, [], "no result")
	ok(w.is_open(), "still open")
	ok(w.hint().contains("No axis moved"), "and it says why")
	w.queue_free()


func test_cancel_and_escape_close_without_a_result() -> void:
	var input := FakeInput.new()
	var w = await _wizard(input)
	var seen := []
	w.finished.connect(func(_k: String, _n: String, _r: Dictionary) -> void: seen.append("finished"))
	w.cancelled.connect(func() -> void: seen.append("cancelled"))
	w.start()
	w._cancel.pressed.emit()
	eq(seen, ["cancelled"], "cancel")
	ok(not w.is_open(), "closed")
	w.start()
	var esc := InputEventKey.new()
	esc.keycode = KEY_ESCAPE
	esc.physical_keycode = KEY_ESCAPE
	esc.pressed = true
	w._input(esc)
	eq(seen, ["cancelled", "cancelled"], "escape")
	w.queue_free()


func test_with_several_controllers_the_picker_chooses_and_restarts() -> void:
	var input := FakeInput.new()
	input.pads = [3, 5]
	var w = await _wizard(input)
	w.start()
	ok(w._pick.visible, "a picker with two pads")
	eq(w.current_device(), 3, "the first")
	_rest(w, input)
	eq(w.step(), "sweep", "past the centres")
	w._pick.select(1)
	w._pick.item_selected.emit(1)
	eq(w.current_device(), 5, "the second")
	eq(w.step(), "rest", "the wizard starts again for it")
	w.queue_free()
