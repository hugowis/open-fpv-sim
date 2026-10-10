extends CanvasLayer
## The heads-up display: simulator status, radio link, battery, flight numbers, the sticks being sent, a banner
## for what needs attention, and short-lived toasts for events.
##
## `update_view` is called every frame with a Dictionary (see its comment); everything else is built here in code.

const SticksView = preload("res://ui/sticks_view.gd")

const GREEN := Color(0.45, 0.95, 0.5)
const YELLOW := Color(1.0, 0.85, 0.3)
const RED := Color(1.0, 0.38, 0.32)
const WHITE := Color(0.95, 0.97, 1.0)
const TOAST_SECONDS := 6.0
const MAX_TOASTS := 6
## No state for this long means the simulator stopped talking.
const STALE_SECONDS := 0.5

const HELP_LINES := [
	"Open FPV Sim",
	"P  pause / resume the simulation",
	"R  reload the quad (back to the start; retry after an error)",
	"C  switch between FPV and chase camera",
	"K  cut / restore the radio link (failsafe test)",
	"H  show / hide this HUD",
	"F1 this help        F2 controls setup        F11 fullscreen",
	"Arm with the aux 1 switch; aux 2 selects Angle mode.",
]

var _status := Label.new()
var _sim := Label.new()
var _link := Label.new()
var _battery := Label.new()
var _vtx := Label.new()
var _video := Label.new()
var _flight := Label.new()
var _configurator := Label.new()
var _banner := Label.new()
var _help := Label.new()
var _toasts := VBoxContainer.new()
var _sticks_view: Control = SticksView.new()
var _toast_items: Array = []  # of {label, age}
var _fatal := ""
var _shown := true


func _ready() -> void:
	var root := Control.new()
	root.name = "HudRoot"
	root.set_anchors_preset(Control.PRESET_FULL_RECT)
	root.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(root)
	_place(root, _status, 0.0, 0.0, 14.0, 14.0, false)
	_place(root, _sim, 0.0, 0.0, 14.0, 44.0, false)
	_place(root, _link, 1.0, 0.0, 14.0, 14.0, true)
	_place(root, _battery, 1.0, 0.0, 14.0, 44.0, true)
	_place(root, _vtx, 1.0, 0.0, 14.0, 74.0, true)
	_place(root, _video, 1.0, 0.0, 14.0, 104.0, true)
	_place(root, _flight, 0.0, 1.0, 14.0, 14.0, false)
	_place(root, _configurator, 1.0, 1.0, 14.0, 14.0, true)
	_place(root, _toasts, 1.0, 0.5, 14.0, 0.0, true)
	_place(root, _sticks_view, 0.5, 1.0, 0.0, 14.0, false)
	_sticks_view.grow_horizontal = Control.GROW_DIRECTION_BOTH
	_banner.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_banner.add_theme_font_size_override("font_size", 30)
	_banner.add_theme_color_override("font_outline_color", Color.BLACK)
	_banner.add_theme_constant_override("outline_size", 8)
	_banner.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_banner.custom_minimum_size = Vector2(900, 0)
	_place(root, _banner, 0.5, 0.3, 0.0, 0.0, false)
	_banner.grow_horizontal = Control.GROW_DIRECTION_BOTH
	_help.text = "\n".join(HELP_LINES)
	_help.visible = false
	_help.add_theme_font_size_override("font_size", 18)
	_help.add_theme_color_override("font_outline_color", Color.BLACK)
	_help.add_theme_constant_override("outline_size", 6)
	_place(root, _help, 0.5, 0.5, 0.0, 0.0, false)
	_help.grow_horizontal = Control.GROW_DIRECTION_BOTH
	_help.grow_vertical = Control.GROW_DIRECTION_BOTH
	for label in [_status, _sim, _link, _battery, _vtx, _video, _flight, _configurator]:
		_style(label, 18)


