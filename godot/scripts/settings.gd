extends RefCounted
## The pilot client's settings. Later sources win: built-in defaults, then the `[client]` section of
## user://ofs_client.cfg, then OFS_* environment variables, then command-line flags after `--`
## (`godot --path godot -- --open-loop --quad=quads/other.toml`).

const CONFIG_PATH := "user://ofs_client.cfg"

## Environment variable -> setting.
const ENV_NAMES := {
	"OFS_SIM_BIN": "server_bin",
	"OFS_SERVER_ADDR": "server_addr",
	"OFS_QUAD": "quad_path",
	"OFS_DATA_DIR": "data_dir",
	"OFS_OPEN_LOOP": "open_loop",
	"OFS_SITL_LAUNCH": "sitl_launch",
	"OFS_WORLD": "world_path",
	"OFS_VIDEO_EFFECTS": "video_effects",
	"OFS_FPV_ASPECT": "fpv_aspect",
	"OFS_DVR_EXPORT": "dvr_export",
	"OFS_DVR_FEED": "dvr_feed",
	"OFS_MENU": "menu",
	"OFS_RACE_START": "race_start",
	"OFS_COURSE": "course",
	"OFS_FULLSCREEN": "fullscreen",
	"OFS_HUD": "hud_visible",
}
## Command-line flag -> setting (`--name=value`; `--open-loop` alone means true).
const FLAG_NAMES := {
	"--sim": "server_bin",
	"--server": "server_addr",
	"--quad": "quad_path",
	"--data-dir": "data_dir",
	"--open-loop": "open_loop",
	"--sitl-launch": "sitl_launch",
	"--world": "world_path",
	"--video-effects": "video_effects",
	"--fpv-aspect": "fpv_aspect",
	"--dvr-export": "dvr_export",
	"--dvr-feed": "dvr_feed",
	"--menu": "menu",
	"--race-start": "race_start",
	"--course": "course",
	"--fullscreen": "fullscreen",
	"--hud": "hud_visible",
}
## What `dvr_feed` may be: the goggles picture (OSD and video breakup), the clean camera image, or both files.
const DVR_FEEDS := ["goggles", "clean", "both"]
## What `dvr_export` may be: nothing, an image sequence, an MP4 (needs ffmpeg), or both.
const DVR_EXPORTS := ["none", "frames", "mp4", "both"]


## The repository root when the project runs from a checkout (godot/ sits one level below it).
static func repo_root() -> String:
	return ProjectSettings.globalize_path("res://").path_join("..").simplify_path()


static func defaults() -> Dictionary:
	var root := repo_root()
	var exe := "ofs-sim.exe" if OS.get_name() == "Windows" else "ofs-sim"
	var server_bin := exe  # found on PATH
	for profile in ["release", "debug"]:
		var candidate := root.path_join("target").path_join(profile).path_join(exe)
		if FileAccess.file_exists(candidate):
			server_bin = candidate
			break
	return {
		"server_addr": "127.0.0.1:50051",
		"server_bin": server_bin,
		"data_dir": root.path_join(".ofs-data"),
		"quad_path": root.path_join("quads").path_join("opendrone-5f-freestyle.toml"),
		"world_path": root.path_join("worlds").path_join("flat.toml"),
		"video_effects": true,
		"fpv_aspect": "4:3",
		"dvr_export": "none",
		"dvr_feed": "goggles",
		"menu": true,
		"race_start": true,
		"course": "",  # the course to race in a world with several; empty = the world's own default
		"fullscreen": false,
		"hud_visible": true,
		"sitl_launch": "",
		"open_loop": false,
		"seed": 1,
		"overrun_policy": "warn",
		"state_rate_hz": 240,
		"stick_rate_hz": 250,
	}


## Merges the sources. `config` holds the `[client]` values, `env` the OFS_* variables that are set, `args` the
## command-line flags after `--`.
static func resolve(config: Dictionary, env: Dictionary, args: PackedStringArray) -> Dictionary:
	var s := defaults()
	for key in config:
		if s.has(key):
			s[key] = _coerce(s[key], config[key])
	for name in env:
		if ENV_NAMES.has(name):
			var key: String = ENV_NAMES[name]
			s[key] = _coerce(s[key], env[name])
	for arg in args:
		var parts := arg.split("=", true, 1)
		if FLAG_NAMES.has(parts[0]):
			var key: String = FLAG_NAMES[parts[0]]
			s[key] = _coerce(s[key], parts[1] if parts.size() > 1 else "true")
	if not s["dvr_export"] in DVR_EXPORTS:
		push_warning("%s is not a DVR export setting; using none" % s["dvr_export"])
		s["dvr_export"] = "none"
	if not s["dvr_feed"] in DVR_FEEDS:
		push_warning("%s is not a DVR feed setting; using goggles" % s["dvr_feed"])
		s["dvr_feed"] = "goggles"
	return s


