extends "res://tests/testing.gd"

const Controls = preload("res://scripts/controls.gd")


## Stands in for the `Input` singleton.
class FakeInput:
	extends RefCounted
	var pads: Array = [0]
	var axes: Dictionary = {}  # "device/axis" -> value
	var buttons: Dictionary = {}  # "device/button" -> bool
	var keys: Dictionary = {}  # keycode -> bool

	func get_connected_joypads() -> Array:
		return pads

	func get_joy_axis(device: int, axis: int) -> float:
		return axes.get("%d/%d" % [device, axis], 0.0)

	func is_joy_button_pressed(device: int, button: int) -> bool:
		return buttons.get("%d/%d" % [device, button], false)

	func is_physical_key_pressed(key: int) -> bool:
		return keys.get(key, false)


func test_radio_preset_reads_aetr_axes_straight() -> void:
	var c := Controls.new("radio")
	var input := FakeInput.new()
	input.axes = {"0/0": 0.5, "0/1": -0.25, "0/2": 1.0, "0/3": 0.75, "0/4": 1.0, "0/5": -1.0}
	var s := c.read(input)
	near(s["roll"], (0.5 - 0.02) / 0.98, "roll")
	near(s["pitch"], -(0.25 - 0.02) / 0.98, "pitch")
	near(s["yaw"], (0.75 - 0.02) / 0.98, "yaw")
	near(s["throttle"], 1.0, "throttle at the top")
	eq(s["aux"], [1.0, -1.0, 0.0, 0.0], "aux1 on, aux2 off, the two idle axes read as the middle")
	eq(s["status"], "", "no warning")


func test_throttle_maps_the_whole_axis_to_zero_to_one() -> void:
	var c := Controls.new("radio")
	var input := FakeInput.new()
	for pair in [[-1.0, 0.0], [0.0, 0.5], [1.0, 1.0]]:
		input.axes = {"0/2": pair[0]}
		near(c.read(input)["throttle"], pair[1], "throttle axis %s" % str(pair[0]))


func test_gamepad_preset_pushes_up_for_throttle_and_forward_for_pitch() -> void:
	var c := Controls.new("gamepad")
	var input := FakeInput.new()
	input.axes = {"0/1": -1.0, "0/3": -1.0, "0/0": 0.0, "0/2": 1.0}  # left stick up, right stick forward and right
	var s := c.read(input)
	near(s["throttle"], 1.0, "left stick up is full throttle")
	near(s["pitch"], 1.0, "right stick forward is positive pitch")
	near(s["roll"], 1.0, "right stick right is positive roll")
	near(s["yaw"], 0.0, "yaw at rest")


func test_deadzone_rescales_what_is_left() -> void:
	var c := Controls.new("radio")
	c.deadzone = 0.1
	var input := FakeInput.new()
	input.axes = {"0/0": 0.05}
	near(c.read(input)["roll"], 0.0, "inside the deadzone")
	input.axes = {"0/0": 0.55}
	near(c.read(input)["roll"], 0.5, "(0.55 - 0.1) / 0.9")
	input.axes = {"0/0": 1.0}
	near(c.read(input)["roll"], 1.0, "full deflection stays full")
	input.axes = {"0/0": -1.0}
	near(c.read(input)["roll"], -1.0, "and negative")


func test_buttons_toggle_on_the_press_not_while_held() -> void:
	var c := Controls.new("gamepad")
	var input := FakeInput.new()
	eq(c.read(input)["aux"][0], -1.0, "arm starts off")
	input.buttons = {"0/%d" % JOY_BUTTON_A: true}
	eq(c.read(input)["aux"][0], 1.0, "pressed: armed")
	eq(c.read(input)["aux"][0], 1.0, "held: still armed, no re-toggle")
	input.buttons = {}
	eq(c.read(input)["aux"][0], 1.0, "released: stays armed")
	input.buttons = {"0/%d" % JOY_BUTTON_A: true}
	eq(c.read(input)["aux"][0], -1.0, "pressed again: disarmed")


func test_keyboard_preset() -> void:
	var c := Controls.new("keyboard")
	var input := FakeInput.new()
	input.pads = []
	input.keys = {KEY_RIGHT: true, KEY_UP: true, KEY_E: true, KEY_W: true}
	var s := c.read(input, true, 0.5)
	near(s["roll"], 0.6, "right arrow")
	near(s["pitch"], 0.6, "up arrow is forward")
	near(s["yaw"], 0.8, "E yaws right")
	near(s["throttle"], 0.3, "W held for half a second at 0.6 per second")
	eq(s["status"], "", "the keyboard needs no controller")
	s = c.read(input, true, 10.0)
	near(s["throttle"], 1.0, "the throttle stops at the top")
	input.keys = {KEY_S: true}
	s = c.read(input, true, 10.0)
	near(s["throttle"], 0.0, "and at the bottom")
	input.keys = {KEY_SPACE: true}
	eq(c.read(input)["aux"][0], 1.0, "space arms")
	input.keys = {KEY_LEFT: true, KEY_DOWN: true, KEY_Q: true}
	s = c.read(input)
	near(s["roll"], -0.6, "left arrow")
	near(s["pitch"], -0.6, "down arrow")
	near(s["yaw"], -0.8, "Q yaws left")


