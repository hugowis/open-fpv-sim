# macOS manual validation and distribution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the two remaining "not done" macOS items — a physical Mac was never touched, and no distribution decision was written — with a repeatable checklist whose results are recorded in `docs/research/macos.md`, and a explicit distribution decision.

**Architecture:** Add a short manual runbook for a maintainer with a Mac: Gatekeeper on the downloaded Godot editor, the game window and Metal renderer, and a gamepad. Add one small Godot script that prints the connected joypads so the gamepad step produces a log instead of a subjective check. Record the distribution decision (source build, no notarised binary) in the findings doc, and link the runbook from `docs/dev-setup.md`. No CI job changes; this plan is for a human at a physical machine.

**Tech Stack:** macOS 26 (Apple silicon or Intel), Godot 4.7.2, Rust 1.85+, `spctl`, `xattr`, `codesign`, `system_profiler`, `godot --headless -s` for the input probe.

**Spec:** PR #101's "Not done / deferred minors" (`docs/research/macos.md`, "Not done, and why" items 1-4), issue #68.

## Execution base

Start from a fresh worktree on `origin/main` (`341ad51`) so the docs and scripts match the current tree:

```bash
git fetch origin
git worktree add ../ofs-macos-manual -b worktree-macos-manual origin/main
```

## Global Constraints

- This plan adds no production code and no CI job. The only code is a read-only Godot input probe under `godot/tests/`.
- Record exactly what the physical machine showed, with the macOS version, architecture, Godot zip name, and the command output. Do not infer a pass from a runner result.
- Do not commit a downloaded Godot binary, a recording, or a screenshot with personal information. Put logs in the findings doc as text; keep images out of the repository unless they are already part of the docs.
- The distribution decision remains "build `ofs-sim` from source; the Python package installs with pip"; this plan records it, it does not add a notarised binary.

## Review Focus

- **Gatekeeper.** A downloaded, unsigned `Godot.app` may be quarantined; the runbook must record the actual prompt and whether `spctl` accepts it, not just "it opened".
- **Rendering.** A window opening is not proof of Metal; record the renderer line from Godot's verbose output and the measured frame rate.
- **Gamepad.** A connected pad is not proof that the game reads it; the probe must print the device name, axes, and buttons, and the game's controls screen must respond.
- **Distribution.** "Not planned" is a decision; the doc must say why (pure-Python package + source-built `ofs-sim`, no signing key/notarisation pipeline) so the next reader does not reopen it accidentally.

---

### Task 1: Add the gamepad probe

**Files:**
- Create: `godot/tests/manual_input.gd`
- Modify: `godot/tests/README.md` if it exists; otherwise `docs/dev-setup.md` (link the probe)

**Interfaces:**
- Produces: `godot --path godot -s res://tests/manual_input.gd` prints one line per connected joypad and exits 0. It does not start the simulator and does not need a server.

- [ ] **Step 1: Check the tests' conventions**

Run: `Get-ChildItem godot/tests -Filter *.gd | Select-Object Name` (or `ls godot/tests`).
Read one `SceneTree` script, for example `godot/tests/e2e_open_loop.gd`, to copy its `_initialize`/`_check` style. Do not invent a different test harness.

- [ ] **Step 2: Write the probe**

Create `godot/tests/manual_input.gd`:

```gdscript
extends SceneTree
## Manual check on a physical Mac: prints the joypads Godot sees, with their axes and buttons.
## Run windowed (not --headless), plug or unplug a pad, and run it again:
##   godot --path godot -s res://tests/manual_input.gd
## The gamepad step in docs/research/macos-manual.md records this output.

func _initialize() -> void:
	var pads := Input.get_connected_joypads()
	print("joypads: ", pads)
	for id in pads:
		print("  %d: %s axes=%d buttons=%d" % [
			id,
			Input.get_joy_name(id),
			Input.get_joy_axis_count(id),
			Input.get_joy_button_count(id),
		])
	quit()
```

- [ ] **Step 3: Run it without a pad**

Run: `godot --path godot -s res://tests/manual_input.gd`
Expected: `joypads: []`, exit 0. (If `godot` is not on PATH, use `GODOT_BIN` from the download in the runbook.)

- [ ] **Step 4: Run it with a pad**

Connect a USB or Bluetooth gamepad, run the same command again.
Expected: at least one id, a non-empty name, and non-zero axes/buttons. Record the exact output in the runbook result.

- [ ] **Step 5: Commit**

