# Game Completion: The Phase Plan — Closing #135 and #136

> **For agentic workers:** this file is the phase umbrella, not an executable task list. Each of the six tasks below has its own spec + implementation-plan pair in this directory (and its design spec under `docs/superpowers/specs/`); those pairs are the executable units. REQUIRED SUB-SKILL for a pair: superpowers:subagent-driven-development (recommended) or superpowers:executing-plans.

**Goal:** Ship the six still-open children of the Game epics — #124, #125, #128, #130, #133, #134 — so both epics close and the Consequence epic #160 can start, which its body makes a precondition ("Starts when the Game epics (#135, #136) close").

**Context:** the Game waves already shipped #123 and #126 (menu and settings, PR #141), #129 (three more quads, PR #140), #131 plus part of #130 (the park and meadow worlds and a modelled quad, PR #147), #127 (controller setup, PR #149) and #132 (courses and race modes, PR #151, protocol 15). Two Consequence-lane children also landed early because they were independent: #153 (heightfield terrain, PR #161) and #158 (flight audio, PR #162). What remains of the epics is the six tasks below. Written 2026-10-31 against `main` at `87c5914` (since then the B6 sweep merged as `b37aae5`); every plan executor re-verifies its own anchors before touching code (the B6 lesson: an evidence SHA copied on trust was wrong).

**Epic rules the whole phase respects** (from the #135/#136 bodies): presentation, onboarding and persistence in the client only — the models do not change; the settings chain (`user://ofs_client.cfg` < `OFS_*` < flags) is not forked; the e2e suites' menu-less path keeps working; every part parameter carries provenance (estimates marked); the worlds' physics stays what it is — flat ground, axis-aligned boxes (#28 and #29 track the physics work, in the next wave); the analog video chain keeps deciding what the goggles show — worlds get better-looking, not cleaner.

## The six tasks, and the order they should land

One PR per task, one issue closed per PR, the epic checkbox ticked by the close — the shape every Game wave so far has used (#141, #147, #149, #151). Proposed order, content before chrome, onboarding last so it teaches the finished surface:

1. **#130 — a real 3D model for the quad** (`specs/2026-10-31-quad-model-design.md` + `plans/2026-10-31-quad-model.md`). First because PR #147 already delivered "part of #130" (a modelled quad): the remaining scope — what separates that placeholder from a real model worth the issue title — must be pinned early, before anything builds on it. Asset provenance is a design call the spec must rule (license-recorded the way part parameters are).
2. **#134 — better world rendering** (`2026-10-31-world-rendering`). The goggles keep their analog truth; only what the world looks like improves.
3. **#133 — best-lap ghost and local records** (`2026-10-31-best-lap-ghost`). Builds on the race state and courses #132 just landed; protocol work if the wire must grow, ruled in its spec.
4. **#128 — a drone workbench over the parts library** (`2026-10-31-drone-workbench`). The epic #160 body says repair is "a workbench action" — this spec must leave the seam for #157's repair loop, without building any of it.
5. **#124 — a first-run tour** (`2026-10-31-first-run-tour`).
6. **#125 — flight school** (`2026-10-31-flight-school`). Last: lessons that teach the full surface, and the race gates (#71's `[[gates]]`) and scripted flights are candidate lesson machinery.

The pairs are independent — order is preference, not dependency; the two cross-task contracts are #128's repair seam for #157 and #130's preview scene, which #128's workbench re-hosts.

## Manual QA

Every plan carries its by-hand checks (what a human must fly, hear or eyeball that no suite covers). They land in #163 — the collector issue the waves feed — as each PR closes; #163 itself stays open until the Consequence wave adds its sections too.

## The next wave (propositioned here, planned later)

Per #160's ordering, once both epics close: representation first — #28 (rotated boxes), #154 (terrain in the radio and video line of sight), #155 (terrain in the Godot world); consequence second — #156 (collisions), #157 (crash damage riding the `PropDamage`/`MotorFailure` seams, repaired in #128's workbench); #159 (audio in the DVR) is the independent lane its sibling #158 was. The lane is already moving: #154, #155 and #159 have open PRs (#168, #169 — which lands protocol 16 — and #170), so the wave's unstarted core is #28 → #156 → #157, and wire work after #169 merges numbers its protocol from 16. The parent issues close with their last child: #27 with #155, #29 with #157, #36 with #159. Riders for that wave's minors tail: #164 (the `battery_current_a` clamp), #165 (the measured magnetometer into SITL); #146 is now checkable — #167 merged as `b37aae5`; read the `windows-sitl` record since that merge, and if the flake survived the sweep, it rides too.

## Every other open issue — dispositions

- **Platform lane (continues outside phase planning):** #107 — its build-and-gate half shipped (`windows-sitl` CI, promoted to gating by B6); the live-test half remains, fed by #105 (the DLL-free ucrt64 port, "which this build is not" per the roadmap B6 rewrote).
- **Collector:** #163 (above).
- **Chore pool (opportunistic, unplanned):** #47 — bump `rerun-sdk` deliberately past the `rerun` extra's 0.39.x pin.
- **Axis epics #72–#77** stay open as the standing roadmap umbrellas; the research backlog below feeds them and is not phase work.
- **Research backlog (no proposition to schedule; each names its axis):** firmware/hardware fidelity — #2 (AM32 in the loop), #3 (ExpressLRS in the loop), #30 (DShot extended telemetry, MSP over CRSF); RF and cross-layer — #24 (ELRS dynamic power and hopping), #25 (frequency-dependent `rf_loss_db`), #26 (2.4 GHz interference), #62 (alternating handset antennas), #63 (UART contention), #64 (video reflections); receivers (help wanted — contribution bait, tracked "elsewhere" by #160's own body) — #5 Crossfire, #6 Tracer, #7 SBUS, #9 FrSky, #10 DSMX, #11 out-of-tree radio models; navigation — #69 (GPS and GPS Rescue), #70 (INAV/ArduPilot); library — #39 (stable plugin API), #41 (fcs/receivers/cameras part kinds); logging/perf — #60 (non-divisor recording rates), #46 (mcap off the git pin; blocked upstream); video — #32 (OSD font pages 2–3 and the HD/digital element set) and #33 (digital video systems), both parked with the digital orbit #160 explicitly defers.
- **Waiting on measured data (needs-data; #77's orbit):** #50 (thrust-stand comparison), #51 (simulated vs real Blackbox steps), #52 (the RPM-notch spectral proof).
- **Blocked:** #4 (real radio over serial, on a verified route), #46 (above).
