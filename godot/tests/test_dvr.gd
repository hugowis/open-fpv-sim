extends "res://tests/testing.gd"
## The DVR's file side and capture timing: the client's dvr_* API writes a parseable MJPEG AVI, and the
## boundary dedup and held-frame freeze are checked against a stand-in client (headless has no renderer, so
## the encode path itself is checked by eye and by shots.gd; test_dvr.gd drives the bytes and the timing).

const Dvr = preload("res://ui/dvr.gd")
const QUAD := "res://../quads/opendrone-5f-freestyle.toml"


## A stand-in for OfsClient: records what the DVR feeds it.
class MockClient:
	var frames: Array = []

	func dvr_start(_path: String, _width: int, _height: int) -> String:
		return ""

	func dvr_write_frame(jpeg: PackedByteArray) -> void:
		frames.append(jpeg)

	func dvr_stop() -> Dictionary:
		return {"frames": frames.size(), "dropped_frames": 0, "bytes": 0}


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
	eq(client.dvr_start(path, 4, 2), "", "starts")
	for i in 10:
		client.dvr_write_frame(PackedByteArray([0xFF, 0xD8, 0xFF, 0xD9, i]))  # a JPEG-shaped blob; the muxer does not validate
	var summary: Dictionary = client.dvr_stop()
	eq(int(summary.get("frames", -1)), 10, "ten frames")
	eq(int(summary.get("dropped_frames", -1)), 0, "nothing dropped")
	eq(_count_frames(path) / 2, 10, "ten 00dc chunks in movi and ten index entries")
	var f := FileAccess.open(path, FileAccess.READ)
	if f != null:
		eq(f.get_buffer(4).get_string_from_ascii(), "RIFF", "it is a RIFF file")
	client.free()


func test_stop_without_start_and_double_start_are_honest() -> void:
	var client := await _client()
	eq(client.dvr_stop().size(), 0, "stop without start is empty")
	var path := OS.get_user_data_dir() + "/test_dvr_double.avi"
	eq(client.dvr_start(path, 4, 2), "", "first start")
	ok(client.dvr_start(path, 4, 2) != "", "a second start fails")
	client.dvr_stop()
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
