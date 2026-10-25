extends RefCounted
## What the main menu picks from: the world and quad files on disk.

const AppSettings = preload("res://scripts/settings.gd")


## The directories that hold `kind` ("worlds" or "quads"): the checkout's, then the data dir's.
static func roots(settings: Dictionary, kind: String) -> Array:
	return [AppSettings.repo_root().path_join(kind), String(settings["data_dir"]).path_join(kind)]


## The `*.toml` files directly in `dirs` as {path, label, gates}, sorted by label. A file reached through two
## directories is listed once; a directory that does not exist holds nothing.
static func list(dirs: Array) -> Array:
	var seen := {}
	var out := []
	for dir in dirs:
		if not DirAccess.dir_exists_absolute(dir):
			continue
		for file in DirAccess.get_files_at(dir):
			var path: String = String(dir).path_join(file).simplify_path()
			if file.get_extension() == "toml" and not seen.has(path):
				seen[path] = true
				out.append(entry(path))
	out.sort_custom(func(a, b): return a["label"].naturalnocasecmp_to(b["label"]) < 0)
	return out


## `list` plus `path` when it is not in it (a flag or a saved choice that lies elsewhere must not vanish).
static func with_current(items: Array, path: String) -> Array:
	var out := items.duplicate()
	var simple := path.simplify_path()
	if path != "" and not out.any(func(item): return item["path"] == simple):
		out.append(entry(simple))
	return out


## The index of `path` in `items`, or 0.
static func index_of(items: Array, path: String) -> int:
	var simple := path.simplify_path()
	for i in items.size():
		if items[i]["path"] == simple:
			return i
	return 0


## One file: its top-level `name = "..."` (else the file name) and how many `[[gates]]` it declares.
static func entry(path: String) -> Dictionary:
	var label := path.get_file().get_basename()
	var gates := 0
	var top_level := true
	for line in FileAccess.get_file_as_string(path).split("\n"):
		var t := line.strip_edges()
		if t.begins_with("["):
			top_level = false
			if t == "[[gates]]":
				gates += 1
		elif top_level and t.begins_with("name") and t.contains("="):
			var value := t.split("=", true, 1)
			if value[0].strip_edges() == "name":
				label = value[1].strip_edges().trim_prefix("\"").get_slice("\"", 0)
	return {"path": path, "label": label, "gates": gates}
