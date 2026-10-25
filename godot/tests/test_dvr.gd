extends "res://tests/testing.gd"
## The DVR's file side and capture timing: the client's dvr_* API writes a parseable MJPEG AVI, and the
## boundary dedup and held-frame freeze are checked against a stand-in client (headless has no renderer, so
## the encode path itself is checked by eye and by shots.gd; test_dvr.gd drives the bytes and the timing).

const Dvr = preload("res://ui/dvr.gd")
const App = preload("res://scripts/app.gd")
const Menu = preload("res://ui/controls_menu.gd")
const Hud = preload("res://ui/hud.gd")
const CleanFeed = preload("res://ui/clean_feed.gd")
const Lens = preload("res://ui/lens.gd")
const QUAD := "res://../quads/opendrone-5f-freestyle.toml"


## A stand-in for OfsClient: records what the DVR feeds it.
class MockClient:
	var frames: Array = []
	var starts := 0

	func dvr_start(_feed: String, _path: String, _width: int, _height: int) -> String:
		starts += 1
		return ""

	func dvr_write_frame(_feed: String, jpeg: PackedByteArray) -> void:
		frames.append(jpeg)

	var fail_stop := false  # the writer stopped abnormally: dvr_stop answers with an empty Dictionary

	func dvr_stop(_feed: String) -> Dictionary:
		if fail_stop:
			return {}
		return {"frames": frames.size(), "dropped_frames": 0, "bytes": 0}

	var exports: Array = []  # [path, mode] per dvr_export call
	var export_results: Array = []

	func dvr_export(path: String, mode: String) -> String:
		exports.append([path, mode])
		return ""

	func dvr_export_poll() -> Array:
		var out := export_results
		export_results = []
		return out


class MockOsd extends CanvasLayer:
	func set_frame(_frame: Dictionary) -> void:
		pass

	func set_enabled(_enabled: bool) -> void:
		pass


## The app's DVR coordination, without its `_ready` (which would start a session): the real menu, HUD and DVR, and
## a stand-in client. The menu signal is wired as `_ready` does it.
func _app_shell(client = MockClient.new()) -> Dictionary:
	var app := App.new()
	var dvr := Dvr.new()
	var menu := Menu.new()
	var hud := Hud.new()
	var viewport := SubViewport.new()
	var box := Control.new()
	var texture := TextureRect.new()
	var osd := MockOsd.new()
	await add_to_tree(dvr)
	await add_to_tree(menu)
	await add_to_tree(hud)
	app.add_child(viewport)
	app.add_child(box)
	app.add_child(texture)
	app.add_child(osd)
	var camera := Camera3D.new()
	var lens := Lens.new()
	var source := SubViewport.new()  # the clean feed needs its source camera and lens in the tree
	source.add_child(camera)
	source.add_child(lens)
	await add_to_tree(source)
	var clean_feed := CleanFeed.new()
	await add_to_tree(clean_feed)
	clean_feed.setup(camera, lens, Vector2i(64, 48))
	app.clean_feed = clean_feed
	app.dvr = dvr
	app.controls_menu = menu
	app.hud = hud
	app.fpv_viewport = viewport
	app.fpv_box = box
	app.fpv_texture = texture
	app.osd = osd
	app.settings = {"fpv_aspect": "4:3"}
	app.client = client
	menu.dvr_toggled.connect(app._set_dvr)
	return {"app": app, "dvr": dvr, "menu": menu, "hud": hud, "viewport": viewport, "box": box, "client": client}


func _client() -> Object:
	ok(ClassDB.class_exists("OfsClient"), "the extension is loaded")
	var client: Object = ClassDB.instantiate("OfsClient")
	await add_to_tree(client)
	# The dvr_* API lives on the session's Client, so these tests start a session that connects nowhere
	# (the address refuses at once); the phase is irrelevant, the recorder never touches the link.
	eq(client.start({"quad_path": QUAD, "server_addr": "127.0.0.1:1"}), "", "a session starts")
	return client


