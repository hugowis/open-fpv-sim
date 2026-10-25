extends "res://tests/testing.gd"

const Calibration = preload("res://scripts/calibration.gd")


## Feeds `seconds` of the same sample, in 0.1 s frames.
func _hold(c, samples: Dictionary, seconds: float) -> void:
	for i in int(round(seconds * 10.0)):
		c.feed(samples, 0.1)


func test_rest_then_sweep_gives_centre_and_end_points() -> void:
	var c := Calibration.new()
	eq(c.step, "rest", "starts at rest")
	_hold(c, {0: 0.05, 1: -0.02, 2: -0.9}, 1.2)
	eq(c.step, "sweep", "a second of stillness moves on")
	for v in [0.9, -0.7, 0.2]:
		c.feed({0: v, 1: v * 0.5, 2: -0.9 + (v + 0.7)}, 0.1)
	var r: Dictionary = c.result()
	near(r["0"]["min"], -0.7, "min")
	near(r["0"]["max"], 0.9, "max")
	near(r["0"]["center"], 0.05, "centre from the rest step")
	near(r["2"]["center"], -0.9, "a throttle at the bottom rests at its minimum")
	ok(r["2"]["min"] <= r["2"]["center"] and r["2"]["center"] <= r["2"]["max"], "min <= centre <= max")


func test_an_axis_that_was_not_moved_is_not_calibrated() -> void:
	var c := Calibration.new()
	_hold(c, {0: 0.0, 1: 0.0}, 1.2)
	c.feed({0: 1.0, 1: 0.1}, 0.1)
	c.feed({0: -1.0, 1: -0.1}, 0.1)
	eq(c.result().keys(), ["0"], "axis 1 swung 0.2 only")


func test_moving_during_rest_restarts_it() -> void:
	var c := Calibration.new()
	_hold(c, {0: 0.0}, 0.7)
	c.feed({0: 0.5}, 0.1)
	eq(c.step, "rest", "still resting")
	_hold(c, {0: 0.0}, 0.8)
	eq(c.step, "rest", "the clock restarted at the movement")
	_hold(c, {0: 0.0}, 0.4)
	eq(c.step, "sweep", "a full still second later")
	near(c.result_centre(0), 0.0, "the centre ignores the movement")


func test_nothing_to_report_before_the_sweep_and_junk_samples_are_skipped() -> void:
	var c := Calibration.new()
	eq(c.result(), {}, "no result at rest")
	_hold(c, {0: 0.0, 1: NAN}, 1.2)
	c.feed({0: 1.0, 1: INF}, 0.1)
	c.feed({0: -1.0, 1: -INF}, 0.1)
	eq(c.result().keys(), ["0"], "a non-finite axis is left out")


func test_merge_keeps_what_was_not_recalibrated() -> void:
	var table := {"g": {"name": "Pad", "axes": {"0": {"min": -1.0, "center": 0.0, "max": 1.0}, "5": {"min": 0.0, "center": 0.5, "max": 1.0}}}}
	Calibration.merge(table, "g", "Pad 2", {"0": {"min": -0.8, "center": 0.1, "max": 0.9}})
	near(table["g"]["axes"]["0"]["max"], 0.9, "recalibrated axis replaced")
	near(table["g"]["axes"]["5"]["max"], 1.0, "other axis kept")
	eq(table["g"]["name"], "Pad 2", "name refreshed")
	Calibration.merge(table, "h", "Other", {"1": {"min": -1.0, "center": 0.0, "max": 1.0}})
	eq(table.keys(), ["g", "h"], "a new controller is added")
	Calibration.merge(table, "h", "Other", {})
	eq(table["h"]["axes"].size(), 1, "an empty result changes nothing")
