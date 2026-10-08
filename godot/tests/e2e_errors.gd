extends SceneTree
## What the pilot sees when things are wrong: a missing server program, a bad quad path. No Betaflight, and no
## working server is needed (one is started only for the bad-quad case).
##   godot --headless --path godot -s res://tests/e2e_errors.gd

var failures := 0


func _initialize() -> void:
	_run()


func _check(condition: bool, message: String) -> void:
	if condition:
		print("  ok   ", message)
	else:
		failures += 1
		printerr("  FAIL ", message)


func _wait_for(condition: Callable, seconds: float) -> bool:
	var deadline := Time.get_ticks_msec() + int(seconds * 1000.0)
	while Time.get_ticks_msec() < deadline:
		if condition.call():
			return true
		await process_frame
	return condition.call()


func _free_port() -> int:
	var server := TCPServer.new()
	server.listen(0, "127.0.0.1")
	var port := server.get_local_port()
	server.stop()
	return port


func _start(env: Dictionary) -> Node:
	for name in env:
		OS.set_environment(name, env[name])
	var app = load("res://main.tscn").instantiate()
	root.add_child(app)
	await process_frame
	return app


func _stop(app: Node) -> void:
	app.queue_free()
	await process_frame
	await process_frame


func _run() -> void:
	var data_dir := OS.get_user_data_dir().path_join("e2e-errors-data")

	# 1. The server program does not exist and nothing listens: the pilot is told what to build.
	var app = await _start({
		"OFS_SIM_BIN": "definitely-not-ofs-sim", "OFS_SERVER_ADDR": "127.0.0.1:%d" % _free_port(),
		"OFS_DATA_DIR": data_dir, "OFS_OPEN_LOOP": "1"})
	var failed: bool = await _wait_for(func(): return app.client.get_phase() == "failed", 15.0)
	_check(failed, "a missing server program fails the connection: %s" % app.client.get_phase())
	await process_frame
	var banner: String = app.hud.banner_text()
	_check(banner.contains("definitely-not-ofs-sim") and banner.contains("cargo build -p ofs-sim"), "the banner names the program and the fix: %s" % banner)
	_check(banner.contains("Press R to retry"), "and says how to retry")
	_check(" ".join(app.hud.toast_texts()).contains("Failed"), "a toast says it failed too: %s" % str(app.hud.toast_texts()))
	await _stop(app)

	# 2. The quad file does not exist: the server's message and the config tip are shown.
	OS.unset_environment("OFS_SIM_BIN")
	app = await _start({
		"OFS_SERVER_ADDR": "127.0.0.1:%d" % _free_port(), "OFS_DATA_DIR": data_dir, "OFS_OPEN_LOOP": "1",
		"OFS_QUAD": "does/not/exist.toml"})
	failed = await _wait_for(func(): return app.client.get_phase() == "failed", 30.0)
	_check(failed, "a missing quad fails the load: %s %s" % [app.client.get_phase(), app.client.get_phase_detail()])
	await process_frame
	banner = app.hud.banner_text()
	_check(banner.contains("does/not/exist.toml") and banner.contains("quad_path"), "the banner names the quad and the setting: %s" % banner)
	await _stop(app)

	print("E2E %s" % ("PASSED" if failures == 0 else "FAILED (%d)" % failures))
	quit(1 if failures > 0 else 0)
