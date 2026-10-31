# Courses and race modes: implementation plan (#132)

Spec: `docs/superpowers/specs/2026-10-26-courses-and-race-modes-design.md`. Test first at each step, one commit per
task. Gates at the end: `cargo test --workspace --locked --no-fail-fast`, `cargo clippy --workspace --all-targets
--locked -- -D warnings`, `python -m pytest python/tests`, `scripts/run-godot-tests.sh all` in WSL (touch Rust sources
first; grep for `SCRIPT ERROR`; `OFS_SIM_BIN` = debug ofs-sim; `OFS_GODOT_DL=/mnt/c/dev/open-fpv-sim/build/godot-dl`).

1. **Config** (`ofs-config/src/world.rs`, tests/world.rs): `[[courses]]`, `[race] course`, `RaceMode`, `Course`
   (resolved) and `WorldConfig::courses()`, validation.
2. **Tracker** (`ofs-sim/src/race.rs`): course-aware tracker, `reset_to`, time attack, signals `race.course`/`race.start_s`.
3. **Protocol 15 + sim wiring**: proto, `PROTOCOL_VERSION`, vehicle reset with a course, journal, server, session events
   and state, world message.
4. **Client + Python**: `ofs-client` model/worker, `ofs-godot` bindings, Python client + tests.
5. **Docs**: `docs/race-timing.md`, protocol notes, README/python README, CHANGELOG Unreleased, park world courses.
6. **Godot**: catalog scan, settings, menu row, app (Shift+N, re-apply), HUD, world gate drawing, tests.
7. **Gates**, final commit.