```bash
git add godot/tests/manual_input.gd
# Godot 4.7 writes a .uid beside a new script on import; the repository tracks the other tests' .uid files.
[ -f godot/tests/manual_input.gd.uid ] && git add godot/tests/manual_input.gd.uid
git commit -m "test(godot): print the joypads for the macOS manual check (#101)"
```

---

### Task 2: Write the physical-Mac runbook

**Files:**
- Create: `docs/research/macos-manual.md`
- Modify: `docs/research/macos.md` (link the runbook from "Not done, and why")
- Modify: `docs/dev-setup.md` (macOS section: link the runbook)

**Interfaces:**
- Consumes: Task 1's `manual_input.gd`; the Godot download/checksum logic in `scripts/run-godot-tests.sh` (do not duplicate the hash; reference it).
- Produces: a checklist with a result table at the top (pass/fail/not run per item) and a "Results" section the maintainer fills in.

- [ ] **Step 1: Write the runbook skeleton**

Create `docs/research/macos-manual.md`:

```markdown
# macOS on a physical machine: manual checklist

The CI survey (`docs/research/macos.md`) runs on GitHub's macOS runners. It cannot answer whether the game opens a
window, renders with Metal, or reads a gamepad on a real Mac. This checklist does. Record the machine and the results
in the table; leave "not run" rather than guessing.

| item | macOS | Godot | result | evidence |
|---|---|---|---|---|
| Gatekeeper opens the downloaded editor | | | not run | |
| The game opens a window | | | not run | |
| The renderer is Metal | | | not run | |
| The frame rate is playable | | | not run | |
| A gamepad is connected and read | | | not run | |
| The Python package installs | | | not run | |
| `ofs-sim` builds from source | | | not run | |

## 1. Download the editor

Use the same pinned zip as `scripts/run-godot-tests.sh` (the `Darwin` case): `Godot_v4.7.2-stable_macos.universal.zip`.
Download it and the release's `SHA512-SUMS.txt` with a browser (so Gatekeeper sees the quarantine flag), then check it:

```bash
shasum -a 512 -c SHA512-SUMS.txt   # the line for the macOS zip, from the release page
unzip Godot_v4.7.2-stable_macos.universal.zip
xattr -l Godot.app
spctl --assess --type execute -vv Godot.app
```

Record: the quarantine attribute, the `spctl` verdict, and whether opening `Godot.app` from Finder showed the
"unidentified developer" prompt. If it did, the expected workaround is Control-click -> Open; say so and whether it
worked.

## 2. Window and renderer

```bash
export GODOT_BIN="$PWD/Godot.app/Contents/MacOS/Godot"
cargo build -p ofs-sim -p ofs-godot --locked
export OFS_SIM_BIN="$PWD/target/debug/ofs-sim"
# Either fly open loop, or set OFS_SITL_LAUNCH to the patched SITL built in step 3:
"$GODOT_BIN" --verbose --path godot -- --open-loop
```

Record: a window appeared; the verbose log's rendering driver line (Metal); whether the HUD and FPV view animate; a
rough frame rate from the HUD or `--verbose`; whether the client started `ofs-sim` itself (the HUD shows a flying state
rather than an "unavailable" tip). A window that opens but shows a black viewport is a fail. If the open-loop flag is
not accepted by this client version, run without `-- --open-loop` after setting `OFS_SITL_LAUNCH` and record the
difference.

## 3. Gamepad

With the editor and the game closed:

```bash
"$GODOT_BIN" --path godot -s res://tests/manual_input.gd
```

Then start the game and open the controls screen. Record: the probe's `joypads:` line, the pad name, and whether the
sticks move the bars. A pad that is listed but does not move the controls is a fail. Note the connection (USB/Bluetooth).

## 4. Python and `ofs-sim`

```bash
python3 -m pip install -e "python[dev]"
python3 -c "import ofs; print(ofs.__version__)"
cargo build -p ofs-sim --locked
```

Record the Python version, the install result, and the build result. On Intel, `rerun-sdk` has no x86_64 wheel; record
that the `[dev]` extra skips it rather than calling the install a failure.

## 5. Findings

Copy the filled table and the short notes into `docs/research/macos.md` under "A physical Mac". If an item fails, open
an issue with the exact command and output; do not change the checklist to make it pass.
```

- [ ] **Step 2: Link it from the findings doc**

In `docs/research/macos.md`, under "Not done, and why" item 2 ("A physical Mac"), replace the paragraph with:

```markdown
2. **A physical Mac.** The window, Metal renderer, gamepad, and Gatekeeper quarantine are untested. Follow
   `docs/research/macos-manual.md` on a real machine and copy its table into this file; the CI runners cannot answer
   these questions.
```

