# Better world rendering (#134, under epic #136)

The world renders as flat-shaded boxes on a plane — honest, and gray (issue #134). The analog link's degradation is
the goggles' character and stays; this makes what is under it look like a place: a per-world sky, sun and distance
fog, a ground that varies, and a quality setting so the low end holds today's frame rate. Everything here is Godot-side
rendering; no simulator model, no protocol, no world file changes.

Written 2026-10-31 against `main` at `87c5914` (the worktree `docs/game-completion-plan`); every anchor was read there.

## Intent

A pilot flying `park` should feel walled-in evening air, `meadow` should read as a big bright field with treelines
fading into haze, and a user's own world should fall back to today's honest grey rather than look broken. The
upgraded world is rendered *under* the video chain, so the goggles still show static, noise and breakup over it —
better-looking, not cleaner (the epic #136 rule).

## Goals

- A rendering pass over the world on the Godot side of `godot/world/world.gd`: per-world **sky, sun (direction, color,
  energy), ambient energy, distance fog, and ground tinting**, from one name-keyed look table.
- **Ground that varies**: shader-math patches (deterministic hash value-noise) and per-world tint uniforms over the
  existing metre grid — no texture files.
- **Distance fog** on every world (depth + height fog, the modes the GL Compatibility renderer supports since Godot
  4.3) for depth cues on the big worlds.
- Visible in **both cameras** (FPV and chase share `FpvViewport/World`) and through the **goggles** unchanged: the
  video chain (`godot/ui/video.gd`, the lens shader) keeps deciding the picture's character.
- A **quality setting** — low/medium/high — in the settings screen, riding the existing settings chain
  (`user://ofs_client.cfg` < `OFS_QUALITY` < `--quality`), applied live.
- The Godot unit suites and e2e suites still pass.

## Non-goals

- Terrain (#155), rotated boxes (#28), new physics or collisions of any kind: the worlds' physics stays what it is.
  `hills.toml` keeps drawing a flat ground with a sky mood over it until #155.
- Any change to the video link model, its Rust tests, or any golden digest: `crates/ofs-video`, `crates/ofs-radio`,
  `crates/ofs-sim` video code, `godot/ui/video.gd`, `video.gdshader`, `lens.gd`/`lens.gdshader`, `osd.gd`, `dvr.gd` are
  untouched.
- A `[look]` section in world files or per-world rendering data on the wire (a protocol bump); selectable time of day;
  a weather system. The look table is GDScript keyed by the world's `name` (`get_world()`'s only per-world identity,
  `crates/ofs-godot/src/lib.rs:517`); a data-driven look is a follow-up if the maintainer wants user worlds dressable.
- New assets: no textures, models, or `.import` changes (the headless first-import trap; the #130 ruling precedent —
  PR #147 built the quad procedurally for the same reason).
- The drone's materials and the quads (#130's lane), menus beyond one settings row, post effects.

## Anti-goals

- Anything that makes the goggles picture *cleaner*: no bloom/glow, no sharpening, no auto-exposure. Glow under the
  analog chain would read as a healthier link — the opposite of the issue's ask — so the compatibility renderer's glow
  stays off at every tier.
- Anything that forks the settings chain or breaks the e2e menu-less boot path (epic rules from the phase umbrella,
  `docs/superpowers/plans/2026-10-31-game-completion.md`).

## What "better" means, concretely

Today's environment is built once in `world.gd::_add_environment` (`godot/world/world.gd:125-146`): one
`ProceduralSkyMaterial` with fixed colors, one `DirectionalLight3D` at `(-50°, 35°, 0°)` with shadows to 150 m, ambient
from the sky at 0.8. The ground is a 6000 m `PlaneMesh` under `grid.gdshader` (`godot/world/grid.gdshader`) — grey-green
with a 1 m grid. There is no fog.

The change: **one look per world, one quality tier table, one shader upgrade.**

### The look of a world (the table)

`world.gd` gains `const LOOKS` (a Dictionary) and `static func look_for(world_name: String) -> Dictionary`. Keys: the
five shipped world names — `"Flat field"`, `"Proximity park"`, `"Open meadow"`, `"Rolling hills"`, `"Circuit"` (the
`name` values in `worlds/*.toml`, which the e2e already pins, `godot/tests/e2e_open_loop.gd:207`) — plus
`"default"` for anything else. Each look carries:

| Field | Meaning |
|---|---|
| `sky_top`, `sky_horizon`, `ground_horizon`, `ground_bottom` | `ProceduralSkyMaterial` colors |
| `sun_rotation_deg`, `sun_color`, `sun_energy` | the `DirectionalLight3D` (the sky's sun disc follows it) |
| `ambient_energy` | sky-sourced ambient |
| `fog_color`, `fog_density`, `fog_sky_affect`, `fog_height`, `fog_height_density` | depth/height fog |
| `ground_base`, `ground_line`, `ground_major` | the grid shader's existing color uniforms |
| `patch_color`, `patch_strength` | the ground's new variation (below); `patch_strength = 0` is today's flat ground |

The default look is **exactly today's environment plus a light haze** (fog density 0.0015): a user's world, or any
dict without a `"name"`, keeps the sim's honest grey. `build(world)` applies the look by name each time a world
arrives, by **mutating the one environment, sky, sun and ground material in place** — never by adding nodes
(`test_scene.gd:36-37` counts one `WorldEnvironment` and one `DirectionalLight3D`, and those counts stay). An empty
dict (a load under way) changes nothing: the last look stays until a named world replaces it.

Art direction (values below are estimates, the way part parameters are; they get eyeballed in #163):

- **Flat field** — today's midday sky and sun, a slightly warm sun color, fog 0.002, mild dirt patches (0.30).
- **Proximity park** — late afternoon: sun at `(-28°, 55°, 0°)`, color `(1.0, 0.86, 0.68)`, energy 1.2; warm horizon;
  greener ground; thicker air (fog 0.004 — a tight world reads well in haze).
- **Open meadow** — bright morning: sun `(-60°, 20°, 0°)` near-white, energy 1.25; big-sky blue; meadow-green ground;
  fog 0.0018 with `fog_sky_affect` 0.15 so the far treelines fade at 250 m.
- **Rolling hills** — golden hour: sun `(-18°, 70°, 0°)`, color `(1.0, 0.80, 0.55)`; dusk horizon; fog 0.003 with
  `fog_sky_affect` 0.3. (The ground stays flat until #155; the mood is sky and light.)
- **Circuit** — neutral overcast: sun `(-65°, 10°, 0°)`, cool white, energy 0.9; grey-blue sky; asphalt-leaning ground;
  fog 0.0025.

### The ground

`grid.gdshader` keeps its identity — the 1 m grid with 10 m majors is the flat world's depth cue — and gains a
variation layer: two octaves of hash value-noise on world position (cells ~6 m and ~23 m), mixing `base_color` toward
`patch_color` by `patch_strength`. Pure fragment math, deterministic, tiling-free at 6000 m, no UVs, no textures. The
existing `fwidth` fade that kills moiré at distance stays; the patch layer rides the same fade so it cannot swallow
the grid. `world.gd` sets the five color uniforms and `patch_strength` from the look (the shader's uniform defaults
keep today's ground, so an undressed world is unchanged).

### Fog

`Environment.fog_enabled` with exponential **depth fog** plus optional **height fog** — the two modes the GL
Compatibility renderer supports (since Godot 4.3; volumetric fog is Forward+-only and is ruled out). Fog is on at
every quality tier: it is a per-pixel mix, effectively free, and it is the issue's headline depth cue. Density is
small (0.0015–0.004): at 100 m the meadow is ~16% fog, the park ~33% — a cue, not pea soup. Re-verify at execution on
4.7.2 that `fog_height`/`fog_height_density` behave on `gl_compatibility` (`godot/project.godot:19`); if height fog
misbehaves, drop those two fields (park keeps density alone) and note it in the ledger.

### Quality

One setting, `quality`, values `low` / `medium` / `high`, default **medium** (today's cost). The tier table lives in
`world.gd` (`const QUALITY`, `static func quality_spec(tier) -> Dictionary`, `func set_quality(tier)`); the application
path is one `apply_quality()` in `app.gd` that sets the viewport MSAA, forwards it to the DVR clean feed, and calls
`world.set_quality()`:

| | low | medium (default) | high |
|---|---|---|---|
| Sun shadows | off | on, 150 m (today, `world.gd:144-145`) | on, 400 m |
| Viewport MSAA (`msaa_3d`) | disabled (0) | 4x — today's `main.tscn:24` value `2`, which is `MSAA_4X`, not 2x | 8x (the docs warn it may be unsupported on low-end GPUs; worst case it silently degrades) |
| Sky, looks, ground variation, fog | on | on | on |

- **low is below today's cost** (today has shadows at 150 m and MSAA 4x), which is what "the low end holding today's
  frame rate" needs.
- The setting is **live** (applies at once, like Sound and Fullscreen), persists through `save_value`, resolves from
  file < `OFS_QUALITY` < `--quality`, and an unknown value falls back to medium (the `dvr_feed` pattern,
  `godot/scripts/settings.gd:112-114`).
- MSAA changes reach the DVR's clean viewport too (`clean_feed.gd:18` copies MSAA only at setup), through a small
  `clean_feed.set_msaa()`.

## How it stays under the video chain

Everything dresses nodes inside `FpvViewport/World` (`godot/main.tscn:26`), which both the FPV camera and the chase
camera render, and which the analog layer (`godot/ui/video.gd`, CanvasLayer 3) and the lens degrade exactly as
before. No video-chain file changes, no exposure or brightness compensation for the link — the look must survive
degradation by contrast, and that is an #163 eyeball item, not code.

## Constraints

- GDScript house style: tabs, snake_case, `##` doc comments; files stay LF.
- Godot tests run in WSL: `bash scripts/run-godot-tests.sh unit|open-loop|all` (the script builds `ofs-sim` and
  `ofs-godot` itself and downloads Godot 4.7.2); read the whole output — it prints `0 failed` even after a SCRIPT
  ERROR.
- No `cargo` invocations beyond what the test script runs; no `worlds/*.toml` edits (their recordings, goldens and
  config tests stay byte-stable).
- Protocol stays 15. Nothing crosses to the server; `quality` is client-local like `fpv_aspect` (excluded from
  `to_client_dict`, `godot/scripts/settings.gd:209-226`).

## Design calls (decided here)

1. **Godot-side only.** No Rust, no protocol, no world TOML edits — the mission's scope; the link model and goldens
   are untouchable, and the look needs no data the client lacks.
2. **Looks keyed by the world's `name` in a GDScript table.** `get_world()`'s `name` is the only per-world identity on
   the client; unknown names get today's look (default), so user worlds never look broken. A `[look]` world section
   would need protocol 16 — deferred, not designed here.
3. **One environment, one sun, mutated in place.** `build()` re-dresses the existing resources; the node counts the
   tests pin stay, and repeated builds cannot stack suns.
4. **Depth + height fog only; fog on every tier.** The GL Compatibility renderer supports exactly these since 4.3;
   volumetric is Forward+-only. Fog is ~free and is the point of the issue.
5. **No glow/bloom/post at any tier.** Bloom under the analog chain would read as a cleaner link; the chain owns the
   picture's character.
6. **Ground variety is shader math, not textures.** No import step headless, no licence/provenance question
   (the #130 ruling precedent), no tiling at 6000 m.
7. **The grid stays.** The metre grid is the flat world's depth cue and the sim's grey-box honesty; it gets tinted,
   not removed.
8. **`quality`: low/medium/high, default medium, live.** low = shadows off + MSAA off (below today's cost); medium =
   today's 150 m shadows + today's MSAA 4x; high = 400 m shadows + MSAA 8x. One table in `world.gd`, applied by one
   `apply_quality()` in `app.gd`.
9. **The default look is today's look plus a light haze.** Unknown worlds and unnamed dicts keep the sim's identity;
   only the five named worlds dress.
10. **An empty world dict changes nothing.** A load under way must not flash the default look over the last one.

## Open questions for the maintainer

None blocking — every decision above is ruled. Two judgement calls to re-tune after seeing it fly (both are #163
eyeball items, not blockers): the five palettes' hues, and whether the chase view should carry the same fog strength
as the goggles view (today's ruling: yes — fog is world, not goggles).

## Done when

- The five shipped worlds dress distinctly by name (sky, sun, fog, ground tint and patches) in both cameras; an
  unknown world name and an empty dict keep today's environment plus the light haze; rebuilding worlds leaves exactly
  one `WorldEnvironment` and one `DirectionalLight3D`.
- The metre grid still reads at 30+ m on every shipped look (eyeball, #163).
- The `quality` setting exists in the settings screen and on the chain (`OFS_QUALITY`, `--quality`), defaults medium,
  falls back to medium on junk, applies live (shadows, MSAA on both viewports), and persists.
- The look is visible *through* degradation: flying meadow behind the barn with the link breaking up still shows the
  world under the static (eyeball, #163); no video-chain file, video-link Rust test or golden changed.
- `bash scripts/run-godot-tests.sh all` passes (SITL betaflight e2e only with `OFS_SITL_LAUNCH`), whole output read.
- CHANGELOG, README (the settings table and one sentence) and `docs/architecture.md` (the Godot client paragraph)
  say it; the by-hand list is written for #163.
