# Better World Rendering (#134) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Godot worlds look like places — a per-world sky, sun and distance fog, a ground that varies under its metre grid — in both cameras and under the same analog video chain, with a low/medium/high quality setting whose low end is cheaper than today. Closes #134.

**Architecture:** One name-keyed look table in `godot/world/world.gd` (the world Dictionary's `name`, the only per-world identity `OfsClient.get_world()` carries) dresses the one environment, sun and ground material in place, re-applied whenever `build` receives a named world. The ground gains a shader-math variation layer (`godot/world/grid.gdshader`, hash value-noise; no textures). One `quality` setting rides the existing settings chain and applies live through a single `apply_quality()` in `app.gd` (viewport MSAA, the DVR clean feed's copy of it, and the sun's shadows via `world.set_quality`). No Rust, no protocol (stays 15), no world TOML edits, no video-chain edits.

**Tech Stack:** GDScript (Godot 4.7.2, tab-indented, GL Compatibility renderer), the Godot spatial shader language, the headless test suites under `godot/tests`.

**Spec:** `docs/superpowers/specs/2026-10-31-world-rendering-design.md` (the design calls there are binding; this plan's tasks argue from them).

## Global Constraints

- **Godot-side only.** No Rust crate, no `proto/`, no `python/`, no `worlds/*.toml` is touched; no `cargo` command is run by hand (the Godot test script builds `ofs-sim` and `ofs-godot` itself).
- **The video chain is untouchable:** `godot/ui/video.gd`, `video.gdshader`, `lens.gd`, `lens.gdshader`, `osd.gd`, `dvr.gd`, and every video-link model, test or golden digest on the Rust side. The look renders *under* the chain.
- **The settings chain is not forked:** `user://ofs_client.cfg` < `OFS_*` < flags; `quality` rides it (`OFS_QUALITY`, `--quality`), persists through `AppSettings.save_value`, and is client-local (never in `to_client_dict`).
- **No new assets:** no textures, models or `.import` changes; the ground variation is fragment math.
- **No post effects:** the GL Compatibility renderer's glow stays off at every tier (spec Design call 5).
- **GDScript house style:** tabs, snake_case, `##` doc comments; files stay LF (`tr -cd '\r' < file | wc -c` prints 0).
- **Godot tests in WSL:** `bash scripts/run-godot-tests.sh unit` (and `open-loop` / `all`), with `OFS_GODOT_DL=/mnt/c/dev/open-fpv-sim/build/godot-dl` when it exists; read the WHOLE output — the suites print `0 failed` even after a SCRIPT ERROR. Worktree-isolated sessions run WSL through a scratchpad script, not inline heredocs. The e2e menu-less boot path keeps working.
- **Baseline:** written against `main` at `87c5914`; re-verify at execution that `godot/world/world.gd` still holds `_add_environment` around line 125 and `_add_ground` around 149, and that `test_scene.gd` still counts one environment and one sun around lines 36-37 (the B6 lesson: anchors can drift).
- **Commits:** explicit `git add <files>` only; one commit per task; no trailer prescribed (recent merged work carries none — an executing session that adds one names its own identity); never push.

## Design calls (decided here; the executor ledgers any change)

1. **Looks keyed by the world's `name`** in a `LOOKS` table in `world.gd`; `look_for(name)` falls back to `"default"`, which is today's environment plus a light haze. A `[look]` world-file section would need protocol 16 — out (spec Design call 2).
2. **One environment, one sun, mutated in place** (`_apply_look` re-dresses the existing resources); `build` applies a look only when the dict has a `"name"`; an empty dict changes nothing (spec calls 3, 10).
3. **Depth + height fog on every tier**; volumetric fog ruled out (Forward+-only). Height fog is park-only; re-verify on 4.7.2 that `fog_height`/`fog_height_density` behave on `gl_compatibility` and, if not, zero them and ledger it.
4. **Ground variation is shader math** (two octaves of hash value-noise under the grid lines, fading with the grid's own `fwidth` fade); the metre grid stays (spec calls 6-7).
5. **`quality`: low/medium/high, default medium, live.** low = shadows off + MSAA off (below today's cost); medium = today's 150 m shadows + today's MSAA (`main.tscn:24`'s `msaa_3d = 2`, which is `MSAA_4X` — the enum counts 0 disabled, 1 2x, 2 4x, 3 8x); high = 400 m shadows + `MSAA_8X` (the docs warn 8x may be unsupported on low-end GPUs; worst case it silently degrades). The tier table lives once in `world.gd`.
6. **No glow/bloom at any tier** — bloom under the analog chain would read as a cleaner link.

## Review Focus

- **A world dict without a `"name"`** (a load under way, or a script-built dict) must keep the last look, not flash the default over it. Pinned by Task 1's `a world without a name keeps the last look`.
- **An unknown world name** (a user's own TOML) must wear the default look, not a blank sky. Pinned by Task 1's `an unknown world wears the default look` and the `quality_spec`/`resolve` fallbacks in Task 3.
- **Repeated builds must not stack environments or suns** (a second sun double-lights the field). Pinned by Task 1's still-one-environment/one-sun counts after three builds.
- **A junk quality value** (`--quality=ultra`, a hand-edited config) must fall back to medium everywhere — the settings resolve, the screen row, `quality_spec` — not index a missing table entry. Pinned by Task 3's settings, screen and world tests.
- **The patch noise must not alias into sparkle at distance** or swallow the metre grid. The shader fades the patch layer with the grid's own `fwidth` fade (Task 2); the visual proof is the #163 eyeball pass (headless renders nothing).
- **A quality change must reach the DVR's clean viewport too**, which copies MSAA only at setup (`clean_feed.gd:18`). Pinned by Task 3's `set_msaa` and the e2e live-application check.

---

### Task 1: The look table — sky, sun, fog, ground tint per world

**Files:**
- Modify: `godot/world/world.gd` (the look table; `_ready`, `_add_environment`, `build`; new `_apply_look`, `look_for`)
- Test: `godot/tests/test_scene.gd` (one new method after `test_the_world_draws_sky_and_ground_and_builds_the_rest_from_the_world_file`)

**Interfaces:**
- Consumes: the world Dictionary `OfsClient.get_world()` returns (its `"name"` key, `crates/ofs-godot/src/lib.rs:517`).
- Produces: `const LOOKS: Dictionary` (keys `"default"`, `"Flat field"`, `"Proximity park"`, `"Open meadow"`, `"Rolling hills"`, `"Circuit"`; each value a Dictionary with `sky_top`, `sky_horizon`, `ground_horizon`, `ground_bottom` (Color), `sun_rotation_deg` (Vector3), `sun_color` (Color), `sun_energy`, `ambient_energy`, `fog_color` (Color), `fog_density`, `fog_sky_affect`, `fog_height`, `fog_height_density` (float), `ground_base`, `ground_line`, `ground_major` (Color)); `static func look_for(world_name: String) -> Dictionary`; nodes named `Sky` (the WorldEnvironment) and `Sun` (the DirectionalLight3D), findable by class as today; `build` applies `look_for(world["name"])` whenever the dict has a `"name"`.

- [ ] **Step 1: Write the failing test** in `godot/tests/test_scene.gd`:

```gdscript
func test_the_world_dresses_from_its_name_in_one_environment() -> void:
	var world := World.new()
	await add_to_tree(world)
	var env: Environment = (world.get_node("Sky") as WorldEnvironment).environment
	var sky: ProceduralSkyMaterial = env.sky.sky_material as ProceduralSkyMaterial
	var sun: DirectionalLight3D = world.get_node("Sun")
	ok(env.fog_enabled, "the default dress carries the light haze")
	ok(World.look_for("Somewhere else") == World.LOOKS["default"], "an unknown world wears the default look")
	world.build({"name": "Proximity park"})
	var park: Dictionary = World.look_for("Proximity park")
	ok(sky.sky_top_color.is_equal_approx(park["sky_top"]), "the park's sky")
	eq(sun.rotation_degrees, park["sun_rotation_deg"], "its low sun")
	ok(sun.light_color.is_equal_approx(park["sun_color"]), "its warm light")
	ok(env.fog_enabled, "fog on")
	near(env.fog_density, park["fog_density"], "its haze")
	world.build({"name": "Open meadow"})
	ok(sky.sky_top_color.is_equal_approx(World.look_for("Open meadow")["sky_top"]), "a new world re-dresses")
	ok(not sky.sky_top_color.is_equal_approx(park["sky_top"]), "in its own colours")
	eq(world.find_children("*", "WorldEnvironment", false, false).size(), 1, "still one environment")
	eq(world.find_children("*", "DirectionalLight3D", false, false).size(), 1, "still one sun")
	var meadow_density: float = World.look_for("Open meadow")["fog_density"]
	world.build({})
	near(env.fog_density, meadow_density, "a world without a name keeps the last look")
	world.queue_free()
```

- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: FAIL — errors and failures confined to the new test (`get_node("Sky")`/`get_node("Sun")` do not exist yet, and `World.LOOKS` is unknown); every other test green.
- [ ] **Step 3: Implement** in `godot/world/world.gd`. Add the table and helpers after the existing consts (values are art-direction estimates from the spec; `default` is today's `world.gd:125-146` environment plus haze):

```gdscript
## The looks the shipped worlds wear (estimates, eyeballed in #163). `default` — today's sky, sun and ground plus a
## light haze — dresses any world whose name is not in the table, and is what the field wears before a world arrives.
const LOOKS := {
	"default": {
		"sky_top": Color(0.30, 0.48, 0.78), "sky_horizon": Color(0.72, 0.80, 0.88),
		"ground_horizon": Color(0.72, 0.80, 0.88), "ground_bottom": Color(0.40, 0.45, 0.40),
		"sun_rotation_deg": Vector3(-50.0, 35.0, 0.0), "sun_color": Color(1.0, 1.0, 1.0), "sun_energy": 1.1,
		"ambient_energy": 0.8, "fog_color": Color(0.72, 0.80, 0.88), "fog_density": 0.0015,
		"fog_sky_affect": 0.1, "fog_height": 0.0, "fog_height_density": 0.0,
		"ground_base": Color(0.30, 0.34, 0.29), "ground_line": Color(0.50, 0.55, 0.48),
		"ground_major": Color(0.82, 0.84, 0.74), "patch_color": Color(0.36, 0.34, 0.26), "patch_strength": 0.0,
	},
	"Flat field": {
		"sky_top": Color(0.30, 0.48, 0.78), "sky_horizon": Color(0.72, 0.80, 0.88),
		"ground_horizon": Color(0.72, 0.80, 0.88), "ground_bottom": Color(0.40, 0.45, 0.40),
		"sun_rotation_deg": Vector3(-50.0, 35.0, 0.0), "sun_color": Color(1.0, 0.97, 0.92), "sun_energy": 1.1,
		"ambient_energy": 0.8, "fog_color": Color(0.74, 0.80, 0.88), "fog_density": 0.002,
		"fog_sky_affect": 0.1, "fog_height": 0.0, "fog_height_density": 0.0,
		"ground_base": Color(0.30, 0.34, 0.29), "ground_line": Color(0.50, 0.55, 0.48),
		"ground_major": Color(0.82, 0.84, 0.74), "patch_color": Color(0.36, 0.34, 0.26), "patch_strength": 0.3,
	},
	"Proximity park": {
		"sky_top": Color(0.34, 0.50, 0.74), "sky_horizon": Color(0.80, 0.76, 0.70),
		"ground_horizon": Color(0.80, 0.76, 0.70), "ground_bottom": Color(0.38, 0.34, 0.28),
		"sun_rotation_deg": Vector3(-28.0, 55.0, 0.0), "sun_color": Color(1.0, 0.86, 0.68), "sun_energy": 1.2,
		"ambient_energy": 0.75, "fog_color": Color(0.80, 0.76, 0.70), "fog_density": 0.004,
		"fog_sky_affect": 0.15, "fog_height": -1.0, "fog_height_density": 0.02,
		"ground_base": Color(0.24, 0.36, 0.20), "ground_line": Color(0.40, 0.54, 0.34),
		"ground_major": Color(0.70, 0.76, 0.60), "patch_color": Color(0.36, 0.32, 0.18), "patch_strength": 0.5,
	},
	"Open meadow": {
		"sky_top": Color(0.36, 0.54, 0.85), "sky_horizon": Color(0.78, 0.84, 0.92),
		"ground_horizon": Color(0.78, 0.84, 0.92), "ground_bottom": Color(0.42, 0.46, 0.40),
		"sun_rotation_deg": Vector3(-60.0, 20.0, 0.0), "sun_color": Color(1.0, 0.99, 0.95), "sun_energy": 1.25,
		"ambient_energy": 0.85, "fog_color": Color(0.78, 0.84, 0.92), "fog_density": 0.0018,
		"fog_sky_affect": 0.15, "fog_height": 0.0, "fog_height_density": 0.0,
		"ground_base": Color(0.30, 0.42, 0.22), "ground_line": Color(0.46, 0.58, 0.36),
		"ground_major": Color(0.80, 0.86, 0.70), "patch_color": Color(0.38, 0.30, 0.18), "patch_strength": 0.35,
	},
	"Rolling hills": {
		"sky_top": Color(0.28, 0.38, 0.60), "sky_horizon": Color(0.85, 0.62, 0.45),
		"ground_horizon": Color(0.85, 0.62, 0.45), "ground_bottom": Color(0.35, 0.30, 0.26),
		"sun_rotation_deg": Vector3(-18.0, 70.0, 0.0), "sun_color": Color(1.0, 0.80, 0.55), "sun_energy": 1.15,
		"ambient_energy": 0.7, "fog_color": Color(0.85, 0.62, 0.45), "fog_density": 0.003,
		"fog_sky_affect": 0.3, "fog_height": 0.0, "fog_height_density": 0.0,
		"ground_base": Color(0.34, 0.36, 0.24), "ground_line": Color(0.52, 0.52, 0.38),
		"ground_major": Color(0.85, 0.80, 0.62), "patch_color": Color(0.42, 0.36, 0.26), "patch_strength": 0.5,
	},
	"Circuit": {
		"sky_top": Color(0.42, 0.48, 0.58), "sky_horizon": Color(0.72, 0.76, 0.82),
		"ground_horizon": Color(0.72, 0.76, 0.82), "ground_bottom": Color(0.40, 0.42, 0.46),
		"sun_rotation_deg": Vector3(-65.0, 10.0, 0.0), "sun_color": Color(0.95, 0.95, 1.0), "sun_energy": 0.9,
		"ambient_energy": 0.9, "fog_color": Color(0.72, 0.76, 0.82), "fog_density": 0.0025,
		"fog_sky_affect": 0.2, "fog_height": 0.0, "fog_height_density": 0.0,
		"ground_base": Color(0.32, 0.33, 0.34), "ground_line": Color(0.50, 0.52, 0.53),
		"ground_major": Color(0.80, 0.82, 0.82), "patch_color": Color(0.26, 0.26, 0.28), "patch_strength": 0.25,
	},
}


## The look `world_name` wears: its own, or the default for any name the table does not carry.
static func look_for(world_name: String) -> Dictionary:
	return LOOKS.get(world_name, LOOKS["default"])
```

Rework the environment plumbing (the `patch_color`/`patch_strength` fields are data only until Task 2's shader reads them — do not set those two parameters yet, the shader has no such uniforms):

```gdscript
var _sky_material: ProceduralSkyMaterial = null
var _environment: Environment = null
var _sun: DirectionalLight3D = null
var _ground_material: ShaderMaterial = null


func _ready() -> void:
	_add_environment()
	_add_ground()
	_apply_look(LOOKS["default"])
```

`_add_environment()` creates the nodes bare and named (no colors — `_apply_look` owns them all): `Sky` (WorldEnvironment holding `_environment`, with `background_mode = Environment.BG_SKY`, the Sky holding `_sky_material`, `ambient_light_source = Environment.AMBIENT_SOURCE_SKY`) and `Sun` (`shadow_enabled = true`, `directional_shadow_max_distance = 150.0` — today's values stay until Task 3's tier table owns them). `_add_ground()` stores the ShaderMaterial in `_ground_material` (node name stays `Ground`). `_apply_look(look: Dictionary) -> void` assigns every field of the look to those four resources (sky colors, sun rotation/color/energy, ambient energy, the six fog properties — `fog_enabled = true`, `fog_light_color = look["fog_color"]`, `fog_density`, `fog_sky_affect`, `fog_height`, `fog_height_density` — and the ground's `base_color`, `line_color`, `major_color` shader parameters), and `build` gains, right after the `_built.clear()` block:

```gdscript
	if world.has("name"):
		_apply_look(look_for(world["name"]))
```

Update the class doc comment's first line ("The grey-box world" → the world dressed per its look, still scenery-only) and the `GRID_SHADER`/const block if it names the fixed sky.

- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: PASS (the new test green; `test_the_world_draws_sky_and_ground_and_builds_the_rest_from_the_world_file` still green — `_world_dict()`'s name `"Test field"` is not in the table, so it wears the default; the one-environment/one-sun counts unchanged). Read the whole output for SCRIPT ERRORs.
- [ ] **Step 5: Commit** `feat(godot): the worlds dress from a name-keyed look table (#134)` — `git add godot/world/world.gd godot/tests/test_scene.gd`.

### Task 2: The varied ground under the metre grid

**Files:**
- Modify: `godot/world/grid.gdshader` (the patch layer), `godot/world/world.gd` (`_apply_look` sets the two new parameters)
- Test: `godot/tests/test_scene.gd` (one new method)

**Interfaces:**
- Consumes: Task 1's `LOOKS` fields `patch_color` and `patch_strength`.
- Produces: shader uniforms `patch_color` (source_color, default `vec3(0.36, 0.34, 0.26)`), `patch_strength` (0..1, default `0.0`) and `patch_scale` (metres, default `6.0`); the ground drawn as base color mixed toward patches of `patch_color` by `patch_strength`, under the grid lines.

- [ ] **Step 1: Write the failing test** in `godot/tests/test_scene.gd`:

```gdscript
func test_the_ground_wears_the_worlds_tint_and_variation() -> void:
	var world := World.new()
	await add_to_tree(world)
	var material: ShaderMaterial = (world.get_node("Ground") as MeshInstance3D).material_override as ShaderMaterial
	near(material.get_shader_parameter("patch_strength"), 0.0, "the default ground is today's flat grey-green")
	world.build({"name": "Open meadow"})
	var meadow: Dictionary = World.look_for("Open meadow")
	ok((material.get_shader_parameter("base_color") as Color).is_equal_approx(meadow["ground_base"]), "meadow green")
	ok((material.get_shader_parameter("patch_color") as Color).is_equal_approx(meadow["patch_color"]), "with dry patches")
	near(material.get_shader_parameter("patch_strength"), meadow["patch_strength"], "at their strength")
	world.build({"name": "Flat field"})
	ok((material.get_shader_parameter("base_color") as Color).is_equal_approx(World.look_for("Flat field")["ground_base"]), "the launch pad's field")
	world.queue_free()
```

- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: FAIL — errors and failures confined to the new test (the shader has no `patch_strength` uniform, so the read returns null and the `near` errors or fails); every other test green.
- [ ] **Step 3: Implement.** In `godot/world/grid.gdshader`, add the uniforms next to the existing ones and the noise helpers above `fragment()`:

```glsl
uniform vec3 patch_color : source_color = vec3(0.36, 0.34, 0.26);
uniform float patch_strength = 0.0;
uniform float patch_scale = 6.0;

// Deterministic ground variation: hash value-noise on world position, no UVs, no tiling at any size.
float hash(vec2 p) {
	return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float vnoise(vec2 p) {
	vec2 i = floor(p);
	vec2 f = fract(p);
	f = f * f * (3.0 - 2.0 * f);
	return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x),
			mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y);
}
```

and in `fragment()`, mix the patch layer in under the grid lines, fading with the grid's own distance fade so neither the patch nor the line sparkles at range:

```glsl
void fragment() {
	vec2 p = world_pos.xz;
	vec3 color = base_color;
	vec2 w6 = fwidth(p / patch_scale);
	float patch_fade = 1.0 - smoothstep(0.15, 0.5, max(w6.x, w6.y));
	float patch = (vnoise(p / patch_scale) * 0.65 + vnoise(p / (patch_scale * 4.0)) * 0.35) * patch_fade;
	color = mix(color, patch_color, clamp(patch_strength, 0.0, 1.0) * patch);
	color = mix(color, line_color, grid(p, cell, 1.0) * 0.55);
	color = mix(color, major_color, grid(p, cell * major_every, 2.0) * 0.8);
	ALBEDO = color;
	ROUGHNESS = 1.0;
}
```

Update the shader's header comment (the ground now varies per world; the grid stays). In `world.gd`'s `_apply_look`, after the three existing ground parameters:

```gdscript
	_ground_material.set_shader_parameter("patch_color", look["patch_color"])
	_ground_material.set_shader_parameter("patch_strength", look["patch_strength"])
```

- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: PASS, whole output read (a shader parse error shows as an ERROR line, not a test failure).
- [ ] **Step 5: Eyeball once if a display is available** (headless proves nothing about a shader): `godot --path godot -s res://tests/shots.gd -- --out=<dir>` — the ground in `start_fpv.png` carries soft tonal patches and the metre grid still reads; otherwise state that the visual check was not done (it lands in #163 either way).
- [ ] **Step 6: Commit** `feat(godot): a varied ground under the metre grid (#134)` — `git add godot/world/grid.gdshader godot/world/world.gd godot/tests/test_scene.gd`.

### Task 3: The quality setting — shadows, MSAA, live

**Files:**
- Modify: `godot/world/world.gd` (the tier table, `set_quality`), `godot/scripts/settings.gd`, `godot/ui/settings_screen.gd`, `godot/scripts/app.gd`, `godot/ui/clean_feed.gd`
- Test: `godot/tests/test_scene.gd`, `godot/tests/test_settings.gd`, `godot/tests/test_settings_screen.gd`, `godot/tests/e2e_open_loop.gd`

**Interfaces:**
- Consumes: the settings chain (`defaults`/`ENV_NAMES`/`FLAG_NAMES`/`resolve`, `godot/scripts/settings.gd:9-47,95-115`), the screen's `ROWS` (`godot/ui/settings_screen.gd:11-24`), `app.gd`'s `_on_setting_changed` match (`godot/scripts/app.gd:404-425`).
- Produces: in `world.gd` — `const QUALITY := {"low": {"shadow_m": 0.0, "msaa": Viewport.MSAA_DISABLED}, "medium": {"shadow_m": 150.0, "msaa": Viewport.MSAA_4X}, "high": {"shadow_m": 400.0, "msaa": Viewport.MSAA_8X}}`, `static func quality_spec(tier: String) -> Dictionary` (fallback `medium`), `func set_quality(tier: String) -> void` (applies the shadow half to `Sun`); in `settings.gd` — `const QUALITIES := ["low", "medium", "high"]`, default `"quality": "medium"`, `"OFS_QUALITY": "quality"`, `"--quality": "quality"`, and the `resolve` fallback to `medium` (the `dvr_feed` pattern); in `settings_screen.gd` — the live row `{"key": "quality", "label": "Graphics quality", "kind": "choice", "values": AppSettings.QUALITIES, "live": true}` after "Picture"; in `app.gd` — `const WorldScript = preload("res://world/world.gd")`, `func apply_quality() -> void`, one `apply_quality()` call in `_ready` (after `clean_feed.setup(...)`, `godot/scripts/app.gd:85`), and a `"quality"` arm in `_on_setting_changed`; in `clean_feed.gd` — `func set_msaa(value: int) -> void`.

- [ ] **Step 1: Write the failing tests:**

In `godot/tests/test_scene.gd`:

```gdscript
func test_quality_tiers_turn_shadows_and_msaa_up_and_down() -> void:
	eq(World.quality_spec("low")["msaa"], Viewport.MSAA_DISABLED, "low: no MSAA")
	eq(World.quality_spec("medium")["msaa"], Viewport.MSAA_4X, "medium: today's MSAA (main.tscn's msaa_3d = 2)")
	eq(World.quality_spec("high")["msaa"], Viewport.MSAA_8X, "high: 8x")
	eq(World.quality_spec("ultra")["shadow_m"], World.QUALITY["medium"]["shadow_m"], "an unknown tier is medium")
	var world := World.new()
	await add_to_tree(world)
	var sun: DirectionalLight3D = world.get_node("Sun")
	world.set_quality("low")
	ok(not sun.shadow_enabled, "low: shadows off")
	world.set_quality("high")
	ok(sun.shadow_enabled, "high: shadows on")
	near(sun.directional_shadow_max_distance, 400.0, "reaching 400 m")
	world.set_quality("medium")
	ok(sun.shadow_enabled, "medium: shadows on")
	near(sun.directional_shadow_max_distance, 150.0, "at today's 150 m")
	world.queue_free()
```

In `godot/tests/test_settings.gd`:

```gdscript
func test_the_quality_setting_defaults_medium_and_comes_from_every_source() -> void:
	eq(AppSettings.defaults()["quality"], "medium", "medium by default")
	eq(AppSettings.resolve({"quality": "low"}, {}, PackedStringArray())["quality"], "low", "the config file")
	eq(AppSettings.resolve({}, {"OFS_QUALITY": "high"}, PackedStringArray())["quality"], "high", "OFS_QUALITY")
	eq(AppSettings.resolve({}, {}, PackedStringArray(["--quality=low"]))["quality"], "low", "--quality")
	eq(AppSettings.resolve({}, {}, PackedStringArray(["--quality=ultra"]))["quality"], "medium", "an unknown value falls back to medium")
	eq(AppSettings.overrides({}, PackedStringArray(["--quality=low"])), {"quality": "--quality"}, "the flag is reported as an override")
```

In `godot/tests/test_settings_screen.gd`:

```gdscript
func test_the_graphics_quality_row_is_live_and_emits_its_steps() -> void:
	var s := await _screen()
	var screen = s[0]
	eq(screen.control("quality").value(), "medium", "medium by default")
	screen.control("quality").step(1)
	eq(s[2], [["quality", "high"]], "one step up is high")
	eq(screen.note("quality"), "", "the row is live")
	screen.queue_free()
```

In `godot/tests/e2e_open_loop.gd`, after the pilot/emitter checks in the world-file section (after the `Handset` check, `godot/tests/e2e_open_loop.gd:211`):

```gdscript
	# The dressed world and the quality setting (the environment is scenery like the rest).
	var dressed: Environment = (app.world.get_node("Sky") as WorldEnvironment).environment
	_check(dressed.fog_enabled, "the field wears distance fog")
	app.settings["quality"] = "low"
	app.apply_quality()
	_check(app.fpv_viewport.msaa_3d == Viewport.MSAA_DISABLED and not app.world.get_node("Sun").shadow_enabled, "low applies at once: no MSAA, no shadows")
	app.settings["quality"] = "medium"
	app.apply_quality()
	_check(app.world.get_node("Sun").shadow_enabled, "medium restores today's shadows")
```

- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: FAIL on the new assertions (`QUALITY` unknown; `quality` not a setting; no row; no `apply_quality`).
- [ ] **Step 3: Implement** the five files as in Interfaces. `world.gd`:

```gdscript
## The quality tiers: the sun's shadow distance in metres (0 disables shadows) and the viewport's MSAA — the enum
## counts 0 disabled, 1 2x, 2 4x, 3 8x; medium is today's `msaa_3d = 2` (main.tscn). `app.gd` applies the MSAA half
## to its viewports; `set_quality` applies the shadow half to the sun.
const QUALITY := {
	"low": {"shadow_m": 0.0, "msaa": Viewport.MSAA_DISABLED},
	"medium": {"shadow_m": 150.0, "msaa": Viewport.MSAA_4X},
	"high": {"shadow_m": 400.0, "msaa": Viewport.MSAA_8X},
}


## The tier's spec; any name the table does not carry is medium.
static func quality_spec(tier: String) -> Dictionary:
	return QUALITY.get(tier, QUALITY["medium"])


## Applies the tier's shadow half (the MSAA half belongs to the viewports; `app.gd` applies it).
func set_quality(tier: String) -> void:
	var spec := quality_spec(tier)
	_sun.shadow_enabled = spec["shadow_m"] > 0.0
	_sun.directional_shadow_max_distance = maxf(spec["shadow_m"], 150.0)
```

`app.gd` (the one application path; `_on_setting_changed`'s match gains `"quality": apply_quality()`):

```gdscript
## The one path for the Graphics quality setting (boot, and a change in the settings screen): the tier's MSAA on the
## FPV viewport and the DVR's clean copy of it, and its shadows on the world's sun. Applies at once.
func apply_quality() -> void:
	var tier: String = settings.get("quality", "medium")
	var spec: Dictionary = WorldScript.quality_spec(tier)
	fpv_viewport.msaa_3d = spec["msaa"]
	clean_feed.set_msaa(spec["msaa"])
	world.set_quality(tier)
```

`clean_feed.gd`:

```gdscript
## Follows the FPV viewport's anti-aliasing when the quality setting changes (setup copies it only once).
func set_msaa(value: int) -> void:
	_viewport.msaa_3d = value
```

- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh all` (WSL; the Betaflight e2e only when `OFS_SITL_LAUNCH` is set) — Expected: PASS, whole output read; the e2e's new checks pass on the menu-less open-loop boot.
- [ ] **Step 5: Commit** `feat(godot): a graphics quality setting over shadows and MSAA (#134)` — `git add godot/world/world.gd godot/scripts/settings.gd godot/ui/settings_screen.gd godot/scripts/app.gd godot/ui/clean_feed.gd godot/tests/test_scene.gd godot/tests/test_settings.gd godot/tests/test_settings_screen.gd godot/tests/e2e_open_loop.gd`.

### Task 4: Docs, the CHANGELOG, and the by-hand list for #163

**Files:**
- Modify: `CHANGELOG.md` (an entry at the top of Unreleased), `README.md` (the settings table around lines 137-146 and one sentence in "The Godot pilot client"), `docs/architecture.md` (the Godot client paragraph around line 89), `docs/dev-setup.md` (the Godot section's tests note, around line 42: the eyeball pass)

**Interfaces:** Consumes Tasks 1-3; produces nothing for code.

- [ ] **Step 1: Write the edits.** CHANGELOG, under a `### Better world rendering (#134)` heading with `#### Added`/`#### Changed`: the look table (per-world sky, sun, fog, ground) dressed in `world.gd` from the world's name, unknown worlds keep today's look; the ground variation layer in the grid shader; the `quality` setting (`OFS_QUALITY`, `--quality`, default medium, live; low below today's cost); "no protocol change (still 15); the analog video chain is untouched". README: the row `| graphics quality (low/medium/high, default medium) | OFS_QUALITY | --quality= |` after the sound row, and one sentence — the worlds are dressed per world (sky, sun, fog, varied ground) under the same analog degradation, with a quality setting whose low end is cheaper than before. `docs/architecture.md`: extend the Godot client bullet with the same one sentence, naming `world.gd`'s look table and the client-local setting. `docs/dev-setup.md`: after the tests sentence, one line — `godot --path godot -s res://tests/shots.gd -- --out=<dir>` renders the dressed worlds for an eyeball pass (headless tests cannot).
- [ ] **Step 2: Check** every claim against the code: `grep -rn "OFS_QUALITY\|quality" README.md docs/architecture.md docs/dev-setup.md godot/scripts/settings.gd` shows the setting in all four; LF check on every file touched in this plan.
- [ ] **Step 3: Run the whole gate** `bash scripts/run-godot-tests.sh all` (WSL) — Expected: PASS, whole output read. (No Rust changed; the script's own `cargo build -p ofs-sim -p ofs-godot` must still succeed.)
- [ ] **Step 4: Write the by-hand list** into the PR description (and, when the PR closes, as a comment on #163, the collector): fly each of the five worlds at medium and see a distinct mood in FPV and chase; fly meadow behind the barn until the link degrades and confirm the world reads under the static (better-looking, not cleaner); the metre grid still reads at 30+ m on every look; low/medium/high visibly differ (shadows, MSAA) and low holds today's frame rate on the dev machine; the golden-hour hills sky over the still-flat ground (terrain is #155).
- [ ] **Step 5: Commit** `docs(godot): the dressed worlds, the quality setting and the changelog (#134)` — `git add CHANGELOG.md README.md docs/architecture.md docs/dev-setup.md`.

## Ledger

Nothing yet — the executor appends any deviation from a design call here, with the ruling and the reason.
