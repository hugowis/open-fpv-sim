extends RefCounted
## The geometry a quad's frame part carries, read the way catalog.gd reads a world file: a line scan of the
## TOML, no parser. The simulator resolves library parts through ofs-config's library_roots (a `library/`
## beside the quads that reference it); this is the drawing side's mirror of that rule.

const AppSettings = preload("res://scripts/settings.gd")

## The 5F arm the glTF model is authored for (library/frames/opendrone-5f.toml), and the fallback for anything
## unreadable.
const DEFAULT_ARM_M := 0.08
const DEFAULT_SPIN: Array = [1.0, -1.0, -1.0, 1.0]


## The frame's motor arm in metres (the first motor position's larger sideways coordinate). A degenerate row
## (zeros) falls back with the unreadable ones: a zero arm would zero-scale the model away.
static func arm_m(quad_path: String) -> float:
	var row := _first_row(frame_part(quad_path), "motor_positions_frd_m")
	if row.size() < 2:
		return DEFAULT_ARM_M
	var arm: float = maxf(absf(row[0]), absf(row[1]))
	return arm if arm > 0.0 else DEFAULT_ARM_M


## The frame's propeller directions, +1 clockwise seen from above.
static func spin(quad_path: String) -> Array:
	var values := _numbers(frame_part(quad_path), "motor_spin")
	return Array(values) if values.size() == 4 else DEFAULT_SPIN


## The frame part the quad file references, among the library roots; "" when it is not on disk.
static func frame_part(quad_path: String) -> String:
	var id := _top_level_string(quad_path, "frame")
	if id == "":
		return ""
	for root in roots(quad_path):
		var path: String = String(root).path_join("frames").path_join(id + ".toml")
		if FileAccess.file_exists(path):
			return path
	return ""


## Where the frame part is looked for: the `library/` beside the quad's own directory (ofs-config's rule), then
## the checkout's.
static func roots(quad_path: String) -> Array:
	return [quad_path.get_base_dir().get_base_dir().path_join("library"), AppSettings.repo_root().path_join("library")]


## The value of a top-level `key = "..."` line, read before the first `[section]`, like catalog.gd's name scan.
static func _top_level_string(path: String, key: String) -> String:
	if not FileAccess.file_exists(path):
		return ""
	var table := ""
	for line in FileAccess.get_file_as_string(path).split("\n"):
		var t := line.strip_edges()
		if t.begins_with("["):
			table = t
		elif table == "" and t.begins_with(key) and t.contains("="):
			var pair := t.split("=", true, 1)
			if pair[0].strip_edges() == key:
				return pair[1].strip_edges().trim_prefix("\"").get_slice("\"", 0)
	return ""


## The first `[a, b, c]` row of the line starting with `key` (an array-of-arrays value), as floats.
static func _first_row(path: String, key: String) -> PackedFloat32Array:
	for line in _lines(path):
		var t := line.strip_edges()
		if t.begins_with(key) and t.contains("="):
			var rest := t.split("=", true, 1)[1]
			var start := rest.find("[[")
			var end := rest.find("]")
			if start == -1 or end == -1:
				return PackedFloat32Array()
			return _floats(rest.substr(start + 2, end - start - 2))
	return PackedFloat32Array()


## The numbers of the line starting with `key` (a flat array value), as floats.
static func _numbers(path: String, key: String) -> PackedFloat32Array:
	for line in _lines(path):
		var t := line.strip_edges()
		if t.begins_with(key) and t.contains("="):
			var rest := t.split("=", true, 1)[1]
			var start := rest.find("[")
			var end := rest.rfind("]")
			if start == -1 or end == -1:
				return PackedFloat32Array()
			return _floats(rest.substr(start + 1, end - start - 1))
	return PackedFloat32Array()


static func _lines(path: String) -> PackedStringArray:
	return FileAccess.get_file_as_string(path).split("\n") if FileAccess.file_exists(path) else PackedStringArray()


static func _floats(text: String) -> PackedFloat32Array:
	var out := PackedFloat32Array()
	for part_str in text.split(","):
		out.append(part_str.strip_edges().to_float())
	return out
