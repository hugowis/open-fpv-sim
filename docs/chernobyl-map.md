# The Chernobyl map

`worlds/chernobyl.toml` is a world of the Chernobyl reactor area as it is today: the New Safe Confinement (NSC) over
reactor 4, units 3 and 4, the turbine hall and the surrounding buildings from OpenStreetMap. The facts and their sources
are in `sites/chernobyl/SOURCES.md`; the numbers themselves are in `sites/chernobyl/recipe.toml`.

## What is in the map now (Stage B)

- **The New Safe Confinement (NSC)**: the steel arch over reactor 4, 257 m wide, 162 m long and 108 m high, its axis
  running east-west, as one thin shell of 288 triangles. Solid to the quad and in the way of the radio and video link.
  Its centre is refined to the bounding-box centre of the OpenStreetMap NSC outline, `[-53.946, -27.065]`.
- **One door**, 40 x 40 m, in the **west** end, with a dark frame so it reads from the spawn. The east end has none: it
  nearly touches unit 3, and the real building has a closed wall at both ends.
- **Units 3 and 4, the shelter object, the stabilisation structure, the turbine hall and the other OSM buildings**: real
  OpenStreetMap footprints (ODbL 1.0), extruded to their OSM heights (unit 3 70 m, shelter 65 m, stabilisation 63 m,
  turbine hall 50 m; the rest as tagged or the default). The NSC outline (way 456732992) and the unit 4 ruins
  (500551950) are skipped: the arch replaces the first, and the second overlaps the shelter object.
- **The turbine hall**: an OSM multipolygon relation (8705336), about 600 m long, with a 30 x 20 m breach in the west
  end wall (bottom 5 m), after the 2013 collapse of 600 m2 of its roof and wall. The breach is a gameplay liberty.
- **The pilot** stands 150 m west of the door (`[-53.946, -258.065, -1.7]`, eye height, facing east); the quad starts
  100 m west of the door (`[-53.946, -208.065, -2]`), outside the arch.
- Flat ground, one omni receiver (the same as `worlds/hall.toml`), no gates.
- The real OSM snapshot is `sites/chernobyl/chernobyl.osm.json`, fetched once on 2026-11-17 (40 buildings) under the
  ODbL; the licence note is confirmed (`docs/research/sitegen-licensing.md`).

## Measured and artistic

**Measured** is a published number with its source; **estimate** is an engineering guess; **artistic** is a gameplay or look
decision that is not a fact about the site; **OSM** is a value from the OpenStreetMap snapshot (ODbL 1.0).

| Item | Value | Kind | Source or reason |
|---|---|---|---|
| Arch span | 257 m | measured | <https://chnpp.gov.ua/en/infocenter/news/204-projects/nsc-construction/578-q-q578> "Span of Arch: 257m" (Structurae agrees) |
| Arch length | 162 m | measured | the same page, "Length: 162m" |
| Arch height | 108 m | measured | the same page, "Height: 108m" |
| Axis east-west (`heading_deg = 90`) | built 180 m west of reactor 4 and slid over it | measured, direction inferred | <https://en.wikipedia.org/wiki/Chernobyl_New_Safe_Confinement>; Wikidata Q1506085 moved 0.005 degrees of longitude east after the slide |
| World origin | 51.38972 N, 30.09917 E | estimate | the "Chernobyl disaster" point on Wikipedia; no source gives reactor 4 itself, so it is within tens of metres of it. It places the OSM outlines and the arch |
| Arch centre | `[-53.946, -27.065]` | OSM, estimate | the bounding-box centre of the OSM NSC outline (way 456732992); the outline is skipped and the arch stands in its place |
| Unit 3 | 70 m | OSM | way 326145079, `height=70` |
| Shelter object | 65 m | OSM | way 500551951, `height=65` |
| Stabilisation steel structure | 63 m | OSM | way 500551953, `height=63` |
| Turbine hall | 50 m, about 600 m long | OSM | relation 8705336, `height=50` |
| Turbine-hall breach | 30 x 20 m, bottom 5 m, west end wall | artistic | the 2013 collapse of 600 m2 of roof and wall; the wall index comes from `python -m sitegen walls` |
| Radio loss of the cladding | 40 dB per skin crossed | estimate | a steel-clad shell at 5.8 GHz; not measured |
| Radio loss of the buildings | 30 dB per wall crossed | estimate | concrete walls at 5.8 GHz; not measured |
| Receiver | -93 dBm floor, omni 2 dBi, rhcp | estimate | the same as `worlds/hall.toml` |
| Door | 40 x 40 m, west end only | artistic | the real end walls are closed; the door is there so a quad can fly in |
| Pilot spot | 150 m west of the door, facing east | artistic | a place to start from |
| Quad spawn | 100 m west of the door | artistic | a clear start outside the arch |
| Vault facets | 48 | artistic | a smooth enough arch |
| Cladding colour | steel grey | artistic | dark enough to read the vault's facets in the sun |
| Building colour | concrete grey | artistic | the OSM buildings |
| Kept area | 300 m half extent each way | artistic | covers the arch and its surroundings |

