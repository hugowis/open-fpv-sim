extends Control
## The sticks being sent, drawn like a transmitter's: the left box is yaw (across) and throttle (up), the right box
## roll (across) and pitch (up is stick forward), and four squares show the aux switches.
## Seeing it move is the quickest way to check that a controller is bound right.

const BOX := 84.0
const GAP := 14.0

var sticks: Dictionary = {}:
	set(value):
		sticks = value
		queue_redraw()


func _init() -> void:
	custom_minimum_size = Vector2(BOX * 2.0 + GAP * 3.0 + 60.0, BOX)
	mouse_filter = Control.MOUSE_FILTER_IGNORE


func _draw() -> void:
	if sticks.is_empty():
		return
	_stick_box(Rect2(0.0, 0.0, BOX, BOX), sticks["yaw"], sticks["throttle"] * 2.0 - 1.0)
	_stick_box(Rect2(BOX + GAP, 0.0, BOX, BOX), sticks["roll"], sticks["pitch"])
	var aux: Array = sticks["aux"]
	for i in aux.size():
		var rect := Rect2(BOX * 2.0 + GAP * 2.0 + (i % 2) * 28.0, (i / 2) * 28.0, 24.0, 24.0)
		var on: bool = aux[i] > 0.5
		draw_rect(rect, Color(0.3, 0.9, 0.4, 0.85) if on else Color(0, 0, 0, 0.4))
		draw_rect(rect, Color(1, 1, 1, 0.6), false, 1.5)


func _stick_box(rect: Rect2, x: float, y: float) -> void:
	draw_rect(rect, Color(0, 0, 0, 0.4))
	draw_rect(rect, Color(1, 1, 1, 0.6), false, 1.5)
	var c := rect.get_center()
	draw_line(Vector2(rect.position.x, c.y), Vector2(rect.end.x, c.y), Color(1, 1, 1, 0.2))
	draw_line(Vector2(c.x, rect.position.y), Vector2(c.x, rect.end.y), Color(1, 1, 1, 0.2))
	var dot := c + Vector2(clampf(x, -1.0, 1.0) * rect.size.x * 0.5, -clampf(y, -1.0, 1.0) * rect.size.y * 0.5)
	draw_circle(dot, 5.0, Color(1.0, 0.85, 0.3))
