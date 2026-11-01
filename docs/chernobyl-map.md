# The Chernobyl map

`worlds/chernobyl.toml` is a world of the Chernobyl reactor area as it is today. It is built in stages; this page says what
is in it now (Stage A), which numbers are published and which are not, and what is still to come. The facts and their sources
are in `sites/chernobyl/SOURCES.md`; the numbers themselves are in `sites/chernobyl/recipe.toml`.

## What is in the map now (Stage A)

- **The New Safe Confinement (NSC)**: the steel arch over reactor 4, 257 m wide, 162 m long and 108 m high, its axis running
  east-west, as one thin shell of 288 triangles (`worlds/assets/chernobyl.glb`, 6.6 kB). Solid to the quad and in the way of
  the radio and video link.
- **One door**, 40 x 40 m, in the **west** end. The east end has none: it nearly touches unit 3, and the real building has a
  closed wall at both ends.
- **The pilot** stands 150 m west of the door (`[0, -231, -1.7]`, eye height, facing east).
- Flat ground, one omni receiver (the same as `worlds/hall.toml`), no gates, no other objects.

There is no OpenStreetMap data in it yet: `sites/chernobyl/chernobyl.osm.json` is a hand-made, empty snapshot, and the
arch is the only geometry. The surrounding buildings come in Stage B (below).

## Measured and artistic

**Measured** is a published number with its source; **estimate** is an engineering guess; **artistic** is a gameplay or look
decision that is not a fact about the site.

| Item | Value | Kind | Source or reason |
|---|---|---|---|
| Arch span | 257 m | measured | <https://chnpp.gov.ua/en/infocenter/news/204-projects/nsc-construction/578-q-q578> "Span of Arch: 257m" (Structurae agrees) |
| Arch length | 162 m | measured | the same page, "Length: 162m" |
| Arch height | 108 m | measured | the same page, "Height: 108m" |
| Axis east-west (`heading_deg = 90`) | built 180 m west of reactor 4 and slid over it | measured, direction inferred | <https://en.wikipedia.org/wiki/Chernobyl_New_Safe_Confinement>; Wikidata Q1506085 moved 0.005 degrees of longitude east after the slide |
| World origin | 51.38972 N, 30.09917 E | estimate | the "Chernobyl disaster" point on Wikipedia; no source gives reactor 4 itself, so it is within tens of metres of it |
| Arch centre at the origin | `[0, 0]` | estimate | refined against the OpenStreetMap outline in Stage B |
| Radio loss of the cladding | 40 dB per skin crossed | estimate | a steel-clad shell at 5.8 GHz; not measured |
| Receiver | -93 dBm floor, omni 2 dBi, rhcp | estimate | the same as `worlds/hall.toml` |
| Door | 40 x 40 m, west end only | artistic | the real end walls are closed; the door is there so a quad can fly in |
| Pilot spot | 150 m west of the door, facing east | artistic | a place to start from |
| Vault facets | 48 | artistic | a smooth enough arch |
| Cladding colour | light steel grey | artistic | |
| Kept area | 300 m half extent each way | artistic | covers the arch and its surroundings |

Not modelled: the internal height of the vault is 92.5 m (Wikipedia) against the 108 m outer height; the generator builds
one shell at the outer size, so the inside is a little taller than reality. Other published sizes exist (Wikipedia's infobox
says 260 m span and 165 m length); the operator's page and Structurae agree on 257 / 162 / 108, which the map follows.

## Why there is no chimney

The ventilation stack VP-2, 75.5 m tall on the roof of unit 4 with its top 150 m above ground, was dismantled between 31
October and 25 November 2013 so that the NSC could be slid over reactor 4
(<https://world-nuclear-news.org/Articles/Chernobyl-ventilation-stack-removed>). The arch and the stack never stood
together. The map is the present-day site, so no chimney is built, and a test (`test_no_chimney_is_built`) caps the height at
the arch's 108 m. OpenStreetMap still maps a 125 m chimney where VP-2 stood; that is stale and is not built either.

## Flying it

The game: set `world_path` in the settings to `worlds/chernobyl.toml`, or start it with `OFS_WORLD=worlds/chernobyl.toml`
or `--world=worlds/chernobyl.toml` (`docs/dev-setup.md`, "The world file and the analog video link"). From Python:

```python
sim.load("quads/opendrone-7lr-longrange.toml", world="worlds/chernobyl.toml")   # or world="chernobyl", by name
```

You start 150 m west of the door. Fly east and in: a 7-inch quad (about 0.4 m tip to tip) passes the 40 m door without
touching anything. Come in 30 m off the door's axis and you hit the end wall (`HIT chernobyl`). The checks are in
`crates/ofs-sim/tests/chernobyl_world.rs`; the cost of the extra mesh is recorded in `docs/research/collision.md`
(about 7 ms more per simulated second, 0.127 s against 0.053 s for 10 s of flight).

## The radio

A path loses 40 dB for every skin of the arch it crosses (an estimate; no diffraction, no reflection, no thickness:
`docs/dev-setup.md`). The video link is therefore dead behind the arch and inside it, except where the line to the pilot
passes through the door: near the door's axis the picture holds, off to the side the line crosses the end wall. The test
`the_video_link_is_cut_by_the_arch_skin_and_not_through_the_door` shows 40 dB or more behind the end wall and none straight
through the door.

## Regenerating it

The `.glb` and the world file are generated; edit the recipe, not them:

    cd tools/sitegen
    python -m sitegen build ../../sites/chernobyl/recipe.toml

The output is byte-identical to what is committed, and `tools/sitegen/tests/test_chernobyl_site.py`
(`test_committed_outputs_are_current`) fails if it is not. Generator documentation: `tools/sitegen/README.md`.

## What Stage B adds

- the real OpenStreetMap snapshot of the area (`python -m sitegen fetch`) in place of the empty one;
- units 3 and 4 and the other buildings around the arch, with the arch's centre refined against the outline;
- the turbine hall (an OpenStreetMap multipolygon relation, about 600 m long) with a breach in its wall, after the 2013
  collapse of 600 m2 of its roof and wall (<https://world-nuclear-news.org/articles/safety-work-planned-for-chernobyl-turbine-hall>).

Stage B waits for the repository owner to confirm `docs/research/sitegen-licensing.md` (status: unconfirmed): whether an ODbL
snapshot may live in this GPLv3 repository. Nothing from OpenStreetMap is committed until then.

## Attribution

The world header carries "© OpenStreetMap contributors (ODbL 1.0)" even though Stage A uses no OpenStreetMap data: the
generator writes it for every world, and over-crediting is harmless. The map is an approximation made for a simulator, not a survey.