func _count_frames(path: String) -> int:
	var f := FileAccess.open(path, FileAccess.READ)
	ok(f != null, "the recording exists")
	if f == null:
		return 0
	var bytes := f.get_buffer(f.get_length())
	var count := 0
	for i in range(bytes.size() - 3):
		if bytes.slice(i, i + 4) == PackedByteArray([0x30, 0x30, 0x64, 0x63]):
			count += 1
	return count  # movi chunks + idx1 entries: an even count, frames = count / 2


func test_a_recording_round_trips() -> void:
	var client := await _client()
	var path := OS.get_user_data_dir() + "/test_dvr_roundtrip.avi"
	DirAccess.remove_absolute(path)  # a previous run's file: a start never overwrites one
	eq(client.dvr_start("goggles", path, 4, 2), "", "starts")
	for i in 10:
		client.dvr_write_frame("goggles", PackedByteArray([0xFF, 0xD8, 0xFF, 0xD9, i]))  # a JPEG-shaped blob; the muxer does not validate
	var summary: Dictionary = client.dvr_stop("goggles")
	eq(int(summary.get("frames", -1)), 10, "ten frames")
	eq(int(summary.get("dropped_frames", -1)), 0, "nothing dropped")
	eq(_count_frames(path) / 2, 10, "ten 00dc chunks in movi and ten index entries")
	var f := FileAccess.open(path, FileAccess.READ)
	if f != null:
		eq(f.get_buffer(4).get_string_from_ascii(), "RIFF", "it is a RIFF file")
	client.free()


func test_stop_without_start_and_double_start_are_honest() -> void:
	var client := await _client()
	eq(client.dvr_stop("goggles").size(), 0, "stop without start is empty")
	var path := OS.get_user_data_dir() + "/test_dvr_double.avi"
	DirAccess.remove_absolute(path)  # a previous run's file: a start never overwrites one
	eq(client.dvr_start("goggles", path, 4, 2), "", "first start")
	ok(client.dvr_start("goggles", path, 4, 2) != "", "a second start fails")
	client.dvr_stop("goggles")
	client.free()


func test_two_feeds_record_independently() -> void:
	var client := await _client()
	var p1 := OS.get_user_data_dir() + "/test_dvr_feed_goggles.avi"
	var p2 := OS.get_user_data_dir() + "/test_dvr_feed_clean.avi"
	DirAccess.remove_absolute(p1)
	DirAccess.remove_absolute(p2)
	eq(client.dvr_start("goggles", p1, 4, 2), "", "goggles starts")
	eq(client.dvr_start("clean", p2, 4, 2), "", "clean starts alongside it")
	ok(client.dvr_start("clean", p2, 4, 2) != "", "a second clean start fails")
	ok(client.dvr_start("bogus", p2, 4, 2) != "", "an unknown feed fails")
	for i in 2:
		client.dvr_write_frame("goggles", PackedByteArray([0xFF, 0xD8, 0xFF, 0xD9, i]))
	for i in 3:
		client.dvr_write_frame("clean", PackedByteArray([0xFF, 0xD8, 0xFF, 0xD9, i]))
	eq(int(client.dvr_stop("goggles").get("frames", -1)), 2, "goggles holds its two frames")
	eq(int(client.dvr_stop("clean").get("frames", -1)), 3, "clean holds its three frames")
	eq(client.dvr_stop("bogus").size(), 0, "stopping an unknown feed is empty")
	client.free()


