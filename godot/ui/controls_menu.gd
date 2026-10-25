extends CanvasLayer
## The controls setup screen (F2): choose a preset, bind each channel by moving its stick or pressing its key or
## button, flip an axis, set the deadzone. Every change is saved at once. The live bars show what each channel is
## sending, so a wrong axis is obvious.

const STICK_CHANNELS := ["roll", "pitch", "yaw", "throttle"]
const LABELS := {
	"roll": "Roll", "pitch": "Pitch", "yaw": "Yaw", "throttle": "Throttle",
	"aux1": "Aux 1 (arm)", "aux2": "Aux 2 (angle mode)", "aux3": "Aux 3", "aux4": "Aux 4",
}
const BIND_THRESHOLD := 0.6
const ASPECTS := ["4:3", "16:9", "window"]
const DVR_EXPORTS := ["none", "frames", "mp4", "both"]
const DVR_FEEDS := ["goggles", "clean", "both"]

## The "Video effects" box changed (the game applies and saves it).
signal video_effects_toggled(on: bool)

## The "Picture" button changed the aspect setting (the game applies and saves it).
signal aspect_changed(value: String)

## The "DVR" box changed (the game starts or stops the recording).
signal dvr_toggled(on: bool)

## The "DVR export" button changed the export setting (the game applies and saves it).
signal dvr_export_changed(value: String)

## The "DVR feed" button changed which picture is recorded (the game applies and saves it).
signal dvr_feed_changed(value: String)

var controls = null  # Controls
var save_path := ""

var _panel := PanelContainer.new()
var _preset := OptionButton.new()
var _deadzone := HSlider.new()
var _deadzone_label := Label.new()
var _hint := Label.new()
var _video_effects := CheckBox.new()
var _dvr := CheckBox.new()
var _aspect: Button = Button.new()
var _aspect_index := 0
var _dvr_export: Button = Button.new()
var _dvr_export_index := 0
var _dvr_feed: Button = Button.new()
var _dvr_feed_index := 0
var _rows := {}  # channel -> {description, bind, invert, live}
var _listening := ""


