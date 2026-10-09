extends SceneTree
## Renders the game to PNG files with a real renderer (a window opens briefly), to check the visuals by eye:
##   godot --path godot -s res://tests/shots.gd -- --out=C:/some/dir
## Flies the open-loop drone forward for a moment, then saves start_fpv.png, start_chase.png, osd_fpv.png, hop_fpv.png,
## hop_chase.png, video_grain.png, video_unstable.png, video_lost.png, help.png and controls.png.

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
	await _frames(30)
	var cells := PackedInt32Array()
	cells.resize(30 * 16)
	cells.fill(0x20)
	var put := func(row: int, col: int, text: String) -> void:
		for i in text.length():
			cells[row * 30 + col + i] = text.unicode_at(i)
	put.call(0, 10, "OPENFPV")
	put.call(1, 1, "ACRO")
	put.call(1, 22, "R1 5658")
	put.call(8, 9, "LOW BATTERY")
	put.call(14, 1, "24.6 0.5 1.2A")
	put.call(15, 1, "01:23 LQ100 12M")
	app.osd.set_frame({"seq": 1, "time_s": 0.0, "present": true, "cols": 30, "rows": 16, "cells": cells})
	await _save("osd_fpv")
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
	# The analog link's looks, forced (the drone stays near the pilot, where the real link is clean). `_process` would
	# overwrite them with the real telemetry, so it is paused while they are shot (after the HUD has caught up).
	await _frames(3)
	app.set_process(false)
	for look in [["video_grain", {"video_noise": 0.45, "video_sparkles": 0.1, "video_chroma": 1.0, "video_sync": "locked"}],
			["video_unstable", {"video_noise": 0.7, "video_sparkles": 0.6, "video_chroma": 0.3, "video_sync": "unstable"}],
			["video_lost", {"video_noise": 1.0, "video_sparkles": 1.0, "video_chroma": 0.0, "video_sync": "lost"}]]:
		var t: Dictionary = look[1]
		t["video_present"] = true
		for i in 10:
			app.video.update_view(t, 0.03)
			await process_frame
		await _save(look[0])
	app.set_process(true)
	app.hud.set_help_visible(true)
	await _save("help")
	app.hud.set_help_visible(false)
	app.controls_menu.toggle()
	await _save("controls")
	app.queue_free()
	await _frames(2)
	quit(0)