func test_the_binding_exports_a_finished_recording_as_frames() -> void:
	var client := await _client()
	var path := OS.get_user_data_dir() + "/test_dvr_export.avi"
	var out_dir := OS.get_user_data_dir() + "/test_dvr_export_frames"
	DirAccess.remove_absolute(path)
	for file in DirAccess.get_files_at(out_dir):
		DirAccess.remove_absolute(out_dir.path_join(file))
	DirAccess.remove_absolute(out_dir)
	eq(client.dvr_start("goggles", path, 4, 2), "", "starts")
	client.dvr_write_frame("goggles", PackedByteArray([0xFF, 0xD8, 0xFF, 0xD9, 1]))
	client.dvr_stop("goggles")
	ok(client.dvr_export(path, "gif") != "", "an unknown mode is refused")
	eq(client.dvr_export(path, "frames"), "", "frames starts")
	var results: Array = []
	for i in 500:
		results = client.dvr_export_poll()
		if not results.is_empty():
			break
		OS.delay_msec(10)
	eq(results.size(), 1, "one result arrives")
	if results.size() == 1:
		ok(results[0]["ok"], "it succeeded: %s" % str(results[0]))
		eq(results[0]["kind"], "frames", "of kind frames")
		eq(results[0]["path"], path, "for that recording")
		eq(results[0]["detail"], out_dir, "into the frames directory")
		eq(DirAccess.get_files_at(out_dir).size(), 1, "holding the frame")
	client.free()


func test_a_drop_stretch_repeats_the_held_frame_once_per_boundary() -> void:
	var dvr: Node = Dvr.new()
	await add_to_tree(dvr)
	var viewport := SubViewport.new()
	await add_to_tree(viewport)
	var mock := MockClient.new()
	eq(dvr.start(mock, viewport), "", "starts against the stand-in")
	dvr._last_jpeg = PackedByteArray([1, 2, 3])  # the held frame, as a previous encode left it
	# A drop stretch: render_time_s holds at 0.04 while the boundaries advance through the held_fields count.
	dvr.capture({"render_time_s": 0.04, "held_fields": 1, "target_field": 3, "field_valid": false})
	eq(mock.frames.size(), 1, "the held frame repeats on the next boundary")
	eq(mock.frames[0], PackedByteArray([1, 2, 3]), "the repeated bytes are the held frame, not a re-encode")
	dvr.capture({"render_time_s": 0.04, "held_fields": 1, "target_field": 3, "field_valid": false})
	eq(mock.frames.size(), 1, "the same boundary does not repeat it twice")
	dvr.capture({"render_time_s": 0.04, "held_fields": 2, "target_field": 4, "field_valid": false})
	eq(mock.frames.size(), 2, "and the boundary after repeats it again")
	# The stretch ends: the recovered field renders for the boundary the last repeat already covered.
	dvr.capture({"render_time_s": 0.08, "held_fields": 0, "target_field": 4, "field_valid": true})
	eq(mock.frames.size(), 2, "the recovery boundary is deduped: one frame per boundary")
	# The next current field would encode from the viewport; headless has no picture, so nothing is written.
	dvr.capture({"render_time_s": 0.1, "held_fields": 0, "target_field": 5, "field_valid": true})
	eq(mock.frames.size(), 2, "headless: no picture to encode, no frame written")
	eq(dvr._last_target, 5, "the dedup key is the integer field the client renders for")
	eq(dvr.stop()["frames"], 2, "the stand-in reports what was fed")
	dvr.queue_free()


func test_the_menu_d_and_a_resize_keep_the_dvr_views_in_sync() -> void:
	var s := await _app_shell()
	var app = s["app"]
	var dvr = s["dvr"]
	var menu = s["menu"]
	var hud = s["hud"]
	var client = s["client"]
	menu._dvr.button_pressed = true
	ok(dvr.recording, "the menu toggle starts the DVR")
	ok(menu.dvr_on(), "and leaves the menu checked")
	ok(hud._rec.visible, "and shows REC")
	eq(client.starts, 1, "one start")
	app._set_dvr(true)
	eq(client.starts, 1, "starting again does not restart")
	menu.toggle()
	ok(menu.is_open(), "F2 opens")
	menu.toggle()
	ok(not menu.is_open(), "F2 closes without touching the DVR")
	ok(dvr.recording, "which keeps recording")
	app._toggle_dvr()
	ok(not dvr.recording, "D stops it")
	ok(not menu.dvr_on(), "D unchecks the menu")
	ok(not hud._rec.visible, "and hides REC")
	app._toggle_dvr()
	ok(dvr.recording, "D starts it again")
	ok(menu.dvr_on() and hud._rec.visible, "and both views follow")
	s["box"].size = Vector2(960, 720)
	s["viewport"].size = Vector2i(1, 1)
	app._layout_fpv()
	ok(dvr.recording, "a picture size change does not stop the DVR")
	ok(menu.dvr_on(), "the menu stays checked")
	ok(hud._rec.visible, "REC stays")
	eq(s["viewport"].size, Vector2i(960, 720), "and the viewport takes the new size")
	app.free()
	menu.queue_free()
	hud.queue_free()
	dvr.queue_free()


