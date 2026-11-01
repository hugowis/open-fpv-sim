extends SubViewportContainer
## A turning model of the selected quad, for the main menu now and the workbench later (#128): a small viewport
## with a camera, a light and the drone; set_quad rebuilds the model at the quad's own scale.

const Drone = preload("res://drone/drone.gd")

## How fast the model turns, radians a second.
const TURNTABLE_RADPS := 0.6

var _viewport: SubViewport
var _turntable: Node3D
var _model: Node3D


func _ready() -> void:
	custom_minimum_size = Vector2(280.0, 220.0)
	mouse_filter = MOUSE_FILTER_IGNORE
	stretch = true
	_viewport = SubViewport.new()
	_viewport.transparent_bg = true
	add_child(_viewport)
	var camera := Camera3D.new()
	camera.position = Vector3(0.35, 0.25, 0.45)
	camera.near = 0.01
	camera.look_at_from_position(camera.position, Vector3(0.0, 0.03, 0.0), Vector3.UP)
	_viewport.add_child(camera)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50.0, -30.0, 0.0)
	_viewport.add_child(light)
	_turntable = Node3D.new()
	_turntable.name = "Turntable"
	_viewport.add_child(_turntable)
	set_quad("")


## Points the preview at a quad file ("" or unreadable: the authored 5F).
func set_quad(path: String) -> void:
	if _model != null:
		_model.queue_free()
	_model = Drone.new()
	_model.name = "Model"
	_turntable.add_child(_model)
	_model.set_quad(path)


## The model shown, for tests.
func model() -> Node3D:
	return _model


func _process(delta: float) -> void:
	_turntable.rotation.y += TURNTABLE_RADPS * delta
