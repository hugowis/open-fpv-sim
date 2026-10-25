extends CanvasLayer
## The main menu: pick a world, a quad, race or free flight and the flight controller, then start; or open the
## settings, or quit. The choices are settings (`world_path`, `quad_path`, `race_start`, `open_loop`): the game applies
## and saves each change (`choice_changed`), so they come back next time. A choice a flag or an OFS_* variable sets
## this run is shown and locked, like on the settings screen.

const AppSettings = preload("res://scripts/settings.gd")
const Catalog = preload("res://scripts/catalog.gd")
const CycleButton = preload("res://ui/cycle_button.gd")

## The pilot changed `key` (`world_path`, `quad_path`, `race_start` or `open_loop`) to `value`.
signal choice_changed(key: String, value)

## Fly the chosen settings.
signal start_requested

signal settings_requested
signal quit_requested

var settings := {}  # the game's live settings
var locks := {}  # key -> "--flag" or "OFS_NAME"

var _worlds: Array = []
var _quads: Array = []
var _start := Button.new()
var _settings_button := Button.new()
var _quit := Button.new()
var _world := CycleButton.new()
var _quad := CycleButton.new()
var _mode := CycleButton.new()
var _controller := CycleButton.new()
var _hint := Label.new()


func _ready() -> void:
	visible = false
	var back := ColorRect.new()
	back.color = Color(0.06, 0.07, 0.09)
	back.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(back)
	var box := VBoxContainer.new()
	box.set_anchors_preset(Control.PRESET_CENTER)
	box.grow_horizontal = Control.GROW_DIRECTION_BOTH
	box.grow_vertical = Control.GROW_DIRECTION_BOTH
	box.custom_minimum_size.x = 460
	box.add_theme_constant_override("separation", 10)
	add_child(box)
	var title := Label.new()
	title.text = "Open FPV Sim"
	title.add_theme_font_size_override("font_size", 40)
	title.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	box.add_child(title)
	_start.text = "Start flying"
	_start.pressed.connect(func() -> void: start_requested.emit())
	for node in [_start, _world, _quad, _mode, _controller, _settings_button, _quit]:
		box.add_child(node)
	_settings_button.text = "Settings"
	_settings_button.pressed.connect(func() -> void: settings_requested.emit())
	_quit.text = "Quit"
	_quit.pressed.connect(func() -> void: quit_requested.emit())
	_world.changed.connect(_on_world)
	_quad.changed.connect(func(path) -> void: choice_changed.emit("quad_path", path))
	_mode.changed.connect(func(race) -> void: choice_changed.emit("race_start", race))
	_controller.changed.connect(func(open_loop) -> void: choice_changed.emit("open_loop", open_loop))
	_hint.modulate = Color(1, 1, 1, 0.6)
	_hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	box.add_child(_hint)
	var keys := Label.new()
	keys.text = "Arrows or d-pad to move, Enter or A to choose, left/right to change a choice. Esc in a flight comes back here."
	keys.modulate = Color(1, 1, 1, 0.4)
	keys.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	box.add_child(keys)
	_refresh()


## Hands the menu the game's live settings and what overrides them this run.
func setup(live_settings: Dictionary, overridden: Dictionary) -> void:
	settings = live_settings
	locks = overridden
	_refresh()


## Shows the menu with the files on disk now and the saved choices.
func open() -> void:
	_refresh()
	visible = true
	_start.grab_focus()


func close() -> void:
	visible = false


func is_open() -> bool:
	return visible


## What the session flies: Betaflight needs a SITL launch command, so without one the flight is open loop.
static func effective_open_loop(s: Dictionary) -> bool:
	return s["open_loop"] or s["sitl_launch"] == ""


## The row for `name` ("world", "quad", "mode", "controller", "start", "settings", "quit"), for tests.
func row(name: String) -> Control:
	return {"world": _world, "quad": _quad, "mode": _mode, "controller": _controller, "start": _start,
		"settings": _settings_button, "quit": _quit}[name]


func hint() -> String:
	return _hint.text


func _refresh() -> void:
	if settings.is_empty() or not is_node_ready():
		return
	_worlds = Catalog.with_current(Catalog.list(Catalog.roots(settings, "worlds")), settings["world_path"])
	_quads = Catalog.with_current(Catalog.list(Catalog.roots(settings, "quads")), settings["quad_path"])
	_fill(_world, "World", "world_path", _worlds)
	_fill(_quad, "Quad", "quad_path", _quads)
	var no_firmware: bool = settings["sitl_launch"] == ""
	_controller.setup(_title("Flight controller", "open_loop"), [true] if no_firmware else [false, true],
		["Open loop"] if no_firmware else ["Betaflight", "Open loop"])
	_controller.set_value(effective_open_loop(settings))
	_controller.disabled = locks.has("open_loop") or no_firmware
	_hint.text = "Betaflight needs a SITL launch command: set it in Settings." if no_firmware else ""
	_refresh_mode()


func _fill(button: CycleButton, label: String, key: String, items: Array) -> void:
	button.setup(_title(label, key), items.map(func(i): return i["path"]), items.map(func(i): return i["label"]))
	button.set_value(settings[key].simplify_path())
	button.disabled = locks.has(key)


## "World (--world)" for a locked row.
func _title(label: String, key: String) -> String:
	return "%s (%s)" % [label, locks[key]] if locks.has(key) else label


## Race or free flight, offered only for a world that declares a course.
func _refresh_mode() -> void:
	_mode.visible = _has_course(settings["world_path"])
	_mode.setup(_title("Flight mode", "race_start"), [true, false], ["Race", "Free flight"])
	_mode.set_value(settings["race_start"])
	_mode.disabled = locks.has("race_start")


func _has_course(world_path: String) -> bool:
	return not _worlds.is_empty() and _worlds[Catalog.index_of(_worlds, world_path)]["gates"] > 0


func _on_world(path: String) -> void:
	choice_changed.emit("world_path", path)
	_mode.visible = _has_course(path)
