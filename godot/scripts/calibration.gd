extends RefCounted
## The calibration wizard's logic, with no engine calls: feed it the controller's axes frame by frame.
## Step "rest" (hands off) takes the centre of every axis; step "sweep" (move everything to its extremes) takes min and max.
## docs/superpowers/specs/2026-10-26-controller-setup-design.md, R3.

const REST_SECONDS := 1.0
## An axis that moves more than this during the rest step restarts it.
const REST_JITTER := 0.1
## An axis that swings less than this over the sweep was not touched and is left out of the result.
const MIN_SWING := 0.5

## "rest" or "sweep".
var step := "rest"

var _rest_time := 0.0
var _rest_sum := {}  # axis -> sum of samples
var _rest_lo := {}
var _rest_hi := {}
var _rest_n := 0
var _centre := {}
var _lo := {}
var _hi := {}


## One frame: `samples` is {axis index: value}; non-finite values are ignored.
func feed(samples: Dictionary, delta: float) -> void:
	if step == "rest":
		_feed_rest(samples, delta)
	else:
		for axis in samples:
			var v: float = samples[axis]
			if is_finite(v):
				_lo[axis] = minf(_lo.get(axis, v), v)
				_hi[axis] = maxf(_hi.get(axis, v), v)


## {"<axis>": {min, center, max}} for the axes that swung enough; {} before the sweep starts.
func result() -> Dictionary:
	var out := {}
	if step != "sweep":
		return out
	for axis in _lo:
		if _hi[axis] - _lo[axis] >= MIN_SWING:
			var centre := clampf(_centre.get(axis, _lo[axis]), _lo[axis], _hi[axis])
			out[str(axis)] = {"min": _lo[axis], "center": centre, "max": _hi[axis]}
	return out


## The centre measured at rest for an axis (0 if it was not seen).
func result_centre(axis: int) -> float:
	return _centre.get(axis, 0.0)


## How far the rest step is, 0..1.
func rest_progress() -> float:
	return 1.0 if step != "rest" else clampf(_rest_time / REST_SECONDS, 0.0, 1.0)


## Adds `result` to a calibration table (`Controls.calibration`) under the controller `key`; axes not in `result` keep
## their old calibration.
static func merge(table: Dictionary, key: String, device_name: String, result: Dictionary) -> void:
	if result.is_empty():
		return
	var entry: Dictionary = table.get(key, {"axes": {}})
	entry["name"] = device_name
	entry["axes"].merge(result, true)
	table[key] = entry


func _feed_rest(samples: Dictionary, delta: float) -> void:
	for axis in samples:
		var v: float = samples[axis]
		if not is_finite(v):
			continue
		_rest_sum[axis] = _rest_sum.get(axis, 0.0) + v
		_rest_lo[axis] = minf(_rest_lo.get(axis, v), v)
		_rest_hi[axis] = maxf(_rest_hi.get(axis, v), v)
		if _rest_hi[axis] - _rest_lo[axis] > REST_JITTER:
			_restart_rest()
			return
	_rest_n += 1
	_rest_time += delta
	if _rest_time >= REST_SECONDS - 1e-9:
		for axis in _rest_sum:
			_centre[axis] = _rest_sum[axis] / _rest_n
		_lo = _centre.duplicate()
		_hi = _centre.duplicate()
		step = "sweep"


func _restart_rest() -> void:
	_rest_time = 0.0
	_rest_n = 0
	_rest_sum.clear()
	_rest_lo.clear()
	_rest_hi.clear()
