# The ExpressLRS link model

How the simulator decides whether a control packet reaches the flight controller, from the handset on the ground to
Betaflight's UART. The code is `crates/ofs-rf/src/propagation.rs` and `crates/ofs-rf/src/fading.rs` (the RF physics,
shared with the video link) and `crates/ofs-radio/src/elrs.rs` (the link itself, a model on the bus). The design is
`docs/superpowers/specs/2026-10-10-m3c-world-collision-elrs-design.md`; the propagation effects this link reuses are
described in `docs/research/video-link.md` and not repeated here.

Every constant below carries its provenance: **ExpressLRS published**, **physics**, **CRSF specification** or
**estimated** (nothing is measured). They sit in one place each, named, so they can be replaced.

## Once per packet (the model runs at the packet rate)

1. The handset samples the sticks into 16 channels — roll, pitch, throttle, yaw in Betaflight's default AETR order,
   then AUX1..4, the rest centred — and queues them; the pipeline holds the samples back by `latency_packets`
   packets (default 1), so what the receiver outputs now is what the pilot set one packet ago.
2. For each receiver antenna on the quad, the packet's RSSI:

   `RSSI = Ptx + Gtx(direction) + Grx(direction) - FSPL(d, 2440 MHz) - polarization loss - body shadow - obstruction`,
   with the ground bounce added by phase and a Rician fade on top (`ofs_rf::propagation::path_gain`,
   `ofs_rf::fading::Fader`). The quad's antenna axes ride the body attitude (`mount_frd`); the handset's are aimed
   in the world file's `[handset]`, relative to the pilot's facing.
3. Diversity picks the antenna with the best RSSI (it changes only for one 2 dB better).
4. One uniform draw against the packet error rate decides reception. A received packet writes one CRSF RC frame to
   the flight controller's UART and counts into the link quality; a LINK_STATISTICS frame follows every
   `link_stats_interval_packets` received packets (default 50). When packets stop the receiver goes silent, as
   ExpressLRS does by default, and Betaflight's own failsafe takes over.
5. The downlink is the same path reversed, the receiver transmitting at 100 mW with its own fader and its own
   100-packet window; it feeds the LINK_STATISTICS downlink fields only.

## What the 2.4 GHz link shares with the 5.8 GHz video link

Path loss, the dipole and patch patterns, the polarization losses, the knife-edge obstruction (a golden-section
search over each object's signed distance, capped at the object's `rf_loss_db`), the two-ray ground bounce, the
Rician fading and the body shadow are the video link's, moved unchanged into `ofs-rf`. Only the numbers differ:

| Effect | At 2440 MHz | Constant |
|---|---|---|
| Free-space path loss | `20 log10(d m) + 20 log10(2440) - 27.55` | 80.2 dB at 100 m |
| Body shadow | the same lobe (forward and down, through the frame, stack and battery) | `BODY_SHADOW_DB` = 8 dB |
| Ground bounce | two rays with their phase difference; the antennas here are linear, so the reflected ray arrives without the video link's circular cross-polarization penalty and low flight shows deep fades | — |
| Fading | Rician, an AR(1) complex Gaussian scatter correlated over the distance the quad moves | λ/2 = 6.1 cm (physics); K = 10 dB with line of sight, lowered dB for dB by the obstruction loss |

Notes:
- **Frequency.** 2440 MHz, the middle of the 2.4 GHz band (**estimated**): frequency hopping is not modelled, so one
  frequency stands for the whole hop sequence.
- **Correlation length.** λ = c/f = 12.3 cm at 2440 MHz, so the fade decorrelates over half that, 6.1 cm of
  movement (**physics**): a hovering quad's link is steady, a fast one's flickers.