## Brings a value from text or a config file to the type of the default it replaces.
static func _coerce(default, value):
	match typeof(default):
		TYPE_BOOL:
			if typeof(value) == TYPE_BOOL:
				return value
			return str(value).to_lower() in ["1", "true", "yes", "on"]
		TYPE_INT:
			return int(value) if str(value).is_valid_int() else default
		TYPE_STRING:
			return str(value)
	return value


## For each setting a flag or an environment variable sets this run, what sets it (`--flag` or `OFS_NAME`): the one
## `resolve` lets win. The settings screen locks those rows, since a saved value could not take effect.
static func overrides(env: Dictionary, args: PackedStringArray) -> Dictionary:
	var out := {}
	for name in env:
		if ENV_NAMES.has(name):
			out[ENV_NAMES[name]] = name
	for arg in args:
		var flag := arg.split("=", true, 1)[0]
		if FLAG_NAMES.has(flag):
			out[FLAG_NAMES[flag]] = flag
	return out


## The OFS_* variables that are set.
static func read_env() -> Dictionary:
	var env := {}
	for name in ENV_NAMES:
		if OS.has_environment(name):
			env[name] = OS.get_environment(name)
	return env


static func load_settings() -> Dictionary:
	var config := {}
	var file := ConfigFile.new()
	if file.load(CONFIG_PATH) == OK:
		for key in file.get_section_keys("client") if file.has_section("client") else PackedStringArray():
			config[key] = file.get_value("client", key)
	return resolve(config, read_env(), OS.get_cmdline_user_args())


## Stores one setting in the `[client]` section of user://ofs_client.cfg (for settings changed in the game, like
## "Video effects"). Only that setting's line changes: comments and the rest of the file stay as they are. Returns false
## when the file cannot be written.
static func save_value(key: String, value, path := CONFIG_PATH) -> bool:
	var text := ""
	if FileAccess.file_exists(path):
		# A file that does not parse (a hand edit gone wrong) is left alone.
		if ConfigFile.new().load(path) != OK:
			return false
		text = FileAccess.get_file_as_string(path)
	var file := FileAccess.open(path, FileAccess.WRITE)
	if file == null:
		return false
	file.store_string(with_value(text, "client", key, "%s=%s" % [key, var_to_str(value)]))
	file.close()
	return true


## Config file `text` with `line` as `key`'s line in `[section]`: the key's line is replaced where it is, or the line is
## added after the section's last line (or in a new section at the end). Every other line, comments included, stays.
static func with_value(text: String, section: String, key: String, line: String) -> String:
	var lines := text.split("\n")
	var current := ""
	var insert_at := -1
	for i in lines.size():
		var t := lines[i].strip_edges()
		if t.begins_with("[") and t.ends_with("]"):
			current = t.substr(1, t.length() - 2).strip_edges()
			if current == section:
				insert_at = i + 1
			continue
		if current != section or t == "":
			continue
		insert_at = i + 1
		if not t.begins_with(";") and not t.begins_with("#") and t.split("=", true, 1)[0].strip_edges() == key:
			lines[i] = line
			return "\n".join(lines)
	if insert_at >= 0:
		lines.insert(insert_at, line)
		return "\n".join(lines)
	var head := text if text == "" or text.ends_with("\n") else text + "\n"
	return head + "[%s]\n%s\n" % [section, line]


## The dictionary `OfsClient.start` takes.
static func to_client_dict(s: Dictionary) -> Dictionary:
	var d := {
		"quad_path": s["quad_path"],
		"world_path": s["world_path"],
		"server_addr": s["server_addr"],
		"seed": s["seed"],
		"open_loop": s["open_loop"],
		"overrun_policy": s["overrun_policy"],
		"state_rate_hz": s["state_rate_hz"],
		"stick_rate_hz": s["stick_rate_hz"],
	}
	if s["server_bin"] != "":
		d["server_bin"] = s["server_bin"]
		d["data_dir"] = s["data_dir"]
		d["log_file"] = String(s["data_dir"]).path_join("ofs-sim.log")
		if s["sitl_launch"] != "":
			d["env"] = {"OFS_SITL_LAUNCH": s["sitl_launch"]}
	return d
