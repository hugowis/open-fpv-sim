extends "res://tests/testing.gd"
## The clean feed: a second viewport that renders the camera image (3D view and lens) without the OSD or the analog
## video layer, and only while it is wanted. Headless has no renderer, so these check the wiring, not pixels;
## tests/shots.gd draws it.

const CleanFeed = preload("res://ui/clean_feed.gd")
const Lens = preload("res://ui/lens.gd")
const Osd = preload("res://ui/osd.gd")
const Video = preload("res://ui/video.gd")


## A clean feed set up over a source camera and lens that are already in the tree; the nodes to free come back too.
func _feed(size := Vector2i(640, 480)) -> Dictionary:
	var camera := Camera3D.new()
	var lens := Lens.new()
	lens.distortion = 0.2
	lens.exposure = 1.5
	lens.vignette = 0.6
	var source := SubViewport.new()  # like FpvViewport, the camera and the lens live in a viewport with MSAA on
	source.msaa_3d = Viewport.MSAA_4X
	source.add_child(camera)
	source.add_child(lens)
	await add_to_tree(source)
	var feed := CleanFeed.new()
	await add_to_tree(feed)
	feed.setup(camera, lens, size)
	await tree.process_frame
	return {"feed": feed, "source": source, "camera": camera}


func _free(parts: Dictionary) -> void:
	parts["feed"].queue_free()
	parts["source"].queue_free()


func test_it_is_off_until_activated() -> void:
	var p := await _feed()
	var feed = p["feed"]
	eq(feed.viewport().render_target_update_mode, SubViewport.UPDATE_DISABLED, "off after setup")
	ok(not feed.is_active(), "not active after setup")
	feed.set_active(true)
	eq(feed.viewport().render_target_update_mode, SubViewport.UPDATE_ALWAYS, "rendering when on")
	ok(feed.is_active(), "active")
	feed.set_active(false)
	eq(feed.viewport().render_target_update_mode, SubViewport.UPDATE_DISABLED, "off again")
	ok(not feed.is_active(), "not active again")
	_free(p)


func test_the_camera_follows_the_source() -> void:
	var p := await _feed()
	var feed = p["feed"]
	var source: Camera3D = p["camera"]
	source.global_transform = Transform3D(Basis.from_euler(Vector3(0.3, -1.1, 0.2)), Vector3(4.0, 2.0, -7.0))
	source.fov = 95.0
	source.near = 0.05
	source.far = 900.0
	source.keep_aspect = Camera3D.KEEP_HEIGHT
	source.cull_mask = 0b101
	feed.sync_camera()
	var clean: Camera3D = feed.viewport().get_camera_3d()
	ok(clean != null and clean != source, "the viewport has a camera of its own")
	ok(clean.global_transform.is_equal_approx(source.global_transform), "same pose")
	near(clean.fov, 95.0, "same fov")
	near(clean.near, 0.05, "same near")
	near(clean.far, 900.0, "same far")
	eq(clean.keep_aspect, Camera3D.KEEP_HEIGHT, "same keep_aspect")
	eq(clean.cull_mask, 0b101, "same cull mask")
	_free(p)


func test_activating_puts_the_camera_where_the_source_is() -> void:
	var p := await _feed()
	var source: Camera3D = p["camera"]
	source.global_transform = Transform3D(Basis.from_euler(Vector3(0.1, 0.7, 0.0)), Vector3(1.0, 2.0, 3.0))
	p["feed"].set_active(true)  # no sync_camera call: the first clean frame must already use this pose
	ok(p["feed"].viewport().get_camera_3d().global_transform.is_equal_approx(source.global_transform), "the clean camera is at the source pose")
	_free(p)


func test_the_lens_settings_are_copied() -> void:
	var p := await _feed()
	var layers: Array = p["feed"].viewport().find_children("*", "CanvasLayer", false, false)
	eq(layers.size(), 1, "one canvas layer")
	var lens: CanvasLayer = layers[0]
	ok(lens.get_script() == Lens, "it is a lens")
	near(lens.distortion, 0.2, "distortion")
	near(lens.exposure, 1.5, "exposure")
	near(lens.vignette, 0.6, "vignette")
	_free(p)


func test_resize_resizes_the_viewport() -> void:
	var p := await _feed(Vector2i(640, 480))
	var feed = p["feed"]
	eq(feed.viewport().size, Vector2i(640, 480), "the size given to setup")
	feed.resize(Vector2i(1280, 720))
	eq(feed.viewport().size, Vector2i(1280, 720), "resized")
	_free(p)


func test_the_clean_viewport_has_no_osd_or_video_layer() -> void:
	var p := await _feed()
	for node in p["feed"].viewport().find_children("*", "", true, false):
		ok(node.get_script() != Osd and node.get_script() != Video, "%s is not an OSD or video layer" % node.name)
	_free(p)


func test_the_viewport_shares_the_world_and_the_source_msaa() -> void:
	var p := await _feed()
	var viewport: SubViewport = p["feed"].viewport()
	ok(not viewport.own_world_3d, "no world of its own")
	ok(viewport.find_world_3d() == p["feed"].get_viewport().find_world_3d(), "the world of the viewport it sits in")
	eq(viewport.msaa_3d, Viewport.MSAA_4X, "same anti-aliasing as the FPV viewport")
	_free(p)
