extends Node3D
## The pilot client. It connects to ofs-sim (starting it when a server binary is configured), sends the pilot's
## sticks, draws the drone from the simulator's state and shows the HUD. Keys are listed in ui/hud.gd.

const Controls = preload("res://scripts/controls.gd")
const ControlProfiles = preload("res://scripts/control_profiles.gd")
const AppSettings = preload("res://scripts/settings.gd")
const ControlsMenu = preload("res://ui/controls_menu.gd")
const MainMenu = preload("res://ui/main_menu.gd")
const FlightAudio = preload("res://drone/flight_audio.gd")
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
@onready var main_menu: CanvasLayer = $MainMenu
@onready var settings_screen: CanvasLayer = $SettingsScreen
@onready var dvr: Node = $Dvr
@onready var clean_feed: Node = $CleanFeed

var client: Object = null  # OfsClient, from the extension
var flight_audio: Node = null
var controls = Controls.new()
var profiles = ControlProfiles.new(controls)
var settings := {}
## Tests set this (a Dictionary like `Controls.read` returns) to fly without a controller.
var sticks_override = null
var radio_cut := false
var inputs_recording := false
var chase_view := false
var last_sticks := {}
var phase_kind := ""
var _link_was_lost := false
var _fpv_render_time := -1.0
var _world_version := -1


func _ready() -> void:
	settings = AppSettings.load_settings()
	settings["fpv_aspect"] = clamp_aspect(settings["fpv_aspect"])
	_load_controls()
	controls_menu.setup(profiles, CONTROLS_PATH)
	controls_menu.closed.connect(_on_controls_closed)
	video.effects = settings["video_effects"]
	controls_menu.set_video_effects(settings["video_effects"])
	controls_menu.video_effects_toggled.connect(func(on: bool) -> void: _on_setting_changed("video_effects", on))
	controls_menu.set_aspect(settings["fpv_aspect"])
	controls_menu.aspect_changed.connect(func(value: String) -> void: _on_setting_changed("fpv_aspect", value))
	controls_menu.set_dvr_export(settings["dvr_export"])
	controls_menu.dvr_export_changed.connect(func(value: String) -> void: _on_setting_changed("dvr_export", value))
	controls_menu.set_dvr_feed(settings["dvr_feed"])
	controls_menu.dvr_feed_changed.connect(func(value: String) -> void: _on_setting_changed("dvr_feed", value))
	controls_menu.dvr_toggled.connect(_set_dvr)
	var overridden := AppSettings.overrides(AppSettings.read_env(), OS.get_cmdline_user_args())
	main_menu.setup(settings, overridden)
	main_menu.choice_changed.connect(_on_setting_changed)
	main_menu.start_requested.connect(_on_menu_start)
	main_menu.controls_requested.connect(func() -> void: main_menu.close(); controls_menu.open_from_menu())
	main_menu.settings_requested.connect(func() -> void: main_menu.close(); settings_screen.open())
	main_menu.quit_requested.connect(func() -> void: get_tree().quit())
	settings_screen.setup(settings, overridden)
	settings_screen.setting_changed.connect(_on_setting_changed)
	settings_screen.closed.connect(main_menu.open)
	fpv_texture.texture = fpv_viewport.get_texture()
	clean_feed.setup(fpv_camera, lens, fpv_viewport.size)
	get_viewport().size_changed.connect(_layout_fpv)
	_layout_fpv()
	if settings["fullscreen"]:
		_set_fullscreen(true)
	if not ClassDB.class_exists("OfsClient"):
		hud.show_fatal("The OfsClient extension is not loaded.\nBuild it with `cargo build -p ofs-godot` (docs/dev-setup.md) and restart.")
		set_process(false)
		return
	client = ClassDB.instantiate("OfsClient")
	client.name = "OfsClient"
	add_child(client)
	flight_audio = FlightAudio.new()
	flight_audio.name = "FlightAudio"
	add_child(flight_audio)
	flight_audio.setup(client)
	flight_audio.set_enabled(settings["audio"])
	client.phase_changed.connect(_on_phase_changed)
	client.event_received.connect(_on_event)
	client.request_failed.connect(_on_request_failed)
	client.session_ready.connect(_on_session_ready)
	if settings["menu"]:
		_show_menu()
	else:
		_start_session()


