# Drone Workbench Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Godot workbench screen that assembles a quad from library parts, writes it with its starting Betaflight diff into `user://quads/` where the menu loads it, and lets the pilot fly it; invalid combinations surface the loader's own refusal. Closes #128.

**Architecture:** `ofs-config` gains one pure listing (`part::list`: every part of a kind across the `OFS_LIBRARY` roots, the first root that has an id wins). `ofs-godot` gains an `OfsLibrary` class — a thin engine shell over that listing and over `ofs_config::load_with_library_env`, which is the simulator's own validation, verbatim. GDScript does the rest: a workbench model (`scripts/workbench.gd`) that seeds an assembly from a starting quad's TOML, patches the six part picks into it, saves to `user://quads/` and reports the loader's verdict; a provenance renderer; a screen behind a menu button. The client passes `OFS_LIBRARY` in the launch env so the server it starts resolves an assembly's parts.

**Tech Stack:** Rust workspace (MSRV 1.85; `ofs-godot` is 1.94), godot-rust 0.5, Godot 4.7.2 GDScript. No protocol change (15), no `ofs-sim`/`ofs-fc`/Python change.

**Spec:** `docs/superpowers/specs/2026-10-31-drone-workbench-design.md` (the design calls DC1–DC9 below are its rulings, condensed; executors read both). GitHub issue #128.

## Global Constraints

- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked` green on Windows; `cargo +1.85 check -p ofs-config` passes (`ofs-godot` stays at its own 1.94 rust-version; the msrv job skips it).
- `PROTOCOL_VERSION` stays 15. No file under `proto/`, `crates/ofs-sim/`, `crates/ofs-fc/` or `python/` is touched.
- The settings chain is not forked: **no new keys** in `settings.gd`; nothing writes `user://ofs_client.cfg` except `save_value` on a menu/settings change; the menu-less e2e path (`OFS_MENU=off`) keeps working.
- No `cargo fmt`; hand-indent new Rust and GDScript (house style). Write GDScript with Write/Edit, never a heredoc.
- Godot tests in WSL: `bash scripts/run-godot-tests.sh unit` then `... e2e` (the runner builds `ofs-sim` + `ofs-godot` itself; it prints `0 failed` even on a SCRIPT ERROR, so grep the output for `SCRIPT ERROR`). Python tests need `PYTHONPATH=<worktree>/python`.
- Anchors cited from files were verified against `main` at `87c5914` (2026-10-31); re-verify at execution anything marked so, and any `file:line` before editing around it.
- Commits: one per task, after its tests pass, conventional style; no trailer prescribed (recent merged work carries none — an executing session that adds one names its own identity).

## Design calls (decided here; the executor ledgers any change)

1. **Listing in `ofs-config`, not GDScript** (DC2): `part::list(roots, kind_dir)` walks the roots in order with the loader's first-root-wins rule; a part that fails to load is listed with its error, never hidden. The workbench never parses part TOML.
2. **Validation is `ofs_config::load_with_library_env`** (DC5): the workbench's `check` is that call; its full `ConfigError` text (all problems at once) is shown verbatim. The workbench duplicates no rule and patches no diff.
3. **Assemblies live in `user://quads/`** (DC3): `<stem>.toml` plus `<stem>.betaflight.diff` copied verbatim from the starting quad; the menu lists the dir because `Catalog.roots` gains it globalized; no sidecar — the quad file is the assembly.
4. **Seed from a starting quad** (DC4): the template's text is patched by line (`name`, `frame`, `motors`, `props`, `escs`, `[battery] part`, `[vtx] part`, `[fc] betaflight_diff`); `schema_version`, wiring, `imu`/`baro`/`mag` are the template's, verbatim.
5. **The launch env carries `OFS_LIBRARY`** (DC6): `to_client_dict` adds it whenever it starts a server, so `user://quads` assemblies resolve; an external server keeps its own environment (documented limit).
6. **Estimated values are marked** (DC7): `~` prefix plus the word "estimate" in `scripts/provenance_view.gd`; AUW is the frame part's `mass_kg` (the library's convention — `library/frames/opendrone-3t.toml:23` says "all-up weight" in the provenance note).
7. **The #157 seam, not built** (DC8): part rows are addressable by kind (`row("motors")`), the details panel is driven by `select_part(kind)`; no health field, no repair call, no fault UI.
8. **The stem rule**: the pilot's name with `<>:"/\|?*` and control characters dropped, runs of whitespace to `-`, leading/trailing dots and dashes trimmed; empty → `assembly`. Save with the same stem overwrites both files.

## Review Focus

- Two roots holding the same id: the listing shows the first root's part (its values, its provenance) — `the_first_root_that_has_an_id_wins` (task 1).
- A part file with missing provenance must not vanish from the listing or crash the screen: it appears with its error — `a_broken_part_is_listed_with_its_error` (task 1), `test_list_marks_a_broken_part` (task 2).
- A 3S battery in a 6S-template assembly is saved, kept, and the loader's `force_battery_cell_count` message is shown verbatim, never patched — `test_save_surfaces_the_loaders_refusal_verbatim` (task 3), the screen test (task 5).
- An assembly in `user://quads` flies only because the launch env carries `OFS_LIBRARY`; the e2e's Start is the proof — the round-trip section (task 6).
- Reopening a refused assembly still seeds the six picks (the tolerant parse never validates) — `test_reopening_reads_picks_from_a_refused_assembly` (task 3).
- The menu still lists repo and data-dir quads, keeps the saved selection, and an `OFS_QUAD` flag still locks the row — `test_quad_roots_include_the_users_quads_dir` / `test_world_roots_do_not` (task 4); the existing menu tests must pass unchanged.

## File Structure

