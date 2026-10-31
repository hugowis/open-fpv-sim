# The analog video link model

How the simulator decides what the pilot's goggles see, from the VTX on the quad to the picture in the FPV view.
The code is `crates/ofs-video/src/propagation.rs` (the physics, pure functions) and `crates/ofs-video/src/link.rs`
(the receiver, a model on the bus). The design is `docs/superpowers/specs/2026-10-09-m3b-video-link-design.md`.

Every constant below is an **estimate** for a typical 5.8 GHz analog setup, not a measurement. They sit in one place
each, named, so they can be replaced by measured values.

## Once per PAL field (50 Hz)

1. Read the quad's position and attitude, and the VTX's frequency, power and pit mode (`vtx.*`, published by the
   SmartAudio VTX of M3a). In pit mode the power is the quad file's `vtx.pit_power_mw` (default 0.1 mW).
2. For each goggle antenna in the world file, compute the received power:

   `P = Ptx + Gtx(direction) + Grx(direction) - FSPL - polarization loss - body shadow - obstruction`, with the
   ground bounce added by phase and a fading term on top.
3. Add the other emitters' power, reduced by the receiver's channel filter, to the noise floor; the SNR of each
   antenna is its signal over that.
4. Diversity picks the antenna with the best SNR (it changes only for one 2 dB better).
5. The receiver turns that SNR into a picture (grain, sparkles, colour) and a sync state.

## Propagation

| Effect | Model | Constant |
|---|---|---|
| Free-space path loss | `20 log10(d m) + 20 log10(f MHz) - 27.55`; d at least 1 m | 87.7 dB at 100 m, 5800 MHz |
| Omni antenna | dipole doughnut `gain + 20 log10(sin θ)` from its axis | floor 20 dB below the peak |
| Patch antenna | `cos^n` main lobe, n set so the gain is -3 dB at half the beamwidth | back lobe 20 dB below the peak |
| Polarization | same-hand circular 0 dB, opposite hands 20 dB, circular to linear 3 dB, linear to linear `-20 log10|cos Δ|` | cap 20 dB |
| Body shadow | up to 8 dB when the pilot is ahead of and below the quad (the frame, stack and battery are between the VTX antenna and the goggles); the lobe is wide, so a level quad flying straight at the pilot already takes 6.75 dB of it | `BODY_SHADOW_DB` = 8 |
| Obstruction | ITU-R P.526 single knife edge, `J(v) = 6.9 + 20 log10(√((v-0.1)²+1) + v - 0.1)` for v > -0.78, capped at the object's `rf_loss_db` | buildings 20-30 dB in `worlds/flat.toml` |
| Ground bounce | two rays: the direct one and one reflected off the ground (coefficient -1), added with their phase difference | — |
| Fading | Rician, an AR(1) complex Gaussian scatter correlated over λ/2 of quad movement | K = 10 dB with line of sight, minus the obstruction loss |

Notes:
- **Obstruction.** The knife edge is the deepest point of the straight path in the object (or the closest it
  passes), found by a golden-section search on the object's signed distance, which is exact for the convex shapes of
  the world file (boxes, cylinders) however thin the object or long the path. An object standing on the ground
  continues below it, so a signal diffracts over its top or around its sides, never underneath. Going behind a
  building fades the picture over a few metres instead of switching it off.
- **Ground bounce and polarization.** A reflection reverses the hand of a circular wave, so a circular receiver
  takes the reflected ray with the 20 dB cross-polarization loss: a ripple under 1.5 dB. With linear antennas the
  two rays are nearly equal and cancel into deep fades when flying low. This is why FPV uses circular antennas.
- **Fading.** A hovering quad sees a steady signal (no movement, no change); fast flight flickers.

## Terrain

With a `[terrain]` section the path also takes the heightfield's knife-edge loss, from the same `Terrain` grid the physics
samples and with the same code as the ELRS link (see "Terrain" in `docs/research/elrs-link.md` for the maths: the Fresnel
parameter `v` maximised along the ray, `J(v)` capped at 60 dB, the clearance early-out). It applies to the wanted
signal and to the interference paths alike, and it adds to the objects' losses. At 5800 MHz (λ = 5.17 cm) an 8 m ridge
20 m from both ends gives `v = 15.7` and **36.8 dB**; at 100 m from both ends, `v = 7.04` and 29.8 dB. Whole chain on
`worlds/hills.toml`, the quad hovering 1.5 m over the ground at the default power: 110 m out in clear view the SNR is
14.8 dB; 167 m out behind a hill it is **-3.1 dB** (picture lost; the same spot without the relief: 18.0 dB).
The limits are the ELRS link's: the ground bounce stays on the plane d = 0, one edge only, and until #155 the Godot
client draws a flat ground, so the picture can be lost behind a hill it does not show.

## Interference

Each emitter of the world file goes through the same propagation (without fading), then the receiver's
adjacent-channel rejection by frequency offset: 0 dB on channel, 10 dB at 20 MHz, 25 dB at 40 MHz, 40 dB at 60 MHz
and beyond, linear in between. The next Raceband channel (37 MHz away) is rejected by 22.75 dB.

`SNR = P - 10 log10(10^(N/10) + Σ 10^(I/10))`, with the noise floor N = -93 dBm by default (thermal noise in about
20 MHz plus an 8 dB noise figure).

## The receiver

