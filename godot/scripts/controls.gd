extends RefCounted
## Turns joystick axes, buttons and keys into the transmitter's sticks.
##
## Every channel (roll, pitch, yaw, throttle, aux1..aux4) has one binding. A binding reads a value in [-1, 1]:
## roll/pitch/yaw/aux are used as they are, throttle becomes (value + 1) / 2. Sign conventions, as Betaflight sees
## them: roll +1 is right, pitch +1 is stick forward (nose down), yaw +1 is right, throttle 1 is full, aux +1 is the
## switch on (aux1 arms, aux2 selects Angle mode in the reference quad).
##
## `read` takes the input source as a parameter (the `Input` singleton in the game, a fake in the tests).

const CHANNELS := ["roll", "pitch", "yaw", "throttle", "aux1", "aux2", "aux3", "aux4"]
const STICK_CHANNELS := ["roll", "pitch", "yaw"]
const PRESET_NAMES := ["radio", "gamepad", "keyboard"]

## device -1 means "the first connected controller".
var bindings: Dictionary = {}
var deadzone := 0.03
## Keyboard throttle: how far the throttle stick moves per second (throttle goes 0..1).
var throttle_ramp_per_s := 0.6
var preset_name := ""

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


static func _pair(neg: int, pos: int, scale: float) -> Dictionary:
	return {"kind": "key_pair", "neg": neg, "pos": pos, "scale": scale}


func apply_preset(name: String) -> void:
	var p := preset(name)
	preset_name = name if name in PRESET_NAMES else "radio"
	bindings = p["bindings"].duplicate(true)
	deadzone = p["deadzone"]
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


func to_json() -> String:
	return JSON.stringify({"version": 1, "preset": preset_name, "deadzone": deadzone, "bindings": bindings}, "\t")


## Loads bindings saved by `to_json`. Returns false (changing nothing) when the text is not a valid controls file.
func from_json(text: String) -> bool:
	var json := JSON.new()
	if json.parse(text) != OK:
		return false
	var data = json.data
	if typeof(data) != TYPE_DICTIONARY or typeof(data.get("bindings")) != TYPE_DICTIONARY:
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
	preset_name = str(data.get("preset", "custom"))
	reset_state()
	return true


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
		"roll": _dead(values["roll"]), "pitch": _dead(values["pitch"]), "yaw": _dead(values["yaw"]),
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


func _dead(v: float) -> float:
	if absf(v) <= deadzone:
		return 0.0
	return signf(v) * (absf(v) - deadzone) / (1.0 - deadzone)


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
