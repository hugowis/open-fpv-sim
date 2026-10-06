extends SceneTree
## End to end with real Betaflight SITL: the game scene starts ofs-sim, Betaflight boots, the sticks arm it through
## the simulated ExpressLRS link, it flies in Angle mode, and cutting the radio makes it fail safe (disarm).
## Needs the extension and the server built, and OFS_SITL_LAUNCH set (docs/dev-setup.md); skips without it.
##   godot --headless --path godot -s res://tests/e2e_betaflight.gd

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


func _sticks(throttle: float, armed: bool) -> Dictionary:
	return {"roll": 0.0, "pitch": 0.0, "yaw": 0.0, "throttle": throttle, "aux": [1.0 if armed else -1.0, 1.0, -1.0, -1.0], "status": ""}


func _run() -> void:
	if not OS.has_environment("OFS_SITL_LAUNCH"):
		print("SKIP: OFS_SITL_LAUNCH is not set, so there is no Betaflight SITL to fly")
		quit(0)
		return
	OS.set_environment("OFS_SERVER_ADDR", "127.0.0.1:%d" % _free_port())
	OS.set_environment("OFS_DATA_DIR", OS.get_user_data_dir().path_join("e2e-betaflight-data"))
	var app = load("res://main.tscn").instantiate()
	root.add_child(app)
	await process_frame
	app.sticks_override = _sticks(0.0, false)
	var flying: bool = await _wait_for(func(): return app.client.get_phase() == "flying", 90.0)
	_check(flying, "reaches the flying phase: %s %s" % [app.client.get_phase(), app.client.get_phase_detail()])
	_check(app.client.get_configurator_address().begins_with("tcp://"), "the Configurator address is offered: %s" % app.client.get_configurator_address())

	# Betaflight needs a few seconds after boot before it accepts arming.
	var booted: bool = await _wait_for(func(): return app.client.get_telemetry().get("time_s", 0.0) > 6.0, 30.0)
	_check(booted, "Betaflight has been running for 6 s of simulated time")
	app.sticks_override = _sticks(0.0, true)
	var armed: bool = await _wait_for(func(): return app.client.get_telemetry().get("motors_spinning", false), 10.0)
	_check(armed, "the arm switch arms Betaflight (motors idle): %s" % str(app.client.get_telemetry().get("motor_cmd")))

	app.sticks_override = _sticks(0.6, true)
	var climbed: bool = await _wait_for(func(): return app.drone.position.y > 2.0, 20.0)
	_check(climbed, "it climbs: y = %.2f" % app.drone.position.y)
	var up: float = app.drone.global_transform.basis.y.dot(Vector3.UP)
	_check(up > 0.9, "and Angle mode keeps it level (up vector . vertical = %.3f)" % up)
	var telemetry: Dictionary = app.client.get_telemetry()
	_check(telemetry["link_up"] and telemetry["tx_enabled"] and telemetry["running"], "the radio link is up")
	_check(telemetry["overruns"] == 0, "no real-time overruns: %d" % telemetry["overruns"])

	app.set_radio_cut(true)
	var told: bool = await _wait_for(func(): return " ".join(app.hud.toast_texts()).contains("Radio link lost"), 10.0)
	_check(told, "cutting the radio is reported")
	var disarmed: bool = await _wait_for(func(): return not app.client.get_telemetry().get("motors_spinning", true), 6.0)
	_check(disarmed, "Betaflight fails safe: the motors stop within 6 s of the cut")
	_press(app, KEY_K)  # the pilot's own failsafe test works through the key too
	_press(app, KEY_K)

	# Reload: Betaflight is stopped and started again, and the pilot link comes back.
	app.sticks_override = _sticks(0.0, false)
	var before_reload: float = app.client.get_telemetry()["time_s"]
	_press(app, KEY_R)
	var reloaded: bool = await _wait_for(func(): return app.client.get_phase() == "flying" and app.client.get_telemetry().get("time_s", 1e9) < before_reload, 60.0)
	_check(reloaded, "R reloads: Betaflight restarts and simulated time restarts")
	var rearmed_ok: bool = await _wait_for(func(): return app.client.get_telemetry().get("time_s", 0.0) > 6.0, 30.0)
	app.sticks_override = _sticks(0.0, true)
	var armed_again: bool = await _wait_for(func(): return app.client.get_telemetry().get("motors_spinning", false), 10.0)
	_check(rearmed_ok and armed_again, "and it arms again")

	app.queue_free()
	await process_frame
	await process_frame
	print("E2E %s" % ("PASSED" if failures == 0 else "FAILED (%d)" % failures))
	quit(1 if failures > 0 else 0)