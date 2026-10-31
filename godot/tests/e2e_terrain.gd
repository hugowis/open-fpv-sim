extends SceneTree
## End to end: the terrain mesh world.gd draws is the surface the simulator samples. A real ofs-sim (open loop) loads
## worlds/hills.toml; the mesh is read back from the built Terrain node and compared with OfsClient.terrain_height
## (the sim's own Terrain::height_up) at hundreds of points. Then worlds/park.toml: its gates must stand above the ground.
## Needs the extension and the server built (target/debug), or OFS_SIM_BIN set.
##   godot --headless --path godot -s res://tests/e2e_terrain.gd

const WorldScript = preload("res://world/world.gd")
## The mesh holds float32 heights, the sim's f64: this is well above the rounding and far below any wrong triangle.
const TOLERANCE_M := 1e-4

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


func _free_port() -> int:
	var server := TCPServer.new()
	server.listen(0, "127.0.0.1")
	var port := server.get_local_port()
	server.stop()
	return port


## Starts the game scene against `world_file` (under the repo's worlds/) and returns it once the world has arrived.
func _start(world_file: String) -> Node:
	var repo := ProjectSettings.globalize_path("res://").path_join("..").simplify_path()
	OS.set_environment("OFS_OPEN_LOOP", "1")
	OS.set_environment("OFS_MENU", "off")
	OS.set_environment("OFS_SERVER_ADDR", "127.0.0.1:%d" % _free_port())
	OS.set_environment("OFS_DATA_DIR", OS.get_user_data_dir().path_join("e2e-terrain-data"))
	# The saved user://ofs_client.cfg is shared by every checkout and can name another one's files: pin them.
	OS.set_environment("OFS_QUAD", repo.path_join("quads").path_join("opendrone-5f-freestyle.toml"))
	OS.set_environment("OFS_WORLD", repo.path_join("worlds").path_join(world_file))
	var app = load("res://main.tscn").instantiate()
	root.add_child(app)
	await process_frame  # the scene's _ready runs on the first frame
	var flying: bool = await _wait_for(func(): return app.client.get_phase() == "flying" and app.client.get_world().has("name"), 30.0)
	_check(flying, "%s: the session reaches the flying phase with its world (phase: %s %s)" % [world_file, app.client.get_phase(), app.client.get_phase_detail()])
	return app


func _stop(app: Node) -> void:
	app.queue_free()
	await process_frame
	await process_frame


## The mesh's height at (x, z) from the surface arrays: the cell from the grid's layout, then the barycentric
## interpolation of whichever of its two triangles contains the point (on the diagonal both do, and agree).
func mesh_height(arrays: Array, t: Dictionary, x: float, z: float) -> float:
	var vertices: PackedVector3Array = arrays[Mesh.ARRAY_VERTEX]
	var indices: PackedInt32Array = arrays[Mesh.ARRAY_INDEX]
	var cell: float = t["cell_m"]
	var nodes_n: int = t["nodes_n"]
	var nodes_e: int = t["nodes_e"]
	var j := clampi(int(floorf((x - float(t["origin_x"])) / cell)), 0, nodes_e - 2)
	var i := clampi(int(floorf((float(t["origin_z"]) - z) / cell)), 0, nodes_n - 2)
	var first := 6 * (i * (nodes_e - 1) + j)
	var best := -INF
	var height := NAN
	for tri in 2:
		var a := vertices[indices[first + 3 * tri]]
		var b := vertices[indices[first + 3 * tri + 1]]
		var c := vertices[indices[first + 3 * tri + 2]]
		var denominator := (b.z - c.z) * (a.x - c.x) + (c.x - b.x) * (a.z - c.z)
		var wa := ((b.z - c.z) * (x - c.x) + (c.x - b.x) * (z - c.z)) / denominator
		var wb := ((c.z - a.z) * (x - c.x) + (a.x - c.x) * (z - c.z)) / denominator
		var wc := 1.0 - wa - wb
		var inside := minf(wa, minf(wb, wc))  # the triangle the point is most inside of (>= 0 when it is in)
		if inside > best:
			best = inside
			height = wa * a.y + wb * b.y + wc * c.y
	return height


