extends RefCounted
## Turns joystick axes, buttons and keys into the transmitter's sticks.
##
## Every channel (roll, pitch, yaw, throttle, aux1..aux4) has one binding. A binding reads a value in [-1, 1]:
## roll/pitch/yaw/aux are used as they are, throttle becomes (value + 1) / 2. Sign conventions, as Betaflight sees
## them: roll +1 is right, pitch +1 is stick forward (nose down), yaw +1 is right, throttle 1 is full, aux +1 is the
## switch on (aux1 arms, aux2 selects Angle mode in the reference quad).
## Per-axis corrections (docs/superpowers/specs/2026-10-26-controller-setup-design.md): `calibration` maps a
## controller (its guid) to the end points and centre of each of its axes, `shaping` gives roll, pitch and yaw their own
## deadband and travel. Rates and expo stay Betaflight's.
##
## `read` takes the input source as a parameter (the `Input` singleton in the game, a fake in the tests).

const CHANNELS := ["roll", "pitch", "yaw", "throttle", "aux1", "aux2", "aux3", "aux4"]
const STICK_CHANNELS := ["roll", "pitch", "yaw"]
const PRESET_NAMES := ["radio", "gamepad", "keyboard"]
const AxisShaping = preload("res://scripts/axis_shaping.gd")
## Substrings of a controller's name (lower case) that say which preset suits it.
const GAMEPAD_NAMES := ["xbox", "playstation", "dualshock", "dualsense", "ps4", "ps5", "wireless controller", "gamepad", "switch pro", "8bitdo"]
const RADIO_NAMES := ["radiomaster", "taranis", "jumper", "tx12", "tx16", "zorro", "boxer", "frsky", "flysky", "opentx", "edgetx", "radio", "transmitter"]

## device -1 means "the first connected controller".
var bindings: Dictionary = {}
var deadzone := 0.03
## Keyboard throttle: how far the throttle stick moves per second (throttle goes 0..1).
var throttle_ramp_per_s := 0.6
var preset_name := ""
## roll/pitch/yaw -> {deadband, travel}, either key optional (deadband falls back to `deadzone`, travel to 1).
var shaping: Dictionary = {}
## Controller key (`device_key`) -> {name, axes: {"<index>": {min, center, max}}}. Belongs to the controllers, not to
## a profile: presets and profile switches leave it alone.
var calibration: Dictionary = {}

var _ramp: Dictionary = {}
var _toggle: Dictionary = {}
var _was_down: Dictionary = {}


func _init(preset: String = "radio") -> void:
	apply_preset(preset)


## The bindings of a preset: "radio" (a USB radio in joystick mode, channels in AETR order), "gamepad" (Mode 2,
## Xbox layout: left stick throttle and yaw, right stick pitch and roll) or "keyboard" (for trying things out).
static func preset(name: String) -> Dictionary:
	match name:
		"gamepad":
			return {
				"deadzone": 0.08,
				"bindings": {
					"roll": _axis(2, false), "pitch": _axis(3, true), "yaw": _axis(0, false), "throttle": _axis(1, true),
					"aux1": _button(JOY_BUTTON_A), "aux2": _button(JOY_BUTTON_B),
					"aux3": _button(JOY_BUTTON_X), "aux4": _button(JOY_BUTTON_Y),
				},
			}
		"keyboard":
			return {
				"deadzone": 0.0,
				"bindings": {
					"roll": _pair(KEY_LEFT, KEY_RIGHT, 0.6), "pitch": _pair(KEY_DOWN, KEY_UP, 0.6),
					"yaw": _pair(KEY_Q, KEY_E, 0.8), "throttle": {"kind": "key_ramp", "down": KEY_S, "up": KEY_W},
					"aux1": {"kind": "key_toggle", "key": KEY_SPACE}, "aux2": {"kind": "key_toggle", "key": KEY_F},
					"aux3": {"kind": "none"}, "aux4": {"kind": "none"},
				},
			}
		_:
			return {
				"deadzone": 0.02,
				"bindings": {
					"roll": _axis(0, false), "pitch": _axis(1, false), "throttle": _axis(2, false), "yaw": _axis(3, false),
					"aux1": _axis(4, false), "aux2": _axis(5, false), "aux3": _axis(6, false), "aux4": _axis(7, false),
				},
			}


static func _axis(index: int, invert: bool) -> Dictionary:
	return {"kind": "axis", "device": -1, "index": index, "invert": invert}


static func _button(index: int) -> Dictionary:
	return {"kind": "button", "device": -1, "index": index}


## "gamepad" or "radio" for a controller name Godot reports, "" when the name says nothing.
static func preset_for_device(device_name: String) -> String:
	var n := device_name.to_lower()
	for marker in GAMEPAD_NAMES:
		if n.contains(marker):
			return "gamepad"
	for marker in RADIO_NAMES:
		if n.contains(marker):
			return "radio"
	return ""


