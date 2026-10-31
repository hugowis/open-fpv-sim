extends Node
## The quad's sound: asks the client for the samples the generator has room for each frame and plays them on the
## "Flight" bus (the quad's microphone). The synthesis is the extension's (OfsClient.audio_render); this node only
## moves frames, and mutes the bus when the "Sound" setting is off.

const BUS := "Flight"
## The generator's buffer: parameter changes are smoothed at sample rate inside the synth, so 0.1 s is plenty.
const BUFFER_S := 0.1

## False while the Sound setting is off: nothing is pushed and the bus is muted.
var enabled := true
## Whether the quad is flying (else the client renders silence): set by the app each frame.
var active := false
var player: AudioStreamPlayer = null
var playback: AudioStreamGeneratorPlayback = null

var _client: Object = null
var _rate := 44100.0


## Creates the bus under Master (once), the player and its generator at the client's sample rate, and starts playing.
func setup(client: Object) -> void:
	_client = client
	_rate = client.audio_sample_rate()
	if AudioServer.get_bus_index(BUS) == -1:
		AudioServer.add_bus()
		var index := AudioServer.bus_count - 1
		AudioServer.set_bus_name(index, BUS)
		AudioServer.set_bus_send(index, "Master")
	if player == null:
		player = AudioStreamPlayer.new()
		player.bus = BUS
		add_child(player)
	var generator := AudioStreamGenerator.new()
	generator.mix_rate = _rate
	generator.buffer_length = BUFFER_S
	player.stream = generator
	_restart()


func set_enabled(on: bool) -> void:
	enabled = on
	var index := AudioServer.get_bus_index(BUS)
	if index != -1:
		AudioServer.set_bus_mute(index, not on)
	if on and player != null:
		_restart()  # whatever the buffer still held from before must not play


## Pushes the frames the generator has room for and returns how many. Nothing while the sound is off.
func pump() -> int:
	if not enabled or playback == null:
		return 0
	var room := mini(playback.get_frames_available(), int(BUFFER_S * _rate))
	if room <= 0:
		return 0
	var frames: PackedVector2Array = _client.audio_render(room, active)
	playback.push_buffer(frames)
	return frames.size()


## Stops and plays again: a new, empty playback.
func _restart() -> void:
	player.stop()
	player.play()
	playback = player.get_stream_playback() as AudioStreamGeneratorPlayback
