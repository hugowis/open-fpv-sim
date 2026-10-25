extends RefCounted
## The maths that corrects one physical axis, in the order the client applies it: calibrate -> invert -> deadband ->
## travel (docs/superpowers/specs/2026-10-26-controller-setup-design.md, R1). Rates and expo stay Betaflight's.

const MIN_TRAVEL := 0.3
## A calibrated side narrower than this is a stick that was never moved to that end: it is not trusted.
const MIN_SPAN := 0.05


## A raw axis value in the controller's own range, mapped to [-1, 1] with the calibration `cal` ({min, center, max}).
## `centred` scales each side of the centre on its own (sticks that spring back); otherwise min..max is the whole
## range (throttle, switches). An empty or useless calibration returns the raw value.
static func calibrate(raw: float, cal: Dictionary, centred: bool) -> float:
	if cal.is_empty():
		return raw
	var low := float(cal["min"])
	var mid := float(cal["center"])
	var high := float(cal["max"])
	if centred:
		var span := (high - mid) if raw >= mid else (mid - low)
		if span < MIN_SPAN:
			return raw
		return clampf((raw - mid) / span, -1.0, 1.0)
	if high - low < MIN_SPAN:
		return raw
	return clampf((raw - low) / (high - low) * 2.0 - 1.0, -1.0, 1.0)


## Deadband (rescaling what is left) then travel (full output at the fraction `travel` of the throw).
static func shape(v: float, deadband: float, travel: float) -> float:
	if absf(v) <= deadband:
		return 0.0
	v = signf(v) * (absf(v) - deadband) / (1.0 - deadband)
	return clampf(v / travel, -1.0, 1.0)


static func clean_travel(travel: float) -> float:
	return clampf(travel, MIN_TRAVEL, 1.0)
