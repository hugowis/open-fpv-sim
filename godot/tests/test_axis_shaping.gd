extends "res://tests/testing.gd"

const AxisShaping = preload("res://scripts/axis_shaping.gd")

const CAL := {"min": -0.8, "center": 0.1, "max": 0.9}


func test_centred_calibration_scales_each_side_on_its_own() -> void:
	near(AxisShaping.calibrate(0.1, CAL, true), 0.0, "rest reads zero")
	near(AxisShaping.calibrate(0.9, CAL, true), 1.0, "the top end point reads one")
	near(AxisShaping.calibrate(-0.8, CAL, true), -1.0, "the bottom end point reads minus one")
	near(AxisShaping.calibrate(0.5, CAL, true), 0.5, "halfway up")
	near(AxisShaping.calibrate(-0.35, CAL, true), -0.5, "halfway down")
	near(AxisShaping.calibrate(1.0, CAL, true), 1.0, "past the end point clamps")
	near(AxisShaping.calibrate(-1.0, CAL, true), -1.0, "and below")


func test_range_calibration_maps_min_to_max_onto_minus_one_to_one() -> void:
	var cal := {"min": -0.9, "center": 0.0, "max": 0.7}
	near(AxisShaping.calibrate(-0.9, cal, false), -1.0, "bottom")
	near(AxisShaping.calibrate(0.7, cal, false), 1.0, "top")
	near(AxisShaping.calibrate(-0.1, cal, false), 0.0, "middle of the range, not of the axis")


func test_no_calibration_or_a_useless_one_reads_raw() -> void:
	near(AxisShaping.calibrate(0.3, {}, true), 0.3, "uncalibrated")
	near(AxisShaping.calibrate(0.3, {"min": 0.0, "center": 0.0, "max": 0.01}, true), 0.3, "a side narrower than 0.05")
	near(AxisShaping.calibrate(0.3, {"min": 0.2, "center": 0.2, "max": 0.22}, false), 0.3, "a range narrower than 0.05")


func test_deadband_rescales_and_travel_reaches_full_early() -> void:
	near(AxisShaping.shape(0.05, 0.1, 1.0), 0.0, "inside the deadband")
	near(AxisShaping.shape(0.55, 0.1, 1.0), 0.5, "(0.55 - 0.1) / 0.9")
	near(AxisShaping.shape(-0.55, 0.1, 1.0), -0.5, "symmetric")
	near(AxisShaping.shape(0.4, 0.0, 0.8), 0.5, "half of the 80% throw")
	near(AxisShaping.shape(0.9, 0.0, 0.8), 1.0, "past the travel clamps")
	near(AxisShaping.shape(-0.9, 0.0, 0.8), -1.0, "and below")
	near(AxisShaping.shape(1.0, 0.0, 1.0), 1.0, "defaults are the identity")


func test_travel_is_kept_inside_its_range() -> void:
	eq(AxisShaping.clean_travel(0.1), AxisShaping.MIN_TRAVEL, "not below the minimum")
	eq(AxisShaping.clean_travel(3.0), 1.0, "never above one")
	eq(AxisShaping.clean_travel(0.75), 0.75, "in range")
