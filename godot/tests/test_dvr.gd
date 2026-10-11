extends "res://tests/testing.gd"
## The DVR's file side: the client's dvr_* API writes a parseable MJPEG AVI (headless has no renderer, so the
## capture path itself is checked by eye and by shots.gd; test_dvr.gd drives the bytes).

const QUAD := "res://../quads/opendrone-5f-freestyle.toml"


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