func _ready() -> void:
	visible = false
	var dim := ColorRect.new()
	dim.color = Color(0, 0, 0, 0.55)
	dim.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(dim)
	_panel.set_anchors_preset(Control.PRESET_CENTER)
	_panel.grow_horizontal = Control.GROW_DIRECTION_BOTH
	_panel.grow_vertical = Control.GROW_DIRECTION_BOTH
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.1, 0.11, 0.14, 0.96)
	style.set_corner_radius_all(8)
	style.set_content_margin_all(18)
	_panel.add_theme_stylebox_override("panel", style)
	add_child(_panel)
	var box := VBoxContainer.new()
	box.add_theme_constant_override("separation", 10)
	_panel.add_child(box)
	var title := Label.new()
	title.text = "Controls"
	title.add_theme_font_size_override("font_size", 26)
	box.add_child(title)

	var preset_row := HBoxContainer.new()
	box.add_child(preset_row)
	var preset_label := Label.new()
	preset_label.text = "Preset"
	preset_row.add_child(preset_label)
	_preset.add_item("Radio (USB joystick, AETR)", 0)
	_preset.add_item("Gamepad (Mode 2)", 1)
	_preset.add_item("Keyboard", 2)
	_preset.item_selected.connect(_on_preset_selected)
	preset_row.add_child(_preset)

	var grid := GridContainer.new()
	grid.columns = 5
	grid.add_theme_constant_override("h_separation", 14)
	box.add_child(grid)
	for channel in ["roll", "pitch", "yaw", "throttle", "aux1", "aux2", "aux3", "aux4"]:
		var name_label := Label.new()
		name_label.text = LABELS[channel]
		name_label.custom_minimum_size.x = 150
		var description := Label.new()
		description.custom_minimum_size.x = 260
		var bind := Button.new()
		bind.text = "Bind"
		bind.pressed.connect(_start_listening.bind(channel))
		var invert := CheckBox.new()
		invert.text = "Invert"
		invert.toggled.connect(_on_invert_toggled.bind(channel))
		var live := ProgressBar.new()
		live.min_value = -1.0
		live.max_value = 1.0
		live.show_percentage = false
		live.custom_minimum_size = Vector2(160, 18)
		for node in [name_label, description, bind, invert, live]:
			grid.add_child(node)
		_rows[channel] = {"description": description, "bind": bind, "invert": invert, "live": live}

	var dz_row := HBoxContainer.new()
	box.add_child(dz_row)
	var dz_title := Label.new()
	dz_title.text = "Deadzone"
	dz_row.add_child(dz_title)
	_deadzone.min_value = 0.0
	_deadzone.max_value = 0.3
	_deadzone.step = 0.005
	_deadzone.custom_minimum_size.x = 220
	_deadzone.value_changed.connect(_on_deadzone_changed)
	dz_row.add_child(_deadzone)
	dz_row.add_child(_deadzone_label)

	_video_effects.text = "Video effects (analog breakup with distance and obstacles)"
	_video_effects.button_pressed = true
	_video_effects.toggled.connect(func(on: bool) -> void: video_effects_toggled.emit(on))
	box.add_child(_video_effects)

	_dvr.text = "DVR: record the FPV view"
	_dvr.toggled.connect(func(on: bool) -> void: dvr_toggled.emit(on))
	box.add_child(_dvr)

	_aspect.text = "Picture: 4:3"
	_aspect.pressed.connect(func(): _aspect_index = (_aspect_index + 1) % ASPECTS.size(); _aspect.text = "Picture: %s" % ASPECTS[_aspect_index]; aspect_changed.emit(ASPECTS[_aspect_index]))
	box.add_child(_aspect)

	_dvr_export.text = "DVR export: none"
	_dvr_export.pressed.connect(func(): _dvr_export_index = (_dvr_export_index + 1) % DVR_EXPORTS.size(); _dvr_export.text = "DVR export: %s" % DVR_EXPORTS[_dvr_export_index]; dvr_export_changed.emit(DVR_EXPORTS[_dvr_export_index]))
	box.add_child(_dvr_export)

	_dvr_feed.text = "DVR feed: goggles"
	_dvr_feed.pressed.connect(func(): _dvr_feed_index = (_dvr_feed_index + 1) % DVR_FEEDS.size(); _dvr_feed.text = "DVR feed: %s" % DVR_FEEDS[_dvr_feed_index]; dvr_feed_changed.emit(DVR_FEEDS[_dvr_feed_index]))
	box.add_child(_dvr_feed)

	_hint.text = "Bind: then move the stick (or press the button or key). Esc cancels, F2 closes."
	box.add_child(_hint)


## Hands the screen the mapper it edits and where to save it.
func setup(mapper, path: String) -> void:
	controls = mapper
	save_path = path
	_refresh()


func toggle() -> void:
	_listening = ""
	visible = not visible
	_hint.text = "Bind: then move the stick (or press the button or key). Esc cancels, F2 closes."
	if visible:
		_refresh()


func is_open() -> bool:
	return visible


## Shows the "Video effects" setting without emitting `video_effects_toggled`.
func set_video_effects(on: bool) -> void:
	_video_effects.set_pressed_no_signal(on)


func video_effects_checked() -> bool:
	return _video_effects.button_pressed


## Shows the DVR's state without emitting `dvr_toggled`.
func set_dvr(on: bool) -> void:
	_dvr.set_pressed_no_signal(on)


func dvr_on() -> bool:
	return _dvr.button_pressed


## Shows the DVR export setting without emitting `dvr_export_changed`.
func set_dvr_export(value: String) -> void:
	_dvr_export_index = maxi(DVR_EXPORTS.find(value), 0)
	_dvr_export.text = "DVR export: %s" % DVR_EXPORTS[_dvr_export_index]


## Shows the DVR feed setting without emitting `dvr_feed_changed`.
func set_dvr_feed(value: String) -> void:
	_dvr_feed_index = maxi(DVR_FEEDS.find(value), 0)
	_dvr_feed.text = "DVR feed: %s" % DVR_FEEDS[_dvr_feed_index]


