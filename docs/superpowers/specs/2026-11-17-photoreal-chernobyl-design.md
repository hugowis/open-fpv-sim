# Photoreal Chernobyl: map creation and graphics (hero area first)

Written 2026-11-17 against `main` at `1030be7`. Status: draft for owner approval.

## Intent

The map's content is right — Stage B ships the New Safe Confinement, units 3 and 4, the shelter object, the
stabilisation structure, the turbine hall and its breach, from OpenStreetMap — but it renders as flat-coloured
extrusions. The original goal was realistic environments. This design makes one **hero area** of the Chernobyl site
photoreal/survey-like, and builds the map-creation and graphics pipeline that the rest of the map (and later worlds)
reuse. The simulator's physics, radio and video models, protocol semantics and the analog video chain stay as they are.

## What "photoreal/survey-like" means here

- PBR materials with textures, normal/roughness maps, not flat colours.
- Detailed hero geometry: the NSC's ribs, panels and door; units 3 and 4; the turbine hall; the ground and roads.
- Real lighting: Godot **Forward+** (Vulkan), sun and sky, shadows, SSAO, SSR, volumetric fog, a fixed look.
- Ground detail, roads, vegetation, fences, debris and props.
- 60 fps at 1080p on the development machine (RTX 2070), with a lower quality tier for weaker GPUs.
- **Without site-specific photos, drone scans or purchased models** (the owner's constraint), the geometry is
  dimensionally accurate and the materials are plausible CC0. This is a photoreal-quality reconstruction, not a
  photographic survey. The owner's eye is the final judge.

## Goals

- A photoreal hero area: the NSC, units 3/4, the turbine hall and the immediate ground/roads, with detailed geometry,
  PBR materials, lighting, shadows, fog and props.
- A reusable map-creation pipeline: OSM footprints → detailed buildings, ground, roads and vegetation; hero assets
  authored in Blender; CC0 textures; LODs and texture compression.
- The physics and RF unchanged: the server keeps the simplified collision/RF mesh; the visual detail is client-only.
- The analog video chain unchanged: the goggles still show noise, breakup and the OSD over the better picture.
- The game moves to Forward+; the headless tests, the DVR, the clean feed and the screenshot checks still pass.
- Clear licences and provenance for every asset; the repository stays redistributable under GPLv3 plus ODbL for the
  OSM snapshot and CC0 for the textures.

## Non-goals

- Rebuilding every world or the whole Chernobyl map in the first milestone. The hero area is the proof; the rest of
  the map uses the same pipeline with less manual art.
- Site-specific photogrammetry, drone scans, satellite imagery or purchased models. No extra data.
- Changing flight physics, the RF model, the collision behaviour or the protocol's meaning.
- Changing the analog video chain, the OSD, the DVR or the goggle look.
- A weather system or selectable time of day. One fixed look, eyeballed by the owner.
- Photoreal for every world; the hero area first.

## Anti-goals

- A visual layer that changes collision/RF or the physics digests.
- Making the goggles picture cleaner: no bloom, no sharpening, no auto-exposure.
- An asset pipeline that needs the Godot editor import in headless CI; the runtime GLB loading stays.
- Unlicensed assets, or assets whose licence cannot travel with the repository.
- A renderer switch that breaks the headless tests, the DVR or the clean feed.

## Constraints

- Godot 4.7.2; Forward+ (Vulkan) for the game; the headless suites use the dummy renderer.
- RTX 2070 development machine; 60 fps at 1080p; a lower quality tier for weaker GPUs.
- No site-specific data; CC0 textures; Blender-authored hero models from published dimensions and the OSM outlines.
- OSM data is ODbL 1.0; the snapshot stays ODbL; the generated world keeps the attribution.
- The repository is GPLv3; CC0 is compatible; the Blender sources are GPLv3.
- The current world file and protocol; the `col_` node convention; the server's triangle cap; the client's runtime
  `.glb` loading.
- The headless Godot tests and the e2e suites; the screenshot script; the DVR; the clean feed.
- Repository size and load time; the hero GLB and textures need explicit budgets.

## Approaches

**A. Hero area first, photoreal end-to-end (chosen).** Switch to Forward+, build one hero area properly (NSC, units
3/4, turbine hall, ground/roads), author the hero models and the asset pipeline, then expand. Gives a visible,
judgeable result and a reusable pipeline. The rest of the map stays blocky until expanded.

**B. Pipeline first, all worlds.** Build the reusable OSM → detailed geometry/terrain/roads/vegetation/PBR pipeline
first, then regenerate every world. Consistent, but much longer before anything looks photoreal; the hero landmarks
still need manual art; high risk of over-engineering before a result.

**C. Renderer/material pass only.** Keep Compatibility, add textures/PBR/lighting to the existing blocky geometry.
Fastest and broadest hardware support, but still blocky and without SSAO/SSR/volumetric fog; it cannot reach
survey-like. Rejected for this target.

The owner chose **A** and **no extra data**.

## The design

### Renderer

The game moves to Godot **Forward+** (`rendering_method = "forward_plus"`). This is what makes SSAO, SSR, volumetric
fog, better shadows and the PBR pipeline available. The RTX 2070 is the target; the README and Settings note the
Vulkan requirement. The headless Godot suites use the dummy renderer and are unaffected. The analog video chain
(`video.gd`/`video.gdshader`, `lens.gd`/`lens.gdshader`, `osd.gd`, `dvr.gd`) is canvas-layer work and stays; the DVR
and clean feed render to textures and must be re-verified on Forward+.

The existing #134 world-rendering plan (sky/sun/fog/ground/quality) is **not merged**; it is the look/quality
foundation this work needs. It lands first as PR 0, then the photoreal pipeline builds on its `LOOKS`/quality table.

A short **renderer spike** precedes the implementation: boot the current game on Forward+, fly the current
Chernobyl world, record a DVR clip, open the clean feed, run the screenshot script, and measure the frame time. The
spike is throwaway; its findings settle the exact Godot settings (MSAA, shadow atlas, fog mode) and the quality
tiers.

### Visual versus collision/RF

The server's physics and RF stay on the simplified mesh. The client draws a detailed visual layer from a **separate
client-only visual file**. This is the builder-check correction: the server caps a mesh at 64 MiB and 400k triangles
(`MAX_MESH_FILE_BYTES`, `MAX_MESH_TRIANGLES`) and parses it synchronously, while a photoreal layer is larger.

- The world file gains `[[visual_meshes]]` (name, file, position, heading, digest). `GetWorld` sends them; the client
  loads them asynchronously; the server never reads them for collision/RF. The repro bundle copies them.
- The existing `[[meshes]]` stay the collision/RF + base visual. The current `col_` rule stays: a file with any
  `col_` node uses only the `col_` nodes for collision/RF. The hero collision file is `col_`-only; the client drops
  those nodes and draws the visual file instead.
- No new node prefix is needed. The visual file is client-only; the collision file is server-only. A world without a
  visual file keeps today's both-purpose mesh.
- The visual file has a client-side budget (triangles, memory, load time) and an async loading path; the server's
  collision caps are unchanged.
- The physics/RF regression tests compare behaviour, not the visual file's digest.

### Asset pipeline

- **Two files**: the collision/RF `chernobyl.glb` stays sitegen-generated and byte-identical-tested; the hero visual
  file is a committed authored+generated asset with a manifest and a load/digest test, not a byte-identical sitegen
  rebuild. The Blender sources are committed; a Blender version change may change the export bytes, so the committed
  file is the source of truth.
- **Hero models**: Blender-authored, committed as `.blend` sources under `art/chernobyl/` (GPLv3) and exported to the
  visual `.glb`. The NSC (ribs, panels, door), units 3/4 and the turbine hall are modeled from the published
  dimensions and the OSM outlines; the details are plausible industrial reconstruction, not a photo match.
- **Textures**: CC0 sources (Poly Haven, ambientCG) with a `LICENSES.md` recording the source, licence and any
  changes. Textures are embedded in the visual `.glb` (runtime loading; no editor import) or loaded beside it; the
  plan settles the format (PNG/KTX2) and compression.
- **Procedural detail**: a Python/Blender step turns OSM footprints into visual facades and roofs (window bands,
  parapets, roof plant, industrial details) with UVs and PBR materials. The simplified `col_` extrusion stays.
- **Ground, roads, vegetation, props**: a flat ground with a photoreal material and decals; the OSM snapshot is
  **extended to roads/paths/landuse** (ODbL) and the detail step draws them as spline meshes/decals; procedural
  scattering of grass, trees, fences and debris with CC0 assets and LODs. The site is a restricted zone, so the
  density is sparse and plausible.
- **LODs and compression**: each hero asset gets 2–3 LODs; the client uses distance/impostor LODs; textures are
  compressed; draw calls are batched where possible.

### Map creation

The map is still a recipe plus generated outputs, but the pipeline gains a **detail stage**:

1. `sitegen` keeps producing the simplified collision/RF `chernobyl.glb` (the current output; byte-identical test).
2. A new detail step (Python and/or Blender) produces the client-only visual `.glb`:
   - hero models for the NSC, units 3/4 and the turbine hall;
   - procedural facades/roofs for the other OSM buildings;
   - ground/road/vegetation/prop meshes, with roads/paths/landuse from the extended OSM snapshot;
   - the fixed lighting/look is Godot-side, not baked into the mesh.
3. The world file names the visual file in `[[visual_meshes]]`; the client loads it; the server ignores it.
4. The hero area is the NSC, units 3/4, the turbine hall and the immediate ground/roads; the rest of the map uses
   the procedural visual layer with the simplified collision base.

The hero area is the NSC, units 3/4, the turbine hall and the immediate ground/roads (about the current 600 × 600 m
extent). The rest of the OSM buildings get the procedural detail; they are not hand-modeled in this milestone.

### Lighting and look

One fixed look for the Chernobyl map, eyeballed by the owner: a clear day with a low sun for readable shadows and
the NSC's form, a neutral sky, distance/height fog for depth, SSAO/SSR on the hero area, and a restrained
post-processing pass. No bloom, no sharpening, no auto-exposure: the analog video chain must stay in charge of the
picture's character. The #134 look table (PR 0) is the starting point; the Chernobyl entry gets its own values.

### Performance

Target 60 fps at 1080p on the RTX 2070, with the simulator running. Budgets (to be refined by the spike):

- hero visual triangles: a few million at LOD0, far fewer at distance;
- texture memory: 2–4 GB;
- draw calls: batched by material/asset;
- LODs: 2–3 per hero asset, impostors for distant buildings/vegetation;
- occlusion culling and distance culling for the OSM buildings.

The visual file is loaded asynchronously with a loading state; its budget is separate from the server's collision
caps (target: < 100 MB, a few million LOD0 triangles, 2–4 GB texture memory).

A **low** quality tier drops shadows, SSAO/SSR, fog height and texture size; a **high** tier keeps the full look.
The setting rides the existing settings chain.

### Licensing and provenance

- OSM snapshot (buildings and the extended roads/paths/landuse): ODbL 1.0; the world header and the game/README credit stay.
- CC0 textures/assets: `LICENSES.md` with source, licence, changes; no attribution required but recorded.
- Blender sources and generated meshes: GPLv3.
- The repository stays redistributable; no site-specific photos or purchased models.

### Testing

- The physics/RF regression tests pass unchanged (the `col_` layer is the same simplified geometry).
- The server never reads the visual file; the client loads it; the collision file's `col_` nodes are not drawn.
- The headless Godot unit/e2e suites pass; the screenshot script produces the visual set on Forward+.
- The DVR and clean feed work on Forward+; the analog chain is unchanged.
- Frame-time benchmark on the RTX 2070; the low tier holds the frame rate.
- The owner's eye: the hero area reads as a photoreal Chernobyl reconstruction; the credit is visible.

## What is left to the builder

- The exact hero-area boundary and the transition to the procedural buildings.
- The exact OSM fetch tags/format for roads/paths/landuse (open ways, not rings) and how the detail step meshes them.
- The Blender modeling detail, texture choices and UV layout for the hero assets.
- The procedural facade/roof rules and the road/vegetation density.
- The Godot Forward+ settings, the look values, the LOD thresholds and the quality tiers.
- The texture format/compression and the final budgets.
- The exact split of the work into PRs.

## Delivery

Four PRs, each independently reviewable:

0. **#134 look/quality foundation** (the existing plan): sky/sun/fog/ground/quality, no photoreal assets.
1. **Renderer and visual layer**: Forward+ spike, `[[visual_meshes]]` world/protocol, client async loading, a small
   test visual asset, the physics/RF regression tests.
2. **Asset pipeline and hero models**: Blender sources, CC0 textures and licences, the NSC/units/turbine-hall
   models, the procedural facade/roof step, the ground/roads/vegetation.
3. **Hero-area integration and look**: the detail layer in the map, lighting/fog/post, LODs/performance, docs,
   screenshots and the owner's look.

The owner's look is the gate for PR 3; the map is not called photoreal until they have flown it.

## Builder-check decisions

The reviewer raised six questions; these are the calls (the owner can override):

1. `col_` rule: stays as today; the collision file is `col_`-only; the visual layer is a separate client-only file.
2. One file vs separate: separate client-only visual file (`[[visual_meshes]]`), because of the 64 MiB/400k server caps.
3. Generated vs authored: the collision file stays sitegen-generated and byte-identical-tested; the visual file is a
   committed authored+generated asset with a manifest/load test; the Blender sources are committed.
4. #134: not merged; it lands first as PR 0 (look/quality foundation).
5. Roads/landuse: **extend the OSM fetch** (owner's call, 2026-11-17): the snapshot gains roads/paths/landuse under
   the same ODbL; the detail step uses them for the ground/roads. The collision build ignores them.
6. Unit 4 ruins / shelter: the hero visual includes the unit 4 ruins and hand-models the shelter/stabilisation; the
   collision base stays the simplified OSM/shelter shapes.