func _process(delta: float) -> void:
	for item in _toast_items.duplicate():
		item["age"] += delta
		var left: float = TOAST_SECONDS - item["age"]
		if left <= 0.0:
			item["label"].queue_free()
			_toast_items.erase(item)
		else:
			item["label"].modulate.a = clampf(left, 0.0, 1.0)


## Places `control` at a fraction of the screen (`ax`, `ay`) with a margin; right and bottom anchored controls
## grow leftwards and upwards so their text stays on the screen.
func _place(parent: Control, control: Control, ax: float, ay: float, margin_x: float, margin_y: float, right_aligned: bool) -> void:
	parent.add_child(control)
	control.anchor_left = ax
	control.anchor_right = ax
	control.anchor_top = ay
	control.anchor_bottom = ay
	var mx := -margin_x if ax >= 1.0 else margin_x
	var my := -margin_y if ay >= 1.0 else margin_y
	control.offset_left = mx
	control.offset_right = mx
	control.offset_top = my
	control.offset_bottom = my
	control.grow_horizontal = Control.GROW_DIRECTION_BEGIN if ax >= 1.0 else Control.GROW_DIRECTION_END
	control.grow_vertical = Control.GROW_DIRECTION_BEGIN if ay >= 1.0 else Control.GROW_DIRECTION_END
	if control is Label and right_aligned:
		control.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	control.mouse_filter = Control.MOUSE_FILTER_IGNORE


func _style(label: Label, size: int) -> void:
	label.add_theme_font_size_override("font_size", size)
	label.add_theme_color_override("font_color", WHITE)
	label.add_theme_color_override("font_outline_color", Color.BLACK)
	label.add_theme_constant_override("outline_size", 5)


func set_help_visible(visible_now: bool) -> void:
	_help.visible = visible_now


func help_visible() -> bool:
	return _help.visible


func set_hud_visible(visible_now: bool) -> void:
	_shown = visible_now
	for node in [_status, _sim, _link, _battery, _vtx, _video, _flight, _configurator, _sticks_view, _toasts]:
		node.visible = visible_now


func toggle_hud() -> void:
	set_hud_visible(not _shown)


func hud_visible() -> bool:
	return _shown


## A message that stays until the next `update_view` without it, for failures that stop everything (the extension
## is missing).
func show_fatal(text: String) -> void:
	_fatal = text
	_banner.text = text
	_banner.add_theme_color_override("font_color", RED)


func add_toast(text: String, level: String = "info") -> void:
	var label := Label.new()
	label.text = text
	_style(label, 18)
	label.add_theme_color_override("font_color", {"info": WHITE, "warn": YELLOW, "error": RED}.get(level, WHITE))
	label.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	_toasts.add_child(label)
	_toast_items.append({"label": label, "age": 0.0})
	while _toast_items.size() > MAX_TOASTS:
		var oldest: Dictionary = _toast_items.pop_front()
		oldest["label"].queue_free()


## The toasts on screen, oldest first (for the tests).
func toast_texts() -> PackedStringArray:
	var texts := PackedStringArray()
	for item in _toast_items:
		texts.append(item["label"].text)
	return texts


func banner_text() -> String:
	return _banner.text


func vtx_text() -> String:
	return _vtx.text


func video_text() -> String:
	return _video.text


## The radio link line (for the tests).
func link_text() -> String:
	return _link.text


