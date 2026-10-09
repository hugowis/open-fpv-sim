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
| Body shadow | up to 8 dB when the pilot is ahead of and below the quad (the frame, stack and battery are between the VTX antenna and the goggles) | `BODY_SHADOW_DB` = 8 |
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

## Link budget check

25 mW (14 dBm) with 2 dBi RHCP omnis at both ends, at their peak gain, with the body shadow, the ground bounce and
fading off, falls to 8 dB SNR at 580 m; 600 mW reaches about 3 km (a unit test pins both). In the shipped world
(diversity goggles: an omni and an 8 dBi patch), with the quad on the ground straight ahead of the pilot:

| VTX power | Clean to | Sync lost by |
|---|---|---|
| 25 mW | about 100 m | 2 km |
| 200 mW | about 300 m | 4 km |
| 600 mW | about 500 m | beyond 4 km |

("Clean" is SNR ≥ 25 dB, no grain at all; the picture stays easily flyable well beyond, with grain and then
sparkles.)

## Where it shows

- Bus signals: `video.snr_db`, `video.rssi_dbm.<antenna>`, `video.antenna`, `video.interference_dbm`,
  `video.noise`, `video.sparkles`, `video.chroma`, `video.sync`.
- The state stream: `State.video` (protocol 4), and the events `video_lost` and `video_restored`.
- The game: the HUD's `VID` line, and the shader of `godot/ui/video.gd` between Betaflight's OSD and the HUD, so the
  OSD breaks up with the picture.
