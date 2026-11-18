extends "res://tests/testing.gd"
## The local records store (#133).

const Records = preload("res://scripts/records.gd")
const PATH := "user://tmp-records-test.json"


func _clean() -> void:
	for path in [PATH, PATH + ".tmp"]:
		if FileAccess.file_exists(path):
			DirAccess.remove_absolute(ProjectSettings.globalize_path(path))


func _capture(lap_s: float) -> Dictionary:
	return {
		"lap_s": lap_s,
		"times": PackedFloat32Array([0.0, 0.5, 1.0]),
		"positions": PackedFloat32Array([1.0, 2.0, -3.0, 1.5, 2.25, -3.5, 2.0, 2.5, -4.0]),
		"rotations": PackedFloat32Array([0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]),
	}


func test_a_round_trip_keeps_every_number() -> void:
	_clean()
	var store := {}
	Records.record_last(store, "Flat", "", "Quad", 31.234)
	ok(Records.offer_capture(store, "Flat", "", "Quad", _capture(29.876)), "the first capture is the best")
	ok(Records.save_records(store, PATH), "saved")
	var loaded := Records.load_records(PATH)
	near(Records.best_lap_s(loaded, "Flat", "", "Quad"), 29.876, "best survives", 0.001)
	near(float(Records.entry(loaded, "Flat", "", "Quad").get("last_lap_s", 0.0)), 31.234, "last survives", 0.001)
	var ghost := Records.ghost_of(loaded, "Flat", "", "Quad")
	eq(int(ghost.get("times", []).size()), 3, "times")
	eq(int(ghost.get("positions", []).size()), 9, "positions")
	eq(int(ghost.get("rotations", []).size()), 12, "rotations")
	near(float(ghost["times"][1]), 0.5, "a time", 0.001)
	near(float(ghost["positions"][1]), 2.0, "a position", 0.001)
	_clean()


func test_a_missing_or_corrupt_file_starts_empty() -> void:
	_clean()
	eq(Records.load_records(PATH), {}, "a missing file is an empty store")
	var file := FileAccess.open(PATH, FileAccess.WRITE)
	file.store_string("not json")
	file.close()
	eq(Records.load_records(PATH), {}, "a corrupt file is an empty store")
	eq(FileAccess.get_file_as_string(PATH), "not json", "the corrupt file is kept")
	_clean()


func test_the_first_capture_becomes_the_best_and_keeps_its_ghost() -> void:
	var store := {}
	ok(Records.offer_capture(store, "Flat", "", "Quad", _capture(29.9)), "the first capture is the best")
	near(Records.best_lap_s(store, "Flat", "", "Quad"), 29.9, "best", 0.001)
	var ghost := Records.ghost_of(store, "Flat", "", "Quad")
	near(float(ghost.get("lap_s", 0.0)), 29.9, "ghost lap", 0.001)
	eq(int(ghost.get("times", []).size()), 3, "the ghost's times")


func test_only_a_strictly_faster_lap_replaces_the_ghost() -> void:
	var store := {}
	ok(Records.offer_capture(store, "Flat", "", "Quad", _capture(29.9)), "the first is the best")
	ok(not Records.offer_capture(store, "Flat", "", "Quad", _capture(31.0)), "a slower lap does not")
	near(Records.best_lap_s(store, "Flat", "", "Quad"), 29.9, "the best is unchanged", 0.001)
	ok(not Records.offer_capture(store, "Flat", "", "Quad", _capture(29.9)), "an equal lap does not")
	ok(Records.offer_capture(store, "Flat", "", "Quad", _capture(28.0)), "a faster lap does")
	near(Records.best_lap_s(store, "Flat", "", "Quad"), 28.0, "the best is replaced", 0.001)
	near(float(Records.ghost_of(store, "Flat", "", "Quad").get("lap_s", 0.0)), 28.0, "the ghost is replaced", 0.001)


func test_record_last_updates_every_lap() -> void:
	var store := {}
	Records.record_last(store, "Flat", "", "Quad", 31.2)
	Records.record_last(store, "Flat", "", "Quad", 30.8)
	near(float(Records.entry(store, "Flat", "", "Quad").get("last_lap_s", 0.0)), 30.8, "the second last", 0.001)
	eq(Records.best_lap_s(store, "Flat", "", "Quad"), 0.0, "no best from a last lap")


func test_the_unnamed_course_is_its_own_key() -> void:
	var store := {}
	Records.offer_capture(store, "Flat", "", "Quad", _capture(30.0))
	Records.offer_capture(store, "Flat", "Hunt", "Quad", _capture(29.0))
	near(Records.best_lap_s(store, "Flat", "", "Quad"), 30.0, "the unnamed course", 0.001)
	near(Records.best_lap_s(store, "Flat", "Hunt", "Quad"), 29.0, "the named course", 0.001)


func test_the_save_replaces_the_old_file_without_a_temp_left() -> void:
	_clean()
	var first := {}
	Records.record_last(first, "Flat", "", "Quad", 31.0)
	ok(Records.save_records(first, PATH), "the first save")
	var second := {}
	Records.record_last(second, "Flat", "", "Quad", 32.0)
	ok(Records.save_records(second, PATH), "the second save replaces the first")
	ok(not FileAccess.file_exists(PATH + ".tmp"), "no temp file is left")
	near(float(Records.entry(Records.load_records(PATH), "Flat", "", "Quad").get("last_lap_s", 0.0)), 32.0, "the new record", 0.001)
	_clean()
