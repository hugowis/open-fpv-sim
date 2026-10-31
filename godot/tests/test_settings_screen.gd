extends "res://tests/testing.gd"

const AppSettings = preload("res://scripts/settings.gd")
const Screen = preload("res://ui/settings_screen.gd")


func _screen(locks := {}) -> Array:
	var settings := AppSettings.defaults()
	settings["server_bin"] = "C:/x/ofs-sim.exe"
	var screen := Screen.new()
	await add_to_tree(screen)
	screen.setup(settings, locks)
	var changes := []
	screen.setting_changed.connect(func(key, value):
		changes.append([key, value])
		settings[key] = value)  # as the game does
	return [screen, settings, changes]


func _esc() -> InputEventKey:
	var e := InputEventKey.new()
	e.physical_keycode = KEY_ESCAPE
	e.pressed = true
	return e


func test_every_row_shows_the_effective_setting() -> void:
	var s := await _screen()
	var screen = s[0]
	s[1]["video_effects"] = false
	s[1]["dvr_feed"] = "both"
	s[1]["sitl_launch"] = "wsl.exe -e x"
	screen.open()
	eq(screen.control("video_effects").button_pressed, false, "video effects off")
	eq(screen.control("fpv_aspect").value(), "4:3", "picture")
	eq(screen.control("dvr_feed").value(), "both", "dvr feed")
	eq(screen.control("hud_visible").button_pressed, true, "the HUD default")
	eq(screen.control("server_bin").text, "C:/x/ofs-sim.exe", "binary")
	eq(screen.control("sitl_launch").text, "wsl.exe -e x", "SITL launch")
	eq(screen.note("server_bin"), "next Start", "a setting the next Start applies says so")
	eq(screen.note("video_effects"), "", "a live one says nothing")
	eq(s[2], [], "showing a value emits nothing")
	screen.queue_free()


func test_an_edit_emits_the_key_and_the_value() -> void:
	var s := await _screen()
	var screen = s[0]
	screen.control("fullscreen").button_pressed = true
	screen.control("dvr_export").step(1)
	screen.control("server_addr").text = "127.0.0.1:6000"
	screen.control("server_addr").text_submitted.emit("127.0.0.1:6000")
	screen.control("server_addr").text_submitted.emit("127.0.0.1:6000")
	eq(s[2], [["fullscreen", true], ["dvr_export", "frames"], ["server_addr", "127.0.0.1:6000"]], "three edits, the text once per distinct value")
	screen.queue_free()


func test_text_that_does_not_change_is_not_a_change() -> void:
	var s := await _screen()
	s[0].control("server_bin").text_submitted.emit("C:/x/ofs-sim.exe")
	eq(s[2], [], "the same text emits nothing")
	s[0].queue_free()


func test_a_row_a_flag_or_variable_sets_is_locked_and_says_why() -> void:
	var s := await _screen({"open_loop": "--open-loop", "server_addr": "OFS_SERVER_ADDR"})
	var screen = s[0]
	s[1]["open_loop"] = true
	screen.open()
	ok(screen.control("open_loop").disabled, "the box is disabled")
	eq(screen.control("open_loop").button_pressed, true, "but shows the effective value")
	eq(screen.note("open_loop"), "set by --open-loop this run", "the flag is named")
	ok(not screen.control("server_addr").editable, "the text row cannot be edited")
	eq(screen.note("server_addr"), "set by OFS_SERVER_ADDR this run", "the variable is named")
	screen.control("server_addr").text_submitted.emit("other:1")
	eq(s[2], [], "a locked text row emits nothing")
	ok(not screen.control("video_effects").disabled, "other rows stay editable")
	screen.queue_free()


func test_escape_and_back_close_it() -> void:
	var s := await _screen()
	var screen = s[0]
	var closed := [0]
	screen.closed.connect(func(): closed[0] += 1)
	screen.open()
	ok(screen.is_open(), "open")
	screen._input(_esc())
	ok(not screen.is_open(), "Escape closes it")
	screen.open()
	screen._back.pressed.emit()
	ok(not screen.is_open(), "Back closes it")
	eq(closed[0], 2, "twice announced")
	screen.queue_free()


func test_the_sound_row_is_a_live_check_box() -> void:
	var s := await _screen()
	var screen = s[0]
	ok(screen.control("audio") is CheckBox, "a check box")
	eq(screen.control("audio").button_pressed, true, "sound shown on by default")
	eq(screen.note("audio"), "", "a live row says nothing")
	screen.control("audio").button_pressed = false
	eq(s[2], [["audio", false]], "toggling emits the key and the value")
	screen.queue_free()
