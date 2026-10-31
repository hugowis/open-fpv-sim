extends "res://tests/testing.gd"

const Controls = preload("res://scripts/controls.gd")
const ControlProfiles = preload("res://scripts/control_profiles.gd")
const Menu = preload("res://ui/controls_menu.gd")
const App = preload("res://scripts/app.gd")

const SAVE_PATH := "user://test_controls.json"


func _menu(preset := "radio") -> Array:
	var controls := Controls.new(preset)
	var menu := Menu.new()
	await add_to_tree(menu)
	DirAccess.remove_absolute(SAVE_PATH)
	menu.setup(ControlProfiles.new(controls), SAVE_PATH)
	menu.toggle()
	return [menu, controls]


## What the file on disk holds, loaded back through a fresh store.
func _saved():
	var store := ControlProfiles.new(Controls.new("keyboard"))
	ok(store.from_json(FileAccess.get_file_as_string(SAVE_PATH)), "the saved file loads")
	return store


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
	eq(_saved().controls.bindings["roll"], controls.bindings["roll"], "and holds the new binding")
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
	var saved = _saved().controls
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


func test_the_dvr_toggle_reports_changes_but_not_its_setup() -> void:
	var m := await _menu()
	var menu = m[0]
	ok(menu._dvr is CheckBox, "the DVR control exists")
	eq(menu._dvr.text, "DVR: record the FPV view", "its label")
	var seen := []
	menu.dvr_toggled.connect(func(on: bool) -> void: seen.append(on))
	menu.set_dvr(true)
	ok(menu.dvr_on(), "set from the DVR state")
	eq(seen, [], "without a signal")
	menu._dvr.button_pressed = false
	eq(seen, [false], "a click off is reported once")
	menu._dvr.button_pressed = true
	eq(seen, [false, true], "a click on is reported once")
	menu.set_dvr(false)
	ok(not menu.dvr_on(), "set off")
	eq(seen, [false, true], "set_dvr never emits")
	menu.queue_free()


func test_the_aspect_button_cycles_from_the_set_value() -> void:
	var m := await _menu()
	var menu = m[0]
	var seen := []
	menu.aspect_changed.connect(func(value: String) -> void: seen.append(value))
	menu.set_aspect("16:9")
	eq(menu._aspect.text, "Picture: 16:9", "set from the settings")
	eq(seen, [], "without a signal")
	menu._aspect.pressed.emit()
	eq(seen, ["window"], "pressing continues the cycle from the set value")
	eq(menu._aspect.text, "Picture: window", "and the label follows")
	menu.set_aspect("vhs")
	eq(menu._aspect.text, "Picture: 4:3", "an unknown value falls back to index 0")
	menu.queue_free()


func test_the_dvr_export_button_cycles_and_shows_a_set_value_silently() -> void:
	var menu = (await _menu())[0]
	var seen := []
	menu.dvr_export_changed.connect(func(value: String) -> void: seen.append(value))
	eq(menu._dvr_export.text, "DVR export: none", "starts at none")
	for i in 4:
		menu._dvr_export.pressed.emit()
	eq(seen, ["frames", "mp4", "both", "none"], "none -> frames -> mp4 -> both -> none")
	eq(menu._dvr_export.text, "DVR export: none", "and the label follows")
	seen.clear()
	menu.set_dvr_export("mp4")
	eq(menu._dvr_export.text, "DVR export: mp4", "set from the settings")
	eq(seen, [], "without a signal")
	menu._dvr_export.pressed.emit()
	eq(seen, ["both"], "pressing continues the cycle from the set value")
	menu.set_dvr_export("zip")
	eq(menu._dvr_export.text, "DVR export: none", "an unknown value falls back to none")
	menu.queue_free()


func test_the_dvr_feed_button_cycles_and_shows_a_set_value_silently() -> void:
	var menu = (await _menu())[0]
	var seen := []
	menu.dvr_feed_changed.connect(func(value: String) -> void: seen.append(value))
	eq(menu._dvr_feed.text, "DVR feed: goggles", "starts at goggles")
	for i in 3:
		menu._dvr_feed.pressed.emit()
	eq(seen, ["clean", "both", "goggles"], "goggles -> clean -> both -> goggles")
	eq(menu._dvr_feed.text, "DVR feed: goggles", "and the label follows")
	seen.clear()
	menu.set_dvr_feed("clean")
	eq(menu._dvr_feed.text, "DVR feed: clean", "set from the settings")
	eq(seen, [], "without a signal")
	menu._dvr_feed.pressed.emit()
	eq(seen, ["both"], "pressing continues the cycle from the set value")
	menu.set_dvr_feed("zip")
	eq(menu._dvr_feed.text, "DVR feed: goggles", "an unknown value falls back to goggles")
	menu.queue_free()


func test_a_junk_setting_resolves_to_the_4x3_label() -> void:
	# The game clamps the pass-through setting before showing it (app.gd), so the label and the layout agree.
	var clamped: String = App.clamp_aspect("vhs")
	eq(clamped, "4:3", "an unknown setting falls back to the 4:3 default")
	eq(App.clamp_aspect("window"), "window", "a known setting passes through")
	var menu = (await _menu())[0]
	menu.set_aspect(clamped)
	eq(menu._aspect.text, "Picture: 4:3", "the label agrees with the clamped setting")
	menu.queue_free()


