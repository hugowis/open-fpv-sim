# The quad model

`quad.gltf` is the Godot client's drone model — the chase camera and the previews. A glTF 2.0 file: one embedded
buffer, one unit cube per colour, no textures. Regenerate with `python tools/make_quad_model.py` from the
repository root; the output is deterministic, so commit the script and the asset together.

Authored in this repository for the OpenDrone 5" freestyle frame — no third-party art, nothing to licence. The
geometry is the procedural model of PR #147 carried into an asset; the numbers' sources:

- Motor positions and arm, 0.08 m: `library/frames/opendrone-5f.toml` (`frame.motor_positions_frd_m`, an M1
  estimate — its provenance entry applies to this asset too).
- Propeller, 0.127 m: `library/props/opendrone-5in-prop.toml` (`prop.diameter_m`, "5 inch"). Other families'
  prop sizes are drawn as the scaled 5" (a 7" frame shows a 7.1" prop): an estimate, imperceptible at the
  distances the model is seen.
- Plate, arm, bell, battery, strap, antenna and camera sizes and colours: estimates, carried from PR #147's
  procedural model. The camera mount's size is bounded by the FPV camera's 0.02 m near clip, not by hardware.

The node names are the contract `drone.gd` loads by (a test pins them): `Frame` (`Plate`, `PlateTop`, `Arm1`,
`Arm2`), `Battery`, `BatteryStrap`, `Motor1..4`, `Prop1..4` (each holding a `Blade`), `Antenna1..2`,
`CameraMount` (holding a `Lens`). The loader (`GLTFDocument.generate_scene`) makes node names unique across the
scene, so the four authored `Blade` nodes come back as `Blade`, `Blade2..4` — the code looks them up by the
`Blade*` prefix. The blur discs, the propeller spin and the per-family scale are drawn in code
(`drone.gd`, `quad_spec.gd`).