```
crates/ofs-config/src/part.rs         ListedPart, list(), per-kind listed loaders (task 1)
crates/ofs-config/tests/part.rs       listing tests (task 1)
crates/ofs-godot/Cargo.toml           + ofs-config workspace dep (task 2)
crates/ofs-godot/src/lib.rs           OfsLibrary: roots / list / check_quad (task 2)
godot/scripts/workbench.gd            the model: roots, template, patch, save, check (task 3)
godot/scripts/provenance_view.gd      pure provenance/summary rendering (task 3)
godot/scripts/settings.gd             to_client_dict passes OFS_LIBRARY (task 3)
godot/scripts/catalog.gd              quads roots gain user://quads, globalized (task 4)
godot/ui/main_menu.gd                 Workbench button + workbench_requested (task 4)
godot/ui/workbench_screen.gd          the screen (task 5)
godot/main.tscn, godot/scripts/app.gd the screen node and wiring (task 5)
godot/tests/test_library.gd           new suites (tasks 2-5), registered in run_tests.gd
godot/tests/test_provenance_view.gd
godot/tests/test_workbench.gd
godot/tests/test_workbench_screen.gd
godot/tests/e2e_open_loop.gd          the round trip (task 6)
README.md, library/README.md, docs/architecture.md, CHANGELOG.md  (task 7)
```

---

### Task 1: The library listing in `ofs-config` (`part::list`)

**Files:**
- Modify: `crates/ofs-config/src/part.rs`
- Test: `crates/ofs-config/tests/part.rs`

**Interfaces:**
- Consumes: the existing `parse_part`, `check_provenance` and part structs in `part.rs` (verified at `crates/ofs-config/src/part.rs:230-266`).
- Produces:
  - `pub struct ListedPart { pub id: String, pub kind: &'static str, pub root: PathBuf, pub name: String, pub values: BTreeMap<String, toml::Value>, pub provenance: BTreeMap<String, Provenance>, pub error: Option<String> }` (`Debug, Clone`; `name` empty and `error` set when the file does not load).
  - `pub fn list(roots: &[PathBuf], kind_dir: &str) -> Vec<ListedPart>` — `kind_dir` is a directory name (`"frames"`, `"motors"`, `"props"`, `"escs"`, `"batteries"`, `"vtx"`, `"imus"`, `"barometers"`, `"magnetometers"`); roots in order, first root that has an id wins; result sorted by id; an unknown `kind_dir` or a missing directory contributes nothing.

