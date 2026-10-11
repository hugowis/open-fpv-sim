extends Node
## The goggles' DVR (M3e spec §5): records the FPV composite — the SubViewport's picture, degradation and OSD
## included, exactly what the pilot sees — to an MJPEG AVI. The Rust side muxes (OfsClient.dvr_*); this node
## times the captures to the received fields and owns the UX. Records in the FPV view only; a resize or aspect
## switch while recording stops it (app.gd).

signal started(path: String)
signal stopped(summary: Dictionary)

## Godot's JPG quality is a float from 0.01 (worst) to 1.0 (best) — an integer like 80 makes the encoder
## return empty buffers, so this must stay on the 0-1 scale.
const JPEG_QUALITY := 0.8

var recording := false
var _client: Object
var _viewport: SubViewport
var _last_render_time := -1.0
var _last_jpeg := PackedByteArray()


func start(client: Object, viewport: SubViewport) -> String:
	var path := "user://recordings/REC_%s.avi" % _timestamp()
	var error: String = client.dvr_start(ProjectSettings.globalize_path(path), viewport.size.x, viewport.size.y)
	if error != "":
		return error
	recording = true
	_client = client
	_viewport = viewport
	_last_render_time = -1.0
	_last_jpeg = PackedByteArray()
	started.emit(path)
	return ""


## Feeds one field. `view` is the Dictionary `OfsClient.get_fpv_view()` returns: each new field boundary
## captures the picture; a dropped one repeats the held frame, so the recording freezes where the pilot froze.
## A frame the size of the viewport's texture; when the size changes app.gd stops the recording.
func capture(view: Dictionary) -> void:
	if not recording or view.is_empty():
		return
	var t: float = view.get("render_time_s", -1.0)
	if t == _last_render_time:
		return
	_last_render_time = t
	if not bool(view.get("field_valid", false)):
		if not _last_jpeg.is_empty():
			_client.dvr_write_frame(_last_jpeg)
		return
	var image: Image = _viewport.get_texture().get_image()
	if image == null or image.is_empty():
		return  # headless or the texture not yet rendered: this field is simply not in the recording
	_last_jpeg = image.save_jpg_to_buffer(JPEG_QUALITY)
	if _last_jpeg.is_empty():
		return  # the encoder produced nothing: a 0-byte frame is never worth writing
	_client.dvr_write_frame(_last_jpeg)


func stop() -> Dictionary:
	if not recording:
		return {}
	recording = false
	var summary: Dictionary = _client.dvr_stop()
	stopped.emit(summary)
	return summary


func _timestamp() -> String:
	return Time.get_datetime_string_from_system(true).replace(":", "").replace("-", "").replace("T", "_")
