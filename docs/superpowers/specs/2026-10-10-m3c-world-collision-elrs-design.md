# M3c: world collision and ELRS on the shared propagation — design

Status: design approved in conversation 2026-10-10, awaiting written-spec review. Parent spec: `2026-10-04-open-fpv-sim-design.md` (§4.2, §5.3, §9). Predecessor: `2026-10-09-m3b-video-link-design.md` (merged into main as 40ac096), whose world file and propagation module this milestone builds on. Carried debt it closes: "Collision with world objects" and "ELRS on the shared propagation" in `docs/superpowers/m3b-carried-debt.md`.

## 1. Purpose and scope

M3 (video) is complete after M3b. The deferred features of M3a and M3b are split into three follow-up milestones, each with its own spec, plan and merge:

| Milestone | Contents | Why this order |
|---|---|---|
| **M3c (this spec): the world matters** | Collision with the world file's objects; the ELRS control link on the shared propagation model. | Builds directly on M3b's world file; the most visible gain; no Betaflight patching. |
| **M3d: flight-controller telemetry** | CRSF telemetry back to the radio (the SITL atomic-block shim from M0 §5); DShot telemetry emulation so per-motor eRPM reaches Betaflight's RPM filter. | Riskiest (SITL patching, a motor-protocol change); starts with a spike. |
| **M3e: video extras** | Tramp, video latency and frame drops, DVR, HD/digital OSD (font page 1), the OSD aspect-ratio setting. | Least core; trimmed in its own design conversation. |

**Success looks like:** flying the Godot client against real Betaflight, the quad bounces off a building wall, clips a gate post and tumbles, lands on the launch pad or on a roof, and a fast touchdown on the ground raises a hard-landing toast. A Python script gets a `COLLISION` event naming the object and the impact speed. The ELRS link's RSSI, SNR and LQ come from geometry: they fall with range, behind buildings and when the quad's linear dipole is cross-polarized to the handset's; Betaflight's OSD LQ element follows; a weak link (10 mW, far away) makes Betaflight fail safe on RX loss. The same seed in lockstep gives the same contacts and the same link.

### Decisions made in the design conversation

| # | Decision | Alternative rejected |
|---|---|---|
| 1 | Split the deferred work into M3c, M3d and M3e (table above). | One large M3c with all four areas. |
| 2 | A hit is **physics plus an event**: contact forces, a `COLLISION` event with object and speed, a HUD toast; Betaflight's own crash logic reacts by itself; no damage. | Physics only; physics plus prop damage (overlaps the M4 fault catalog). |
| 3 | **2.4 GHz ELRS only**, the packet rate setting the sensitivity (the LoRa modes). | 2.4 GHz and 900 MHz. |
| 4 | **Impulse contact on spheres** against the world shapes' signed distance; the ground keeps its M1 spring-damper for the landing contact points. | A penalty spring-damper for objects (a sphere sinks far enough at 20–30 m/s to pass a 12 cm post); a physics engine (Rapier). |
| 5 | A new **`ofs-rf` crate** holds the shared RF code; `Shape` moves to `ofs-core`. | Leave it in `ofs-video` and have `ofs-radio` depend on `ofs-video`. |

### Out of scope

900 MHz; ELRS dynamic power and frequency hopping; Wi-Fi or other 2.4 GHz interference; crash damage (M4 faults); CRSF telemetry to the radio (M3d); non-flat terrain; rotated boxes; a frequency-dependent `rf_loss_db` (one number per object for 2.4 and 5.8 GHz); collisions between objects other than the quad.

## 2. Code moves

- **`ofs-core::shape`**: `Shape` (axis-aligned box, vertical cylinder), `rooted`, `signed_distance`, and a new `normal` (the gradient of the signed distance, analytic per shape, unit length) and `bounds` (an axis-aligned bounding box). Physics and RF both use it.
- **`ofs-rf`** (new crate, depends on `ofs-core`): `ofs-video::propagation` moves here unchanged (FSPL, antenna patterns, polarization, knife-edge obstruction, ground bounce, adjacent-channel rejection), together with the Rician fader, the diversity switch (`Diversity`, 2 dB hysteresis) and `body_shadow_db` from `ofs-video::link`. `ofs-video` and `ofs-radio` depend on it.
- The move is behaviour-neutral: every M3b test passes unchanged except for import paths.

