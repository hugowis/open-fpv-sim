# Chernobyl map: sources

Stage B of the Chernobyl map: the New Safe Confinement (NSC) arch from published dimensions, plus the surrounding
buildings and the turbine hall from the OpenStreetMap snapshot (`chernobyl.osm.json`, ODbL 1.0, fetched 2026-11-17).
The licence note is confirmed in `docs/research/sitegen-licensing.md`.

Each number in `recipe.toml` is one of: **published** (a source and the figure as it reads there), **estimate** (a guess
with a stated reason), **artistic** (a gameplay or look decision, not a fact about the site), or **OSM** (a value from the
OpenStreetMap snapshot, ODbL 1.0).

| Value | What it is | Source | Kind |
|---|---|---|---|
| `origin = [51.38972, 30.09917]` | NED origin of the world, used as the reactor 4 area | <https://en.wikipedia.org/wiki/Chernobyl_disaster>: "51.38972°N 30.09917°E", labelled "Chernobyl disaster", the same point as the Location field (the plant); five decimals, about 1 m of notation, accuracy not stated. The Chernobyl NPP article gives the plant at "51.38917°N 30.09944°E" (about 60 m south, 20 m east). Wikidata's NSC (Q1506085) after the slide reads 51.389, 30.099, precision 0.001 | estimate: no source gives reactor 4 itself; this is the plant/disaster point, within tens of metres of it. It places the OSM outlines and the arch |
| `half_extent_m = [300.0, 300.0]` | The area the build keeps | none | artistic |
| `span_m = 257.0` | Arch width | <https://chnpp.gov.ua/en/infocenter/news/204-projects/nsc-construction/578-q-q578>: "Span of Arch: 257m"; also Structurae (<https://structurae.net/en/structures/chernobyl-new-safe-confinement>, "arches span 257 m", read through a search result: the page itself answers 403 to a script) | published |
| `length_m = 162.0` | Arch length along its axis | the same page: "Length: 162m"; Structurae "length 162 m" | published |
| `height_m = 108.0` | Outer height | the same page: "Height: 108m"; Structurae "height 108 m"; Wikipedia infobox "108 metres" | published |
| internal height 92.5 m | Not in the recipe. Noted only: the inside of the vault is lower than the outer height; the generator builds one thin shell at the outer size, so the inside is a little taller than reality | <https://en.wikipedia.org/wiki/Chernobyl_New_Safe_Confinement>: "internal height of 92.5 metres (303.5 ft)"; not on the Structurae page | published (single source) |
| `heading_deg = 90.0` | Axis east-west | Built west of reactor 4 and slid east: <https://en.wikipedia.org/wiki/Chernobyl_New_Safe_Confinement> "constructed 180 metres (590 ft) west of reactor 4, and slid into place" (the article does not name the direction of the final slide; it follows from the arch standing west of the reactor and ending over it, and it says "moved 112 metres (367 feet) eastward" for an earlier move). Wikidata Q1506085 (P625): 51.389, 30.094 before 2016-11-14 and 51.389, 30.099 after: the position moved east, by 0.005 degrees of longitude, about 350 m, with the article's 327 m slide. The rail axis is therefore east-west | published (direction inferred) |
| `center_ned_m = [-53.946, -27.065]` | Centre of the arch | the bounding-box centre of the OSM NSC outline (way 456732992), projected to NED; the outline is skipped and the arch stands in its place | OSM, estimate (the arch's own centre is within a metre or so) |
| `segments = 48` | Facets round the vault | none | artistic |
| `end_doors = [["back", 40.0, 40.0]]` | An open 40 x 40 m door in the west ("back": heading 90 puts the front at the east) end. Only the west end because the east end almost touches unit 3 | none: the real end walls are closed | artistic (a gameplay liberty so a quad can fly in) |
| `rf_loss_db = 40.0` (`nsc_cladding`) | Loss per surface crossed | none | estimate (a steel-clad shell; not measured) |
| `rf_loss_db = 30.0` (`building`) | Loss per OSM building wall crossed | none | estimate (concrete at 5.8 GHz; not measured) |
| `nsc_cladding = [0.45, 0.47, 0.50]` | Colour, steel grey | none | artistic (dark enough to read the vault's facets in the sun) |
| `building = [0.55, 0.52, 0.48]` | Colour of the OSM buildings | none | artistic |
| pilot `[-53.946, -258.065, -1.7]`, facing `90.0` | 150 m west of the door (the west end is at east = -108.065, the arch centre minus half of 162 m) at eye height, looking east | none | artistic |
| quad `[world.initial] [-53.946, -208.065, -2.0]`, yaw `90.0` | 100 m west of the door, outside the arch | none | artistic |
| receiver (-93 dBm, omni 2 dBi, rhcp) | The same receiver as `worlds/hall.toml` | `worlds/hall.toml` | estimate |
| OSM snapshot | 40 building footprints in the 600 x 600 m box, fetched 2026-11-17 | Overpass API; `chernobyl.osm.json` records the bbox, query and source; © OpenStreetMap contributors, ODbL 1.0 | OSM |
| Unit 3 | 70 m | OSM way 326145079, `height=70` | OSM |
| Shelter object | 65 m | OSM way 500551951, `height=65` | OSM |
| Stabilisation steel structure | 63 m | OSM way 500551953, `height=63` | OSM |
| Turbine hall | 50 m, about 600 m long | OSM relation 8705336, `height=50` | OSM |
| Turbine-hall breach | 30 x 20 m, bottom 5 m, west end wall | the 2013 collapse of 600 m2 of roof and wall: <https://world-nuclear-news.org/articles/safety-work-planned-for-chernobyl-turbine-hall>; the wall index comes from `python -m sitegen walls` | artistic |

The recipe skips the OSM NSC outline (way 456732992, replaced by the arch) and the unit 4 ruins (way 500551950, which
overlap the shelter object). The other 38 footprints are kept; the build report pins the count.

Other published figures cited for comparison, which the recipe does not follow: Wikipedia's infobox gives a span of 260 m and an
external length of 165 m (and a body text length of 150 m, internal span 245 m, external span 270 m); the Vinci page gives
105 m high and 150 m long. The operator's page and Structurae agree on 257 / 162 / 108, which the recipe uses.

## What the map deliberately omits

- **The ventilation stack VP-2** (75.5 m tall on the roof of unit 4, its top 150 m above ground) was dismantled from 31 October to 25 November 2013 so the NSC
  could be slid over reactor 4 (<https://world-nuclear-news.org/Articles/Chernobyl-ventilation-stack-removed>). The NSC and VP-2 never
  stood together; the map is the present-day site, so no chimney is built (`test_no_chimney_is_built` caps the height at 108.5 m).
- The real arch is a double-skin steel structure with a closed end wall at each end; this map is one thin shell with one open door.
- The OSM buildings are flat-roofed extrusions with no interiors; the shelter object and the stabilisation structure are
  kept as blocks, not detailed interiors.
- The 2013 turbine-hall collapse is a wall breach only; the generator's `openings` are wall openings, not roof holes.
- Flat ground: the generator has no elevation.
