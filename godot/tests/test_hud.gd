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
		"radio_antenna": "antenna", "downlink_lq_pct": 100.0, "fc_battery_v": 24.6, "fc_battery_a": 3.2,
		"fc_battery_mah": 120.0, "fc_roll_deg": 1.0, "fc_pitch_deg": -2.0, "fc_yaw_deg": 90.0,
		"fc_flight_mode": "ACRO", "fc_frame_count": 120, "fc_telemetry_age_s": 0.05,
		"collision_speed_mps": 0.0, "running": true,
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


func test_the_status_line_shows_when_the_sticks_are_being_recorded() -> void:
	var hud := await _hud()
	hud.update_view(_view())
	ok(not hud.sim_text().contains("REC"), "not recording: %s" % hud.sim_text())
	hud.update_view(_view({"recording_inputs": true}))
	ok(hud.sim_text().contains("REC STICKS"), "the indicator: %s" % hud.sim_text())
	hud.update_view(_view({"telemetry": {}, "recording_inputs": true}))
	ok(hud.sim_text().contains("REC STICKS"), "and before the first state: %s" % hud.sim_text())
	hud.queue_free()


func test_the_dvr_indicator_is_a_separate_rec_label() -> void:
	var hud := await _hud()
	eq(hud._rec.text, "REC", "the label")
	ok(not hud._rec.visible, "hidden at first")
	hud.set_dvr_recording(true)
	ok(hud._rec.visible, "shown while recording")
	hud.set_hud_visible(false)
	ok(not hud._rec.visible, "hidden with the HUD")
	hud.set_hud_visible(true)
	ok(hud._rec.visible, "shown again with the HUD while recording")
	hud.set_dvr_recording(false)
	ok(not hud._rec.visible, "hidden when recording stops")
	hud.queue_free()


func test_the_lap_line_shows_the_race() -> void:
	var hud := await _hud()
	hud.update_view(_view())
	eq(hud.race_text(), "", "a world without gates: no line")
	var waiting := _telemetry({"race_present": true, "race_running": false, "race_gate_count": 3})
	hud.update_view(_view({"telemetry": waiting}))
	eq(hud.race_text(), "RACE  fly through the start gate", "before the clock starts")
	var racing := _telemetry({"race_present": true, "race_running": true, "race_gate_count": 5, "race_next_gate": 3, "race_laps": 0, "race_lap_s": 12.4})
	hud.update_view(_view({"telemetry": racing}))
	eq(hud.race_text(), "LAP 1   NEXT 3   0:12.4", "no LAST or BEST before a lap completes")
	var second := _telemetry({"race_present": true, "race_running": true, "race_gate_count": 5, "race_next_gate": 0, "race_laps": 1,
		"race_lap_s": 75.3, "race_last_lap_s": 31.2, "race_best_lap_s": 29.9})
	hud.update_view(_view({"telemetry": second}))
	eq(hud.race_text(), "LAP 2   NEXT START   1:15.3   LAST 31.2   BEST 29.9", "the start gate is next once all are passed")
	var almost_a_minute := _telemetry({"race_present": true, "race_running": true, "race_gate_count": 5, "race_next_gate": 3, "race_laps": 0, "race_lap_s": 59.96})
	hud.update_view(_view({"telemetry": almost_a_minute}))
	eq(hud.race_text(), "LAP 1   NEXT 3   1:00.0", "rounding to the tenth carries into the minute, never 0:60.0")
	var just_before := _telemetry({"race_present": true, "race_running": true, "race_gate_count": 5, "race_next_gate": 3, "race_laps": 0, "race_lap_s": 59.94})
	hud.update_view(_view({"telemetry": just_before}))
	eq(hud.race_text(), "LAP 1   NEXT 3   0:59.9", "59.94 rounds down")
	hud.update_view(_view({"telemetry": {}}))
	eq(hud.race_text(), "", "no state, no line")
	var limited := _telemetry({"race_present": true, "race_running": true, "race_gate_count": 5, "race_next_gate": 2, "race_laps": 1, "race_lap_limit": 3,
		"race_lap_s": 5.0, "race_last_lap_s": 31.2, "race_best_lap_s": 29.9})
	hud.update_view(_view({"telemetry": limited}))
	eq(hud.race_text(), "LAP 2/3   NEXT 2   0:05.0   LAST 31.2   BEST 29.9", "the lap limit shows beside the lap")
	var finished := _telemetry({"race_present": true, "race_running": false, "race_finished": true, "race_gate_count": 5, "race_laps": 3, "race_lap_limit": 3,
		"race_best_lap_s": 29.9})
	hud.update_view(_view({"telemetry": finished}))
	eq(hud.race_text(), "RACE FINISHED   3 LAPS   BEST 29.9   (N: new race)", "the finished race says so and how to start another")
	hud.queue_free()


func test_the_lap_line_names_the_course_and_a_time_attack_counts_down() -> void:
	var hud := await _hud()
	var named := _telemetry({"race_present": true, "race_running": true, "race_course": "Diamond", "race_next_gate": 2, "race_laps": 1, "race_lap_limit": 5,
		"race_lap_s": 5.0, "race_last_lap_s": 31.2, "race_best_lap_s": 29.9})
	hud.update_view(_view({"telemetry": named}))
	eq(hud.race_text(), "DIAMOND   LAP 2/5   NEXT 2   0:05.0   LAST 31.2   BEST 29.9", "the course leads the lap line")
	var waiting := _telemetry({"race_present": true, "race_running": false, "race_course": "Hunt", "race_mode": "time_attack", "race_time_limit_s": 60.0})
	hud.update_view(_view({"telemetry": waiting}))
	eq(hud.race_text(), "HUNT   RACE  fly through the start gate   (60 s)", "a time attack says how long it runs")
	var hunting := _telemetry({"race_present": true, "race_running": true, "race_course": "Hunt", "race_mode": "time_attack", "race_time_limit_s": 60.0,
		"race_time_left_s": 41.24, "race_next_gate": 0, "race_laps": 2, "race_lap_s": 12.3, "race_last_lap_s": 29.9, "race_best_lap_s": 29.1})
	hud.update_view(_view({"telemetry": hunting}))
	eq(hud.race_text(), "HUNT   LEFT 0:41.2   LAP 3   NEXT START   0:12.3   LAST 29.9   BEST 29.1", "the clock counts down, laps count up")
	var over := _telemetry({"race_present": true, "race_finished": true, "race_course": "Hunt", "race_mode": "time_attack", "race_laps": 4, "race_best_lap_s": 29.1})
	hud.update_view(_view({"telemetry": over}))
	eq(hud.race_text(), "HUNT   TIME UP   4 LAPS   BEST 29.1   (N: new race)", "the end of a time attack")
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
	ok(hud.link_text().contains("SNR –"), "down: no bogus SNR number ('%s')" % hud.link_text())
	hud.queue_free()


func test_the_battery_line_shows_betaflight_while_fresh_and_dashes_when_stale() -> void:
	var hud := await _hud()
	hud.update_view(_view())
	eq(hud.battery_text(), "24.60 V   3.2 A   ACRO", "fresh: Betaflight's values and its mode")
	hud.update_view(_view({"telemetry": _telemetry({"fc_telemetry_age_s": 1.5})}))
	eq(hud.battery_text(), "-- V   -- A   --", "stale: dashes")
	hud.update_view(_view({"telemetry": _telemetry({"fc_telemetry_age_s": INF})}))
	eq(hud.battery_text(), "-- V   -- A   --", "never arrived (open loop): dashes")
	hud.queue_free()