## 3. Configuration

### The quad file, schema 3 → 4

Fields are removed, so the schema version moves; a schema-3 file is a load error that names what changed (`radio.rssi_dbm`, `radio.snr_db` and the loss parameters are gone; `radio.tx_power_mw` and `radio.antennas` replace them).

**`[radio]`** (`kind = "elrs"`):
- `packet_rate_hz`: one of 50, 150, 250, 500 (the ExpressLRS 2.4 GHz LoRa modes); anything else is a config error.
- `tx_power_mw`: the handset's power, one of 10, 25, 50, 100, 250, 500, 1000 (default 250).
- `uart`, `latency_packets`, `link_stats_interval_packets` stay.
- Removed: `rssi_dbm`, `snr_db`, `loss_good`, `loss_bad`, `p_good_to_bad`, `p_bad_to_good`.
- `[[radio.antennas]]` (1 or 2; two means receiver diversity): `name` (unique), `kind` (`omni`, i.e. a dipole), `gain_dbi`, `polarization` (`linear`, `rhcp`, `lhcp`), `mount_frd` (the antenna axis in the body frame). Default: one 2 dBi linear dipole leaning up and back, `mount_frd = [-0.5, 0.0, -1.0]`.

**`[collision]`** (new, optional):
- `restitution` (default 0.3), `friction_coeff` (default 0.5).
- `spheres_frd_m`: a list of `[x, y, z, radius]` in the body frame. Default (generated at load): 6 spheres of radius 1.5 cm evenly spaced on each prop's tip circle (centred on the motor, radius `prop.diameter_m / 2`, in the motor plane) plus one body sphere of radius 4 cm at the centre of mass. On the shipped quad no gap between them admits a 12 cm post, and they sit above the landing contact points, so the quad rests on its legs.
- Validation: restitution in [0, 1], friction ≥ 0, radii > 0, finite numbers.

### The world file (schema stays 1)

