extends Node
## The clean feed for the DVR: the camera image (the 3D view and the lens layer) without the OSD or the analog video
## layer. A second SubViewport that shares the world with the FPV viewport (no `own_world_3d`) and costs nothing
## while off: it only renders while `set_active(true)`.

const Lens = preload("res://ui/lens.gd")

var _viewport := SubViewport.new()
var _camera := Camera3D.new()
var _source: Camera3D = null


## Builds the viewport at `size`, with the source viewport's anti-aliasing and a lens copying `source_lens`.
func setup(source_camera: Camera3D, source_lens: CanvasLayer, size: Vector2i) -> void:
	_source = source_camera
	_viewport.name = "CleanViewport"
	_viewport.size = size
	_viewport.msaa_3d = source_camera.get_viewport().msaa_3d
	_viewport.handle_input_locally = false
	_viewport.render_target_update_mode = SubViewport.UPDATE_DISABLED
	_camera.current = true
	_viewport.add_child(_camera)
	var lens := Lens.new()
	lens.name = "Lens"
	lens.layer = 1
	lens.distortion = source_lens.distortion
	lens.exposure = source_lens.exposure
	lens.vignette = source_lens.vignette
	_viewport.add_child(lens)
	add_child(_viewport)
	sync_camera()


func set_active(on: bool) -> void:
	_viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS if on else SubViewport.UPDATE_DISABLED


func is_active() -> bool:
	return _viewport.render_target_update_mode == SubViewport.UPDATE_ALWAYS


## Puts the clean camera where the source camera is, with its optics.
func sync_camera() -> void:
	_camera.global_transform = _source.global_transform
	_camera.fov = _source.fov
	_camera.near = _source.near
	_camera.far = _source.far
	_camera.keep_aspect = _source.keep_aspect
	_camera.cull_mask = _source.cull_mask


func resize(size: Vector2i) -> void:
	_viewport.size = size


func viewport() -> SubViewport:
	return _viewport
