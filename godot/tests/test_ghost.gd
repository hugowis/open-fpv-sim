extends "res://tests/testing.gd"
## The ghost's playback and delta (#133).

const Ghost = preload("res://drone/ghost.gd")


func _track() -> Dictionary:
	var times := PackedFloat32Array()
	var positions := PackedFloat32Array()
	var rotations := PackedFloat32Array()
	for i in 5:
		times.append(i * 0.5)
		positions.append_array(PackedFloat32Array([float(i), 0.0, 0.0]))
		rotations.append_array(PackedFloat32Array([0.0, 0.0, 0.0, 1.0]))
	return {"times": times, "positions": positions, "rotations": rotations}


func test_a_sample_time_is_that_pose() -> void:
	var ghost = Ghost.new()
	ghost.setup(_track())
	near(ghost.pose_at(1.0).origin.x, 2.0, "sample 2", 0.001)
	near(ghost.pose_at(0.5).origin.x, 1.0, "sample 1", 0.001)
	ghost.free()


func test_between_samples_interpolates() -> void:
	var ghost = Ghost.new()
	ghost.setup(_track())
	near(ghost.pose_at(0.25).origin.x, 0.5, "the midpoint", 0.001)
	near(ghost.pose_at(0.75).origin.x, 1.5, "the three-quarter point", 0.001)
	ghost.free()


func test_outside_the_track_clamps_to_the_ends() -> void:
	var ghost = Ghost.new()
	ghost.setup(_track())
	near(ghost.pose_at(-1.0).origin.x, 0.0, "before the start", 0.001)
	near(ghost.pose_at(9.0).origin.x, 4.0, "past the finish", 0.001)
	ghost.free()


func test_delta_is_negative_ahead_and_positive_behind() -> void:
	var ghost = Ghost.new()
	ghost.setup(_track())
	var pilot := ghost.pose_at(1.0).origin
	near(ghost.delta(pilot, 0.7), -0.3, "the pilot is ahead", 0.001)
	near(ghost.delta(pilot, 1.3), 0.3, "the pilot is behind", 0.001)
	ghost.free()


func test_delta_survives_a_pilot_far_off_the_line() -> void:
	var ghost = Ghost.new()
	ghost.setup(_track())
	var d := ghost.delta(Vector3(50.0, 50.0, 0.0), 1.0)
	ok(is_finite(d), "a finite delta far away: %s" % d)
	ghost.free()


func test_delta_without_a_track_is_nan() -> void:
	var ghost = Ghost.new()
	ok(is_nan(ghost.delta(Vector3.ZERO, 1.0)), "no track, no delta")
	ghost.free()


func test_an_empty_track_clears() -> void:
	var ghost = Ghost.new()
	ghost.setup(_track())
	ok(ghost.has_track(), "a track")
	ghost.setup({})
	ok(not ghost.has_track(), "setup({}) clears it")
	ghost.free()


func _long_track() -> Dictionary:
	var times := PackedFloat32Array()
	var positions := PackedFloat32Array()
	var rotations := PackedFloat32Array()
	for i in 600:
		times.append(i * 0.1)
		positions.append_array(PackedFloat32Array([float(i), 0.0, 0.0]))
		rotations.append_array(PackedFloat32Array([0.0, 0.0, 0.0, 1.0]))
	return {"times": times, "positions": positions, "rotations": rotations}


func test_delta_widens_to_the_true_nearest_when_the_cursor_is_parked() -> void:
	var ghost = Ghost.new()
	ghost.setup(_long_track())
	near(ghost.delta(Vector3(400.0, 0.0, 0.0), 40.0), 0.0, "the nearest sample, not the window edge", 0.001)
	ghost.free()


func test_the_model_is_translucent() -> void:
	var ghost = Ghost.new()
	await add_to_tree(ghost)
	ghost.setup(_track())
	var meshes := ghost.find_children("*", "MeshInstance3D", true, false)
	ok(not meshes.is_empty(), "the model has meshes")
	var shared = null
	for mesh in meshes:
		var material = (mesh as MeshInstance3D).material_override
		ok(material is StandardMaterial3D, "a material override")
		if material is StandardMaterial3D:
			if shared == null:
				shared = material
			ok(is_same(material, shared), "the same ghost material, discs included")
			near(material.albedo_color.a, 0.35, "the ghost alpha", 0.001)
			ok(material.transparency != BaseMaterial3D.TRANSPARENCY_DISABLED, "alpha")
	ghost.queue_free()
