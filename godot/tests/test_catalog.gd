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
	eq(items[0]["courses"], [], "no courses declared")
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

func test_the_courses_of_a_world_are_listed_in_file_order() -> void:
	_fresh()
	_write(DIR + "/repo/worlds/park.toml", "name = \"Park\"
[[gates]]
name = \"G0\"
[[courses]]
name = \"Diamond\"
gates = [\"G0\"]
[[courses]]
name = \"Hunt\"
mode = \"time_attack\"
[[emitters]]
name = \"tx\"
")
	var item := Catalog.entry(DIR + "/repo/worlds/park.toml")
	eq([item["label"], item["gates"], item["courses"]], ["Park", 1, ["Diamond", "Hunt"]], "the world's name stays its label, and a gate or emitter is not a course")
	_fresh()


func test_the_fc_kind_of_a_quad_file_is_scanned() -> void:
	_fresh()
	_write(DIR + "/q/sitl.toml", "name = \"A\"\n[fc]\nkind = \"sitl\"\n")
	_write(DIR + "/q/stab.toml", "name = \"B\"\n[fc]\nkind = \"open_loop_stabilized\"\n# kind = \"open_loop\"\n")
	_write(DIR + "/q/outside.toml", "kind = \"open_loop_stabilized\"\n[fc]\nkind = \"sitl\"\n")
	_write(DIR + "/q/kindred.toml", "[fc]\nkindred = \"open_loop_stabilized\"\nkind = \"open_loop\"\n")
	_write(DIR + "/w/field.toml", "name = \"Field\"\n[[gates]]\nname = \"G0\"\n")
	eq(Catalog.fc_kind(DIR + "/q/sitl.toml"), "sitl", "a sitl quad")
	eq(Catalog.fc_kind(DIR + "/q/stab.toml"), "open_loop_stabilized", "a stabilized quad, comments ignored")
	eq(Catalog.fc_kind(DIR + "/q/outside.toml"), "sitl", "a kind outside [fc] does not count")
	eq(Catalog.fc_kind(DIR + "/q/kindred.toml"), "open_loop", "only a key named kind counts")
	eq(Catalog.fc_kind(DIR + "/w/field.toml"), "", "a world file has no [fc]")
	eq(Catalog.fc_kind(DIR + "/q/missing.toml"), "", "a missing file has no kind")
	ok(Catalog.quad_is_stabilized(DIR + "/q/stab.toml"), "the predicate names the stabilized file")
	ok(not Catalog.quad_is_stabilized(DIR + "/q/missing.toml"), "and nothing else")
	_fresh()
