# The stabilized start (#172)

## Why

`docs/scripted-flight.md` (lines 90–92) promises that a quad file with `[fc] kind = "open_loop_stabilized"` "flies
stabilized in Godot just as in Python" — and Python can: `load(..., open_loop_fc=True, stabilize=True)`
(`python/ofs/client.py:474-495`), where `stabilize` has been `LoadRequest` field 8 since protocol 12
(`proto/ofs/v1/sim.proto:101-103`, the header's own line 12). The promise holds in Godot only while the open-loop
setting is **off**. When it is on, the chain breaks:

- The menu's Start sends `open_loop: effective_open_loop(settings)` (`godot/scripts/app.gd:148-152`;
  `effective_open_loop` at `godot/ui/main_menu.gd:105-106`), and `to_client_dict` carries no `stabilize` key
  (`godot/scripts/settings.gd:209-226`).
- The bridge maps only `open_loop` into the Rust client's settings (`crates/ofs-godot/src/lib.rs:635-637` inside
  `parse_settings`, lines 606–653) — even though the Rust `Settings` already holds `stabilize`
  (`crates/ofs-client/src/model.rs:788-791`) and the worker already sends it in every `LoadRequest`
  (`crates/ofs-client/src/worker.rs:274-283`).
- So the server receives `open_loop_fc = true, stabilize = false` for a file whose own kind is stabilized — an
  override that would downgrade its stabilizer to throttle-only — and refuses
  (`crates/ofs-sim/src/server.rs:301-313`): *"this quad file is open_loop_stabilized; load with stabilize = true, or
  without the open_loop_fc override"*. The game can ask for neither. (The issue body's anchors `server.rs:306-312`
  and `lib.rs:612-614` have drifted to these lines on this tree; re-verify at execution.)

