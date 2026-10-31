# A drone workbench over the parts library (#128; epic #136)

Client work: Godot screen + bridge + one listing function in `ofs-config`. No protocol, server, physics or Python
change (`PROTOCOL_VERSION` stays 15). The brief is issue #128; no design conversation took place, so every ruling
below is the author's call and open to change on review. The phase umbrella (task 4 of
`docs/superpowers/plans/2026-10-31-game-completion.md`) adds one binding: this spec leaves the seam epic #160 names —
repair is "a workbench action" — without building any of it.

## Why

The parts library is the simulator's best-kept secret: nine kind directories of TOML files with recorded provenance,
`OFS_LIBRARY` roots, quads that reference parts by id — and assembling one is a text editor (`library/README.md`).
The menu (PR #141, f122b83) lists quads but cannot make one. The workbench is the screen that assembles a quad from
library parts, writes it where the menu loads it, and lets the pilot fly it.

## Goals

1. A **workbench screen** reached from the main menu: pick a starting quad, swap its **frame, motors, props, ESC,
   battery and VTX** for any part the library roots hold, name the assembly, save.
2. Every listed part shows its **parameters with provenance** — `datasheet` / `measured` / `estimated`, link and note —
   estimated values marked, the vocabulary `sim.parts` already reports (`LoadReply.parts`, proto `PartInfo`).
3. **Save writes a quad**: the assembly's TOML plus a copied starting Betaflight diff into a user directory the menu
   already-style lists, with the template's wiring untouched (the "wiring defaults filled in" of #128).
4. **Invalid combinations are the loader's refusal, surfaced** — never re-written by the workbench.
5. The round trip: assemble, save, pick it in the menu, Start, fly.
6. The **seam for #157** (crash damage: degrade and repair) is named and preserved; nothing of it is built.

## Non-goals

- No physics, model or protocol change; no new bus signal; no part kinds; no part authoring (that is the
  text-editor flow `library/README.md` documents); no computed flight character (thrust-to-weight and hover throttle
  come from flying, `crates/ofs-sim/tests/quads.rs`, not from the library).
- No editing of wiring: UARTs, `sim.base_hz`, `[radio]`, `[home]`, `[initial]`, VTX band/channel/power, battery
  `initial_soc`/`rate_hz` are the starting quad's, verbatim.
- No per-motor mixed picks (the broadcast `motors = ["id"]` every shipped quad uses), no picking `imu`/`baro`/`mag`
  (carried from the starting quad, shown read-only), no rename/delete UI for assemblies (the user directory is the UI).
- No fork of the settings chain: **no new keys** in `settings.gd`, nothing written to `user://ofs_client.cfg` by the
  workbench, the menu-less e2e path (`OFS_MENU=off`) keeps working.

## Design calls

**DC1. The screen and how it is reached.** `godot/ui/workbench_screen.gd`, a `CanvasLayer` on layer 8 of
`main.tscn` (the menu is 6, settings 7), opened by a **Workbench** button on the main menu (the Controls button's
pattern: menu closes, screen opens, Esc/Back returns). The model lives in `godot/scripts/workbench.gd`
(`RefCounted`, no engine calls beyond file access) so the headless tests drive it without the screen — the
calibration split of PR #149 (`scripts/calibration.gd` vs the wizard panel). Pure provenance rendering is
`godot/scripts/provenance_view.gd`, static functions over part dictionaries.

**DC2. Listing and validation go through the bridge, not a GDScript TOML parser.** `ofs-config` gains
`part::list(roots, kind_dir) -> Vec<ListedPart>` beside its loaders: every `*.toml` of `kind_dir` under each root
**in root order, the first root that has an id wins** (the loader's own rule, `crates/ofs-config/src/lib.rs:786`);
each entry carries `id`, `kind` (singular, `sim.parts`'s vocabulary), `name`, the root it came from, the dotted
parameter paths with their values and their provenance, or the load error when the file does not parse. A broken
part is listed with its error, never hidden and never fatal. The new Godot class `OfsLibrary` (`ofs-godot`,
`RefCounted`, a thin engine shell like `OfsClient` is over `ofs-client`) exposes three functions: `roots(env,
quad_dir)` (wraps `ofs_config::library_roots`), `list(roots, kind_dir)` (wraps `part::list`), and `check_quad(path,
library_env)` — `ofs_config::load_with_library_env`, returning `{ok, name, auw_kg, parts}` or the loader's refusal
text verbatim (`ConfigError`'s all-problems-at-once rendering). `ofs-godot` gains the `ofs-config` workspace
dependency (it already builds at MSRV 1.94 against 1.85 crates). Nothing joins the protocol.

**DC3. Assemblies persist in `user://quads/` as plain quad files.** `<stem>.toml` plus `<stem>.betaflight.diff`
(the starting quad's diff copied verbatim, `betaflight_diff` re-pointed at the copy). `user://` is the client's
per-user home (the pattern of `user://controls.json` and `user://recordings`); the files are hand-editable,
copyable, deletable. The menu lists them because `Catalog.roots` gains the globalized
`ProjectSettings.globalize_path("user://quads")` as a third quads root (`Catalog.list` already de-duplicates and
skips missing dirs) — so one absolute-path shape flows through `quad_path`, the menu row and the server's `Load`.
No sidecar, no database: the quad file **is** the assembly (reopening reads the six ids and the name back out of
it, DC5). The settings chain is untouched — no new keys, nothing saved unless the pilot picks the assembly in the
menu (the menu's existing `choice_changed("quad_path", …)`).

**DC4. An assembly is seeded from a starting quad, not from built-in defaults.** New assembly picks a quad from the
menu's own list (default: the quad the menu has selected). The workbench copies that file's text as the template
and the pilot swaps the six picks and the name; everything else — `schema_version` (6/7 today, `SCHEMA_VERSION = 7`
at `crates/ofs-config/src/lib.rs:20`; #128's "schema-5" predates schemas 6–7), `[sim]`, `[home]`, `[initial]`,
`[fc]` (launch, timeouts, the diff), `[radio]`, `[esc_telemetry]`, `[osd]`, `[vtx]` wiring (uart, band, channel,
power index), `imu`/`baro`/`mag`, `[battery]` wiring — is the template's, verbatim: that is "the wiring defaults
filled in". The write is a line patch of the template text (the `settings.gd` `with_value` style): `name`,
`frame`, `motors`, `props`, `escs`, `[battery] part`, `[vtx] part`, `[fc] betaflight_diff`. A starting quad
without a `[vtx]` section (none shipped) keeps having none; the VTX row is then shown as absent, not picked.

**DC5. Validation is the simulator's own, surfaced.** The workbench duplicates no rule and pre-implements no
check. Save always writes the file, then calls `check_quad`; the screen shows the summary (loads clean) or the
loader's refusal text **verbatim** — the same `ofs_config::load` ofs-sim runs at `Load`
(`crates/ofs-sim/src/server.rs:288`, refused as `config`/`InvalidArgument` with the full message), so the workbench,
the menu's Start failure and a hand load all say the same thing. The refusal does not delete the file. With
shipped parts the live cases are real: a 3S battery (`opendrone-3s-450`) in an assembly whose starting quad's diff
sets `force_battery_cell_count = 6` refuses with the loader's message naming the line to fix
(`crates/ofs-config/src/lib.rs:1439`); `motor_poles` vs `pole_pairs` is the same shape (all shipped motors are 7
pole pairs, so it does not fire today). Reopening an assembly reads the ids by a tolerant line parse (it does not
validate), so an invalid assembly still opens and can be fixed.

**DC6. The library the workbench lists is the library the session flies.** The client computes its roots exactly as
the loader does — `OFS_LIBRARY` from the OS environment when set, else the checkout's `library/`
(`library_roots(None, <repo>/quads)`). When it starts the server it now also passes
`env["OFS_LIBRARY"] = <the joined roots>` in `to_client_dict`'s launch env (the existing `OFS_SITL_LAUNCH`
mechanism; `crates/ofs-client/src/launch.rs:21` adds without clearing, so a shell-set variable already passes
through). This is what makes a `user://quads` assembly resolvable at all: its own default root
(`<assembly dir>/../library`) does not exist. Consequences, accepted: a server the client starts always resolves
through the client's roots (a `library/` beside a custom quads directory is shadowed — set `OFS_LIBRARY` to add
roots, the README's rule); a server started by hand keeps its own environment's `OFS_LIBRARY`, so an external
server that does not see the workbench's roots refuses the assembly with the loader's unknown-part message. That
limit is documented, not engineered around.

**DC7. Provenance rendering and the summary.** A part's detail lists every dotted parameter path with its value,
its source (`datasheet` | `measured` | `estimated`), its link and note; **estimated values are marked** (a `~`
prefix and the word "estimate"), matching `sim.parts`' `part.estimated` in spirit (`python/ofs/client.py:314`).
The assembly summary is what the library knows: **all-up weight = the frame part's `mass_kg`** (the library's
convention — the shipped frames' own provenance notes say "all-up weight", `library/frames/opendrone-3t.toml:23`),
shown in grams, plus the part list by name and each part's headline numbers (cells/capacity, kV, prop diameter,
VTX mW). No thrust, no hover estimate, no AUW arithmetic over parts (no other part carries mass).

**DC8. The repair seam for #157 (binding, nothing built).** Epic #160: damage rides the existing fault seams
(`Fault::PropDamage`, `Fault::MotorFailure`, `crates/ofs-sim/src/vehicle.rs:227`), repair is "a workbench action",
and `Load` is a reset. The seam this spec leaves: **the workbench's per-kind part rows are the per-part surface** —
#157's condition display and Repair action attach to those rows (per motor/prop index when mixed picks arrive),
clearing or injecting faults on the running session, never a parallel damage system and never a field in the
assembly file (condition is session state; `Load` resets it). To keep #157 an extension rather than a rewrite, the
part rows are addressable by kind (`row("motors")`) the way the menu's are (`main_menu.gd`'s `row(name)`), and the
screen's details panel is driven by `select_part(kind)` rather than by UI focus order. Nothing of it exists in
this milestone: no health field, no repair call, no fault UI.

**DC9. Not in this milestone.** Editing wiring; picking `imu`/`baro`/`mag`; mixed per-motor picks; a "save and
fly" button (the menu's Start is the load-and-fly path, and the round trip is the test); editing or generating
diff content (a stale `force_battery_cell_count` is surfaced, not patched — patching it would be the workbench
knowing a loader rule, DC5); deleting or renaming assemblies in the UI; listing `user://worlds` (worlds are the
world tasks' ground).

## What is left to the builder

Layout and wording of the screen; the details panel's density; how many parameters show before scrolling; the
summary's exact line order.

## Checked by eye (headless Godot renders nothing)

The screen's look and focus navigation on a display; gamepad navigation across the part rows; the `~`-marked
estimate at a glance; an assembly saved from a real starting quad appearing in the menu and flying with Betaflight
(manual QA lands in #163 per the umbrella).

## Open questions for the maintainer

1. **Sensor picks.** `imus/tdk-icm-42688p.toml` exists to be swapped (the README says so), and DC4 carries
   `imu`/`baro`/`mag` over from the starting quad rather than letting the pilot pick them. Ruled out as scope
   (the issue names six kinds); say the word and they become three more cycle rows, same mechanism.
2. **Stale diffs.** DC5 surfaces the `force_battery_cell_count` refusal verbatim instead of patching the copied
   diff when the battery's cell count differs from the starting quad's. Ruled for honesty (the workbench knows no
   loader rules); the alternative — the workbench rewriting that one `set` line on save — would make battery swaps
   always flyable at the cost of DC5's purity.