## Starts the simulator client with the settings (plus `choices`, for what this start decides on its own).
func _start_session(choices := {}) -> void:
	var session := settings.duplicate()
	session.merge(choices, true)
	# A new session numbers its worlds from the start, and has no faults or recordings yet.
	_world_version = -1
	_fpv_render_time = -1.0
	_link_was_lost = false
	radio_cut = false
	inputs_recording = false
	hud.set_hud_visible(settings["hud_visible"])
	var error: String = client.start(AppSettings.to_client_dict(session))
	if error != "":
		push_error("OfsClient.start: " + error)
		hud.show_fatal("Could not start the simulator client: %s
Check the [client] section of user://ofs_client.cfg and the OFS_* variables, then restart." % error)
		set_process(false)
		return
	set_process(true)


## Ends the flight (the simulator and the Betaflight it started stop with it) and shows the main menu.
func _show_menu() -> void:
	if dvr.recording:
		_stop_dvr(false)
		_sync_dvr()
	client.stop()
	set_process(false)
	fpv_box.visible = false
	hud.visible = false
	main_menu.open()


func _on_menu_start() -> void:
	main_menu.close()
	fpv_box.visible = true
	hud.visible = true
	_start_session({"open_loop": MainMenu.effective_open_loop(settings)})


## The controls screen opened from the main menu closed: show the menu again.
func _on_controls_closed(from_menu: bool) -> void:
	if from_menu:
		main_menu.open()


func _in_menu() -> bool:
	return main_menu.is_open() or settings_screen.is_open() or controls_menu.is_menu_mode()


## The Picture setting arrives as a pass-through string (the config file, the environment, the command line):
## anything outside the controls menu's cycle falls back to the 4:3 default, so the label and the layout agree.
static func clamp_aspect(value: String) -> String:
	if value in ControlsMenu.ASPECTS:
		return value
	push_warning("%s is not a Picture setting; using 4:3" % value)
	return "4:3"


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


## Sizes the FPV box and the viewport's internal resolution. A recording keeps going: the DVR fits each frame to
## the size it started with.
func _layout_fpv() -> void:
	var layout: Dictionary = fpv_box_for(fpv_box.size, settings["fpv_aspect"], chase_view)
	fpv_texture.position = (layout["box"] as Rect2).position
	fpv_texture.size = (layout["box"] as Rect2).size
	fpv_viewport.size = layout["internal"]
	clean_feed.resize(layout["internal"])


func _load_controls() -> void:
	if FileAccess.file_exists(CONTROLS_PATH):
		if not profiles.from_json(FileAccess.get_file_as_string(CONTROLS_PATH)):
			push_warning("%s is not a valid controls file; using the %s preset" % [CONTROLS_PATH, controls.preset_name])
		return
	# First run: start from the preset the connected controller's name suggests.
	var pads := Input.get_connected_joypads()
	var preset := Controls.preset_for_device(Input.get_joy_name(pads[0])) if not pads.is_empty() else ""
	if preset != "":
		controls.apply_preset(preset)
		profiles.snapshot()


func _process(delta: float) -> void:
	var focused := get_window().has_focus()
	last_sticks = sticks_override if sticks_override != null else controls.read(Input, focused, delta)
	client.set_sticks(last_sticks["roll"], last_sticks["pitch"], last_sticks["yaw"], last_sticks["throttle"], PackedFloat32Array(last_sticks["aux"]))
	_poll_dvr_exports()
	flight_audio.active = client.get_phase() == "flying"
	flight_audio.pump()
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
		if clean_feed.is_active():
			clean_feed.sync_camera()
		dvr.capture(fpv)
	elif client.has_pose():
		drone.transform = client.get_pose()
	var world_version: int = client.get_world_version()
	if world_version != _world_version:
		_world_version = world_version
		var field: Dictionary = client.get_world()
		world.build(field)
		_pick_course(field)
	var telemetry: Dictionary = client.get_telemetry()
	# Free flight flies a course as scenery: no lap clock, no gate highlight.
	if not settings["race_start"] and telemetry.has("race_present"):
		telemetry["race_present"] = false
	# The gate to fly through next, from the state just read; none before there is a race, and none once it is over.
	var racing: bool = telemetry.get("race_present", false) and not telemetry.get("race_finished", false)
	world.set_course(int(telemetry.get("race_course_index", 0)))
	world.set_next_gate(int(telemetry.get("race_next_gate", 0)) if racing else -1)
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
		"recording_inputs": inputs_recording,
	})


