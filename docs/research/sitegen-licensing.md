# OpenStreetMap data in a GPLv3 repository

This is the implementer's reading, not legal advice, and the repo owner confirms it before any snapshot is committed in piece 3 (the Chernobyl map).

**Status: UNCONFIRMED.** Nothing below is a ruling. No OSM snapshot is committed until the owner has ticked every box in "Owner sign-off" at the end.

The site generator (`tools/sitegen/README.md`) turns building outlines from OpenStreetMap into a `.glb` mesh and a world file. OpenStreetMap data is licensed under the Open Database License 1.0 (ODbL), not the repository's GPLv3. This note says what the ODbL asks for, how the repository meets it, and what to do when it does not apply or cannot be met. Sources, paraphrased here and not copied:

- the ODbL 1.0 text: <https://opendatacommons.org/licenses/odbl/1-0/> (section numbers below are its own);
- the OSMF attribution guidelines: <https://osmfoundation.org/wiki/Licence/Attribution_Guidelines>;
- OSM's copyright and licence page: <https://www.openstreetmap.org/copyright>.

The sections cited were read on 2026-11-01. The licence text and the guidelines can change; the owner should read both once, not rely on this paraphrase.

## The three objects

| Object | Where it lives | ODbL term (section 1.0) |
|---|---|---|
| The snapshot: building ways with their tags, as fetched and normalised | `<site>.osm.json`, committed next to the recipe | A **Derivative Database**: a database built from a substantial part of the OSM database (here a rearranged extract of it). |
| The generated mesh and world file | `<site>.glb`, `worlds/<site>.toml` | A **Produced Work**: a work that results from querying the database for a whole or substantial part of it. |
| The recipe | `recipe.toml` | Neither: it holds our own choices (heights, materials, landmarks, skips). Footprint ids refer to OSM ways but carry no geometry. |

## What the ODbL asks

1. **Attribution of the Produced Work (4.3).** Creating a Produced Work needs no notices, but publicly using one requires a notice that the contents came from the database and are available under the ODbL. The OSMF guidelines say what that looks like in practice: credit "OpenStreetMap" (the older "© OpenStreetMap contributors" is still accepted), make it clear the data is under the ODbL, link to <https://www.openstreetmap.org/copyright> where a link is possible and print the URL where it is not. For a game the guidelines allow a splash screen, credits or a menu, with details reachable somewhere. For a dataset they ask for the attribution and the licence text or a link to it where users will look, such as a readme.
2. **Share-alike for the Derivative Database (4.4).** A Derivative Database that is publicly used must be licensed under the ODbL (or a later version or a compatible licence). Extracting a substantial part of the contents into a new database counts. Section 4.4 also treats a Derivative Database as publicly used when a Produced Work made from it is.
3. **Limits of share-alike (4.5).** Making a Produced Work from the database does not make the Produced Work a Derivative Database, so the Produced Work is not forced under the ODbL. Internal use is not public use.
4. **Notices on the Database itself (4.2).** Whoever publicly conveys the database or a Derivative Database includes the licence or its URI and keeps existing notices intact.
5. **No technical measures against the licence (4.7).** Nothing in the repository may restrict what the ODbL lets a recipient do with the snapshot.

## Reading for this repository

- **The snapshot stays ODbL.** It is a Derivative Database, and committing it to a public repository is public use. It is not covered by the repository's GPLv3 and must say so. The GPLv3 does not apply to it, and the ODbL does not apply to the Rust, Python and Godot code.
- **The mesh and the world file are a Produced Work.** Our reading is that they may be distributed under the repository's GPLv3 with the OSM attribution next to them, because 4.5 says a Produced Work is not a Derivative Database.
- **The weak point of that reading** is "substantial extraction". The `.glb` holds the outlines (to a millimetre) and heights of every footprint used, so someone could argue it is the database in another form and therefore itself a Derivative Database that must be ODbL. We do not know how a court or the OSMF would draw that line for a triangle mesh. The conservative response costs nothing here: the full snapshot is public in the same repository under the ODbL, so anyone can get the data under the licence whatever the status of the mesh. The owner decides whether to additionally mark the generated `.glb` and world file as "contains ODbL data".
- **Landmarks are ours.** Arch, stack and walls landmarks are built from recipe numbers, not from OSM geometry (their `center` coordinates are points we chose, and a recipe may use `center_ned_m` instead). Only footprints come from the snapshot.

## How the repository satisfies it

