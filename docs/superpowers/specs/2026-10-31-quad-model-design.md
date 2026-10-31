# A real 3D model for the quad (#130)

## Why

PR #147 (`ead8a1f`) delivered "a modelled quad, part of #130": `godot/drone/drone.gd` draws a plate, arms, motor
bells, battery and strap, two antennas and a tilted camera housing from `BoxMesh`es built in `_ready()`, and its PR
body says what it left open — "a glTF model also shown in the menu and workbench previews; this is built in code,
with no previews yet". Two things landed since that make the remainder cheap: the state stream grew `motor_rpm`
(PR #162, `d07e0fb`: `crates/ofs-sim/src/vehicle.rs:1695` converts `motor.{i}.omega_radps` to RPM, `proto/ofs/v1/sim.proto:236`
carries it, `crates/ofs-godot/src/lib.rs:274-275` exposes it as a `PackedFloat32Array` in `get_telemetry`), and the
main menu exists (`f122b83`, `godot/ui/main_menu.gd`). What is left of #130 is the asset, the real per-motor speeds,
the per-family size and the previews.

**The remainder, ruled against #147's delivery** (do not re-plan what shipped):

| #130 "done when" | State after #147 | This spec's remainder |
|---|---|---|
| A real model (glTF), instantiated in place of the boxes | Procedural boxes built in GDScript | A glTF 2.0 asset, loaded at runtime, drawn by the chase camera |
| Props spin at each motor's own speed from the state stream; blur disc at speed | Props turn with `motor_cmd` (the command, not the speed) | Spin from `motor_rpm`; blade hides above a threshold and the existing disc stands in |
| Shown in the chase camera, the menu and the workbench preview | Chase camera only | A preview scene in the main menu; the workbench (#128) re-hosts it |
| Frame, arms, motors, battery, antennas, camera at its configured tilt | All present, but one 5" size for every family | Scale per quad family from the frame part's motor positions |
| FPV unchanged, frame rate no regression, scene tests pass | Holds (housing inside the 0.02 m near clip) | Same guarantees re-pinned for the asset (Design call 8) |

## Goals

- `godot/drone/quad.gltf`: a glTF 2.0 model of the OpenDrone 5" quad-X at true scale (authored arm 0.08 m), generated
  by a committed, deterministic script (`tools/make_quad_model.py`, the OSD-font precedent), instantiated by `drone.gd`
  in place of the procedural boxes.
- Props turn at each motor's own `motor_rpm` (slowed by a fixed factor for the eye); above a threshold RPM the blade
  hides and the translucent disc stands in, so a dead motor stops, a windmilling descent turns, spool-up shows.
- The model scales with the quad's frame part (`motor_positions_frd_m`): the toothpick reads small, the 7" long-range
  big, with no new assets.
- The main menu shows a turning preview of the selected quad; the workbench (#128, later in the phase) re-hosts the
  same preview scene — the seam this spec leaves.
- No Rust, proto or Python change: `motor_rpm` already flows; nothing under `crates/` or `python/` is touched.

## Non-goals

- No downloaded or third-party model (Design call 2), no textures or normal maps, no per-family modelling beyond the
  uniform scale, no blade-shape or prop-wash detail.
- No workbench screen (#128), no crash damage on the model (#157 rides `PropDamage`/`MotorFailure` later), no
  protocol or telemetry change, no change to the FPV camera or the analog video chain.

## Design calls (decided here; the plan's executors ledger any change)

1. **"A real model (glTF)" = a glTF 2.0 asset loaded at runtime, replacing the code-built boxes.** The bar #147
   deferred is the asset form — model data separate from code, instantiated wherever a quad is shown — not sculpted
   realism; a low-poly quad is what a chase camera 1.2 m away reads anyway (`chase_camera.gd:5`).
2. **The asset is authored in this repo; no third-party download.** A sourced model brings a licence an executing
   agent cannot verify and geometry that contradicts the simulated airframe's own numbers. Provenance follows the
   house rule (every part parameter carries provenance) the OSD-font way: a committed generator
   (`tools/make_quad_model.py`, `tools/make_osd_font.py` precedent) plus a provenance note (`godot/drone/QUAD_MODEL.md`)
   naming the value sources — `library/frames/opendrone-5f.toml` (motor positions, spin), `library/props/opendrone-5in-prop.toml`
   (diameter 0.127 m) — with visual-only sizes marked estimates, like the part files' M1 estimates. Generated in this
   project, so no licence file is owed; the note says so.
3. **Runtime load via Godot's glTF API, not the editor import pipeline.** `drone.gd` builds the scene with
   `GLTFDocument`/`GLTFState` from `res://drone/quad.gltf` (single file, embedded base64 buffer, no textures — flat
   `baseColorFactor` colours; re-verify the exact 4.7 method signatures at execution). The headless suites never
   depend on the import cache; the one committed `.import` sidecar follows `godot/ui/osd_font.png.import` if the
   editor's first `--import` writes one.
4. **The node names are the contract** between asset and code, pinned by a test: `Frame`, `Battery`, `BatteryStrap`,
   `Motor1..Motor4`, `Prop1..Prop4` (empty pivots, each holding a `Blade`), `Antenna1..Antenna2`, `CameraMount`
   (holding `Lens`). `drone.gd` keeps `_props` (the four pivots, found by name) and adds the translucent blur discs
   in code under each pivot, as today — the asset stays geometry-only. Authored in Godot's frame (Y up, forward -Z;
   M1 rear-right at (0.08, y, 0.08), matching `drone.gd:8` and `opendrone-5f.toml:11`).
5. **Props spin from `motor_rpm`, drawn at a fixed fraction of true speed.** `set_motors(rpm, delta)` turns pivot *i*
   by `rpm[i]/60 * TAU / SLOWDOWN * delta` with `SLOWDOWN = 50` (30 000 RPM draws as 62.8 rad/s, today's `DRAWN_SPIN`
   is 60), sign per the frame's `motor_spin`. Above `BLUR_RPM = 3000` the blade hides and the disc stands in; below
   it the blade shows (spool-up, idle, windmill). `SLOWDOWN` and `BLUR_RPM` are visual estimates, marked in the code.
   `app.gd:248` switches from `motor_cmd` to `motor_rpm` (both already in `get_telemetry`). The mapping and threshold
   are unit-tested against scripted RPM arrays — the issue's test requirement.
6. **Per-family scale from the frame part; the camera mount is exempt.** A new `godot/scripts/quad_spec.gd`
   (Catalog-style line scanner) reads `frame = "…"` from the quad TOML, then `motor_positions_frd_m` and `motor_spin`
   from `library/frames/<id>.toml` beside the quad directory (the `library_roots` rule, `crates/ofs-config/src/lib.rs:140-155`;
   re-verify at execution). `Drone.set_quad(path)` scales the whole model by `arm / 0.08` (3T 0.044 → 0.55, 5R 0.078 →
   0.975, 7LR 0.113 → 1.4125, all square X-frames) and overrides the spin directions; a missing or unparseable file
   falls back to the authored 5F defaults. The `CameraMount` is reset to sit at the FPV eye point at authored size
   (position `FPV_EYE / k`, scale `1/k`): a bigger frame carries the same camera, and the near-clip guarantee
   survives any scale. Prop discs scale with the airframe; a scaled 7" prop shows 7.1" against the true 7" — noted
   as an estimate in `QUAD_MODEL.md`.
7. **Menu preview now; workbench seam for #128.** A new `godot/ui/quad_preview.gd` (a `SubViewportContainer` with a
   camera, a light and a slowly turning `Drone`, `set_quad(path)` rebuilding it) is hosted by the main menu beside
   the quad row and re-pointed on quad changes. #128's workbench re-hosts the same scene — the spec's only seam, the
   way #128 must leave #157's repair seam (the phase umbrella's one cross-task contract).
8. **FPV and performance unchanged, re-pinned.** The camera mount's farthest corner stays under the 0.02 m near clip
   (`fpv_camera.gd:18`) at every scale (call 6), asserted by test; the FPV camera never renders a new node.
   Instantiating one scene replaces ~20 in-`_ready` mesh builds, so the chase-camera cost is equal or lower; the
   frame-rate check on the dev machines stays a by-hand item (headless draws nothing), reported to #163.
9. **The camera tilt follows the configured tilt.** `drone.gd` reads `uptilt_deg` from a child `FpvCamera` when one
   is present (`main.tscn` mounts it there; default 30, `fpv_camera.gd:11`) and applies it to `CameraMount`'s pitch;
   without the child the authored 30° stands — the model's tilt and the camera's can no longer drift apart.

## Tests

- `godot/tests/test_scene.gd` (rewritten drone tests): the model instantiates from `quad.gltf` with the node-name
  contract; the camera mount sits at the eye inside the near clip at scales 0.55 and 1.4125; `set_motors` maps RPM
  to rotation (zero RPM does not turn, faster turns faster, M1/M4 clockwise, short arrays safe); the blade hides
  above `BLUR_RPM` and the disc is present.
- `godot/tests/test_quad_spec.gd` (new): the shipped frames' arm and spin values (0.044/0.078/0.08/0.113); a quad
  without a frame part falls back to the 5F defaults; a frame part in a data dir beside its quads is found.
- `godot/tests/test_quad_preview.gd` and `test_main_menu.gd` (new/extended): the preview hosts a model that turns,
  `set_quad` rescales it, and the menu re-points the preview when the quad row changes.
- The e2e open-loop suite stays green unchanged (the menu-less path and the drawn-pose assertions do not depend on
  the model's internals).

## Docs

`CHANGELOG.md` "Unreleased" (the model, the speeds, the preview), `README.md`'s Godot client paragraph (the quad is a
glTF model shown in the menu), `godot/drone/QUAD_MODEL.md` (provenance, node contract, regeneration command). The
by-hand checks (chase-camera look, FPV unchanged, frame rate, the preview) land in the plan and feed #163.
