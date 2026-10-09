extends SceneTree
## End to end without Betaflight: the game scene starts a real ofs-sim (open loop), flies it and reacts to the
## radio being cut. Needs the extension and the server built (target/debug), or OFS_SIM_BIN set.
##   godot --headless --path godot -s res://tests/e2e_open_loop.gd

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


func _press(app: Node, keycode: int) -> void:
	var event := InputEventKey.new()
	event.physical_keycode = keycode
	event.pressed = true
	app._unhandled_key_input(event)


func _free_port() -> int:
	var server := TCPServer.new()
	server.listen(0, "127.0.0.1")
	var port := server.get_local_port()
	server.stop()
	return port


func _run() -> void:
	var data_dir := OS.get_user_data_dir().path_join("e2e-data")
	OS.set_environment("OFS_OPEN_LOOP", "1")
	OS.set_environment("OFS_SERVER_ADDR", "127.0.0.1:%d" % _free_port())
	OS.set_environment("OFS_DATA_DIR", data_dir)
	var app = load("res://main.tscn").instantiate()
	root.add_child(app)
	await process_frame  # the scene's _ready runs on the first frame
	_check(app.client != null, "the OfsClient extension is loaded and started")
	var flying: bool = await _wait_for(func(): return app.client.get_phase() == "flying", 30.0)
	_check(flying, "reaches the flying phase (phase: %s %s)" % [app.client.get_phase(), app.client.get_phase_detail()])
	_check(app.client.get_quad_name().contains("OpenDrone"), "the quad name is known: %s" % app.client.get_quad_name())
	_check(app.hud.toast_texts().size() > 0 and app.hud.toast_texts()[0].begins_with("Loaded"), "the HUD announces the quad: %s" % str(app.hud.toast_texts()))

	app.sticks_override = {"roll": 0.0, "pitch": 0.0, "yaw": 0.0, "throttle": 0.9, "aux": [1.0, -1.0, -1.0, -1.0], "status": ""}
	var climbed: bool = await _wait_for(func(): return app.drone.position.y > 0.5, 20.0)
	_check(climbed, "the drone climbs under the sticks: y = %.2f" % app.drone.position.y)
	var telemetry: Dictionary = app.client.get_telemetry()
	_check(not telemetry.is_empty() and telemetry["tx_enabled"] and telemetry["link_up"] and telemetry["running"], "telemetry shows a live link: %s" % str(telemetry))
	_check(absf(app.drone.position.y - telemetry["altitude_m"]) < 1.0, "the drawn height follows the simulated altitude")
	_check(app.hud.banner_text() == "", "no banner while all is well: '%s'" % app.hud.banner_text())

	# Keys, through the game's own handler.
	_press(app, KEY_K)
	_check(app.radio_cut, "K cuts the radio")
	var told: bool = await _wait_for(func(): return " ".join(app.hud.toast_texts()).contains("Radio link lost"), 15.0)
	_check(told, "cutting the radio raises a link-lost toast: %s" % str(app.hud.toast_texts()))
	var banner: bool = await _wait_for(func(): return app.hud.banner_text().begins_with("RADIO LINK"), 5.0)
	_check(banner, "and a banner: '%s'" % app.hud.banner_text())
	_press(app, KEY_K)
	_check(not app.radio_cut, "K restores it")
	var restored: bool = await _wait_for(func(): return " ".join(app.hud.toast_texts()).contains("restored"), 15.0)
	_check(restored, "and the HUD says so")

	_press(app, KEY_P)
	var paused: bool = await _wait_for(func(): return app.client.get_phase() == "paused" and not app.client.get_telemetry()["running"], 5.0)
	_check(paused, "P pauses the simulation")
	var frozen_at: float = app.client.get_telemetry()["time_s"]
	await _wait_for(func(): return false, 0.3)
	_check(app.client.get_telemetry()["time_s"] == frozen_at, "simulated time stands still")
	_press(app, KEY_P)
	var resumed: bool = await _wait_for(func(): return app.client.get_phase() == "flying" and app.client.get_telemetry()["time_s"] > frozen_at + 0.1, 5.0)
	_check(resumed, "P resumes it")

	_press(app, KEY_C)
	await process_frame
	_check(app.chase_camera.current and not app.lens.visible, "C: the chase camera takes over and the lens is off")
	_press(app, KEY_C)
	await process_frame
	_check(app.fpv_camera.current and app.lens.visible, "C again: back to FPV")
	_press(app, KEY_F1)
	_check(app.hud.help_visible(), "F1 shows the help")
	_press(app, KEY_ESCAPE)
	_check(not app.hud.help_visible(), "Escape hides it")
	_press(app, KEY_F2)
	_check(app.controls_menu.is_open(), "F2 opens the controls screen")
	_press(app, KEY_ESCAPE)
	_check(not app.controls_menu.is_open(), "Escape closes it")
	_press(app, KEY_H)
	_check(not app.hud.hud_visible(), "H hides the HUD")
	_press(app, KEY_H)
	_check(app.hud.hud_visible(), "and shows it again")

	var before_reload: float = app.client.get_telemetry()["time_s"]
	app.sticks_override = {"roll": 0.0, "pitch": 0.0, "yaw": 0.0, "throttle": 0.0, "aux": [-1.0, -1.0, -1.0, -1.0], "status": ""}
	_press(app, KEY_R)
	var reloaded: bool = await _wait_for(func(): return app.client.get_phase() == "flying" and app.client.get_telemetry().get("time_s", 1e9) < before_reload, 30.0)
	_check(reloaded, "R reloads the quad: simulated time restarts (%.1f s -> %.1f s)" % [before_reload, app.client.get_telemetry().get("time_s", -1.0)])
	await _wait_for(func(): return false, 0.3)
	_check(app.drone.position.y < 0.2, "and the drone is back on the ground: y = %.2f" % app.drone.position.y)
	_check(app.client.get_telemetry()["tx_enabled"], "the pilot link is back")

	var osd: Dictionary = app.client.get_osd()
	_check(osd.has("present") and not osd["present"] and osd["cols"] == 0, "open loop: the OSD is absent: %s" % str(osd.keys()))
	var t: Dictionary = app.client.get_telemetry()
	_check(t.get("vtx_present", false) and t.get("vtx_freq_mhz", 0) == 5658, "open loop: the VTX transmits R1: %s" % str(t.get("vtx_freq_mhz")))

	# The world file and the video link.
	var world: Dictionary = app.client.get_world()
	_check(world.get("name", "") == "Flat field", "the shipped world is loaded: %s" % world.get("name", ""))
	var building = app.world.get_node_or_null("BuildingA")
	_check(building != null and building.position.is_equal_approx(Vector3(-45.0, 7.0, -110.0)), "building A stands where it always did")
	_check(app.world.get_node_or_null("Pilot") != null and app.world.get_node_or_null("Emitter_parked-quad") != null, "the pilot and the parked quad are marked")
	_check(t.get("video_present", false) and t.get("video_sync", "") == "locked", "the video link is locked on the pad: %s" % str(t.get("video_sync")))
	_check(app.hud.video_text().begins_with("VID "), "the HUD shows the video line: '%s'" % app.hud.video_text())
	_check(not app.video.is_active(), "a clean link draws nothing over the picture")
	app.set_process(false)  # the next frame would feed the real (clean) telemetry
	app.video.update_view({"video_present": true, "video_sync": "lost", "video_noise": 1.0, "video_sparkles": 1.0, "video_chroma": 0.0}, 0.016)
	_check(app.video.is_active(), "a lost link draws static")
	app.set_chase_view(true)
	_check(not app.video.is_active(), "the chase view is clean")
	app.set_chase_view(false)
	_check(app.video.is_active(), "and the FPV view degraded again")
	app.set_process(true)

	app.queue_free()
	await process_frame
	await process_frame
	print("E2E %s" % ("PASSED" if failures == 0 else "FAILED (%d)" % failures))
	quit(1 if failures > 0 else 0)
