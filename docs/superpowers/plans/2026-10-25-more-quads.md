# More pre-assembled quads — implementation plan

Spec: `docs/superpowers/specs/2026-10-25-more-quads-design.md`. Test first, one commit per task. Gates at the end:
`cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`,
`python -m pytest python/tests`, `scripts/run-godot-tests.sh all` in WSL if a Godot suite lists quads.

1. **Sweep tests (red).** `crates/ofs-config/tests/quads.rs`: every `quads/*.toml` loads; every library part is in the
   ledger; the three new stems exist. Fails on the missing quads and the unlisted parts only after Task 2 adds files.
   Commit the spec, plan and test.
2. **Toothpick.** Parts (`opendrone-3t` frame, `opendrone-motor-1404`, `opendrone-3in-prop`, `opendrone-4in1-esc-20a`,
   `opendrone-3s-450`, `opendrone-smartaudio-400`), `quads/opendrone-3t-toothpick.{toml,betaflight.diff}`, the
   ledger's sections. `ofs-sim/tests/quads.rs` flight-sanity test (red first, then bands from the running vehicle).
3. **Race.** Parts (`opendrone-5r`, `opendrone-motor-2306`, `opendrone-5in-race-prop`, `opendrone-4in1-esc-60a`),
   quad + diff (IMU `tdk-icm-42688p`, pack `opendrone-6s-1300`), ledger, flight-sanity case.
4. **Long-range.** Parts (`opendrone-7lr`, `opendrone-motor-2806`, `opendrone-7in-prop`, `opendrone-6s-4000`),
   quad + diff (900 MHz ELRS), ledger, flight-sanity case, the ordering test.
5. **Live SITL.** Run `hover.py` for each new quad under WSL SITL; adjust the diffs (rates, PID multiplier) until the
   hover holds; note results in the diff comments.
6. **Docs.** README quad list and library section, CHANGELOG, `docs/dev-setup.md` if it lists quads.
7. **Gates**, then stop (no push or PR until the user says so).
