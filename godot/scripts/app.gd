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

@onready var drone: Node3D = $Drone
@onready var fpv_camera: Camera3D = $Drone/FpvCamera
@onready var chase_camera: Camera3D = $ChaseCamera
@onready var lens: CanvasLayer = $Lens
@onready var osd: CanvasLayer = $Osd
@onready var world: Node3D = $World
@onready var hud: CanvasLayer = $Hud
@onready var controls_menu: CanvasLayer = $ControlsMenu

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
var _osd_version := -1
var _world_version := -1


func _ready() -> void:
	settings = AppSettings.load_settings()
	_load_controls()
	controls_menu.setup(controls, CONTROLS_PATH)
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


func _load_controls() -> void:
	if FileAccess.file_exists(CONTROLS_PATH):
		var text := FileAccess.get_file_as_string(CONTROLS_PATH)
		if not controls.from_json(text):
			push_warning("%s is not a valid controls file; using the %s preset" % [CONTROLS_PATH, controls.preset_name])


func _process(delta: float) -> void:
	var focused := get_window().has_focus()
	last_sticks = sticks_override if sticks_override != null else controls.read(Input, focused, delta)
	client.set_sticks(last_sticks["roll"], last_sticks["pitch"], last_sticks["yaw"], last_sticks["throttle"], PackedFloat32Array(last_sticks["aux"]))
	if client.has_pose():
		drone.transform = client.get_pose()
	var osd_version: int = client.get_osd_version()
	if osd_version != _osd_version:
		_osd_version = osd_version
		osd.set_frame(client.get_osd())
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
		_osd_version = -1
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
		"serial_overflow":
			hud.add_toast("Betaflight UART output dropped: %s" % message, "warn")
		_:
			hud.add_toast("%s: %s" % [kind, message])


func _exit_tree() -> void:
	if client != null:
		client.stop()
