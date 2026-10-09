extends "res://tests/testing.gd"
## The analog video layer's logic (headless has no renderer, so these check the amounts and when the layer draws, not
## pixels; tests/shots.gd draws it).

const Video = preload("res://ui/video.gd")


func _layer() -> CanvasLayer:
	var layer := Video.new()
	await add_to_tree(layer)
	return layer


func _telemetry(overrides := {}) -> Dictionary:
	var t := {"video_present": true, "video_snr_db": 30.0, "video_sync": "locked", "video_noise": 0.0, "video_sparkles": 0.0,
		"video_chroma": 1.0, "video_antenna": "omni", "video_rssi": {"omni": -60.0}}
	t.merge(overrides, true)
	return t


func test_the_amounts_follow_the_telemetry() -> void:
	var u := Video.uniforms_for(_telemetry({"video_noise": 0.4, "video_sparkles": 0.2, "video_chroma": 0.5, "video_sync": "unstable"}))
	eq(u, {"active": true, "noise": 0.4, "sparkles": 0.2, "chroma": 0.5, "sync": 1}, "unstable")
	eq(Video.uniforms_for(_telemetry({"video_sync": "lost"}))["sync"], 2, "lost")
	eq(Video.uniforms_for(_telemetry())["sync"], 0, "locked")
	eq(Video.uniforms_for(_telemetry({"video_noise": 7.0, "video_chroma": -1.0}))["noise"], 1.0, "clamped")
	eq(Video.uniforms_for({})["active"], false, "no telemetry: no link")
	eq(Video.uniforms_for(_telemetry({"video_present": false}))["active"], false, "a quad without a VTX")
	ok(Video.is_clean(Video.uniforms_for(_telemetry())), "a locked link without noise is clean")
	ok(not Video.is_clean(Video.uniforms_for(_telemetry({"video_chroma": 0.9}))), "fading colour is not")


func test_the_layer_draws_only_over_a_degraded_fpv_picture() -> void:
	var layer := await _layer()
	ok(not layer.is_active(), "nothing before the first telemetry")
	layer.update_view(_telemetry(), 0.016)
	ok(not layer.is_active(), "a clean link: pass-through, the layer stays off")
	layer.update_view(_telemetry({"video_noise": 0.5}), 0.016)
	ok(layer.is_active(), "grain: the layer draws")
	layer.set_enabled(false)
	ok(not layer.is_active(), "off in the chase view")
	layer.set_enabled(true)
	layer.effects = false
	ok(not layer.is_active(), "off with the Video effects setting off")
	layer.effects = true
	ok(layer.is_active(), "and back")
	layer.update_view(_telemetry({"video_present": false, "video_noise": 0.5}), 0.016)
	ok(not layer.is_active(), "off for a quad without a VTX")
	layer.queue_free()


func test_a_lost_picture_rolls_and_a_locked_one_does_not() -> void:
	var layer := await _layer()
	var material: ShaderMaterial = layer.get_node("VideoRect").material
	layer.update_view(_telemetry({"video_sync": "lost", "video_noise": 1.0}), 0.1)
	var first: float = material.get_shader_parameter("roll")
	layer.update_view(_telemetry({"video_sync": "lost", "video_noise": 1.0}), 0.1)
	var second: float = material.get_shader_parameter("roll")
	near(second - first, Video.ROLL_SPEED * 0.1, "rolls at its speed", 1e-5)
	near(material.get_shader_parameter("noise"), 1.0, "static")
	layer.update_view(_telemetry({"video_sync": "unstable", "video_noise": 0.7}), 0.1)
	near(material.get_shader_parameter("roll"), 0.0, "relocked: the roll stops")
	near(material.get_shader_parameter("tear"), 1.0, "unstable: tearing")
	layer.update_view(_telemetry({"video_noise": 0.3}), 0.1)
	near(material.get_shader_parameter("tear"), 0.0, "locked: no tearing")
	layer.queue_free()
