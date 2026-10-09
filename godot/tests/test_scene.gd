extends "res://tests/testing.gd"
## The world, the drone and its cameras, built from their scripts (no simulator needed).

const World = preload("res://world/world.gd")
const Drone = preload("res://drone/drone.gd")
const FpvCamera = preload("res://drone/fpv_camera.gd")
const ChaseCamera = preload("res://drone/chase_camera.gd")
const Lens = preload("res://ui/lens.gd")


## A world like the one `OfsClient.get_world()` returns (Godot's frame).
func _world_dict() -> Dictionary:
	return {
		"name": "Test field",
		"pilot_position": Vector3(2.0, 1.7, 3.0),
		"antennas": [
			{"name": "omni", "kind": "omni", "aim": Vector3.UP},
			{"name": "patch", "kind": "patch", "aim": Vector3(0.0, 0.17, -0.98).normalized()},
		],
		"objects": [
			{"name": "BuildingA", "shape": "box", "center": Vector3(-45.0, 7.0, -110.0), "size": Vector3(14.0, 14.0, 12.0),
				"radius": 0.0, "height": 0.0, "color": Color(0.62, 0.64, 0.66), "rf_loss_db": 25.0},
			{"name": "Pylon1L", "shape": "cylinder", "center": Vector3(-6.0, 1.5, -15.0), "size": Vector3.ZERO,
				"radius": 0.15, "height": 3.0, "color": Color(0.92, 0.92, 0.9), "rf_loss_db": 0.0},
		],
		"emitters": [{"name": "parked-quad", "position": Vector3(-60.0, 1.0, -30.0), "freq_mhz": 5695.0, "power_mw": 25.0}],
	}


func test_the_world_draws_sky_and_ground_and_builds_the_rest_from_the_world_file() -> void:
	var world := World.new()
	await add_to_tree(world)
	ok(world.get_node_or_null("Ground") is MeshInstance3D, "ground")
	var ground: MeshInstance3D = world.get_node("Ground")
	ok(ground.material_override is ShaderMaterial, "the ground is drawn by the grid shader")
	eq(world.find_children("*", "WorldEnvironment", false, false).size(), 1, "one environment")
	eq(world.find_children("*", "DirectionalLight3D", false, false).size(), 1, "one sun")
	eq(world.built_names(), PackedStringArray(), "no objects before the simulator sends a world")
	world.build(_world_dict())
	var building: MeshInstance3D = world.get_node("BuildingA")
	eq(building.position, Vector3(-45.0, 7.0, -110.0), "a box where the world file puts it")
	eq((building.mesh as BoxMesh).size, Vector3(14.0, 14.0, 12.0), "at its size")
	eq((building.material_override as StandardMaterial3D).albedo_color, Color(0.62, 0.64, 0.66), "in its colour")
	var pylon: MeshInstance3D = world.get_node("Pylon1L")
	near((pylon.mesh as CylinderMesh).height, 3.0, "a cylinder")
	near((pylon.mesh as CylinderMesh).top_radius, 0.15, "its radius")
	var pilot: Node3D = world.get_node("Pilot")
	eq(pilot.position, Vector3(2.0, 1.7, 3.0), "the pilot marker's head is at the goggles")
	ok(pilot.get_node_or_null("Aim_patch") != null and pilot.get_node_or_null("Aim_omni") == null, "a pointer for the patch only")
	var emitter: Node3D = world.get_node("Emitter_parked-quad")
	eq(emitter.position, Vector3(-60.0, 0.0, -30.0), "the emitter's post stands on the ground below it")
	ok((emitter.get_node("Label") as Label3D).text.contains("5695 MHz"), "labelled with its frequency")
	world.queue_free()


func test_a_new_world_replaces_the_old_one() -> void:
	var world := World.new()
	await add_to_tree(world)
	world.build(_world_dict())
	eq(world.built_names().size(), 4, "two objects, the pilot, one emitter")
	var smaller := _world_dict()
	smaller["objects"] = [smaller["objects"][1]]
	smaller["emitters"] = []
	world.build(smaller)
	eq(world.built_names(), PackedStringArray(["Pylon1L", "Pilot"]), "only the new world's nodes")
	ok(world.get_node_or_null("BuildingA") == null, "the old building is gone at once")
	world.build({})
	eq(world.built_names(), PackedStringArray(), "an empty world clears the field")
	ok(world.get_node_or_null("Ground") != null, "but keeps the ground")
	world.queue_free()


