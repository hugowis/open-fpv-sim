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


func test_a_course_draws_its_own_gates_in_its_own_order() -> void:
	var world := World.new()
	await add_to_tree(world)
	var gate := func(n: String, z: float) -> Dictionary:
		return {"name": n, "center": Vector3(0.0, 1.0, z), "heading_deg": 0.0, "width": 3.0, "height": 2.0}
	world.build({"gates": [gate.call("A", -10.0), gate.call("B", -20.0), gate.call("C", -30.0)], "default_course": 0, "courses": [
		{"name": "Line", "gates": ["A", "B", "C"]},
		{"name": "Out", "gates": ["C", "A"]},
	]})
	eq(world.built_names(), PackedStringArray(["Gate_A", "Gate_B", "Gate_C"]), "the default course: all three")
	world.set_course(1)
	eq(world.built_names(), PackedStringArray(["Gate_C", "Gate_A"]), "the second course: its two gates in its order")
	eq((world.get_node("Gate_C/Label") as Label3D).text, "START", "its first gate is the start, whatever the file says")
	eq((world.get_node("Gate_A/Label") as Label3D).text, "1", "and the next is gate 1")
	world.set_next_gate(1)
	ok(((world.get_node("Gate_A/Opening") as MeshInstance3D).material_override as StandardMaterial3D).albedo_color.is_equal_approx(World.gate_color(1, 1)), "the highlight follows the course position")
	world.set_course(0)
	eq(world.built_names().size(), 3, "back to the first")
	world.build({"gates": [gate.call("A", -10.0), gate.call("B", -20.0)]})
	world.set_course(0)
	eq(world.built_names(), PackedStringArray(["Gate_A", "Gate_B"]), "a world without courses draws all its gates, in file order")
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


func test_the_propellers_follow_the_motor_speeds() -> void:
	var drone := Drone.new()
	await add_to_tree(drone)
	eq(drone._props.size(), 4, "four propellers")
	var before: Array = drone._props.map(func(p: Node3D) -> float: return p.rotation.y)
	drone.set_motors(PackedFloat32Array([0.0, 6000.0, 12000.0, 6000.0]), 0.01)
	var after: Array = drone._props.map(func(p: Node3D) -> float: return p.rotation.y)
	near(after[0], before[0], "a stopped motor does not turn its propeller")
	ok(absf(after[2] - before[2]) > absf(after[1] - before[1]), "a faster motor turns its propeller faster")
	# Betaflight's default directions: M1 and M4 clockwise (negative about +Y), M2 and M3 counter-clockwise.
	ok(after[1] > before[1] and after[2] > before[2], "M2 and M3 turn counter-clockwise")
	drone.set_motors(PackedFloat32Array([12000.0, 0.0, 0.0, 12000.0]), 0.01)
	ok(drone._props[0].rotation.y < after[0] and drone._props[3].rotation.y < after[3], "M1 and M4 turn clockwise")
	drone.set_motors(PackedFloat32Array([1000.0]), 0.01)  # fewer speeds than propellers: no error
	drone.queue_free()


func test_a_spinning_propeller_blurs_into_its_disc() -> void:
	var drone := Drone.new()
	await add_to_tree(drone)
	var blades := drone._props.map(func(p: Node3D) -> Node3D: return p.find_children("Blade*", "", true, false)[0])
	drone.set_motors(PackedFloat32Array([1000.0, 0.0, 30000.0, 0.0]), 0.01)
	ok(blades[0].visible, "a slow propeller shows its blade")
	ok(blades[1].visible, "a stopped motor's blade stays visible")
	ok(not blades[2].visible, "a fast one hides it: the blur disc stands in")
	drone.queue_free()