## What identifies a controller across plugs: its guid, else its name ("" for no such device).
static func device_key(input, device: int) -> String:
	var guid: String = input.get_joy_guid(device) if input.has_method("get_joy_guid") else ""
	return guid if guid != "" else (input.get_joy_name(device) if input.has_method("get_joy_name") else "")


## Keeps the well-formed controllers and axes of a calibration table (from a file): finite numbers, min <= centre <= max.
static func clean_calibration(table) -> Dictionary:
	var out := {}
	if typeof(table) != TYPE_DICTIONARY:
		return out
	for key in table:
		var entry = table[key]
		if typeof(entry) != TYPE_DICTIONARY or typeof(entry.get("axes")) != TYPE_DICTIONARY:
			continue
		var axes := {}
		for axis in entry["axes"]:
			var cal = entry["axes"][axis]
			if not str(axis).is_valid_int() or typeof(cal) != TYPE_DICTIONARY:
				continue
			var low := float(cal.get("min", NAN))
			var mid := float(cal.get("center", NAN))
			var high := float(cal.get("max", NAN))
			if is_finite(low) and is_finite(mid) and is_finite(high) and low <= mid and mid <= high:
				axes[str(int(axis))] = {"min": low, "center": mid, "max": high}
		if not axes.is_empty():
			out[str(key)] = {"name": str(entry.get("name", "")), "axes": axes}
	return out


static func _pair(neg: int, pos: int, scale: float) -> Dictionary:
	return {"kind": "key_pair", "neg": neg, "pos": pos, "scale": scale}


func apply_preset(name: String) -> void:
	var p := preset(name)
	preset_name = name if name in PRESET_NAMES else "radio"
	bindings = p["bindings"].duplicate(true)
	deadzone = p["deadzone"]
	shaping = {}
	reset_state()


func reset_state() -> void:
	_ramp.clear()
	_toggle.clear()
	_was_down.clear()


func set_binding(channel: String, binding: Dictionary) -> void:
	if channel in CHANNELS:
		bindings[channel] = binding
		preset_name = "custom"
		reset_state()


## "Axis 2 of any controller", "Button 0", "Key Space", ... for the controls screen.
func describe(channel: String) -> String:
	var b: Dictionary = bindings.get(channel, {"kind": "none"})
	var device := "any controller" if int(b.get("device", -1)) < 0 else "controller %d" % int(b.get("device", -1))
	match b.get("kind", "none"):
		"axis":
			return "axis %d of %s%s" % [int(b["index"]), device, " (inverted)" if b.get("invert", false) else ""]
		"button":
			return "button %d of %s (toggle)" % [int(b["index"]), device]
		"key_toggle":
			return "key %s (toggle)" % OS.get_keycode_string(int(b["key"]))
		"key_pair":
			return "keys %s / %s" % [OS.get_keycode_string(int(b["neg"])), OS.get_keycode_string(int(b["pos"]))]
		"key_ramp":
			return "keys %s / %s (hold)" % [OS.get_keycode_string(int(b["down"])), OS.get_keycode_string(int(b["up"]))]
	return "not bound"


## One profile's worth: preset name, deadzone, bindings, shaping.
func to_profile() -> Dictionary:
	return {"preset": preset_name, "deadzone": deadzone, "bindings": bindings.duplicate(true), "shaping": shaping.duplicate(true)}


func to_json() -> String:
	return JSON.stringify(to_profile(), "\t")


## Loads bindings saved by `to_json`. Returns false (changing nothing) when the text is not a valid controls file.
func from_json(text: String) -> bool:
	var json := JSON.new()
	if json.parse(text) != OK:
		return false
	return typeof(json.data) == TYPE_DICTIONARY and from_profile(json.data)


## Loads a profile (a parsed `to_profile`, or a version 1 file, which has no shaping). False, changing nothing, when
## it is not a valid one.
func from_profile(data: Dictionary) -> bool:
	if typeof(data.get("bindings")) != TYPE_DICTIONARY:
		return false
	var loaded := {}
	for channel in CHANNELS:
		var b = data["bindings"].get(channel, {"kind": "none"})
		if typeof(b) != TYPE_DICTIONARY:
			return false
		var clean := _clean_binding(b)
		if clean.is_empty():
			return false
		loaded[channel] = clean
	bindings = loaded
	deadzone = clampf(float(data.get("deadzone", 0.03)), 0.0, 0.5)
	shaping = _clean_shaping(data.get("shaping"))
	preset_name = str(data.get("preset", "custom"))
	reset_state()
	return true


## Keeps roll, pitch and yaw entries; clamps their numbers and drops anything else.
static func _clean_shaping(raw) -> Dictionary:
	var out := {}
	if typeof(raw) != TYPE_DICTIONARY:
		return out
	for channel in STICK_CHANNELS:
		if typeof(raw.get(channel)) != TYPE_DICTIONARY:
			continue
		var entry := {}
		if typeof(raw[channel].get("deadband")) in [TYPE_FLOAT, TYPE_INT]:
			entry["deadband"] = clampf(float(raw[channel]["deadband"]), 0.0, 0.5)
		if typeof(raw[channel].get("travel")) in [TYPE_FLOAT, TYPE_INT]:
			entry["travel"] = AxisShaping.clean_travel(float(raw[channel]["travel"]))
		out[channel] = entry
	return out


