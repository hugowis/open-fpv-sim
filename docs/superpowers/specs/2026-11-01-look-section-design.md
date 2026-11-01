# A `[look]` section in the world file — dressable user worlds (#173)

Epic #136's rendering child #134 dressed the five shipped worlds from a name-keyed GDScript table; the world
dictionary's `name` is the only per-world identity the client holds (`crates/ofs-godot/src/lib.rs:516`), so a user's
own world wears the default look. An opt-in `[look]` section in the world file — validated by `ofs-config`, carried on
`GetWorld`, applied by the same `_apply_look` path — lets a user world dress itself without a code change. It needs a
protocol bump; per the standing rule in both wave umbrellas, the number is **the next free one at execution** (main
holds 16 at `fdc4917`; the Consequence wave's three pairs queue first), never a fixed 17.

Written 2026-11-01 against `main` at `fdc4917`. Every anchor was read there — including the fact that **#134 is merged
as a plan but not yet executed**: `godot/world/world.gd` today still builds its fixed environment in
`_add_environment` (`godot/world/world.gd:135-156`) and has no `LOOKS`. This design builds on the shapes #134's plan
prescribes (`docs/superpowers/plans/2026-10-31-world-rendering.md`, Task 1: `LOOKS`, `look_for`, `_apply_look`, nodes
`Sky`/`Sun`), and its Godot task cannot execute before that pair does. Re-verify at execution.

## Intent

A pilot flying their own world file should be able to give it evening air, a dirty haze or a red desert without
touching GDScript — and the dress renders *under* the video chain exactly as #134's does, with the analog link still
the picture's judge. A world without the section is dressed exactly as today, bit for bit.

## Goals

- `[look]` in a world file, every key optional, validated with named field paths like every other section.
- The look travels on `GetWorld` with per-field presence: an absent section, a bare `[look]` and a partially-filled
  one are all distinguishable and all degrade gracefully to the name-keyed dress.
- The Godot client applies it through the same `_apply_look` path (one merge helper in front, no new nodes, no
  application fork); both cameras and the goggles see it.
- The Rust client, the bridge and the Python client all expose it (the m8a rule: every wire field reaches every
  client).
- No simulator behaviour changes: no model, no bus signal, no digest, no physics read. The look is passenger data on
  the world, like an object's colour.

## Non-goals and anti-goals

- No physics, no collision, no RF or video-link change of any kind; the section is scenery. `crates/ofs-radio`,
  `crates/ofs-video`, `ofs-sim`'s link code and every golden digest are untouched.
- No video-chain change (`godot/ui/video.gd`, the lens shaders, `osd.gd`, `dvr.gd`): the look renders under the chain
  (#134's rule, extended to data).
- No time-of-day or weather: the look is one static mood. No animation, no keyframing.
- The quality tiers (`world.gd`'s `QUALITY`) are untouched — the look dresses a world, the tier prices it.
- No shipped world gains a `[look]` (Design call 4). No UI: no settings row, no menu.
- Anti-goal: any world without the section changing by one bit — its flights, its bus digest, its signal list, its
  dress (the binding bit-for-bit rule, same as `[terrain]`'s).

## The vocabulary (what a `[look]` can say)

Ruling: the whole coded table travels, with one re-expression. A subset would fork the vocabulary — two sources of
truth for what a look is, and a user world forever a second-class citizen that cannot say something "Flat field"
can. The 18 fields of #134's `LOOKS` (`docs/superpowers/plans/2026-10-31-world-rendering.md`, Task 1), with the sun's
Godot Euler triple replaced by two NED-native angles:

| Field (TOML key) | Type | Meaning |
|---|---|---|
| `sky_top`, `sky_horizon`, `ground_horizon`, `ground_bottom` | `[r, g, b]`, each 0..=1 | the `ProceduralSkyMaterial`'s four colours |
| `sun_az_deg` | f64 | where the sun stands: 0 = north, 90 = east, clockwise — the file's own convention (`pilot.facing_deg`, gates' `heading_deg`), not Godot's Euler |
| `sun_elev_deg` | f64 | the sun above the horizon, degrees up |
| `sun_color` | `[r, g, b]` | the `DirectionalLight3D`'s colour (the sky's sun disc follows it) |
| `sun_energy`, `ambient_energy` | f64 | the light's energy; sky-sourced ambient |
| `fog_color` | `[r, g, b]` | depth/height fog colour |
| `fog_density` | f64 | exponential depth fog (shipped looks: 0.0015–0.004) |
| `fog_sky_affect` | f64 | how much the fog washes the sky, 0..1 |
| `fog_height`, `fog_height_density` | f64 | height fog (a world-Y in metres, and its density) |
| `ground_base`, `ground_line`, `ground_major` | `[r, g, b]` | the grid shader's three colour uniforms |
| `patch_color` | `[r, g, b]` | the ground variation's tint |
| `patch_strength` | f64 | 0 = today's flat ground; 1 = fully patched |

Two fields are atomic: a colour is set whole (it is one `Vec3` on the wire), and the sun is set as **both**
`sun_az_deg` and `sun_elev_deg` or neither — a half-set sun is a validation error, not a guess. Everything else is
independently optional.

The two height-fog fields ride #134's own condition: its plan says re-verify on 4.7.2 that they behave on
`gl_compatibility` and, if not, drop them. If #134's ledger dropped them, they are not carried here either — the
vocabulary is the `LOOKS` table as it stands on main at execution.

## Precedence: the file wins, per field

The client's dress is decided by one merge, in `world.gd`:

- **Absent section, bare `[look]`, or a section that sets nothing** → exactly the name-keyed look of #134:
  `look_for(world["name"])`. The five shipped names dress from the code table, an unknown name wears `default`,
  an empty world dict (a load under way) keeps the last look. Bit for bit today's behaviour.
- **A section that sets some fields** → start from `look_for(world["name"])` and overwrite, key by key, only what
  the file set. A shipped-named world with `[look] fog_density = 0.006` keeps its coded sky, sun and ground and
  thickens the air; a user world starts from `default` and takes the author's tweaks. Whole-section replacement was
  weighed and rejected: discarding the coded mood the moment any field is set is the surprising behaviour, and
  per-field merge needs no second rule for the shipped five.
- **The file never undresses**: setting a field always dresses; there is no "back to default" per-field escape (an
  author who wants the default's value writes it).

A world with both a name in the table and a `[look]` is exactly the partial case above: the name picks the base, the
file wins per field.

## Which worlds ship one: none

No `worlds/*.toml` gains a `[look]`. Reasons: (1) the shipped five already dress from the code table and shipping
the section would move their rendering from code to data — a regression surface with zero user value, and it would
re-open #163's eyeball pass for all five; (2) the code table stays load-bearing anyway — it is the fallback for any
world without the section and the merge base for partial ones, so migration would duplicate every value into TOML
without deleting anything; (3) the diff stays purely additive and the e2e's pins (like the "Flat field" name check,
`godot/tests/e2e_open_loop.gd:207`) move nothing. A config test fences `worlds/*.toml` against quietly gaining one;
migration, if ever wanted, is its own PR.

## Validation (in `WorldConfig::validate`, paths `look.<field>`)

Every rule names its field and shows the value, the house style (the `Checker`, `crates/ofs-config/src/lib.rs:1017`,
used throughout `world.rs`'s validate):

- **Every colour**: finite, each component in [0, 1] — the `objects[i].color` rule (`world.rs:548`), one message per
  colour ("each value must be in [0, 1]").
- `sun_az_deg`: finite, in [-180, 180]. `sun_elev_deg`: finite, in [-90, 90] (below -90 the sun shines up from
  under the world). Both or neither: "the sun needs both `sun_az_deg` and `sun_elev_deg`".
- `sun_energy`, `ambient_energy`: finite, in [0, 16] — the shipped looks span 0.7–1.25; 16 is a generous ceiling
  that still catches a typo'd 110.
- `fog_density`: finite, in [0, 0.05] — at 0.05 a target 60 m out is ~95% fog; beyond that the world is invisible,
  which is a mistake, not a mood.
- `fog_sky_affect`, `fog_height_density`, `patch_strength`: finite, in [0, 1] (their meaningful ranges; the shader
  clamps `patch_strength` anyway — validate the file, not the GPU).
- `fog_height`: finite, in [-1000, 1000] (a world-Y in metres; the park's coded value is -1.0).
- `deny_unknown_fields` on the section (every section's rule), and a bare `[look]` is legal and sets nothing — the
  `[terrain]` friendliness (`crates/ofs-config/src/terrain.rs:17-20`), harmless as a no-op.

## The wire

- **Protocol number: the next free one at execution** — read `crates/ofs-proto/src/lib.rs`'s `PROTOCOL_VERSION`
  (16 at `fdc4917`), take the next, bump the three places (`crates/ofs-proto/src/lib.rs:12`,
  `python/ofs/client.py:26`, `crates/ofs-proto/tests/messages.rs:8`), regenerate the Python stubs
  (`docs/dev-setup.md:21`), and update the live "protocol 16" references outside `docs/superpowers`
  (`docs/dev-setup.md:126,282,290,299` at `fdc4917` — grep, do not trust the line numbers). The Consequence wave's
  pairs (#28, #156, #157) queue first; this pair executes after whichever of them has merged.
- **`Look` message**: the ten colours as `Vec3` (message fields carry presence in proto3), the nine scalars as
  `optional double` (the `Fault.at_s` shape, `proto/ofs/v1/sim.proto:376-377`), fields numbered 1..19 in the
  vocabulary's order, the sun as the two angles. Unset fields do not travel; an unset look message never appears.
- **`World.look`**: the `Look` message at the next free field on `World` — 12 at `fdc4917` (`terrain = 11` is the
  last, `sim.proto:477`); re-verify at execution, the Consequence pairs add to `WorldObject`, not `World`, but the
  standing rule is "take the next free field", not "take 12".
- `world_msg` (`crates/ofs-sim/src/session.rs:238`) fills `look` from `WorldConfig.look`; nothing else in `ofs-sim`
  reads it. No `State` field, no event, no handshake change beyond the number.

## The clients

- **Rust (`crates/ofs-client`)**: `WorldLook` beside the other world types in `model.rs` — ten `Option<[f64; 3]>`
  colours, `Option<f64>` scalars, the sun as `sun_az_deg`/`sun_elev_deg` — and `World.look: Option<WorldLook>`
  (`model.rs:310-329` the struct, `from_pb` the fill). Frame math stays out: NED angles are the world's own.
- **The sun's conversion lives with the frame conversions** (`crates/ofs-client/src/frames.rs:11`,
  `vec_to_godot`): `sun_rotation_deg(az_deg, elev_deg) -> DVec3 = (-elev, 180 - az, 0)` in degrees. Under Godot's
  `DirectionalLight3D` Euler this puts the sun at azimuth `az` (clockwise from north) and elevation `elev` up; the
  shipped default's coded `Vector3(-50, 35, 0)` (`world.gd:152` today) is az 145°, elev 50° under it. The formula
  is analytic; a Godot round-trip test over `Basis` is the authority (if it fails, the formula's branch is wrong —
  fix the formula, not the test).
- **The bridge (`crates/ofs-godot`)**: `get_world()` gains a `"look"` key, present only when the world has one
  (`lib.rs:462-557` the function, `Color::from_rgb(x as f32, ...)` the colour pattern at `:482`): the ten colours
  as `Color`, the seven non-sun scalars as floats, and `sun_rotation_deg` as a `Vector3` **converted here** — so
  the dictionary's keys are exactly `LOOKS`' keys and GDScript never sees an azimuth.
- **Python (`python/ofs`)**: a frozen `Look` dataclass, every field `None` by default (the colours as tuples), and
  `World.look: Look | None = None` (`client.py:185-199` the `World`, `_world` at `:339` the fill). The terrain grid
  does not travel to Python because it is millions of floats; a look is nineteen numbers and does.

## The Godot application

One helper in front of #134's path, nothing behind it changes:

```gdscript
## The look the world dict wears: the one its name codes, dressed by the fields its file set (`look`, when the
## world has a section). The dict's look keys are this table's keys.
static func merged_look(world: Dictionary) -> Dictionary:
	var look := look_for(String(world.get("name", ""))).duplicate()
	for key in world.get("look", {}):
		look[key] = world["look"][key]
	return look
```

`build`'s dress line becomes `_apply_look(merged_look(world))` inside the existing `if world.has("name")` guard
(#134's plan, Task 1) — a dict without a name still changes nothing, and `duplicate()` is load-bearing: `LOOKS`' inner
dictionaries are shared references, and merging into one of them would corrupt the table for every later build.
`_apply_look` itself, the one-environment/one-sun counts, and the quality tiers are untouched.

## Tests

- `ofs-config`: a section parses and round-trips; partial keeps its `None`s; every validation rule fires once with
  its path (`look.sky_top: each value must be in [0, 1]`, the half-sun, the ceilings); a bare `[look]` is a no-op;
  **no shipped world has a look** (the fence of Design call 4).
- `ofs-proto`: the version test pins the new number and the Python file together (the existing
  `the_protocol_version_matches_the_python_client`).
- `ofs-sim`: `world_msg` carries a hand-loaded look and carries none for a section-less world; the gRPC flat-world
  reply has no `look`.
- `ofs-client`: `from_pb` fills a `WorldLook` and leaves `World.look` `None` without one;
  `frames::sun_rotation_deg` maps (145, 50) to `(-50, 35, 0)` and wraps other quadrants sanely.
- `ofs-godot`: the look dictionary carries the table's keys (ten `Color`s, the seven scalars, the converted
  `sun_rotation_deg`).
- Python: a temp world file with a `[look]` loads and `get_world().look` reads back its fields; without the
  section `look` is `None`.
- Godot (`test_scene.gd`): a world dict with a partial `look` over a coded name dresses the coded look plus the
  file's fields; over an unknown name it starts from `default`; no `look` key dresses from the name alone exactly
  as #134 did; repeated builds with different looks do not corrupt `LOOKS` (the `duplicate()` pin). A Godot
  round-trip test pins the sun: `Basis(sun_rotation_deg)` puts the sun where az/elev say, and az 145/elev 50 lands
  on today's default rotation. One e2e line: the flat world's dict has no `look` key.

## Docs and bookkeeping

`docs/dev-setup.md`'s world-file list gains the `[look]` bullet (after `[terrain]`, `docs/dev-setup.md:114-129`);
`python/README.md`'s world section gains `World.look`; `CHANGELOG.md` gains the entry and the breaking-protocol note;
the proto header's version comment says what N added. The by-hand eyeball (a user world's dress surviving the analog
chain) lands in #163's list, as #134's did.

## Design calls (decided here)

1. **The whole table vocabulary travels, minus the Euler.** A subset forks the vocabulary; the sun's Godot Euler
   becomes `sun_az_deg`/`sun_elev_deg` because a world file speaks NED angles, and the client converts.
2. **Per-field precedence: the file wins over the look the name would wear.** Absent/partial/empty all degrade to
   the name-keyed look; per-field, not whole-section, because replacing the base on any touch is the surprise.
3. **Colours and the sun are atomic; everything else is per-key.** One `Vec3` per colour; both sun angles or
   neither — the merge stays uniform and the wire stays honest about what was set.
4. **No shipped world gains a `[look]`.** Zero shipped-world rendering changes, the code table stays load-bearing,
   the diff is additive; a test fences the five.
5. **`Look` on the wire with per-field presence; `World.look` at the next free field; the protocol number is the
   next free one at execution.** The Consequence wave's bumps queue first; never a fixed 17.
6. **The look is passenger data.** No model, no signal, no digest, no physics read — the bit-for-bit rule holds by
   construction, not by care.
7. **The bridge converts the sun, GDScript stays vocabulary-uniform.** `sun_rotation_deg` is a `Vector3` in the
   dictionary, so `_apply_look` is unchanged and `LOOKS`' keys are the only keys.
8. **One merge helper, `merged_look`, in front of `_apply_look`.** `duplicate()` because the table's inner
   dictionaries are shared; the no-name rule and the quality tiers are untouched.
9. **All three clients expose it.** The m8a rule; Python gets the small dataclass the terrain never got.

## Open questions for the maintainer

1. **The scalar ceilings** (energy 16, fog density 0.05, fog height ±1000 m) are generous sanity caps chosen to
   catch typos, not taste — loosen or tighten them and the validation and its tests move together.
2. **Whether the five shipped worlds should ever migrate their dress into their TOMLs** is ruled no for this issue
   (call 4); if you want the code table gone eventually, that is its own PR and its own eyeball pass.

## Done when

- A world file can set its sky, sun, fog and ground (the table's whole vocabulary) and the Godot client dresses the
  field with it, in both cameras, under the unchanged analog chain.
- An absent or partial look keeps the name-keyed dress exactly (bit for bit for section-less worlds); a world with
  both a coded name and a `[look]` wears the file's fields over the coded mood.
- No shipped world file changed; no golden digest, video-chain file or physics test changed; the protocol bump
  carries the regeneration choreography and every live protocol reference is updated.
- `cargo test --workspace --locked --no-fail-fast`, `cargo clippy --workspace --all-targets --locked -- -D
  warnings`, `PYTHONPATH=python python -m pytest python/tests -q` and `bash scripts/run-godot-tests.sh all` (WSL,
  whole output read) pass.