- **Polarization.** The shipped antennas are linear at both ends (the handset's default is a vertical 2 dBi dipole,
  the quad's a 2 dBi dipole leaning up and back), so banking the quad across the handset's dipole costs
  `-20 log10|cos Δ|`, capped at 20 dB — at 5.8 GHz the same flight keeps a circular link. A crossed linear pair
  (a quad antenna along the path, or a horizontal handset dipole under a level quad) takes the full 20 dB.

## The receiver

| Constant | Value | Provenance |
|---|---|---|
| Frequency | 2440 MHz (band middle, hopping not modelled) | estimated |
| LoRa bandwidth | 812.5 kHz | ExpressLRS published |
| Noise figure | 6 dB | estimated |
| Noise floor | `-174 + 10·log10(812 500) + 6 = -108.9 dBm` (the code keeps all the digits: -108.90176630349089) | computed from the two above |
| Sensitivity at 50 Hz | -117 dBm | ExpressLRS published |
| Sensitivity at 150 Hz | -112 dBm | ExpressLRS published |
| Sensitivity at 250 Hz | -108 dBm | ExpressLRS published |
| Sensitivity at 500 Hz | -105 dBm | ExpressLRS published |
| PER curve width | 1 dB | estimated |
| Downlink TX power | 100 mW | estimated |
| LQ window | the last 100 packets, per direction | ExpressLRS published (the definition of LQ) |
| RSSI while no packet is heard | -130 dBm | estimated |

Notes:
- **Packet error rate.** `PER = 1 / (1 + exp((RSSI - sensitivity) / 1 dB))`: half the packets decode at the
  sensitivity, under 1 % five dB above it, effectively all of them are lost 15 dB below.
- **SNR.** `SNR = RSSI - noise floor`. LoRa decodes below the noise, so at the sensitivities the SNR is negative:
  -8.1 dB at 50 Hz, +3.9 dB at 500 Hz. This is the SNR the LINK_STATISTICS frame reports and the HUD shows.
- **Link quality.** LQ is the share of the last 100 packets received, uplink and downlink separately; the link
  counts as up while any packet in the uplink window arrived.

## Protocol mappings

The receiver's LINK_STATISTICS frame carries the mode and the power as CRSF indices:

| Packet rate | `rf_mode` |
|---|---|
| 50 Hz | 1 |
| 150 Hz | 2 |
| 250 Hz | 3 |
| 500 Hz | 4 |

| Handset power | `uplink_tx_power` |
|---|---|
| 10 mW | 1 |
| 25 mW | 6 |
| 50 mW | 10 |
| 100 mW | 13 |
| 250 mW | 17 |
| 500 mW | 20 |
| 1000 mW | 23 |

(CRSF specification. A power between two rows takes the lower row's index: 15 mW reports as the 10 mW row.)

## Diversity

Receiver diversity (one or two antennas in the quad file's `[[radio.antennas]]` and the world file's
`[[handset.antennas]]`) changes antenna only when the other one is better by 2 dB
(`ofs_rf::fading::Diversity`), so a fade has to persist to switch. The handset transmits on its *active* antenna —
the one it receives best — and the quad answers on the antenna it receives best. Real dual-antenna handsets
alternate per packet instead; this is a modelling simplification (see `docs/superpowers/m3c-carried-debt.md`).

## Link budget check

The bare budget, 2 dBi dipoles at both ends at peak gain and co-polarized, body shadow, ground bounce and fading
off (a unit test pins both rows):

`RSSI = 10·log10(Ptx/mW) + 2 dBi + 2 dBi - FSPL(d, 2440 MHz)`

- **250 mW at 500 Hz** (24 dBm) reaches its -105 dBm sensitivity where the path loss is `24 + 4 + 105 = 133 dB`:
  `20·log10(43 600) + 20·log10(2440) - 27.55 = 92.8 + 67.7 - 27.55 = 133.0` — about **43.6 km**.
- **10 mW** (10 dBm): `FSPL = 119 dB`, and `20·log10(8 700) + 67.7 - 27.55 = 78.8 + 67.7 - 27.55 = 119.0` —
  about **8.7 km**.

Each halving of the power costs 3 dB, about 30 % of the range; dropping from 500 Hz to 50 Hz buys 12 dB of
sensitivity, about four times the range. The shipped world is a few hundred metres across, so there the link is
lost to buildings and polarization, never to free space — 50 km out at 10 mW (a test pose, 15 dB below the 500 Hz
sensitivity) not a single packet survives.

## Where it shows

- Bus signals: `radio.rssi_dbm` (active antenna), `radio.snr_db`, `radio.lq`, `radio.link_up`, `radio.antenna`,
  `radio.downlink_lq`.
- CRSF on the flight controller's UART: one RC frame per received packet, LINK_STATISTICS every N received packets;
  silence when they stop.
- The state stream: `State.radio` (protocol 5: `snr_db`, `active_antenna`, `downlink_lq_pct`), and the events
  `LINK_UP` / `LINK_DOWN` on the up→down transition.
- The game: the HUD's `LINK` line (`LINK UP   LQ 100 %   -62 dBm   SNR 9`); Betaflight's own OSD LQ element follows
  the link, and the failsafe is Betaflight's.