## A deterministic set of points over the grid: for a stride of cells their corners, centre and points along the
## diagonal (exactly on it and 1e-3 off it either side), plus the grid's edge nodes.
func sample_points(t: Dictionary) -> PackedVector2Array:
	var points := PackedVector2Array()
	var cell: float = t["cell_m"]
	var x0: float = t["origin_x"]
	var z0: float = t["origin_z"]
	var nodes_n: int = t["nodes_n"]
	var nodes_e: int = t["nodes_e"]
	var stride := maxi(1, int(ceil(float(nodes_n - 1) / 17.0)))
	for i in range(0, nodes_n - 1, stride):
		for j in range(0, nodes_e - 1, stride + 1):
			var cx := x0 + j * cell
			var cz := z0 - i * cell
			points.append(Vector2(cx, cz))  # the corner
			points.append(Vector2(cx + cell * 0.5, cz - cell * 0.5))  # the centre (on the diagonal)
			for f in [0.13, 0.37, 0.71]:  # along the 00-11 diagonal, and just off it
				points.append(Vector2(cx + cell * f, cz - cell * f))
				points.append(Vector2(cx + cell * f + 1e-3, cz - cell * f))
				points.append(Vector2(cx + cell * f, cz - cell * f - 1e-3))
			points.append(Vector2(cx + cell * 0.8, cz - cell * 0.2))  # off the diagonal, in the lower triangle
			points.append(Vector2(cx + cell * 0.2, cz - cell * 0.8))  # and the upper
	for i in nodes_n:  # the east and west edges, node by node
		points.append(Vector2(x0, z0 - i * cell))
		points.append(Vector2(x0 + (nodes_e - 1) * cell, z0 - i * cell))
	for j in nodes_e:  # the north and south edges
		points.append(Vector2(x0 + j * cell, z0))
		points.append(Vector2(x0 + j * cell, z0 - (nodes_n - 1) * cell))
	return points


func _run() -> void:
	# Hills: the mesh against the sim.
	var app = await _start("hills.toml")
	var client = app.client
	var terrain_in: bool = await _wait_for(func(): return client.get_world().has("terrain"), 10.0)
	_check(terrain_in, "the hills world carries a terrain")
	var field: Dictionary = client.get_world()
	if not terrain_in:
		await _stop(app)
		_finish()
		return
	var t: Dictionary = field["terrain"]
	_check(RegEx.create_from_string("^[0-9a-f]{16}$").search(t["digest"]) != null, "the digest is 16 hex digits: %s" % t["digest"])
	_check(t["vertices"].size() == t["nodes_n"] * t["nodes_e"] and t["indices"].size() == 6 * (t["nodes_n"] - 1) * (t["nodes_e"] - 1),
		"%d x %d nodes: %d vertices, %d indices" % [t["nodes_n"], t["nodes_e"], t["vertices"].size(), t["indices"].size()])

	var started := Time.get_ticks_usec()
	var drawn := WorldScript.new()
	root.add_child(drawn)  # _ready adds the sky and the ground
	var after_ready := Time.get_ticks_usec()
	drawn.build(field)
	var build_ms := (Time.get_ticks_usec() - after_ready) / 1000.0
	print("  note world.build() on hills took %.1f ms (%d vertices); the node's _ready %.1f ms" % [build_ms, t["vertices"].size(), (after_ready - started) / 1000.0])
	var instance: MeshInstance3D = drawn.get_node_or_null("Terrain")
	_check(instance != null and instance.mesh.get_surface_count() == 2, "world.build makes a Terrain mesh with the grid and the ring")
	_check(not drawn.get_node("Ground").visible, "and the flat ground plane is hidden")
	var arrays: Array = instance.mesh.surface_get_arrays(0)

	var points := sample_points(t)
	var worst := 0.0
	var worst_at := Vector2.ZERO
	var bad := 0
	for p in points:
		var error := absf(mesh_height(arrays, t, p.x, p.y) - client.terrain_height(p.x, p.y))
		if not (error < TOLERANCE_M):
			bad += 1
		if not (error <= worst):  # also catches NaN
			worst = error
			worst_at = p
	_check(points.size() >= 300, "%d sample points" % points.size())
	_check(bad == 0, "the mesh matches terrain_height within %s m at every point (%d off, worst %s m at %s)" % [TOLERANCE_M, bad, String.num_scientific(worst), worst_at])
	var low := INF
	var high := -INF
	for v in t["vertices"]:
		low = minf(low, v.y)
		high = maxf(high, v.y)
	var span := high - low
	_check(span > 1.0, "the hills are not flat (heights span %.1f m)" % span)

	var far := Vector2(float(t["origin_x"]) - 40.0, float(t["origin_z"]) + 40.0)
	_check(client.terrain_height(far.x, far.y) == 0.0, "outside the grid the sim's ground is the plane 0: %s" % client.terrain_height(far.x, far.y))
	var pilot: Vector3 = field["pilot_position"]
	_check(pilot.y >= client.terrain_height(pilot.x, pilot.z), "the pilot is not below the ground: %.2f vs %.2f" % [pilot.y, client.terrain_height(pilot.x, pilot.z)])
	drawn.queue_free()
	await _stop(app)

	# Park: every gate hangs above its ground. Task 5 gives the park a terrain; until then the ground is the plane 0.
	app = await _start("park.toml")
	client = app.client
	field = client.get_world()
	print("  note park %s" % ("has a terrain" if field.has("terrain") else "has no terrain yet: only the flat-ground checks apply"))
	_check(field.get("gates", []).size() > 0, "the park has gates")
	for g in field.get("gates", []):
		var c: Vector3 = g["center"]
		var above: float = c.y - client.terrain_height(c.x, c.z)
		_check(above >= 0.5 and above <= 2.5, "gate %s stands %.2f m above the ground" % [g["name"], above])
	await _stop(app)
	_finish()


func _finish() -> void:
	print("E2E %s" % ("PASSED" if failures == 0 else "FAILED (%d)" % failures))
	quit(1 if failures > 0 else 0)
