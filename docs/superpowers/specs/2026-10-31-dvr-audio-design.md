# Audio in the DVR (#159, part 2 of 2 of #36)

The DVR records a silent MJPEG AVI. This gives the MP4 export an AAC audio track: the flight sound of part 1
(#158), synthesised offline from what the recording saw. The issue is the design intent; this document rules on
what it left open. Parent epic: #160, whose rulings bind: audio is derived from state and is not in the lockstep
digest; the video stays analog.

## Intent

A recorded flight sounds like the flight: motors that follow the throttle, wind that follows speed, the thud of a
crash. Exporting the same recording twice gives the same audio track, because the synthesis is a pure function of
recorded numbers, never of the wall clock, an audio device, or the live bus.

## Goals

- The MP4 export carries an AAC track (ffmpeg beside libx264, from `PATH`, as it already is) and the clip plays
  with sound.
- The same recording gives the same audio samples on every export (a test says so).
- The recording is the single source of truth: audio is derived from data stored next to the AVI, and exporting
  never needs a running session.
- The sound is part 1's: the same `params`, `thud_gain`, `Synth` and `Audio` facade, so a live session and an
  export agree for the same state.

## Non-goals

- Audio in the AVI itself, or an audio track in the image-sequence export (which stays silent, unchanged).
- A setting to turn DVR audio on or off, a volume, or a separate audio file export.
- The VTX buzzer (see "Rulings", 6).
- Re-timing the sound to the goggles' latency (see "Known limits").
- Changing the live audio behaviour, the protocol (stays 15), or `ofs-sim` and its crates.

## Anti-goals

- A recording whose audio length drifts from its video length: a dropped video frame must not leave its sound behind.
- An export that overwrites anything, or leaves a temporary file behind.
- Audio state in the server or the digest.

## How the audio inputs travel

The AVI carries no state, so the recording needs the numbers the synthesis reads. While a feed records, the client
appends them to a **sidecar** `<name>.audio` next to the AVI (`REC_<ts>.audio` for `REC_<ts>.avi`,
`REC_<ts>_clean.audio` for the clean feed), one record per video frame the writer accepted, plus one per collision
event:

```
ofs-dvr-audio 1
C <event_time_s>
F <state_time_s> <collision_speed_mps> <airspeed_mps> <descent_mps> <n> <rpm_1..rpm_n> <cmd_1..cmd_n>
```

Text, LF, every number a Rust `f64` `Display` (shortest form that round-trips exactly, so a parse gives back the same
bits). `F` is the flight state when the frame was written (the newest telemetry, built into an `AudioInput` by the
same function the live audio uses: `Telemetry::audio_input`); a frame written before any telemetry has arrived is
written as `F` with `NaN` time, no motors and zero speeds, which replays as silence. `C` is a `Collision` event as
the client's `poll` sees it, written before the frame of that poll.

```
state stream -> Telemetry (newest)     collision events (Client::poll)
        \                                   /
   dvr_write_frame(jpeg): writer accepts frame -> append F        append C (every recording feed)
                                  |
                      REC_x.avi  +  REC_x.audio           (the recording)
                                  |   export (mp4)
        replay: Audio::new(); for each record: C -> collision_event, F -> frame(.., 882 samples)
                                  |   s16le mono PCM, 44100 Hz, in a temp file
        ffmpeg -i REC_x.avi -f s16le -ar 44100 -ac 1 -i pcm -c:v libx264 ... -c:a aac -shortest REC_x.mp4
```

The replay is exactly what the live path does: `Audio::frame` resolves a collision against the first state at or
after the event, and the replay feeds records in the order they were logged. One frame at 50 fps is
`44100 / 50 = 882` samples exactly, so the audio is exactly as long as the accepted video frames and stays in step.

## The pieces

**`ofs-client`** (gains a dependency on `ofs-audio`; `ofs-sim` and its crates do not):

- `Telemetry::audio_input(&self, active: bool) -> AudioInput` (`model.rs`): the one place the telemetry becomes an
  audio input (motor rpm and command, ground speed as airspeed, descent from the climb rate). `ofs-godot`'s
  `audio_render` calls it instead of building the struct inline: same values, so the live behaviour is unchanged.
- `dvr_audio.rs`: `AudioLog` (create the sidecar with `File::create_new`, append `C` and `F` records through a
  `BufWriter`, `finish` flushes), `write_pcm(sidecar, out) -> Result<u64 frames>` (the replay above, writing 16-bit
  little-endian samples to a `Write`, one chunk per frame so memory stays flat), and the parser. A truncated last
  line (a crash mid-write) is ignored; any other malformed line, or an unknown header, is an error naming the line.
  Samples are `round(clamp(x, -1, 1) * 32767)` as `i16`.
- `dvr.rs`: `DvrWriter::write_frame` returns whether the frame was accepted (it was dropped when the writer queue is
  full); existing callers ignore it.
- `client.rs`: each feed's recording pairs its `DvrWriter` with an `AudioLog`. `dvr_start` creates the AVI (which refuses an
  existing file, as today), then the sidecar (and removes the AVI if the sidecar cannot be created, so a refused start
  leaves nothing); `dvr_write_frame` appends the `F`
  record when the writer accepted the frame; `poll` appends `C` for each collision event to every recording feed;
  `dvr_stop` finishes the log.
