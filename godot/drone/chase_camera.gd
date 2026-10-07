extends Camera3D
## A third-person camera behind the drone, for finding out what the drone is doing when the FPV view is confusing.

@export var target_path: NodePath = ^"../Drone"
@export var distance := 1.2
@export var height := 0.45
@export var smoothing := 8.0


func _ready() -> void:
	near = 0.05
	far = 6000.0
	var target := get_node_or_null(target_path) as Node3D
	if target != null:
		global_position = _desired(target)
		_aim(target)


func _process(delta: float) -> void:
	var target := get_node_or_null(target_path) as Node3D
	if target == null:
		return
	global_position = global_position.lerp(_desired(target), 1.0 - exp(-smoothing * delta))
	_aim(target)


## Behind the drone along its heading (ignoring pitch and roll), a little above.
func _desired(target: Node3D) -> Vector3:
	var forward := -target.global_transform.basis.z
	forward.y = 0.0
	forward = Vector3(0, 0, -1) if forward.length() < 1e-3 else forward.normalized()
	return target.global_position - forward * distance + Vector3.UP * height


func _aim(target: Node3D) -> void:
	var look_at_point := target.global_position + Vector3.UP * 0.05
	if not global_position.is_equal_approx(look_at_point):
		look_at(look_at_point, Vector3.UP)
