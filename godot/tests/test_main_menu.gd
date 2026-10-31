extends "res://tests/testing.gd"

const AppSettings = preload("res://scripts/settings.gd")
const Menu = preload("res://ui/main_menu.gd")

const DIR := "user://menu-test"


func _write(path: String, text: String) -> void:
	DirAccess.make_dir_recursive_absolute(path.get_base_dir())
	FileAccess.open(path, FileAccess.WRITE).store_string(text)


func _remove(path: String) -> void:
	for sub in DirAccess.get_directories_at(path):
		_remove(path.path_join(sub))
	for file in DirAccess.get_files_at(path):
		DirAccess.remove_absolute(path.path_join(file))
	DirAccess.remove_absolute(path)


## A menu over a fake data dir holding one world with a course, one without, and two quads.
func _menu(overrides := {}, locks := {}) -> Array:
	if DirAccess.dir_exists_absolute(DIR):
		_remove(DIR)
	_write(DIR + "/data/worlds/zz-course.toml", "name = \"Zz course\"\n[[gates]]\nname = \"G0\"\n")
	_write(DIR + "/data/worlds/zz-field.toml", "name = \"Zz field\"\n")
	_write(DIR + "/data/quads/zz-a.toml", "name = \"Zz quad A\"\n")
	_write(DIR + "/data/quads/zz-b.toml", "name = \"Zz quad B\"\n")
	var settings := AppSettings.defaults()
	settings["data_dir"] = DIR + "/data"
	settings["sitl_launch"] = "wsl.exe -e betaflight"
	settings["world_path"] = DIR + "/data/worlds/zz-field.toml"
	settings["quad_path"] = DIR + "/data/quads/zz-a.toml"
	for key in overrides:
		settings[key] = overrides[key]
	var menu := Menu.new()
	await add_to_tree(menu)
	menu.setup(settings, locks)
	var events := []
	menu.choice_changed.connect(func(key, value):
		events.append([key, value])
		settings[key] = value)  # as the game does
	menu.start_requested.connect(func(): events.append(["start"]))
	menu.settings_requested.connect(func(): events.append(["settings"]))
	menu.controls_requested.connect(func(): events.append(["controls"]))
	menu.quit_requested.connect(func(): events.append(["quit"]))
	menu.open()
	return [menu, settings, events]


func test_the_rows_list_the_files_and_show_the_saved_choice() -> void:
	var m := await _menu()
	var menu = m[0]
	var worlds: Array = menu.row("world").values
	ok(worlds.has(DIR + "/data/worlds/zz-course.toml") and worlds.has(DIR + "/data/worlds/zz-field.toml"), "the data dir's worlds are listed")
	ok(worlds.size() > 2, "and the checkout's too: %s" % str(worlds))
	eq(menu.row("world").text, "World: Zz field", "the saved world is shown")
	eq(menu.row("quad").text, "Quad: Zz quad A", "the saved quad is shown")
	eq(menu.row("controller").text, "Flight controller: Betaflight", "Betaflight with a launch command")
	ok(menu.is_open(), "open")
	menu.free()


func test_choices_are_emitted_to_be_saved_and_the_course_row_follows_the_world() -> void:
	var m := await _menu()
	var menu = m[0]
	ok(not menu.row("mode").visible, "no race option for a world without gates")
	menu.row("world").set_value(DIR + "/data/worlds/zz-course.toml")
	menu.row("world").changed.emit(DIR + "/data/worlds/zz-course.toml")
	ok(menu.row("mode").visible, "a course offers race or free flight")
	menu.row("mode").step(1)
	menu.row("quad").set_value(DIR + "/data/quads/zz-b.toml")
	menu.row("quad").changed.emit(DIR + "/data/quads/zz-b.toml")
	menu.row("controller").step(1)
	eq(m[2], [["world_path", DIR + "/data/worlds/zz-course.toml"], ["race_start", false],
		["quad_path", DIR + "/data/quads/zz-b.toml"], ["open_loop", true]], "each change, in order")
	menu.close()
	menu.open()
	eq(menu.row("world").text, "World: Zz course", "the choice is remembered when the menu opens again")
	eq(menu.row("mode").text, "Flight mode: Free flight", "and the mode")
	menu.free()


func test_a_world_with_several_courses_offers_them() -> void:
	var m := await _menu({"world_path": DIR + "/data/worlds/zz-field.toml"})
	var menu = m[0]
	_write(DIR + "/data/worlds/zz-courses.toml", "name = \"Zz courses\"
[[gates]]
name = \"G0\"
[[courses]]
name = \"Line\"
[[courses]]
name = \"Hunt\"
")
	menu.open()
	ok(not menu.row("course").visible, "a world without courses: no course row")
	menu.row("world").set_value(DIR + "/data/worlds/zz-courses.toml")
	menu.row("world").changed.emit(DIR + "/data/worlds/zz-courses.toml")
	ok(menu.row("course").visible, "two courses: the row appears")
	eq(menu.row("course").text, "Course: Default", "the world's own course until the pilot picks")
	menu.row("course").step(2)
	eq(m[2].back(), ["course", "Hunt"], "a pick is emitted by name")
	menu.close()
	menu.open()
	eq(menu.row("course").text, "Course: Hunt", "and remembered")
	menu.row("world").set_value(DIR + "/data/worlds/zz-course.toml")
	menu.row("world").changed.emit(DIR + "/data/worlds/zz-course.toml")
	ok(not menu.row("course").visible, "one gate, no courses: no row")
	menu.free()


func test_the_buttons_emit() -> void:
	var m := await _menu()
	m[0].row("start").pressed.emit()
	m[0].row("controls").pressed.emit()
	m[0].row("settings").pressed.emit()
	m[0].row("quit").pressed.emit()
	eq(m[2], [["start"], ["controls"], ["settings"], ["quit"]], "start, controls, settings, quit")
	m[0].free()


func test_without_a_launch_command_the_flight_is_open_loop() -> void:
	var m := await _menu({"sitl_launch": ""})
	var menu = m[0]
	eq(menu.row("controller").text, "Flight controller: Open loop", "only open loop is offered")
	ok(menu.row("controller").disabled, "and it is locked")
	ok(menu.hint().contains("SITL launch command"), "with the way out: %s" % menu.hint())
	ok(Menu.effective_open_loop(m[1]), "the session runs open loop")
	m[1]["sitl_launch"] = "x"
	ok(not Menu.effective_open_loop(m[1]), "with a command, the saved choice (Betaflight) decides")
	menu.free()


func test_a_choice_a_flag_sets_is_locked_and_named() -> void:
	var m := await _menu({"open_loop": true}, {"world_path": "--world", "open_loop": "OFS_OPEN_LOOP"})
	var menu = m[0]
	ok(menu.row("world").disabled and menu.row("world").text.contains("(--world)"), "the world row: %s" % menu.row("world").text)
	ok(menu.row("controller").disabled and menu.row("controller").text.contains("OFS_OPEN_LOOP"), "the controller row: %s" % menu.row("controller").text)
	ok(not menu.row("quad").disabled, "the quad stays free")
	menu.free()


func test_a_world_outside_the_roots_is_kept() -> void:
	_write("user://menu-test-elsewhere/mine.toml", "name = \"Mine\"\n")
	var m := await _menu({"world_path": "user://menu-test-elsewhere/mine.toml"})
	eq(m[0].row("world").text, "World: Mine", "a world given by a flag or an earlier choice still shows")
	m[0].free()
	_remove("user://menu-test-elsewhere")
	_remove(DIR)