func test_the_drone_is_a_gltf_model_the_fpv_camera_never_sees() -> void:
	var drone := Drone.new()
	var camera := FpvCamera.new()
	camera.name = "FpvCamera"
	camera.uptilt_deg = 15.0
	drone.add_child(camera)
	await add_to_tree(drone)
	for part_name in ["Frame", "Plate", "Arm1", "Battery", "BatteryStrap", "Antenna1", "Antenna2", "CameraMount", "Lens", "Blade"]:
		ok(not drone.find_children(part_name, "", true, false).is_empty(), "the asset has a %s" % part_name)
	for i in [1, 2, 3, 4]:
		ok(not drone.find_children("Motor%d" % i, "", true, false).is_empty(), "motor bell %d" % i)
	eq(drone._props.size(), 4, "four propeller pivots")
	var pivot: Vector3 = drone._props[0].position
	ok(absf(pivot.x - 0.08) < 1e-4 and absf(pivot.y - 0.028) < 1e-4 and absf(pivot.z - 0.08) < 1e-4,
		"M1's pivot at the authored rear-right motor: %s" % str(pivot))
	eq(drone.find_children("Disc", "MeshInstance3D", true, false).size(), 4, "a blur disc under every propeller")
	var mounts := drone.find_children("CameraMount", "MeshInstance3D", true, false)
	eq(mounts.size(), 1, "one camera mount")
	eq(mounts[0].position, Vector3(0.0, 0.02, -0.04), "the mount sits at the FPV camera's eye point")
	var corner: float = ((mounts[0] as MeshInstance3D).mesh.get_aabb().size * mounts[0].scale * 0.5).length()
	ok(corner < 0.02, "the mount hides inside the near clip (corner %f)" % corner)
	near(mounts[0].rotation_degrees.x, 15.0, "the mount follows the camera's up-tilt, not the authored 30")
	drone.queue_free()


func test_a_model_that_does_not_load_leaves_a_flying_but_bare_drone() -> void:
	var drone := Drone.new()
	drone.model_path = "res://drone/there-is-no-such-model.gltf"
	await add_to_tree(drone)
	eq(drone._props.size(), 0, "no propeller pivots")
	ok(drone.get_node_or_null("Model") != null, "the fallback node stands in")
	drone.set_motors(PackedFloat32Array([0.0, 0.5, 1.0, 0.5]), 0.01)  # no crash
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


## A one-cell terrain like the one `OfsClient.get_world()` returns: 2 x 2 nodes 2 m apart, node (0, 0) at x 10, z -20
## (vertex i * nodes_e + j, north the slow index so z falls with i), the cell's two triangles as terrain_mesh lays them.
func _terrain_world() -> Dictionary:
	return {"terrain": {
		"origin_x": 10.0, "origin_z": -20.0, "cell_m": 2.0, "nodes_n": 2, "nodes_e": 2,
		"vertices": PackedVector3Array([Vector3(10, 0, -20), Vector3(12, 0.5, -20), Vector3(10, 0, -22), Vector3(12, 1, -22)]),
		"normals": PackedVector3Array([Vector3.UP, Vector3.UP, Vector3.UP, Vector3.UP]),
		"indices": PackedInt32Array([0, 2, 3, 0, 3, 1]),
		"digest": "0123456789abcdef",
	}}


## Every triangle of `indices` over `vertices` faces up under Godot's clockwise-front rule.
func _all_face_up(vertices: PackedVector3Array, indices: PackedInt32Array) -> bool:
	for t in indices.size() / 3:
		if Plane(vertices[indices[3 * t]], vertices[indices[3 * t + 1]], vertices[indices[3 * t + 2]]).normal.y <= 0.0:
			return false
	return true


func test_a_terrain_world_draws_a_mesh_and_hides_the_flat_ground() -> void:
	var world := World.new()
	await add_to_tree(world)
	var dict := _terrain_world()
	world.build(dict)
	ok(world.built_names().has("Terrain"), "the terrain is one of the built nodes")
	var terrain: MeshInstance3D = world.get_node("Terrain")
	ok(terrain.material_override is ShaderMaterial, "drawn by the grid shader")
	var mesh := terrain.mesh as ArrayMesh
	eq(mesh.get_surface_count(), 2, "the grid and the ring")
	ok(not (world.get_node("Ground") as MeshInstance3D).visible, "the flat ground is hidden")
	var grid := mesh.surface_get_arrays(0)
	var t: Dictionary = dict["terrain"]
	eq(grid[Mesh.ARRAY_VERTEX], t["vertices"], "surface 0 holds the dict's vertices")
	eq(grid[Mesh.ARRAY_INDEX], t["indices"], "and its indices")
	ok(_all_face_up(grid[Mesh.ARRAY_VERTEX], grid[Mesh.ARRAY_INDEX]), "every grid triangle is front-facing from above")
	var ring := mesh.surface_get_arrays(1)
	var rv: PackedVector3Array = ring[Mesh.ARRAY_VERTEX]
	eq(rv.size(), 8, "the ring has an outer and an inner corner each")
	eq((ring[Mesh.ARRAY_INDEX] as PackedInt32Array).size(), 24, "eight triangles")
	ok(_all_face_up(rv, ring[Mesh.ARRAY_INDEX]), "every ring triangle is front-facing from above")
	# The grid's rectangle: x from origin_x east over one cell, z from origin_z (the south edge) one cell north.
	var inner := [rv[4], rv[5], rv[6], rv[7]]
	eq(inner, [Vector3(10, 0, -22), Vector3(12, 0, -22), Vector3(12, 0, -20), Vector3(10, 0, -20)], "the ring's inner corners are the grid's")
	eq([rv[0].x, rv[1].x, rv[0].z, rv[2].z], [-2990.0, 3012.0, -3022.0, 2980.0], "and the outer ones lie 3000 m beyond")
	world.queue_free()