## Shows the aspect setting without emitting `aspect_changed`.
func set_aspect(value: String) -> void:
	_aspect_index = ASPECTS.find(value)
	if _aspect_index < 0:
		_aspect_index = 0
	_aspect.text = "Picture: %s" % ASPECTS[_aspect_index]


## The channel being bound ("" when none).
func listening_for() -> String:
	return _listening


## Feeds the live bars from the sticks being sent (the dictionary `Controls.read` returns).
func update_live(sticks: Dictionary) -> void:
	if not visible or sticks.is_empty():
		return
	var values := {
		"roll": sticks["roll"], "pitch": sticks["pitch"], "yaw": sticks["yaw"], "throttle": sticks["throttle"] * 2.0 - 1.0,
		"aux1": sticks["aux"][0], "aux2": sticks["aux"][1], "aux3": sticks["aux"][2], "aux4": sticks["aux"][3],
	}
	for channel in _rows:
		_rows[channel]["live"].value = values[channel]


func _refresh() -> void:
	if controls == null or _rows.is_empty():
		return
	_preset.selected = {"radio": 0, "gamepad": 1, "keyboard": 2}.get(controls.preset_name, -1)
	_deadzone.set_value_no_signal(controls.deadzone)
	_deadzone_label.text = "%.3f" % controls.deadzone
	for channel in _rows:
		var binding: Dictionary = controls.bindings.get(channel, {"kind": "none"})
		_rows[channel]["description"].text = controls.describe(channel)
		var is_axis: bool = binding.get("kind", "none") == "axis"
		_rows[channel]["invert"].disabled = not is_axis
		_rows[channel]["invert"].set_pressed_no_signal(is_axis and binding.get("invert", false))
		_rows[channel]["bind"].text = "Bind"


func _start_listening(channel: String) -> void:
	_listening = channel
	_rows[channel]["bind"].text = "..."
	_hint.text = "Binding %s: move its stick%s. Esc cancels." % [LABELS[channel], "" if channel in STICK_CHANNELS else ", or press its button or key"]


func _input(event: InputEvent) -> void:
	if not visible or _listening == "":
		return
	var binding := {}
	if event is InputEventKey and event.pressed and not event.echo:
		if event.physical_keycode == KEY_ESCAPE:
			_listening = ""
			_refresh()
			get_viewport().set_input_as_handled()
			return
		if not _listening in STICK_CHANNELS:
			binding = {"kind": "key_toggle", "key": event.physical_keycode}
	elif event is InputEventJoypadMotion and absf(event.axis_value) >= BIND_THRESHOLD:
		binding = {"kind": "axis", "device": _device_for(event.device), "index": event.axis, "invert": false}
	elif event is InputEventJoypadButton and event.pressed and not _listening in STICK_CHANNELS:
		binding = {"kind": "button", "device": _device_for(event.device), "index": event.button_index}
	if binding.is_empty():
		return
	controls.set_binding(_listening, binding)
	_listening = ""
	_save()
	_refresh()
	get_viewport().set_input_as_handled()


## One controller connected: "any" (-1) survives it being replugged under another id; several: bind the one used.
func _device_for(device: int) -> int:
	return -1 if Input.get_connected_joypads().size() <= 1 else device


func _on_preset_selected(index: int) -> void:
	controls.apply_preset(["radio", "gamepad", "keyboard"][index])
	_save()
	_refresh()


func _on_invert_toggled(pressed: bool, channel: String) -> void:
	var binding: Dictionary = controls.bindings.get(channel, {}).duplicate()
	if binding.get("kind", "none") != "axis":
		return
	binding["invert"] = pressed
	controls.set_binding(channel, binding)
	_save()
	_refresh()


func _on_deadzone_changed(value: float) -> void:
	controls.deadzone = value
	_deadzone_label.text = "%.3f" % value
	_save()


func _save() -> void:
	if save_path == "":
		return
	var file := FileAccess.open(save_path, FileAccess.WRITE)
	if file != null:
		file.store_string(controls.to_json())
