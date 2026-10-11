extends Node3D
## The pilot client. It connects to ofs-sim (starting it when a server binary is configured), sends the pilot's
## sticks, draws the drone from the simulator's state and shows the HUD. Keys are listed in ui/hud.gd.

const Controls = preload("res://scripts/controls.gd")
const AppSettings = preload("res://scripts/settings.gd")
const CONTROLS_PATH := "user://controls.json"

## What to try, by error kind, shown under the error message.
const TIPS := {
	"unavailable": "Is ofs-sim running? The game starts it itself when the `server_bin` setting points at it (docs/dev-setup.md).",
	"launch": "Build the server (cargo build -p ofs-sim) and set OFS_SIM_BIN, or check that its port is free.",
	"firmware": "Betaflight SITL did not start. Set OFS_SITL_LAUNCH (docs/dev-setup.md), or add `-- --open-loop` to fly without firmware.",
	"config": "The quad file is missing or wrong (setting quad_path, or OFS_QUAD).",
	"pilot_busy": "Another pilot (a script or a second game) is connected to this server.",
	"protocol": "The server and this game are different versions: rebuild both.",
	"not_loaded": "The session ended on the server.",
}

@onready var fpv_viewport: SubViewport = $FpvViewport
@onready var drone: Node3D = $FpvViewport/Drone
@onready var fpv_camera: Camera3D = $FpvViewport/Drone/FpvCamera
@onready var chase_camera: Camera3D = $FpvViewport/ChaseCamera
@onready var lens: CanvasLayer = $FpvViewport/Lens
@onready var osd: CanvasLayer = $FpvViewport/Osd
@onready var video: CanvasLayer = $FpvViewport/Video
@onready var world: Node3D = $FpvViewport/World
@onready var fpv_box: Control = $FpvBox
@onready var fpv_texture: TextureRect = $FpvBox/FpvTexture
@onready var hud: CanvasLayer = $Hud
@onready var controls_menu: CanvasLayer = $ControlsMenu
@onready var dvr: Node = $Dvr

var client: Object = null  # OfsClient, from the extension
var controls = Controls.new()
var settings := {}
## Tests set this (a Dictionary like `Controls.read` returns) to fly without a controller.
var sticks_override = null
var radio_cut := false
var chase_view := false
var last_sticks := {}
var phase_kind := ""
var _link_was_lost := false
var _fpv_render_time := -1.0
var _world_version := -1


