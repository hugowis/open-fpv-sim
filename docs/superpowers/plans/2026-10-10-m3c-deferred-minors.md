# M3c Deferred Minors Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Clear the 16 deferred minors the M3c final review triaged past merge — event counting, lever-arm accuracy, radio hot-loop allocations, test-robustness margins, NaN error attribution, and a handful of stale docs — with the two load-bearing ones (event counting, lever arms) implemented and pinned by tests.

**Architecture:** Five small tasks, grouped by crate, each independently green and committable. Two touch simulation semantics deliberately: `body.collision_count` becomes a true event counter (`events.len()`, the server's inequality edge detector is unaffected), and the impulse resolver recomputes its lever arms from the post-correction centre (removing the sub-cm approximation). Everything else is hygiene: a dead dependency, hot-loop hoisting, comment fixes, margin widows, and the HUD showing a dash instead of a bogus SNR while the link is down.

**Tech Stack:** Rust 1.85+ (workspace), Python 3.10+ test suite, Godot 4.7.2 GDScript tests.

**Spec:** No new spec — this plan addresses the deferred-minor list recorded in the M3c final whole-branch review (controller ledger, triaged "defer past merge") and mirrored in `docs/superpowers/m3c-carried-debt.md`. The M3c design spec (`docs/superpowers/specs/2026-10-10-m3c-world-collision-elrs-design.md`) remains the behavioural authority; nothing here changes specified behaviour.

## Global Constraints

- Rust edition 2021, `rust-version = "1.85"`; Python >= 3.10; Godot **4.7.2** only. License GPL-3.0-or-later.
- **Determinism:** same quad + world + seed + inputs in lockstep give identical output. The physics edits in Task 1 deliberately change impact arithmetic, so `same_seed_and_inputs_give_identical_runs`-style tests compare runs against each other (both change identically) — no cross-version digest is ever asserted anywhere in the suite.
- **No protocol, schema, or wire-format changes.** `PROTOCOL_VERSION` stays 5, quad schema 4, world schema 1. `body.collision_count`'s bus name and `State.collision_speed_mps` are untouched.
- Pristine test output: no warnings in any suite. Godot tests need `GODOT_BIN=<Godot_v4.7.2-stable_win64_console.exe> bash scripts/run-godot-tests.sh [unit|e2e|all]`; Python runs with `PYTHONPATH=python python -m pytest python/tests -q` after `cargo build -p ofs-sim --locked` (live SITL tests skip without `OFS_SITL_LAUNCH` and are NOT re-run by this plan — none of the minors touch the live path's behaviour).
- Every text file ends with a newline. Commit messages end with the trailer `Co-Authored-By: <the model of the session executing the task> <noreply@anthropic.com>`.
- Never use `pkill -f`; never run `wsl --shutdown`; do not touch Windows Firewall settings.
- Every edit below is "In `file`, replace: ... with: ...": the old text appears exactly once in the file at that point of the plan. Apply the edits of a step in the order given.

## Review Focus

Failure modes these small edits could introduce; each has a pinning test in the named task.

1. **Event counting vs the server's edge detector:** `Session::events_for` fires when the count *changes*, so a two-events-in-one-step jump must still raise exactly one protocol event, and existing single-event tests must stay green. Test: Task 1 (two-post same-tick count == 2; all existing count assertions unchanged).
2. **Lever-arm recompute changes impact numbers:** every existing analytic physics assertion (bounce apex ±5 %, the 30 m/s post, the rotated-attitude 1e-9 test, rest windows) must hold after the lever is computed from the post-correction centre; the rotated test's hand-derived expectation is updated deliberately and its comment re-derived. Test: Task 1.
3. **Buffer reuse must not perturb the draw stream:** the ELRS same-seed byte test and the draw-independence test must stay byte-identical after `rssi`/`downlink` become reused buffers and `tx_ant` is hoisted. Test: Task 2 (both existing tests re-run, unchanged).
4. **HUD format change must keep the healthy-link line byte-identical:** `"LINK UP   LQ 100 %   -50 dBm   SNR 49"` unchanged when up; only the down case gains `SNR –`. Test: Task 5 (test_hud.gd asserts both).
5. **NaN attribution must not weaken validation:** with a derived (unset) handset position the finite check is skipped (the pilot's own finite check already fired), but the below-ground check still applies to the effective position, and an explicitly given non-finite position is still reported. Tests: Task 3.

## File Structure

Modify only:
- `crates/ofs-physics/src/{collision.rs,rigid_body.rs}`, `crates/ofs-physics/tests/{collision.rs,rigid_body.rs}`, `crates/ofs-core/src/shape.rs`
- `crates/ofs-rf/Cargo.toml`, `crates/ofs-radio/src/elrs.rs`, `crates/ofs-core/src/names.rs`, `Cargo.lock`
- `crates/ofs-config/src/world.rs`, `crates/ofs-config/tests/world.rs`
- `crates/ofs-sim/tests/open_loop_vehicle.rs`, `python/tests/{test_client.py,test_sitl_link.py}`
- `godot/ui/hud.gd`, `godot/world/world.gd`, `godot/tests/test_hud.gd`
- `docs/superpowers/m3c-carried-debt.md`

---

### Task 1: Physics — count events, recompute levers after correction, comment fixes

**Files:**
- Modify: `crates/ofs-physics/src/collision.rs`, `crates/ofs-physics/src/rigid_body.rs`, `crates/ofs-physics/tests/collision.rs`, `crates/ofs-physics/tests/rigid_body.rs`, `crates/ofs-core/src/shape.rs`

**Interfaces:**
- Consumes: the merged M3c resolver (`resolve(p, bounds, bounding_radius_m, s, point_touches, touch, time_s)`, `struct Contact { normal, point, penetration, object, inward }`).
- Produces: `Contact` gains `centre_frd: DVec3` and `radius: f64` (private struct, no public change); `RigidBody.collision_events` counts raised events (`+= events.len()`); two `Contact` construction sites updated; one test's hand-derived expectation re-derived.

- [ ] **Step 1: Write the failing tests**

In `crates/ofs-physics/tests/collision.rs`, add:

```rust
/// Two posts side by side, two prop spheres flying into them symmetrically: both objects start
/// touching on the same tick, so one step raises two events.
#[test]
fn two_objects_touched_on_one_tick_count_two_events() {
    let objects = vec![
        post_at(5.0, -0.5, 0),
        post_at(5.0, 0.5, 1),
    ];
    let spheres = vec![(DVec3::new(0.5, -0.5, 0.0), 0.015), (DVec3::new(0.5, 0.5, 0.0), 0.015)];
    let mut p = params(objects, spheres, 0.3);
    p.contact_points_frd_m = vec![DVec3::new(0.0, 0.0, 0.03)];
    let mut s = sim(p, state(DVec3::new(0.0, 0.0, -1.5), DVec3::new(10.0, 0.0, 0.0)));
    s.run_for(0.6).unwrap();
    assert_eq!(scalar(&s, names::BODY_COLLISION_COUNT), 2.0, "one event per object, both this spell");
    assert!(scalar(&s, names::BODY_COLLISION_SPEED) > 9.0, "the reported speed is the touch's");
}

/// A post standing at `(north, east)`, with an explicit object index.
fn post_at(north: f64, east: f64, index: usize) -> WorldObject {
    let mut post = post(5.0, 0.0);
    post.name = format!("post{index}");
    WorldObject { name: post.name.clone(), shape: ofs_core::shape::Shape::Cylinder { center: DVec3::new(north, east, -1.5), radius: 0.06, half_height: 1.5 } }
}
```

(The file's existing `post(north, east)` helper is reused only for its shape — `post_at` builds both objects explicitly. If `scalar`/`params`/`state` carry different local names in the current file, use theirs.)

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ofs-physics --test collision two_objects -- --nocapture`
Expected: FAIL — `BODY_COLLISION_COUNT` reads `1.0` (the counter advances once per step), not `2.0`.

- [ ] **Step 3: Count the events, not the steps**

In `crates/ofs-physics/src/rigid_body.rs`, replace:

```rust
        if let Some((object, speed)) = events.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
            self.last_collision = Some((*object, *speed));
            self.collision_events += 1;
        }
```

with:

```rust
        if let Some((object, speed)) = events.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
            self.last_collision = Some((*object, *speed));
        }
        self.collision_events += events.len() as u64;
```

- [ ] **Step 4: Recompute the impulse lever arms from the post-correction centre**

In `crates/ofs-physics/src/collision.rs`, replace the `Contact` field block:

```rust
    /// The sphere's surface point along the normal (its deepest point into the solid).
    point: DVec3,
    penetration: f64,
```

with:

```rust
    /// The sphere's surface point along the normal (its deepest point into the solid), as of collection;
    /// the impulse loop recomputes it from the post-correction centre.
    point: DVec3,
    /// The sphere's centre in the body frame and its radius, to recompute `point` after position correction.
    centre_frd: DVec3,
    radius: f64,
    penetration: f64,
```

Replace both construction sites in `collect` (they differ only in `object`/`penetration` lines; show each in full):

```rust
            contacts.push(Contact { normal, point, penetration, object: GROUND_OBJECT_INDEX, inward });
```

with:

```rust
            contacts.push(Contact { normal, point, centre_frd: *centre_frd, radius: *radius, penetration, object: GROUND_OBJECT_INDEX, inward });
```

and:

```rust
            contacts.push(Contact { normal, point, penetration, object: i as i32, inward });
```

with:

```rust
            contacts.push(Contact { normal, point, centre_frd: *centre_frd, radius: *radius, penetration, object: i as i32, inward });
```

In `resolve`'s impulse loop, replace:

```rust
    for c in &contacts {
        let lever = c.point - s.pos_ned_m;
```

with:

```rust
    for c in &contacts {
        // The lever arm runs from the post-correction centre to the surface point, recomputed for the
        // corrected body position (the collection-time point is up to one penetration stale).
        let point = s.pos_ned_m + s.att * c.centre_frd - c.normal * c.radius;
        let lever = point - s.pos_ned_m;
```

- [ ] **Step 5: Update the rotated-attitude test's hand-derived lever**

The merged test (`a_rotated_hit_turns_the_body_through_the_body_frame_inertia` in `crates/ofs-physics/tests/collision.rs`) already derives its lever from the post-correction centre using the STALE collection-time surface point. After Step 4 the lever loses the penetration along the normal. Apply exactly these edits:

In the comment block, replace:

```rust
    // Hand-computed, with inv_mass = 20/13, inv_inertia = (400, 400, 2000/9), e = 0.3. The position correction
    // (0.01 along -x) leaves the lever at point - pos = (0.13, -0.03, 0.06), and the normal impulse is J = n*j:
```

with:

```rust
    // Hand-computed, with inv_mass = 20/13, inv_inertia = (400, 400, 2000/9), e = 0.3. The lever is recomputed
    // from the post-correction centre: point_new = pos_new + R*centre_frd - n*r = (0, 0, -0.1), so lever =
    // point_new - pos_new = (0.12, -0.03, 0.06), and the normal impulse is J = n*j:
```

and replace:

```rust
    let lever = DVec3::new(0.13, -0.03, 0.06);
```

with:

```rust
    let lever = DVec3::new(0.12, -0.03, 0.06);
```

Everything else in the test stays: `lever × n` is unchanged at `(0, −0.06, −0.03)` (the lever's x-component does not contribute against the −x normal), so `kn = 20/13 + 1.16` and `j = 1.3/kn` are invariant, and the expected-rate line keeps its structure — `expected = diag(I⁻¹)·(Rᵀ·(lever × J_total))` to 1e-9, with `J_total = m·Δv` recovered from the attitude-free linear update. The discriminating property survives: with the Task 4 inertia bug reintroduced, the buggy form gives `(0, −6.67 j, 24 j)` — a different direction, so the 1e-9 assert still fails.

Run: `cargo test -p ofs-physics --test collision --locked`
Expected: the two-post test PASSES; the rotated test passes with the re-derived lever; every other collision test passes unchanged (the ±5 % bounce window absorbs the sub-cm lever change; if any test fails, fix the test's derivation, never the tolerance).

- [ ] **Step 6: Comment fixes (no behaviour)**

In `crates/ofs-physics/tests/rigid_body.rs`, replace:

```rust
    // The legs (0.03 below the CoM) carry it 2 mm into the surface (mg/k), never through it.
```

with:

```rust
    // The legs (0.03 below the CoM) carry it ~0.5 mm into the surface (mg/(4k): four feet share the weight),
    // never through it.
```

In `crates/ofs-core/src/shape.rs`, replace (box branch):

```rust
                n.try_normalize().unwrap_or(DVec3::NEG_Z)
```

with:

```rust
                // Unreachable defensive fallback: the chosen axis always gets ±1 (glam's signum maps ±0.0 to ±1.0).
                n.try_normalize().unwrap_or(DVec3::NEG_Z)
```

and (cylinder branch):

```rust
                    cap.try_normalize().unwrap_or(DVec3::NEG_Z)
```

with:

```rust
                    // Unreachable defensive fallback: signum(±0.0) is ±1, so `cap` is never zero-length.
                    cap.try_normalize().unwrap_or(DVec3::NEG_Z)
```

- [ ] **Step 7: Run the physics suite and the workspace**

Run: `cargo test -p ofs-physics -p ofs-core --locked && cargo test --workspace --locked`
Expected: PASS, no warnings.

- [ ] **Step 8: Commit**

```bash
git add crates/ofs-physics crates/ofs-core/src/shape.rs
git commit -m "fix(physics): collision_count counts events, impulse levers use the post-correction centre

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 2: Radio — drop the dead `rand` dep, hoist and reuse in the hot loop, fix the fault doc

**Files:**
- Modify: `crates/ofs-rf/Cargo.toml`, `crates/ofs-radio/src/elrs.rs`, `crates/ofs-core/src/names.rs`, `Cargo.lock`

**Interfaces:**
- Consumes: the merged `ElrsLink` (buffers `rssi`/`downlink` are new private fields; no public change).
- Produces: no API change; the draw stream must be byte-identical (the same-seed and draw-independence tests re-run unchanged).

- [ ] **Step 1: Remove the unused `rand` dependency from `ofs-rf`**

Verify it is unused first: `grep -rn "use rand::\|rand::" crates/ofs-rf/src/` must come back empty. In `crates/ofs-rf/Cargo.toml`, replace:

```toml
[dependencies]
glam.workspace = true
ofs-core.workspace = true
rand.workspace = true
rand_chacha.workspace = true
rand_distr.workspace = true
```

with:

```toml
[dependencies]
glam.workspace = true
ofs-core.workspace = true
rand_chacha.workspace = true
rand_distr.workspace = true
```

- [ ] **Step 2: Hoist the loop-invariant handset antenna and reuse the RSSI buffers in `ElrsLink`**

In `crates/ofs-radio/src/elrs.rs`, add two private fields to `ElrsLink` (after `handset_faders`):

```rust
    quad_faders: Vec<Fader>,
    handset_faders: Vec<Fader>,
    /// Reused per-packet scratch: the per-antenna RSSI vectors (no allocation in the hot loop).
    rssi: Vec<f64>,
    downlink: Vec<f64>,
    last_pos: Option<DVec3>,
```

in the `Self { ... }` literal of `new`, after `handset_faders,` add:

```rust
            rssi: vec![NO_SIGNAL_RSSI_DBM; params.quad_antennas.len()],
            downlink: vec![NO_SIGNAL_RSSI_DBM; params.handset_antennas.len()],
```

In `step`, replace:

```rust
        let mut rssi = vec![NO_SIGNAL_RSSI_DBM; self.params.quad_antennas.len()];
        for j in 0..self.params.quad_antennas.len() {
            let configured = self.params.quad_antennas[j];
            let rx_ant = Antenna { axis: (att * configured.axis).normalize(), ..configured };
            let tx_ant = self.params.handset_antennas[self.handset_diversity.active().min(self.params.handset_antennas.len() - 1)];
```

with:

```rust
        // The handset transmits on its active antenna: loop-invariant across the quad's antennas.
        let tx_ant = self.params.handset_antennas[self.handset_diversity.active().min(self.params.handset_antennas.len() - 1)];
        self.rssi.clear();
        self.rssi.resize(self.params.quad_antennas.len(), NO_SIGNAL_RSSI_DBM);
        let rssi = &mut self.rssi;
        for (j, configured) in self.params.quad_antennas.iter().enumerate() {
            let rx_ant = Antenna { axis: (att * configured.axis).normalize(), ..*configured };
```

and inside the same loop, replace:

```rust
            rssi[j] = (mw_to_dbm(self.params.tx_power_mw) + path.gain_db + fade - shadow).max(NO_SIGNAL_RSSI_DBM);
```

with:

```rust
            rssi[j] = (mw_to_dbm(self.params.tx_power_mw) + path.gain_db + fade - shadow).max(NO_SIGNAL_RSSI_DBM);
```

(no change — `rssi` is now the reborrowed slice; the loop body's other lines are untouched). After the loop, replace:

```rust
        let active = self.quad_diversity.choose(&rssi);
```

with:

```rust
        let active = self.quad_diversity.choose(rssi);
```

and replace the downlink block's opening:

```rust
        let mut downlink = vec![NO_SIGNAL_RSSI_DBM; self.params.handset_antennas.len()];
        for a in 0..self.params.handset_antennas.len() {
            let rx_ant = self.params.handset_antennas[a];
```

with:

```rust
        self.downlink.clear();
        self.downlink.resize(self.params.handset_antennas.len(), NO_SIGNAL_RSSI_DBM);
        let downlink = &mut self.downlink;
        for (a, rx_ant) in self.params.handset_antennas.iter().enumerate() {
```

and inside it, replace:

```rust
            downlink[a] = (mw_to_dbm(DOWNLINK_TX_POWER_MW) + path.gain_db + fade - shadow).max(NO_SIGNAL_RSSI_DBM);
```

with:

```rust
            downlink[a] = (mw_to_dbm(DOWNLINK_TX_POWER_MW) + path.gain_db + fade - shadow).max(NO_SIGNAL_RSSI_DBM);
```

(no change, same reborrow), and after that loop replace:

```rust
        let downlink_active = self.handset_diversity.choose(&downlink);
```

with:

```rust
        let downlink_active = self.handset_diversity.choose(downlink);
```

Finally, replace the statistics block's uses of the owned vectors with the reborrowed slices — they are the same bindings, so only borrow-dependent lines change. If the borrow checker objects to `self.quad_faders[j].next(&mut self.rng, ...)` while `rssi = &mut self.rssi` is live: it should not (disjoint fields), but if it does, scope the slice:

```rust
        let rssi: &mut Vec<f64> = &mut self.rssi;
```

and drop it with `drop(rssi);` before any line that needs `&mut self` beyond the disjoint fields. Do NOT clone the vectors back.

- [ ] **Step 3: Fix the fault doc**

In `crates/ofs-core/src/names.rs`, replace:

```rust
/// Fault: 1 = every uplink packet is lost.
pub const FAULT_RADIO_LINK_LOSS: &str = "fault.radio.link_loss";
```

with:

```rust
/// Fault: 1 = every packet is lost, uplink and downlink alike (the receiver and the handset both go silent).
pub const FAULT_RADIO_LINK_LOSS: &str = "fault.radio.link_loss";
```

- [ ] **Step 4: Run the radio and workspace suites**

Run (first without `--locked` so Cargo.lock drops `rand` from ofs-rf):
`cargo test -p ofs-rf -p ofs-radio --locked && cargo test --workspace --locked`
Expected: PASS — in particular `the_same_seed_gives_the_same_packet_stream` and `the_loss_draws_do_not_depend_on_the_transmitter` pass UNCHANGED (the draw order is untouched), no warnings.

- [ ] **Step 5: Commit**

```bash
git add Cargo.lock crates/ofs-rf/Cargo.toml crates/ofs-radio/src/elrs.rs crates/ofs-core/src/names.rs
git commit -m "refactor(radio): no per-packet allocations in the ELRS hot loop, the loop-invariant tx antenna hoisted, rand dropped from ofs-rf; the link-loss fault doc covers the downlink

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 3: Config — NaN pilot height no longer misattributes the handset error

**Files:**
- Modify: `crates/ofs-config/src/world.rs`, `crates/ofs-config/tests/world.rs`

**Interfaces:**
- Consumes: `HandsetSection.position_ned_m: Option<[f64; 3]>` and the merged validation block.
- Produces: with an unset handset position and a NaN pilot height, the error reports `pilot.position_ned_m` only (no `handset.position_ned_m` "values must be finite"); an explicitly given non-finite position is still reported; the below-ground check still applies to the effective position.

- [ ] **Step 1: Write the failing test**

In `crates/ofs-config/tests/world.rs`, add:

```rust
#[test]
fn a_nan_pilot_height_reports_the_pilot_not_the_derived_handset() {
    let err = written_world(
        r#"schema_version = 1
name = "nan pilot"
[pilot]
position_ned_m = [-3.0, 2.0, nan]
[receiver.antennas]
name = "omni"
kind = "omni"
gain_dbi = 2.0
polarization = "rhcp"
"#,
    )
    .unwrap_err();
    let message = err.to_string();
    assert!(message.contains("pilot.position_ned_m"), "{message}");
    assert!(!message.contains("handset.position_ned_m"), "the derived position is not the fault: {message}");
}
```

(Use the file's existing world-writing helper if `written_world` was named differently when Task 5 of M3c landed — keep one helper.)

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ofs-config --test world a_nan_pilot --locked`
Expected: FAIL — the message contains `handset.position_ned_m: values must be finite` (derived from the NaN pilot).

- [ ] **Step 3: Skip the finite check for the derived position**

In `crates/ofs-config/src/world.rs`, replace:

```rust
        let h = &self.handset;
        let handset_at = h.position(&p);
        c.check(handset_at.iter().all(|v| v.is_finite()), "handset.position_ned_m", "values must be finite");
```

with:

```rust
        let h = &self.handset;
        let handset_at = h.position(&p);
        // A given position is the handset's own fault; a derived one inherits the pilot's, whose finite
        // check already fired — report the pilot only.
        if h.position_ned_m.is_some() {
            c.check(handset_at.iter().all(|v| v.is_finite()), "handset.position_ned_m", "values must be finite");
        }
```

(The below-ground check on the effective position stays exactly as it is — NaN is exempt there by the existing `is_nan() ||` arm, and a legitimately derived above-ground handset still reports.)

- [ ] **Step 4: Run the config suite and the workspace**

Run: `cargo test -p ofs-config --locked && cargo test --workspace --locked`
Expected: PASS — the new test passes, the pre-existing NaN-pilot test and the bare-`[handset]` guard test are unchanged and green.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-config
git commit -m "fix(config): a NaN pilot height no longer reports the derived handset position as non-finite

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 4: Test hygiene — margins with derived windows, path derivation, dead imports, sweep planes

**Files:**
- Modify: `crates/ofs-sim/tests/open_loop_vehicle.rs`, `python/tests/test_client.py`, `python/tests/test_sitl_link.py`, `crates/ofs-core/tests/shape.rs`

**Interfaces:**
- Consumes: the merged tests as they stand.
- Produces: no API change; the drop heights move to the middle of their mathematically derivable windows, with the windows derived in comments.

- [ ] **Step 1: Centre the drop heights in their derivable window**

Both collision e2e tests are bounded by the event mechanics: the impact must exceed the 1 m/s event threshold comfortably and the `> 3.0` assertion, while the restitution-0.3 rebound must stay below 1 m/s so exactly one event fires (`collision_speed_mps` carries the LAST event's speed). The falling body touches on its **legs** (3 cm below the CoM), so a drop of `d` gives a fall of `d − 0.03` and impact `v = √(2·g·(d−0.03))`; `v > 3.0` and `0.3·v < 1` bound `v ∈ (3.0, 3.33)`, i.e. `d ∈ (0.489, 0.596)`. The midpoint fall `0.51` gives `d = 0.54` — both margins ~5 % of the window, symmetric.

In `crates/ofs-sim/tests/open_loop_vehicle.rs`, replace:

```rust
    // Open loop cannot steer, so the quad falls 1.5 m onto BuildingA's roof (its top is the plane z = -14).
    let mut cfg = load(Path::new(QUAD)).unwrap();
    cfg.initial.position_ned_m = [110.0, -45.0, -15.5];
```

with:

```rust
    // Open loop cannot steer, so the quad falls onto BuildingA's roof (its top is the plane z = -14).
    // Drop 0.54 m is the midpoint of the one-event window: the legs touch after falling 0.51 m, so the
    // impact sqrt(2*g*0.51) = 3.16 m/s clears the 3.0 assertion and the 0.3-restitution rebound (0.95)
    // stays under the 1 m/s event threshold (collision_speed_mps carries the LAST event's speed, so a
    // second event would break it).
    let mut cfg = load(Path::new(QUAD)).unwrap();
    cfg.initial.position_ned_m = [110.0, -45.0, -14.54];
```

In `python/tests/test_client.py`, replace:

```python
        "position_ned_m = [0.0, 0.0, -0.03]", "position_ned_m = [110.0, -45.0, -14.55]")
```

with:

```python
        "position_ned_m = [0.0, 0.0, -0.03]", "position_ned_m = [110.0, -45.0, -14.54]")
```

(and extend the test's existing drop-height comment, if it has one, with the window derivation above).

- [ ] **Step 2: Derive the diff path instead of hardcoding it**

In `python/tests/test_client.py`, replace:

```python
    shutil.copy(pathlib.Path(QUAD).with_name("opendrone-5f-freestyle.betaflight.diff"), tmp_path)
```

with:

```python
    shutil.copy(pathlib.Path(QUAD).with_suffix(".betaflight.diff"), tmp_path)
```

(`QUAD` is `.../opendrone-5f-freestyle.toml`, so `with_suffix` yields the sibling diff; there is one such call per test — replace both if the file has two.)

- [ ] **Step 3: Remove the dead import and the double world load**

In `python/tests/test_sitl_link.py`, replace:

```python
import pytest

import ofs
from conftest import QUAD
```

with:

```python
import pytest

from conftest import QUAD
```

In `crates/ofs-sim/tests/open_loop_vehicle.rs`, replace:

```rust
    let building_a = flat_world().objects.iter().position(|o| o.name == "BuildingA").unwrap() as i32;
    let mut v = vehicle_in(&cfg, flat_world());
```

with:

```rust
    let world = flat_world();
    let building_a = world.objects.iter().position(|o| o.name == "BuildingA").unwrap() as i32;
    let mut v = vehicle_in(&cfg, world);
```

- [ ] **Step 4: Sweep the shape normals at three heights**

In `crates/ofs-core/tests/shape.rs`, replace:

```rust
    for shape in [BOX, CYL] {
        let (lo, hi) = (shape.bounds().min, shape.bounds().max);
        for i in 0..20 {
            for j in 0..20 {
                let p = DVec3::new(
                    lo.x + (hi.x - lo.x) * i as f64 / 19.0,
                    lo.y + (hi.y - lo.y) * j as f64 / 19.0,
                    lo.z - 1.0,
                );
```

with:

```rust
    for shape in [BOX, CYL] {
        let (lo, hi) = (shape.bounds().min, shape.bounds().max);
        // One plane above the top, one through the middle (beside the sides), one below the bottom.
        for z in [lo.z - 1.0, (lo.z + hi.z) * 0.5, hi.z + 1.0] {
        for i in 0..20 {
            for j in 0..20 {
                let p = DVec3::new(
                    lo.x + (hi.x - lo.x) * i as f64 / 19.0,
                    lo.y + (hi.y - lo.y) * j as f64 / 19.0,
                    z,
                );
```

and close the extra loop by replacing (the same test's tail):

```rust
                assert!(step >= -1e-9, "gradient points inward at {p}");
            }
        }
    }
}
```

with:

```rust
                assert!(step >= -1e-9, "gradient points inward at {p}");
            }
        }
        }
    }
}
```

(Keep the file's rustfmt-compatible indentation when applying — the doubled braces close the new `for z` loop.)

- [ ] **Step 5: Run everything**

Run: `cargo test -p ofs-core -p ofs-sim --locked && cargo build -p ofs-sim --locked && PYTHONPATH=python python -m pytest python/tests -q`
Expected: PASS — python 28 passed (13 live-skipped), Rust green, no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/ofs-sim/tests/open_loop_vehicle.rs crates/ofs-core/tests/shape.rs python/tests/test_client.py python/tests/test_sitl_link.py
git commit -m "test: drop heights centred in their one-event windows with derived bounds, diff path derived from QUAD, dead import gone, shape sweep at three heights

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 5: Godot and the debt doc — the SNR dash, two comment fixes, one closed finding

**Files:**
- Modify: `godot/ui/hud.gd`, `godot/world/world.gd`, `godot/tests/test_hud.gd`, `docs/superpowers/m3c-carried-debt.md`

**Interfaces:**
- Consumes: the merged HUD (`link_text()`, `test_the_link_line_carries_the_snr`), the merged world dict.
- Produces: with the link down the HUD shows `SNR –` instead of the no-signal number; the healthy-link line is byte-identical to today's.

- [ ] **Step 1: Write the failing Godot test**

In `godot/tests/test_hud.gd`, replace:

```gdscript
func test_the_link_line_carries_the_snr() -> void:
	var hud := await _hud()
	hud.update_view(_view())
	eq(hud.link_text(), "LINK UP   LQ 100 %   -50 dBm   SNR 49", "the link line")
	hud.update_view(_view({"telemetry": _telemetry({"link_up": false})}))
	ok(hud.link_text().begins_with("LINK DOWN"), "down: '%s'" % hud.link_text())
	hud.queue_free()
```

with:

```gdscript
func test_the_link_line_carries_the_snr() -> void:
	var hud := await _hud()
	hud.update_view(_view())
	eq(hud.link_text(), "LINK UP   LQ 100 %   -50 dBm   SNR 49", "the link line")
	hud.update_view(_view({"telemetry": _telemetry({"link_up": false})}))
	ok(hud.link_text().begins_with("LINK DOWN"), "down: '%s'" % hud.link_text())
	ok(hud.link_text().contains("SNR –"), "down: no bogus SNR number ('%s')" % hud.link_text())
	hud.queue_free()
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `GODOT_BIN=<Godot_v4.7.2-stable_win64_console.exe> bash scripts/run-godot-tests.sh unit`
Expected: FAIL — the down line still reads `SNR -21`.

- [ ] **Step 3: The dash**

In `godot/ui/hud.gd`, replace:

```gdscript
		var up: bool = t["link_up"] and t["tx_enabled"]
		_link.text = "LINK %s   LQ %d %%   %d dBm   SNR %d" % ["UP" if up else "DOWN", t["lq_pct"], t["rssi_dbm"], roundi(t.get("radio_snr_db", 0.0))]
```

with:

```gdscript
		var up: bool = t["link_up"] and t["tx_enabled"]
		var snr: String = str(roundi(t.get("radio_snr_db", 0.0))) if up else "–"
		_link.text = "LINK %s   LQ %d %%   %d dBm   SNR %s" % ["UP" if up else "DOWN", t["lq_pct"], t["rssi_dbm"], snr]
```

- [ ] **Step 4: The two world.gd comment/verification items**

In `godot/world/world.gd`, replace:

```gdscript
## The pilot's handset: a small dark transmitter on the ground beside the pilot, 0.5 m below the goggles.
```

with:

```gdscript
## The pilot's handset: a small dark transmitter, placed at the world dict's handset_position (the config
## default sits it 0.5 m below the goggles).
```

Do NOT touch the `world.has("handset_position")` guard: on verification it is load-bearing — `build()` also receives an EMPTY dictionary (the field-clear path), where the guard correctly skips. Record that closure in the debt doc (Step 5) instead of changing code.

- [ ] **Step 5: The debt doc**

In `docs/superpowers/m3c-carried-debt.md`:
- Fix the dangling-verb bullet (the one beginning "the Python collision test's hardcoded diff name and holds ~5 % margins") to read: "the Python collision test hardcodes the diff's file name and holds ~5 % margins" — or delete the bullet outright if Task 4 has already fixed both (it has): delete it.
- Delete the bullets this plan resolved (the minor-leftovers items that no longer hold after Tasks 1-4: the event-count semantics, the lever-arm approximation if listed, the `rand` dependency, the per-packet allocations, the NaN attribution, the unused import, the `mg/k` comment).
- Add one line under "Rulings made while planning": "The `world.has(\"handset_position\")` guard in `godot/world/world.gd` was audited and kept: it serves the empty-dictionary clear path; the HUD shows `SNR –` while the link is down."

- [ ] **Step 6: Run the Godot suites**

Run: `GODOT_BIN=<Godot_v4.7.2-stable_win64_console.exe> bash scripts/run-godot-tests.sh all`
Expected: PASS — unit checks green including the updated link-line test, e2e green (the e2e asserts toast text, not the link line).

- [ ] **Step 7: Commit**

```bash
git add godot docs/superpowers/m3c-carried-debt.md
git commit -m "fix(ui): the HUD shows SNR - while the link is down; handset doc placement clarified; the world-dict guard audited and kept

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

## Coverage of the deferred-minor list

| Minor | Where |
|---|---|
| `collision_count` counts steps, not events | Task 1 (+ two-post test) |
| Lever arms stale after position correction | Task 1 (+ rotated-test re-derivation) |
| Unreachable `NEG_Z` fallbacks undocumented | Task 1 (comments) |
| `mg/k` test comment | Task 1 |
| `ofs-rf` keeps an unused `rand` dep | Task 2 |
| Per-packet allocations + loop-invariant hoist in `ElrsLink::step` | Task 2 |
| `FAULT_RADIO_LINK_LOSS` doc ignores the downlink | Task 2 |
| NaN pilot height misattributes the handset error | Task 3 (+ test) |
| Python/Rust drop-height margins ~5 % | Task 4 (mid-window 0.51 m, derived) |
| Hardcoded betaflight.diff name | Task 4 (`with_suffix`) |
| Unused `import ofs` in test_sitl_link.py | Task 4 |
| `flat_world()` loaded twice | Task 4 |
| Shape sweep samples one plane | Task 4 (three heights) |
| HUD shows a bogus SNR number when down | Task 5 (+ test) |
| `_add_handset` doc mentions the offset | Task 5 |
| `world.gd` handset guard "dead" | Task 5 — closed after audit: it guards the empty-dictionary clear path; recorded in the debt doc, no code change |
| T10 dangling-verb debt bullet | Task 5 |

## Self-Review

- **Coverage:** every ledger minor has a row above; nothing from the final review's triage is unaddressed.
- **No placeholders:** every edit carries its exact old/new text; the two places where the plan defers to the file's existing helper names (`written_world`, the collision-test locals) say so explicitly and bound the adaptation.
- **Type consistency:** `Contact.centre_frd`/`radius` match `collect`'s loop variables (`*centre_frd`, `*radius` deref the existing tuple bindings); `events.len() as u64` matches `collision_events: u64`; the GDScript ternary yields a String for a `%s` slot.
- **Review Focus:** each of the five failure modes has its pinning test named above.