func test_entering_the_chase_view_stops_the_dvr() -> void:
	var s := await _app_shell()
	var app = s["app"]
	var menu = s["menu"]
	var hud = s["hud"]
	# set_chase_view also switches the cameras and the picture layers: stand-ins for those.
	app.fpv_camera = Camera3D.new()
	app.chase_camera = Camera3D.new()
	app.lens = MockOsd.new()
	app.video = MockOsd.new()
	for node in [app.fpv_camera, app.chase_camera, app.lens, app.video]:
		app.add_child(node)
	s["box"].size = Vector2(960, 720)
	app._layout_fpv()
	menu._dvr.button_pressed = true
	ok(s["dvr"].recording, "recording in the FPV view")
	app.set_chase_view(true)
	ok(not s["dvr"].recording, "the chase view stops the DVR")
	ok(not menu.dvr_on(), "the menu unchecks")
	ok(not hud._rec.visible, "REC hides")
	ok("chase" in " ".join(hud.toast_texts()), "with a chase toast: %s" % str(hud.toast_texts()))
	app.free()
	menu.queue_free()
	hud.queue_free()
	s["dvr"].queue_free()


func _toast_color(hud: Node, index: int) -> Color:
	return hud._toast_items[index]["label"].get_theme_color("font_color")


func test_finished_exports_are_toasted_and_failures_only_warn() -> void:
	var client := MockClient.new()
	client.export_results = [
		{"path": "/r/REC.avi", "kind": "frames", "ok": true, "detail": "/r/REC_frames"},
		{"path": "/r/REC.avi", "kind": "mp4", "ok": false, "detail": "disk exploded"},
		{"path": "/r/REC.avi", "kind": "mp4", "ok": false, "detail": "ffmpeg was not found on PATH"},
	]
	var s := await _app_shell(client)
	var app = s["app"]
	var hud = s["hud"]
	app._poll_dvr_exports()
	var texts: PackedStringArray = hud.toast_texts()
	eq(texts.size(), 3, "one toast per result: %s" % str(texts))
	ok("exported" in texts[0] and "REC_frames" in texts[0], "the first is an export: %s" % texts[0])
	eq(_toast_color(hud, 0), Hud.WHITE, "an info toast")
	ok("disk exploded" in texts[1], "the second carries the error: %s" % texts[1])
	eq(_toast_color(hud, 1), Hud.YELLOW, "a warning, never an error")
	eq(texts[2], "ffmpeg was not found on PATH: the AVI was kept", "a missing ffmpeg says the AVI is safe")
	eq(_toast_color(hud, 2), Hud.YELLOW, "also a warning")
	app._poll_dvr_exports()
	eq(hud.toast_texts().size(), 3, "nothing new, nothing toasted")
	app.free()
	s["menu"].queue_free()
	hud.queue_free()
	s["dvr"].queue_free()


func test_a_successful_stop_exports_the_recording_per_the_setting() -> void:
	var s := await _app_shell()
	var app = s["app"]
	var client = s["client"]
	app.settings["dvr_export"] = "both"
	s["menu"]._dvr.button_pressed = true
	var path: String = s["dvr"].paths[0]
	ok(path.is_absolute_path() and path.ends_with(".avi"), "the recording has an absolute path: %s" % path)
	app._toggle_dvr()
	eq(client.exports, [[path, "both"]], "D stops, then exports that file")
	app._toggle_dvr()
	app.settings["dvr_export"] = "none"
	app._toggle_dvr()
	eq(client.exports.size(), 1, "the none setting exports nothing")
	app.free()
	s["menu"].queue_free()
	s["hud"].queue_free()
	s["dvr"].queue_free()


