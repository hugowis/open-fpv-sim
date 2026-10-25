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


func test_gates_are_drawn_as_openings_facing_their_direction_of_travel() -> void:
	var world := World.new()
	await add_to_tree(world)
	world.build({"gates": [
		{"name": "A", "center": Vector3(0.0, 1.22, -30.0), "heading_deg": 0.0, "width": 3.08, "height": 2.44},
		{"name": "B", "center": Vector3(50.0, 1.5, 0.0), "heading_deg": 90.0, "width": 3.0, "height": 2.0},
	]})
	eq(world.built_names(), PackedStringArray(["Gate_A", "Gate_B"]), "one node per gate, in course order")
	var a: Node3D = world.get_node("Gate_A")
	eq(a.position, Vector3(0.0, 1.22, -30.0), "centred where the world file puts the opening")
	eq(((a.get_node("Opening") as MeshInstance3D).mesh as QuadMesh).size, Vector2(3.08, 2.44), "the opening's size")
	ok(a.global_transform.basis.z.distance_to(Vector3(0.0, 0.0, -1.0)) < 1e-5, "heading north faces -Z: %s" % str(a.global_transform.basis.z))
	var b: Node3D = world.get_node("Gate_B")
	ok(b.global_transform.basis.z.distance_to(Vector3(1.0, 0.0, 0.0)) < 1e-5, "heading east faces +X: %s" % str(b.global_transform.basis.z))
	eq((a.get_node("Label") as Label3D).text, "START", "gate 0 is the start and finish")
	eq((b.get_node("Label") as Label3D).text, "1", "the others carry their number")
	world.build({})
	eq(world.built_names(), PackedStringArray(), "an empty world clears the gates")
	world.queue_free()


func test_a_tilted_gate_is_drawn_on_its_tilted_plane() -> void:
	var upright := World.gate_basis(0.0)
	ok(upright.z.distance_to(Vector3(0.0, 0.0, -1.0)) < 1e-6 and upright.y.distance_to(Vector3.UP) < 1e-6, "north, upright")
	var pitched := World.gate_basis(0.0, 45.0)
	var s := sqrt(0.5)
	ok(pitched.z.distance_to(Vector3(0.0, s, -s)) < 1e-6, "a positive pitch lifts the direction of travel: %s" % str(pitched.z))
	ok(pitched.y.distance_to(Vector3(0.0, s, s)) < 1e-6, "and leans the top back toward the approach: %s" % str(pitched.y))
	var rolled := World.gate_basis(0.0, 0.0, 90.0)
	ok(rolled.z.distance_to(Vector3(0.0, 0.0, -1.0)) < 1e-6, "a roll keeps the direction of travel")
	ok(rolled.y.distance_to(Vector3(1.0, 0.0, 0.0)) < 1e-6, "a quarter turn clockwise from the approach puts the opening's up to the east: %s" % str(rolled.y))
	for b in [pitched, rolled, World.gate_basis(137.0, -33.0, 71.0)]:
		ok(absf(b.determinant() - 1.0) < 1e-6 and b.is_conformal(), "a proper rotation")
	var world := World.new()
	await add_to_tree(world)
	world.build({"gates": [{"name": "T", "center": Vector3(0.0, 3.0, -30.0), "heading_deg": 0.0, "pitch_deg": 45.0, "roll_deg": 0.0, "width": 3.0, "height": 2.0}]})
	var t: Node3D = world.get_node("Gate_T")
	ok(t.global_transform.basis.z.distance_to(Vector3(0.0, s, -s)) < 1e-5, "the node carries the tilt: %s" % str(t.global_transform.basis.z))
	world.queue_free()


func test_the_next_gate_is_highlighted_and_the_start_gate_stays_distinct() -> void:
	var next_color := World.gate_color(2, 2)
	var plain := World.gate_color(1, 2)
	var start := World.gate_color(0, 2)
	ok(next_color.a > plain.a and next_color.g > next_color.r, "the next gate is stronger and green: %s" % str(next_color))
	ok(plain.is_equal_approx(Color(0.3, 0.85, 1.0, 0.25)), "the others stay translucent cyan")
	ok(start.is_equal_approx(Color(1.0, 0.85, 0.2, 0.25)), "the start gate stays yellow while it is not next")
	ok(World.gate_color(0, 0).is_equal_approx(next_color), "and takes the highlight when it is next")
	var world := World.new()
	await add_to_tree(world)
	world.build({"gates": [
		{"name": "A", "center": Vector3(0.0, 1.22, -30.0), "heading_deg": 0.0, "width": 3.0, "height": 2.0},
		{"name": "B", "center": Vector3(0.0, 1.22, -60.0), "heading_deg": 0.0, "width": 3.0, "height": 2.0},
	]})
	var opening := func(n: String) -> Color: return ((world.get_node(n).get_node("Opening") as MeshInstance3D).material_override as StandardMaterial3D).albedo_color
	world.set_next_gate(1)
	ok(opening.call("Gate_B").is_equal_approx(next_color), "gate 1 is highlighted")
	ok(opening.call("Gate_A").is_equal_approx(start), "gate 0 is not")
	world.set_next_gate(0)
	ok(opening.call("Gate_A").is_equal_approx(next_color) and opening.call("Gate_B").is_equal_approx(plain), "the highlight follows the next gate at once")
	world.set_next_gate(-1)
	ok(opening.call("Gate_A").is_equal_approx(start) and opening.call("Gate_B").is_equal_approx(plain), "none highlighted when there is no next gate")
	world.build({"gates": [{"name": "C", "center": Vector3.ZERO, "heading_deg": 0.0, "width": 1.0, "height": 1.0}]})
	world.set_next_gate(0)
	ok(opening.call("Gate_C").is_equal_approx(next_color), "a rebuilt world is highlighted too")
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


func test_the_drone_is_a_modelled_quad_the_fpv_camera_never_sees() -> void:
	var drone := Drone.new()
	await add_to_tree(drone)
	for i in [1, 2, 3, 4]:
		ok(drone.get_node_or_null("Motor%d" % i) != null, "motor bell %d" % i)
		ok(drone.get_node_or_null("Prop%d" % i) != null, "prop %d" % i)
	ok(drone.get_node_or_null("Antenna1") != null and drone.get_node_or_null("Antenna2") != null, "two VTX antennas")
	var eye := Vector3(0.0, 0.02, -0.04)  # fpv_camera.gd's _ready position, near = 0.02
	var housing: MeshInstance3D = drone.get_node("CameraHousing")
	eq(housing.position, eye, "the camera housing sits at the FPV camera's eye point")
	var corner: float = ((housing.mesh as BoxMesh).size * 0.5).length()
	ok(corner < 0.02, "the housing hides inside the near clip (corner %f)" % corner)
	var plate: MeshInstance3D = drone.get_node("Plate")
	var plate_nose: float = plate.position.z - (plate.mesh as BoxMesh).size.z * 0.5
	ok(plate_nose >= eye.z, "the plate's nose stays behind the FPV camera")
	var battery_material: StandardMaterial3D = drone.get_node("Battery").material_override
	ok(battery_material.albedo_color.is_equal_approx(Color(0.9, 0.55, 0.1)), "the battery stays orange")
	ok((drone.get_node("Plate").material_override as StandardMaterial3D).albedo_color.r < 0.2, "the plate is carbon")
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
