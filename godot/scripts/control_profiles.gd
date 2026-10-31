extends RefCounted
## The controls file: named profiles (each one a snapshot of the mapper: preset, deadzone, bindings, shaping) and the
## calibration table, which belongs to the controllers. The store owns the live `Controls` object, so the rest of the
## client keeps reading `controls`. docs/superpowers/specs/2026-10-26-controller-setup-design.md, R4.

const Controls = preload("res://scripts/controls.gd")
const FIRST_NAME := "Default"
const MAX_NAME := 40

var controls  # Controls, the active profile's live setup
var active := FIRST_NAME
## name -> profile (`Controls.to_profile`), in creation order. The active one is stale until `snapshot`.
var profiles := {}


func _init(mapper) -> void:
	controls = mapper
	profiles = {FIRST_NAME: mapper.to_profile()}


func names() -> Array:
	return profiles.keys()


## Stores the mapper's current setup in the active profile.
func snapshot() -> void:
	profiles[active] = controls.to_profile()


func switch_to(name: String) -> bool:
	if not profiles.has(name):
		return false
	snapshot()
	active = name
	controls.from_profile(profiles[name])
	return true


## A new profile from a preset, made active. Returns its (trimmed, made unique) name.
func add(name: String, preset: String) -> String:
	snapshot()
	active = unique_name(name)
	controls.apply_preset(preset)
	profiles[active] = controls.to_profile()
	return active


## False when it is the last profile or does not exist.
func remove(name: String) -> bool:
	if not profiles.has(name) or profiles.size() < 2:
		return false
	profiles.erase(name)
	if name == active:
		active = profiles.keys()[0]
		controls.from_profile(profiles[active])
	return true


func unique_name(name: String) -> String:
	var base := name.strip_edges().left(MAX_NAME)
	if base == "":
		base = "Profile"
	var candidate := base
	var n := 2
	while profiles.has(candidate):
		candidate = "%s %d" % [base, n]
		n += 1
	return candidate


func to_json() -> String:
	snapshot()
	return JSON.stringify({"version": 2, "active": active, "calibration": controls.calibration, "profiles": profiles}, "\t")


## Loads a controls file: this format, or the old bare-bindings one (which becomes the profile "Default"). False,
## changing nothing, when the text holds no usable profile.
func from_json(text: String) -> bool:
	var json := JSON.new()
	if json.parse(text) != OK or typeof(json.data) != TYPE_DICTIONARY:
		return false
	var data: Dictionary = json.data
	var loaded := {}
	var scratch = Controls.new()
	if data.has("profiles"):
		if typeof(data["profiles"]) != TYPE_DICTIONARY:
			return false
		for name in data["profiles"]:
			if typeof(data["profiles"][name]) == TYPE_DICTIONARY and str(name) != "" and scratch.from_profile(data["profiles"][name]):
				loaded[str(name).left(MAX_NAME)] = scratch.to_profile()
	elif scratch.from_profile(data):
		loaded[FIRST_NAME] = scratch.to_profile()
	if loaded.is_empty():
		return false
	profiles = loaded
	active = str(data.get("active", "")) if loaded.has(str(data.get("active", ""))) else loaded.keys()[0]
	if data.has("profiles"):
		controls.calibration = Controls.clean_calibration(data.get("calibration"))
	controls.from_profile(profiles[active])
	return true