| SNR | Picture |
|---|---|
| ≥ 25 dB | clean |
| 25 → 12 dB | grain rises to half of full static |
| < 12 dB | sparkles (FM threshold clicks), full at 4 dB |
| < 8 dB | colour fades, gone at 5 dB |
| < 6 dB | sync unstable: tearing, line jitter |
| < 3 dB for 3 fields | sync lost: the picture rolls; static at 0 dB and below |
| ≥ 6 dB for 5 fields | a lost sync relocks |

Every number in that table is world data: the `[video]` table of a world file sets it, and a world without the table
(or a key) gets the value shown. All are estimates for a typical analog 5.8 GHz receiver; none is measured.

| Key | Default | Meaning |
|---|---|---|
| `clean_snr_db` | 25.0 | at or above this the picture is clean |
| `sparkle_snr_db` | 12.0 | grain reaches half of full static here; sparkles start |
| `sparkle_full_snr_db` | 4.0 | sparkles cover the picture at or below this |
| `color_fade_snr_db` | 8.0 | colour starts to fade below this |
| `color_lost_snr_db` | 5.0 | colour is gone at this |
| `unstable_snr_db` | 6.0 | below this sync is unstable; a lost sync relocks only above it |
| `lost_snr_db` | 3.0 | below this for `lost_after_fields` fields in a row, sync is lost |
| `lost_after_fields` | 3 | |
| `relock_after_fields` | 5 | fields in a row at or above `unstable_snr_db` that a lost sync needs |
| `field_drop_p_max` | 0.5 | the drop probability at `lost_snr_db`, ramping down to 0 at `unstable_snr_db` |

The world loader refuses values that would divide by zero or invert a ramp: every threshold finite,
`sparkle_snr_db > 0`, `clean > sparkle > sparkle_full`, `color_fade > color_lost`, `unstable > lost`, both field counts
at least 1 and `field_drop_p_max` within 0 to 1. The defaults live in `ofs-video`'s `link.rs` as constants and in the
world loader as `VideoSection::default()`; a test in `ofs-sim` keeps the two equal.

## Link budget check

25 mW (14 dBm) with 2 dBi RHCP omnis at both ends, at their peak gain, with the body shadow, the ground bounce and
fading off, falls to 8 dB SNR at 580 m; 600 mW reaches about 3 km (a unit test pins both). In the shipped world
(diversity goggles: an omni and an 8 dBi patch), with the quad on the ground straight ahead of the pilot:

| VTX power | Clean to | Sync lost |
|---|---|---|
| 25 mW | about 100 m | at 2 km in most cases (SNR about 0.5 dB: the fade decides); always at 4 km |
| 200 mW | about 300 m | borderline at 4 km (SNR about 3.5 dB, just above the 3 dB loss threshold: the fade decides) |
| 600 mW | about 500 m | beyond 4 km |

("Clean" is SNR ≥ 25 dB, no grain at all; the picture stays easily flyable well beyond, with grain and then
sparkles. The SNRs are for R1 with the parked quad of `worlds/flat.toml` raising the patch's noise floor to about
-92.3 dBm.)

## Timing

The link computes each field at the instant of reception, but the pilot sees the world as it was when the
picture was captured: the glass-to-glass latency of a real camera + VTX + air + goggles pipeline. Two M3e
additions, both estimates:

- **Latency.** `latency_ms` on the world file's `[receiver]` section (the goggles live there), default 40 and
  validated at most 1000, is quantized **down** to whole fields (20 ms at 50 Hz) — a pipeline that buffers
  fields delivers whole fields, and the simulator can only delay whole fields. The default 40 ms = 2 fields is
  an estimate for a typical analog setup: the camera's own pipeline, the VTX and the air, and the goggles'
  buffering, together two fields. `State.video.latency_ms` carries the quantized value.
- **Field drops.** At a marginal SNR the receiver fails to complete fields: the drop probability
  (`field_drop_probability`) is 0 at or above 6 dB (`UNSTABLE_SNR_DB`), ramps linearly to
  `FIELD_DROP_P_MAX` = 0.5 at 3 dB (`LOST_SNR_DB`) and is 0 outside that band — a locked link never drops, and
  a lost link outputs static, where a drop would mean nothing. The probability is an estimate; the draw is one
  seeded RNG draw per field, after the fading draws, so lockstep runs drop the same fields seed for seed. The
  draw is not made while the sync is already lost, and a dropped field never updates the sync tracker (the
  receiver is busy failing, not relocking). The result is the `video.field_valid` signal and
  `State.video.field_valid`; a dropped field leaves sync and the picture at their last values.

The client composites the whole picture from **one field-quantized instant**: the newest field boundary at or
before `sim_now − latency`. Pose, video quality and the OSD all sample that instant, so the picture ages as a
unit at the field rate — the judder of real analog glass. A dropped field at the render instant holds the
previous composite (pose, OSD and quality all freeze for one field, what a real receiver does with an
incomplete field); the client's FPV view carries `held_fields`, the count of field boundaries the shown
picture is behind.

## Where it shows

- Bus signals: `video.snr_db`, `video.rssi_dbm.<antenna>`, `video.antenna`, `video.interference_dbm`,
  `video.noise`, `video.sparkles`, `video.chroma`, `video.sync`, `video.field_valid`.
- The state stream: `State.video` (protocol 7; also `field_valid` and `latency_ms`), and the events
  `video_lost` and `video_restored`.
- The game: the HUD's `VID` line, and the shader of `godot/ui/video.gd` between Betaflight's OSD and the HUD, so the
  OSD breaks up with the picture.
