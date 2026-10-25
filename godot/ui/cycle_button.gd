extends Button
## A button that cycles through a list of values: press (or right, d-pad right) for the next, left for the previous.
## The F2 screen works the same way, and it needs no popup, so a gamepad drives it.

## The value changed by the pilot (`set_value` does not emit it).
signal changed(value)

var values: Array = []
var labels: Array = []
var index := 0
var title := ""


## `labels` default to the values; `title` ("World") prefixes the shown label when it is not empty.
func setup(new_title: String, new_values: Array, new_labels: Array = []) -> void:
	title = new_title
	values = new_values
	labels = new_labels if not new_labels.is_empty() else new_values.map(func(v): return str(v))
	index = clampi(index, 0, maxi(values.size() - 1, 0))
	_show()


func value():
	return values[index] if not values.is_empty() else null


## Shows `new_value` (the first value when it is not in the list) without emitting `changed`.
func set_value(new_value) -> void:
	index = maxi(values.find(new_value), 0)
	_show()


func step(by: int) -> void:
	if values.size() < 2:
		return
	index = posmod(index + by, values.size())
	_show()
	changed.emit(values[index])


func _pressed() -> void:
	step(1)


func _gui_input(event: InputEvent) -> void:
	if event.is_action_pressed("ui_right"):
		step(1)
		accept_event()
	elif event.is_action_pressed("ui_left"):
		step(-1)
		accept_event()


func _show() -> void:
	var label: String = str(labels[index]) if index < labels.size() else ""
	text = "%s: %s" % [title, label] if title != "" else label
