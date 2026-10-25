# The IMU and the barometer as library parts: implementation plan (#42)

Spec: `docs/superpowers/specs/2026-10-25-imu-baro-parts-design.md`. Test first at each step; the workspace goldens are
the regression check. Gates at the end: `cargo test --workspace --locked --no-fail-fast`,
`cargo clippy --workspace --all-targets --locked -- -D warnings`, `python/tests` for `sim.parts`. No `cargo fmt`.

1. **Part files, test first** (`crates/ofs-config/tests/part.rs`): `IMU`/`BARO` string constants like `MOTOR`; tests for a
   good load, missing provenance, unknown provenance path, and `every_shipped_part_loads_and_names_its_id` extended with
   `imus/opendrone-imu`, `imus/tdk-icm-42688p`, `barometers/opendrone-baro`. Then `ImuPart`, `BaroPart`, `load_imu`,
   `load_baro` in `part.rs`.
2. **Library files**: `library/imus/opendrone-imu.toml`, `library/imus/tdk-icm-42688p.toml`,
   `library/barometers/opendrone-baro.toml`, each parameter with provenance (R5 for the TDK part).
3. **Quad reader, test first** (`crates/ofs-config/tests/config.rs`): the tests of the spec's "Tests" paragraph. Then
   `SCHEMA_VERSION = 6`; an untagged `ImuSlot`/`BaroSlot` (part id | inline table) in `QuadFile`; `PartRefs.imu`/`baro`
   as `Option<String>`; resolve `imus`/`barometers`; push `imu` and `baro` provenance between `battery` and `vtx`; the
   schema checks in `validate`; the schema-4 and schema-5 messages updated.
4. **Quads**: the five `quads/*.toml` become `schema_version = 6` with `imu = …` / `baro = …`. Fix the tests and fixtures
   that name `schema_version = 5` or the kind lists (`config.rs`, `crates/ofs-sim/tests/grpc.rs`, `python/tests/test_client.py`,
   anything the workspace run flags).
5. **Docs**: `library/README.md`, root `README.md` "Component library", `docs/research/component-library.md` (three new
   sections with sources), `docs/roadmap.md` (drop "and the IMU and barometer as parts"), `docs/architecture.md` if it
   lists kinds, `CHANGELOG.md` Unreleased entry (a quad-schema bump is a user-visible change).
6. **Gates**, then commit `feat(library): the IMU and the barometer as component-library parts (#42)`.
