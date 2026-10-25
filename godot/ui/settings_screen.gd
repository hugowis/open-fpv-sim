extends CanvasLayer
## The settings screen: a view of `[client]` in user://ofs_client.cfg. Each row shows the effective setting; the game
## applies and saves a change (`setting_changed`). A setting a flag or an OFS_* variable sets this run is locked and says
## which one, since a saved value could not take effect.

const AppSettings = preload("res://scripts/settings.gd")
const ControlsMenu = preload("res://ui/controls_menu.gd")
const CycleButton = preload("res://ui/cycle_button.gd")

## key, label, kind (bool, choice or text), whether it applies at once (else at the next Start).
const ROWS := [
	{"key": "video_effects", "label": "Video effects", "kind": "bool", "live": true},
	{"key": "fpv_aspect", "label": "Picture", "kind": "choice", "values": ControlsMenu.ASPECTS, "live": true},
	{"key": "hud_visible", "label": "HUD shown at start", "kind": "bool", "live": true},
	{"key": "fullscreen", "label": "Fullscreen", "kind": "bool", "live": true},
	{"key": "dvr_feed", "label": "DVR feed", "kind": "choice", "values": AppSettings.DVR_FEEDS, "live": true},
	{"key": "dvr_export", "label": "DVR export", "kind": "choice", "values": AppSettings.DVR_EXPORTS, "live": true},
	{"key": "open_loop", "label": "Open loop (no Betaflight)", "kind": "bool", "live": false},
	{"key": "server_bin", "label": "Simulator binary", "kind": "text", "live": false},
	{"key": "server_addr", "label": "Simulator address", "kind": "text", "live": false},
	{"key": "data_dir", "label": "Data directory", "kind": "text", "live": false},
	{"key": "sitl_launch", "label": "Betaflight SITL launch command", "kind": "text", "live": false},
]

## The pilot changed `key` to `value` (the game applies and saves it).
signal setting_changed(key: String, value)

## Back to the menu.
signal closed

var settings := {}  # the game's live settings
var locks := {}  # key -> "--flag" or "OFS_NAME"

var _controls := {}  # key -> control
var _notes := {}  # key -> Label
var _back := Button.new()


func _ready() -> void:
	visible = false
	var dim := ColorRect.new()
	dim.color = Color(0, 0, 0, 0.7)
	dim.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(dim)
	var panel := PanelContainer.new()
	panel.set_anchors_preset(Control.PRESET_CENTER)
	panel.grow_horizontal = Control.GROW_DIRECTION_BOTH
	panel.grow_vertical = Control.GROW_DIRECTION_BOTH
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.1, 0.11, 0.14, 0.96)
	style.set_corner_radius_all(8)
	style.set_content_margin_all(18)
	panel.add_theme_stylebox_override("panel", style)
	add_child(panel)
	var box := VBoxContainer.new()
	box.add_theme_constant_override("separation", 8)
	panel.add_child(box)
	var title := Label.new()
	title.text = "Settings"
	title.add_theme_font_size_override("font_size", 26)
	box.add_child(title)
	var grid := GridContainer.new()
	grid.columns = 3
	grid.add_theme_constant_override("h_separation", 14)
	box.add_child(grid)
	for row in ROWS:
		var label := Label.new()
		label.text = row["label"]
		var control := _make_control(row)
		var note := Label.new()
		note.modulate = Color(1, 1, 1, 0.6)
		for node in [label, control, note]:
			grid.add_child(node)
		_controls[row["key"]] = control
		_notes[row["key"]] = note
	var hint := Label.new()
	hint.text = "Saved to user://ofs_client.cfg. Esc goes back."
	hint.modulate = Color(1, 1, 1, 0.6)
	box.add_child(hint)
	_back.text = "Back"
	_back.pressed.connect(close)
	box.add_child(_back)
	_refresh()


func _make_control(row: Dictionary) -> Control:
	var key: String = row["key"]
	match row["kind"]:
		"bool":
			var check := CheckBox.new()
			check.toggled.connect(func(on: bool) -> void: setting_changed.emit(key, on))
			return check
		"choice":
			var cycle := CycleButton.new()
			cycle.setup("", row["values"])
			cycle.custom_minimum_size.x = 140
			cycle.changed.connect(func(value) -> void: setting_changed.emit(key, value))
			return cycle
	var edit := LineEdit.new()
	edit.custom_minimum_size.x = 380
	edit.text_submitted.connect(func(text: String) -> void: _commit_text(key, text))
	edit.focus_exited.connect(func() -> void: _commit_text(key, edit.text))
	return edit


func _commit_text(key: String, text: String) -> void:
	if text != str(settings.get(key, "")) and not locks.has(key):
		setting_changed.emit(key, text)


## Hands the screen the game's live settings and what overrides them this run.
func setup(live_settings: Dictionary, overridden: Dictionary) -> void:
	settings = live_settings
	locks = overridden
	_refresh()


func open() -> void:
	_refresh()
	visible = true
	for row in ROWS:
		if not locks.has(row["key"]):
			(_controls[row["key"]] as Control).grab_focus()
			return
	_back.grab_focus()


func close() -> void:
	if visible:
		visible = false
		closed.emit()


func is_open() -> bool:
	return visible


## The control that edits `key` (for tests).
func control(key: String) -> Control:
	return _controls[key]


func note(key: String) -> String:
	return (_notes[key] as Label).text


func _refresh() -> void:
	if settings.is_empty() or _controls.is_empty():
		return
	for row in ROWS:
		var key: String = row["key"]
		var control: Control = _controls[key]
		var locked := locks.has(key)
		match row["kind"]:
			"bool":
				(control as CheckBox).set_pressed_no_signal(settings[key])
				(control as CheckBox).disabled = locked
			"choice":
				(control as CycleButton).set_value(settings[key])
				(control as CycleButton).disabled = locked
			_:
				(control as LineEdit).text = str(settings[key])
				(control as LineEdit).editable = not locked
		(_notes[key] as Label).text = "set by %s this run" % locks[key] if locked else ("" if row["live"] else "next Start")


func _input(event: InputEvent) -> void:
	if visible and event is InputEventKey and event.pressed and not event.echo and event.physical_keycode == KEY_ESCAPE:
		close()
		get_viewport().set_input_as_handled()
