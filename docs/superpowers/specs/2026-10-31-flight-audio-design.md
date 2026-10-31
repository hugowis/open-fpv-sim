# Procedural flight audio (#158, part 1 of 2 of #36)

The sim is silent. This adds sound to the flight: a tone per motor that follows the motor's eRPM and command, prop wash,
wind that follows airspeed, and a thud on a collision. It is also the cross-cutting proposal #36 asks for: the audio bus
layout, why the sound is procedural, and how its parameters travel. Part 2 (#159) puts the same sound into the DVR file
and is out of scope here, but this design is shaped so it can reuse every line of the synthesis.

## Intent

A pilot hears the quad: the motors' pitch is the quickest read on throttle and on a motor that is struggling, the wind
is the read on speed, and a hit has to be audible. The sound must come from the simulation's state, never from the
pilot's controls and never feeding back into the simulation.

## Goals

- Live audio while flying in the Godot client: per-motor tone (pitch from eRPM, loudness from the motor command), prop
  wash, wind (from airspeed), and a thud on each collision event.
- The synthesis is a deterministic pure function of state: the same state gives the same sound parameters, and the
  same parameter sequence gives the same samples, bit for bit.
- Audio stays outside the lockstep digest: it is derived from state on the client, like the FC telemetry, and nothing
  in `ofs-sim` knows it exists.
- A settings entry ("Sound") turns audio off; it persists like the other settings.
- Procedural only: no sample files, no licence question.

## Non-goals

- The DVR audio track and any muxing (#159). The pure core is where #159 will plug in; no muxer code lands here.
- Spatial audio, doppler, a pilot standing in the field hearing the quad from a distance. The microphone is on the
  quad (what the goggles' DVR would carry); the listener is the quad.
- Samples of real motors; a VTX buzzer or beeper (a possible later addition on the same bus); music.
- A volume slider, per-source mixing, an ambient-wind model in the simulation (there is none; see "Wind").
- Calibrated realism. The constants are estimates for a plausible 5 inch quad, kept in one struct and documented as
  such; they wait for a recording to tune against.

## Anti-goals

- Anything that makes two runs of the simulation differ, or that adds audio state to the bus, the recording or the
  digest. A synthesiser that reads the pilot's sticks would also fail: the sound is derived from the simulation's state.
- Heavy per-sample work in GDScript, or a Godot-only implementation that #159 would have to rewrite.

## Why procedural

The eRPM of each motor is already simulated, so the motor's sound is a function of numbers we have: pitch from rpm,
level from the command. A sample library would need licences (the question #36 raises), would loop audibly, and could
not follow four independent motors. A procedural source has no files to ship, is exact about pitch, and is testable:
the tests assert the spectrum rather than listen.

## How the parameters travel

**No protocol change** (stays 15): the `State` stream the client already receives carries `motor_rpm`, `motor_cmd`,
`velocity_ned_mps` and `collision_speed_mps`, and collisions already arrive as `EVENT_KIND_COLLISION` events. Audio is
derived on the client, never carried by the server, which is what keeps it out of the digest by construction (the
server crates do not depend on the audio crate, and a test guards that).

```
State stream (240 Hz)      ->  Telemetry (ofs-client, now with motor_rpm)
Collision event            ->  Audio::collision_event(time)        (resolved against the next state)
                                  |
Godot frame (~60 Hz)       ->  OfsClient.audio_render(frames)  ->  ofs_audio::params(input)   [pure]
                                                               ->  Synth::set_params, render   [deterministic DSP]
                                  PackedVector2Array  ->  AudioStreamGeneratorPlayback  ->  bus "Flight"
```

The low-rate derived stream is the sequence of `AudioParams` computed once per Godot frame (about 60 Hz, a quarter of
the state rate; the audio buffer is 0.1 s, so parameter changes are smoothed at sample rate inside the synth, never
stepped). `params` is computed from the newest telemetry at that moment; it is independent of the wall clock, so a
test or the DVR feeds it any sequence of states.

## The pieces

**`crates/ofs-audio`** (new, no dependencies, no Godot, no tokio, no I/O):

- `AudioInput`: the numbers that matter, flat and plain: `motor_rpm: Vec<f64>`, `motor_cmd: Vec<f64>`,
  `airspeed_mps`, `descent_mps` (positive when sinking), `active` (false while paused, in a menu, or with audio off).
  #159 builds it from the recorded bus (`motor.N.rpm`, `motor.N.cmd`, the body velocity, `body.collision_count`).
- `AudioParams` and `params(&AudioInput, &AudioConfig) -> AudioParams`: the pure function. One `MotorTone
  { freq_hz, gain }` per motor, `wash_gain`, `wind: { gain, cutoff_hz }`. All values are clamped and finite for any
  finite input; non-finite input maps to silence.
- `thud_gain(collision_speed_mps, &AudioConfig) -> f64`: the pure map from a collision's inward speed to a hit gain.
- `Synth` (deterministic DSP): additive tones (the fundamental and the blade-pass harmonics per motor), a seeded noise
  generator through a one-pole low-pass for wind and a band-pass-ish colouring for wash, and a decaying thud (a low
  sine plus a noise click). `set_params`, `trigger_thud(gain)`, `render(&mut self, out: &mut [f32])` (mono). Gains and
  frequencies slew toward their targets over about 20 ms; the noise generator is a fixed-seed xorshift, so the same
  calls give the same samples. Sample rate is `ofs_audio::SAMPLE_RATE` (44100), the single source of truth Godot reads.
- `Audio` (the facade the client owns): a `Synth` plus the pending collisions. `collision_event(time_s)` queues a
  thud; `frame(&AudioInput, state_time_s, collision_speed_mps, frames) -> Vec<f32>` resolves queued thuds against the
  state (a collision event arrives on its own stream and may beat the state that carries its speed, so a thud waits
  for the first state at or after the event's time, then sounds with that state's `collision_speed_mps`), sets the
  params and renders. `reset()` drops the queue and silences the synth (a reload).

**`ofs-client`**: `Telemetry` gains `motor_rpm` (a plain copy of `State.motor_rpm`); nothing else changes.

**`ofs-godot`**: `OfsClient` owns an `Audio`. `audio_render(frames, active) -> PackedVector2Array` (mono copied to both
channels), `audio_sample_rate() -> float`, collision events feed `collision_event`, a "loading" phase calls `reset`.
`get_telemetry` also reports `motor_rpm`.

**Godot** (`godot/`):

- `drone/flight_audio.gd`: a node with an `AudioStreamPlayer` on a `Flight` bus and an `AudioStreamGenerator`
  (`mix_rate = audio_sample_rate()`, buffer 0.1 s). Each `_process` it asks the client for the frames the generator
  has room for and pushes them. The bus is created in code under Master, so no `.tres` layout is needed.
- `scripts/settings.gd`: a `audio` bool, default true, `OFS_AUDIO` / `--audio`. A row "Sound" in the settings screen
  (live) and the shared `_on_setting_changed` path in `app.gd`, which persists it with the other settings and mutes
  the `Flight` bus. Audio is also `active = false` unless the phase is "flying", so a paused or menu session is silent.

## The bus layout (the proposal #36 asks for)

```
Master
  Flight      (the quad's microphone: motors, wash, wind, thuds)   mute = the "Sound" setting
  (later) Alerts   a VTX buzzer / beeper, if ever wanted
```

#159's DVR audio does not tap the live bus (a live bus carries the device's timing and dropouts and is silent in
headless runs). It renders the same `Synth` offline from the recorded state at the DVR's field times, so the file's
audio is as deterministic as its video, and the muxer takes PCM from `Synth::render`. That is why the DSP is in a
Godot-free crate with no knowledge of time other than the samples it is asked for.

## The sound design (constants are estimates, `AudioConfig`, one struct)

- **Motor tone.** Fundamental `f0 = rpm / 60`; the audible tone is the blade-pass frequency `f0 × blades`, blades = 3
  (the shipped 5 inch tri-blade) plus a weaker `f0` and a weaker second harmonic of the blade-pass. Gain per motor is
  `cmd^1.5 × motor_gain` (roughly the thrust curve, and silent for a disarmed quad with zero command), scaled by
  `1/sqrt(motors)` so four motors do not clip. A motor whose rpm and command disagree is audible as its own pitch:
  motors are independent voices. The shipped tone is the blade-pass fundamental plus its 2x and 3x harmonics, without
  the `rpm / 60` component (a call made in implementation).
- **Prop wash.** Noise whose level rises with the mean command squared, with how hard the motors are being asked to
  differ (command spread, a stand-in for manoeuvring), and strongly when the quad descends through its own wash:
  `wash = mean² × (0.25 + 0.75 × clamp(descent / 5 m/s))` plus `0.5 × spread × mean`, clamped to `[0, 1]`.
- **Wind.** Noise whose gain follows `(airspeed / 25 m/s)^1.5` and whose low-pass cutoff opens with speed (300 Hz at rest
  to 3.5 kHz at 30 m/s and above). The simulation has no ambient wind, so airspeed is the speed over the ground; when
  a wind model lands, only the code that builds `AudioInput` changes to an air-relative velocity.
- **Thud.** A collision's gain is `clamp(speed / 12 m/s)^0.7`, with a floor so a scrape is audible: a 90 Hz sine
  decaying over about 120 ms plus a short filtered noise click, the same for a hard landing and for hitting an object
  (the event's message differs, the sound does not, in this part).
- **Silence.** `active = false`, no motors, or a non-finite input: every gain goes to zero (smoothly).

## Constraints

- `ofs-audio` has no dependencies and no I/O; `ofs-sim` and its crates must not depend on it (guarded by a test that
  reads their manifests).
- Files stay LF. Godot tests print `0 failed` even on a script error, so the full output is read.
- Godot headless has no audio device (its dummy driver still takes frames), so the Godot tests check the parameters
  that go to the generator and the setting's effect; sound itself is checked in Rust.

## Rulings (my calls, open to change)

1. No protocol bump: the existing state and events carry what audio needs. Protocol stays 15.
2. The DSP lives in Rust (`ofs-audio`), not in GDScript: four motors at 44.1 kHz per-sample in GDScript is too slow, and
   #159 needs the same renderer offline.
3. The "low-rate derived stream" is the per-frame `AudioParams` derived on the client from the state stream, not a new
   RPC or a new `State` field. An RPC would move audio into the server's world for no gain.
4. Thuds wait for a state at or after the event time and take their strength from that state's `collision_speed_mps`,
   rather than parsing the event's message text.
5. On/off only, no volume setting; one `Flight` bus. Mono (the quad's microphone) copied to both channels.
6. Hard landing and object hit sound alike; collision damage (#157) can later vary the thud by object.
7. Airspeed is ground speed until the simulation has wind.

## Done when

- `cargo test -p ofs-audio` proves: the same input gives equal params; the same param/thud sequence gives identical
  samples; a motor's tone peaks at its blade-pass frequency and moves with rpm; wind gain and cutoff rise with
  airspeed; prop wash rises with descent; silence for `active = false` and non-finite input; a thud is audible and
  decays; a collision event waits for its state; `ofs-sim` does not depend on `ofs-audio`.
- `bash scripts/run-godot-tests.sh` passes: the `audio` setting resolves from file, environment and flag; the settings
  screen has the row and emits `audio`; the flight-audio node pushes frames when on and none when off; the bus is
  muted by the setting.
- The CHANGELOG, README (settings) and `docs/architecture.md` (the `ofs-audio` crate and its place) say it.
- A manual check on a machine with speakers is listed in the report (headless has no audio output): pitch follows
  throttle, wind rises with speed, the thud on a crash, the setting silences it.