func test_the_world_is_only_scenery() -> void:
	var world := World.new()
	await add_to_tree(world)
	world.build(_world_dict())
	eq(world.find_children("*", "CollisionObject3D", true, false).size(), 0, "nothing to collide with: the simulator knows only the ground")
	world.queue_free()


func test_the_drone_has_four_propellers_that_follow_the_motor_commands() -> void:
	var drone := Drone.new()
	await add_to_tree(drone)
	eq(drone._props.size(), 4, "four propellers")
	var before: Array = drone._props.map(func(p: Node3D) -> float: return p.rotation.y)
	drone.set_motors(PackedFloat32Array([0.0, 0.5, 1.0, 0.5]), 0.01)
	var after: Array = drone._props.map(func(p: Node3D) -> float: return p.rotation.y)
	near(after[0], before[0], "an idle motor does not turn its propeller")
	ok(absf(after[2] - before[2]) > absf(after[1] - before[1]), "a harder commanded motor turns faster")
	# Betaflight's default directions: M1 and M4 clockwise (negative about +Y), M2 and M3 counter-clockwise.
	ok(after[1] > before[1] and after[2] > before[2], "M2 and M3 turn counter-clockwise")
	drone.set_motors(PackedFloat32Array([1.0, 0.0, 0.0, 1.0]), 0.01)
	ok(drone._props[0].rotation.y < after[0] and drone._props[3].rotation.y < after[3], "M1 and M4 turn clockwise")
	drone.set_motors(PackedFloat32Array([1.0]), 0.01)  # fewer commands than propellers: no error
	drone.queue_free()


func test_the_fpv_camera_looks_forward_and_up() -> void:
	var drone := Drone.new()
	var camera := FpvCamera.new()
	drone.add_child(camera)
	await add_to_tree(drone)
	eq(camera.keep_aspect, Camera3D.KEEP_WIDTH, "the field of view is horizontal")
	near(camera.fov, 110.0, "default field of view")
	near(camera.rotation_degrees.x, 30.0, "default up-tilt")
	var view := -camera.global_transform.basis.z
	ok(view.y > 0.45 and view.z < -0.8, "looks forward (-Z) and up: %s" % str(view))
	camera.uptilt_deg = 0.0
	camera.fov_h_deg = 90.0
	near(camera.rotation_degrees.x, 0.0, "up-tilt changes at once")
	near(camera.fov, 90.0, "so does the field of view")
	drone.queue_free()


func test_the_chase_camera_follows_from_behind_and_above() -> void:
	var drone := Drone.new()
	drone.name = "Drone"
	var chase := ChaseCamera.new()
	chase.name = "Chase"
	var holder := Node3D.new()
	holder.add_child(drone)
	holder.add_child(chase)
	chase.target_path = ^"../Drone"
	await add_to_tree(holder)
	var start_offset := chase.global_position - drone.global_position
	ok(start_offset.z > 1.0 and start_offset.y > 0.3, "starts behind (+Z when facing -Z) and above: %s" % str(start_offset))
	drone.global_position = Vector3(10.0, 5.0, -20.0)
	for i in 120:
		chase._process(0.02)
	ok(chase.global_position.distance_to(drone.global_position + start_offset) < 0.05, "catches up with the drone")
	# Facing east: the camera moves to the west side.
	drone.rotation.y = -PI / 2.0
	for i in 200:
		chase._process(0.02)
	ok(chase.global_position.x < drone.global_position.x - 1.0, "follows the heading: %s" % str(chase.global_position - drone.global_position))
	holder.queue_free()


func test_the_lens_toggles_and_feeds_its_shader() -> void:
	var lens := Lens.new()
	await add_to_tree(lens)
	var rect: ColorRect = lens.get_node("LensRect")
	ok(rect.material is ShaderMaterial, "a shader material")
	near(rect.material.get_shader_parameter("k1"), 0.06, "default distortion")
	lens.distortion = 0.2
	lens.exposure = 1.5
	near(rect.material.get_shader_parameter("k1"), 0.2, "distortion reaches the shader")
	near(rect.material.get_shader_parameter("exposure"), 1.5, "and exposure")
	lens.set_enabled(false)
	ok(not lens.visible, "hidden for the chase view")
	lens.queue_free()
