extends "res://tests/testing.gd"

const Hud = preload("res://ui/hud.gd")


func _hud() -> CanvasLayer:
	var hud := Hud.new()
	await add_to_tree(hud)
	return hud


func _telemetry(overrides := {}) -> Dictionary:
	var t := {
		"time_s": 12.3, "altitude_m": 4.5, "speed_mps": 6.7, "climb_mps": -0.4, "battery_voltage_v": 24.1,
		"battery_current_a": 12.0, "motor_cmd": PackedFloat32Array([0.3, 0.3, 0.3, 0.3]), "motors_spinning": true,
		"tx_enabled": true, "link_up": true, "lq_pct": 100.0, "rssi_dbm": -50.0, "radio_snr_db": 49.0,
		"radio_antenna": "antenna", "downlink_lq_pct": 100.0, "collision_speed_mps": 0.0, "running": true,
		"overruns": 0, "fc_restarts": 0, "age_s": 0.01,
	}
	t.merge(overrides, true)
	return t


func _view(overrides := {}) -> Dictionary:
	var v := {"phase": "flying", "detail": "", "telemetry": _telemetry(), "sticks": {}, "controls_status": "",
		"configurator": "", "quad": "Test quad", "camera": "FPV", "radio_cut": false}
	v.merge(overrides, true)
	return v


func test_a_healthy_flight_has_no_banner() -> void:
	var hud := await _hud()
	hud.update_view(_view())
	eq(hud.banner_text(), "", "no banner")
	hud.queue_free()


func test_each_problem_has_its_banner() -> void:
	var hud := await _hud()
	hud.update_view(_view({"phase": "failed", "detail": "no ofs-sim answers on 127.0.0.1:50051"}))
	ok(hud.banner_text().contains("no ofs-sim answers") and hud.banner_text().contains("R to retry"), "failed: %s" % hud.banner_text())
	hud.update_view(_view({"phase": "loading", "detail": "loading quad.toml (Betaflight takes a few seconds to boot)", "telemetry": {}}))
	ok(hud.banner_text().begins_with("loading quad.toml"), "loading: %s" % hud.banner_text())
	hud.update_view(_view({"phase": "connecting", "detail": "", "telemetry": {}}))
	eq(hud.banner_text(), "Connecting...", "connecting without a detail")
	hud.update_view(_view({"phase": "stopped", "telemetry": {}}))
	eq(hud.banner_text(), "Disconnected", "stopped")
	hud.update_view(_view({"telemetry": _telemetry({"age_s": 2.0})}))
	ok(hud.banner_text().begins_with("NO DATA"), "stale telemetry: %s" % hud.banner_text())
	hud.update_view(_view({"radio_cut": true}))
	ok(hud.banner_text().begins_with("RADIO LINK CUT"), "cut by the pilot: %s" % hud.banner_text())
	hud.update_view(_view({"telemetry": _telemetry({"link_up": false})}))
	ok(hud.banner_text().begins_with("RADIO LINK LOST"), "lost: %s" % hud.banner_text())
	hud.update_view(_view({"controls_status": "Controller not connected: the sticks are centred"}))
	ok(hud.banner_text().begins_with("Controller not connected"), "controller: %s" % hud.banner_text())
	hud.update_view(_view({"phase": "paused"}))
	ok(hud.banner_text().begins_with("PAUSED"), "paused: %s" % hud.banner_text())
	hud.queue_free()


func test_a_fatal_message_stays() -> void:
	var hud := await _hud()
	hud.show_fatal("The OfsClient extension is not loaded.")
	hud.update_view(_view())
	eq(hud.banner_text(), "The OfsClient extension is not loaded.", "the banner is not overwritten")
	hud.queue_free()


func test_toasts_are_limited_and_expire() -> void:
	var hud := await _hud()
	for i in 10:
		hud.add_toast("event %d" % i)
	eq(hud.toast_texts().size(), Hud.MAX_TOASTS, "only the newest are kept")
	eq(hud.toast_texts()[-1], "event 9", "newest last")
	eq(hud.toast_texts()[0], "event %d" % (10 - Hud.MAX_TOASTS), "oldest first")
	hud._process(Hud.TOAST_SECONDS + 0.1)
	eq(hud.toast_texts().size(), 0, "they fade away")
	hud.queue_free()


func test_the_hud_can_be_hidden_and_the_help_shown() -> void:
	var hud := await _hud()
	ok(hud.hud_visible(), "visible at first")
	hud.toggle_hud()
	ok(not hud.hud_visible(), "hidden")
	hud.toggle_hud()
	ok(hud.hud_visible(), "visible again")
	ok(not hud.help_visible(), "no help at first")
	hud.set_help_visible(true)
	ok(hud.help_visible(), "help shown")
	hud.queue_free()


func test_the_vtx_line_shows_channel_frequency_and_power() -> void:
	var hud := await _hud()
	hud.update_view(_view())
	eq(hud.vtx_text(), "", "no VTX in the telemetry")
	hud.update_view(_view({"telemetry": _telemetry({"vtx_present": true, "vtx_channel_name": "R3", "vtx_freq_mhz": 5732, "vtx_power_mw": 600, "vtx_pit_mode": false})}))
	eq(hud.vtx_text(), "VTX R3  5732 MHz  600 mW", "the line")
	hud.update_view(_view({"telemetry": _telemetry({"vtx_present": true, "vtx_channel_name": "", "vtx_freq_mhz": 5800, "vtx_power_mw": 25, "vtx_pit_mode": true})}))
	eq(hud.vtx_text(), "VTX user  5800 MHz  25 mW  PIT", "user frequency in pit mode")
	hud.update_view(_view({"telemetry": {}}))
	eq(hud.vtx_text(), "", "cleared with the telemetry")
	hud.queue_free()


func test_updates_cope_with_missing_data() -> void:
	var hud := await _hud()
	hud.update_view({})
	hud.update_view({"phase": "flying", "telemetry": {}})
	hud.update_view(_view({"sticks": {"roll": 0.1, "pitch": -0.2, "yaw": 0.0, "throttle": 0.5, "aux": [1.0, -1.0, -1.0, -1.0]}}))
	ok(true, "no errors")
	hud.queue_free()


func test_the_video_line_shows_the_signal_and_its_antenna() -> void:
	var hud := await _hud()
	hud.update_view(_view())
	eq(hud.video_text(), "", "no video link in the telemetry")
	var video := {"video_present": true, "video_snr_db": 18.4, "video_sync": "locked", "video_noise": 0.2,
		"video_antenna": "patch", "video_rssi": {"omni": -78.2, "patch": -71.4}}
	hud.update_view(_view({"telemetry": _telemetry(video)}))
	eq(hud.video_text(), "VID 18 dB  patch  -71 dBm", "the line")
	video["video_sync"] = "lost"
	video["video_snr_db"] = -2.6
	hud.update_view(_view({"telemetry": _telemetry(video)}))
	eq(hud.video_text(), "VID -3 dB  patch  -71 dBm  NO SYNC", "without sync")
	hud.update_view(_view({"telemetry": {}}))
	eq(hud.video_text(), "", "cleared with the telemetry")
	hud.queue_free()


func test_the_link_line_carries_the_snr() -> void:
	var hud := await _hud()
	hud.update_view(_view())
	eq(hud.link_text(), "LINK UP   LQ 100 %   -50 dBm   SNR 49", "the link line")
	hud.update_view(_view({"telemetry": _telemetry({"link_up": false})}))
	ok(hud.link_text().begins_with("LINK DOWN"), "down: '%s'" % hud.link_text())
	hud.queue_free()
