extends Node
## The goggles' DVR (M3e spec §5): records the FPV composite — the SubViewport's picture, degradation and OSD
## included, exactly what the pilot sees — to an MJPEG AVI. The Rust side muxes (OfsClient.dvr_*); this node
## times the captures to the received field boundaries and owns the UX. A boundary the client cannot show
## freshly (`held_fields > 0`: dropped fields held the picture back) repeats the held frame, so the recording
## freezes where the pilot froze. Records in the FPV view only; a resize or aspect switch while recording
## stops it (app.gd).

signal started(path: String)
signal stopped(summary: Dictionary)

## One analog video field: 20 ms of simulated time (50 fields a second) — the same period as ofs-client's
## `fpv::FIELD_PERIOD_S`, duplicated here because a GDScript constant cannot reach it. The boundary the client
## renders for is `render_time_s + held_fields × this`, which is what captures dedup on.
const FIELD_PERIOD_S := 0.02

## Godot's JPG quality is a float from 0.01 (worst) to 1.0 (best) — an integer like 80 makes the encoder
## return empty buffers, so this must stay on the 0-1 scale.
const JPEG_QUALITY := 0.8

var recording := false
var _client: Object
var _viewport: SubViewport
var _last_target := -1.0
var _last_jpeg := PackedByteArray()


func start(client: Object, viewport: SubViewport) -> String:
	var path := "user://recordings/REC_%s.avi" % _timestamp()
	var error: String = client.dvr_start(ProjectSettings.globalize_path(path), viewport.size.x, viewport.size.y)
	if error != "":
		return error
	recording = true
	_client = client
	_viewport = viewport
	_last_target = -1.0
	_last_jpeg = PackedByteArray()
	started.emit(path)
	return ""


## Feeds one field. `view` is the Dictionary `OfsClient.get_fpv_view()` returns, deduped on the field boundary
## the client renders for (`render_time_s` + `held_fields` × FIELD_PERIOD_S, which advances through a drop
## stretch even though `render_time_s` holds): each new boundary contributes exactly one frame. A current field
## (`held_fields == 0`) is encoded from the viewport; a held one repeats the last frame as-is — the SubViewport
## still animates prop spin from live telemetry, so re-encoding would break the freeze. A frame the size of the
## viewport's texture; when the size changes app.gd stops the recording.
func capture(view: Dictionary) -> void:
	if not recording or view.is_empty():
		return
	var held := int(view.get("held_fields", 0))
	var target: float = view.get("render_time_s", -1.0) + float(held) * FIELD_PERIOD_S
	if target == _last_target:
		return
	_last_target = target
	if held > 0:
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
