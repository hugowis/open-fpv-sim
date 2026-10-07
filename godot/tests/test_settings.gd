extends "res://tests/testing.gd"

const AppSettings = preload("res://scripts/settings.gd")


func test_defaults_point_into_the_checkout() -> void:
	var s := AppSettings.defaults()
	var root := AppSettings.repo_root()
	ok(not root.contains(".."), "the repository root is simplified: %s" % root)
	ok(s["quad_path"].begins_with(root) and s["quad_path"].ends_with("opendrone-5f-freestyle.toml"), "quad path %s" % s["quad_path"])
	ok(FileAccess.file_exists(s["quad_path"]), "the reference quad exists at %s" % s["quad_path"])
	eq(s["server_addr"], "127.0.0.1:50051", "server address")
	eq(s["open_loop"], false, "Betaflight by default")
	ok(s["server_bin"].ends_with("ofs-sim") or s["server_bin"].ends_with("ofs-sim.exe"), "server binary %s" % s["server_bin"])


func test_later_sources_win() -> void:
	var s := AppSettings.resolve(
		{"server_addr": "10.0.0.1:1", "seed": 7, "state_rate_hz": 120},
		{"OFS_SERVER_ADDR": "127.0.0.1:6000", "OFS_OPEN_LOOP": "1"},
		PackedStringArray(["--server=127.0.0.1:7000", "--quad=quads/other.toml"]))
	eq(s["server_addr"], "127.0.0.1:7000", "the command line beats the environment, which beats the file")
	eq(s["open_loop"], true, "the environment turns open loop on")
	eq(s["seed"], 7, "the file's seed stays")
	eq(s["state_rate_hz"], 120, "and its rate")
	eq(s["quad_path"], "quads/other.toml", "the quad from the command line")


func test_flags_without_a_value_mean_true_and_junk_is_ignored() -> void:
	var s := AppSettings.resolve({}, {}, PackedStringArray(["--open-loop", "--unknown=1", "positional"]))
	eq(s["open_loop"], true, "--open-loop")
	var t := AppSettings.resolve({}, {"OFS_OPEN_LOOP": "no", "NOT_OURS": "x"}, PackedStringArray())
	eq(t["open_loop"], false, "OFS_OPEN_LOOP=no")
	var u := AppSettings.resolve({"seed": "not a number", "bogus": 1}, {}, PackedStringArray())
	eq(u["seed"], 1, "a seed that is not a number keeps the default")
	ok(not u.has("bogus"), "unknown keys are dropped")


func test_the_client_dictionary_launches_a_server_only_when_one_is_configured() -> void:
	var s := AppSettings.defaults()
	s["server_bin"] = "C:/x/ofs-sim.exe"
	s["data_dir"] = "C:/x/data"
	s["sitl_launch"] = "wsl.exe -d Ubuntu -e /home/me/betaflight_SITL.elf"
	var d := AppSettings.to_client_dict(s)
	eq(d["server_bin"], "C:/x/ofs-sim.exe", "server binary")
	eq(d["log_file"], "C:/x/data/ofs-sim.log", "log file")
	eq(d["env"], {"OFS_SITL_LAUNCH": "wsl.exe -d Ubuntu -e /home/me/betaflight_SITL.elf"}, "SITL launch for the server")
	s["server_bin"] = ""
	d = AppSettings.to_client_dict(s)
	ok(not d.has("server_bin") and not d.has("env"), "attach only")
	eq(d["quad_path"], s["quad_path"], "quad path")