func _unhandled_key_input(event: InputEvent) -> void:
	if not (event is InputEventKey) or not event.pressed or event.echo:
		return
	if _in_menu():
		if event.physical_keycode == KEY_F11:
			_set_fullscreen(DisplayServer.window_get_mode() != DisplayServer.WINDOW_MODE_FULLSCREEN)
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
		KEY_I:
			_toggle_inputs_recording()
		KEY_N:
			if client != null:
				if event.shift_pressed:
					_next_course()
				else:
					client.reset_race("")
		KEY_H:
			hud.toggle_hud()
		KEY_F1:
			hud.set_help_visible(not hud.help_visible())
		KEY_F2:
			controls_menu.toggle()
		KEY_F11:
			_set_fullscreen(DisplayServer.window_get_mode() != DisplayServer.WINDOW_MODE_FULLSCREEN)
		KEY_ESCAPE:
			# Help and the F2 screen close first; with neither open, Esc returns to the menu.
			var closing: bool = hud.help_visible() or controls_menu.is_open()
			hud.set_help_visible(false)
			if controls_menu.is_open() and controls_menu.listening_for() == "":
				controls_menu.toggle()
			if not closing and client != null:
				_show_menu()


func _set_fullscreen(on: bool) -> void:
	DisplayServer.window_set_mode(DisplayServer.WINDOW_MODE_FULLSCREEN if on else DisplayServer.WINDOW_MODE_WINDOWED)


func set_chase_view(chase: bool) -> void:
	chase_view = chase
	if chase and dvr != null and dvr.recording:
		_stop_dvr(true)
		_sync_dvr()
	(chase_camera if chase else fpv_camera).make_current()
	lens.set_enabled(not chase)
	osd.set_enabled(not chase)
	video.set_enabled(not chase)
	_layout_fpv()  # the chase view always fills the window


func _toggle_dvr() -> void:
	_set_dvr(not dvr.recording)


## The one start/stop path: the D key and the F2 toggle both land here. It changes the recording only when needed,
## then makes the menu checkbox and the HUD marker show the state that actually happened (a refused or failed
## start leaves both off).
func _set_dvr(on: bool) -> void:
	if on == dvr.recording:
		_sync_dvr()
		return
	if on:
		if chase_view:
			hud.add_toast("DVR records the FPV view - switch with C first", "warn")
			_sync_dvr()
			return
		if client == null:
			hud.add_toast("DVR failed: the simulator is not connected", "error")
			_sync_dvr()
			return
		var feed: String = settings.get("dvr_feed", "goggles")
		var error: String = dvr.start(client, fpv_viewport, feed, clean_feed.viewport())
		if error != "":
			hud.add_toast("DVR failed: %s" % error, "error")
			_sync_dvr()
			return
		clean_feed.set_active(feed != "goggles")  # the clean viewport renders only while a clean file is written
		hud.add_toast("DVR: recording to user://recordings")
	else:
		_stop_dvr(false)
	_sync_dvr()


## The one stop path (D, the F2 box, entering the chase view and a lost or reloaded session): idles the clean feed,
## stops the recording, toasts how it went and, when it stopped normally with frames, hands each recorded file to the
## export the `dvr_export` setting asks for (every feed holds the same number of frames, so none is empty).
func _stop_dvr(chase: bool) -> void:
	clean_feed.set_active(false)
	var summary: Dictionary = dvr.stop()
	if summary.is_empty():
		hud.add_toast("DVR failed: the writer stopped abnormally%s" % (" (the chase camera is not recorded)" if chase else ""), "error")
		return
	var saved := "%d frames saved" % summary.get("frames", 0)
	if chase:
		hud.add_toast("DVR stopped: the chase camera is not recorded (%s)%s" % [saved, _dvr_drop_note(summary)], "warn")
	else:
		hud.add_toast("DVR: %s%s" % [saved, _dvr_drop_note(summary)])
	var mode: String = settings.get("dvr_export", "none")
	if mode == "none" or client == null or int(summary.get("frames", 0)) == 0:
		return  # nothing to export from an empty recording
	for path in summary.get("paths", PackedStringArray()):
		var error: String = client.dvr_export(path, mode)
		if error != "":
			hud.add_toast("DVR export failed: %s" % error, "warn")


