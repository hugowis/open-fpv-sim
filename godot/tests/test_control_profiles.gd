extends "res://tests/testing.gd"

const Controls = preload("res://scripts/controls.gd")
const ControlProfiles = preload("res://scripts/control_profiles.gd")


func _store(preset := "radio"):
	return ControlProfiles.new(Controls.new(preset))


func test_a_new_store_has_one_default_profile_of_the_mapper() -> void:
	var p = _store("gamepad")
	eq(p.names(), ["Default"], "one profile")
	eq(p.active, "Default", "active")
	eq(p.profiles["Default"]["preset"], "gamepad", "holding the mapper's setup")


func test_new_profiles_start_from_a_preset_and_switching_keeps_each_setup() -> void:
	var p = _store("radio")
	p.controls.set_binding("roll", {"kind": "axis", "device": -1, "index": 5, "invert": true})
	eq(p.add("Pad", "gamepad"), "Pad", "the name comes back")
	eq(p.active, "Pad", "the new profile is active")
	eq(p.controls.preset_name, "gamepad", "with the preset's bindings")
	p.controls.shaping = {"roll": {"travel": 0.8}}
	ok(p.switch_to("Default"), "back to the first")
	eq(p.controls.bindings["roll"]["index"], 5, "its edit was kept")
	eq(p.controls.shaping, {}, "and it has no shaping")
	ok(p.switch_to("Pad"), "and forth")
	eq(p.controls.shaping, {"roll": {"travel": 0.8}}, "the other's shaping was kept")
	ok(not p.switch_to("Nope"), "an unknown name is refused")
	eq(p.active, "Pad", "and changes nothing")


func test_names_are_trimmed_and_made_unique() -> void:
	var p = _store()
	eq(p.add("  Pad  ", "gamepad"), "Pad", "trimmed")
	eq(p.add("Pad", "gamepad"), "Pad 2", "a taken name gets a number")
	eq(p.add("", "radio"), "Profile", "an empty name gets a default")
	eq(p.add("Default", "radio"), "Default 2", "the first profile's name is taken too")
	eq(p.names(), ["Default", "Pad", "Pad 2", "Profile", "Default 2"], "in creation order")


func test_the_last_profile_cannot_be_deleted_and_deleting_the_active_one_switches() -> void:
	var p = _store("radio")
	ok(not p.remove("Default"), "the only profile stays")
	p.add("Pad", "gamepad")
	ok(p.remove("Pad"), "the active one goes")
	eq(p.active, "Default", "and the first takes over")
	eq(p.controls.preset_name, "radio", "loaded into the mapper")
	ok(not p.remove("Pad"), "already gone")


func test_a_round_trip_keeps_profiles_the_active_one_and_the_calibration() -> void:
	var a = _store("radio")
	a.add("Pad", "gamepad")
	a.controls.shaping = {"pitch": {"deadband": 0.1}}
	a.controls.calibration = {"g": {"name": "Pad", "axes": {"0": {"min": -0.8, "center": 0.0, "max": 0.9}}}}
	var b = _store("keyboard")
	ok(b.from_json(a.to_json()), "loads")
	eq(b.names(), ["Default", "Pad"], "profiles")
	eq(b.active, "Pad", "active")
	eq(b.controls.preset_name, "gamepad", "the active one is in the mapper")
	eq(b.controls.shaping, {"pitch": {"deadband": 0.1}}, "with its shaping")
	near(b.controls.calibration["g"]["axes"]["0"]["max"], 0.9, "calibration")


func test_the_old_bindings_file_becomes_the_default_profile() -> void:
	var old := Controls.new("gamepad")
	old.deadzone = 0.07
	var legacy := JSON.stringify({"version": 1, "preset": "gamepad", "deadzone": 0.07, "bindings": old.bindings})
	var p = _store("keyboard")
	p.add("Extra", "radio")
	ok(p.from_json(legacy), "loads")
	eq(p.names(), ["Default"], "one profile")
	eq(p.active, "Default", "active")
	eq(p.controls.bindings, old.bindings, "bindings as they were")
	near(p.controls.deadzone, 0.07, "deadzone as it was")
	eq(p.controls.preset_name, "gamepad", "preset name")


func test_a_file_with_a_bad_profile_loads_the_good_ones_and_a_bad_file_changes_nothing() -> void:
	var good := Controls.new("radio").to_profile()
	var text := JSON.stringify({"version": 2, "active": "Missing", "profiles": {"Bad": {"bindings": 3}, "Good": good}})
	var p = _store("keyboard")
	ok(p.from_json(text), "the usable profile loads")
	eq(p.names(), ["Good"], "the bad one is dropped")
	eq(p.active, "Good", "an unknown active name falls back to the first")
	var before: String = p.to_json()
	for junk in ["", "nope", "[]", "{}", JSON.stringify({"profiles": {}}), JSON.stringify({"profiles": {"Bad": 3}}),
			JSON.stringify({"profiles": 3})]:
		ok(not p.from_json(junk), "rejects %s" % junk)
	eq(p.to_json(), before, "nothing changed")


func test_calibration_in_a_file_is_cleaned() -> void:
	var p = _store()
	var good := Controls.new("radio").to_profile()
	var text := JSON.stringify({"version": 2, "active": "A", "profiles": {"A": good},
		"calibration": {"g": {"name": "P", "axes": {"0": {"min": 1, "center": 0, "max": 1}}}}})
	ok(p.from_json(text), "loads")
	eq(p.controls.calibration, {}, "an impossible axis is dropped, and its controller with it")