Not modelled: the internal height of the vault is 92.5 m (Wikipedia) against the 108 m outer height; the generator builds
one shell at the outer size, so the inside is a little taller than reality. The OSM buildings are flat-roofed extrusions
with no interiors. The real arch is a double-skin steel structure; this map is one thin shell with one open door. Other
published sizes exist (Wikipedia's infobox says 260 m span and 165 m length); the operator's page and Structurae agree on
257 / 162 / 108, which the map follows.

## Why there is no chimney

The ventilation stack VP-2, 75.5 m tall on the roof of unit 4 with its top 150 m above ground, was dismantled between 31
October and 25 November 2013 so that the NSC could be slid over reactor 4
(<https://world-nuclear-news.org/Articles/Chernobyl-ventilation-stack-removed>). The arch and the stack never stood
together. The map is the present-day site, so no chimney is built, and a test (`test_no_chimney_is_built`) caps the height at
108.5 m, the arch's 108 m plus a margin. As of 2026-11-01 OpenStreetMap still maps a 125 m chimney where VP-2 stood (way
301362167); that is stale and is not built either.

## Flying it

The easiest way is the game's main menu: it lists every `worlds/*.toml` of the checkout by absolute path
(`godot/scripts/catalog.gd`), so "Chernobyl" is in the World row and works from any directory. Other ways to name it:
`--world=chernobyl` or `OFS_WORLD=chernobyl`, a bare name, which the server resolves to `worlds/chernobyl.toml` beside the
quad file's directory (`ofs_config::world::resolve`, called at `crates/ofs-sim/src/server.rs:298`); that works because the
game's default quad path is absolute. A relative path (`--world=worlds/chernobyl.toml`,
`OFS_WORLD=worlds/chernobyl.toml`, or `world_path` in the settings) is resolved against the repository root before the
client sends it (`godot/scripts/settings.gd`), so it works from any directory; a bare name is left for the server to
resolve beside the quad file. See also `docs/dev-setup.md`, "The world file and the analog video link". From Python, a
relative path is resolved against the Python process's working directory, so run from the repository root (the quad path
below is relative; or use absolute paths):

```python
sim.load("quads/opendrone-7lr-longrange.toml", world="chernobyl")   # a bare name: worlds/chernobyl.toml beside the quad's directory
sim.load("quads/opendrone-7lr-longrange.toml", world="worlds/chernobyl.toml")   # a path: from the repository root, or absolute
```

The pilot stands 150 m west of the door; the quad starts 100 m west of it, outside the arch. Fly east and in: a 7-inch
quad (about 0.4 m tip to tip) passes the 40 m door without touching anything. The door is at north -53.946, east -108.065;
the shelter object and the stabilisation structure stand inside the arch, so a straight run from the door meets them about
26 m in. Come in 30 m off the door's axis and you hit the end wall (`HIT chernobyl`). The turbine hall is east of the arch;
its west end wall has the 30 x 20 m breach. The checks are in `crates/ofs-sim/tests/chernobyl_world.rs`; the cost of the
extra mesh is recorded in `docs/research/collision.md`.

## The radio

A path loses 40 dB for every skin of the arch it crosses and 30 dB for every wall of an OSM building (estimates; no
diffraction, no reflection, no thickness: `docs/dev-setup.md`). The video link is therefore dead behind the arch and inside
it, except where the line to the pilot passes through the door: where it passes through the 40 x 40 m opening the picture
holds; where it crosses the closed end wall the skin takes 40 dB. The tests check it at two points each: in Rust
(`the_video_link_is_cut_by_the_arch_skin_and_not_through_the_door`) the quad 8 m inside the door gets 0 dB and 40 m off
the door's axis 40 dB or more; in Python (`test_the_pilot_sees_the_door`) the line of sight from the pilot passes through
the door and is blocked beside it.

## Regenerating it

The `.glb` and the world file are generated; edit the recipe, not them. The snapshot is fetched once (network) and then
committed under the ODbL:

    cd tools/sitegen
    python -m sitegen fetch ../../sites/chernobyl/recipe.toml
    python -m sitegen build ../../sites/chernobyl/recipe.toml

The output is byte-identical to what is committed, and `tools/sitegen/tests/test_chernobyl_site.py`
(`test_committed_outputs_are_current`) fails if it is not. Generator documentation: `tools/sitegen/README.md`.

## What is still not modelled

- The ventilation stack VP-2 (gone since 2013) and the replacement stack (position not sourced).
- Interiors beyond the OSM building volumes; the real arch is double-skin, and its internal height is 92.5 m.
- Roof breaches: the generator's `openings` are wall openings only; the 2013 collapse is represented by the wall breach.
- DEM ground: the site is flat.
- The real turbine hall's internal structure and the exact shape of the 2013 collapse.

## Attribution

The OpenStreetMap snapshot (`sites/chernobyl/chernobyl.osm.json`) is a Derivative Database under the ODbL 1.0; the
generated mesh and world file are a Produced Work and carry the attribution in the world header. The game credits
OpenStreetMap in the help screen, and the top-level README names the licence. The map is an approximation made for a
simulator, not a survey.