**`[handset]`** (new, optional): `position_ned_m` (default: the pilot's position 0.5 m lower) and `[[handset.antennas]]` (1 or 2, same fields as the quad's radio antennas, plus `aim_az_deg`/`aim_el_deg` relative to the pilot's facing like the goggle antennas; default: one vertical 2 dBi linear dipole). The built-in open field gets the defaults.

**`[[objects]]`** keep their fields. Every object collides; there is no per-object opt-out.

## 4. Collision (`ofs-physics`)

The rigid body receives the world's shapes (`ofs-core::shape`, as given in the world file, not rooted) when built.

**Landing contact points** (the quad file's existing `contact_points_frd_m`): the M1 spring-damper and friction of `[ground]` now act against the ground plane **and** every world object. For an object, the depth is minus the signed distance of the point and the normal is the shape's `normal` there; the normal force uses the point's velocity along that normal, the friction its tangential velocity. This is how the quad lands on the pad or on a roof.

**Collision spheres** (`[collision]`): impulse contact against every world object and the ground plane, applied at the end of each step after integration:
1. For each sphere whose centre is closer than its radius to a shape (or to the ground), the contact point is the sphere's surface point along the normal, the penetration is `radius − signed distance`.
2. Position correction: the body is moved out along the deepest contact's normal by its penetration.
3. Normal impulse: the velocity of the contact point along the normal, if inward, is reversed and scaled by `restitution` (restitution 0 below an inward speed of 0.2 m/s, so nothing jitters at rest), with the impulse computed through the mass and the inertia tensor so that an off-centre hit turns the quad.
4. Friction impulse: Coulomb, at most `friction_coeff` times the normal impulse, opposing the tangential velocity of the contact point.
5. Contacts are resolved one after another, deepest first, in a fixed order, so the result is deterministic.

At 8 kHz the quad moves a few millimetres per step at racing speed, so no sphere passes through a 12 cm post between steps.

**Broad phase:** each object's bounding box is computed once; a sphere is only tested against objects whose box, grown by the quad's bounding radius, contains the quad's centre.

**Events and signals:**
- `EVENT_KIND_COLLISION` is raised when a sphere or a contact point starts touching an object or the ground with an inward speed of at least 1 m/s (named constant). The message gives the object's name (`ground` for the ground plane) and the speed. A touching object raises no further event until it has been out of contact for 20 ms. Gentle landings raise nothing.
- Bus: `body.collision_speed` (inward speed of the last event, m/s) and `body.collision_object` (index of the object, −1 for the ground) for the event's message.
- A non-finite value after a contact is caught by the existing per-step check.

## 5. The ELRS link (`ofs-radio::elrs` on `ofs-rf`)

**Per packet** (the model's rate is the packet rate):

**Uplink, for each quad antenna:**
`RSSI = Ptx + Gtx(dir) + Grx(dir) − FSPL(d, 2440 MHz) − polarization loss − body shadow − obstruction`, combined with the ground bounce and the fading term, exactly as the video link computes it (`ofs-rf`):
- 2440 MHz, the middle of the band; frequency hopping is not modelled.
- The handset antennas are at `[handset]`, aimed relative to the pilot's facing; the quad's antennas use the body attitude and `mount_frd`.
- Linear to linear polarization loss follows `−20·log10|cos Δ|` (capped at 20 dB), so banking and diving against a vertical handset dipole cost signal.
- Obstruction: the world objects' knife-edge losses, capped at each object's `rf_loss_db`.
- Ground bounce: the two-ray model; linear antennas take the reflected ray without the circular cross-polarization benefit, so low flight shows deep fades.
- Fading: a seeded Rician process per antenna, correlated over the distance the quad moves (λ/2 at 2440 MHz is 6.1 cm), K-factor lowered by obstruction as in M3b.

**Receiver:**
- Diversity: the antenna with the best RSSI, switching only when another is better by 2 dB; with one antenna, that one.
- Noise floor: `−174 + 10·log10(812 500) + 6 = −108.9 dBm` (LoRa bandwidth 812.5 kHz, noise figure 6 dB, estimated). `SNR = RSSI − noise floor`; LoRa decodes below the noise, so the SNR may be negative.
- Packet error: `PER = 1 / (1 + exp((RSSI − sensitivity) / 1 dB))`, with the sensitivity by packet rate: 500 Hz −105 dBm, 250 Hz −108, 150 Hz −112, 50 Hz −117 (ExpressLRS's published figures; the 1 dB slope is an estimate). One uniform draw per packet decides loss.
- Forced loss: `tx_enabled` false or the `radio_link_loss` fault, as now.
- LQ over the last 100 packets and silence on loss, as now; Betaflight's failsafe does the rest.

**Downlink** (the LINK_STATISTICS downlink fields only): the same path reversed, the receiver transmitting at 100 mW (named constant, estimated), its own fading process and its own 100-packet window.

**LINK_STATISTICS** (every `link_stats_interval_packets` received packets): `uplink_rssi_1`/`_2` per antenna, `active_antenna`, `uplink_lq`, `uplink_snr`, `rf_mode` from the packet rate (ExpressLRS's index), `uplink_tx_power` as the CRSF power index of `tx_power_mw`, and the downlink RSSI, LQ and SNR.

**Determinism:** both fading processes and the loss draws come from the model's own seeded stream, and every packet draws the same count of numbers whatever the inputs.

**Range check (a unit test):** 250 mW at 500 Hz, 2 dBi dipoles at both ends at peak gain and co-polarized, with fading, ground bounce and body shadow off: the RSSI reaches the sensitivity at 43.6 km (±5 %); 10 mW reaches it at 8.7 km (±5 %).

**Bus signals:** `radio.rssi_dbm` (active antenna), `radio.snr_db`, `radio.antenna`, `radio.lq`, `radio.link_up`, `radio.downlink_lq`.

## 6. Protocol and clients

**Protocol 5** (`PROTOCOL_VERSION = 5`; all clients move together):
- `RadioLink` gains `snr_db`, `active_antenna`, `downlink_lq_pct`.
- `State.collision_speed_mps`: the last collision's speed (0 until one happens).
- `EVENT_KIND_COLLISION = 13`; the message names the object and the speed.
- `World` gains `Handset` (position, antennas).

**Python:** `state.radio.snr_db`, `.active_antenna`, `.downlink_lq_pct`; `state.collision_speed_mps`; the event kind; `get_world()` includes the handset.

**`ofs-client` and the `OfsClient` node:** telemetry gains `radio_snr_db`, `radio_antenna`, `collision_speed_mps`; the world Dictionary gains the handset in Godot's frame.

**Godot:**
- HUD link line: `LINK UP   LQ 100 %   −62 dBm   SNR 47`.
- A toast on `COLLISION`: `HIT BuildingA 7.2 m/s` (`HARD LANDING 3.1 m/s` for the ground).
- Chase view: a handset marker beside the pilot tripod.

## 7. Order of work, failure handling and tests

**Order, riskiest first:**
1. Extract `ofs-core::shape` and `ofs-rf`; all M3b tests green unchanged.
2. Collision in `ofs-physics` with analytic tests.
3. The ELRS link on `ofs-rf` with analytic tests.
4. Quad schema 4, `[collision]`, world `[handset]`, wiring in the vehicle, protocol 5, server events, Python.
5. `ofs-client`, the Godot HUD line, the toast and the handset marker.
6. Docs, carried debt, CI.

**Failure handling:** the new validation errors of §3 are config errors at load, collected with file and field. Collisions and link loss are drone behaviour: events, never errors.

**Tests:**
- **`ofs-core::shape`:** `normal` is the unit gradient on each face, edge and the cylinder's rim and caps; `bounds` contain the shape.
- **Collision (unit):** a sphere dropped on a box bounces to `restitution²` of its height (±5 %); a quad at 30 m/s into a 12 cm post never ends up past it; a quad resting on the pad and on a roof stays still (no drift, no jitter above 1 mm over 10 s); a prop sphere clipping a post starts a yaw or roll rotation; friction stops a sliding sphere; the event fires once per contact with the 20 ms rearm; the same inputs give bit-identical states.
- **ELRS (unit):** the PER curve (50 % at the sensitivity, < 1 % at +5 dB); the sensitivity table; the range check; linear cross-polarization (a dipole at 90° loses 20 dB); behind building B the RSSI drops by its knife-edge loss; diversity hysteresis; a hovering quad in the open field keeps LQ at 100; the LINK_STATISTICS fields; same seed gives the same sequence.
- **Config:** schema 3 refused with the explanatory message; the `[collision]` defaults for the shipped quad (25 spheres, no 12 cm gap); validation errors collected; the world `[handset]` defaults and the open field.
- **Simulator, open loop:** a quad driven into building A raises one `COLLISION` naming it and ends up outside it; at 50 km with 10 mW (15 dB below the sensitivity) the link goes down and `LINK_DOWN` is raised; two lockstep runs give identical body and radio signals; `GetWorld` carries the handset.
- **Live SITL (`python/tests/test_sitl_link.py`):** at home the quad arms over CRSF as before; a test quad file at 10 mW with its initial pose 50 km out makes Betaflight report RX loss (failsafe in `MSP_STATUS`) and the OSD LQ cell read 0.
- **Godot:** the HUD link text, the collision toast, the handset marker; the open-loop e2e gains a collision toast.
- **CI:** no new jobs.

**Docs:** `docs/research/elrs-link.md` (equations and every constant with its provenance) and `docs/research/collision.md` (the contact models, for the education audience); `dev-setup.md` (the quad schema 4 change, the manual check: hit a wall, clip a gate, land on a roof, and with a 10 mW quad file watch LQ fall when flying low behind building B); `README.md`; `docs/superpowers/m3c-carried-debt.md`.

## 8. Risks

| Risk | Mitigation |
|---|---|
| Impulse contact jitters at rest or on a roof. | The landing contact points carry the resting weight with the tuned M1 spring-damper; restitution is 0 below 0.2 m/s; a 10 s rest test. |
| Clipping gates feels wrong in flight. | `restitution` and `friction_coeff` are config values; the user's manual flight check judges them. |
| The control link never drops in practice (2.4 GHz LoRa is strong at short range), so the feature looks inert. | The HUD shows RSSI and SNR moving; the range test pins the budget; the live 10 mW test at 50 km shows the full chain to Betaflight's failsafe. |
| Quad schema 4 breaks user quad files. | The load error names the removed and added fields; `dev-setup.md` documents the change. |
| Moving the RF code changes M3b behaviour. | Step 1 is a pure move with all M3b tests unchanged. |
| Collision tests are slow at 8 kHz. | The broad phase skips distant objects; the unit tests drive the rigid body directly. |