func test_a_flat_world_after_a_hilly_one_shows_the_ground_again() -> void:
	var world := World.new()
	await add_to_tree(world)
	world.build(_terrain_world())
	world.build({})
	ok(world.get_node_or_null("Terrain") == null, "the terrain is removed at once")
	ok((world.get_node("Ground") as MeshInstance3D).visible, "the flat ground is back")
	world.queue_free()


func test_the_ring_arrays_wind_upward_around_any_rectangle() -> void:
	var a: Array = World.ring_arrays(-5.0, 7.0, -40.0, -10.0, 100.0)
	var v: PackedVector3Array = a[Mesh.ARRAY_VERTEX]
	ok(_all_face_up(v, a[Mesh.ARRAY_INDEX]), "front-facing")
	for p in v:
		eq(p.y, 0.0, "at the sea-level of the plane")
	eq(v[4], Vector3(-5, 0, -40), "the inner rectangle starts at its north-west corner")
	eq(v[0], Vector3(-105, 0, -140), "and the outer one is `extent` beyond")


func test_an_emitter_post_stands_on_the_ground_below_it() -> void:
	var world := World.new()
	await add_to_tree(world)
	world.build({"emitters": [{"name": "hill", "position": Vector3(5.0, 4.0, -6.0), "freq_mhz": 5800.0, "power_mw": 25.0, "ground_y": 1.5}]})
	var marker: Node3D = world.get_node("Emitter_hill")
	eq(marker.position, Vector3(5.0, 1.5, -6.0), "the post starts on the ground")
	eq(((marker.get_node("Post") as MeshInstance3D).mesh as CylinderMesh).height, 2.5, "and rises to the emitter")
	world.queue_free()


## Writes a GLB of flat nodes (each {name, tris: PackedVector3Array of 3 points per triangle}) and returns its absolute path.
func _write_glb(file_name: String, nodes: Array) -> String:
	var bin := PackedFloat32Array()
	var views := []
	var accessors := []
	var meshes := []
	var json_nodes := []
	for n in nodes:
		var tris: PackedVector3Array = n["tris"]
		var offset := bin.size() * 4
		for v in tris:
			bin.append_array([v.x, v.y, v.z])
		views.append({"buffer": 0, "byteOffset": offset, "byteLength": tris.size() * 12})
		accessors.append({"bufferView": views.size() - 1, "componentType": 5126, "count": tris.size(), "type": "VEC3",
			"min": [-100, -100, -100], "max": [100, 100, 100]})
		meshes.append({"primitives": [{"attributes": {"POSITION": accessors.size() - 1}}]})
		json_nodes.append({"name": n["name"], "mesh": meshes.size() - 1})
	var ids := []
	for i in nodes.size():
		ids.append(i)
	var json := JSON.stringify({"asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": ids}], "nodes": json_nodes,
		"meshes": meshes, "accessors": accessors, "bufferViews": views, "buffers": [{"byteLength": bin.size() * 4}]}).to_utf8_buffer()
	while json.size() % 4 != 0:
		json.append(32)
	var data := bin.to_byte_array()
	var out := StreamPeerBuffer.new()
	out.put_u32(0x46546C67)
	out.put_u32(2)
	out.put_u32(12 + 8 + json.size() + 8 + data.size())
	out.put_u32(json.size())
	out.put_u32(0x4E4F534A)
	out.put_data(json)
	out.put_u32(data.size())
	out.put_u32(0x004E4942)
	out.put_data(data)
	var path := "user://%s" % file_name
	var file := FileAccess.open(path, FileAccess.WRITE)
	file.store_buffer(out.data_array)
	file.close()
	return ProjectSettings.globalize_path(path)


