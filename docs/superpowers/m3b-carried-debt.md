# M3b carried debt

These items were deliberately left out of M3b, or surfaced while building it. They are listed so M4 planning and the maintainer can pick them up.

## Deferred features
- **Collision with world objects.** The world file's boxes and cylinders are drawn and block the video signal, but the drone flies through them: the physics contact model still knows only the ground plane. The world file is the natural input for it.
- **ELRS on the shared propagation.** `ofs-video::propagation` is pure and frequency-agnostic; the ELRS link still uses the quad file's fixed RSSI, SNR and loss figures. Moving it onto the same geometry would make range and buildings cost control-link quality too.
- **The "VTX interference" fault (M4).** Emitters are static world data today; the fault would add, move or remove emitters at run time.
- **Non-flat terrain and rotated boxes.** The ground is the plane d = 0 (ground bounce, rooted objects) and boxes are axis-aligned.
- **Configurable receiver thresholds.** The picture thresholds (25, 12, 8, 6, 3 dB...) are named constants in `ofs-video::link`, not world-file fields.
- **Video latency, frame drops and DVR.** The link changes the picture's quality only.

## Modelling simplifications (estimates, see docs/research/video-link.md)
- One knife edge per object, losses of several objects added; no reflections off objects, only off the ground.
- The reflected ray is not obstructed separately: an object's loss applies to the combined direct and reflected signal.
- The body shadow is one smooth lobe (forward and down) of up to 8 dB, not a measured pattern of a real frame.
- Every constant is an estimate; none is measured.

## Rulings made while planning
- **The VTX transmits in open loop.** The spec's open-loop tests need a transmitting VTX, and a powered VTX transmits whether or not a flight controller talks to it, so an open-loop quad with `[vtx]` gets the VTX model (on its power-up channel and power) and the link. M3a's tests that expected no VTX in open loop were updated.
- **The obstruction is found by a search, not 32 samples.** A golden-section search on the convex shape's signed distance finds the deepest point exactly, so a thin post on a long path is not stepped over (the spec's risk table named this).
- **Objects standing on the ground are rooted below it.** Otherwise the nearest face of a building to a low path is its bottom face, and the knife edge measured under the building.
- **Emitter frequencies may be 5300 to 6000 MHz.** The spec said 5600 to 6000; Lowband (5362 to 5621 MHz) is one of the VTX's own bands, so it is accepted for emitters too.
- **The sync-loss test is at 4 km and 25 mW**, not 2 km: with the shipped diversity goggles (8 dBi patch), 25 mW at 2 km is at the edge (SNR about 0.5 dB), so whether the sync is lost there depends on the fade; at 4 km it is lost with any fade (see docs/research/video-link.md, "Link budget check").
