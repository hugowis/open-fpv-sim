extends "res://tests/testing.gd"

const Catalog = preload("res://scripts/catalog.gd")

const DIR := "user://catalog-test"


func _write(path: String, text: String) -> void:
	DirAccess.make_dir_recursive_absolute(path.get_base_dir())
	var file := FileAccess.open(path, FileAccess.WRITE)
	file.store_string(text)


func _fresh() -> void:
	if DirAccess.dir_exists_absolute(DIR):
		_remove(DIR)


func _remove(path: String) -> void:
	for sub in DirAccess.get_directories_at(path):
		_remove(path.path_join(sub))
	for file in DirAccess.get_files_at(path):
		DirAccess.remove_absolute(path.path_join(file))
	DirAccess.remove_absolute(path)


func test_lists_the_toml_files_of_every_root_sorted_by_label() -> void:
	_fresh()
	_write(DIR + "/repo/worlds/zeta.toml", "schema_version = 1\nname = \"Zeta field\"\n")
	_write(DIR + "/repo/worlds/alpha.toml", "name = \"Alpha\"\n[[gates]]\nname = \"G0\"\n[[gates]]\nname = \"G1\"\n")
	_write(DIR + "/repo/worlds/notes.txt", "not a world")
	_write(DIR + "/data/worlds/mine.toml", "schema_version = 1\n")
	var items := Catalog.list([DIR + "/repo/worlds", DIR + "/data/worlds", DIR + "/nowhere/worlds"])
	eq(items.map(func(i): return i["label"]), ["Alpha", "mine", "Zeta field"], "sorted labels; a file without a name uses its stem")
	eq(items[0]["gates"], 2, "Alpha declares two gates")
	eq(items[1]["gates"], 0, "a world without gates")
	eq(items[0]["path"], DIR + "/repo/worlds/alpha.toml", "the path")
	_fresh()


func test_a_name_inside_a_table_is_not_the_label_and_a_file_in_two_roots_is_listed_once() -> void:
	_fresh()
	_write(DIR + "/q/a.toml", "schema_version = 6\n[battery]\nname = \"Not me\"\n")
	var items := Catalog.list([DIR + "/q", DIR + "/q/../q"])
	eq(items.size(), 1, "listed once")
	eq(items[0]["label"], "a", "the stem, not a table's name")
	_fresh()


func test_a_chosen_path_outside_the_roots_stays_in_the_list() -> void:
	_fresh()
	_write(DIR + "/r/one.toml", "name = \"One\"\n")
	_write(DIR + "/elsewhere/other.toml", "name = \"Other\"\n")
	var items := Catalog.list([DIR + "/r"])
	var withc := Catalog.with_current(items, DIR + "/elsewhere/other.toml")
	eq(withc.size(), 2, "the custom path is added")
	eq(Catalog.index_of(withc, DIR + "/elsewhere/other.toml"), 1, "and found")
	eq(Catalog.with_current(items, DIR + "/r/one.toml").size(), 1, "a listed path is not duplicated")
	eq(Catalog.index_of(withc, "nope.toml"), 0, "an unknown path falls back to the first entry")
	eq(Catalog.with_current(items, "").size(), 1, "an empty path adds nothing")
	_fresh()


func test_roots_are_the_checkout_and_the_data_dir() -> void:
	var roots := Catalog.roots({"data_dir": "C:/x/data"}, "quads")
	eq(roots.size(), 2, "two roots")
	ok(roots[0].ends_with("/quads") and roots[1] == "C:/x/data/quads", "roots: %s" % str(roots))
