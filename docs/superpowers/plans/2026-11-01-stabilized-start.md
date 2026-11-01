# The Stabilized Start Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** With the open-loop setting on, starting a quad file whose `[fc] kind = "open_loop_stabilized"` from the menu (or a menu-less boot) flies it stabilized instead of being refused at Load. Closes #172.

**Architecture:** The wire already carries `stabilize` (`LoadRequest` field 8, protocol 12) and the Rust client already sends it — only the Godot bridge drops it. The bridge grows a `stabilize` key; a Catalog-style line-scan reads the chosen quad's `[fc] kind`; `_start_session` derives `stabilize: true` when the merged session is open loop **and** the quad scans stabilized — one decision point both start paths flow through. No simulator, proto, Python or settings-chain change.

**Tech Stack:** Rust workspace (MSRV 1.85; only `ofs-godot` changes), Godot 4.7.2 GDScript through the gdext bridge.

**Spec:** `docs/superpowers/specs/2026-11-01-stabilized-start-design.md` (calls D1–D7 are the binding rules; anchors verified at `main` = `fdc4917` on branch `docs/platform-followups-plan` — re-verify at execution, the Consequence wave may have moved lines).

## Global Constraints

- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked` green; `cargo +1.85 check --workspace --all-targets --locked --exclude ofs-godot` passes (the crate pins `rust-version = "1.94"` and the msrv job skips it; stable clippy and test cover it).
- No golden digest changes (`crates/ofs-radio/tests/golden/*`, `crates/ofs-sim/tests/golden/*`): nothing outside `crates/ofs-godot/src/lib.rs` changes on the Rust side.
- No protocol bump: `LoadRequest.stabilize` is field 8, on the wire since protocol 12 (`proto/ofs/v1/sim.proto:101-103`). Whatever `PROTOCOL_VERSION` is at execution (16 at planning; the Consequence wave's pairs bump it), this pair must not move it.
- No settings-chain fork: `stabilize` appears in no `defaults()`, `ENV_NAMES`, `FLAG_NAMES`, menu row or `ofs_client.cfg` write — it is a per-start derived key only (spec D4).
- The menu-less e2e path stays green untouched: `godot/tests/e2e_betaflight.gd`, `e2e_errors.gd`, `e2e_terrain.gd` and `shots.gd` need no change to pass.
- `cargo fmt` is not a gate; do not reformat existing files.
- Worktree-isolated sessions refuse heredocs and WSL through Bash: use Write/Edit for files and PowerShell for WSL; Godot tests run as `bash scripts/run-godot-tests.sh <suite>` in WSL (it downloads Godot 4.7.2 and builds `ofs-sim` + `ofs-godot` itself).
- Python tests need `PYTHONPATH=<worktree>/python` only if Task 4's full gate is run on a tree where python tests are otherwise exercised; this pair changes nothing under `python/`.
- Commit trailers: none prescribed; an executing session that adds one names its own identity.

## Design calls (decided here from the spec; the executor ledgers any change)

1. **Shape (spec D1):** the bridge grows a `stabilize` key and Start asks for the stabilizer — not `open_loop: false` for stabilized quads. Price: a three-line Rust change and a bridge rebuild; the reward is a `LoadRequest` that says what flew and Python parity.
2. **Learning the kind (spec D2):** `Catalog.fc_kind(path)` / `Catalog.quad_is_stabilized(path)` line-scan in `catalog.gd` (the `entry()` idioms) — independent of the quad-model plan's unexecuted `quad_spec.gd`.
3. **One decision point (spec D3):** the derivation sits in `_start_session` after the choices merge; `effective_open_loop` stays the menu's rule alone, and the menu-less boot's `open_loop` semantics are untouched.
4. **Derived, never a setting (spec D4);** the server, proto and Python are untouched (spec D5); the proof is the open-loop e2e with a temp stabilized quad written into the e2e data dir (spec D6); `docs/scripted-flight.md` stops describing an exception (spec D7).

## Review Focus

- A quad file whose `[fc] kind = "open_loop_stabilized"` under `OFS_OPEN_LOOP=1`: Start flies it stabilized (Task 3's e2e — the failing run on main shows the refusal).
- A commented `# kind = …` line, a `kind` outside `[fc]`, or a `kindred =` key must not read stabilized — a sitl quad flown open loop stays throttle-only (Task 2's tests).
- A missing or unreadable quad file returns `""` — never stabilized — so the load fails with the usual config error, not a stabilize surprise (Task 2's test).
- `stabilize` never rides without `open_loop` (the server refuses it, `crates/ofs-sim/src/server.rs:301-302`): the derivation gates on the merged session's `open_loop`, and `to_client_dict` defaults false (Task 3's unit test and e2e).
- A Betaflight pick (`open_loop` false) of a stabilized quad sends exactly what it sends today — no override, the file's own kind flies — because the derivation is dormant when `open_loop` is false (Task 3's code path; the unchanged e2e suites cover the rest).

---

### Task 1: The bridge's `stabilize` key (ofs-godot)

**Files:**
- Modify: `crates/ofs-godot/src/lib.rs` (the `start` doc comment, lines 97–101; `parse_settings`, lines 606–653)
- Test: `godot/tests/test_extension.gd`

**Interfaces:**
- Consumes: the Rust client's existing `Settings.stabilize` (`crates/ofs-client/src/model.rs:788-791`, default false) which the worker already sends in every `LoadRequest` (`crates/ofs-client/src/worker.rs:274-283`).
- Produces: `OfsClient.start` accepts a `stabilize` bool (default false), documented beside `open_loop`; Task 3's GDScript relies on the key crossing.

- [ ] **Step 1: Write the test** in `godot/tests/test_extension.gd` (after `test_invalid_settings_are_reported_as_text`, line 63):

```gdscript
func test_a_stabilized_start_is_valid_settings() -> void:
	var client := await _client()
	var error: String = client.start({"quad_path": QUAD, "server_addr": "127.0.0.1:1", "open_loop": true, "stabilize": true})
	eq(error, "", "open_loop with stabilize parses (the wire proof is e2e_open_loop.gd's stabilized start)")
	eq(client.start({"quad_path": QUAD}), "already started", "and it really started")
	client.stop()
	client.queue_free()
```

  This pins the parse contract; it cannot fail before the change (an unknown dictionary key is silently ignored today — that silence is the bug), and the plan says so honestly. The wire-level proof is Task 3's e2e.
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` in WSL — Expected: PASS (the new test passes against the old bridge; it is a contract pin, not the fix).
- [ ] **Step 3: Implement** in `crates/ofs-godot/src/lib.rs`. In `parse_settings`, directly under the `open_loop` mapping (lines 635–637), add:

```rust
    if let Some(stabilize) = d.get("stabilize").and_then(|v| v.try_to::<bool>().ok()) {
        settings.stabilize = stabilize;
    }
```

  And in the `start` doc comment (line 100), the key list "`seed`, `open_loop`, `overrun_policy`" becomes "`seed`, `open_loop`, `stabilize` (with `open_loop`: the built-in angle-mode stabilizer), `overrun_policy`".
- [ ] **Step 4: Run** `cargo clippy -p ofs-godot --all-targets --locked -- -D warnings`, `cargo test -p ofs-godot --locked`, and `bash scripts/run-godot-tests.sh unit` in WSL (the runner rebuilds the extension) — Expected: all PASS.
- [ ] **Step 5: Commit** `feat(godot): the bridge accepts a stabilize key (#172)`.

### Task 2: The quad's fc kind in the catalog scanner

**Files:**
- Modify: `godot/scripts/catalog.gd`
- Test: `godot/tests/test_catalog.gd`

**Interfaces:**
- Produces (Task 3 consumes): `static func fc_kind(path: String) -> String` — the `[fc] kind` value (`"sitl"`, `"open_loop"`, `"open_loop_stabilized"`; `""` when the file has no `[fc]` or cannot be read); `static func quad_is_stabilized(path: String) -> bool` — `fc_kind(path) == "open_loop_stabilized"`.

- [ ] **Step 1: Write the failing tests** in `godot/tests/test_catalog.gd`:

```gdscript
func test_the_fc_kind_of_a_quad_file_is_scanned() -> void:
	_fresh()
	_write(DIR + "/q/sitl.toml", "name = \"A\"\n[fc]\nkind = \"sitl\"\n")
	_write(DIR + "/q/stab.toml", "name = \"B\"\n[fc]\nkind = \"open_loop_stabilized\"\n# kind = \"open_loop_stabilized\"\n")
	_write(DIR + "/q/outside.toml", "kind = \"open_loop_stabilized\"\n[fc]\nkind = \"sitl\"\n")
	_write(DIR + "/q/kindred.toml", "[fc]\nkindred = \"open_loop_stabilized\"\nkind = \"open_loop\"\n")
	_write(DIR + "/w/field.toml", "name = \"Field\"\n[[gates]]\nname = \"G0\"\n")
	eq(Catalog.fc_kind(DIR + "/q/sitl.toml"), "sitl", "a sitl quad")
	eq(Catalog.fc_kind(DIR + "/q/stab.toml"), "open_loop_stabilized", "a stabilized quad, comments ignored")
	eq(Catalog.fc_kind(DIR + "/q/outside.toml"), "sitl", "a kind outside [fc] does not count")
	eq(Catalog.fc_kind(DIR + "/q/kindred.toml"), "open_loop", "only a key named kind counts")
	eq(Catalog.fc_kind(DIR + "/w/field.toml"), "", "a world file has no [fc]")
	eq(Catalog.fc_kind(DIR + "/q/missing.toml"), "", "a missing file has no kind")
	ok(Catalog.quad_is_stabilized(DIR + "/q/stab.toml"), "the predicate names the stabilized file")
	ok(not Catalog.quad_is_stabilized(DIR + "/q/missing.toml"), "and nothing else")
	_fresh()
```

- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` in WSL — Expected: FAIL (`fc_kind` does not exist).
- [ ] **Step 3: Implement** in `godot/scripts/catalog.gd`, after `entry()` (line 68), using `entry()`'s own scanning idioms:

```gdscript
## The quad file's `[fc] kind` ("sitl", "open_loop", "open_loop_stabilized"; "" without a `[fc]` or when the file
## cannot be read — never stabilized, so a broken file still fails at Load with the server's config error).
static func fc_kind(path: String) -> String:
	if not FileAccess.file_exists(path):
		return ""
	var kind := ""
	var table := ""
	for line in FileAccess.get_file_as_string(path).split("\n"):
		var t := line.strip_edges()
		if t.begins_with("["):
			table = t
		elif table == "[fc]" and t.begins_with("kind") and t.contains("=") and t.split("=", true, 1)[0].strip_edges() == "kind":
			kind = t.split("=", true, 1)[1].strip_edges().trim_prefix("\"").get_slice("\"", 0)
	return kind


## Whether the quad file at `path` carries its own angle-mode stabilizer: an open-loop load of it must ask for the
## stabilizer or the server refuses (`crates/ofs-sim/src/server.rs`, the open_loop_stabilized refusal).
static func quad_is_stabilized(path: String) -> bool:
	return fc_kind(path) == "open_loop_stabilized"
```

- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` in WSL — Expected: PASS.
- [ ] **Step 5: Commit** `feat(godot): the quad file's fc kind in the catalog scanner (#172)`.

### Task 3: Start asks for the stabilizer — the wiring and the e2e proof

**Files:**
- Modify: `godot/scripts/settings.gd` (`to_client_dict`, lines 209–226), `godot/scripts/app.gd` (preloads, line 10; `_start_session`, lines 114–133)
- Modify: `godot/tests/e2e_open_loop.gd` (after the menu round-trip block, before `app.queue_free()`, line 406)
- Test: `godot/tests/test_settings.gd`, `godot/tests/e2e_open_loop.gd`

**Interfaces:**
- Consumes: Task 1's bridge key; Task 2's `Catalog.quad_is_stabilized`.
- Produces: `to_client_dict` always carries `"stabilize": bool(...)`; `_start_session` sets `session["stabilize"] = true` when the merged session is open loop and the quad scans stabilized. Nothing new is exported for other tasks.

- [ ] **Step 1: Write the failing unit test** in `godot/tests/test_settings.gd` (after `test_the_client_dictionary_launches_a_server_only_when_one_is_configured`, line 47):

```gdscript
func test_the_client_dictionary_forwards_a_derived_stabilize() -> void:
	var s := AppSettings.defaults()
	eq(AppSettings.to_client_dict(s)["stabilize"], false, "no stabilize asked by default")
	s["stabilize"] = true
	eq(AppSettings.to_client_dict(s)["stabilize"], true, "a session carrying stabilize forwards it")
	ok(not AppSettings.resolve({"stabilize": true}, {}, PackedStringArray()).has("stabilize"),
		"stabilize is not a setting: the config file cannot set it")
```

- [ ] **Step 2: Write the failing e2e block** in `godot/tests/e2e_open_loop.gd`, between the `race mode shows it` check (line 404) and `app.queue_free()` (line 406). The suite already boots `OFS_OPEN_LOOP=1` (line 83) — the refusing boot — and `data_dir`/`repo` are in scope (lines 82, 385):

```gdscript
	# The stabilized start (#172): under this open-loop boot, a quad file that is its own stabilizer flies stabilized —
	# Start asks the server for the stabilizer instead of being refused at Load (docs/scripted-flight.md).
	var stabilized: String = data_dir.path_join("quads").path_join("e2e-stabilized.toml")
	DirAccess.make_dir_recursive_absolute(stabilized.get_base_dir())
	var quad_text := FileAccess.get_file_as_string(repo.path_join("quads").path_join("opendrone-5f-freestyle.toml"))
	FileAccess.open(stabilized, FileAccess.WRITE).store_string(quad_text
		.replace("kind = \"sitl\"", "kind = \"open_loop_stabilized\"")
		.replace("name = \"OpenDrone 5F freestyle\"", "name = \"E2E stabilized 5F\""))
	_press(app, KEY_ESCAPE)
	app.settings["quad_path"] = stabilized
	app.main_menu.row("start").pressed.emit()
	var stabilized_flying: bool = await _wait_for(func(): return app.client.get_phase() == "flying", 30.0)
	_check(stabilized_flying, "Start flies a stabilized quad under the open-loop boot (phase: %s %s)" % [app.client.get_phase(), app.client.get_phase_detail()])
	_check(app.client.get_quad_name() == "E2E stabilized 5F", "the stabilized quad is the one flying: %s" % app.client.get_quad_name())
	app.sticks_override = {"roll": 0.0, "pitch": 0.0, "yaw": 0.0, "throttle": 0.9, "aux": [1.0, -1.0, -1.0, -1.0], "status": ""}
	var lifted: bool = await _wait_for(func(): return app.drone.position.y > 0.5, 20.0)
	_check(lifted, "and it lifts under the stabilizer: y = %.2f" % app.drone.position.y)
```

(Esc first — the suite's own idiom at its line 370 — because the race-mode flight above is still flying and `start` would return "already started"; Esc also opens the menu, so no explicit `open()` is needed. Throttle 0.9 is the suite's proven climb recipe at its line 98.)

- [ ] **Step 3: Run** `bash scripts/run-godot-tests.sh unit` in WSL — Expected: FAIL (`to_client_dict` has no `stabilize` key). Then `bash scripts/run-godot-tests.sh open-loop` — Expected: FAIL on the new `_check`s: the phase is `failed` and the detail carries the server's refusal ("this quad file is open_loop_stabilized; load with stabilize = true, or without the open_loop_fc override") — the bug on record; keep that output in the task log.
- [ ] **Step 4: Implement.** In `godot/scripts/settings.gd`'s `to_client_dict` (line 215), after `"open_loop": s["open_loop"],` add:

```gdscript
		"stabilize": bool(s.get("stabilize", false)),
```

  In `godot/scripts/app.gd`: add `const Catalog = preload("res://scripts/catalog.gd")` after the `FlightAudio` preload (line 10); in `_start_session`, directly after `session.merge(choices, true)` (line 117), add:

```gdscript
	# A quad file that is its own stabilized FC must be asked for as such: an open-loop load of it without the
	# stabilizer is refused by the server. Derived per start, never a setting (docs/scripted-flight.md).
	if session["open_loop"] and Catalog.quad_is_stabilized(session["quad_path"]):
		session["stabilize"] = true
```

  Nothing else in `_start_session`, `_on_menu_start` or the menu changes: `effective_open_loop` stays the menu's rule, and the menu-less boot's raw `open_loop` semantics are untouched.
- [ ] **Step 5: Run** `bash scripts/run-godot-tests.sh unit` and `bash scripts/run-godot-tests.sh open-loop` in WSL — Expected: PASS (grep the output for `SCRIPT ERROR`; none may appear).
- [ ] **Step 6: Commit** `fix(godot): Start asks for the stabilizer a stabilized quad needs (#172)`.

### Task 4: Docs, changelog and the full gates

**Files:**
- Modify: `docs/scripted-flight.md` (lines 90–92), `README.md` (the menu paragraph, around line 111), `CHANGELOG.md` ("Unreleased")

**Interfaces:**
- Consumes: nothing from earlier tasks beyond their existence.

- [ ] **Step 1: Update `docs/scripted-flight.md`.** The paragraph's last two sentences (lines 90–92) — "The game has no `stabilize` option, but a quad file with `[fc] kind = "open_loop_stabilized"` flies stabilized in Godot just as in Python (the same gRPC path the Rust stabilizer test drives); there is no Godot-specific test for it." — become:

```
The game has no `stabilize` option to pick, but it asks the server for the stabilizer itself when the chosen
quad's kind is `open_loop_stabilized` (an open-loop start sends `stabilize` through the bridge), so such a file
flies stabilized in Godot just as in Python under every controller setting — `godot/tests/e2e_open_loop.gd` ends
with that start.
```

- [ ] **Step 2: Update `README.md`'s** menu paragraph: "and the flight controller (Betaflight is offered when `OFS_SITL_LAUNCH` is set, else the flight is open loop)" gains a closing clause — "… else the flight is open loop; a quad file with `[fc] kind = "open_loop_stabilized"` flies stabilized under either pick)". And `CHANGELOG.md` "Unreleased" gains, above the DVR section:

```markdown
### The stabilized start (#172)
#### Fixed
- **The menu's Start no longer refuses an `open_loop_stabilized` quad under an open-loop setting.** The start asks
  the server for the built-in stabilizer (`Load`'s `stabilize`, already on the wire since protocol 12) when the
  chosen quad file's `[fc] kind` says the quad is its own stabilized FC — so the doc's promise ("flies stabilized in
  Godot just as in Python") holds under every controller pick, menu or menu-less boot. The Godot bridge's `start`
  now accepts a `stabilize` key. No protocol, settings or simulator change.
```

- [ ] **Step 3: Run the full gates:** `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and in WSL `bash scripts/run-godot-tests.sh all` — Expected: PASS (the e2e suites include the menu-less boots; none may need a change). Check by reading that no golden under `crates/*/tests/golden/` appears in `git status`.
- [ ] **Step 4: Commit** `docs: the stabilized start in scripted-flight, README and the changelog (#172)`.

## Ledger

- Ruled at planning (2026-11-01, `main` = `fdc4917`): the issue body's anchors have drifted — the refusal is `crates/ofs-sim/src/server.rs:301-313` (issue: 306-312) and the bridge's `open_loop` mapping is `crates/ofs-godot/src/lib.rs:635-637` (issue: 612-614); `python/ofs/client.py`'s `load` signature is at lines 474-495. Executors re-verify before citing.
- Ruled at planning: the flight-school plan's Task (`docs/superpowers/plans/2026-10-31-flight-school.md`, line 208) still says "the bridge has no `stabilize` key" — historical after Task 1 here, but its behaviour (`open_loop: false` in the lesson choices) stays correct and unchanged; that plan's executor ledgers the wording, this pair does not edit it.
