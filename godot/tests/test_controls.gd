extends "res://tests/testing.gd"

const Controls = preload("res://scripts/controls.gd")


## Stands in for the `Input` singleton.
class FakeInput:
	extends RefCounted
	var pads: Array = [0]
	var axes: Dictionary = {}  # "device/axis" -> value
	var buttons: Dictionary = {}  # "device/button" -> bool
	var keys: Dictionary = {}  # keycode -> bool
	var guids: Dictionary = {}  # device -> guid
	var names: Dictionary = {}  # device -> name

	func get_joy_guid(device: int) -> String:
		return guids.get(device, "")

	func get_joy_name(device: int) -> String:
		return names.get(device, "")

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


func _cal(low: float, centre: float, high: float) -> Dictionary:
	return {"name": "Test radio", "axes": {"0": {"min": low, "center": centre, "max": high}}}


func test_a_calibrated_axis_reaches_full_deflection_at_its_end_points() -> void:
	var c := Controls.new("radio")
	c.deadzone = 0.0
	c.calibration = {"guid-a": _cal(-0.8, 0.1, 0.9)}
	var input := FakeInput.new()
	input.guids = {0: "guid-a"}
	input.axes = {"0/0": 0.1}
	near(c.read(input)["roll"], 0.0, "the calibrated centre is zero")
	input.axes = {"0/0": 0.9}
	near(c.read(input)["roll"], 1.0, "top end point")
	input.axes = {"0/0": -0.8}
	near(c.read(input)["roll"], -1.0, "bottom end point")
	input.guids = {0: "guid-b"}
	input.axes = {"0/0": 0.9}
	near(c.read(input)["roll"], 0.9, "another controller is read raw")


func test_calibration_is_per_axis_and_the_throttle_uses_the_whole_range() -> void:
	var c := Controls.new("radio")
	c.calibration = {"g": {"name": "r", "axes": {"2": {"min": -0.5, "center": 0.0, "max": 0.5}}}}
	var input := FakeInput.new()
	input.guids = {0: "g"}
	input.axes = {"0/2": 0.5}
	near(c.read(input)["throttle"], 1.0, "throttle top")
	input.axes = {"0/2": -0.5}
	near(c.read(input)["throttle"], 0.0, "throttle bottom")
	input.axes = {"0/0": 0.5}
	near(c.read(input)["roll"], (0.5 - 0.02) / 0.98, "roll (axis 0) has no calibration")


func test_invert_comes_after_calibration() -> void:
	var c := Controls.new("radio")
	c.deadzone = 0.0
	c.calibration = {"g": _cal(-0.5, 0.0, 1.0)}
	c.set_binding("roll", {"kind": "axis", "device": -1, "index": 0, "invert": true})
	var input := FakeInput.new()
	input.guids = {0: "g"}
	input.axes = {"0/0": -0.5}
	near(c.read(input)["roll"], 1.0, "the bottom end point, inverted")


func test_per_channel_deadband_and_travel() -> void:
	var c := Controls.new("radio")
	c.shaping = {"roll": {"deadband": 0.2, "travel": 0.5}, "pitch": {"travel": 0.5}}
	var input := FakeInput.new()
	input.axes = {"0/0": 0.1, "0/1": 0.1, "0/3": 0.1}
	var s := c.read(input)
	near(s["roll"], 0.0, "inside roll's own deadband")
	near(s["pitch"], (0.1 - 0.02) / 0.98 / 0.5, "pitch keeps the global deadzone, with travel")
	near(s["yaw"], (0.1 - 0.02) / 0.98, "yaw is untouched")
	input.axes = {"0/0": 0.5, "0/1": 0.6}
	near(c.read(input)["roll"], 0.75, "(0.5 - 0.2) / 0.8 / 0.5")
	near(c.read(input)["pitch"], 1.0, "pitch reaches full early")


func test_presets_reset_the_shaping_but_not_the_calibration() -> void:
	var c := Controls.new("radio")
	c.shaping = {"roll": {"travel": 0.5}}
	c.calibration = {"g": _cal(-1, 0, 1)}
	c.apply_preset("gamepad")
	eq(c.shaping, {}, "shaping cleared")
	eq(c.calibration.size(), 1, "calibration kept")


func test_shaping_survives_a_json_round_trip_and_is_cleaned_on_load() -> void:
	var a := Controls.new("radio")
	a.shaping = {"roll": {"deadband": 0.1, "travel": 0.8}}
	var b := Controls.new("keyboard")
	ok(b.from_json(a.to_json()), "loads")
	eq(b.shaping, a.shaping, "shaping")
	var dirty := Controls.new("radio").to_profile()
	dirty["shaping"] = {"roll": {"deadband": 9.0, "travel": 0.0}, "aux1": {"travel": 0.5}, "pitch": 5, "yaw": {"travel": "x"}}
	ok(b.from_profile(dirty), "a dirty shaping section still loads")
	eq(b.shaping, {"roll": {"deadband": 0.5, "travel": 0.3}, "yaw": {}}, "clamped; aux and non-dictionaries dropped; junk values dropped")


func test_an_old_file_without_shaping_loads_with_none() -> void:
	var c := Controls.new("radio")
	c.shaping = {"roll": {"travel": 0.5}}
	var old := {"version": 1, "preset": "gamepad", "deadzone": 0.07, "bindings": Controls.preset("gamepad")["bindings"]}
	ok(c.from_json(JSON.stringify(old)), "a version 1 file loads")
	eq(c.shaping, {}, "no shaping")
	near(c.deadzone, 0.07, "deadzone as saved")


func test_the_device_name_picks_a_preset() -> void:
	eq(Controls.preset_for_device("Xbox Wireless Controller"), "gamepad", "xbox")
	eq(Controls.preset_for_device("PS5 Controller"), "gamepad", "playstation")
	eq(Controls.preset_for_device("RadioMaster Pocket Joystick"), "radio", "radiomaster")
	eq(Controls.preset_for_device("FrSky Taranis X9D"), "radio", "taranis")
	eq(Controls.preset_for_device("Logitech Extreme 3D"), "", "unknown")
	eq(Controls.preset_for_device(""), "", "empty")


func test_the_device_key_is_the_guid_else_the_name() -> void:
	var input := FakeInput.new()
	input.guids = {0: "abc"}
	input.names = {0: "Pad", 1: "Other"}
	eq(Controls.device_key(input, 0), "abc", "guid")
	eq(Controls.device_key(input, 1), "Other", "name when there is no guid")
	eq(Controls.device_key(input, 2), "", "nothing")


func test_calibration_tables_are_cleaned() -> void:
	var clean := Controls.clean_calibration({
		"g": {"name": "Pad", "axes": {"0": {"min": -1, "center": 0, "max": 1}, "1": {"min": 1, "center": 0, "max": 1}, "x": {}, "2": 3}},
		"bad": 5, "worse": {"axes": 3},
	})
	eq(clean.keys(), ["g"], "only the usable controller")
	eq(clean["g"]["name"], "Pad", "name")
	eq(clean["g"]["axes"].keys(), ["0"], "only the well-formed axis (min <= centre <= max)")


func test_the_real_input_singleton_has_the_device_methods_the_key_uses() -> void:
	ok(Input.has_method("get_joy_guid") and Input.has_method("get_joy_name"), "Input exposes them")
	eq(Controls.device_key(Input, 7), "", "a device that is not there has no key")
