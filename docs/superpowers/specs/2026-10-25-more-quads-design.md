# More pre-assembled quads: toothpick, race, long-range (#129)

## Why

Five quads ship, and they are one airframe — the OpenDrone 5" freestyle in five radio/VTX configurations. A pilot who
picks up the game wants a quad for the living room and one for distance, and the workbench (#43) needs a library with
more than one frame, motor and prop to assemble. #138 made every part of a quad a library part (schema 6), so a new
quad is now data: parts, a quad TOML and a Betaflight diff, with no code.

## Goals

- Three new quads that fly differently, each built from library parts:

  | Quad | File stem | Class | AUW | Pack | Character |
  |---|---|---|---|---|---|
  | OpenDrone 3T toothpick | `opendrone-3t-toothpick` | 3", 125 mm | 105 g | 3S 450 mAh | gentle: thrust-to-weight about 5, hover at about half throttle, slow |
  | OpenDrone 5R race | `opendrone-5r-race` | 5", 220 mm | 540 g | 6S 1300 mAh | sharp, hungry: thrust-to-weight about 16, hover at about a fifth, 45 A+ per motor |
  | OpenDrone 7LR long-range | `opendrone-7lr-longrange` | 7", 320 mm | 950 g | 6S 4000 mAh | heavy, efficient: thrust-to-weight about 7, about 115 W in the hover, a long flight |

- New parts under `library/`, every parameter with a `[provenance]` entry; the ledger
  (`docs/research/component-library.md`) lists them and their status.
- Each quad loads headless (the open-loop quick start) and its flight character is pinned by a test that the part
  files explain; each also has a Betaflight diff, checked live under SITL.
- The shipped five quads, their goldens and the 5" parts do not change.

## Non-goals

- No code: no new schema field, no new model. A quad that needs one is out of scope.
- No Godot change (the main menu session owns `godot/ui/` and quad selection; the new files are picked up by whatever
  lists `quads/`) and no world change.
- No new per-OS golden digests. `radio_golden` has one file per OS and macOS digests can only be generated on a
  macOS runner; the new quads get flight-sanity tests instead of byte digests.
- No datasheet transcription. Anti-goal: a figure presented as a datasheet value that was not read from one.

## Calls made (the brief said to state them rather than ask)

1. **Provenance.** Every new parameter is `estimated`, with a note that names the reasoning (a class-typical value, or
   a scaling from the 5F part). The one `datasheet` value is the Quad-X `motor_spin`, as in `opendrone-5f`. The race
   quad flies the existing datasheet-sourced `tdk-icm-42688p` IMU, which shows the part kinds mix.
2. **Sizing method.** Motor, prop and pack numbers were chosen so a steady-state model of the simulator's own
   equations (`V = d·Vbus`, `I = (V − ω/Kv)/R`, `Q = Cp·ρ·n²·D⁵/2π`, `T = Ct·ρ·n²·D⁴`) lands on the table above.
   Inertia is `0.3·m·r²` and `Izz = 1.8·Ixx` (the 5F relation); drag area scales with the square of the size; ground
   stiffness and damping scale with mass so the contact damping ratio stays at the 5F's 0.45.
3. **Reused parts.** The race quad flies the shipped `opendrone-6s-1300` pack and the long-range quad the shipped
   `opendrone-4in1-esc` (45 A, against its 28 A peak). New: three frames, three motors, three props, two ESCs
   (20 A AIO for the toothpick, 60 A for the race quad), two packs (3S 450, 6S 4000), one VTX (SmartAudio 400 mW).
4. **Names.** Motor ids are `opendrone-motor-<size>` (the `openmotor` name belongs to a specific open-hardware part
   this work does not describe).
5. **Radio and VTX.** Toothpick: ELRS 2.4 GHz at 250 Hz, 100 mW. Race: ELRS 2.4 GHz at 500 Hz, 100 mW, SmartAudio 1 W
   part starting at its 25 mW step (race rules). Long-range: ELRS 900 MHz at 200 Hz, 1 W, SmartAudio 1 W at 600 mW.
6. **Betaflight diffs** start from the freestyle diff (receiver, ESC sensor, OSD, VTX, Blackbox wiring) and differ in
   `force_battery_cell_count`, `craft_name`, the rates (`rates_type = ACTUAL` and the per-axis rate, super rate and expo:
   gentle, sharp, smooth) and `simplified_master_multiplier`, which scales the default PIDs by the ratio of the 5F's
   control authority (`T/W·g / arm`) to the quad's. These are starting points checked for a stable hover under SITL,
   not a tune; the diff comments say so.
7. **Top speed** is not simulated by a test (the open-loop stabilizer's gains are tuned for the 5F and say so). Each
   quad file's header states AUW, thrust-to-weight, hover throttle and hover power from the sizing model, and the
   flight-sanity test pins the first three against the running vehicle.

## Tests

- `ofs-config`: every `quads/*.toml` loads and validates (this adds the five shipped quads, which had no such sweep);
  every library part file is listed in the ledger; the new quads reference the parts above.
- `ofs-sim`: each new quad builds open-loop; it sits on the ground below its hover throttle and climbs above it; its
  thrust-to-weight at full throttle falls in a band; and the three order as the table says (race > freestyle > long-range
  > toothpick for thrust-to-weight; the reverse for hover throttle).
- Live SITL (not part of the gates, run once): each quad arms and holds 1 m in angle mode with `hover.py`.

## Docs

README (the quad list and the library section), `library/README.md` if it lists parts, the ledger, CHANGELOG
"Unreleased".