- [ ] **Step 3: Link it from dev setup**

In `docs/dev-setup.md`, in the macOS section, add:

```markdown
For a physical-Mac check (window, Metal, gamepad, Gatekeeper), follow `docs/research/macos-manual.md`.
```

- [ ] **Step 4: Check the links and whitespace**

Run: `git diff --check`
Expected: no output. There is no automated Markdown link checker in this repository; open each new relative link
(`docs/research/macos-manual.md`, `docs/dev-setup.md`) once from the repository root to confirm it resolves.

- [ ] **Step 5: Commit**

```bash
git add docs/research/macos-manual.md docs/research/macos.md docs/dev-setup.md godot/tests/manual_input.gd
git commit -m "docs(macos): a physical-machine checklist (#101)"
```

---

### Task 3: Record the distribution decision

**Files:**
- Modify: `docs/research/macos.md` (replace "Distribution" in "Not done, and why" with a decision paragraph)
- Modify: `CHANGELOG.md` only if the repository already records documentation changes there; otherwise leave it.

**Interfaces:**
- Produces: a decision paragraph that names the supported install routes and the reason there is no notarised binary.

- [ ] **Step 1: Write the decision**

In `docs/research/macos.md`, replace item 3 ("Distribution.") in "Not done, and why" with:

```markdown
3. **Distribution: source build, no notarised binary (decided).** The `ofs` Python package is pure Python and installs
   with `pip install -e "python[dev]"`; `ofs-sim` is built from source with `cargo build -p ofs-sim --locked`. A
   signed and notarised `.app` or universal binary is not planned: it needs an Apple Developer ID, a notarisation
   step in the release workflow, and a decision about the Godot editor download that the current release does not
   have. Reopen this only with an Apple Developer account and a release-workflow owner.
```

- [ ] **Step 2: Remove the open-item wording**

Check that "Not done, and why" now lists only the genuinely open items (radio goldens, physical Mac, native cleanup
wait, if those plans have not landed yet). Do not leave a "Distribution" bullet that says "not planned" and also a
decision paragraph that says the same thing twice.

- [ ] **Step 3: Commit**

```bash
git add docs/research/macos.md
git commit -m "docs(macos): decide distribution: source build, no notarised binary (#101)"
```

---

### Task 4: Run the checklist and record it

**Files:**
- Modify: `docs/research/macos.md` (the "A physical Mac" section's table)
- Modify: `docs/research/macos-manual.md` (fill the table with the result, if the maintainer wants the raw checklist kept)

- [ ] **Step 1: Run the runbook on the Mac**

Follow `docs/research/macos-manual.md` from a fresh checkout of the branch. Keep the terminal output for each step.

- [ ] **Step 2: Fill the table**

Copy the filled table into `docs/research/macos.md` under a new `### A physical Mac` section. Use `pass`, `fail`, or
`not run`; put the exact error in `evidence` for a failure.

- [ ] **Step 3: Open issues for failures**

For each `fail`, open an issue with the runbook step, the command, the expected result, and the actual output. Do not
edit the runbook to hide a failure.

- [ ] **Step 4: Commit**

```bash
git add docs/research/macos.md docs/research/macos-manual.md
git commit -m "docs(macos): physical-machine results (#101)"
```

---

### Task 5: Verify the docs

**Files:**
- No source changes.

- [ ] **Step 1: Check the links and formatting**

```bash
git diff --check
```

Expected: no output. There is no Markdown link checker in this repository; the runbook and the two links were read
through once from the repository root.

- [ ] **Step 2: Review the final diff**

```bash
git diff main...HEAD -- docs/research/macos.md docs/research/macos-manual.md docs/dev-setup.md godot/tests/manual_input.gd
```

Expected: only the runbook, the table, the links, and the probe. No CI job, no production code, no downloaded binary.

- [ ] **Step 3: Commit any doc fix**

```bash
git add -A docs godot/tests/manual_input.gd
git commit -m "docs(macos): link check and runbook fixes (#101)"
```

## Self-review notes

- Coverage: the physical-Mac validation and the distribution decision are both addressed; the radio goldens, cleanup wait, and Intel survey are in the three companion plans.
- Placeholder scan: the result tables intentionally say "not run" until a human fills them; no result is invented.
- Type consistency: the probe uses Godot 4's `Input.get_connected_joypads`, `get_joy_name`, `get_joy_axis_count`, and `get_joy_button_count`; the runbook's commands match the script's path and the pinned Godot version.