func test_a_missing_controller_centres_the_sticks_and_warns() -> void:
	var c := Controls.new("radio")
	var input := FakeInput.new()
	input.pads = []
	input.axes = {"0/0": 1.0, "0/2": 1.0}
	var s := c.read(input)
	eq([s["roll"], s["pitch"], s["yaw"], s["throttle"]], [0.0, 0.0, 0.0, 0.0], "centred sticks and closed throttle")
	eq(s["aux"], [-1.0, -1.0, -1.0, -1.0], "switches off")
	ok(s["status"].contains("not connected"), "a warning for the HUD: %s" % s["status"])
	# A binding to a specific device that has been unplugged.
	c.set_binding("roll", {"kind": "axis", "device": 3, "index": 0, "invert": false})
	input.pads = [0]
	input.axes = {"0/0": 1.0, "3/0": 1.0}
	s = c.read(input)
	near(s["roll"], 0.0, "device 3 is not connected")
	ok(s["status"] != "", "and the HUD is told")
	input.pads = [0, 3]
	s = c.read(input)
	near(s["roll"], 1.0, "plugged back in")
	eq(s["status"], "", "the warning clears")


func test_losing_window_focus_centres_the_sticks_but_keeps_the_switches() -> void:
	var c := Controls.new("gamepad")
	var input := FakeInput.new()
	input.axes = {"0/1": -1.0, "0/2": 1.0}
	input.buttons = {"0/%d" % JOY_BUTTON_A: true}
	var s := c.read(input, true)
	near(s["throttle"], 1.0, "focused")
	eq(s["aux"][0], 1.0, "armed")
	s = c.read(input, false)
	eq([s["roll"], s["pitch"], s["yaw"], s["throttle"]], [0.0, 0.0, 0.0, 0.0], "unfocused: centred")
	eq(s["aux"][0], 1.0, "the arm switch stays on")


func test_unfocused_keyboard_throttle_starts_again_from_zero() -> void:
	var c := Controls.new("keyboard")
	var input := FakeInput.new()
	input.pads = []
	input.keys = {KEY_W: true}
	near(c.read(input, true, 1.0)["throttle"], 0.6, "ramped up")
	input.keys = {}
	near(c.read(input, false, 0.1)["throttle"], 0.0, "focus lost")
	near(c.read(input, true, 0.1)["throttle"], 0.0, "and it stays closed when focus returns")


func test_non_finite_axis_values_are_treated_as_a_missing_controller() -> void:
	var c := Controls.new("radio")
	var input := FakeInput.new()
	input.axes = {"0/0": NAN, "0/1": INF}
	var s := c.read(input)
	for key in ["roll", "pitch", "yaw", "throttle"]:
		ok(is_finite(s[key]), "%s is finite" % key)
	ok(s["status"] != "", "reported")


func test_bindings_survive_a_json_round_trip() -> void:
	for name in Controls.PRESET_NAMES:
		var a := Controls.new(name)
		a.deadzone = 0.07
		var b := Controls.new("radio")
		ok(b.from_json(a.to_json()), "%s loads" % name)
		eq(b.bindings, a.bindings, "%s bindings" % name)
		near(b.deadzone, 0.07, "%s deadzone" % name)
		eq(b.preset_name, name, "%s preset name" % name)


func test_invalid_controls_files_are_rejected_and_change_nothing() -> void:
	var c := Controls.new("keyboard")
	var before := c.bindings.duplicate(true)
	for text in ["", "not json", "[]", "{\"bindings\": 3}", "{\"bindings\": {\"roll\": {\"kind\": \"telepathy\"}}}",
			"{\"bindings\": {\"roll\": {\"kind\": \"axis\"}}}", "{\"bindings\": {\"roll\": 5}}"]:
		ok(not c.from_json(text), "rejects %s" % text)
	eq(c.bindings, before, "bindings unchanged")


func test_describe_names_each_kind_of_binding() -> void:
	var c := Controls.new("gamepad")
	eq(c.describe("throttle"), "axis 1 of any controller (inverted)", "axis")
	eq(c.describe("aux1"), "button 0 of any controller (toggle)", "button")
	c.apply_preset("keyboard")
	eq(c.describe("aux1"), "key Space (toggle)", "toggle key")
	eq(c.describe("aux3"), "not bound", "unbound")
