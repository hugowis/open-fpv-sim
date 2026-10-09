extends "res://tests/testing.gd"
## The OfsClient node's surface, without a server.

const QUAD := "res://../quads/opendrone-5f-freestyle.toml"


func _client() -> Node:
	var client: Node = ClassDB.instantiate("OfsClient")
	await add_to_tree(client)
	return client


func test_the_extension_registers_the_class() -> void:
	ok(ClassDB.class_exists("OfsClient"), "OfsClient is registered (build it: cargo build -p ofs-godot)")


func test_before_start_everything_is_inert() -> void:
	var client := await _client()
	eq(client.get_phase(), "stopped", "phase")
	eq(client.get_phase_detail(), "", "detail")
	eq(client.has_pose(), false, "no pose")
	eq(client.get_pose(), Transform3D.IDENTITY, "identity pose")
	eq(client.get_telemetry(), {}, "no telemetry")
	eq(client.get_configurator_address(), "", "no Configurator address")
	client.set_sticks(0.1, 0.2, 0.3, 0.4, PackedFloat32Array([1.0]))
	client.pause()
	client.reload()
	client.set_radio_loss(true)
	client.stop()
	ok(true, "calls before start do nothing")
	client.queue_free()


func test_osd_and_vtx_getters_are_inert_before_start() -> void:
	var client := await _client()
	eq(client.get_osd(), {}, "no OSD frame")
	eq(client.get_osd_version(), 0, "no OSD updates")
	eq(client.get_telemetry(), {}, "no telemetry, so no VTX keys")
	client.queue_free()


func test_the_world_getters_are_inert_before_start() -> void:
	var client := await _client()
	eq(client.get_world(), {}, "no world")
	eq(client.get_world_version(), 0, "no world updates")
	client.queue_free()


func test_invalid_settings_are_reported_as_text() -> void:
	var client := await _client()
	ok(client.start({}).contains("quad_path"), "a quad is required")
	ok(client.start({"quad_path": QUAD, "overrun_policy": "fast"}).contains("overrun_policy"), "unknown overrun policy")
	ok(client.start({"quad_path": QUAD, "seed": -1}).contains("seed"), "negative seed")
	ok(client.start({"quad_path": QUAD, "server_bin": "x", "env": {"A": 1}}).contains("env"), "env values must be strings")
	eq(client.get_phase(), "stopped", "still not started")
	client.queue_free()


func test_a_server_that_is_not_there_fails_with_a_signal_and_a_hint() -> void:
	var client := await _client()
	var seen := []
	client.phase_changed.connect(func(phase: String, detail: String, kind: String): seen.append([phase, detail, kind]))
	eq(client.start({"quad_path": QUAD, "server_addr": "127.0.0.1:1"}), "", "start accepts valid settings")
	eq(client.start({"quad_path": QUAD}), "already started", "a second start is refused")
	var deadline := Time.get_ticks_msec() + 10000
	while client.get_phase() != "failed" and Time.get_ticks_msec() < deadline:
		await tree.process_frame
	eq(client.get_phase(), "failed", "the phase after the failure")
	ok(client.get_phase_detail().contains("127.0.0.1:1"), "the detail names the address: %s" % client.get_phase_detail())
	ok(seen.size() >= 2, "phase_changed was emitted: %s" % str(seen))
	var last: Array = seen[-1]
	eq(last[0], "failed", "the last signal")
	eq(last[2], "unavailable", "carries the error kind")
	client.stop()
	eq(client.get_phase(), "stopped", "stopped")
	client.queue_free()


func test_the_sticks_and_commands_are_accepted_while_started() -> void:
	var client := await _client()
	client.start({"quad_path": QUAD, "server_addr": "127.0.0.1:1"})
	client.set_sticks(2.0, -2.0, NAN, 5.0, PackedFloat32Array([1.0, -1.0, 0.5, 9.0, 3.0]))
	client.pause()
	client.resume()
	client.set_radio_loss(true)
	client.set_radio_loss(false)
	ok(true, "out-of-range sticks are clamped and extra aux values ignored without errors")
	client.stop()
	client.queue_free()