func test_an_abnormal_stop_does_not_export() -> void:
	var s := await _app_shell()
	var app = s["app"]
	var client = s["client"]
	app.settings["dvr_export"] = "frames"
	s["menu"]._dvr.button_pressed = true
	client.fail_stop = true
	app._toggle_dvr()
	eq(client.exports.size(), 0, "no summary, no export")
	ok("abnormally" in " ".join(s["hud"].toast_texts()), "the failure is toasted")
	app.free()
	s["menu"].queue_free()
	s["hud"].queue_free()
	s["dvr"].queue_free()


func test_the_chase_stop_goes_through_the_same_stop_path() -> void:
	var s := await _app_shell()
	var app = s["app"]
	var client = s["client"]
	app.fpv_camera = Camera3D.new()
	app.chase_camera = Camera3D.new()
	app.lens = MockOsd.new()
	app.video = MockOsd.new()
	for node in [app.fpv_camera, app.chase_camera, app.lens, app.video]:
		app.add_child(node)
	app.settings["dvr_export"] = "mp4"
	s["menu"]._dvr.button_pressed = true
	var path: String = s["dvr"].paths[0]
	app.set_chase_view(true)
	eq(client.exports, [[path, "mp4"]], "the chase stop exports too")
	ok("chase" in " ".join(s["hud"].toast_texts()), "and keeps the chase toast: %s" % str(s["hud"].toast_texts()))
	app.free()
	s["menu"].queue_free()
	s["hud"].queue_free()
	s["dvr"].queue_free()


func _solid(width: int, height: int, color: Color) -> Image:
	var image := Image.create(width, height, false, Image.FORMAT_RGB8)
	image.fill(color)
	return image


func test_fit_image_keeps_a_same_size_image_untouched() -> void:
	var image := _solid(8, 4, Color.RED)
	ok(Dvr.fit_image(image, 8, 4) == image, "the same object comes back")


func test_fit_image_letterboxes_a_wider_image() -> void:
	var fitted: Image = Dvr.fit_image(_solid(16, 4, Color.RED), 8, 8)
	eq(fitted.get_size(), Vector2i(8, 8), "the target size")
	eq(fitted.get_format(), Image.FORMAT_RGB8, "the input's format")
	eq(fitted.get_pixel(4, 0), Color.BLACK, "the top row is black")
	eq(fitted.get_pixel(4, 4), Color.RED, "the centre is the picture")
	eq(fitted.get_pixel(4, 7), Color.BLACK, "the last row is black")


func test_fit_image_pillarboxes_a_taller_image() -> void:
	var fitted: Image = Dvr.fit_image(_solid(4, 16, Color.RED), 8, 8)
	eq(fitted.get_size(), Vector2i(8, 8), "the target size")
	eq(fitted.get_pixel(0, 4), Color.BLACK, "the left column is black")
	eq(fitted.get_pixel(4, 4), Color.RED, "the centre is the picture")
	eq(fitted.get_pixel(7, 4), Color.BLACK, "the right column is black")


func test_a_resize_mid_recording_keeps_the_declared_size() -> void:
	var dvr: Node = Dvr.new()
	await add_to_tree(dvr)
	var viewport := SubViewport.new()
	viewport.size = Vector2i(8, 4)
	await add_to_tree(viewport)
	var mock := MockClient.new()
	eq(dvr.start(mock, viewport), "", "starts against the stand-in")
	var sizes := [Vector2i(6, 6), Vector2i(16, 16)]
	dvr.image_source = func() -> Image:
		var size: Vector2i = sizes.pop_front()
		return _solid(size.x, size.y, Color.GREEN)
	dvr.capture({"render_time_s": 0.02, "held_fields": 0, "target_field": 1, "field_valid": true})
	dvr.capture({"render_time_s": 0.04, "held_fields": 0, "target_field": 2, "field_valid": true})
	eq(mock.frames.size(), 2, "both fields are written")
	for jpeg in mock.frames:
		var decoded := Image.new()
		eq(decoded.load_jpg_from_buffer(jpeg), OK, "decodes")
		eq(decoded.get_size(), Vector2i(8, 4), "at the size the file declared")
	dvr.stop()
	dvr.queue_free()
	viewport.queue_free()


