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
	ok(s["world_path"].begins_with(root) and s["world_path"].ends_with("flat.toml"), "world path %s" % s["world_path"])
	ok(FileAccess.file_exists(s["world_path"]), "the shipped world exists at %s" % s["world_path"])
	eq(s["video_effects"], true, "video effects on by default")
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


func test_the_world_and_the_video_effects_come_from_every_source() -> void:
	var s := AppSettings.resolve({"video_effects": false}, {"OFS_WORLD": "worlds/other.toml"}, PackedStringArray())
	eq(s["world_path"], "worlds/other.toml", "OFS_WORLD")
	eq(s["video_effects"], false, "the file turns the effects off")
	var t := AppSettings.resolve({}, {}, PackedStringArray(["--world=w.toml", "--video-effects=no"]))
	eq(t["world_path"], "w.toml", "--world")
	eq(t["video_effects"], false, "--video-effects=no")
	eq(AppSettings.to_client_dict(t)["world_path"], "w.toml", "the client gets the world")


func test_a_setting_saved_in_the_game_keeps_the_rest_of_the_file() -> void:
	var path := "user://test_ofs_client.cfg"
	DirAccess.remove_absolute(path)
	var file := ConfigFile.new()
	file.set_value("client", "seed", 7)
	file.save(path)
	ok(AppSettings.save_value("video_effects", false, path), "saved")
	var back := ConfigFile.new()
	eq(back.load(path), OK, "the file reads back")
	eq(back.get_value("client", "video_effects"), false, "the new value")
	eq(back.get_value("client", "seed"), 7, "the other values stay")
	DirAccess.remove_absolute(path)


func test_saving_a_setting_never_overwrites_a_config_file_it_cannot_read() -> void:
	var path := "user://test_broken_ofs_client.cfg"
	var broken := "[client]\nseed = 7\nthis line is not valid = = =\n"
	var file := FileAccess.open(path, FileAccess.WRITE)
	file.store_string(broken)
	file.close()
	ok(not AppSettings.save_value("video_effects", false, path), "refused")
	eq(FileAccess.get_file_as_string(path), broken, "the hand-edited file is left as it was")
	DirAccess.remove_absolute(path)
	ok(AppSettings.save_value("video_effects", false, path), "a missing file is created")
	DirAccess.remove_absolute(path)
