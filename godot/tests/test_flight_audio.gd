extends "res://tests/testing.gd"

const FlightAudio = preload("res://drone/flight_audio.gd")


## Stands in for OfsClient: records what the node asks for and answers with silence.
class FakeClient:
	extends RefCounted
	var rate := 44100.0
	var calls: Array = []

	func audio_sample_rate() -> float:
		return rate

	func audio_render(frames: int, active: bool) -> PackedVector2Array:
		calls.append([frames, active])
		var out := PackedVector2Array()
		out.resize(frames)
		return out


func _bus_count() -> int:
	var n := 0
	for i in AudioServer.bus_count:
		if AudioServer.get_bus_name(i) == FlightAudio.BUS:
			n += 1
	return n


func _node() -> Array:
	var fake := FakeClient.new()
	var node := FlightAudio.new()
	await add_to_tree(node)
	node.setup(fake)
	return [node, fake]


func test_pump_asks_the_client_for_the_frames_the_generator_has_room_for() -> void:
	var s := await _node()
	var node = s[0]
	var fake: FakeClient = s[1]
	node.active = true
	var pushed: int = node.pump()
	ok(pushed > 0, "frames are pushed: %d" % pushed)
	eq(fake.calls.size(), 1, "one render call")
	eq(fake.calls[0][1], true, "the node's active flag is passed")
	ok(fake.calls[0][0] <= int(0.1 * fake.rate), "never more than the generator's 0.1 s: %d" % fake.calls[0][0])
	node.active = false
	node.pump()
	ok(fake.calls.size() < 2 or fake.calls[1][1] == false, "inactive is passed on")
	node.queue_free()


func test_a_disabled_node_pushes_nothing_and_mutes_the_bus() -> void:
	var s := await _node()
	var node = s[0]
	var fake: FakeClient = s[1]
	node.set_enabled(false)
	eq(node.pump(), 0, "no frames while off")
	eq(fake.calls.size(), 0, "the client is not asked")
	ok(AudioServer.is_bus_mute(AudioServer.get_bus_index(FlightAudio.BUS)), "the bus is muted")
	node.set_enabled(true)
	ok(not AudioServer.is_bus_mute(AudioServer.get_bus_index(FlightAudio.BUS)), "and unmuted when back on")
	ok(node.pump() > 0, "frames flow again")
	node.queue_free()


func test_turning_sound_back_on_restarts_the_player_with_an_empty_buffer() -> void:
	var s := await _node()
	var node = s[0]
	node.pump()
	var before = node.playback
	node.set_enabled(false)
	node.set_enabled(true)
	ok(node.playback != before, "a fresh playback: nothing stale left to play")
	ok(node.player.playing, "and the player is playing")
	var fake: FakeClient = s[1]
	fake.calls.clear()
	node.pump()
	eq(fake.calls[0][0], int(0.1 * fake.rate), "the whole buffer is free again: %d" % fake.calls[0][0])
	node.queue_free()


func test_setup_twice_keeps_one_bus() -> void:
	var s := await _node()
	var node = s[0]
	node.setup(s[1])
	eq(_bus_count(), 1, "one Flight bus")
	eq(node.get_child_count(), 1, "and one player")
	node.queue_free()