| Obligation | What the code and docs do |
|---|---|
| Attribution travels with the data | `sitegen.osm.ATTRIBUTION` is written as the `attribution` key of every snapshot: `© OpenStreetMap contributors, ODbL 1.0 https://www.openstreetmap.org/copyright`. The snapshot also records its bbox, the exact Overpass query and the source (`source_note`), so it can be checked against OSM. |
| Attribution on the Produced Work | `sitegen.build.ATTRIBUTION` is the third line of the header of every generated world file: `# © OpenStreetMap contributors (ODbL 1.0, https://www.openstreetmap.org/copyright)`. The tests pin it (`test_world_header_has_attribution_and_cyrillic_name_round_trips`). |
| Attribution in the docs | `tools/sitegen/README.md` and the "Generating a world from real data" section of `docs/dev-setup.md` carry the line and link here. |
| Licence text or link | Each attribution line names the ODbL and links the OSM copyright page. |
| Snapshot kept ODbL, not relicensed | The snapshot format has no field that claims another licence, and the recipe and code never rewrite it. |

Gaps the owner should look at (nothing below is done yet):

1. **No in-game credit.** The world file's header is only seen by someone who opens the file. The Godot client shows nothing about OSM. If the owner reads 4.3 as applying to the game, the Chernobyl world needs a credit in the client (a credits or help screen) or at least in the top-level README. This is a decision for piece 3.
2. **A committed snapshot has the attribution inside the JSON but no licence file beside it.** The OSMF guidelines ask for the licence text or a link where users will look. Piece 3 could add a short `README` or `LICENSE-ODbL` note in the snapshot's directory and a line in the top-level README, saying that these files are ODbL and the rest of the repository is GPLv3.
3. **Hand edits.** The recipe is where our own judgement goes (skips, heights). A hand-edited snapshot is still a Derivative Database and keeps its licence; the format is meant to be re-fetched, not edited.

## A recipe whose footprints are not from OSM

`footprints` entries in a recipe only adjust ways that exist in a snapshot (`skip`, `height_m`, `material`). A site with no snapshot is not supported by the code today: `site.snapshot` is a required key and `build` fails if the file is missing or empty of geometry. Two ways to avoid OSM data:

- **Hand-listed outlines as `walls` landmarks** (corners in `corners` or `corners_ned_m`). These are our own measurements, so no ODbL applies, provided the corners were not read off OSM, off an OSM-based map or tiles, or traced from a source with its own licence. Say where they came from in the recipe, with a comment per building. The snapshot of such a site can be a hand-written file with `"ways": []`, but then a build with no landmarks fails with "no geometry", so the site needs at least one landmark. The README says the same.
- **Outlines traced from imagery or taken from a mapping agency.** Each source has its own licence. Check it before using it. Nothing here covers them.

Mixing the two is possible: in our reading the ODbL then concerns the OSM-derived part only, but the generated `.glb` contains both. Keep the OSM footprints in a separate recipe if the owner wants a mesh with no ODbL ancestry.

## Fallback if the owner is not satisfied

If the owner decides the ODbL cannot sit in this repository, or that the `.glb` must not be a Produced Work of an ODbL database:

1. Do not commit a snapshot. Keep the fetch and the build local, and commit only the recipe (which has no OSM geometry) and the instructions. Each contributor then fetches their own snapshot with `python -m sitegen fetch`; the generated world stays on their machine. The Chernobyl map would not ship.
2. Or build the site from hand-measured landmarks only (no snapshot geometry), citing its own sources as above.
3. Or put the snapshot, and possibly the generated mesh, in a separate repository or release asset that carries the ODbL, and keep the GPLv3 repository to the generator.

None of these needs a change to the generator, apart from allowing an empty snapshot path for option 2 if that route is chosen.

## Owner sign-off

Status: UNCONFIRMED. Tick each line, date it, and change the status line at the top when all are done.

- [ ] I read the ODbL 1.0 (sections 1.0, 4.2 to 4.5, 4.7) and agree with the "Reading for this repository" section, or I amended it.
- [ ] A committed snapshot is ODbL and is marked as such where a reader of the repository will find it (decision on gap 2).
- [ ] The generated `.glb` and world file may be distributed under GPLv3 with the OSM attribution, or I chose to mark them "contains ODbL data" as well.
- [ ] I decided how the Chernobyl world credits OpenStreetMap in the game (gap 1).
- [ ] I accept the fallback above if any of these cannot be met.

Date and initials: ____________