func test_the_fpv_box_fits_the_aspect_and_centers() -> void:
	var wide := App.fpv_box_for(Vector2(1600, 800), "4:3", false)
	ok((wide["box"] as Rect2).is_equal_approx(Rect2(266.666, 0, 1066.666, 800)), "a wide window: height-fitted 4:3, centred")
	eq(wide["internal"], Vector2i(960, 720), "the boxed aspect renders at its fixed resolution")
	var tall := App.fpv_box_for(Vector2(600, 900), "16:9", false)
	eq((tall["box"] as Rect2).size, Vector2(600, 337.5), "a tall window: width-fitted 16:9")
	var window_mode := App.fpv_box_for(Vector2(1600, 800), "window", false)
	eq(window_mode["box"], Rect2(0, 0, 1600, 800), "the window aspect fills the window")
	eq(window_mode["internal"], Vector2i(1600, 800), "at the window's own resolution")
	var chase := App.fpv_box_for(Vector2(1600, 800), "4:3", true)
	eq(chase["box"], Rect2(0, 0, 1600, 800), "the chase view always fills the window")


func test_a_shaping_slider_sets_the_channel_and_saves() -> void:
	var m := await _menu()
	var menu = m[0]
	var controls = m[1]
	near(menu._rows["roll"]["deadband"].value, 0.02, "starts at the default deadzone")
	near(menu._rows["roll"]["travel"].value, 1.0, "and full travel")
	menu._rows["roll"]["travel"].value = 0.8
	menu._rows["pitch"]["deadband"].value = 0.1
	eq(controls.shaping, {"roll": {"travel": 0.8}, "pitch": {"deadband": 0.1}}, "stored per channel")
	var saved = _saved().controls
	eq(saved.shaping, controls.shaping, "and saved")
	menu._on_deadzone_changed(0.05)
	near(menu._rows["roll"]["deadband"].value, 0.05, "a channel without its own deadband follows the default")
	near(menu._rows["pitch"]["deadband"].value, 0.1, "one with its own keeps it")
	ok(not menu._rows.has("throttle") or not menu._rows["throttle"].has("travel"), "throttle has no shaping sliders")
	menu.queue_free()


func test_profiles_are_created_switched_and_deleted_from_the_screen() -> void:
	var m := await _menu("radio")
	var menu = m[0]
	var controls = m[1]
	ok(menu._delete.disabled, "the only profile cannot be deleted")
	menu._profile_name.text = "Pad"
	menu._new.pressed.emit()
	eq(menu.store.active, "Pad", "the new profile is active")
	eq(menu._profile_name.text, "", "the name field is cleared")
	eq(menu._profile.item_count, 2, "listed")
	eq(menu._profile.get_item_text(menu._profile.selected), "Pad", "and selected")
	menu._on_invert_toggled(true, "pitch")
	menu._on_profile_selected(0)
	eq(menu.store.active, "Default", "back to the first")
	ok(not controls.bindings["pitch"].get("invert", false), "its own pitch")
	var saved = _saved()
	eq(saved.names(), ["Default", "Pad"], "both are saved")
	eq(saved.profiles["Pad"]["bindings"]["pitch"]["invert"], true, "with the other's edit")
	menu._on_profile_selected(1)
	menu._delete.pressed.emit()
	eq(menu.store.names(), ["Default"], "deleted")
	eq(menu._profile.item_count, 1, "and gone from the list")
	menu.queue_free()


func test_a_calibration_result_is_merged_and_saved() -> void:
	var m := await _menu()
	var menu = m[0]
	var controls = m[1]
	menu._on_calibrated("guid-x", "Radio X", {"0": {"min": -0.8, "center": 0.0, "max": 0.9}})
	near(controls.calibration["guid-x"]["axes"]["0"]["max"], 0.9, "merged into the mapper")
	near(_saved().controls.calibration["guid-x"]["axes"]["0"]["min"], -0.8, "and saved")
	ok(menu._hint.text.contains("Calibrated 1 axes"), "the hint says so")
	menu.queue_free()


func test_the_stick_view_follows_the_live_sticks() -> void:
	var menu = (await _menu())[0]
	var sticks := {"roll": 0.5, "pitch": -0.5, "yaw": 0.0, "throttle": 1.0, "aux": [1.0, -1.0, -1.0, -1.0]}
	menu.update_live(sticks)
	eq(menu._sticks_view.sticks, sticks, "the same sticks the bars show")
	menu.queue_free()


func test_opened_from_the_menu_it_closes_on_cancel_and_reports_it() -> void:
	var menu = (await _menu())[0]
	menu.toggle()  # _menu() leaves it open; start from closed
	var seen := []
	menu.closed.connect(func(from_menu: bool) -> void: seen.append(from_menu))
	menu.open_from_menu()
	ok(menu.is_open() and menu.is_menu_mode(), "open in menu mode")
	var esc := InputEventKey.new()
	esc.keycode = KEY_ESCAPE
	esc.physical_keycode = KEY_ESCAPE
	esc.pressed = true
	menu._start_listening("roll")
	menu._input(esc)
	ok(menu.is_open() and menu.listening_for() == "", "escape first cancels a binding")
	menu._input(esc)
	ok(not menu.is_open() and not menu.is_menu_mode(), "then closes")
	eq(seen, [true], "and tells the game it came from the menu")
	menu.toggle()
	menu.toggle()
	eq(seen, [true, false], "a flight's F2 screen reports false")
	menu.queue_free()


func test_calibrate_is_disabled_without_a_controller() -> void:
	var menu = (await _menu())[0]
	if Input.get_connected_joypads().is_empty():
		ok(menu._calibrate.disabled, "nothing to calibrate")
		ok(menu._pads.text.contains("none connected"), "the controller line says so")
	menu.queue_free()
