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


## The frames of an MJPEG AVI the client's muxer wrote: a minimal RIFF walk to the movi list, then its 00dc chunks.
func _avi_frames(path: String) -> Array:
	var bytes := FileAccess.get_file_as_bytes(path)
	var frames: Array = []
	if bytes.size() < 12 or bytes.slice(0, 4).get_string_from_ascii() != "RIFF" or bytes.slice(8, 12).get_string_from_ascii() != "AVI ":
		_check(false, "the recording is a RIFF AVI: %s (%d bytes)" % [path, bytes.size()])
		return frames
	var movi := -1
	for i in range(12, bytes.size() - 4):
		if bytes[i] == 0x6D and bytes.slice(i, i + 4).get_string_from_ascii() == "movi":
			movi = i
			break
	var at := movi + 4
	while movi >= 0 and at + 8 <= bytes.size() and bytes.slice(at, at + 4).get_string_from_ascii() == "00dc":
		var size := bytes.decode_u32(at + 4)
		frames.append(bytes.slice(at + 8, at + 8 + size))
		at += 8 + size + (size & 1)
	return frames


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

	var logs := data_dir.path_join("logs")
	for stale in DirAccess.get_files_at(logs):  # the data directory outlives runs: start from none
		if stale.contains("-inputs-") and stale.ends_with(".csv"):
			DirAccess.remove_absolute(logs.path_join(stale))
	_press(app, KEY_I)
	var recording: bool = await _wait_for(func(): return app.inputs_recording, 10.0)
	_check(recording, "I starts recording the sticks")
	await _wait_for(func(): return app.hud.sim_text().contains("REC STICKS"), 2.0)  # the HUD refreshes on the next frame
	_check(app.hud.sim_text().contains("REC STICKS"), "and the status line says so: %s" % app.hud.sim_text())
	await _wait_for(func(): return false, 0.1)
	app.sticks_override = {"roll": 0.25, "pitch": 0.0, "yaw": 0.0, "throttle": 0.6, "aux": [1.0, -1.0, -1.0, -1.0], "status": ""}
	await _wait_for(func(): return false, 0.2)
	_press(app, KEY_K)
	_check(app.radio_cut, "the recording includes a K cut")
	await _wait_for(func(): return false, 0.2)
	_press(app, KEY_I)
	var recorded: bool = await _wait_for(func(): return not app.inputs_recording, 10.0)
	_check(recorded, "I again stops it")
	var csv_files: Array = []
	for f in DirAccess.get_files_at(logs):
		if f.contains("-inputs-") and f.ends_with(".csv"):
			csv_files.append(f)
	_check(csv_files.size() == 1, "one inputs CSV in the logs directory: %s" % str(csv_files))
	if csv_files.size() == 1:
		var text := FileAccess.get_file_as_string(logs.path_join(csv_files[0]))
		_check(text.begins_with("t_s,roll,pitch,yaw,throttle,aux1,aux2,aux3,aux4,tx,link_loss\n"), "it has the play_inputs header: %s" % text.left(80))
		var lines := text.split("\n", false)
		_check(lines.size() >= 3, "the first row, the stick move and the cut: %s" % text)
		var cut_row := ""
		var previous := ""
		for line in lines.slice(1):
			var cells := line.split(",")
			if cells.size() == 11 and cells[10] == "1":
				cut_row = line
				var previous_cells := previous.split(",")
				_check(previous_cells.size() == 11, "a data row precedes the cut: %s" % text)
				_check(cells.slice(1, 9) == previous_cells.slice(1, 9), "the cut changes only link_loss: %s" % text)
				_check(cells[9] == "1", "the K cut leaves the transmitter on: %s" % line)
				break
			previous = line
		_check(cut_row != "", "the K cut is a link_loss=1 row: %s" % text)
	_press(app, KEY_K)
	_check(not app.radio_cut, "K restores the link after the recording")

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
	_check(app.world.get_node_or_null("Handset") != null, "the pilot's handset is marked beside him")
	_check(t.get("video_present", false) and t.get("video_sync", "") == "locked", "the video link is locked on the pad: %s" % str(t.get("video_sync")))
	_check(app.hud.video_text().begins_with("VID "), "the HUD shows the video line: '%s'" % app.hud.video_text())
	_check(not app.video.is_active(), "a clean link draws nothing over the picture")

	# The delayed composite: pose, video and OSD sampled at one field-quantized instant in the past.
	var fpv: Dictionary = app.client.get_fpv_view()
	_check(fpv.has("pose"), "the fpv view carries a pose")
	_check(fpv.has("render_time_s") and float(fpv["render_time_s"]) > 0.0, "the render instant is set")
	_check(bool(fpv.get("field_valid", false)), "a clean link completes every field")
	_check(int(fpv.get("held_fields", -1)) == 0, "a clean link holds nothing: held_fields is 0")
	_check(int(fpv.get("latency_ms", -1)) == 40, "the default latency reaches the client")
	_check(float(fpv["render_time_s"]) <= float(app.client.get_telemetry()["time_s"]), "the picture trails the sim clock")

	# A hard touchdown raises the collision toast (the quad falls back onto the launch pad or the ground).
	app.sticks_override = {"roll": 0.0, "pitch": 0.0, "yaw": 0.0, "throttle": 0.9, "aux": [1.0, -1.0, -1.0, -1.0], "status": ""}
	var up: bool = await _wait_for(func(): return app.drone.position.y > 2.0, 20.0)
	_check(up, "climbs for the landing test: y = %.2f" % app.drone.position.y)
	app.sticks_override = {"roll": 0.0, "pitch": 0.0, "yaw": 0.0, "throttle": 0.0, "aux": [1.0, -1.0, -1.0, -1.0], "status": ""}
	var hit: bool = await _wait_for(func():
		var toasts := " ".join(app.hud.toast_texts())
		return toasts.contains("HIT LaunchPad") or toasts.contains("HARD LANDING"), 20.0)
	_check(hit, "the fall raises a collision toast: %s" % str(app.hud.toast_texts()))
	app.set_process(false)  # the next frame would feed the real (clean) telemetry
	app.video.update_view({"video_present": true, "video_sync": "lost", "video_noise": 1.0, "video_sparkles": 1.0, "video_chroma": 0.0}, 0.016)
	_check(app.video.is_active(), "a lost link draws static")
	app.set_chase_view(true)
	_check(not app.video.is_active(), "the chase view is clean")
	app.set_chase_view(false)
	_check(app.video.is_active(), "and the FPV view degraded again")
	app.set_process(true)

	# The DVR end to end (m3e debt): the D key records two simulated seconds through dvr.gd and the client's muxer.
	# Headless Godot renders nothing, so a synthetic image stands in for the SubViewport's picture; the field dedup,
	# the JPEG encode and the muxer are the real ones.
	var size: Vector2i = app.fpv_viewport.size
	app.dvr.image_source = func() -> Image:
		var image := Image.create(maxi(size.x, 16), maxi(size.y, 16), false, Image.FORMAT_RGB8)
		image.fill(Color(0.4, 0.5, 0.6))
		return image
	var started: Array = []
	app.dvr.started.connect(func(path: String): started.append(path))
	var t0: float = app.client.get_telemetry()["time_s"]
	var seen_targets := {}
	var deadline := Time.get_ticks_msec() + 20_000
	_press(app, KEY_D)
	_check(app.dvr.recording, "D starts the DVR")
	while float(app.client.get_telemetry()["time_s"]) < t0 + 2.0:
		var target := int(app.client.get_fpv_view().get("target_field", -1))
		if target >= 0:
			seen_targets[target] = true
		await process_frame
		if Time.get_ticks_msec() >= deadline:
			break
	var t1: float = app.client.get_telemetry()["time_s"]
	_press(app, KEY_D)
	_check(t1 >= t0 + 2.0, "the client advanced two simulated seconds (%.2f)" % (t1 - t0))
	_check(not app.dvr.recording and started.size() == 1, "D again stops it")
	if started.size() == 1:
		var frames := _avi_frames(ProjectSettings.globalize_path(started[0]))
		# The DVR is a screen recorder: it writes one frame per field boundary the client actually renders, not one
		# per simulated 20 ms. A runner slower than 50 Hz renders fewer boundaries; that is the pilot's picture.
		var expected := seen_targets.size()
		_check(expected > 0, "the client rendered at least one field boundary")
		_check(frames.size() >= expected - 1 and frames.size() <= expected + 3,
			"%d frames for %d rendered field boundaries (%.2f simulated seconds)" % [frames.size(), expected, t1 - t0])
		_check(frames.all(func(f: PackedByteArray) -> bool: return f.size() > 2 and f[0] == 0xFF and f[1] == 0xD8),
			"every frame is a JPEG")

	# The F2 DVR toggle (i38): the checkbox drives the same start/stop path as D, and D moves the checkbox. The
	# synthetic image source set above still feeds the recorder.
	var menu_started: Array = []
	app.dvr.started.connect(func(path: String): menu_started.append(path))
	_press(app, KEY_F2)
	_check(app.controls_menu.is_open(), "F2 opens the controls screen")
	app.controls_menu._dvr.button_pressed = true  # a click on the box, not a call into the DVR
	_check(app.dvr.recording and app.controls_menu.dvr_on(), "the DVR box starts the recording")
	_check(app.hud._rec.visible, "and the HUD shows REC")
	_check(app.hud.toast_texts()[app.hud.toast_texts().size() - 1].begins_with("DVR: recording to"), "through the app's one start path (the toast): %s" % str(app.hud.toast_texts()))
	var menu_t0: float = app.client.get_telemetry()["time_s"]
	var menu_deadline := Time.get_ticks_msec() + 20_000
	while float(app.client.get_telemetry()["time_s"]) < menu_t0 + 0.5 and Time.get_ticks_msec() < menu_deadline:
		await process_frame
	app.controls_menu._dvr.button_pressed = false
	_check(not app.dvr.recording and not app.controls_menu.dvr_on(), "unchecking the box stops it")
	_check(not app.hud._rec.visible, "and the REC marker goes away")
	_check(app.hud.toast_texts()[app.hud.toast_texts().size() - 1].begins_with("DVR:") and app.hud.toast_texts()[app.hud.toast_texts().size() - 1].contains("frames saved"), "through the app's one stop path (the toast): %s" % str(app.hud.toast_texts()))
	if menu_started.size() == 1:
		var menu_frames := _avi_frames(ProjectSettings.globalize_path(menu_started[0]))
		_check(menu_frames.size() >= 1, "the toggled recording captured frames: %d" % menu_frames.size())
	else:
		_check(false, "the box started exactly one recording: %s" % str(menu_started))
	_press(app, KEY_D)
	_check(app.dvr.recording and app.controls_menu.dvr_on(), "D starts it with the screen open")
	_press(app, KEY_D)
	_check(not app.dvr.recording and not app.controls_menu.dvr_on(), "D stops it and the box follows")
	_press(app, KEY_ESCAPE)
	_check(not app.controls_menu.is_open(), "Escape closes the controls screen")

	# A reload drops the recorder without an event: the indicator must go with it (the sticks recording is last so a
	# reload here disturbs nothing above).
	_press(app, KEY_I)
	var recording_again: bool = await _wait_for(func(): return app.inputs_recording, 10.0)
	_check(recording_again, "I starts a recording before the reload")
	_press(app, KEY_R)
	var reloaded_again: bool = await _wait_for(func(): return app.client.get_phase() == "flying" and not app.inputs_recording, 30.0)
	_check(reloaded_again, "R reloads the quad (phase: %s)" % app.client.get_phase())
	await _wait_for(func(): return false, 0.3)
	_check(not app.inputs_recording, "the reload clears the recording state")
	_check(not app.hud.sim_text().contains("REC STICKS"), "and the status line no longer says REC STICKS: %s" % app.hud.sim_text())
	_press(app, KEY_I)
	var recording_after: bool = await _wait_for(func(): return app.inputs_recording, 10.0)
	_check(recording_after, "one I press records again after the reload")
	_press(app, KEY_I)
	var stopped_after: bool = await _wait_for(func(): return not app.inputs_recording, 10.0)
	_check(stopped_after, "and the next I press stops it")

	app.queue_free()
	await process_frame
	await process_frame
	print("E2E %s" % ("PASSED" if failures == 0 else "FAILED (%d)" % failures))
	quit(1 if failures > 0 else 0)
