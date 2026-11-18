extends "res://drone/drone.gd"
## A translucent second quad that replays the best lap on the race's lap clock (#133).
##
## The app gives it the stored track (`Records.ghost_of`), shows it only while racing and hands it the drawn lap
## time. `pose_at` interpolates the track (position lerp, attitude slerp); `delta` is the pilot's drawn lap time
## minus the ghost's time at the stored pose nearest the pilot.

const WINDOW_S := 2.0

var _times := PackedFloat32Array()
var _positions := PackedFloat32Array()
var _rotations := PackedFloat32Array()
var _cursor := 0
var _pose_cursor := 0
var _step_s := 1.0 / 60.0


func _ready() -> void:
	super._ready()
	_make_translucent()
	visible = false


func _process(delta: float) -> void:
	# The ghost's props idle at a fixed speed; the pilot's motors are the drone's business.
	super.set_motors(PackedFloat32Array([900.0, 900.0, 900.0, 900.0]), delta)


## `{times, positions (xyz triplets), rotations (xyzw quadruplets)}`; `{}` clears. The app passes the stored
## track (plain arrays from JSON) or the bridge's packed arrays.
func setup(track: Dictionary) -> void:
	_times = _packed(track.get("times", []))
	_positions = _packed(track.get("positions", []))
	_rotations = _packed(track.get("rotations", []))
	_cursor = 0
	_pose_cursor = 0
	_step_s = 1.0 / 60.0
	if _times.size() < 2 or _positions.size() != _times.size() * 3 or _rotations.size() != _times.size() * 4:
		_times = PackedFloat32Array()
		_positions = PackedFloat32Array()
		_rotations = PackedFloat32Array()
		return
	var span := maxf(_times[_times.size() - 1] - _times[0], 0.001)
	_step_s = span / float(_times.size() - 1)


func has_track() -> bool:
	return _times.size() >= 2


## The track's pose at a lap time: interpolated between samples, the first/last pose held outside the track.
func pose_at(lap_s: float) -> Transform3D:
	if not has_track():
		return Transform3D.IDENTITY
	var t := clampf(lap_s, _times[0], _times[_times.size() - 1])
	if _pose_cursor >= _times.size() - 1 or t < _times[_pose_cursor]:
		_pose_cursor = 0
	while _pose_cursor + 1 < _times.size() and _times[_pose_cursor + 1] < t:
		_pose_cursor += 1
	var i := _pose_cursor
	var j := mini(i + 1, _times.size() - 1)
	var span := _times[j] - _times[i]
	var f := 0.0 if span <= 0.0 else (t - _times[i]) / span
	var pos := _position(i).lerp(_position(j), f)
	var att := _rotation(i).slerp(_rotation(j), f)
	return Transform3D(Basis(att), pos)


## The pilot's drawn lap time minus the ghost's time at the nearest stored pose; NAN without a track.
func delta(pilot: Vector3, lap_s: float) -> float:
	if not has_track():
		return NAN
	var span := int(ceil(WINDOW_S / _step_s))
	var lo := maxi(_cursor - span, 0)
	var hi := mini(_cursor + span, _times.size() - 1)
	var best := _nearest(lo, hi, pilot)
	# A parked cursor (a crash or a cut far along the line) must not hide the true nearest point: widen once.
	if best == lo or best == hi:
		best = _nearest(0, _times.size() - 1, pilot)
	_cursor = best
	return lap_s - _times[best]


func _nearest(lo: int, hi: int, pilot: Vector3) -> int:
	var best := lo
	var best_distance := INF
	for i in range(lo, hi + 1):
		var distance := _position(i).distance_squared_to(pilot)
		if distance < best_distance:
			best_distance = distance
			best = i
	return best


func _position(i: int) -> Vector3:
	return Vector3(_positions[i * 3], _positions[i * 3 + 1], _positions[i * 3 + 2])


func _rotation(i: int) -> Quaternion:
	return Quaternion(_rotations[i * 4], _rotations[i * 4 + 1], _rotations[i * 4 + 2], _rotations[i * 4 + 3])


func _packed(value) -> PackedFloat32Array:
	return value if value is PackedFloat32Array else PackedFloat32Array(value)


func _make_translucent() -> void:
	var material := StandardMaterial3D.new()
	material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	material.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	material.albedo_color = Color(1.0, 1.0, 1.0, 0.35)
	for mesh in find_children("*", "MeshInstance3D", true, false):
		(mesh as MeshInstance3D).material_override = material
