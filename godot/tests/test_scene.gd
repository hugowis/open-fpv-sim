extends "res://tests/testing.gd"
## The world, the drone and its cameras, built from their scripts (no simulator needed).

const World = preload("res://world/world.gd")
const Drone = preload("res://drone/drone.gd")
const FpvCamera = preload("res://drone/fpv_camera.gd")
const ChaseCamera = preload("res://drone/chase_camera.gd")
const Lens = preload("res://ui/lens.gd")


func test_the_world_has_ground_pad_and_landmarks() -> void:
	var world := World.new()
	await add_to_tree(world)
	ok(world.get_node_or_null("Ground") is MeshInstance3D, "ground")
	ok(world.get_node_or_null("LaunchPad") != null, "launch pad")
	ok(world.get_node_or_null("Pylon1L") != null and world.get_node_or_null("Pylon8R") != null, "pylons")
	ok(world.get_node_or_null("Gate0Bar") != null and world.get_node_or_null("Gate2PostR") != null, "gates")
	ok(world.get_node_or_null("BuildingA") != null, "buildings")
	var environments := world.find_children("*", "WorldEnvironment", false, false)
	var suns := world.find_children("*", "DirectionalLight3D", false, false)
	eq(environments.size(), 1, "one environment")
	eq(suns.size(), 1, "one sun")
	var ground: MeshInstance3D = world.get_node("Ground")
	ok(ground.material_override is ShaderMaterial, "the ground is drawn by the grid shader")
	world.queue_free()


func test_the_world_is_only_scenery() -> void:
	var world := World.new()
	await add_to_tree(world)
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