## view: {phase, detail, telemetry: Dictionary (empty before the first state), sticks: Dictionary from Controls.read,
## controls_status, configurator, quad, camera, radio_cut}
func update_view(view: Dictionary) -> void:
	var t: Dictionary = view.get("telemetry", {})
	var phase: String = view.get("phase", "")
	_status.text = "%s  |  %s camera" % [view.get("quad", "") if view.get("quad", "") != "" else "Open FPV Sim", view.get("camera", "FPV")]
	if t.is_empty():
		_sim.text = phase.capitalize()
		_link.text = ""
		_battery.text = ""
		_vtx.text = ""
		_video.text = ""
		_flight.text = ""
	else:
		var running: bool = t["running"]
		_sim.text = "t = %.1f s   %s   overruns %d%s" % [
			t["time_s"], "REAL TIME" if running else "PAUSED", t["overruns"],
			"   Betaflight restarts %d" % t["fc_restarts"] if t["fc_restarts"] > 0 else ""]
		_sim.add_theme_color_override("font_color", WHITE if running else YELLOW)
		var up: bool = t["link_up"] and t["tx_enabled"]
		_link.text = "LINK %s   LQ %d %%   %d dBm   SNR %d" % ["UP" if up else "DOWN", t["lq_pct"], t["rssi_dbm"], roundi(t.get("radio_snr_db", 0.0))]
		_link.add_theme_color_override("font_color", GREEN if up and t["lq_pct"] >= 80.0 else (YELLOW if up else RED))
		_battery.text = "%.2f V   %.1f A" % [t["battery_voltage_v"], t["battery_current_a"]]
		if t.get("vtx_present", false):
			var channel: String = t.get("vtx_channel_name", "")
			_vtx.text = "VTX %s  %d MHz  %d mW%s" % [channel if channel != "" else "user", t["vtx_freq_mhz"], t["vtx_power_mw"], "  PIT" if t["vtx_pit_mode"] else ""]
		else:
			_vtx.text = ""
		_update_video(t)
		_flight.text = "ALT   %.1f m
SPEED %.1f m/s
CLIMB %+.1f m/s
MOTORS %s" % [
			t["altitude_m"], t["speed_mps"], t["climb_mps"], "ON" if t["motors_spinning"] else "off"]
	var sticks: Dictionary = view.get("sticks", {})
	if not sticks.is_empty():
		_sticks_view.sticks = sticks
	var address: String = view.get("configurator", "")
	_configurator.text = "Betaflight Configurator: %s" % address if address != "" else ""
	_update_banner(view, t, phase)


## "VID 18 dB  patch  -71 dBm": the goggles' signal, the antenna in use and its power; green when the picture is clean,
## yellow while it degrades, red without sync.
func _update_video(t: Dictionary) -> void:
	if not t.get("video_present", false):
		_video.text = ""
		return
	var antenna: String = t.get("video_antenna", "")
	var rssi: Dictionary = t.get("video_rssi", {})
	var sync: String = t.get("video_sync", "")
	_video.text = "VID %d dB  %s  %d dBm%s" % [
		roundi(t["video_snr_db"]), antenna, roundi(rssi.get(antenna, 0.0)), "  NO SYNC" if sync == "lost" else ""]
	var clean: bool = t.get("video_noise", 0.0) <= 0.0 and sync == "locked"
	_video.add_theme_color_override("font_color", GREEN if clean else (RED if sync == "lost" else YELLOW))


func _update_banner(view: Dictionary, t: Dictionary, phase: String) -> void:
	if _fatal != "":
		return
	var text := ""
	var color := YELLOW
	if phase == "failed":
		text = "%s\nPress R to retry." % view.get("detail", "")
		color = RED
	elif phase == "connecting" or phase == "loading":
		text = view.get("detail", "")
		if text == "":
			text = phase.capitalize() + "..."
	elif phase == "stopped":
		text = "Disconnected"
		color = RED
	elif not t.is_empty() and t["age_s"] > STALE_SECONDS:
		text = "NO DATA from the simulator (%.0f s)" % t["age_s"]
		color = RED
	elif view.get("radio_cut", false):
		text = "RADIO LINK CUT (K restores it)"
		color = RED
	elif not t.is_empty() and not t["link_up"] and t["tx_enabled"]:
		text = "RADIO LINK LOST - failsafe"
		color = RED
	elif view.get("controls_status", "") != "":
		text = view["controls_status"]
	elif phase == "paused":
		text = "PAUSED (P resumes)"
	_banner.text = text
	_banner.add_theme_color_override("font_color", color)
