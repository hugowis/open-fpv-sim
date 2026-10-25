# Plan: contact friction and full position correction (#65)

Spec: `docs/superpowers/specs/2026-10-25-contact-friction-design.md`. One commit per task, test first.

1. **Position correction tests (red), then the fix.** Tests: a corner (ground + wall) penetration resolved in one
   `resolve` call; a V-groove (two acute walls is not available with boxes: use ground + a box face with the sphere
   centre pushed so normals differ) converges; a single contact is bit-identical to the old result (hand-computed
   shift). Fix: sequential projection in `resolve`.
2. **Friction function tests (red), then `point_contact` in `collision.rs`.** Tests: incline holds at 0.95 of the
   friction angle, slides at 1.05 with the analytic acceleration, flat deceleration `mu*g`, no creep on the flat.
   Implement `point_contact(...)` (normal force + anchored Coulomb friction).
3. **Wire it into `RigidBody::contact`.** Anchor list in the body; `contact` takes `&mut self`. Integration test:
   a quad on the ground pushed sideways with a force below `mu*m*g` holds, above it slides.
4. **Gates and goldens.** `cargo test --workspace --locked`; for each changed golden: find why, regenerate with
   `OFS_GOLDEN_WRITE=1`, inspect the diff. No-contact flights stay bit-identical (existing test kept).
5. **Docs and changelog.** `docs/research/collision.md` (contact model, landing points, constants, limits),
   `docs/roadmap.md` limitations line, `CHANGELOG.md` Unreleased. Then clippy `-D warnings`, `python -m pytest
   python/tests`, and the Godot suites in WSL if a golden Godot reads changed.
