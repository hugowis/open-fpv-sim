extends Node
## The goggles' DVR (M3e spec §5): records the FPV composite — the SubViewport's picture, degradation and OSD
## included, exactly what the pilot sees — to an MJPEG AVI. The Rust side muxes (OfsClient.dvr_*); this node
## times the captures to the field the client renders for (`target_field`, an integer index computed in Rust)
## and owns the UX. A boundary the client cannot show freshly (`held_fields > 0`: dropped fields held the picture back) repeats the held frame, so the recording
## freezes where the pilot froze. Records in the FPV view only (entering the chase view stops it, app.gd); a resize
## or aspect switch while recording keeps going, each frame letterboxed to the starting size. It can also record the
## clean feed (the camera image without the OSD or video layer) alone or beside the goggles, in `REC_<ts>_clean.avi`;
## both feeds are captured on the same field boundary.

signal started(path: String)
signal stopped(summary: Dictionary)

## Godot's JPG quality is a float from 0.01 (worst) to 1.0 (best) — an integer like 80 makes the encoder
## return empty buffers, so this must stay on the 0-1 scale.
const JPEG_QUALITY := 0.8

var recording := false
## The absolute .avi paths of the current (or last) recording, goggles first; `stop()`'s summary carries them as `paths`.
var paths := PackedStringArray()
var _client: Object
## One entry per recorded feed, in start order: {feed, viewport, size, jpeg}. `size` is the picture size the feed
## declared at start (every frame is fitted to it: an AVI has one size for its whole length), `jpeg` its last frame.
var _feeds: Array = []
var _last_target := -1
## True until the first field after `start` was captured: a disabled SubViewport keeps its last texture, so the clean
## viewport's picture on that field is the previous recording's last one and must not be encoded.
var _first_field := false
## Where a current field's picture comes from: the FPV SubViewport (goggles) or the clean one, unless a test sets
## these to a Callable returning an Image (headless Godot renders nothing; the tests feed synthetic images through the
## same encode-and-write path).
var image_source := Callable()
var clean_image_source := Callable()
## Test seam: a Callable returning the timestamp part of the file name (the system clock when unset).
var clock := Callable()


## Starts recording `feed`: "goggles" (the FPV composite), "clean" or "both" (the clean feed needs `clean_viewport`).
## Returns an error string, and starts nothing, when any feed cannot start.
func start(client: Object, viewport: SubViewport, feed := "goggles", clean_viewport: SubViewport = null) -> String:
	var wanted := {"goggles": ["goggles"], "clean": ["clean"], "both": ["goggles", "clean"]}.get(feed, []) as Array
	if wanted.is_empty():
		return "unknown DVR feed: %s" % feed
	if "clean" in wanted and clean_viewport == null:
		return "the clean feed is not available"
	var stem := _free_stem(wanted)
	var started_feeds: Array = []
	var started_paths := PackedStringArray()
	var first_path := ""
	for which in wanted:
		var source_viewport: SubViewport = viewport if which == "goggles" else clean_viewport
		var path := stem + (".avi" if which == "goggles" else "_clean.avi")
		first_path = first_path if first_path != "" else path
		var absolute := ProjectSettings.globalize_path(path)
		var error: String = client.dvr_start(which, absolute, source_viewport.size.x, source_viewport.size.y)
		if error != "":
			for done in started_feeds:  # no half-started recording: stop what started and remove its empty file
				client.dvr_stop(done["feed"])
			for done_path in started_paths:
				DirAccess.remove_absolute(done_path)
				DirAccess.remove_absolute(done_path.get_basename() + ".audio")
			return error
		started_feeds.append({"feed": which, "viewport": source_viewport, "size": source_viewport.size, "jpeg": PackedByteArray()})
		started_paths.append(absolute)
	recording = true
	paths = started_paths
	_client = client
	_feeds = started_feeds
	_last_target = -1
	_first_field = true
	started.emit(first_path)
	return ""


