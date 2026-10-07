extends Node3D
## A grey-box world: sky, sun, a ground plane with a metre grid, a launch pad, pylons, gates and a few buildings.
## It is only drawn. The simulator knows the ground (a plane at height 0) and nothing else, so nothing here
## collides with the drone.
##
## Godot's frame: +Y up, forward (north, where the drone starts facing) is -Z, +X is right (east).

const GRID_SHADER := preload("res://world/grid.gdshader")

const ORANGE := Color(1.0, 0.45, 0.1)
const WHITE := Color(0.92, 0.92, 0.9)
const GREEN := Color(0.2, 0.8, 0.35)
const YELLOW := Color(0.95, 0.8, 0.15)
const CONCRETE := Color(0.62, 0.64, 0.66)


func _ready() -> void:
	_add_environment()
	_add_ground()
	_add_launch_pad()
	_add_pylons()
	_add_gates()
	_add_buildings()


func _add_environment() -> void:
	var sky_material := ProceduralSkyMaterial.new()
	sky_material.sky_top_color = Color(0.30, 0.48, 0.78)
	sky_material.sky_horizon_color = Color(0.72, 0.80, 0.88)
	sky_material.ground_horizon_color = Color(0.72, 0.80, 0.88)
	sky_material.ground_bottom_color = Color(0.40, 0.45, 0.40)
	var sky := Sky.new()
	sky.sky_material = sky_material
	var environment := Environment.new()
	environment.background_mode = Environment.BG_SKY
	environment.sky = sky
	environment.ambient_light_source = Environment.AMBIENT_SOURCE_SKY
	environment.ambient_light_energy = 0.8
	var world_environment := WorldEnvironment.new()
	world_environment.environment = environment
	add_child(world_environment)
	var sun := DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	sun.light_energy = 1.1
	sun.shadow_enabled = true
	sun.directional_shadow_max_distance = 150.0
	add_child(sun)


func _add_ground() -> void:
	var plane := PlaneMesh.new()
	plane.size = Vector2(6000.0, 6000.0)
	var material := ShaderMaterial.new()
	material.shader = GRID_SHADER
	var ground := MeshInstance3D.new()
	ground.name = "Ground"
	ground.mesh = plane
	ground.material_override = material
	add_child(ground)


func _add_launch_pad() -> void:
	_box("LaunchPad", Vector3(0, 0.01, 0), Vector3(3.0, 0.02, 3.0), Color(0.78, 0.78, 0.76))


func _add_pylons() -> void:
	# Two rows of pylons along the flight direction, every 15 m.
	for i in range(1, 9):
		var z := -15.0 * i
		_cylinder("Pylon%dL" % i, Vector3(-6.0, 1.5, z), 0.15, 3.0, ORANGE if i % 2 == 0 else WHITE)
		_cylinder("Pylon%dR" % i, Vector3(6.0, 1.5, z), 0.15, 3.0, WHITE if i % 2 == 0 else ORANGE)


func _add_gates() -> void:
	var gates := [[-30.0, GREEN], [-60.0, YELLOW], [-90.0, GREEN]]
	for i in gates.size():
		var z: float = gates[i][0]
		var color: Color = gates[i][1]
		_box("Gate%dPostL" % i, Vector3(-1.6, 1.25, z), Vector3(0.12, 2.5, 0.12), color)
		_box("Gate%dPostR" % i, Vector3(1.6, 1.25, z), Vector3(0.12, 2.5, 0.12), color)
		_box("Gate%dBar" % i, Vector3(0.0, 2.5, z), Vector3(3.32, 0.12, 0.12), color)


func _add_buildings() -> void:
	_box("BuildingA", Vector3(-45.0, 7.0, -110.0), Vector3(14.0, 14.0, 12.0), CONCRETE)
	_box("BuildingB", Vector3(38.0, 11.0, -75.0), Vector3(10.0, 22.0, 10.0), CONCRETE.darkened(0.15))
	_box("BuildingC", Vector3(70.0, 5.0, -140.0), Vector3(30.0, 10.0, 18.0), CONCRETE.lightened(0.1))
	_box("BuildingD", Vector3(-80.0, 9.0, -30.0), Vector3(12.0, 18.0, 12.0), CONCRETE.darkened(0.05))


func _material(color: Color) -> StandardMaterial3D:
	var material := StandardMaterial3D.new()
	material.albedo_color = color
	material.roughness = 0.9
	return material


func _box(node_name: String, center: Vector3, size: Vector3, color: Color) -> void:
	var mesh := BoxMesh.new()
	mesh.size = size
	var instance := MeshInstance3D.new()
	instance.name = node_name
	instance.mesh = mesh
	instance.material_override = _material(color)
	instance.position = center
	add_child(instance)


func _cylinder(node_name: String, center: Vector3, radius: float, height: float, color: Color) -> void:
	var mesh := CylinderMesh.new()
	mesh.top_radius = radius
	mesh.bottom_radius = radius
	mesh.height = height
	var instance := MeshInstance3D.new()
	instance.name = node_name
	instance.mesh = mesh
	instance.material_override = _material(color)
	instance.position = center
	add_child(instance)
