# Platform and Follow-ups: The Wave After the Two Phases

> **For agentic workers:** this file is the wave umbrella, not an executable task list. The pairs below are the executable units; the #47 chore is specified inline (a B6-style single task). REQUIRED SUB-SKILL for a pair: superpowers:subagent-driven-development (recommended) or superpowers:executing-plans.

**Goal:** plan the last actionable unplanned issues — the native-Windows-SITL port (#105, with #107 verified-and-closed on its evidence), the menu refusing an `open_loop_stabilized` quad (#172), and the `[look]` world section (#173) — plus the rerun-sdk chore (#47). After this wave, every open issue in the repository is either planned (the Game-completion set of PR #171, the Consequence-completion set of PR #175, or this wave), executing, or honestly parked (backlog, needs-data, blocked, wait-for-strike).

**Context:** written 2026-11-01 against `main` at `fdc4917` (protocol 16 — the Consequence wave's three pairs will bump it; this wave's only wire work, #173's, queues behind theirs with the next-free-number rule both umbrellas already state). Both phase plans are merged and **await execution**: at survey time no PRs were open — PR #176 (the `feat/mesh-worlds` lane, a `.glb`→`TriMesh` loader in `ofs-config`) opened as this wave was being planned, and a dvr-frame-gap worktree is also in flight; both are the maintainer's own lanes, tracked by neither phase plan, and this wave plans around them, not over them. The rhythm after this PR merges is execution: one PR per issue, Game wave and Consequence wave in their umbrellas' orders, this wave's pairs anywhere (they touch no wave surface except #173's queued bump, and #173's Godot half additionally follows #134's execution).

**The rules this wave respects:** the platform lane changes no simulator behaviour (a ucrt64 build is the same lockstep SITL or the differences are documented, per #105's own done-when); #172 may not fork the settings chain and must keep the menu-less e2e path intact (the flight-school plan's D7 workaround stays the workaround until this fixes the real path); #173 changes no physics and keeps the analog chain the picture's judge (#134's rules, extended to data); #47 is a deliberate bump — pin, port, docs, and a note of the API changes hit.

## The tasks

1. **#105 — the ucrt64 port** (`specs/2026-11-01-native-windows-sitl-design.md` + `plans/2026-11-01-native-windows-sitl.md`), with **#107 verified-and-closed on its evidence**: the windows-sitl CI job already builds, live-tests, gates `publish` (since #167), uploads the binary artifact, and fails loudly on a stub-symbol miss (`ci.yml`'s own comment) — #107's done-when is satisfied on main, and the pair's plan carries the close evidence. The port itself is the real work: Winsock for the UDP/serial shims, `ioctlsocket`, the compat shims, `_WIN32`/`MINGW`-fenced so Linux and `msys` are untouched, a build-script flag, and the whole-seeded-flight cross-build comparison.
2. **#172 — the stabilized start** (`2026-11-01-stabilized-start`): with the open-loop setting on, starting an `open_loop_stabilized` quad from the menu flies it stabilized. The bridge-vs-client ruling (a `stabilize` key vs `open_loop: false` for such quads) is the spec's first call.
3. **#173 — the `[look]` world section** (`2026-11-01-look-section`): dressable user worlds; the wire queues behind the Consequence wave's bumps (next free protocol number and fields, per the standing coordination rule).
4. **#47 — the rerun-sdk chore, inline:** check the newest rerun release; port `python/ofs/rerun.py` to it; update the `rerun` extra's pin and the docs; record the API changes hit in the PR body. One commit; `PYTHONPATH=python python -m pytest python/tests -q` green; the rerun-dependent tests (find them: `grep -rln rerun python/tests`) run locally with the extra installed before the commit.

## Manual QA

The platform pair's by-hand checks (a plain-PowerShell start with no MSYS2 on `PATH`, the Configurator port) land in #163 via its plan; the follow-ups' are each pair's. `docs/human-check.md` grows this wave's section with this PR.

## Every other open issue — dispositions

The standing dispositions live in the two merged umbrellas (`docs/superpowers/plans/2026-10-31-game-completion.md` and `docs/superpowers/plans/2026-10-31-consequence-completion.md`) and are unchanged. Deltas since #175:

- **Both phase waves await execution** (no open PRs). The Game six execute per #171's umbrella; the Consequence core and minors per #175's.
- **In-flight local lanes** on this machine, unplanned here: `feat/mesh-worlds` (a `.glb`→`TriMesh` loader, two commits) and a dvr-frame-gap worktree — the maintainer's lanes; this wave touches neither surface.
- **#107**: satisfied on main and **closed at planning time (2026-11-01) with the evidence comment** (the job, the live tests, the artifact, the stub check, the duration/RTF records, each with its SHA) — the native pair's Task 1 is the re-verification recipe; any residue lives in #105's pair.
- **#174 and #146**: wait-for-strike flakes; #146's conditional task lives in the Consequence minors plan, #174's next strike is its own to record.
- **The backlog** (axis epics, research, receivers, needs-data, blocked) is unchanged and honestly parked.