## Feeds one field. `view` is the Dictionary `OfsClient.get_fpv_view()` returns, deduped on `target_field` — the
## integer index of the field boundary the client renders for, which advances through a drop stretch even though
## `render_time_s` holds: each new boundary contributes exactly one frame. A current field (`held_fields == 0`) is
## encoded from the feed's viewport; a held one repeats the last frame as-is — the SubViewport still animates prop spin
## from live telemetry, so re-encoding would break the freeze. Every frame is fitted to the size declared at start.
## All or nothing per boundary, so the files stay on one timeline: once any feed has a frame, every feed writes one (a
## feed that has none yet, like the clean one on its first field, writes a black frame); while no feed has one, nobody
## writes. The clean feed does not encode on the first field after `start` (its viewport has not rendered for this
## recording yet), so that frame is black.
func capture(view: Dictionary) -> void:
	if not recording or view.is_empty():
		return
	var held := int(view.get("held_fields", 0))
	var target := int(view.get("target_field", -1))
	if target == _last_target:
		return
	_last_target = target
	var any_frame := false
	for entry in _feeds:
		if held == 0 and not (_first_field and entry["feed"] == "clean"):
			_encode(entry)  # a feed with no usable picture keeps its previous frame
		any_frame = any_frame or not entry["jpeg"].is_empty()
	_first_field = false
	if not any_frame:
		return  # headless, or nothing rendered yet: this field is not in the recording
	for entry in _feeds:
		_client.dvr_write_frame(entry["feed"], entry["jpeg"] if not entry["jpeg"].is_empty() else _black(entry))


## Encodes this feed's current picture into `entry["jpeg"]`; leaves it as it was when there is no picture or the
## encoder produced nothing (a 0-byte frame is never worth writing).
func _encode(entry: Dictionary) -> void:
	var source := image_source if entry["feed"] == "goggles" else clean_image_source
	var image: Image = source.call() if source.is_valid() else entry["viewport"].get_texture().get_image()
	if image == null or image.is_empty():
		return
	var size: Vector2i = entry["size"]
	var jpeg := fit_image(image, size.x, size.y).save_jpg_to_buffer(JPEG_QUALITY)
	if not jpeg.is_empty():
		entry["jpeg"] = jpeg


## A black frame at the feed's declared size, encoded once.
func _black(entry: Dictionary) -> PackedByteArray:
	if not entry.has("black"):
		var size: Vector2i = entry["size"]
		var image := Image.create(size.x, size.y, false, Image.FORMAT_RGB8)
		image.fill(Color.BLACK)
		entry["black"] = image.save_jpg_to_buffer(JPEG_QUALITY)
	return entry["black"]


## `image` itself when it already is width x height; otherwise a black width x height image with `image` scaled to fit
## (aspect kept, bilinear) and centred, so a resize or aspect switch mid-recording still yields the declared size.
@warning_ignore("integer_division")
static func fit_image(image: Image, width: int, height: int) -> Image:
	if image.get_width() == width and image.get_height() == height:
		return image
	var scale := minf(float(width) / image.get_width(), float(height) / image.get_height())
	var w := maxi(1, roundi(image.get_width() * scale))
	var h := maxi(1, roundi(image.get_height() * scale))
	var scaled := image.duplicate() as Image
	scaled.resize(w, h, Image.INTERPOLATE_BILINEAR)
	var fitted := Image.create(width, height, false, image.get_format())
	fitted.fill(Color.BLACK)
	fitted.blit_rect(scaled, Rect2i(0, 0, w, h), Vector2i((width - w) / 2, (height - h) / 2))
	return fitted


## Stops every feed. The first feed's summary comes back with `paths` and `dropped_frames` summed over all feeds (each
## feed has its own writer queue, so one file can drop frames the other does not); it is empty when any feed failed to
## finalize (after all of them were stopped), so there is nothing to export.
func stop() -> Dictionary:
	if not recording:
		return {}
	recording = false
	var summary := {}
	var failed := false
	var dropped := 0
	for i in _feeds.size():
		var one: Dictionary = _client.dvr_stop(_feeds[i]["feed"])
		failed = failed or one.is_empty()
		dropped += int(one.get("dropped_frames", 0))
		if i == 0:
			summary = one
	_feeds = []
	if failed:
		summary = {}
	else:
		summary["dropped_frames"] = dropped
		summary["paths"] = paths
	stopped.emit(summary)
	return summary


## The path stem (no extension) of a recording none of whose files exists yet: the muxer never overwrites, so two starts
## within one millisecond (the timestamp's resolution) would otherwise fail the second. Appends _2, _3, ... on a clash.
func _free_stem(wanted: Array) -> String:
	var base := "user://recordings/REC_%s" % _timestamp()
	var stem := base
	var n := 2
	while wanted.any(func(feed: String) -> bool: return FileAccess.file_exists(stem + ("_clean.avi" if feed == "clean" else ".avi"))):
		stem = "%s_%d" % [base, n]
		n += 1
	return stem


func _timestamp() -> String:
	if clock.is_valid():
		return clock.call()
	# Plus a millisecond uniqueness suffix: most starts within one second get a new name on their own.
	return "%s_%03d" % [Time.get_datetime_string_from_system(true).replace(":", "").replace("-", "").replace("T", "_"), Time.get_ticks_msec() % 1000]