## A world with one mesh the way `get_world()` describes it: the digest check passed.
func _mesh_world(path: String, heading_deg := 0.0, digest_ok := true) -> Dictionary:
	return {"meshes": [{"name": "hall", "path": path, "position": Vector3(1.0, 2.0, 3.0), "heading_deg": heading_deg,
		"digest_ok": digest_ok, "error": "" if digest_ok else "digest mismatch"}]}


## The world-space position of the first vertex of the first mesh instance under `node`.
func _first_vertex(node: Node) -> Vector3:
	var instance: MeshInstance3D = node.find_children("*", "MeshInstance3D", true, false)[0]
	return instance.global_transform * instance.mesh.get_faces()[0]


func test_mesh_is_built_and_named() -> void:
	var world := World.new()
	await add_to_tree(world)
	var path := ProjectSettings.globalize_path("res://../worlds/assets/hall.glb")
	ok(FileAccess.file_exists(path), "the demo hall resolves: %s" % path)
	world.build(_mesh_world(path))
	eq(world.built_names(), PackedStringArray(["hall"]), "one node named after the mesh")
	var hall: Node3D = world.get_node("hall")
	eq(hall.position, Vector3(1.0, 2.0, 3.0), "where the world puts it")
	ok(hall.find_children("*", "MeshInstance3D", true, false).size() > 0, "with the file's meshes")
	world.queue_free()


func test_col_nodes_are_not_drawn() -> void:
	var world := World.new()
	await add_to_tree(world)
	var tri := PackedVector3Array([Vector3(0, 0, 0), Vector3(0, 1, 0), Vector3(1, 0, 0)])
	var path := _write_glb("test_col.glb", [{"name": "wall", "tris": tri}, {"name": "col_wall", "tris": tri}])
	world.build(_mesh_world(path))
	var drawn := world.get_node("hall")
	ok(drawn.find_child("wall", true, false) != null, "the visible node is drawn")
	ok(drawn.find_child("col_*", true, false) == null, "the collision-only node is not")
	world.queue_free()


func test_heading_90_turns_the_mesh_to_face_east() -> void:
	var world := World.new()
	await add_to_tree(world)
	var tri := PackedVector3Array([Vector3(0, 0, -10), Vector3(0, 1, -10), Vector3(1, 0, -10)])
	var path := _write_glb("test_heading.glb", [{"name": "wall", "tris": tri}])
	var at := Vector3(1.0, 2.0, 3.0)
	world.build(_mesh_world(path, 0.0))
	ok((_first_vertex(world.get_node("hall")) - (at + Vector3(0, 0, -10))).length() < 1e-4, "heading 0: a vertex 10 m north stays north")
	world.build(_mesh_world(path, 90.0))
	ok((_first_vertex(world.get_node("hall")) - (at + Vector3(10, 0, 0))).length() < 1e-4, "heading 90: it lands 10 m east")
	world.queue_free()


func test_a_mesh_with_a_bad_digest_draws_nothing() -> void:
	var world := World.new()
	await add_to_tree(world)
	var path := ProjectSettings.globalize_path("res://../worlds/assets/hall.glb")
	world.build(_mesh_world(path, 0.0, false))
	eq(world.built_names(), PackedStringArray(), "a file that is not the one the sim collides with is not drawn")
	world.build(_mesh_world("/no/such/file.glb"))
	eq(world.built_names(), PackedStringArray(), "nor is one that fails to load")
	world.queue_free()


func test_a_world_without_meshes_builds_exactly_as_before() -> void:
	var world := World.new()
	await add_to_tree(world)
	world.build(_world_dict())
	var before := world.built_names()
	var with_empty := _world_dict()
	with_empty["meshes"] = []
	world.build(with_empty)
	eq(world.built_names(), before, "an empty meshes array changes nothing")
	world.queue_free()
