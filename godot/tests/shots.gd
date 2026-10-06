extends SceneTree
## Renders the game to PNG files with a real renderer (a window opens briefly), to check the visuals by eye:
##   godot --path godot -s res://tests/shots.gd -- --out=C:/some/dir
## Flies the open-loop drone forward for a moment, then saves fpv.png, chase.png, help.png and controls.png.

func _initialize() -> void:
	_run()


func _out_dir() -> String:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--out="):
			return arg.substr(6)
	return OS.get_user_data_dir()


func _free_port() -> int:
	var server := TCPServer.new()
	server.listen(0, "127.0.0.1")
	var port := server.get_local_port()
	server.stop()
	return port


func _frames(count: int) -> void:
	for i in count:
		await process_frame


func _save(name: String) -> void:
	await _frames(3)
	var image := root.get_viewport().get_texture().get_image()
	var path := _out_dir().path_join(name + ".png")
	print("saved ", path, " ", image.get_size(), " ", image.save_png(path))


func _run() -> void:
	OS.set_environment("OFS_OPEN_LOOP", "1")
	OS.set_environment("OFS_SERVER_ADDR", "127.0.0.1:%d" % _free_port())
	OS.set_environment("OFS_DATA_DIR", OS.get_user_data_dir().path_join("shots-data"))
	var app = load("res://main.tscn").instantiate()
	root.add_child(app)
	await _frames(2)
	var deadline := Time.get_ticks_msec() + 30000
	while app.client.get_phase() != "flying" and Time.get_ticks_msec() < deadline:
		await process_frame
	await _save("start_fpv")
	app.set_chase_view(true)
	await _frames(20)
	await _save("start_chase")
	app.set_chase_view(false)
	# A short low hop so the picture moves: climb a little, then let the drone settle forward.
	app.sticks_override = {"roll": 0.0, "pitch": 0.0, "yaw": 0.0, "throttle": 0.32, "aux": [1.0, -1.0, -1.0, -1.0], "status": ""}
	var until := Time.get_ticks_msec() + 1200
	while Time.get_ticks_msec() < until:
		await process_frame
	await _save("hop_fpv")
	app.set_chase_view(true)
	await _frames(20)
	await _save("hop_chase")
	app.set_chase_view(false)
	app.hud.set_help_visible(true)
	await _save("help")
	app.hud.set_help_visible(false)
	app.controls_menu.toggle()
	await _save("controls")
	app.queue_free()
	await _frames(2)
	quit(0)