# The `[look]` World Section Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A world file can dress itself — sky, sun, fog, ground, per field, validated and carried on `GetWorld` — and the Godot client applies it over the look the world's name codes, through the same `_apply_look` path; worlds without the section are bit-for-bit unchanged. Closes #173.

**Architecture:** An opt-in `[look]` section in `ofs-config` (every key optional, per-field `Option`s), a `Look` message with per-field presence on `World` (protocol bump), `world_msg` fills it as passenger data (no model, no signal, no digest), the three clients expose it — the bridge converting the section's NED sun angles to the Godot `DirectionalLight3D` Euler beside the existing frame conversions — and `world.gd` gains one merge helper (`merged_look`) in front of #134's untouched `_apply_look`.

**Tech Stack:** Rust workspace (MSRV 1.85), tonic/prost proto, Python `ofs` (grpc_tools stubs), Godot 4.7.2 GDScript through the gdext bridge.

**Spec:** `docs/superpowers/specs/2026-11-01-look-section-design.md` (its design calls are binding; this plan argues from them).

## Global Constraints

- **Baseline — #134 must have executed.** This pair's Godot task builds on `LOOKS`, `look_for`, `_apply_look` and the named `Sky`/`Sun` nodes that #134's plan (`docs/superpowers/plans/2026-10-31-world-rendering.md`, Task 1) creates; at `fdc4917` they are merged as a plan but **not yet in `godot/world/world.gd`**. Before Task 4, verify `grep -n "LOOKS\|_apply_look" godot/world/world.gd` finds them; if it does not, #134's pair has not executed — stop and re-plan rather than building the merge on air. Tasks 1-3 have no such dependency.
- **The wire takes the next free number and fields, never a fixed one** (the rule both wave umbrellas state): at execution read `crates/ofs-proto/src/lib.rs`'s `PROTOCOL_VERSION` (16 at `fdc4917`; the Consequence pairs #28/#156/#157 queue ahead and each takes the next), and take `World`'s next free field for `look` (12 at `fdc4917`, after `terrain = 11`; the Consequence pairs add to `WorldObject`, not `World` — re-verify anyway). Below, **N** means the number you read plus one.
- No golden digest changes (`crates/ofs-radio/tests/golden/*`, `crates/ofs-sim/tests/golden/*`): the look is passenger data — no model, no bus signal — so a moved golden is a bug, on any world.
- No `worlds/*.toml` edits: no shipped world gains a `[look]` (spec Design call 4; a config test fences it).
- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked` green on Windows; `cargo +1.85 check` passes on every changed crate.
- `cargo fmt` is not a gate; do not reformat existing files.
- Worktree-isolated sessions refuse heredocs and WSL through Bash: use Write/Edit for files and PowerShell for WSL; for multi-line WSL commands write a script file in the scratchpad and run `wsl -d Ubuntu -- bash <path>`.
- Regenerate Python stubs after the proto change: `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto` (`docs/dev-setup.md:21`); the three generated files change and nothing else under `python/ofs/v1`.
- Python tests need `PYTHONPATH=<worktree>/python`. Godot tests: `bash scripts/run-godot-tests.sh unit|all` in WSL (it downloads Godot 4.7.2); read the WHOLE output — the suites print `0 failed` even after a SCRIPT ERROR.
- Commit trailers: none prescribed (an executing session that adds one names its own identity); never push.

## Design calls (decided here; the executor ledgers any change)

1. **The whole table vocabulary travels** — the 18 fields of #134's `LOOKS`, with the sun's Godot Euler triple re-expressed as `sun_az_deg` + `sun_elev_deg` (NED angles like `pilot.facing_deg`); the height-fog pair only if #134's ledger kept it (re-verify at Task 4).
2. **Per-field precedence: the file wins over the look the name would wear.** Absent section, bare `[look]` or all-unset = exactly `look_for(name)`, bit for bit; partial = the coded look with the file's keys overwritten.
3. **Colours and the sun are atomic** — one `Vec3` per colour; both sun angles or neither (a validation error, not a guess).
4. **No shipped world gains a `[look]`;** the code table stays the five's dress and the merge base.
5. **`Look` on the wire with per-field presence** (colours as `Vec3`, scalars `optional double`), `World.look` at the next free field, protocol N per the constraint above.
6. **The bridge converts the sun** (`frames::sun_rotation_deg(az, elev) = (-elev, 180 - az, 0)` degrees) so the Godot dictionary's keys are exactly `LOOKS`' keys and `_apply_look` is unchanged.
7. **`merged_look` duplicates the base** — `LOOKS`' inner dictionaries are shared references; merging without `duplicate()` corrupts the table for every later build.
8. **All three clients expose it** (the m8a rule); Python gets the `Look` dataclass the terrain never got (nineteen numbers, not a million).

## Review Focus

- **A half-set sun** (`sun_az_deg` alone) is refused at load naming the field — not silently paired with a default elevation. Pinned by Task 1's `a_half_set_sun_is_refused`.
- **A world with both a coded name and a partial `[look]`** wears the coded mood plus the file's fields — not default-plus-fields, not wholly replaced. Pinned by Task 4's `a_look_dresses_the_coded_look_field_by_field`.
- **Merging must not corrupt the coded table**: two dressed builds in a row, then `World.LOOKS["Flat field"]` still holds its coded values. Pinned by Task 4's `merged_look_does_not_touch_the_coded_table`.
- **The bridge's sun conversion could carry the wrong Euler branch** — the Rust arithmetic test cannot catch a wrong convention; the Godot `Basis` round-trip is the authority (if it fails, fix the formula in both places, not the test). Pinned by Task 4's `the_looks_sun_rotation_places_the_sun_where_the_angles_say`.
- **The protocol bump is real and complete**: a client speaking N-1 is refused by the handshake, and no live "protocol 16" reference survives outside `docs/superpowers`. Pinned by Task 2's version test and grep step.

---

### Task 1: The `[look]` world section (ofs-config)

**Files:**
- Modify: `crates/ofs-config/src/world.rs` (the section, the `WorldConfig.look` field, validation), `crates/ofs-config/src/lib.rs` (re-export)
- Test: `crates/ofs-config/tests/world.rs`

**Interfaces:**
- Produces: `pub struct LookSection` in `world.rs` — `#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]` + `#[serde(default, deny_unknown_fields)]`, fields (in this order) `sky_top, sky_horizon, ground_horizon, ground_bottom: Option<[f64; 3]>`, `sun_az_deg, sun_elev_deg: Option<f64>`, `sun_color: Option<[f64; 3]>`, `sun_energy, ambient_energy: Option<f64>`, `fog_color: Option<[f64; 3]>`, `fog_density, fog_sky_affect, fog_height, fog_height_density: Option<f64>`, `ground_base, ground_line, ground_major, patch_color: Option<[f64; 3]>`, `patch_strength: Option<f64>`; `WorldConfig.look: Option<LookSection>` (`#[serde(default)]`, `None` in `open_field()`); re-exported as `ofs_config::LookSection`.
- Validation (field paths `look.<key>`, messages show the value, the house style of `world.rs:454-639`): every colour finite with each component in [0, 1] ("each value must be in [0, 1]"); `sun_az_deg` finite in [-180, 180]; `sun_elev_deg` finite in [-90, 90]; the sun both-or-neither ("the sun needs both `sun_az_deg` and `sun_elev_deg`", reported under whichever key is set); `sun_energy`/`ambient_energy` finite in [0, 16]; `fog_density` finite in [0, 0.05]; `fog_sky_affect`/`fog_height_density`/`patch_strength` finite in [0, 1]; `fog_height` finite in [-1000, 1000].

