> Retired at the v1 close-out (2026-10-13): every item here now lives in `docs/roadmap.md` or was fixed (see
> `CHANGELOG.md`); this file is kept as history.

# M4a carried debt

## Deferred features
- **imu/baro are not parts** — they stay quad-level sections without provenance; they become parts when the
  sensor models have hardware identity (an MPU6000 vs an ICM42688 behaves differently).
- **fcs/, receivers/, cameras/ part kinds do not exist** — no model consumes them (the [fc] section is sim
  plumbing; receiver antennas are quad wiring; there is no camera model).
- **Provenance is validated, not exposed** — no RPC or API surfaces a part's provenance yet; it is file data.
- **OFS_LIBRARY is one path** — no search path list; point it at one library root.

## Rulings worth remembering
- SITL quads refuse mixed motor pole_pairs (one Betaflight motor_poles, one pack eRPM in the KISS frames).
- The eRPM datagram block takes per-motor pole pairs, so an open-loop mixed build is honest even though
  SITL cannot be.
- `rotor_inertia_kgm2` in AirframeParams is per-motor (a Vec) since M4a; uniform quads put the same number
  in every slot.