static func _clean_binding(b: Dictionary) -> Dictionary:
	# JSON turns integers into floats; bring the integer fields back and reject unknown kinds.
	var kind := str(b.get("kind", "none"))
	var out := {"kind": kind}
	for field in ["device", "index", "key", "neg", "pos", "down", "up"]:
		if b.has(field):
			out[field] = int(b[field])
	if b.has("invert"):
		out["invert"] = bool(b["invert"])
	if b.has("scale"):
		out["scale"] = float(b["scale"])
	var needs := {"none": [], "axis": ["index"], "button": ["index"], "key_toggle": ["key"], "key_pair": ["neg", "pos"], "key_ramp": ["down", "up"]}
	if not needs.has(kind):
		return {}
	for field in needs[kind]:
		if not out.has(field):
			return {}
	return out


## Reads the sticks. `focused` false (the game window lost focus) centres the sticks and closes the throttle;
## the aux switches stay as they are. `delta` is the time since the last read, for the keyboard throttle.
## Returns {roll, pitch, yaw, throttle, aux: [4 values], status}; `status` is a warning for the HUD or "".
func read(input, focused := true, delta := 0.0) -> Dictionary:
	var missing := false
	var values := {}
	for channel in CHANNELS:
		var v := _channel_value(channel, input, delta)
		if is_nan(v):
			missing = true
			v = -1.0 if channel == "throttle" or channel.begins_with("aux") else 0.0
		values[channel] = clampf(v, -1.0, 1.0) if is_finite(v) else 0.0
	var out := {
		"roll": _shaped("roll", values["roll"]), "pitch": _shaped("pitch", values["pitch"]), "yaw": _shaped("yaw", values["yaw"]),
		"throttle": (values["throttle"] + 1.0) / 2.0,
		"aux": [values["aux1"], values["aux2"], values["aux3"], values["aux4"]],
		"status": "",
	}
	if missing:
		out["status"] = "Controller not connected: the sticks are centred"
	if not focused:
		out["roll"] = 0.0
		out["pitch"] = 0.0
		out["yaw"] = 0.0
		out["throttle"] = 0.0
		_ramp.erase("throttle")
	return out


func _shaped(channel: String, v: float) -> float:
	var s: Dictionary = shaping.get(channel, {})
	return AxisShaping.shape(v, float(s.get("deadband", deadzone)), float(s.get("travel", 1.0)))


## The channel's value in [-1, 1], or NAN when its controller is not there.
func _channel_value(channel: String, input, delta: float) -> float:
	var b: Dictionary = bindings.get(channel, {"kind": "none"})
	var neutral := -1.0 if channel == "throttle" or channel.begins_with("aux") else 0.0
	match b.get("kind", "none"):
		"axis":
			var device := _device(b, input)
			if device < 0:
				return NAN
			var v: float = input.get_joy_axis(device, int(b["index"]))
			if not is_finite(v):
				return NAN
			var cal: Dictionary = calibration.get(device_key(input, device), {}).get("axes", {}).get(str(int(b["index"])), {})
			v = AxisShaping.calibrate(v, cal, channel in STICK_CHANNELS)
			return -v if b.get("invert", false) else v
		"button":
			var device := _device(b, input)
			if device < 0:
				return NAN
			if _pressed_edge("%s/button" % channel, input.is_joy_button_pressed(device, int(b["index"]))):
				_toggle[channel] = not _toggle.get(channel, false)
			return 1.0 if _toggle.get(channel, false) else -1.0
		"key_toggle":
			if _pressed_edge("%s/key" % channel, input.is_physical_key_pressed(int(b["key"]))):
				_toggle[channel] = not _toggle.get(channel, false)
			return 1.0 if _toggle.get(channel, false) else -1.0
		"key_pair":
			var scale := float(b.get("scale", 1.0))
			var v := 0.0
			if input.is_physical_key_pressed(int(b["pos"])):
				v += scale
			if input.is_physical_key_pressed(int(b["neg"])):
				v -= scale
			return v
		"key_ramp":
			var speed := throttle_ramp_per_s * 2.0 * delta
			var v: float = _ramp.get(channel, -1.0)
			if input.is_physical_key_pressed(int(b["up"])):
				v += speed
			if input.is_physical_key_pressed(int(b["down"])):
				v -= speed
			v = clampf(v, -1.0, 1.0)
			_ramp[channel] = v
			return v
	return neutral


## The controller a binding reads: its own device if connected, else (device -1) the first connected one; -1 if none.
func _device(b: Dictionary, input) -> int:
	var pads: Array = input.get_connected_joypads()
	var wanted := int(b.get("device", -1))
	if wanted >= 0:
		return wanted if wanted in pads else -1
	return int(pads[0]) if pads.size() > 0 else -1


func _pressed_edge(id: String, down: bool) -> bool:
	var edge: bool = down and not _was_down.get(id, false)
	_was_down[id] = down
	return edge
