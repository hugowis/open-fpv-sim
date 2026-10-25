extends PanelContainer
## The calibration wizard: pick the controller, leave it alone for a second (centres), then move every stick to all
## its extremes and flip the switches (end points), then Done. The maths is `Calibration`; this node polls the
## controller and shows each axis. docs/superpowers/specs/2026-10-26-controller-setup-design.md, R3.

const Calibration = preload("res://scripts/calibration.gd")
const Controls = preload("res://scripts/controls.gd")

## Done pressed with a usable result: the controller's key and name and {"<axis>": {min, center, max}}.
signal finished(key: String, device_name: String, result: Dictionary)
signal cancelled

## Stands in for the `Input` singleton in the tests.
var input = Input
var calibration := Calibration.new()
var device := -1

var _pick := OptionButton.new()
var _text := Label.new()
var _rest_bar := ProgressBar.new()
var _bars := {}  # axis -> ProgressBar
var _grid := GridContainer.new()
var _done := Button.new()
var _cancel := Button.new()
var _pads: Array = []


func _ready() -> void:
	visible = false
	set_anchors_preset(Control.PRESET_CENTER)
	grow_horizontal = Control.GROW_DIRECTION_BOTH
	grow_vertical = Control.GROW_DIRECTION_BOTH
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.14, 0.15, 0.2, 0.99)
	style.set_corner_radius_all(8)
	style.set_content_margin_all(18)
	add_theme_stylebox_override("panel", style)
	var box := VBoxContainer.new()
	box.add_theme_constant_override("separation", 10)
	box.custom_minimum_size.x = 420
	add_child(box)
	var title := Label.new()
	title.text = "Calibrate a controller"
	title.add_theme_font_size_override("font_size", 22)
	box.add_child(title)
	_pick.item_selected.connect(func(_i: int) -> void: _restart())
	box.add_child(_pick)
	_text.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	box.add_child(_text)
	_rest_bar.max_value = 1.0
	_rest_bar.show_percentage = false
	box.add_child(_rest_bar)
	_grid.columns = 2
	_grid.add_theme_constant_override("h_separation", 10)
	box.add_child(_grid)
	var buttons := HBoxContainer.new()
	box.add_child(buttons)
	_done.text = "Done"
	_done.pressed.connect(finish)
	buttons.add_child(_done)
	_cancel.text = "Cancel"
	_cancel.pressed.connect(cancel)
	buttons.add_child(_cancel)


## Opens the wizard on the connected controllers. False (and nothing opens) when none is connected.
func start() -> bool:
	_pads = input.get_connected_joypads()
	if _pads.is_empty():
		return false
	_pick.clear()
	for pad in _pads:
		_pick.add_item("%d: %s" % [pad, input.get_joy_name(pad)])
	_pick.visible = _pads.size() > 1
	_restart()
	visible = true
	_done.grab_focus()
	return true


func is_open() -> bool:
	return visible


func step() -> String:
	return calibration.step


func hint() -> String:
	return _text.text


## The controller being calibrated (a device id).
func current_device() -> int:
	return device


## Polls the controller; called every frame while open (and by the tests).
func poll(delta: float) -> void:
	if not visible or device < 0:
		return
	var samples := {}
	for axis in JOY_AXIS_MAX:
		samples[axis] = input.get_joy_axis(device, axis)
	calibration.feed(samples, delta)
	_rest_bar.value = calibration.rest_progress()
	if calibration.step == "sweep":
		_text.text = "Move every stick to all its extremes, a few circles, and flip each switch through its positions. Then press Done."
	_done.disabled = calibration.step != "sweep"
	for axis in samples:
		_show_axis(axis, samples[axis])


func _process(delta: float) -> void:
	poll(delta)


func finish() -> void:
	var result: Dictionary = calibration.result()
	if result.is_empty():
		_text.text = "No axis moved enough. Move the sticks all the way, then press Done."
		return
	visible = false
	finished.emit(Controls.device_key(input, device), input.get_joy_name(device), result)


func cancel() -> void:
	visible = false
	cancelled.emit()


func _input(event: InputEvent) -> void:
	if visible and event.is_action_pressed("ui_cancel"):
		cancel()
		get_viewport().set_input_as_handled()


func _restart() -> void:
	device = _pads[_pick.selected] if _pick.selected >= 0 and _pick.selected < _pads.size() else int(_pads[0])
	calibration = Calibration.new()
	for child in _grid.get_children():
		child.queue_free()
	_bars.clear()
	_text.text = "Leave every stick and switch alone for a second (a throttle can stay at the bottom)."
	_rest_bar.value = 0.0
	_done.disabled = true


## An axis gets a bar the first time it leaves 0 by more than a little (an unused axis shows nothing).
func _show_axis(axis: int, value: float) -> void:
	if not _bars.has(axis):
		if absf(value) < 0.05:
			return
		var label := Label.new()
		label.text = "Axis %d" % axis
		var bar := ProgressBar.new()
		bar.min_value = -1.0
		bar.max_value = 1.0
		bar.show_percentage = false
		bar.custom_minimum_size = Vector2(240, 16)
		_grid.add_child(label)
		_grid.add_child(bar)
		_bars[axis] = bar
	_bars[axis].value = value