- `dvr_export.rs`: `export_mp4` looks for `<stem>.audio` beside the AVI. If it exists, the replay is written to a
  temp file next to the output (`.<stem>.pcm-<pid>.tmp`, created with `create_new`, removed whatever happens), and
  `ffmpeg_args` adds the second input, `-c:a aac -b:a 128k` and `-shortest`. No sidecar (a recording from before this
  change, or a hand-made AVI): the MP4 is exported silent, as before. A malformed sidecar is an error and no MP4 is
  made (the AVI and the sidecar are kept, so the user can delete the sidecar for a silent export).

**`ofs-godot`**, **Godot**: no new API. `dvr.gd` keeps calling `dvr_write_frame`; the audio rides on that call in
Rust. `_free_stem` is unchanged (the `.audio` file shares the AVI's stem, and a stem is free only if the AVI is, as
today). **`ofs-audio`** is not changed.

## Rulings (my calls, open to change)

1. **Where the inputs come from:** a sidecar written during the recording, not a re-run of the simulation and not
   the inputs/blackbox log. The AVI is the recording; the sidecar travels with it.
2. **Reuse:** the replay uses `Audio::frame` unchanged, fed the same `AudioInput` the live path builds, so live and
   exported sound agree for the same state. Records are the unprocessed inputs rather than synthesis parameters,
   so a later retuning of `AudioConfig` improves old recordings too.
3. **Sampling time:** the state is the newest telemetry at the moment the frame is written, as for the live sound.
   The sound is the quad's microphone; the picture lags it by the goggles' latency (default 40 ms).
4. **Independent of the live "Sound" setting:** muting the speakers does not silence the recording (as muting the
   goggles' speaker does not stop a real DVR hearing its microphone). No new setting.
5. **Held fields:** a dropped video field repeats the held picture but the sound keeps going (the microphone is not
   on the analog link).
6. **The VTX buzzer does not fit.** The simulation has no buzzer or beeper model: nothing on the bus says when the
   flight controller beeps, and a VTX has no buzzer. Faking beeps would be invented content, so the track is the
   flight sound only. If a beeper model ever exists it adds records and a voice to the synthesis; the sidecar
   version number is there for that.
7. **Format:** AAC-LC 128 kb/s mono at 44.1 kHz in the MP4. Mono is the microphone; players put it on both speakers.
   The PCM goes through a temp file (an hour of flight is about 320 MB briefly, a five minute flight 26 MB), not a
   pipe: it keeps `-nostdin` and a plain `output()`.
8. **The AVI's name decides:** the sidecar is created (truncating) only after its AVI was newly created, so a leftover
   `<name>.audio` whose AVI was deleted never blocks a start (the Godot tests reuse fixed names). `_free_stem` and the
   never-overwrite rule for the AVI are unchanged. The sidecar is never modified after the recording stops.
9. **Determinism scope:** the PCM is a pure function of the sidecar bytes on one platform. Sin/exp/tanh come from
   the platform libm, so PCM is not byte-identical across operating systems, and the AAC bytes also depend on the
   ffmpeg build; tests compare PCM from the same machine and never carry golden audio files. The AAC track of two
   exports on one machine and one ffmpeg is identical (checked by hand with ffmpeg's md5 muxer, not in CI).
10. **Protocol unchanged (15).**

## Known limits

- The sound leads the picture by the goggles' latency (ruling 3); a retiming would need telemetry history at the
  render instant. Not done.
- A recording made by a crash has a truncated sidecar tail and an AVI the crash may have cut at a different frame;
  `-shortest` ends the clip at the shorter. The export of an unfinalized AVI already works and stays.
- The temp file is up to hundreds of MB for hour-long recordings.
- Nothing here is checked by ear or eye: headless Godot has no audio device and no display. Playing the clip in VLC
  and comparing sound to picture is a human check.

## Done when

- `cargo test -p ofs-audio -p ofs-client` passes in WSL, including: the log round trip is exact; the replay of one
  sidecar twice gives identical PCM; a collision record gives a thud and a silent state gives silence; the PCM length
  is `882 x frames`; a truncated tail is tolerated and a bad line is an error; `Telemetry::audio_input` matches what
  `audio_render` used; a client recording writes one `F` per accepted frame and `C` for collisions; `ffmpeg_args` has
  the audio inputs only when asked; the export with an unreadable sidecar fails before spawning ffmpeg; the image
  export is untouched; `ofs-sim` still does not depend on `ofs-audio`.
- With the WSL ffmpeg: an exported MP4 has an AAC stream of the video's length, and two exports of one recording
  give the same audio md5.
- `bash scripts/run-godot-tests.sh` passes (the DVR tests still pass; the sidecar next to a recording is checked).
- CHANGELOG, `docs/dev-setup.md` (DVR section), `docs/roadmap.md` ("No audio" removed) and `docs/architecture.md`
  say it.
