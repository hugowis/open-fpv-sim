extends Camera3D
## The FPV camera on the drone: a wide horizontal field of view, tilted up so the horizon stays in view while the
## drone leans forward to fly fast. (Lens distortion and exposure are the `Lens` layer's job, see ui/lens.gd.)

## Horizontal field of view in degrees. A rectilinear projection gets very stretched at the edges above about 130.
@export_range(60.0, 160.0) var fov_h_deg := 110.0:
	set(value):
		fov_h_deg = value
		_apply()
## How far the camera is tilted up from the drone's forward axis, in degrees.
@export_range(0.0, 70.0) var uptilt_deg := 30.0:
	set(value):
		uptilt_deg = value
		_apply()


func _ready() -> void:
	near = 0.02
	far = 6000.0
	position = Vector3(0.0, 0.02, -0.04)  # just ahead of and above the drone's centre
	_apply()


func _apply() -> void:
	keep_aspect = Camera3D.KEEP_WIDTH  # `fov` is then the horizontal field of view
	fov = clampf(fov_h_deg, 1.0, 179.0)
	# Positive rotation about the camera's X axis pitches its view (-Z) up.
	rotation_degrees = Vector3(uptilt_deg, 0.0, 0.0)