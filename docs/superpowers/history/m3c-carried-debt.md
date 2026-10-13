> Retired at the v1 close-out (2026-10-13): every item here now lives in `docs/roadmap.md` or was fixed (see
> `CHANGELOG.md`); this file is kept as history.

# M3c carried debt

These items were deliberately left out of M3c, or surfaced while building it. They are listed so M3d/M3e planning and the maintainer can pick them up.

## Deferred features
- **The generated body sphere reaches 1 cm past the shipped quad's legs** (4 cm radius at the centre of mass against 3 cm legs): on flat ground the quad rests on its belly sphere and the legs never carry the weight. **Resolved by the maintainer's manual flight check (2026-10-11): fine as it is** — the rest and the takeoffs feel right, so the belly resting is accepted behaviour; no geometry change wanted.
- **MSP_STATUS failsafe flags are not decoded in Python.** The live 50 km test observes the failsafe as "never arms + the OSD's LQ cell reads 0"; Betaflight's actual failsafe state (the `MSP_STATUS` failsafe phase / RX-loss flags) is not parsed. A decoded flag would be stricter — it would fail if Betaflight failed safe for the wrong reason. (Note for whoever does: the pinned Betaflight renders the CRSF OSD LQ element as icon + rf_mode + ":<lq>", so the OSD text form is `'{0: 0`, not `LQ 0`.)
- **Open-loop sessions cannot steer.** Without a flight controller there is no stabilizer, so a scripted stick sequence cannot fly a precise path: wall-hit behaviour is unit-tested (a quad driven straight at 30 m/s into a post) and e2e-tested via falls onto BuildingA, not by flying a circuit around the gates. A scripted-FC test (waypoint-like stick scheduling against real Betaflight) could.
- **The spec's own out-of-scope list:** 900 MHz; ELRS dynamic power and frequency hopping; Wi-Fi or other 2.4 GHz interference; crash damage (M4 faults); CRSF telemetry to the radio (M3d); non-flat terrain; rotated boxes; a frequency-dependent `rf_loss_db` (one number per object stands for 2.4 and 5.8 GHz); collisions between objects other than the quad.

## Modelling simplifications (estimates, see docs/research/elrs-link.md and docs/research/collision.md)
- **The handset transmits on its active antenna** — the one it receives the downlink best on. Real dual-antenna handsets alternate per packet (or bind per direction); a per-packet schedule would be closer.
- **Two contact mechanisms, one per job.** Hits get impulses, resting weight gets the M1 spring-damper; a 3 m/s touchdown both bounces (impulse) and compresses the leg springs (contact points).
- **Position correction moves only the deepest contact** out; the other penetrating spheres rely on their impulses and the next steps.
- **One frequency, one lobe.** 2440 MHz stands for the whole hop sequence; the body shadow is the video link's smooth lobe, not a measured 2.4 GHz pattern.
- Every constant is an estimate or a published figure; none is measured.

## Rulings made while planning
- **`PROP_SPHERE_RADIUS_M` is 2.0 cm, not the spec's 1.5 cm.** The two claims conflicted on the shipped quad: the outermost generated sphere centre sits 0.17524 m from the quad's centre (the ring's 60° grid straddles the 45° motor arm), so at 1.5 cm the worst ring-to-body gap was 0.12024 m — the spec's own "no gap admits a 12 cm post" failed by 0.24 mm. The safety claim won over the example figure; at 2 cm the worst gap is 0.1152 m.
- **`collision_speed_mps` carries the LAST event's speed** (the spec's own "the last collision's speed"). A 1.5 m drop onto BuildingA rebounds at restitution 0.3 (~1.6 m/s, over the 1 m/s threshold), raises a second event and overwrites the first — so the "one collision" tests drop from 0.55 m instead (impact ~3.3 m/s, rebound under the threshold).
- **A 50 km / 10 mW link is never up, so `LINK_DOWN` has no edge to detect there.** The events are raised on the remembered up→down transition; a link that starts down and stays down raises nothing. The spec's "LINK_DOWN is raised" is pinned on the fault test (the link up, then `radio_link_loss` or a radio cut) instead; the 50 km test pins "the link never comes up".
- **The `world.has("handset_position")` guard in `godot/world/world.gd` was audited and kept**: it serves the empty-dictionary clear path (`build({})`), where the key is legitimately absent. The HUD shows `SNR –` while the link is down instead of the no-signal number.
- **Flakes observed — `test_moving_off_the_parked_quads_channel_lowers_the_interference` (CI run 38078545752, 2026-10-10):** `TimeoutError: no MSP reply to command 89` (`MSP_SET_VTX_CONFIG`) on the merge push; it passed on the previous push and in the Windows suite. Not reproduced in three warm local attempts (isolated and in CI's module order) against a freshly built sim — but the first local cold run after a fresh WSL target build also failed it once (reason uncaptured). CI runners are always cold, so a cold-start timing sensitivity is the open hypothesis. Verdict deferred to the next CI run on main: if it times out again, capture the test output AND Betaflight's stderr before diagnosing (a VTX-config race in SITL, the MSP pump budget in the test helper, or an M3c-caused slow-down are the candidates).
