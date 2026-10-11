extends CanvasLayer
## Betaflight's OSD, drawn from the character grid the simulator decodes. It sits on layer 2: above the lens (layer 1,
## the camera optics) and below the analog video layer (3, ui/video.gd), which breaks up the picture and this OSD
## together as a real analog link does; the HUD is on layer 4. Hidden in the chase view.

const FONT: Texture2D = preload("res://ui/osd_font.png")
const GLYPH_W := 12
const GLYPH_H := 18
const ATLAS_COLS := 16
const BLINK_HALF_PERIOD_S := 0.5

var _canvas := Control.new()
var _frame: Dictionary = {}
var _enabled := true
var _blink_on := true
var _blink_clock := 0.0


func _ready() -> void:
	_canvas.name = "OsdCanvas"
	_canvas.set_anchors_preset(Control.PRESET_FULL_RECT)
	_canvas.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_canvas.texture_filter = CanvasItem.TEXTURE_FILTER_NEAREST
	_canvas.draw.connect(_draw_grid)
	add_child(_canvas)


func _process(delta: float) -> void:
	advance_blink(delta)


## Replaces the frame: the Dictionary `OfsClient.get_osd()` returns ({} = nothing to show).
func set_frame(frame: Dictionary) -> void:
	_frame = frame
	_canvas.queue_redraw()


func set_enabled(enabled: bool) -> void:
	_enabled = enabled
	_canvas.queue_redraw()


## True when there is an OSD to see: enabled, a frame the simulator marks present, and a grid.
func is_drawing() -> bool:
	return _enabled and bool(_frame.get("present", false)) and int(_frame.get("cols", 0)) > 0 and int(_frame.get("rows", 0)) > 0


func blink_visible() -> bool:
	return _blink_on


func advance_blink(delta: float) -> void:
	_blink_clock += delta
	if _blink_clock >= BLINK_HALF_PERIOD_S:
		_blink_clock = fposmod(_blink_clock, BLINK_HALF_PERIOD_S)
		_blink_on = not _blink_on
		_canvas.queue_redraw()


## How many font pages the atlas holds (the PNG stacks them vertically, 16 glyph rows of 18 px each).
static func font_pages() -> int:
	return int(FONT.get_size().y / (16.0 * GLYPH_H))


## Where glyph `code` of font `page` is in the atlas (pages beyond the atlas fall back to page 0: Betaflight
## addresses up to four pages, the analog font ships two).
@warning_ignore("integer_division")
static func glyph_rect(page: int, code: int) -> Rect2:
	var p := page if page < font_pages() else 0
	return Rect2((code % ATLAS_COLS) * GLYPH_W, (code / ATLAS_COLS) * GLYPH_H + p * ATLAS_COLS * GLYPH_H, GLYPH_W, GLYPH_H)


## How many cells would be drawn now: non-blank ones, minus the blinking ones in the off phase.
func drawn_cell_count() -> int:
	var count := 0
	if not is_drawing():
		return count
	for packed in _frame.get("cells", PackedInt32Array()):
		if _is_drawn(packed):
			count += 1
	return count


func _is_drawn(packed: int) -> bool:
	if (packed & 0xFF) == 0x20:
		return false
	return _blink_on or ((packed >> 10) & 1) == 0


func _draw_grid() -> void:
	if not is_drawing():
		return
	var cols: int = _frame["cols"]
	var rows: int = _frame["rows"]
	var cells: PackedInt32Array = _frame.get("cells", PackedInt32Array())
	if cells.size() != cols * rows:
		return
	var box := Rect2(Vector2.ZERO, _canvas.size)
	var cell_size := Vector2(box.size.x / cols, box.size.y / rows)
	for row in rows:
		for col in cols:
			var packed := cells[row * cols + col]
			if _is_drawn(packed):
				_canvas.draw_texture_rect_region(FONT, Rect2(box.position + Vector2(col, row) * cell_size, cell_size), glyph_rect((packed >> 8) & 0x3, packed & 0xFF))