func test_a_reload_stops_the_dvr_and_syncs_the_views() -> void:
	var s := await _app_shell()
	var app = s["app"]
	var menu = s["menu"]
	var hud = s["hud"]
	menu._dvr.button_pressed = true
	ok(s["dvr"].recording, "recording before the reload")
	app._on_phase_changed("loading", "", "")
	ok(not s["dvr"].recording, "the reload stops the DVR")
	ok(not menu.dvr_on(), "the menu unchecks")
	ok(not hud._rec.visible, "REC hides")
	app.free()
	menu.queue_free()
	hud.queue_free()
	s["dvr"].queue_free()


func test_a_failed_dvr_start_leaves_the_menu_and_hud_off() -> void:
	var s := await _app_shell(null)
	var app = s["app"]
	var menu = s["menu"]
	var hud = s["hud"]
	menu._dvr.button_pressed = true
	ok(not app.dvr.recording, "no client: no recording")
	ok(not menu.dvr_on(), "the checkbox goes back off")
	ok(not hud._rec.visible, "REC stays hidden")
	ok("DVR failed" in " ".join(hud.toast_texts()), "the failure is toasted: %s" % str(hud.toast_texts()))
	app.free()
	menu.queue_free()
	hud.queue_free()
	s["dvr"].queue_free()


func test_the_chase_view_refuses_a_dvr_start_and_clears_the_toggle() -> void:
	var s := await _app_shell()
	var app = s["app"]
	app.chase_view = true
	s["menu"]._dvr.button_pressed = true
	ok(not app.dvr.recording, "the chase view does not record")
	ok(not s["menu"].dvr_on(), "the checkbox goes back off")
	ok(not s["hud"]._rec.visible, "REC stays hidden")
	ok("switch with C first" in " ".join(s["hud"].toast_texts()), "with the chase-view toast")
	app.free()
	s["menu"].queue_free()
	s["hud"].queue_free()
	s["dvr"].queue_free()


func _synthetic_image() -> Image:
	var image := Image.create(64, 48, false, Image.FORMAT_RGB8)
	image.fill(Color(0.2, 0.6, 0.3))
	return image


## m3e debt: the encode path was only checked by eye. A current field goes through Image -> save_jpg_to_buffer at
## JPEG_QUALITY -> dvr_write_frame, and what arrives must be a real, decodable JPEG.
func test_a_current_field_is_encoded_as_a_jpeg_at_the_dvr_quality() -> void:
	ok(Dvr.JPEG_QUALITY > 0.0 and Dvr.JPEG_QUALITY <= 1.0, "Godot's JPEG quality is on the 0-1 scale")
	var dvr: Node = Dvr.new()
	await add_to_tree(dvr)
	var viewport := SubViewport.new()
	viewport.size = Vector2i(64, 48)
	await add_to_tree(viewport)
	var mock := MockClient.new()
	eq(dvr.start(mock, viewport), "", "starts against the stand-in")
	dvr.image_source = _synthetic_image
	dvr.capture({"render_time_s": 0.02, "held_fields": 0, "target_field": 1, "field_valid": true})
	eq(mock.frames.size(), 1, "a current field writes one frame")
	if mock.frames.size() == 1:
		var jpeg: PackedByteArray = mock.frames[0]
		ok(jpeg.size() > 100, "a real encode, not an empty buffer: %d bytes" % jpeg.size())
		eq(jpeg.slice(0, 2), PackedByteArray([0xFF, 0xD8]), "it starts with the JPEG SOI marker")
		eq(jpeg.slice(jpeg.size() - 2), PackedByteArray([0xFF, 0xD9]), "and ends with EOI")
		var decoded := Image.new()
		eq(decoded.load_jpg_from_buffer(jpeg), OK, "Godot decodes it back")
		eq(decoded.get_size(), Vector2i(64, 48), "at the captured size")
	dvr.stop()
	dvr.queue_free()
	viewport.queue_free()