## Toasts the exports that finished since the last frame. A failure only warns: the AVI is never touched.
func _poll_dvr_exports() -> void:
	for done in client.dvr_export_poll():
		var detail: String = done["detail"]
		if done["ok"]:
			hud.add_toast("DVR exported (%s): %s" % [done["kind"], detail.get_file()])
		elif detail.begins_with("ffmpeg was not found"):
			hud.add_toast("%s: the AVI was kept" % detail, "warn")
		else:
			hud.add_toast("DVR export failed (%s): %s (the AVI was kept)" % [done["kind"], detail], "warn")


func _sync_dvr() -> void:
	controls_menu.set_dvr(dvr.recording)
	hud.set_dvr_recording(dvr.recording)


func _dvr_drop_note(summary: Dictionary) -> String:
	var dropped: int = summary.get("dropped_frames", 0)
	return "" if dropped == 0 else " (%d dropped)" % dropped


## The one path for a setting changed in the game (the F2 screen, the menu or the settings screen): remember it, apply it
## where the client can at once, show it on the F2 screen and save it to user://ofs_client.cfg. A new `dvr_feed` applies
## to the next recording; the rest of the settings (server, open loop, ...) to the next start.
func _on_setting_changed(key: String, value) -> void:
	settings[key] = value
	match key:
		"video_effects":
			video.effects = value
			controls_menu.set_video_effects(value)
		"fpv_aspect":
			_layout_fpv()
			controls_menu.set_aspect(value)
		"dvr_export":
			controls_menu.set_dvr_export(value)
		"dvr_feed":
			controls_menu.set_dvr_feed(value)
		"fullscreen":
			_set_fullscreen(value)
		"hud_visible":
			hud.set_hud_visible(value)
		"audio":
			if flight_audio != null:
				flight_audio.set_enabled(value)
	if not AppSettings.save_value(key, value):
		hud.add_toast("Could not save the %s setting to %s" % [key, AppSettings.CONFIG_PATH], "warn")


func set_radio_cut(cut: bool) -> void:
	radio_cut = cut
	if client != null:
		client.set_radio_loss(cut)


func _toggle_inputs_recording() -> void:
	if client == null:
		return
	# The state flips when the server's event arrives, so the indicator is never ahead of the file.
	client.set_inputs_recording(not inputs_recording)


func _on_phase_changed(phase: String, detail: String, kind: String) -> void:
	phase_kind = kind
	if phase in ["loading", "failed", "stopped"]:
		inputs_recording = false  # a reloaded quad, or a lost session, has no recorder
		if dvr != null and dvr.recording:
			_stop_dvr(false)  # a reloaded quad, or a lost session, has no picture to record
			_sync_dvr()
	if phase == "loading":
		radio_cut = false  # a reloaded quad has no faults
		_link_was_lost = false
		osd.set_frame({})
		_fpv_render_time = -1.0
	if phase == "failed":
		hud.add_toast("Failed: %s" % detail.get_slice("\n", 0), "error")


## A new session starts on the world's default course: if the pilot chose another one (the menu, or Shift+N earlier) and
## this world has it, move there.
func _pick_course(field: Dictionary) -> void:
	var chosen: String = settings["course"]
	var courses: Array = field.get("courses", [])
	if chosen == "" or not courses.any(func(c): return c["name"] == chosen):
		return
	if courses[int(field.get("default_course", 0))]["name"] != chosen:
		client.reset_race(chosen)


## Shift+N: the next course of the world (all of them in turn), restarted. A world with fewer than two does nothing.
func _next_course() -> void:
	var courses: Array = client.get_world().get("courses", [])
	if courses.size() < 2:
		hud.add_toast("This world has a single course")
		return
	var current := int(client.get_telemetry().get("race_course_index", 0))
	var next: String = courses[(current + 1) % courses.size()]["name"]
	_on_setting_changed("course", next)
	client.reset_race(next)


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
		"inputs_recording_started":
			inputs_recording = true
			hud.add_toast("Recording the sticks to %s" % message.get_file())
		"inputs_recording_stopped":
			inputs_recording = false
			hud.add_toast("Sticks recording stopped: %s" % message.get_file())
		"vtx_changed":
			hud.add_toast("VTX: %s" % message)
		"collision":
			hud.add_toast(message, "warn")
		"gate_passed", "lap_completed":
			hud.add_toast(message)
		"gate_hit":
			hud.add_toast(message, "warn")
		"race_finished", "race_reset":
			hud.add_toast(message)
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