Any pilot who sets open loop (`OFS_OPEN_LOOP`, the menu's controller pick) and flies such a quad hits it. The
flight-school plan works around it for its own lesson starts by ruling `open_loop: false` into the choices (its spec's
D7, `docs/superpowers/specs/2026-10-31-flight-school-design.md:100-107`; its plan's Task at line 208); the menu itself
stays broken — this pair fixes the real path. Found in the whole-branch review of the Game-completion set (PR #171);
filed as #172 with `area:godot` and `bug`.

The wave umbrella's rules for this pair (`docs/superpowers/plans/2026-11-01-platform-followups.md:9`): no
settings-chain fork; the menu-less e2e path (`OFS_MENU=off`) keeps working; the flight-school plan's D7 workaround
stays valid regardless.

## Goals

1. With the open-loop setting on, starting an `open_loop_stabilized` quad from the menu flies it stabilized — the
   server's Load succeeds and the flight lifts under the built-in angle-mode stabilizer.
2. The same holds for the menu-less boot (`OFS_MENU=off` with `OFS_OPEN_LOOP=1` and such a quad chosen): the fix
   covers the Start path, not just the menu's button.
3. The Godot bridge can ask the server for everything the Python client can ask at Load — `stabilize` included — so
   future callers (the workbench, the school) need no further Rust work to reach it.
4. A regression e2e: under the open-loop boot, a stabilized quad started through the menu reaches flying and climbs.

## Non-goals

- No new user-facing option: no menu row, no `OFS_*` variable, no flag, no `[client]` key for `stabilize` (call 4).
- No simulator behaviour change: `ofs-sim`, `ofs-config`, `ofs-proto`, `ofs-client` and `python/` are untouched; the
  server's refusal logic (`server.rs:301-313`) is correct and stays.
- No protocol bump: `LoadRequest.stabilize` is field 8, on the wire since protocol 12; nothing here renumbers
  anything, whatever number the Consequence wave has moved `PROTOCOL_VERSION` to by execution time.
- No shipped stabilized quad file — the school's `school-5f.toml` stays the flight-school plan's to ship (its plan
  line 130); this pair's test writes its own temp quad (call 6).
- No change to the flight-school plan's lesson choices: D7's `open_loop: false` keeps working before and after.

## Design calls (decided here; the executor ledgers any change)

**D1. The fix's shape: the bridge grows a `stabilize` key and Start asks for the stabilizer — not the
`open_loop: false` dodge.** The alternative (send `open_loop: false` for stabilized quads, D7 generalized into the
menu) flies identically — with no override the file's own kind flies (`crates/ofs-sim/src/vehicle.rs:1037`,
`opts.fc_override.unwrap_or(cfg.fc.kind)`) — and needs no Rust, but the session's `LoadRequest` would then say
`open_loop_fc = false` while the pilot's locked menu row says Open loop: the request lies about what was asked. The
key costs three lines in `parse_settings` beside the `open_loop` mapping (`lib.rs:635-637`) plus a doc-comment word in
`start`'s key list (`lib.rs:97-101`), needs no protocol bump (non-goal above), and gives the bridge parity with
Python's `load()`. The repro bundle is honest under both shapes — it records the *effective* FC, not the request
(`server.rs:350-354`, `effective_fc_flags` at `crates/ofs-sim/src/repro.rs:215-225`) — so the request itself is the
only thing (b) would falsify; it is also the thing every future caller reads. Ruled: (a).

**D2. How the client learns the kind: a Catalog-style line-scan, `Catalog.fc_kind(path)` — not `quad_spec.gd`, not
the wire.** The quad-model plan's `quad_spec.gd` (unexecuted; `docs/superpowers/plans/2026-10-31-quad-model.md`, Task
at its line 465, spec call 6) scans the quad TOML for `frame` and motor geometry — a different question; depending on
it would queue this small fix behind the Game wave's execution for no gain. The server-tells-the-client route needs a
proto change, refused by the no-bump preference. A focused scanner beside `entry()` in `godot/scripts/catalog.gd`
follows the shipped precedent — `entry()` already line-scans `name`, `[[gates]]` and `[[courses]]`
(`catalog.gd:49-68`) with the same strip/`[`-table/`split("=", true, 1)`/quote-trim idioms. Semantics: track the
current table; inside `[fc]` a line whose first token is `kind` sets the kind (comments and other tables cannot; a
`kindred =` line does not match `value[0].strip_edges() == "kind"`); a missing or unreadable file returns `""` —
never stabilized, and the load fails at the server with the proper config error anyway. One predicate wraps it:
`Catalog.quad_is_stabilized(path)` is `fc_kind(path) == "open_loop_stabilized"` (the kind strings are
`crates/ofs-config/src/lib.rs:336-344`, `snake_case` serde). Seam note: when `quad_spec.gd` lands, the two scanners
coexist (different questions); folding `fc_kind` into it is a follow-up, not a dependency.

**D3. One decision point: `_start_session`, after the choices merge.** `godot/scripts/app.gd:114-117` builds the
session dictionary (`settings.duplicate()` + `choices` merge) for both start paths — the menu's
`_on_menu_start` (lines 148–152) and the menu-less boot (`_ready`'s else, lines 108–111, `_start_session()` with no
choices). The derivation — `if session["open_loop"] and Catalog.quad_is_stabilized(session["quad_path"]):
session["stabilize"] = true` — sits immediately after the merge, so both paths get it while each keeps its own
`open_loop` semantics (the menu-less path still sends the raw setting; `effective_open_loop` stays the menu's rule
alone — no behaviour change off the menu's button). The flight-school lesson starts also flow through
`_start_session`, but their choices carry `open_loop: false` explicitly (its plan line 208), so the derivation stays
dormant there and D7 keeps working unchanged — the umbrella's rule, kept by construction rather than by amendment.

**D4. `stabilize` is derived, never a setting.** No `defaults()` key, no `ENV_NAMES`/`FLAG_NAMES` entries, no menu row,
nothing written to `ofs_client.cfg`: it is computed per start from the quad file and the effective open loop, and
lives only in the session copy. The chain cannot fork even by hand: `resolve()` copies only keys `defaults()` has
(`settings.gd:95-99`; pinned by `test_settings.gd`'s "unknown keys are dropped"), so a hand-edited
`[client] stabilize=true` is dropped like any unknown key. `to_client_dict` forwards it as an always-present bool —
`"stabilize": bool(s.get("stabilize", false))` — mirroring the bridge's own default-false.

**D5. The server, the proto and Python are untouched.** The guards are right: `stabilize` without `open_loop_fc` is
refused (`server.rs:301-302`), and the downgrade refusal is this issue's own text (`server.rs:304-313`). The client
only ever sets `stabilize` when the merged session's `open_loop` is true, so the first guard cannot trip from the
game. Python already has the flag; the Rust client already sends it. `PROTOCOL_VERSION` (16 at planning,
`crates/ofs-proto/src/lib.rs:12`) does not move — and if the Consequence wave's bumps land first, nothing here
renumbers, so no queueing is needed.

**D6. The proof is the open-loop e2e, with a temp stabilized quad written into the e2e data dir.** No shipped quad
carries the kind today — all nine `quads/*.toml` say `kind = "sitl"` — and the first stabilized file, `school-5f.toml`,
is the flight-school plan's to ship; waiting for it would couple this pair to the Game wave's execution. The
regression instead writes `<data_dir>/quads/` a copy of `quads/opendrone-5f-freestyle.toml` with the kind (and the
name, for a distinct assertion) flipped — the school quad's own recipe (D7: a copy of the 5F freestyle; the sitl-only
`launch`/`betaflight_diff` checks are gated on `kind == Sitl`, `crates/ofs-config/src/lib.rs:1396-1399`, so the copy
loads) — and starts it through the menu under the suite's existing `OFS_OPEN_LOOP=1` boot
(`godot/tests/e2e_open_loop.gd:83`), beside the menu round-trip block (lines 368–404): press Start, wait for flying,
then throttle up and wait for the drone to climb — the stabilizer is what turns sticks into lift
(`docs/scripted-flight.md`'s own subject). On main this start fails with the refusal — that failing run is the bug on
record. The block is the same file the flight-school plan's Task 7 appends to (its line 216): whichever pair executes
second re-reads the file (the phase umbrellas' standing rule for shared surfaces).

**D7. The doc keeps promising, now truly.** `docs/scripted-flight.md:90-92` — "The game has no `stabilize` option,
but a quad file with `[fc] kind = "open_loop_stabilized"` flies stabilized in Godot just as in Python" — becomes
"the game has no `stabilize` option to pick, but it asks the server for the stabilizer itself when the chosen quad's
kind is stabilized (an open-loop start sends `stabilize`)", and "there is no Godot-specific test for it" now points
at the e2e block of call 6. The sentence stops describing an exception and starts describing the rule.

## Tests

- `godot/tests/test_extension.gd`: a `start` dictionary carrying `open_loop: true, stabilize: true` is accepted
  (returns `""`; a second start returns "already started") — the parse-level contract. It cannot fail before the
  bridge change (an unknown key is silently ignored today); the wire-level proof is the e2e, and the task says so.
- `godot/tests/test_catalog.gd`: the shipped quads scan as `"sitl"`; a written stabilized quad as
  `"open_loop_stabilized"`; a world file (no `[fc]`) as `""`; a commented `# kind` line and a `kind` outside `[fc]`
  do not fool it; a missing file is `""` and `quad_is_stabilized` false.
- `godot/tests/test_settings.gd`: `to_client_dict` forwards `stabilize` — false from plain settings, true from a
  session carrying it — and `resolve` still drops an unknown `stabilize` key from the config file (call 4's guard).
- `godot/tests/e2e_open_loop.gd`: the stabilized start under `OFS_OPEN_LOOP=1` reaches flying and climbs (call 6);
  the rest of the suite — the menu-less boot, the default quad's start — unchanged and green (the umbrella's
  menu-less rule).
- The Rust gates (`cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo test --workspace --locked`, `cargo +1.85 check -p ofs-godot`) stay green; no golden can move (nothing
  outside `ofs-godot` changes).

## Docs

`docs/scripted-flight.md` (call 7), `README.md`'s menu paragraph (one clause: a stabilized quad file flies stabilized
whatever the controller pick, around line 111), `CHANGELOG.md` "Unreleased" (Fixed: the menu's Start no longer refuses
an `open_loop_stabilized` quad under an open-loop setting; the Godot bridge accepts `stabilize`). The by-hand check
(a real display, a real controller: pick open loop, start such a quad, feel the angle-mode sticks) feeds the wave's
`docs/human-check.md` section.

## What is left to the builder

The temp quad's name in the e2e (any string that cannot collide with a shipped label); whether the e2e block sits
before or after the school plan's additions in `e2e_open_loop.gd` (whichever pair executes second reads the file as
it then is).

## Open questions for the maintainer

1. **Ship a stabilized quad ahead of the school?** This pair's test writes its own temp quad, so the fix lands
   user-invisible until `school-5f.toml` arrives with the Game wave. An extra shipped `quads/*.toml` with the kind
   would make the fix exercisable from a default install today — but it duplicates the school quad's provenance and
   its catalog slot. Ruled: no extra file (the school's is weeks away at most and carries the lesson texts); reversed
   only if the school slips.

## Checked by eye (headless Godot renders nothing)

With a real display and controller: open loop picked in the menu, a stabilized quad started, the sticks feel like
angle mode (a held roll holds a lean, the quad self-levels); the HUD shows no Betaflight boot; the same flight from a
`--menu=off` boot with `OFS_QUAD` pointing at a stabilized file.
