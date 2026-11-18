extends RefCounted
## Local best/last laps and the best lap's ghost (#133). The store is
## `{"version": 1, "worlds": {world: {course: {quad: {best_lap_s, best_at, last_lap_s, last_at, ghost}}}}}`
## in `user://records.json`; the settings chain is not involved.

const RECORDS_PATH := "user://records.json"


static func load_records(path := RECORDS_PATH) -> Dictionary:
	if not FileAccess.file_exists(path):
		return {}
	var file := FileAccess.open(path, FileAccess.READ)
	if file == null:
		return {}
	var text := file.get_as_text()
	file.close()
	var json := JSON.new()
	if json.parse(text) != OK:
		return {}
	var data = json.data
	if not data is Dictionary:
		return {}
	if int(data.get("version", 0)) != 1:
		return {}
	if not data.get("worlds", {}) is Dictionary:
		return {}
	return data


## Writes through a temp file and renames it over the old one: a crash cannot truncate every record.
static func save_records(store: Dictionary, path := RECORDS_PATH) -> bool:
	var tmp := path + ".tmp"
	var file := FileAccess.open(tmp, FileAccess.WRITE)
	if file == null:
		return false
	file.store_string(JSON.stringify(store, "\t"))
	file.close()
	var from := ProjectSettings.globalize_path(tmp)
	var to := ProjectSettings.globalize_path(path)
	if DirAccess.rename_absolute(from, to) != OK:
		DirAccess.remove_absolute(from)
		return false
	return true


static func entry(store: Dictionary, world: String, course: String, quad: String) -> Dictionary:
	var worlds = store.get("worlds", {})
	if not worlds is Dictionary:
		return {}
	var courses = worlds.get(world, {})
	if not courses is Dictionary:
		return {}
	var quads = courses.get(course, {})
	if not quads is Dictionary:
		return {}
	var e = quads.get(quad, {})
	return e if e is Dictionary else {}


static func best_lap_s(store: Dictionary, world: String, course: String, quad: String) -> float:
	return float(entry(store, world, course, quad).get("best_lap_s", 0.0))


static func record_last(store: Dictionary, world: String, course: String, quad: String, lap_s: float) -> void:
	var e := _ensure(store, world, course, quad)
	e["last_lap_s"] = snappedf(lap_s, 0.001)
	e["last_at"] = Time.get_datetime_string_from_system(true)


## True when `capture` became the best: absent or strictly faster. The ghost is the capture's track.
static func offer_capture(store: Dictionary, world: String, course: String, quad: String, capture: Dictionary) -> bool:
	# Compare at the stored precision: a lap equal to the best after rounding is not strictly faster.
	var lap_s := snappedf(float(capture.get("lap_s", 0.0)), 0.001)
	if lap_s <= 0.0:
		return false
	var e := _ensure(store, world, course, quad)
	var best := float(e.get("best_lap_s", 0.0))
	if best > 0.0 and lap_s >= best:
		return false
	e["best_lap_s"] = lap_s
	e["best_at"] = Time.get_datetime_string_from_system(true)
	e["ghost"] = {
		"lap_s": lap_s,
		"times": _rounded(capture.get("times", []), 0.001),
		"positions": _rounded(capture.get("positions", []), 0.001),
		"rotations": _rounded(capture.get("rotations", []), 0.0001),
	}
	return true


static func ghost_of(store: Dictionary, world: String, course: String, quad: String) -> Dictionary:
	var ghost = entry(store, world, course, quad).get("ghost", {})
	return ghost if ghost is Dictionary else {}


static func _ensure(store: Dictionary, world: String, course: String, quad: String) -> Dictionary:
	if not store.has("version"):
		store["version"] = 1
	var worlds = store.get("worlds")
	if not worlds is Dictionary:
		worlds = {}
		store["worlds"] = worlds
	var courses = worlds.get(world)
	if not courses is Dictionary:
		courses = {}
		worlds[world] = courses
	var quads = courses.get(course)
	if not quads is Dictionary:
		quads = {}
		courses[course] = quads
	var e = quads.get(quad)
	if not e is Dictionary:
		e = {}
		quads[quad] = e
	return e


static func _rounded(values, step: float) -> Array:
	var out: Array = []
	for value in values:
		out.append(snappedf(float(value), step))
	return out