- [ ] **Step 1: Write the failing tests** in `crates/ofs-config/tests/part.rs` (reuse the file's existing `write` helper and `FRAME`/`PROP` consts; add `use std::path::PathBuf; use ofs_config::part::list;`):

```rust
#[test]
fn the_listing_names_every_part_of_a_root_sorted_by_id() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    write(&root.join("frames"), "b.toml", FRAME);
    write(&root.join("frames"), "a.toml", FRAME);
    let parts = list(&[root.clone()], "frames");
    assert_eq!(parts.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    assert_eq!(parts[0].kind, "frame");
    assert_eq!(parts[0].name, "Test 5F frame");
    assert_eq!(parts[0].root, root.join("frames").parent().unwrap());
    assert_eq!(parts[0].values["frame.mass_kg"].as_float(), Some(0.65));
    assert_eq!(parts[0].provenance["frame.mass_kg"].source, ProvenanceSource::Estimated);
}

#[test]
fn the_first_root_that_has_an_id_wins() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("one");
    let second = dir.path().join("two");
    write(&first.join("frames"), "x.toml", FRAME);
    let heavier = FRAME.replace("mass_kg = 0.65", "mass_kg = 9.9").replace("0.65", "9.9");
    write(&second.join("frames"), "x.toml", &heavier);
    write(&second.join("frames"), "y.toml", FRAME);
    let parts = list(&[first.clone(), second.clone()], "frames");
    assert_eq!(parts.len(), 2);
    let x = parts.iter().find(|p| p.id == "x").unwrap();
    assert_eq!(x.values["frame.mass_kg"].as_float(), Some(0.65), "the first root's x wins");
    assert_eq!(x.root, first);
    assert_eq!(parts.iter().find(|p| p.id == "y").unwrap().root, second);
}

#[test]
fn a_broken_part_is_listed_with_its_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let missing = FRAME.replace("\n[provenance.\"ground.friction_coeff\"]\nsource = \"estimated\"\n", "\n");
    write(&root.join("frames"), "broken.toml", &missing);
    let parts = list(&[root], "frames");
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name, "");
    assert!(parts[0].error.as_deref().unwrap_or("").contains("ground.friction_coeff"), "{:?}", parts[0].error);
}

#[test]
fn a_missing_or_unknown_kind_directory_lists_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    write(&root.join("frames"), "a.toml", FRAME);
    assert!(list(&[root.clone()], "vtx").is_empty(), "no vtx directory");
    assert!(list(&[root], "wings").is_empty(), "no such kind");
}

#[test]
fn the_shipped_library_lists_its_motors() {
    let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../library"));
    let motors = list(&[root], "motors");
    let m = motors.iter().find(|p| p.id == "opendrone-openmotor-2207").unwrap();
    assert_eq!(m.values["motor.pole_pairs"].as_integer(), Some(7));
    assert!(m.provenance.contains_key("motor.kv_rpm_per_v"));
}
```

- [ ] **Step 2: Run** `cargo test -p ofs-config --test part list` — Expected: FAIL (`no function or associated item named list`).
- [ ] **Step 3: Implement** in `crates/ofs-config/src/part.rs`:

```rust
/// One part a `list` pass found: its id, what it is, where it came from. `error` is set (and `name` empty)
/// when the file does not load — a broken part is listed, not hidden.
#[derive(Debug, Clone)]
pub struct ListedPart {
    pub id: String,
    /// The part kind, singular ("frame", "motor", …) — sim.parts' vocabulary.
    pub kind: &'static str,
    /// The root the part came from (the first that has the id).
    pub root: PathBuf,
    pub name: String,
    /// Every data parameter by dotted path (tables recurse, arrays are single leaves — check_provenance's paths).
    pub values: BTreeMap<String, toml::Value>,
    pub provenance: BTreeMap<String, Provenance>,
    pub error: Option<String>,
}

type Listed = (String, BTreeMap<String, Provenance>, BTreeMap<String, toml::Value>);

/// Every part of `kind_dir` across `roots`, in root order — the first root that has an id wins, the loader's own
/// resolution rule — sorted by id. `list` is a catalog; `lib.rs` stays the only validator.
pub fn list(roots: &[PathBuf], kind_dir: &str) -> Vec<ListedPart> {
    let Some(load) = listed_loader(kind_dir) else { return Vec::new() };
    let mut seen: BTreeMap<String, ListedPart> = BTreeMap::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root.join(kind_dir)) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(id) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
            if path.extension().is_some_and(|x| x == "toml") && !seen.contains_key(&id) {
                seen.insert(id.clone(), listed(&path, &id, kind_of(kind_dir), root, load));
            }
        }
    }
    seen.into_values().collect()
}

fn listed(path: &Path, id: &str, kind: &'static str, root: &Path,
          load: fn(&Path, &str) -> Result<Listed, ConfigError>) -> ListedPart {
    let read = std::fs::read_to_string(path).map_err(|source| ConfigError::Io { path: path.to_path_buf(), source });
    match read.and_then(|text| load(path, &text)) {
        Ok((name, provenance, values)) =>
            ListedPart { id: id.into(), kind, root: root.into(), name, values, provenance, error: None },
        Err(e) => ListedPart {
            id: id.into(), kind, root: root.into(), name: String::new(),
            values: BTreeMap::new(), provenance: BTreeMap::new(), error: Some(e.to_string()),
        },
    }
}
```

plus, reusing `parse_part` and `check_provenance`: a `data_values(raw) -> BTreeMap<String, toml::Value>` walker (the `leaf_paths` shape — skip `schema_version`/`name`/`provenance`, tables recurse, other values are leaves keyed by dotted path), a macro `listed!(frame_listed, FramePart)` and its eight siblings (each: `parse_part::<T>`, `check_provenance` — problems return `ConfigError::Invalid` like the loaders — then `(name, provenance, data_values)`), `listed_loader(kind_dir) -> Option<fn…>` matching the nine directory names, and `kind_of(kind_dir) -> &'static str` (`"frames" => "frame"`, `"barometers" => "baro"`, `"magnetometers" => "mag"`, …). Add `use std::fs;` and `std::collections::BTreeMap` is already imported.

- [ ] **Step 4: Run** `cargo test -p ofs-config --locked` then `cargo test --workspace --locked --no-fail-fast` — Expected: PASS (nothing consumes `list` yet; the heavier test replaces only `0.65` occurrences inside `FRAME` consistently — re-verify the const when writing it).
- [ ] **Step 5: Commit** `feat(config): part::list - the library listing across roots, first root wins (#128)`.

### Task 2: The `OfsLibrary` bridge class (`ofs-godot`)

**Files:**
- Modify: `crates/ofs-godot/Cargo.toml` (add `ofs-config.workspace = true` to `[dependencies]`), `crates/ofs-godot/src/lib.rs`
- Create: `godot/tests/test_library.gd`; Modify: `godot/tests/run_tests.gd` (add `"res://tests/test_library.gd"` to `SUITES`)

**Interfaces:**
- Consumes: Task 1's `part::list`, `ListedPart`; `ofs_config::{library_roots, load_with_library_env}`.
- Produces: ClassDB class `OfsLibrary`, base `RefCounted`, instantiated with `ClassDB.instantiate("OfsLibrary")`:
  - `fn roots(env: GString, quad_dir: GString) -> PackedStringArray` — `ofs_config::library_roots(env trimmed empty → `None`, quad_dir)`; the empty env yields `<quad dir>/../library`.
  - `fn list(roots: PackedStringArray, kind_dir: GString) -> Array<Dictionary>` — one `{id: String, kind: String, name: String, root: String, error: String, parameters: Array}` per `ListedPart`; each parameter `{path: String, value: String, source: String, link: String, note: String}` sorted by path (`value` is `toml::Value`'s Display text; `source` is `datasheet|measured|estimated`).
  - `fn check_quad(path: GString, library_env: GString) -> Dictionary` — `Ok` → `{ok: true, name: String, auw_kg: f64, parts: Array of {kind, id, name}}` (`auw_kg = cfg.frame.mass_kg`, `parts` from `cfg.part_provenance`); `Err` → `{ok: false, error: String}` (the whole `ConfigError` text, problems and all).

- [ ] **Step 1: Write the failing tests** in `godot/tests/test_library.gd` (`extends "res://tests/testing.gd"`; the runner builds the extension, so `OfsLibrary` exists):

```gdscript
## `res://..` globalized, so the Rust side of the bridge gets an OS path (a `const` cannot call globalize_path).
static var REPO := ProjectSettings.globalize_path("res://..")

func test_the_class_exists() -> void:
	ok(ClassDB.class_exists("OfsLibrary"), "OfsLibrary is registered (cargo build -p ofs-godot)")

func test_roots_mirror_the_loader_rule() -> void:
	var lib: RefCounted = ClassDB.instantiate("OfsLibrary")
	var sep := ";" if OS.get_name() == "Windows" else ":"
	eq(lib.roots("", REPO.path_join("quads")), PackedStringArray([REPO.path_join("library")]), "empty env: the checkout's library")
	eq(lib.roots("C:/a" + sep + "C:/b", "C:/quads"), PackedStringArray(["C:/a", "C:/b"]), "env roots, in order")

func test_list_names_the_shipped_motors_with_provenance() -> void:
	var lib: RefCounted = ClassDB.instantiate("OfsLibrary")
	var parts: Array = lib.list(PackedStringArray([REPO.path_join("library")]), "motors")
	var m: Dictionary = parts.filter(func(p): return p["id"] == "opendrone-openmotor-2207")[0]
	eq(m["kind"], "motor", "the singular kind")
	var kv: Dictionary = m["parameters"].filter(func(p): return p["path"] == "motor.kv_rpm_per_v")[0]
	eq(kv["source"], "estimated", "the provenance source travels")
	eq(kv["value"], "1950.0", "the value travels as TOML text")

func test_list_marks_a_broken_part() -> void:
	var dir := "user://library-test"
	DirAccess.make_dir_recursive_absolute(ProjectSettings.globalize_path(dir).path_join("frames"))
	var bad := FileAccess.get_file_as_string(REPO.path_join("library/frames/opendrone-5f.toml"))
	bad = bad.replace("[provenance.\"frame.mass_kg\"]\nsource = \"estimated\"\nnote = \"M1 estimate\"\n", "")
	FileAccess.open("%s/frames/broken.toml" % dir, FileAccess.WRITE).store_string(bad)
	var lib: RefCounted = ClassDB.instantiate("OfsLibrary")
	var parts: Array = lib.list(PackedStringArray([ProjectSettings.globalize_path(dir)]), "frames")
	eq(parts.size(), 1, "the broken frame is listed, not hidden")
	ok(String(parts[0]["error"]).contains("frame.mass_kg"), "with the loader's message: %s" % parts[0]["error"])

func test_check_quad_loads_the_shipped_quad() -> void:
	var lib: RefCounted = ClassDB.instantiate("OfsLibrary")
	var verdict: Dictionary = lib.check_quad(REPO.path_join("quads/opendrone-5f-freestyle.toml"), "")
	eq(verdict["ok"], true, "the freestyle quad loads through its own library root")
	eq(verdict["name"], "OpenDrone 5F freestyle", "the quad's name")
	near(verdict["auw_kg"], 0.65, "the frame part's mass is the AUW")
	var kinds: Array = verdict["parts"].map(func(p): return p["kind"])
	for expected in ["frame", "motor", "battery", "vtx"]:
		ok(kinds.has(expected), "%s is among the parts" % expected)
```

(An `OfsLibrary` is a `RefCounted`; never call `free()` on it. Add a cleanup that removes `user://library-test` before and after, the `test_main_menu.gd` `_remove` pattern.)

- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: FAIL (`test_library.gd` errors — class unknown).
- [ ] **Step 3: Implement**: the Cargo.toml dependency; in `lib.rs`, beside `OfsClient`:

```rust
/// The component library the workbench lists: roots, the parts of a kind, and the loader's verdict on a quad
/// file — the same code the simulator runs at Load. Stateless; instantiate per use.
#[derive(GodotClass)]
#[class(base = RefCounted)]
struct OfsLibrary {}
```

with `#[func]` wrappers converting types (`VarDictionary` for the dictionaries, `Array<VarDictionary>` for the lists; `GString` empty → `None` for the env parameters). Match the file's existing wrapper style around `parse_settings` (`crates/ofs-godot/src/lib.rs:578`). The `check_quad` `Err` arm maps `ofs_config::ConfigError` with `e.to_string()`.
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` and `cargo clippy --workspace --all-targets --locked -- -D warnings` — Expected: PASS, no `SCRIPT ERROR` in the output.
- [ ] **Step 5: Commit** `feat(godot): the OfsLibrary bridge - roots, the part listing and check_quad (#128)`.

### Task 3: The workbench model, provenance rendering, and the launch `OFS_LIBRARY`

**Files:**
- Create: `godot/scripts/workbench.gd`, `godot/scripts/provenance_view.gd`
- Modify: `godot/scripts/settings.gd` (`to_client_dict`), `godot/tests/run_tests.gd` (two new suites), `godot/tests/test_settings.gd`
- Create: `godot/tests/test_workbench.gd`, `godot/tests/test_provenance_view.gd`

**Interfaces:**
- Consumes: Task 2's `OfsLibrary`; `AppSettings.repo_root()`; the menu's quad listing inputs (`Catalog` in Task 4 — not needed here).
- Produces (`scripts/workbench.gd`, `RefCounted`; every function except the constructor-callable `OfsLibrary` uses is `static` or pure state, so tests drive it without a scene):
  - `const USER_QUADS := "user://quads"`; `static func user_quads_dir() -> String` (globalized, created); `static func stem(name: String) -> String` (Design call 8).
  - `static func library_roots(env: String, repo_quads: String) -> PackedStringArray` (the `OfsLibrary.roots` call; `env` is passed in, never read from `OS` inside, for testability); `static func joined(roots: PackedStringArray) -> String` (`";"` on Windows, `":"` else).
  - `static func refs_of(text: String) -> Dictionary` — the tolerant line parse → `{name, frame, motors, props, escs, battery, vtx}` (`motors`/`props`/`escs` the first id of the list; `battery`/`vtx` the `part` inside their table; missing keys absent).
  - `static func patched(template: String, picks: Dictionary, name: String, stem: String) -> String` — the line patch of Design call 4.
  - Instance: `var name := ""`, `var picks := {}`, `func start_from(quad_path: String) -> void` (reads the template, seeds `picks` from `refs_of`, remembers the template's diff path), `func save() -> Dictionary` (writes `<user_quads_dir>/<stem>.toml`, copies the template's diff to `<stem>.betaflight.diff`, returns the `check` verdict), `func check(path: String) -> Dictionary` (the `OfsLibrary.check_quad` call with `joined(library_roots(...))`).
- Produces (`scripts/provenance_view.gd`, all `static`): `source_mark(source) -> String` (`"estimated"` → `"~ estimated"`, others unchanged), `parameter_line(p) -> String` — `"<path> = <value>  [<source_mark>]  (<note>; <link>)"`, the parenthesised part only when the note, the link or both are non-empty, both joined with `"; "` — `part_title(part) -> String`, `details(part) -> Array[String]` (sorted by path), `summary_line(kind, part) -> String` (frame: `"AUW 650 g"` from `frame.mass_kg`; motor: `"<kv> KV"`; prop: `"<inches> in"` from `prop.diameter_m`; esc: `"<limit> A"` from `esc.current_limit_a`; battery: `"<cells>S <mah> mAh"`; vtx: the `power_levels_mw` values joined `"/"` + `" mW"` — all read from the part's `parameters` array).

- [ ] **Step 1: Write the failing tests.** In `test_provenance_view.gd` (pure dictionaries, no engine):

```gdscript
const View = preload("res://scripts/provenance_view.gd")
const PART := {"id": "m", "kind": "motor", "name": "Test motor", "error": "", "parameters": [
	{"path": "motor.kv_rpm_per_v", "value": "1950.0", "source": "estimated", "link": "", "note": "M1 estimate"},
	{"path": "motor.pole_pairs", "value": "7", "source": "datasheet", "link": "https://x", "note": ""},
]}

func test_estimates_are_marked() -> void:
	eq(View.source_mark("estimated"), "~ estimated", "the marker")
	eq(View.source_mark("datasheet"), "datasheet", "unchanged")
	ok(View.parameter_line(PART["parameters"][0]).contains("[~ estimated]"), "in a parameter line")

func test_parameter_lines_carry_note_and_link_only_when_present() -> void:
	var with_note: String = View.parameter_line(PART["parameters"][0])
	ok(with_note.contains("(M1 estimate)") and not with_note.contains("https"), with_note)
	var with_link: String = View.parameter_line(PART["parameters"][1])
	ok(with_link.contains("https://x") and not with_link.contains("()"), with_link)

func test_details_are_sorted_and_complete() -> void:
	eq(View.details(PART), ["motor.kv_rpm_per_v = 1950.0  [~ estimated]  (M1 estimate)",
		"motor.pole_pairs = 7  [datasheet]  (https://x)"], "one line per parameter, by path")

func test_summary_lines_read_the_parameters() -> void:
	eq(View.summary_line("motor", PART), "1950 KV")
	eq(View.summary_line("frame", {"parameters": [{"path": "frame.mass_kg", "value": "0.105"}]}), "AUW 105 g")
	eq(View.summary_line("battery", {"parameters": [{"path": "battery.cells", "value": "3"}, {"path": "battery.capacity_mah", "value": "450.0"}]}), "3S 450 mAh")
	eq(View.summary_line("vtx", {"parameters": [{"path": "vtx.power_levels_mw", "value": "[25, 200, 600, 1000]"}]}), "25/200/600/1000 mW")

func test_a_broken_part_titles_with_its_error() -> void:
	ok(View.part_title({"name": "", "id": "x", "error": "x.toml: 1 problem(s):"}).contains("x.toml"), "the error is the title")
```

In `test_workbench.gd` (reads the checkout's quad and library; cleans `user://quads/<stem>*` around saves, the `_remove` pattern):

```gdscript
const Workbench = preload("res://scripts/workbench.gd")
static var QUAD := ProjectSettings.globalize_path("res://../quads/opendrone-5f-freestyle.toml")

func test_refs_of_reads_the_six_picks() -> void:
	var refs: Dictionary = Workbench.refs_of(FileAccess.get_file_as_string(QUAD))
	eq(refs["name"], "OpenDrone 5F freestyle", "the name")
	eq(refs["frame"], "opendrone-5f")
	eq(refs["motors"], "opendrone-openmotor-2207")
	eq(refs["battery"], "opendrone-6s-1300")
	eq(refs["vtx"], "opendrone-smartaudio-1w")

func test_patched_rewrites_the_picks_and_the_diff_name() -> void:
	var picks := Workbench.refs_of(FileAccess.get_file_as_string(QUAD))
	picks["battery"] = "opendrone-6s-4000"
	var out: String = Workbench.patched(FileAccess.get_file_as_string(QUAD), picks, "My 3T", "My-3T")
	var round: Dictionary = Workbench.refs_of(out)
	eq(round["battery"], "opendrone-6s-4000", "the swap stuck")
	eq(round["name"], "My 3T", "the name stuck")
	ok(out.contains("betaflight_diff = \"My-3T.betaflight.diff\""), "the diff is re-pointed at the copy")

func test_save_writes_the_assembly_and_its_diff_and_reports_the_verdict() -> void:
	var wb := Workbench.new()
	wb.start_from(QUAD)
	wb.name = "Workbench save test"
	var verdict: Dictionary = wb.save()
	eq(verdict["ok"], true, "the same parts still load")
	near(verdict["auw_kg"], 0.65, "the AUW is the frame part's mass")
	var path: String = ProjectSettings.globalize_path("user://quads/Workbench-save-test.toml")
	ok(FileAccess.file_exists(path) and FileAccess.file_exists(path.trim_suffix(".toml") + ".betaflight.diff"), "both files exist")

func test_save_surfaces_the_loaders_refusal_verbatim() -> void:
	var wb := Workbench.new()
	wb.start_from(QUAD)
	wb.picks["battery"] = "opendrone-3s-450"  # the freestyle diff sets force_battery_cell_count = 6
	wb.name = "Workbench refusal test"
	var verdict: Dictionary = wb.save()
	eq(verdict["ok"], false, "a 3S pack against the 6S diff is refused")
	ok(String(verdict["error"]).contains("force_battery_cell_count"), "the loader's own words: %s" % verdict["error"])
	ok(FileAccess.file_exists(ProjectSettings.globalize_path("user://quads/Workbench-refusal-test.toml")), "the file is kept")

func test_reopening_reads_picks_from_a_refused_assembly() -> void:
	eq(Workbench.refs_of(FileAccess.get_file_as_string(ProjectSettings.globalize_path("user://quads/Workbench-refusal-test.toml")))["battery"], "opendrone-3s-450", "the tolerant parse never validates")

func test_the_stem_rule() -> void:
	eq(Workbench.stem("My: Fancy 3T?"), "My-Fancy-3T", "illegal characters dropped, runs of whitespace to a dash")
	eq(Workbench.stem("  "), "assembly", "never empty")
```

And in `test_settings.gd`:

```gdscript
func test_to_client_dict_passes_the_library_roots_to_the_server_it_starts() -> void:
	var s := AppSettings.defaults()
	var d: Dictionary = AppSettings.to_client_dict(s)
	ok(String(d["env"].get("OFS_LIBRARY", "")).contains("library"), "the roots travel: %s" % str(d.get("env")))
	eq(d["env"].get("OFS_LIBRARY", ""), AppSettings.to_client_dict(AppSettings.defaults())["env"]["OFS_LIBRARY"], "deterministic")
```

- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: FAIL on the new suites (`preload` of missing scripts is a parse error).
- [ ] **Step 3: Implement** the two scripts and the `settings.gd` change. In `to_client_dict` (verified at `godot/scripts/settings.gd:209-226`), inside the `if s["server_bin"] != "":` block, build the env dict with both keys:

```gdscript
	var launch_env := {}
	if s["sitl_launch"] != "":
		launch_env["OFS_SITL_LAUNCH"] = s["sitl_launch"]
	launch_env["OFS_LIBRARY"] = Workbench.joined(Workbench.library_roots(OS.get_environment("OFS_LIBRARY"), repo_root().path_join("quads")))
	d["env"] = launch_env
```

(`settings.gd` gains `const Workbench = preload(...)`; `workbench.gd` must NOT preload `settings.gd` — take `repo_quads` as a parameter, as the signature above does, to keep the preload graph acyclic. `patched` walks lines tracking the current `[table]` the way `settings.gd`'s `with_value` does; it replaces an existing key line in place and writes nothing when the template lacks the key — the verdict then refuses, which is the honest outcome. `save()` reads the template's diff with `FileAccess.get_file_as_string(template_dir + betaflight_diff_line)`; when the template names no diff that exists, no diff is written and the verdict says so. Re-verify at execution whether `test_settings.gd` already pins `to_client_dict`'s env dict — if it does, extend it; the key set grows by design.)

- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit`, `cargo test --workspace --locked --no-fail-fast` — Expected: PASS (the settings test is the only Rust-adjacent change; nothing else consumes `to_client_dict`'s new env key — the bridge already forwards arbitrary string→string env, `crates/ofs-godot/src/lib.rs:594-599`).
- [ ] **Step 5: Commit** `feat(godot): the workbench model - assemblies, provenance rendering and the launch library env (#128)`.

### Task 4: The menu lists workbench assemblies and gains the Workbench button

**Files:**
- Modify: `godot/scripts/catalog.gd`, `godot/ui/main_menu.gd`
- Test: `godot/tests/test_catalog.gd`, `godot/tests/test_main_menu.gd`

**Interfaces:**
- Produces: `Catalog.USER_QUADS: String` (the globalized `user://quads`) and the third entry of `Catalog.roots(settings, "quads")` — worlds roots unchanged; `MainMenu` signal `workbench_requested`, button `_workbench_button` reachable as `row("workbench")` between Settings and Quit.

- [ ] **Step 1: Write the failing tests.** In `test_catalog.gd`:

```gdscript
func test_quad_roots_include_the_users_quads_dir() -> void:
	var roots: Array = Catalog.roots(AppSettings.defaults(), "quads")
	eq(roots[roots.size() - 1], ProjectSettings.globalize_path("user://quads"), "the workbench's dir, globalized, last")

func test_world_roots_do_not() -> void:
	eq(Catalog.roots(AppSettings.defaults(), "worlds").size(), 2, "worlds are another task's ground")

func test_the_users_quads_are_listed_and_a_missing_dir_is_skipped() -> void:
	var user := ProjectSettings.globalize_path("user://quads")
	DirAccess.make_dir_recursive_absolute(user)
	FileAccess.open(user.path_join("zz-mine.toml"), FileAccess.WRITE).store_string("name = \"Zz mine\"\n")
	var listed: Array = Catalog.list(Catalog.roots(AppSettings.defaults(), "quads"))
	ok(listed.any(func(e): return e["label"] == "Zz mine"), "listed with its label")
	DirAccess.remove_absolute(user.path_join("zz-mine.toml"))
```

In `test_main_menu.gd`:

```gdscript
func test_the_menu_has_a_workbench_button() -> void:
	var m := await _menu()
	var menu = m[0]
	var events: Array = m[2]
	menu.workbench_requested.connect(func(): events.append(["workbench"]))
	menu.row("workbench").pressed.emit()
	eq(events.back(), ["workbench"], "the button announces the workbench")
```

- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` — Expected: FAIL (`USER_QUADS` unknown; `row("workbench")` returns null).
- [ ] **Step 3: Implement.** `catalog.gd` (`roots` verified at `godot/scripts/catalog.gd:8-9`):

```gdscript
const USER_QUADS := ProjectSettings.globalize_path("user://quads")

static func roots(settings: Dictionary, kind: String) -> Array:
	var dirs := [AppSettings.repo_root().path_join(kind), String(settings["data_dir"]).path_join(kind)]
	if kind == "quads":
		dirs.append(USER_QUADS)  # the workbench's assemblies (#128); user worlds are another task's
	return dirs
```

`main_menu.gd`: add `signal workbench_requested`, a `_workbench_button` (`text = "Workbench"`, `pressed` → emit) placed after the Settings button in the `_ready` node list and in `row()`'s dictionary (the buttons block verified at `godot/ui/main_menu.gd:56-65`, `row` at `:110-112`). Re-verify at execution whether `test_roots_are_the_checkout_and_the_data_dir` (`godot/tests/test_catalog.gd:65`) pins the quads roots to two entries — if it does, extend it to the three the quads now have (worlds stay two); the Step 1 tests then cover both.
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` — Expected: PASS, including every pre-existing menu and catalog test unchanged.
- [ ] **Step 5: Commit** `feat(godot): the menu lists workbench assemblies and gains a Workbench button (#128)`.

### Task 5: The workbench screen

**Files:**
- Create: `godot/ui/workbench_screen.gd`
- Modify: `godot/main.tscn` (a `WorkbenchScreen` `CanvasLayer`, `layer = 8`, after `SettingsScreen` — layers verified at `godot/main.tscn:80-85`), `godot/scripts/app.gd`, `godot/tests/run_tests.gd`
- Create: `godot/tests/test_workbench_screen.gd`

**Interfaces:**
- Consumes: Task 3's model and renderer, Task 4's menu signal, the `CycleButton`/screen patterns of `ui/settings_screen.gd`.
- Produces: `WorkbenchScreen` (`CanvasLayer`): `signal closed`; `setup(settings: Dictionary)`; `open()` / `close()` / `is_open() -> bool`; `row(name: String) -> Control` for `{"start", "frame", "motors", "props", "escs", "battery", "vtx", "name", "save", "back"}` (the #157 seam: rows addressable by kind, DC8); `select_part(kind: String)`; `summary_text() -> String`; `verdict_text() -> String`; `details_text() -> String` (test handles).
- Rows: a starting-quad `CycleButton` over `Catalog.with_current(Catalog.list(Catalog.roots(settings, "quads")), settings["quad_path"])` (changing it calls `start_from`, resetting the picks); one `CycleButton` per kind over the workbench listing (`"<name> (<id>)"` labels, id values); a name `LineEdit`; Save and Back. The details panel shows `select_part`'s part (every parameter, estimates marked); the summary shows AUW and one `summary_line` per pick; the verdict area shows the loader's refusal verbatim or "Saved to <path>" plus the summary. Esc and Back emit `closed`.

- [ ] **Step 1: Write the failing tests** in `test_workbench_screen.gd` (cleans `user://quads/Screen-test*` around saves):

```gdscript
const Screen = preload("res://ui/workbench_screen.gd")
static var REPO := ProjectSettings.globalize_path("res://..")

func _screen() -> Array:
	var settings := AppSettings.defaults()
	settings["quad_path"] = REPO.path_join("quads/opendrone-5f-freestyle.toml")
	var screen = Screen.new()
	await add_to_tree(screen)
	screen.setup(settings)
	screen.open()
	return [screen, settings]

func test_the_rows_list_the_library_and_the_summary_knows_the_auw() -> void:
	var s = await _screen()
	var labels: Array = s[0].row("frame").values
	ok(labels.any(func(l): return String(l).contains("OpenDrone 5F frame")), "the frame row lists the library: %s" % str(labels))
	ok(s[0].summary_text().contains("650 g"), "the freestyle AUW: %s" % s[0].summary_text())

func test_estimates_are_marked_in_the_details() -> void:
	var s = await _screen()
	s[0].select_part("motors")
	ok(s[0].details_text().contains("motor.kv_rpm_per_v"), "the parameters show")
	ok(s[0].details_text().contains("[~ estimated]"), "estimates are marked")

func test_save_reports_the_verdict_and_a_clash_shows_the_loaders_words() -> void:
	var s = await _screen()
	var screen = s[0]
	screen.name_edit().text = "Screen test"
	screen.save_pressed()  # the Save button's handler, so the test takes the UI path
	ok(screen.verdict_text().contains("Saved"), "a same-parts assembly saves clean: %s" % screen.verdict_text())
	screen.row("battery").set_value("opendrone-3s-450")
	screen.save_pressed()
	ok(screen.verdict_text().contains("force_battery_cell_count"), "the loader's refusal, verbatim: %s" % screen.verdict_text())

func test_back_and_esc_close() -> void:
	var s = await _screen()
	var closed := []
	s[0].closed.connect(func(): closed.append(true))
	s[0].row("back").pressed.emit()
	ok(closed.size() == 1, "Back closes")
```

(Expose `name_edit() -> LineEdit` and `save_pressed()` for the tests, the `row()` pattern's spirit.)
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` — Expected: FAIL (missing script).
- [ ] **Step 3: Implement** the screen (a dim `ColorRect` and a centered `PanelContainer`, the `settings_screen.gd` look; focus order start → frame → … → save). In `app.gd` (menu wiring verified at `godot/scripts/app.gd:74-83`): add `@onready var workbench_screen: CanvasLayer = $WorkbenchScreen`, `workbench_screen.setup(settings)`, `workbench_screen.closed.connect(main_menu.open)` and `main_menu.workbench_requested.connect(func() -> void: main_menu.close(); workbench_screen.open())`.
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` — Expected: PASS. Then `bash scripts/run-godot-tests.sh open-loop` — Expected: PASS (the menu-less boot and the existing menu round trip are untouched).
- [ ] **Step 5: Commit** `feat(godot): the workbench screen (#128)`.

### Task 6: The round-trip e2e — assemble, save, load from the menu, fly

**Files:**
- Modify: `godot/tests/e2e_open_loop.gd` (a new section at the end of `_run`, after the existing menu round trip — re-verify at execution where that section ends; the harness and `_check`/`_wait_for` helpers are at the file's top)

**Interfaces:**
- Consumes: Tasks 3–5; the existing e2e harness (`OFS_MENU=off` boot, then the menu exercised at the end, `e2e_open_loop.gd:84-90`); `app.client.get_quad_name()`.

- [ ] **Step 1: Add the section** (cleanup `user://quads/E2E-assembly*` first, the `_remove` pattern):

```gdscript
	# #128: the workbench round trip — assemble from the menu's quad, save, and fly the assembly from the menu.
	_check(app.main_menu.is_open() or app.settings["menu"], "the menu is up for the workbench")
	app.main_menu.row("workbench").pressed.emit()
	_check(app.workbench_screen.is_open(), "the workbench opens from the menu")
	app.workbench_screen.name_edit().text = "E2E assembly"
	app.workbench_screen.row("props").set_value("opendrone-5in-race-prop")
	app.workbench_screen.save_pressed()
	_check(app.workbench_screen.verdict_text().contains("Saved"), "the assembly saves clean: %s" % app.workbench_screen.verdict_text())
	app.workbench_screen.row("back").pressed.emit()
	var assembly := ProjectSettings.globalize_path("user://quads/E2E-assembly.toml")
	_check(FileAccess.file_exists(assembly), "the assembly landed in user://quads")
	var quads: Array = app.main_menu.row("quad").values
	_check(quads.has(assembly), "the menu lists the assembly: %s" % str(quads))
	app.main_menu.row("quad").set_value(assembly)
	app.main_menu.start_requested.emit()
	var flying: bool = await _wait_for(func(): return app.client.get_phase() == "flying", 30.0)
	_check(flying, "Start flies the assembly (phase: %s)" % app.client.get_phase())
	_check(app.client.get_quad_name() == "E2E assembly", "it is the assembly that flies: %s" % app.client.get_quad_name())
	DirAccess.remove_absolute(assembly)
	DirAccess.remove_absolute(assembly.trim_suffix(".toml") + ".betaflight.diff")  # the dev's user://quads stays clean
```

- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh open-loop` (WSL; needs the built server and extension) — Expected: PASS. This is the proof of Design call 5: without the launch `OFS_LIBRARY`, a `user://quads` assembly would be refused with "part not found in any library root" (its own default root `<dir>/../library` does not exist) — if the flight fails with that message, the env seam of Task 3 broke.
- [ ] **Step 3: Run** `bash scripts/run-godot-tests.sh all` — Expected: PASS (betaflight suite skips without `OFS_SITL_LAUNCH`).
- [ ] **Step 4: Commit** `test(godot): the workbench round trip - assemble, save, menu, fly (#128)`.

### Task 7: Docs and gates

**Files:**
- Modify: `README.md` (a "The workbench" paragraph beside "Component library": assembling without a text editor, `user://quads/`, the `OFS_LIBRARY` launch behaviour and its external-server limit), `library/README.md` (one line: the workbench assembles quads over these parts), `docs/architecture.md` (the Godot file list gains `workbench.gd`/`workbench_screen.gd`; `ofs-godot`'s row mentions `OfsLibrary` — its table is at `docs/architecture.md:29`), `CHANGELOG.md` (`Unreleased` — Added: the drone workbench). Check `docs/roadmap.md` for a workbench backlog line and remove it if present.
- [ ] **Step 1: Write the docs** — plain words, no new claims beyond this plan; the README paragraph names the file layout (`user://quads/<stem>.toml` + `.betaflight.diff`) and that the menu lists the directory.
- [ ] **Step 2: Run every gate**: `cargo test --workspace --locked --no-fail-fast`; `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo +1.85 check -p ofs-config`; WSL `bash scripts/run-godot-tests.sh all`; `PYTHONPATH=python python -m pytest python/tests -q` (Python is untouched; it must stay green). Expected: all PASS.
- [ ] **Step 3: List the by-eye checks** (spec's "Checked by eye") in the PR description for #163: the screen's look and focus navigation, gamepad navigation across the rows, the `~`-marked estimate at a glance, an assembly flying with Betaflight.
- [ ] **Step 4: Commit** `docs: the drone workbench (#128)`.

## Ledger

- Ruled 2026-10-31 (against `main` at `87c5914`): #128's "a schema-5 quad TOML" is the issue's age, not a requirement — the workbench writes the starting quad's `schema_version` verbatim (schemas 6/7 today; `SCHEMA_VERSION = 7`, `crates/ofs-config/src/lib.rs:20`).
- Ruled 2026-10-31: `motor_poles` cross-checks never fire with shipped parts (every `library/motors/*.toml` has `pole_pairs = 7`, Betaflight's default 14); the live refusal is `force_battery_cell_count` (the freestyle diff sets 6 at `quads/opendrone-5f-freestyle.betaflight.diff:20`). The workbench still surfaces whatever the loader says — never its own version of either.
- None further; the executor records every deviation from the design calls here.