func _ready() -> void:
	settings = AppSettings.load_settings()
	_load_controls()
	controls_menu.setup(controls, CONTROLS_PATH)
	video.effects = settings["video_effects"]
	controls_menu.set_video_effects(settings["video_effects"])
	controls_menu.video_effects_toggled.connect(_on_video_effects_toggled)
	controls_menu.set_aspect(settings["fpv_aspect"])
	controls_menu.aspect_changed.connect(_on_aspect_changed)
	fpv_texture.texture = fpv_viewport.get_texture()
	get_viewport().size_changed.connect(_layout_fpv)
	_layout_fpv()
	if not ClassDB.class_exists("OfsClient"):
		hud.show_fatal("The OfsClient extension is not loaded.\nBuild it with `cargo build -p ofs-godot` (docs/dev-setup.md) and restart.")
		set_process(false)
		return
	client = ClassDB.instantiate("OfsClient")
	client.name = "OfsClient"
	add_child(client)
	client.phase_changed.connect(_on_phase_changed)
	client.event_received.connect(_on_event)
	client.request_failed.connect(_on_request_failed)
	client.session_ready.connect(_on_session_ready)
	var error: String = client.start(AppSettings.to_client_dict(settings))
	if error != "":
		push_error("OfsClient.start: " + error)
		hud.show_fatal("Could not start the simulator client: %s
Check the [client] section of user://ofs_client.cfg and the OFS_* variables, then restart." % error)
		set_process(false)


## The FPV box and the viewport's internal resolution for `window_size`. The chase view and the "window"
## aspect fill the window (the viewport at the window's size, today's quality); the boxed aspects center a
## fixed-resolution picture like goggles do.
static func fpv_box_for(window_size: Vector2, aspect: String, chase: bool) -> Dictionary:
	var box := Rect2(Vector2.ZERO, window_size)
	var internal := Vector2i(window_size)
	if not chase and aspect != "window":
		var ratio := 4.0 / 3.0 if aspect == "4:3" else 16.0 / 9.0
		var w: float = minf(window_size.x, window_size.y * ratio)
		box = Rect2((window_size - Vector2(w, w / ratio)) * 0.5, Vector2(w, w / ratio))
		internal = Vector2i(960, 720) if aspect == "4:3" else Vector2i(1280, 720)
	return {"box": box, "internal": internal}


## Sizes the FPV box and the viewport's internal resolution. A size change while the DVR records stops it
## (Review Focus 5): stop before the resize so the comparison sees the old size.
func _layout_fpv() -> void:
	var layout: Dictionary = fpv_box_for(fpv_box.size, settings["fpv_aspect"], chase_view)
	var internal: Vector2i = layout["internal"]
	if dvr != null and dvr.recording and fpv_viewport.size != internal:
		var summary: Dictionary = dvr.stop()
		hud.add_toast("DVR stopped: the picture size changed (%d frames saved)" % summary.get("frames", 0), "warn")
	fpv_texture.position = (layout["box"] as Rect2).position
	fpv_texture.size = (layout["box"] as Rect2).size
	fpv_viewport.size = internal


func _load_controls() -> void:
	if FileAccess.file_exists(CONTROLS_PATH):
		var text := FileAccess.get_file_as_string(CONTROLS_PATH)
		if not controls.from_json(text):
			push_warning("%s is not a valid controls file; using the %s preset" % [CONTROLS_PATH, controls.preset_name])


func _process(delta: float) -> void:
	var focused := get_window().has_focus()
	last_sticks = sticks_override if sticks_override != null else controls.read(Input, focused, delta)
	client.set_sticks(last_sticks["roll"], last_sticks["pitch"], last_sticks["yaw"], last_sticks["throttle"], PackedFloat32Array(last_sticks["aux"]))
	if not chase_view:
		var fpv: Dictionary = client.get_fpv_view()
		if fpv.has("pose"):
			drone.transform = fpv["pose"]
			video.update_view({
				"video_present": fpv["video_present"], "video_noise": fpv["video_noise"],
				"video_sparkles": fpv["video_sparkles"], "video_chroma": fpv["video_chroma"],
				"video_sync": fpv["video_sync"],
			}, delta)
		var render_time: float = fpv.get("render_time_s", -1.0)
		if render_time != _fpv_render_time:
			_fpv_render_time = render_time
			osd.set_frame(fpv.get("osd", {}))
		dvr.capture(fpv)
	elif client.has_pose():
		drone.transform = client.get_pose()
	var world_version: int = client.get_world_version()
	if world_version != _world_version:
		_world_version = world_version
		world.build(client.get_world())
	var telemetry: Dictionary = client.get_telemetry()
	if telemetry.has("motor_cmd"):
		drone.set_motors(telemetry["motor_cmd"], delta)
	var detail: String = client.get_phase_detail()
	if client.get_phase() == "failed" and TIPS.has(phase_kind):
		detail += "\n" + TIPS[phase_kind]
	controls_menu.update_live(last_sticks)
	hud.update_view({
		"phase": client.get_phase(), "detail": detail, "telemetry": telemetry, "sticks": last_sticks,
		"controls_status": last_sticks.get("status", ""), "configurator": client.get_configurator_address(),
		"quad": client.get_quad_name(), "camera": "Chase" if chase_view else "FPV", "radio_cut": radio_cut,
	})


func _unhandled_key_input(event: InputEvent) -> void:
	if not (event is InputEventKey) or not event.pressed or event.echo:
		return
	match event.physical_keycode:
		KEY_P:
			if client != null:
				client.resume() if client.get_phase() == "paused" else client.pause()
		KEY_R:
			if client != null:
				client.reload()
		KEY_C:
			set_chase_view(not chase_view)
		KEY_D:
			_toggle_dvr()
		KEY_K:
			set_radio_cut(not radio_cut)
		KEY_H:
			hud.toggle_hud()
		KEY_F1:
			hud.set_help_visible(not hud.help_visible())
		KEY_F2:
			controls_menu.toggle()
		KEY_F11:
			var mode := DisplayServer.window_get_mode()
			DisplayServer.window_set_mode(DisplayServer.WINDOW_MODE_WINDOWED if mode == DisplayServer.WINDOW_MODE_FULLSCREEN else DisplayServer.WINDOW_MODE_FULLSCREEN)
		KEY_ESCAPE:
			hud.set_help_visible(false)
			if controls_menu.is_open() and controls_menu.listening_for() == "":
				controls_menu.toggle()


func set_chase_view(chase: bool) -> void:
	chase_view = chase
	(chase_camera if chase else fpv_camera).make_current()
	lens.set_enabled(not chase)
	osd.set_enabled(not chase)
	video.set_enabled(not chase)
	_layout_fpv()  # the chase view always fills the window


func _toggle_dvr() -> void:
	if chase_view:
		hud.add_toast("DVR records the FPV view - switch with C first", "warn")
		return
	if dvr.recording:
		var summary: Dictionary = dvr.stop()
		if summary.is_empty():
			hud.add_toast("DVR failed: the writer stopped abnormally", "error")
		else:
			hud.add_toast("DVR: %d frames saved%s" % [summary.get("frames", 0), _dvr_drop_note(summary)])
	else:
		var error: String = dvr.start(client, fpv_viewport)
		hud.add_toast("DVR: recording to user://recordings" if error == "" else "DVR failed: %s" % error, "info" if error == "" else "error")
		if error != "":
			return


func _dvr_drop_note(summary: Dictionary) -> String:
	var dropped: int = summary.get("dropped_frames", 0)
	return "" if dropped == 0 else " (%d dropped)" % dropped


func _on_video_effects_toggled(on: bool) -> void:
	video.effects = on
	settings["video_effects"] = on
	if not AppSettings.save_value("video_effects", on):
		hud.add_toast("Could not save the Video effects setting to %s" % AppSettings.CONFIG_PATH, "warn")


func _on_aspect_changed(value: String) -> void:
	settings["fpv_aspect"] = value
	_layout_fpv()
	if not AppSettings.save_value("fpv_aspect", value):
		hud.add_toast("Could not save the Picture setting to %s" % AppSettings.CONFIG_PATH, "warn")


func set_radio_cut(cut: bool) -> void:
	radio_cut = cut
	if client != null:
		client.set_radio_loss(cut)


func _on_phase_changed(phase: String, detail: String, kind: String) -> void:
	phase_kind = kind
	if phase == "loading":
		radio_cut = false  # a reloaded quad has no faults
		_link_was_lost = false
		osd.set_frame({})
		_fpv_render_time = -1.0
	if phase == "failed":
		hud.add_toast("Failed: %s" % detail.get_slice("\n", 0), "error")


func _on_session_ready(quad_name: String, configurator_address: String) -> void:
	hud.add_toast("Loaded %s" % quad_name)
	if configurator_address != "":
		hud.add_toast("Betaflight Configurator: %s" % configurator_address)


func _on_request_failed(kind: String, message: String) -> void:
	hud.add_toast("%s: %s" % [kind, message], "warn")


func _on_event(kind: String, message: String, _time_s: float) -> void:
	match kind:
		"link_down":
			_link_was_lost = true
			hud.add_toast("Radio link lost - Betaflight fails safe", "warn")
		"link_up":
			hud.add_toast("Radio link restored" if _link_was_lost else "Radio link up")
			_link_was_lost = false
		"overrun":
			hud.add_toast("Real-time overrun: %s" % message, "warn")
		"firmware_restarted":
			hud.add_toast("Betaflight restarted: %s" % message)
		"sim_error":
			hud.add_toast("Simulator error: %s" % message, "error")
		"session_ended":
			hud.add_toast("Session ended: %s" % message, "warn")
		"pilot_connected", "pilot_disconnected":
			hud.add_toast(message)
		"vtx_changed":
			hud.add_toast("VTX: %s" % message)
		"collision":
			hud.add_toast(message, "warn")
		"video_lost":
			hud.add_toast("Video lost - %s" % message.trim_prefix("video lost: "), "warn")
		"video_restored":
			hud.add_toast("Video back - %s" % message.trim_prefix("video restored: "))
		"serial_overflow":
			hud.add_toast("Betaflight UART output dropped: %s" % message, "warn")
		_:
			hud.add_toast("%s: %s" % [kind, message])


func _exit_tree() -> void:
	if client != null:
		client.stop()
