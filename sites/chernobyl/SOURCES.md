# Chernobyl map: sources

Stage A of the Chernobyl map: the New Safe Confinement (NSC) arch alone, built from published dimensions. No
OpenStreetMap data is used yet (`chernobyl.osm.json` is hand-made and empty; the real snapshot waits for the licence
sign-off in `docs/research/sitegen-licensing.md`). The world header still carries the OpenStreetMap credit, which is harmless.

Each number in `recipe.toml` is one of: **published** (a source and the figure as it reads there), **estimate** (a guess
with a stated reason), or **artistic** (a gameplay or look decision, not a fact about the site).

| Value | What it is | Source | Kind |
|---|---|---|---|
| `origin = [51.38972, 30.09917]` | NED origin of the world, used as the reactor 4 area | <https://en.wikipedia.org/wiki/Chernobyl_disaster>: "51.38972°N 30.09917°E", labelled "Chernobyl disaster", the same point as the Location field (the plant); five decimals, about 1 m of notation, accuracy not stated. The Chernobyl NPP article gives the plant at "51.38917°N 30.09944°E" (about 60 m south, 20 m east). Wikidata's NSC (Q1506085) after the slide reads 51.389, 30.099, precision 0.001 | estimate: no source gives reactor 4 itself; this is the plant/disaster point, within tens of metres of it. The origin has no effect on Stage A's output (the same bytes with another origin); it places the OSM outlines in Stage B |
| `half_extent_m = [300.0, 300.0]` | The area the build keeps | none | artistic |
| `span_m = 257.0` | Arch width | <https://chnpp.gov.ua/en/infocenter/news/204-projects/nsc-construction/578-q-q578>: "Span of Arch: 257m"; also Structurae (<https://structurae.net/en/structures/chernobyl-new-safe-confinement>, "arches span 257 m", read through a search result: the page itself answers 403 to a script) | published |
| `length_m = 162.0` | Arch length along its axis | the same page: "Length: 162m"; Structurae "length 162 m" | published |
| `height_m = 108.0` | Outer height | the same page: "Height: 108m"; Structurae "height 108 m"; Wikipedia infobox "108 metres" | published |
| internal height 92.5 m | Not in the recipe. Noted only: the inside of the vault is lower than the outer height; the generator builds one thin shell at the outer size, so the inside is a little taller than reality | <https://en.wikipedia.org/wiki/Chernobyl_New_Safe_Confinement>: "internal height of 92.5 metres (303.5 ft)"; not on the Structurae page | published (single source) |
| `heading_deg = 90.0` | Axis east-west | Built west of reactor 4 and slid east: <https://en.wikipedia.org/wiki/Chernobyl_New_Safe_Confinement> "constructed 180 metres (590 ft) west of reactor 4, and slid into place" (the article does not name the direction of the final slide; it follows from the arch standing west of the reactor and ending over it, and it says "moved 112 metres (367 feet) eastward" for an earlier move). Wikidata Q1506085 (P625): 51.389, 30.094 before 2016-11-14 and 51.389, 30.099 after: the position moved east, by 0.005 degrees of longitude, about 350 m, with the article's 327 m slide. The rail axis is therefore east-west | published (direction inferred) |
| `center_ned_m = [0.0, 0.0]` | Centre of the arch at the origin | none | estimate; refined against the OSM outline in Stage B |
| `segments = 48` | Facets round the vault | none | artistic |
| `end_doors = [["back", 40.0, 40.0]]` | An open 40 x 40 m door in the west ("back": heading 90 puts the front at the east) end. Only the west end because the east end almost touches unit 3 | none: the real end walls are closed | artistic (a gameplay liberty so a quad can fly in) |
| `rf_loss_db = 40.0` (`nsc_cladding`) | Loss per surface crossed | none | estimate (a steel-clad shell; not measured) |
| `nsc_cladding = [0.72, 0.74, 0.76]` | Colour, a light steel grey | none | artistic |
| `building = [0.55, 0.52, 0.48]` | Colour the recipe schema requires; no triangle uses it | none | artistic |
| pilot `[0.0, -231.0, -1.7]`, facing `90.0` | 150 m west of the door (the west end is at east = -81 m, half of 162 m) at eye height, looking east | none | artistic |
| receiver (-93 dBm, omni 2 dBi, rhcp) | The same receiver as `worlds/hall.toml` | `worlds/hall.toml` | estimate |

Other published figures cited for comparison, which the recipe does not follow: Wikipedia's infobox gives a span of 260 m and an
external length of 165 m (and a body text length of 150 m, internal span 245 m, external span 270 m); the Vinci page gives
105 m high and 150 m long. The operator's page and Structurae agree on 257 / 162 / 108, which the recipe uses.

## What the map deliberately omits

- **The ventilation stack VP-2** (75.5 m tall on the roof of unit 4, its top 150 m above ground) was dismantled from 31 October to 25 November 2013 so the NSC
  could be slid over reactor 4 (<https://world-nuclear-news.org/Articles/Chernobyl-ventilation-stack-removed>). The NSC and VP-2 never
  stood together; the map is the present-day site, so no chimney is built (`test_no_chimney_is_built` caps the height at 108.5 m).
- The old sarcophagus, now inside the NSC, and everything else on the site. Stage B adds the surrounding buildings from
  OpenStreetMap once the licence is confirmed.
- The real arch is a double-skin steel structure with a closed end wall at each end; this map is one thin shell with one open door.
- Flat ground: the generator has no elevation.