- [ ] **Step 1: Write the failing tests** in `crates/ofs-config/tests/world.rs` (beside the gate tests; reuse `world_text()`/`problems()`):
  - `a_look_section_parses_and_keeps_what_was_not_set`: `world_text() + "\n[look]\nsky_top = [0.22, 0.14, 0.3]\nsun_az_deg = -100.0\nsun_elev_deg = 25.0\nfog_density = 0.006\n"` loads; `w.look` is `Some` with `sky_top == Some([0.22, 0.14, 0.3])`, the sun pair both `Some`, `fog_density == Some(0.006)`, and `sun_color`, `patch_strength`, `ground_base` all `None`; the bare `world_text()` loads with `w.look == None`.
  - `a_bare_look_section_sets_nothing`: `world_text() + "\n[look]\n"` loads with `w.look == Some(LookSection::default())`.
  - `every_look_problem_is_reported_at_once`: `world_text()` + one `[look]` block setting `sky_top = [0.0, 1.4, 0.5]`, `sky_horizon = [nan, 0.0, 0.0]`, `sun_az_deg = 200.0`, `sun_elev_deg = 100.0`, `sun_energy = 20.0`, `ambient_energy = -1.0`, `fog_density = 0.5`, `fog_sky_affect = -0.1`, `fog_height = 2000.0`, `fog_height_density = 2.0`, `patch_strength = 1.5` — assert one message per problem, each starting `look.<key>: ...` with the expected text (the style of `every_world_problem_is_reported_at_once`).
  - `a_half_set_sun_is_refused`: twice — `sun_az_deg` alone and `sun_elev_deg` alone — each yields the both-or-neither message under the set key.
  - `no_shipped_world_has_a_look`: for each of `circuit, flat, hills, meadow, park`, `world::load` on `Path::new(env!("CARGO_MANIFEST_DIR")).join("../../worlds").join(format!("{name}.toml"))` gives `look == None`.
  - `a_look_key_must_be_known`: `world_text() + "\n[look]\nshimmer = 1.0\n"` is refused as a parse error naming `shimmer` (the `deny_unknown_fields` catch, `a_world_file_with_a_gate_missing_a_field_is_refused`'s assertion shape).
- [ ] **Step 2: Run** `cargo test -p ofs-config --test world look` — Expected: FAIL (unknown field `look`, `LookSection` not found).
- [ ] **Step 3: Implement.** `LookSection` after `RaceSection` in `world.rs` (doc comment: the world's dress, every field optional, colours `[r, g, b]` 0..=1, the client dresses the name-coded look with whatever is set); the `look` field on `WorldConfig` (doc comment: "The world's dress; absent = the client's name-keyed look", `None` in `open_field()`); a `check_look` block in `validate` (`if let Some(l) = &self.look { ... }`): a small closure `color = |c: Option<[f64; 3]>, key: &str|` checking finite + 0..=1 per component, a `range = |v: Option<f64>, key, lo, hi|` closure checking finite + bounds, then the both-or-neither sun rule (skip it when neither is set; when one is `None` report under the set one — note in the message which is missing). Re-export in `lib.rs` next to `TerrainSection`.
- [ ] **Step 4: Run** `cargo test -p ofs-config --locked` then `cargo test --workspace --locked --no-fail-fast` — Expected: PASS (nothing consumes `look` yet, so no other test can see it).
- [ ] **Step 5: Commit** `feat(config): a [look] section in the world file (#173)` — `git add crates/ofs-config/src/world.rs crates/ofs-config/src/lib.rs crates/ofs-config/tests/world.rs`.

### Task 2: Protocol N — the `Look` message and `world_msg`

**Files:**
- Modify: `proto/ofs/v1/sim.proto` (the `Look` message; `World.look`), `crates/ofs-proto/src/lib.rs` (version N, doc comment), `python/ofs/client.py` (`PROTOCOL_VERSION = N`), `python/ofs/v1/sim_pb2.py`, `sim_pb2.pyi`, `sim_pb2_grpc.py` (regenerated), `crates/ofs-sim/src/session.rs` (`world_msg`)
- Test: `crates/ofs-proto/tests/messages.rs`, `crates/ofs-sim/src/session.rs` unit tests, `crates/ofs-sim/tests/grpc.rs`

**Interfaces:**
- Consumes: Task 1's `LookSection`.
- Produces: `message Look` — `Vec3 sky_top = 1; Vec3 sky_horizon = 2; Vec3 ground_horizon = 3; Vec3 ground_bottom = 4; optional double sun_az_deg = 5; optional double sun_elev_deg = 6; Vec3 sun_color = 7; optional double sun_energy = 8; optional double ambient_energy = 9; Vec3 fog_color = 10; optional double fog_density = 11; optional double fog_sky_affect = 12; optional double fog_height = 13; optional double fog_height_density = 14; Vec3 ground_base = 15; Vec3 ground_line = 16; Vec3 ground_major = 17; Vec3 patch_color = 18; optional double patch_strength = 19;` (colours as `Vec3` carry presence as messages; scalars take `optional` — the `Fault.at_s` shape, `sim.proto:376-377`); `optional Look look = <next free>;` on `World` (12 at `fdc4917`); `world_msg`'s `look: w.look.map(|l| pb::Look { sky_top: l.sky_top.and_then(vec3_of), ..., sun_az_deg: l.sun_az_deg, ... })` (`vec3_of` is the existing `[f64; 3] -> Option<pb::Vec3>` helper, `session.rs:255`).

- [ ] **Step 1: Write the failing tests:**
  - `messages.rs`: change `assert_eq!(PROTOCOL_VERSION, 16)` to N in `the_protocol_version_matches_the_python_client`; add `a_look_survives_an_encode_decode_round_trip` — build a `pb::Look` with `sky_top`, the sun pair, `fog_density`, `patch_strength` set (and nothing else), round-trip it through `Message::encode/decode`, assert the set fields survive and the unset ones are `None`/absent.
  - `session.rs`, beside `the_world_message_carries_the_world_file`: `the_world_message_carries_the_look` — `let mut w = WorldConfig::open_field(); w.look = Some(LookSection { sky_top: Some([0.22, 0.14, 0.3]), sun_az_deg: Some(-100.0), sun_elev_deg: Some(25.0), fog_density: Some(0.006), ..Default::default() });` then `world_msg(&w, None).look` is `Some` with those fields and, say, `sun_color` absent; extend `the_world_message_carries_the_world_file` with `assert!(msg.look.is_none(), "the shipped world has no look")`.
  - `grpc.rs`, beside `a_session_flies_in_the_world_it_was_loaded_with`: `get_world_has_no_look_for_the_flat_world` — after the existing flat-world load, `c.get_world(Empty {}).await.unwrap().into_inner().look.is_none()`.
- [ ] **Step 2: Run** `cargo test -p ofs-proto -p ofs-sim --locked look protocol` — Expected: FAIL (version mismatch first; the rest compile errors).
- [ ] **Step 3: Implement.** Add the `Look` message to `sim.proto` after `Terrain` with its doc comment ("The world's dress, as its `[look]` section set it: every field optional (unset fields do not travel), colours r,g,b in 0..1; applied by the client over the look the world's name codes there"), add `World.look`, extend the header's version comment ("protocol version N: protocol N-1 plus the world's `[look]` section (`World.look`)"), bump `crates/ofs-proto/src/lib.rs` and `python/ofs/client.py` to N, regenerate the stubs (Global Constraints command), and fill `world_msg`. Then `grep -rn "protocol 16" --include=*.rs --include=*.md --include=*.py .` outside `docs/superpowers` and update every live hit (at `fdc4917`: `docs/dev-setup.md:126,282,290,299` — Task 5 rewrites them properly; here only the number must stop being wrong).
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast` and `PYTHONPATH=python python -m pytest python/tests -q` — Expected: PASS (the SITL tests skip without `OFS_SITL_LAUNCH`).
- [ ] **Step 5: Commit** `feat(proto)!: protocol N - the world's [look] on GetWorld (#173)` — `git add proto/ofs/v1/sim.proto crates/ofs-proto/src/lib.rs python/ofs/client.py python/ofs/v1 crates/ofs-sim/src/session.rs crates/ofs-proto/tests/messages.rs crates/ofs-sim/tests/grpc.rs`.

### Task 3: The clients — Rust, the bridge, Python

**Files:**
- Modify: `crates/ofs-client/src/model.rs` (`WorldLook`, `World.look`, `from_pb`), `crates/ofs-client/src/frames.rs` (`sun_rotation_deg`), `crates/ofs-godot/src/lib.rs` (`get_world`'s `look` key), `python/ofs/client.py` (`Look`, `World.look`, `_world`)
- Test: `crates/ofs-client/tests/model.rs`, `python/tests/test_scripting.py`

**Interfaces:**
- Consumes: Task 2's `pb::Look`.
- Produces:
  - `model.rs`: `#[derive(Debug, Clone, Copy, Default, PartialEq)] pub struct WorldLook` with `pub sky_top, sky_horizon, ground_horizon, ground_bottom, sun_color, fog_color, ground_base, ground_line, ground_major, patch_color: Option<[f64; 3]>` and `pub sun_az_deg, sun_elev_deg, sun_energy, ambient_energy, fog_density, fog_sky_affect, fog_height, fog_height_density, patch_strength: Option<f64>` (the same order as `LookSection`); `World.look: Option<WorldLook>`; `from_pb` fills colours with `l.sky_top.map(|c| [c.x, c.y, c.z])` and passes the scalars through.
  - `frames.rs`: `/// The sun's direction (azimuth clockwise from north, elevation up) as the `DirectionalLight3D` Euler rotation in degrees that puts it there in Godot. The shipped default's coded Vector3(-50, 35, 0) is az 145, elev 50 under this mapping.` `pub fn sun_rotation_deg(az_deg: f64, elev_deg: f64) -> DVec3 { DVec3::new(-elev_deg, 180.0 - az_deg, 0.0) }`
  - `get_world()`: a `"look"` key, present only when the world has one, holding only the set fields, keys exactly `LOOKS`': the ten colours as `Color` (`Color::from_rgb(c[0] as f32, c[1] as f32, c[2] as f32)`, the `lib.rs:482` pattern), `sun_rotation_deg` a `Vector3` from `sun_rotation_deg(az, elev)` **only when both angles are set** (validation guarantees the pair), and `sun_energy, ambient_energy, fog_density, fog_sky_affect, fog_height, fog_height_density, patch_strength` as floats; the doc comment above `get_world` (`lib.rs:447-460`) grows the `look` sentence.
  - `python/ofs/client.py`: `@dataclass(frozen=True) class Look` — "The world's dress, as its `[look]` section set it: every field `None` when the file left it out (colours as `(r, g, b)`); the Godot client applies it over the look the world's name codes." — ten `tuple | None = None` colour fields, nine `float | None = None` scalars, same names; `World.look: Look | None = None`; `_world` fills `look=Look(sky_top=_v(m.look.sky_top), ..., sun_az_deg=m.look.sun_az_deg, ...) if m.HasField("look") else None`.

- [ ] **Step 1: Write the failing tests:**
  - `crates/ofs-client/tests/model.rs`, beside `the_world_converts_to_godots_frame`:
    - `a_look_reaches_the_world_model`: `World::from_pb(pb::World { look: Some(pb::Look { sky_top: v(0.22, 0.14, 0.3), sun_az_deg: Some(-100.0), sun_elev_deg: Some(25.0), fog_density: Some(0.006), ..Default::default() }), ..Default::default() })` gives `look.sky_top == Some([0.22, 0.14, 0.3])`, the sun pair set, `sun_color == None`; and `World::from_pb(pb::World::default()).unwrap().look.is_none()`.
    - `the_suns_azimuth_and_elevation_become_the_godot_rotation`: `sun_rotation_deg(145.0, 50.0) == DVec3::new(-50.0, 35.0, 0.0)` (today's coded default sun, `godot/world/world.gd:152`'s value); `sun_rotation_deg(0.0, 0.0) == DVec3::new(0.0, 180.0, 0.0)`; `sun_rotation_deg(-90.0, 30.0) == DVec3::new(-30.0, 270.0, 0.0)`.
  - `python/tests/test_scripting.py`, beside `test_a_world_can_be_named`:

```python
def test_a_world_can_dress_itself(sim, tmp_path):
    toml = tmp_path / "dressed.toml"
    toml.write_text(
        "schema_version = 1\nname = \"Sunset scratch\"\n"
        "[pilot]\nposition_ned_m = [0.0, 0.0, -1.7]\nfacing_deg = 0.0\n"
        "[[receiver.antennas]]\nname = \"omni\"\nkind = \"omni\"\ngain_dbi = 2.0\npolarization = \"rhcp\"\n"
        "[look]\nsky_top = [0.22, 0.14, 0.3]\nsun_az_deg = -100.0\nsun_elev_deg = 25.0\nfog_density = 0.006\n")
    sim.load(QUAD, open_loop_fc=True, world=str(toml))
    look = sim.get_world().look
    assert look.sky_top == (0.22, 0.14, 0.3) and look.sun_az_deg == -100.0 and look.fog_density == 0.006
    assert look.sun_color is None
    sim.load(QUAD, open_loop_fc=True, world="flat")
    assert sim.get_world().look is None
```

- [ ] **Step 2: Run** `cargo test -p ofs-client --locked look sun` and `PYTHONPATH=python python -m pytest python/tests/test_scripting.py -q` — Expected: FAIL (no `WorldLook`; no `World.look`; `look` attribute errors).
- [ ] **Step 3: Implement** the four files as in Interfaces. In `get_world`, after the `terrain` block: `if let Some(l) = &world.look { let mut e = VarDictionary::new(); if let Some(c) = l.sky_top { e.set("sky_top", &look_color(c)); } ... if let (Some(az), Some(el)) = (l.sun_az_deg, l.sun_elev_deg) { e.set("sun_rotation_deg", vector3(x, y, z)) } ...; d.set("look", &e); }` with `fn look_color(c: [f64; 3]) -> Color` beside `vector3` (`lib.rs:594`) and the `sun_rotation_deg` values cast to f32.
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast`, `PYTHONPATH=python python -m pytest python/tests -q`, `cargo clippy --workspace --all-targets --locked -- -D warnings` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(client): the look in the Rust, Godot and Python clients (#173)` — `git add crates/ofs-client/src/model.rs crates/ofs-client/src/frames.rs crates/ofs-client/tests/model.rs crates/ofs-godot/src/lib.rs python/ofs/client.py python/tests/test_scripting.py`.

### Task 4: Godot — the file's look over the coded one

**Files:**
- Modify: `godot/world/world.gd` (`merged_look`; `build`'s dress line), `godot/tests/e2e_open_loop.gd` (one line)
- Test: `godot/tests/test_scene.gd`

**Interfaces:**
- Consumes: #134's `LOOKS`, `look_for`, `_apply_look` (verified present per Global Constraints) and Task 3's `"look"` dictionary key (only set fields; `sun_rotation_deg` arrives as a `Vector3`).
- Produces: `static func merged_look(world: Dictionary) -> Dictionary` — `look_for(String(world.get("name", ""))).duplicate()` overwritten key-by-key by `world.get("look", {})`; `build`'s dress line becomes `_apply_look(merged_look(world))` inside the existing `if world.has("name")` guard.

- [ ] **Step 1: Write the failing tests** in `godot/tests/test_scene.gd`:

```gdscript
func test_a_look_dresses_the_coded_look_field_by_field() -> void:
	var world := World.new()
	await add_to_tree(world)
	var env: Environment = (world.get_node("Sky") as WorldEnvironment).environment
	world.build({"name": "Flat field", "look": {"fog_density": 0.006}})
	var coded: Dictionary = World.LOOKS["Flat field"]
	near(env.fog_density, 0.006, "the file's field wins")
	var sky: ProceduralSkyMaterial = env.sky.sky_material as ProceduralSkyMaterial
	ok(sky.sky_top_color.is_equal_approx(coded["sky_top"]), "the coded mood stays where the file is silent")
	world.build({"name": "Nowhere", "look": {"sky_top": Color(0.9, 0.45, 0.2)}})
	ok(sky.sky_top_color.is_equal_approx(Color(0.9, 0.45, 0.2)), "an unknown name starts from default, the file's fields on top")
	near(env.fog_density, World.LOOKS["default"]["fog_density"], "default's fog where the file said nothing")
	world.queue_free()


func test_merged_look_does_not_touch_the_coded_table() -> void:
	var world := World.new()
	await add_to_tree(world)
	world.build({"name": "Flat field", "look": {"fog_density": 0.006, "sky_top": Color(0.0, 0.0, 0.0)}})
	world.build({"name": "Open meadow"})
	var coded: Dictionary = World.LOOKS["Flat field"]
	ok((coded["sky_top"] as Color).is_equal_approx(Color(0.30, 0.48, 0.78)), "the coded table keeps its values")
	world.build({"name": "Flat field"})
	var env: Environment = (world.get_node("Sky") as WorldEnvironment).environment
	var sky: ProceduralSkyMaterial = env.sky.sky_material as ProceduralSkyMaterial
	ok(sky.sky_top_color.is_equal_approx(coded["sky_top"]), "and a later build dresses from the clean table")
	world.queue_free()


## The contract mirror of frames::sun_rotation_deg (ofs-client): if this fails, the formula is wrong in BOTH places.
func test_the_looks_sun_rotation_places_the_sun_where_the_angles_say() -> void:
	for probe in [[0.0, 90.0], [145.0, 50.0], [-90.0, 25.0]]:
		var az: float = probe[0]
		var elev: float = probe[1]
		var rot := Vector3(-elev, 180.0 - az, 0.0)
		var sun_pos: Vector3 = -(Basis.from_euler(Vector3(deg_to_rad(rot.x), deg_to_rad(rot.y), deg_to_rad(rot.z))) * Vector3.BACK)
		var expected := Vector3(sin(deg_to_rad(az)) * cos(deg_to_rad(elev)), sin(deg_to_rad(elev)), -cos(deg_to_rad(az)) * cos(deg_to_rad(elev)))
		ok(sun_pos.is_equal_approx(expected.normalized()) and expected.length() > 0.99, "az %.0f elev %.0f: %s" % [az, elev, sun_pos])
	ok(Vector3(-50.0, 35.0, 0.0) == Vector3(-50.0, 180.0 - 145.0, 0.0), "the coded default sun is az 145, elev 50")
```

  In `godot/tests/e2e_open_loop.gd`, right after the `"the shipped world is loaded"` check (line 207 at `fdc4917`): `_check(not world.has("look"), "the shipped flat world has no [look] section: %s" % str(world.has("look")))`.
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: FAIL on the new tests (`merged_look` unknown); `test_the_looks_sun_rotation_places_the_sun_where_the_angles_say` must PASS already — it pins Godot's own Euler semantics against the formula, and if it fails the formula's branch is wrong: fix `frames::sun_rotation_deg` (Task 3) to match reality, its Rust test with it, and ledger the correction.
- [ ] **Step 3: Implement** in `godot/world/world.gd`, beside `look_for`:

```gdscript
## The look the world dict wears: the one its name codes, dressed by the fields its file set (`look`, when the
## world has a section; only the set fields travel). The dict's look keys are this table's keys.
static func merged_look(world: Dictionary) -> Dictionary:
	var look := look_for(String(world.get("name", ""))).duplicate()
	for key in world.get("look", {}):
		look[key] = world["look"][key]
	return look
```

  and change `build`'s dress line from `_apply_look(look_for(world["name"]))` to `_apply_look(merged_look(world))` (the guard around it is unchanged). Update `LOOKS`' doc comment to say the table is the dress a `[look]` section dresses over.
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh all` (WSL; the Betaflight e2e only with `OFS_SITL_LAUNCH`) — Expected: PASS, whole output read.
- [ ] **Step 5: Commit** `feat(godot): the world file's look dresses the field (#173)` — `git add godot/world/world.gd godot/tests/test_scene.gd godot/tests/e2e_open_loop.gd`.

### Task 5: Docs, changelog and the protocol references

**Files:**
- Modify: `CHANGELOG.md`, `docs/dev-setup.md` (the world-file list's `[look]` bullet; the protocol references), `python/README.md` (`World.look`), `README.md` (one sentence beside the world-file lines around 94)

**Interfaces:** Consumes Tasks 1-4; produces nothing for code.

- [ ] **Step 1: Write the edits.** `docs/dev-setup.md`: a `[look]` bullet after the `[terrain]` one (`docs/dev-setup.md:123-129`) — the ten colours as `[r, g, b]` 0..=1, the sun as `sun_az_deg`/`sun_elev_deg` (both or neither), the energies, the fog and ground fields, every key optional, the file winning per field over the look the world's name codes client-side, no shipped world shipping one, and the ranges in one line. Rewrite the four protocol sentences (126, 282, 290, 299 at `fdc4917`) to N with `[look]` named in the `GetWorld` one. `python/README.md`: `World.look` (`Look` with all-`None` defaults) in the world paragraph. `README.md`: one sentence after the world-as-data-file line (~94) — a world file can dress itself (sky, sun, fog, ground) with a `[look]` section. `CHANGELOG.md` under Unreleased, `### The [look] world section (#173)`: Added (the section, the validation ranges, `World.look` on the wire at protocol N, the three clients, the Godot merge) and Changed (protocol N, breaking for clients of N-1: the handshake refuses them).
- [ ] **Step 2: Check** every claim against the code: `grep -rn "protocol 16\|PROTOCOL_VERSION" README.md docs/dev-setup.md python/README.md docs/architecture.md` shows N everywhere it is live; `grep -rn "\[look\]" worlds/` is empty.
- [ ] **Step 3: Run the whole gate** — `cargo test --workspace --locked --no-fail-fast`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo +1.85 check -p ofs-config -p ofs-sim -p ofs-client -p ofs-proto -p ofs-godot`, `PYTHONPATH=python python -m pytest python/tests -q`, and WSL `cargo test --workspace --locked --no-fail-fast -- --test-threads=1` (goldens must not move) plus `bash scripts/run-godot-tests.sh all` — Expected: PASS, whole outputs read.
- [ ] **Step 4: Commit** `docs: the [look] section and the protocol N changelog (#173)` — `git add CHANGELOG.md README.md docs/dev-setup.md python/README.md`.

## Ledger

Nothing yet — the executor appends any deviation from a design call here, with the ruling and the reason. Expected drift points: the protocol number and `World.look`'s field (the Consequence pairs' merges), the height-fog fields' survival (call 1), and the sun formula's branch (Task 4's Step 2 authority).
