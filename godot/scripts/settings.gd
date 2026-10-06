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
}
## Command-line flag -> setting (`--name=value`; `--open-loop` alone means true).
const FLAG_NAMES := {
	"--sim": "server_bin",
	"--server": "server_addr",
	"--quad": "quad_path",
	"--data-dir": "data_dir",
	"--open-loop": "open_loop",
	"--sitl-launch": "sitl_launch",
}


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


static func load_settings() -> Dictionary:
	var config := {}
	var file := ConfigFile.new()
	if file.load(CONFIG_PATH) == OK:
		for key in file.get_section_keys("client") if file.has_section("client") else PackedStringArray():
			config[key] = file.get_value("client", key)
	var env := {}
	for name in ENV_NAMES:
		if OS.has_environment(name):
			env[name] = OS.get_environment(name)
	return resolve(config, env, OS.get_cmdline_user_args())


## The dictionary `OfsClient.start` takes.
static func to_client_dict(s: Dictionary) -> Dictionary:
	var d := {
		"quad_path": s["quad_path"],
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
