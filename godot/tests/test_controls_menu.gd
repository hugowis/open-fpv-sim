extends "res://tests/testing.gd"

const Controls = preload("res://scripts/controls.gd")
const Menu = preload("res://ui/controls_menu.gd")

const SAVE_PATH := "user://test_controls.json"


func _menu(preset := "radio") -> Array:
	var controls := Controls.new(preset)
	var menu := Menu.new()
	await add_to_tree(menu)
	DirAccess.remove_absolute(SAVE_PATH)
	menu.setup(controls, SAVE_PATH)
	menu.toggle()
	return [menu, controls]


func _axis_event(device: int, axis: int, value: float) -> InputEventJoypadMotion:
	var e := InputEventJoypadMotion.new()
	e.device = device
	e.axis = axis
	e.axis_value = value
	return e


func _key_event(keycode: int) -> InputEventKey:
	var e := InputEventKey.new()
	e.physical_keycode = keycode
	e.pressed = true
	return e


func test_moving_a_stick_binds_the_channel_and_saves() -> void:
	var m := await _menu()
	var menu = m[0]
	var controls = m[1]
	menu._start_listening("roll")
	eq(menu.listening_for(), "roll", "listening")
	menu._input(_axis_event(0, 5, 0.2))
	eq(menu.listening_for(), "roll", "a small movement is ignored")
	menu._input(_axis_event(0, 5, -0.9))
	eq(menu.listening_for(), "", "bound")
	eq(controls.bindings["roll"]["kind"], "axis", "an axis binding")
	eq(controls.bindings["roll"]["index"], 5, "axis 5")
	eq(controls.bindings["roll"]["device"], -1, "any controller when at most one is connected")
	eq(controls.preset_name, "custom", "no longer a preset")
	var saved := Controls.new("keyboard")
	ok(saved.from_json(FileAccess.get_file_as_string(SAVE_PATH)), "the saved file loads")
	eq(saved.bindings["roll"], controls.bindings["roll"], "and holds the new binding")
	menu.queue_free()


func test_sticks_take_axes_only_and_switches_take_buttons_and_keys() -> void:
	var m := await _menu("gamepad")
	var menu = m[0]
	var controls = m[1]
	var before: Dictionary = controls.bindings["pitch"].duplicate()
	menu._start_listening("pitch")
	menu._input(_key_event(KEY_X))
	var button := InputEventJoypadButton.new()
	button.pressed = true
	button.button_index = 4
	menu._input(button)
	eq(controls.bindings["pitch"], before, "keys and buttons do not bind a stick")
	eq(menu.listening_for(), "pitch", "still listening")
	menu._input(_key_event(KEY_ESCAPE))
	eq(menu.listening_for(), "", "escape cancels")
	menu._start_listening("aux3")
	menu._input(button)
	eq(controls.bindings["aux3"]["kind"], "button", "a switch can take a button")
	eq(controls.bindings["aux3"]["index"], 4, "button 4")
	menu._start_listening("aux4")
	menu._input(_key_event(KEY_X))
	eq(controls.bindings["aux4"], {"kind": "key_toggle", "key": KEY_X}, "or a key")
	menu.queue_free()


func test_invert_preset_and_deadzone_change_the_controls_and_save() -> void:
	var m := await _menu("radio")
	var menu = m[0]
	var controls = m[1]
	menu._on_invert_toggled(true, "pitch")
	eq(controls.bindings["pitch"]["invert"], true, "inverted")
	menu._on_invert_toggled(true, "aux1")
	menu._on_preset_selected(1)
	eq(controls.preset_name, "gamepad", "the preset changes the bindings")
	eq(controls.bindings["throttle"]["invert"], true, "gamepad throttle is inverted by the preset")
	menu._on_deadzone_changed(0.12)
	near(controls.deadzone, 0.12, "deadzone")
	var saved := Controls.new("radio")
	ok(saved.from_json(FileAccess.get_file_as_string(SAVE_PATH)), "saved")
	near(saved.deadzone, 0.12, "the saved deadzone")
	eq(saved.preset_name, "gamepad", "the saved preset")
	menu.queue_free()


func test_the_live_bars_follow_the_sticks_only_while_open() -> void:
	var m := await _menu()
	var menu = m[0]
	var sticks := {"roll": 0.5, "pitch": -0.5, "yaw": 0.0, "throttle": 1.0, "aux": [1.0, -1.0, -1.0, -1.0]}
	menu.update_live(sticks)
	near(menu._rows["roll"]["live"].value, 0.5, "roll bar")
	near(menu._rows["throttle"]["live"].value, 1.0, "throttle bar (0..1 shown as -1..1)")
	near(menu._rows["aux1"]["live"].value, 1.0, "aux bar")
	menu.toggle()
	ok(not menu.is_open(), "closed")
	menu.update_live({"roll": -1.0, "pitch": 0.0, "yaw": 0.0, "throttle": 0.0, "aux": [-1.0, -1.0, -1.0, -1.0]})
	near(menu._rows["roll"]["live"].value, 0.5, "a closed screen does not update")
	menu.queue_free()


func test_the_video_effects_box_reports_changes_but_not_its_setup() -> void:
	var m := await _menu()
	var menu = m[0]
	var seen := []
	menu.video_effects_toggled.connect(func(on: bool) -> void: seen.append(on))
	menu.set_video_effects(false)
	ok(not menu.video_effects_checked(), "set from the settings")
	eq(seen, [], "without a signal")
	menu._video_effects.button_pressed = true
	eq(seen, [true], "a click is reported")
	menu.queue_free()
