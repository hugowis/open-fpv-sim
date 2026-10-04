# M1 — Headless Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A headless Rust simulator (`ofs-sim`) that steps physics, electrical and sensor models at 8 kHz in lockstep with real Betaflight SITL, driven from Python over gRPC — exit criterion: a Python script arms the quad and holds a 1 m hover.

**Architecture:** A Cargo workspace of small crates around `ofs-core` (typed signal bus, `Model` trait, multi-rate scheduler). Every simulated part is a `Model` that reads and writes named bus signals. `ofs-sim` assembles a quad from a TOML file into a scheduler, adds either the Betaflight SITL bridge or an open-loop stand-in FC, and serves a gRPC API that the `ofs` Python package wraps.

**Tech Stack:** Rust stable (≥ 1.80), glam 0.29 (f64), rand 0.8 / rand_chacha 0.3 / rand_distr 0.4, serde + toml 0.8, thiserror 1, tonic 0.12 / prost 0.13 / tokio 1, clap 4, tracing; Python ≥ 3.10 with grpcio, protobuf, pytest; Betaflight SITL 2026.6.2.

**Spec:** `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`
**M0 findings (read before Task 10):** `docs/research/sitl-interface.md`

## Global Constraints

- License: GPL-3.0-or-later (code). Every crate's `Cargo.toml` uses `license.workspace = true`.
- Platforms: Windows and Linux. Rust code must build and test on both; SITL runs natively on Linux and under WSL2 on Windows.
- Toolchain: Rust stable ≥ 1.80 (Cargo workspace), Python ≥ 3.10.
- Repository layout: `crates/` (Rust workspace), `python/` (client package), `quads/`, `proto/`, `docs/`. Crate prefix `ofs-`.
- Frames: world **NED**, body **FRD**, quaternions **body→world**. SI units; field and signal names carry a unit suffix (`_m`, `_mps`, `_radps`, `_n`, `_nm`, `_a`, `_v`, `_pa`, `_kg`, …).
- Default base tick 8 kHz; every model's rate is an integer divisor of the base tick.
- Determinism: same quad + seed + inputs ⇒ identical bus digest for runs without SITL. All randomness comes from `ofs_core::rng::model_rng(seed, model_name)`.
- Simulator failures (`SimError`) stop the run loudly and are never presented as drone behaviour.
- `SimError` variants are fixed: `Firmware(String)`, `NonFinite(String)`, `InvalidArgument(String)`, `Other(String)`.
- Not in M1 (later milestones): real-time mode, CRSF/ELRS, Godot, video, MCAP/Rerun, repro bundles, fault injection, motor vibration, wind/turbulence, the MSP API-version CI check (M2, with the Configurator).

## M0 results (already applied to Tasks 8, 10 and 11)

M0 is complete; the evidence is in `docs/research/sitl-interface.md` (§7 has the full assumption audit). The SITL facts this plan relies on:

- **SITL build:** `bash scripts/build-sitl.sh` (Linux or WSL2) builds Betaflight 2026.6.2, pinned commit `e0b7bb0`, with `third_party/betaflight/ofs-sitl.patch` and `-DENABLE_GAZEBO_BRIDGE=0 -DENABLE_SIMULATOR_EXTERNAL_TIME=1`. Binary: `obj/main/betaflight_SITL.elf`. `--config <file>` writes `eeprom.bin` in the working directory and exits. stdout is line-buffered.
- **Lockstep protocol:** send **one 184-byte datagram** (`fdm_packet` 144 B ‖ `rc_packet` 40 B) to UDP **9003** per exchange; read exactly **one** 16-byte `servo_packet` from UDP **9002**. Never send to 9004.
  - Exchange rate = Betaflight's virtual gyro rate = **1000 Hz**.
  - `motor_speed` ∈ [0, 1]: 0 when disarmed, 0.055 at armed idle.
  - Same inputs ⇒ bit-identical motor outputs.
- **Sign mapping** (verified 8/8 by motor responses):
  - gyro sent as FRD `(ωx, ωy, ωz)` **unchanged**;
  - accel sent as `(−fx, fy, fz)` of the FRD specific force;
  - quaternion `Rx(π)·q_ned·Rx(π)` with w ≥ 0 (only feeds the compass; unverified, unused in acro/angle);
  - velocity ENU; `position_xyz` = (lon°, lat°, alt m); pressure in Pa.
- **Startup:** TCP 5761 accepting does not mean the main loop is running. The first reply needs a **generous timeout** (default 5 s); later replies use the normal timeout (500 ms). A line containing `bind port` and `failed` in SITL's output means a stale SITL is holding ports: **startup error**.
- **Arming config:** unchanged. ARM on AUX1, ANGLE on AUX2, `motor_pwm_protocol = PWM`, `small_angle = 180`. Keep `pid_process_denom = 1`.
- **Windows (WSL2, default NAT networking):**
  - When `launch[0]` is `wsl.exe`, the bridge discovers the WSL VM IP (`hostname -I`, first field) and the Windows host IP as seen from WSL (default route).
  - It appends `--ip <host IP>` to SITL's argv, sends state datagrams to the VM IP, and binds the motor socket on the host IP.
  - **Cleanup is required:** default `<wsl prefix> pkill -x betaflight_SITL`, run before every launch and after stop.
  - Overrides: `OFS_SITL_HOST` (send address), `OFS_SITL_REPLY_IP` (`--ip` and bind address).
- **Time origin:** SITL's clock never runs backwards, so every `Load` launches a fresh SITL (already the design).

## Review Focus

- **SITL launch command wrong or binary missing** (typical on Windows with a WSL path): startup must fail with an error naming the command, not hang. Test: Task 11 `launch_failure_names_the_command`.
- **Ports held by a stale SITL or a second simulator**: a held UDP 9002 must fail with an error naming the port (Test: Task 11 `busy_pwm_port_is_reported`), and a SITL that reports `bind port … failed` must fail startup instead of letting the simulator talk to the stale instance (Test: Task 11 `bind_failure_lines_are_recognised`).
- **Quad file loaded from a different working directory**: `fc.betaflight_diff` must resolve relative to the quad file. Test: Task 8 `diff_path_resolves_relative_to_quad_file`.
- **Scripts sending bad values** (negative/NaN run duration, NaN sticks): reject with InvalidArgument and keep the session usable. Tests: Task 12 `invalid_run_durations_do_not_poison_the_session`, `non_finite_sticks_are_rejected`.
- **Client and server protocol versions differ**: typed ProtocolMismatch error. Tests: Task 12 `handshake_checks_protocol_version`, Task 13 `test_protocol_mismatch_is_typed`.

## File Structure

```
Cargo.toml                         workspace
LICENSE                            GPL-3.0 text
proto/ofs/v1/sim.proto             gRPC API
quads/opendrone-5f-freestyle.toml  reference quad (M1 estimates)
quads/opendrone-5f-freestyle.betaflight.diff
crates/ofs-core/src/{lib,bus,interp,rng,model,scheduler,names,consts}.rs
crates/ofs-physics/src/{lib,rigid_body,propeller}.rs
crates/ofs-electrical/src/{lib,battery,esc_motor}.rs
crates/ofs-sensors/src/{lib,imu,baro}.rs
crates/ofs-config/src/lib.rs
crates/ofs-fc/src/{lib,open_loop}.rs, crates/ofs-fc/src/sitl/{mod,codec,frames,process,bridge}.rs
crates/ofs-sim/{build.rs, src/{lib,vehicle,server,main}.rs}
python/pyproject.toml, python/ofs/{__init__,client,errors}.py, python/ofs/v1/ (generated)
python/tests/{conftest,test_client,test_sitl_hover}.py, python/examples/hover.py
docs/dev-setup.md, README.md, .github/workflows/ci.yml
```

---

### Task 1: Workspace and `ofs-core` signal bus

**Files:**
- Create: `Cargo.toml`, `LICENSE`, `.gitignore` (if M0 did not create it)
- Create: `crates/ofs-core/Cargo.toml`, `crates/ofs-core/src/lib.rs`, `crates/ofs-core/src/bus.rs`, `crates/ofs-core/src/interp.rs`, `crates/ofs-core/src/rng.rs`
- Test: `crates/ofs-core/tests/bus.rs`

**Interfaces:**
- Produces:
  - `ofs_core::Bus` — `new()`, `signal<T: BusValue>(&mut self, name: &str) -> Signal<T>` (registers or returns existing; panics on type mismatch with "already registered"), `get<T>(&self, Signal<T>) -> T`, `set<T>(&mut self, Signal<T>, T)`, `lookup<T>(&self, name) -> Option<Signal<T>>`, `digest(&self) -> u64`, `first_non_finite(&self) -> Option<&str>`.
  - `ofs_core::Signal<T>` (Copy), `ofs_core::BusValue` (implemented for `f64`, `glam::DVec3`, `glam::DQuat`), `ofs_core::SignalKind`.
  - `ofs_core::interp::linear(table: &[(f64, f64)], x: f64) -> f64`.
  - `ofs_core::rng::{fnv1a64(&[u8]) -> u64, Fnv1a, model_rng(seed: u64, model_name: &str) -> rand_chacha::ChaCha8Rng}`.

- [ ] **Step 1: Create the workspace manifest**

`Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "GPL-3.0-or-later"
rust-version = "1.80"

[workspace.dependencies]
glam = "0.29"
rand = "0.8"
rand_chacha = "0.3"
rand_distr = "0.4"
serde = { version = "1", features = ["derive"] }
toml = "0.8"
thiserror = "1"
tonic = "0.12"
prost = "0.13"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net"] }
tokio-stream = { version = "0.1", features = ["net"] }
tonic-build = "0.12"
protoc-bin-vendored = "3"
clap = { version = "4", features = ["derive"] }
tracing = "0.1"
tracing-subscriber = "0.3"
tempfile = "3"
ofs-core = { path = "crates/ofs-core" }
ofs-physics = { path = "crates/ofs-physics" }
ofs-electrical = { path = "crates/ofs-electrical" }
ofs-sensors = { path = "crates/ofs-sensors" }
ofs-config = { path = "crates/ofs-config" }
ofs-fc = { path = "crates/ofs-fc" }
```

Add the license text (this downloads a file; ask the user first): `curl -sSL https://www.gnu.org/licenses/gpl-3.0.txt -o LICENSE`. If `.gitignore` does not exist yet, create it with:
```gitignore
/target/
/.ofs-data/
/spikes/m0/runs/
__pycache__/
*.egg-info/
.venv/
```

- [ ] **Step 2: Create the crate manifest and module skeleton**

`crates/ofs-core/Cargo.toml`:
```toml
[package]
name = "ofs-core"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
glam.workspace = true
rand.workspace = true
rand_chacha.workspace = true
thiserror.workspace = true
```

`crates/ofs-core/src/lib.rs`:
```rust
//! Open FPV Sim core: typed signal bus, models and scheduler. Deterministic, no I/O.
pub mod bus;
pub mod interp;
pub mod rng;

pub use bus::{Bus, BusValue, Signal, SignalKind};
```

- [ ] **Step 3: Write the failing tests**

`crates/ofs-core/tests/bus.rs`:
```rust
use glam::{DQuat, DVec3};
use ofs_core::{interp, rng, Bus, Signal};
use rand::Rng;

#[test]
fn registers_and_reads_back_typed_signals() {
    let mut bus = Bus::new();
    let a: Signal<f64> = bus.signal("a");
    let v: Signal<DVec3> = bus.signal("v");
    let q: Signal<DQuat> = bus.signal("q");
    assert_eq!(bus.get(a), 0.0);
    assert_eq!(bus.get(q), DQuat::IDENTITY);
    bus.set(a, 2.5);
    bus.set(v, DVec3::new(1.0, 2.0, 3.0));
    assert_eq!(bus.get(a), 2.5);
    assert_eq!(bus.get(v), DVec3::new(1.0, 2.0, 3.0));
}

#[test]
fn same_name_returns_same_signal() {
    let mut bus = Bus::new();
    let a: Signal<f64> = bus.signal("x");
    let b: Signal<f64> = bus.signal("x");
    bus.set(a, 7.0);
    assert_eq!(bus.get(b), 7.0);
    assert!(bus.lookup::<f64>("x").is_some());
    assert!(bus.lookup::<DVec3>("x").is_none());
    assert!(bus.lookup::<f64>("missing").is_none());
}

#[test]
#[should_panic(expected = "already registered")]
fn same_name_different_type_panics() {
    let mut bus = Bus::new();
    let _a: Signal<f64> = bus.signal("x");
    let _b: Signal<DVec3> = bus.signal("x");
}

#[test]
fn digest_tracks_values_and_ignores_registration_order() {
    let mut b1 = Bus::new();
    let x1: Signal<f64> = b1.signal("x");
    let y1: Signal<f64> = b1.signal("y");
    let mut b2 = Bus::new();
    let y2: Signal<f64> = b2.signal("y");
    let x2: Signal<f64> = b2.signal("x");
    b1.set(x1, 1.0);
    b1.set(y1, 2.0);
    b2.set(x2, 1.0);
    b2.set(y2, 2.0);
    assert_eq!(b1.digest(), b2.digest());
    b2.set(y2, 2.000_000_1);
    assert_ne!(b1.digest(), b2.digest());
}

#[test]
fn finds_non_finite_signal_by_name() {
    let mut bus = Bus::new();
    let _ok: Signal<f64> = bus.signal("ok");
    let v: Signal<DVec3> = bus.signal("bad.vec");
    assert_eq!(bus.first_non_finite(), None);
    bus.set(v, DVec3::new(0.0, f64::NAN, 0.0));
    assert_eq!(bus.first_non_finite(), Some("bad.vec"));
}

#[test]
fn linear_interp_clamps_and_interpolates() {
    let t = [(0.0, 10.0), (10.0, 20.0), (20.0, 0.0)];
    assert_eq!(interp::linear(&t, -5.0), 10.0);
    assert_eq!(interp::linear(&t, 5.0), 15.0);
    assert_eq!(interp::linear(&t, 15.0), 10.0);
    assert_eq!(interp::linear(&t, 99.0), 0.0);
    assert_eq!(interp::linear(&[(3.0, 4.0)], 100.0), 4.0);
}

#[test]
fn model_rng_is_seeded_per_model() {
    let a: u64 = rng::model_rng(42, "imu").gen();
    let b: u64 = rng::model_rng(42, "imu").gen();
    let c: u64 = rng::model_rng(42, "baro").gen();
    let d: u64 = rng::model_rng(43, "imu").gen();
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_ne!(a, d);
}

#[test]
fn fnv1a_matches_reference_vectors() {
    assert_eq!(rng::fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(rng::fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p ofs-core`
Expected: compile errors (`bus`, `interp`, `rng` modules missing).

- [ ] **Step 5: Implement `rng.rs`**

`crates/ofs-core/src/rng.rs`:
```rust
//! Stable hashing and per-model seeded random streams.
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// 64-bit FNV-1a. Stable across platforms and Rust versions (unlike `DefaultHasher`).
#[derive(Debug, Clone, Copy)]
pub struct Fnv1a(u64);

impl Fnv1a {
    pub fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    pub fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

impl Default for Fnv1a {
    fn default() -> Self {
        Self::new()
    }
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = Fnv1a::new();
    h.write(bytes);
    h.finish()
}

/// Independent random stream per model, so adding a model never perturbs another's noise.
pub fn model_rng(seed: u64, model_name: &str) -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(seed ^ fnv1a64(model_name.as_bytes()))
}
```

- [ ] **Step 6: Implement `interp.rs`**

`crates/ofs-core/src/interp.rs`:
```rust
//! Table lookups.

/// Piecewise-linear interpolation over `(x, y)` points sorted by `x`; clamps outside the range.
pub fn linear(table: &[(f64, f64)], x: f64) -> f64 {
    assert!(!table.is_empty(), "interpolation table is empty");
    if x <= table[0].0 {
        return table[0].1;
    }
    for w in table.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        if x <= x1 {
            if x1 == x0 {
                return y1;
            }
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    table[table.len() - 1].1
}
```

- [ ] **Step 7: Implement `bus.rs`**

`crates/ofs-core/src/bus.rs`:
```rust
//! Named, typed signals shared by all models. Values live in per-type arenas; a `Signal<T>` is an index.
use std::collections::BTreeMap;
use std::marker::PhantomData;

use glam::{DQuat, DVec3};

use crate::rng::Fnv1a;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalKind {
    Scalar,
    Vec3,
    Quat,
}

#[derive(Debug)]
pub struct Signal<T> {
    index: usize,
    _t: PhantomData<T>,
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Signal<T> {}

pub trait BusValue: Copy + Default + 'static {
    fn kind() -> SignalKind;
    fn arena(bus: &Bus) -> &Vec<Self>;
    fn arena_mut(bus: &mut Bus) -> &mut Vec<Self>;
}

#[derive(Debug, Default)]
pub struct Bus {
    scalars: Vec<f64>,
    vec3s: Vec<DVec3>,
    quats: Vec<DQuat>,
    names: BTreeMap<String, (SignalKind, usize)>,
}

impl BusValue for f64 {
    fn kind() -> SignalKind {
        SignalKind::Scalar
    }
    fn arena(bus: &Bus) -> &Vec<Self> {
        &bus.scalars
    }
    fn arena_mut(bus: &mut Bus) -> &mut Vec<Self> {
        &mut bus.scalars
    }
}

impl BusValue for DVec3 {
    fn kind() -> SignalKind {
        SignalKind::Vec3
    }
    fn arena(bus: &Bus) -> &Vec<Self> {
        &bus.vec3s
    }
    fn arena_mut(bus: &mut Bus) -> &mut Vec<Self> {
        &mut bus.vec3s
    }
}

impl BusValue for DQuat {
    fn kind() -> SignalKind {
        SignalKind::Quat
    }
    fn arena(bus: &Bus) -> &Vec<Self> {
        &bus.quats
    }
    fn arena_mut(bus: &mut Bus) -> &mut Vec<Self> {
        &mut bus.quats
    }
}

impl Bus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `name`, or returns the existing signal if it was already registered with the same type.
    pub fn signal<T: BusValue>(&mut self, name: &str) -> Signal<T> {
        if let Some(&(kind, index)) = self.names.get(name) {
            assert_eq!(kind, T::kind(), "signal '{name}' already registered as {kind:?}");
            return Signal { index, _t: PhantomData };
        }
        let arena = T::arena_mut(self);
        let index = arena.len();
        arena.push(T::default());
        self.names.insert(name.to_string(), (T::kind(), index));
        Signal { index, _t: PhantomData }
    }

    pub fn lookup<T: BusValue>(&self, name: &str) -> Option<Signal<T>> {
        match self.names.get(name) {
            Some(&(kind, index)) if kind == T::kind() => Some(Signal { index, _t: PhantomData }),
            _ => None,
        }
    }

    pub fn get<T: BusValue>(&self, s: Signal<T>) -> T {
        T::arena(self)[s.index]
    }

    pub fn set<T: BusValue>(&mut self, s: Signal<T>, value: T) {
        T::arena_mut(self)[s.index] = value;
    }

    /// Hash of every signal's name and exact bit pattern, in name order.
    pub fn digest(&self) -> u64 {
        let mut h = Fnv1a::new();
        for (name, &(kind, i)) in &self.names {
            h.write(name.as_bytes());
            let values: Vec<f64> = match kind {
                SignalKind::Scalar => vec![self.scalars[i]],
                SignalKind::Vec3 => self.vec3s[i].to_array().to_vec(),
                SignalKind::Quat => self.quats[i].to_array().to_vec(),
            };
            for v in values {
                h.write(&v.to_bits().to_le_bytes());
            }
        }
        h.finish()
    }

    /// Name of the first signal (in name order) holding NaN or infinity.
    pub fn first_non_finite(&self) -> Option<&str> {
        self.names
            .iter()
            .find(|&(_, &(kind, i))| match kind {
                SignalKind::Scalar => !self.scalars[i].is_finite(),
                SignalKind::Vec3 => !self.vec3s[i].is_finite(),
                SignalKind::Quat => !self.quats[i].is_finite(),
            })
            .map(|(name, _)| name.as_str())
    }
}
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p ofs-core`
Expected: 8 passed.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml LICENSE .gitignore crates/ofs-core
git commit -m "feat(core): typed signal bus, interpolation and seeded RNG"
```

---

### Task 2: `Model` trait, scheduler, signal names

**Files:**
- Create: `crates/ofs-core/src/model.rs`, `crates/ofs-core/src/scheduler.rs`, `crates/ofs-core/src/names.rs`, `crates/ofs-core/src/consts.rs`
- Modify: `crates/ofs-core/src/lib.rs`
- Test: `crates/ofs-core/tests/scheduler.rs`

**Interfaces:**
- Consumes: Task 1 `Bus`, `Signal`.
- Produces:
  - `ofs_core::StepCtx { tick: u64, time_s: f64, dt_s: f64 }`.
  - `ofs_core::SimError` (Clone, PartialEq): `Firmware(String)`, `NonFinite(String)`, `InvalidArgument(String)`, `Other(String)`.
  - `ofs_core::Model: Send` — `name(&self) -> &str`, `rate_divisor(&self) -> u32`, `step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError>`.
  - `ofs_core::Scheduler` — `new(base_hz: u32, bus: Bus)`, `add(Box<dyn Model>)`, `step() -> Result<(), SimError>`, `run_for(seconds: f64) -> Result<(), SimError>`, `base_hz()`, `tick()`, `time_s()`, `bus()`, `bus_mut()`.
  - `ofs_core::names` constants/functions (listed in Step 4) and `ofs_core::consts::{GRAVITY_MPS2, AIR_DENSITY_KGPM3}`.

- [ ] **Step 1: Write the failing tests**

`crates/ofs-core/tests/scheduler.rs`:
```rust
use ofs_core::{Bus, Model, Scheduler, Signal, SimError, StepCtx};

struct Counter {
    name: String,
    div: u32,
    count: Signal<f64>,
    last_dt: Signal<f64>,
    last_time: Signal<f64>,
}

impl Counter {
    fn new(name: &str, div: u32, bus: &mut Bus) -> Self {
        Self {
            name: name.to_string(),
            div,
            count: bus.signal(&format!("{name}.count")),
            last_dt: bus.signal(&format!("{name}.dt")),
            last_time: bus.signal(&format!("{name}.time")),
        }
    }
}

impl Model for Counter {
    fn name(&self) -> &str {
        &self.name
    }
    fn rate_divisor(&self) -> u32 {
        self.div
    }
    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let c = bus.get(self.count);
        bus.set(self.count, c + 1.0);
        bus.set(self.last_dt, ctx.dt_s);
        bus.set(self.last_time, ctx.time_s);
        Ok(())
    }
}

struct Writer(Signal<f64>);
impl Model for Writer {
    fn name(&self) -> &str {
        "writer"
    }
    fn rate_divisor(&self) -> u32 {
        1
    }
    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        bus.set(self.0, ctx.tick as f64 + 1.0);
        Ok(())
    }
}

struct Reader(Signal<f64>, Signal<f64>);
impl Model for Reader {
    fn name(&self) -> &str {
        "reader"
    }
    fn rate_divisor(&self) -> u32 {
        1
    }
    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let v = bus.get(self.0);
        bus.set(self.1, v);
        Ok(())
    }
}

struct NanAt(u64, Signal<f64>);
impl Model for NanAt {
    fn name(&self) -> &str {
        "nan"
    }
    fn rate_divisor(&self) -> u32 {
        1
    }
    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        if ctx.tick == self.0 {
            bus.set(self.1, f64::NAN);
        }
        Ok(())
    }
}

fn value(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

#[test]
fn runs_models_at_their_divisors() {
    let mut bus = Bus::new();
    let fast = Counter::new("fast", 1, &mut bus);
    let slow = Counter::new("slow", 4, &mut bus);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(fast));
    s.add(Box::new(slow));
    for _ in 0..8 {
        s.step().unwrap();
    }
    assert_eq!(value(&s, "fast.count"), 8.0);
    assert_eq!(value(&s, "slow.count"), 2.0);
    assert_eq!(value(&s, "slow.dt"), 4.0 / 8000.0);
    assert_eq!(value(&s, "slow.time"), 4.0 / 8000.0);
    assert_eq!(s.tick(), 8);
    assert_eq!(s.time_s(), 0.001);
}

#[test]
fn models_run_in_registration_order_within_a_tick() {
    let mut bus = Bus::new();
    let w = bus.signal("w");
    let r = bus.signal("r");
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(Writer(w)));
    s.add(Box::new(Reader(w, r)));
    s.step().unwrap();
    assert_eq!(value(&s, "r"), 1.0);
}

#[test]
fn non_finite_value_stops_the_run_without_advancing() {
    let mut bus = Bus::new();
    let bad = bus.signal("bad");
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(NanAt(3, bad)));
    for _ in 0..3 {
        s.step().unwrap();
    }
    assert_eq!(s.step(), Err(SimError::NonFinite("bad".into())));
    assert_eq!(s.tick(), 3);
}

#[test]
fn run_for_steps_whole_ticks_and_rejects_bad_durations() {
    let mut s = Scheduler::new(8000, Bus::new());
    s.run_for(0.5).unwrap();
    assert_eq!(s.tick(), 4000);
    assert!(matches!(s.run_for(-1.0), Err(SimError::InvalidArgument(_))));
    assert!(matches!(s.run_for(f64::NAN), Err(SimError::InvalidArgument(_))));
    assert_eq!(s.tick(), 4000);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ofs-core --test scheduler`
Expected: compile errors (`Model`, `Scheduler`, `SimError`, `StepCtx` not found).

- [ ] **Step 3: Implement `model.rs` and `scheduler.rs`**

`crates/ofs-core/src/model.rs`:
```rust
//! The interface every simulated part implements.
use crate::bus::Bus;

/// Timing for one step of one model.
#[derive(Debug, Clone, Copy)]
pub struct StepCtx {
    pub tick: u64,
    pub time_s: f64,
    pub dt_s: f64,
}

/// Simulator failures. Simulated drone failures are never reported through this type.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SimError {
    #[error("firmware: {0}")]
    Firmware(String),
    #[error("non-finite value in signal '{0}'")]
    NonFinite(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("{0}")]
    Other(String),
}

/// A simulated part. Reads and writes bus signals only; runs every `rate_divisor` base ticks.
pub trait Model: Send {
    fn name(&self) -> &str;
    fn rate_divisor(&self) -> u32;
    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError>;
}
```

`crates/ofs-core/src/scheduler.rs`:
```rust
//! Fixed-rate, multi-rate, deterministic stepping of models over one bus.
use crate::bus::Bus;
use crate::model::{Model, SimError, StepCtx};

pub struct Scheduler {
    base_hz: u32,
    tick: u64,
    bus: Bus,
    models: Vec<Box<dyn Model>>,
}

impl Scheduler {
    pub fn new(base_hz: u32, bus: Bus) -> Self {
        assert!(base_hz > 0, "base_hz must be > 0");
        Self { base_hz, tick: 0, bus, models: Vec::new() }
    }

    pub fn add(&mut self, model: Box<dyn Model>) {
        assert!(model.rate_divisor() >= 1, "model '{}' has rate divisor 0", model.name());
        self.models.push(model);
    }

    pub fn base_hz(&self) -> u32 {
        self.base_hz
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn time_s(&self) -> f64 {
        self.tick as f64 / f64::from(self.base_hz)
    }

    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    pub fn bus_mut(&mut self) -> &mut Bus {
        &mut self.bus
    }

    /// Runs every model due on this tick, in registration order, then checks all signals are finite.
    /// On error the tick is not advanced.
    pub fn step(&mut self) -> Result<(), SimError> {
        let time_s = self.time_s();
        for model in self.models.iter_mut() {
            let div = u64::from(model.rate_divisor());
            if self.tick % div == 0 {
                let ctx = StepCtx { tick: self.tick, time_s, dt_s: div as f64 / f64::from(self.base_hz) };
                model.step(&ctx, &mut self.bus)?;
            }
        }
        if let Some(name) = self.bus.first_non_finite() {
            return Err(SimError::NonFinite(name.to_string()));
        }
        self.tick += 1;
        Ok(())
    }

    pub fn run_for(&mut self, seconds: f64) -> Result<(), SimError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(SimError::InvalidArgument(format!("seconds must be finite and >= 0 (got {seconds})")));
        }
        let ticks = (seconds * f64::from(self.base_hz)).round() as u64;
        for _ in 0..ticks {
            self.step()?;
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Add shared names and constants, export everything**

`crates/ofs-core/src/names.rs`:
```rust
//! Canonical bus signal names shared by all crates.

pub const BODY_POS_NED: &str = "body.pos_ned_m";
pub const BODY_VEL_NED: &str = "body.vel_ned_mps";
/// FRD body -> NED world.
pub const BODY_ATT: &str = "body.att_q";
pub const BODY_RATE_FRD: &str = "body.rate_frd_radps";
/// Kinematic acceleration (not specific force), NED.
pub const BODY_ACCEL_NED: &str = "body.accel_ned_mps2";

pub const BATTERY_VOLTAGE: &str = "battery.voltage_v";
pub const BATTERY_CURRENT: &str = "battery.current_a";
pub const BATTERY_SOC: &str = "battery.soc";
pub const BATTERY_CONSUMED: &str = "battery.consumed_mah";

pub const IMU_GYRO: &str = "imu.gyro_frd_radps";
/// Specific force, FRD.
pub const IMU_ACCEL: &str = "imu.accel_frd_mps2";
pub const BARO_PRESSURE: &str = "baro.pressure_pa";

/// Sticks in [-1, 1]; throttle in [0, 1]; aux in [-1, 1].
pub const RC_ROLL: &str = "rc.roll";
pub const RC_PITCH: &str = "rc.pitch";
pub const RC_YAW: &str = "rc.yaw";
pub const RC_THROTTLE: &str = "rc.throttle";
pub const RC_AUX_COUNT: usize = 4;

pub fn rc_aux(i: usize) -> String {
    format!("rc.aux.{i}")
}

/// Flight-controller motor command in [0, 1].
pub fn motor_cmd(i: usize) -> String {
    format!("fc.motor.{i}.cmd")
}

pub fn esc_duty(i: usize) -> String {
    format!("esc.{i}.duty")
}

pub fn esc_bus_current(i: usize) -> String {
    format!("esc.{i}.bus_current_a")
}

pub fn motor_current(i: usize) -> String {
    format!("motor.{i}.current_a")
}

/// Rotor speed magnitude (>= 0).
pub fn motor_omega(i: usize) -> String {
    format!("motor.{i}.omega_radps")
}

pub fn motor_omega_dot(i: usize) -> String {
    format!("motor.{i}.omega_dot_radps2")
}

/// Thrust magnitude along body -Z.
pub fn prop_thrust(i: usize) -> String {
    format!("prop.{i}.thrust_n")
}

/// Aerodynamic drag torque magnitude on the rotor.
pub fn prop_torque(i: usize) -> String {
    format!("prop.{i}.torque_nm")
}
```

`crates/ofs-core/src/consts.rs`:
```rust
//! Physical constants (v1: fixed sea-level standard atmosphere).
pub const GRAVITY_MPS2: f64 = 9.80665;
pub const AIR_DENSITY_KGPM3: f64 = 1.225;
```

`crates/ofs-core/src/lib.rs` (replace whole file):
```rust
//! Open FPV Sim core: typed signal bus, models and scheduler. Deterministic, no I/O.
pub mod bus;
pub mod consts;
pub mod interp;
pub mod model;
pub mod names;
pub mod rng;
pub mod scheduler;

pub use bus::{Bus, BusValue, Signal, SignalKind};
pub use model::{Model, SimError, StepCtx};
pub use scheduler::Scheduler;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p ofs-core`
Expected: 12 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/ofs-core
git commit -m "feat(core): Model trait, multi-rate scheduler and shared signal names"
```

---

### Task 3: Rigid body with drag and ground contact

**Files:**
- Create: `crates/ofs-physics/Cargo.toml`, `crates/ofs-physics/src/lib.rs`, `crates/ofs-physics/src/rigid_body.rs`
- Test: `crates/ofs-physics/tests/rigid_body.rs`

**Interfaces:**
- Consumes: `ofs_core::{Bus, Model, Signal, SimError, StepCtx, names, consts}`.
- Produces:
  - `MotorMount { position_frd_m: DVec3, spin: f64 }` — `spin` = +1 when the rotor's angular velocity points along +Z_FRD (clockwise seen from above), −1 otherwise.
  - `GroundParams { stiffness_npm, damping_nspm, friction_coeff }` (f64).
  - `AirframeParams { mass_kg, inertia_kgm2: DVec3, cda_m2: DVec3, angular_damping_nms, rotor_inertia_kgm2, mounts: Vec<MotorMount>, contact_points_frd_m: Vec<DVec3>, ground: GroundParams }`.
  - `BodyState { pos_ned_m, vel_ned_mps, att: DQuat, rate_frd_radps }` (Copy).
  - `RigidBody::new(params, initial: BodyState, bus: &mut Bus) -> RigidBody`, `RigidBody::state() -> BodyState`. Model name `"body"`, rate divisor 1. Reads `prop_thrust(i)`, `prop_torque(i)`, `motor_omega_dot(i)`; writes `BODY_POS_NED`, `BODY_VEL_NED`, `BODY_ATT`, `BODY_RATE_FRD`, `BODY_ACCEL_NED`.

- [ ] **Step 1: Create the crate**

`crates/ofs-physics/Cargo.toml`:
```toml
[package]
name = "ofs-physics"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
ofs-core.workspace = true
glam.workspace = true
```

`crates/ofs-physics/src/lib.rs`:
```rust
//! Flight physics: 6-DOF rigid body, propellers.
pub mod rigid_body;
```

- [ ] **Step 2: Write the failing tests**

`crates/ofs-physics/tests/rigid_body.rs`:
```rust
use glam::{DQuat, DVec3};
use ofs_core::{consts::GRAVITY_MPS2, names, Bus, Scheduler};
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, MotorMount, RigidBody};

const A: f64 = 0.08;

fn quad_x() -> Vec<MotorMount> {
    vec![
        MotorMount { position_frd_m: DVec3::new(-A, A, 0.0), spin: 1.0 },
        MotorMount { position_frd_m: DVec3::new(A, A, 0.0), spin: -1.0 },
        MotorMount { position_frd_m: DVec3::new(-A, -A, 0.0), spin: -1.0 },
        MotorMount { position_frd_m: DVec3::new(A, -A, 0.0), spin: 1.0 },
    ]
}

fn feet() -> Vec<DVec3> {
    vec![
        DVec3::new(-A, A, 0.03),
        DVec3::new(A, A, 0.03),
        DVec3::new(-A, -A, 0.03),
        DVec3::new(A, -A, 0.03),
    ]
}

fn params(mounts: Vec<MotorMount>, contacts: Vec<DVec3>) -> AirframeParams {
    AirframeParams {
        mass_kg: 0.65,
        inertia_kgm2: DVec3::new(0.0025, 0.0025, 0.0045),
        cda_m2: DVec3::ZERO,
        angular_damping_nms: 0.0,
        rotor_inertia_kgm2: 0.0,
        mounts,
        contact_points_frd_m: contacts,
        ground: GroundParams { stiffness_npm: 3000.0, damping_nspm: 40.0, friction_coeff: 0.6 },
    }
}

fn at(pos: DVec3) -> BodyState {
    BodyState { pos_ned_m: pos, vel_ned_mps: DVec3::ZERO, att: DQuat::IDENTITY, rate_frd_radps: DVec3::ZERO }
}

fn sim(p: AirframeParams, s: BodyState) -> Scheduler {
    let mut bus = Bus::new();
    let body = RigidBody::new(p, s, &mut bus);
    let mut sch = Scheduler::new(8000, bus);
    sch.add(Box::new(body));
    sch
}

fn vec3(s: &Scheduler, name: &str) -> DVec3 {
    s.bus().get(s.bus().lookup::<DVec3>(name).unwrap())
}

fn att(s: &Scheduler) -> DQuat {
    s.bus().get(s.bus().lookup::<DQuat>(names::BODY_ATT).unwrap())
}

fn set_scalar(s: &mut Scheduler, name: &str, v: f64) {
    let sig = s.bus().lookup::<f64>(name).unwrap();
    s.bus_mut().set(sig, v);
}

#[test]
fn free_fall_matches_kinematics() {
    let mut s = sim(params(vec![], vec![]), at(DVec3::new(0.0, 0.0, -100.0)));
    s.run_for(1.0).unwrap();
    let vel = vec3(&s, names::BODY_VEL_NED);
    let pos = vec3(&s, names::BODY_POS_NED);
    assert!((vel.z - GRAVITY_MPS2).abs() < 1e-9, "vel {vel}");
    assert!((pos.z - (-100.0 + 0.5 * GRAVITY_MPS2)).abs() < 1e-3, "pos {pos}");
}

#[test]
fn torque_free_spin_conserves_energy_and_angular_momentum() {
    let mut p = params(vec![], vec![]);
    p.inertia_kgm2 = DVec3::new(0.002, 0.003, 0.004);
    let mut s0 = at(DVec3::new(0.0, 0.0, -1000.0));
    s0.rate_frd_radps = DVec3::new(0.3, 0.2, 5.0);
    let i = p.inertia_kgm2;
    let energy = |w: DVec3| 0.5 * (i * w * w).element_sum();
    let e0 = energy(s0.rate_frd_radps);
    let l0 = s0.att * (i * s0.rate_frd_radps);
    let mut s = sim(p, s0);
    s.run_for(2.0).unwrap();
    let w = vec3(&s, names::BODY_RATE_FRD);
    let l = att(&s) * (i * w);
    assert!(((energy(w) - e0) / e0).abs() < 1e-4, "energy drift");
    assert!((l - l0).length() / l0.length() < 1e-4, "momentum drift");
}

#[test]
fn equal_thrust_balances_gravity_and_reaction_torque_yaws() {
    let mut s = sim(params(quad_x(), vec![]), at(DVec3::new(0.0, 0.0, -10.0)));
    for k in 0..4 {
        set_scalar(&mut s, &names::prop_thrust(k), 0.65 * GRAVITY_MPS2 / 4.0);
    }
    s.run_for(1.0).unwrap();
    assert!(vec3(&s, names::BODY_VEL_NED).length() < 1e-9);
    assert!(vec3(&s, names::BODY_RATE_FRD).length() < 1e-12);

    // Drag torque on the CW rotors (mounts 0 and 3) reacts on the body as -Z (counter-clockwise).
    set_scalar(&mut s, &names::prop_torque(0), 0.01);
    set_scalar(&mut s, &names::prop_torque(3), 0.01);
    s.run_for(0.1).unwrap();
    let expected = -0.02 / 0.0045 * 0.1;
    assert!((vec3(&s, names::BODY_RATE_FRD).z - expected).abs() < 1e-3);
}

#[test]
fn dropped_quad_settles_on_its_feet() {
    let mut s = sim(params(quad_x(), feet()), at(DVec3::new(0.0, 0.0, -0.08)));
    s.run_for(3.0).unwrap();
    let pos = vec3(&s, names::BODY_POS_NED);
    let static_sink = 0.65 * GRAVITY_MPS2 / (4.0 * 3000.0);
    assert!((pos.z - (-0.03 + static_sink)).abs() < 1e-4, "pos {pos}");
    assert!(vec3(&s, names::BODY_VEL_NED).length() < 1e-3);
    assert!(vec3(&s, names::BODY_RATE_FRD).length() < 1e-3);
}

#[test]
fn quadratic_drag_matches_analytic_decay() {
    let mut p = params(vec![], vec![]);
    p.cda_m2 = DVec3::new(0.01, 0.01, 0.02);
    let mut s0 = at(DVec3::new(0.0, 0.0, -1000.0));
    s0.vel_ned_mps = DVec3::new(10.0, 0.0, 0.0);
    let mut s = sim(p, s0);
    s.run_for(1.0).unwrap();
    let k = 0.5 * 1.225 * 0.01;
    let expected = 10.0 / (1.0 + k * 10.0 * 1.0 / 0.65);
    let vx = vec3(&s, names::BODY_VEL_NED).x;
    assert!(((vx - expected) / expected).abs() < 1e-3, "vx {vx} expected {expected}");
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p ofs-physics`
Expected: compile error (`rigid_body` items not found).

- [ ] **Step 4: Implement the rigid body**

`crates/ofs-physics/src/rigid_body.rs`:
```rust
//! 6-DOF rigid body: prop thrust and reaction torque, quadratic body drag, spring-damper ground contact.
use glam::{DQuat, DVec3};
use ofs_core::consts::{AIR_DENSITY_KGPM3, GRAVITY_MPS2};
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};

#[derive(Debug, Clone)]
pub struct MotorMount {
    pub position_frd_m: DVec3,
    /// +1: rotor angular velocity along +Z_FRD (clockwise seen from above); -1: counter-clockwise.
    pub spin: f64,
}

#[derive(Debug, Clone)]
pub struct GroundParams {
    pub stiffness_npm: f64,
    pub damping_nspm: f64,
    pub friction_coeff: f64,
}

#[derive(Debug, Clone)]
pub struct AirframeParams {
    pub mass_kg: f64,
    /// Diagonal inertia about FRD axes.
    pub inertia_kgm2: DVec3,
    /// Drag area (Cd * A) per FRD axis.
    pub cda_m2: DVec3,
    pub angular_damping_nms: f64,
    /// Motor rotor + propeller inertia, for the reaction torque of spinning up.
    pub rotor_inertia_kgm2: f64,
    pub mounts: Vec<MotorMount>,
    pub contact_points_frd_m: Vec<DVec3>,
    pub ground: GroundParams,
}

#[derive(Debug, Clone, Copy)]
pub struct BodyState {
    pub pos_ned_m: DVec3,
    pub vel_ned_mps: DVec3,
    /// FRD body -> NED world.
    pub att: DQuat,
    pub rate_frd_radps: DVec3,
}

pub struct RigidBody {
    p: AirframeParams,
    s: BodyState,
    thrust: Vec<Signal<f64>>,
    torque: Vec<Signal<f64>>,
    omega_dot: Vec<Signal<f64>>,
    pos: Signal<DVec3>,
    vel: Signal<DVec3>,
    att: Signal<DQuat>,
    rate: Signal<DVec3>,
    accel: Signal<DVec3>,
}

impl RigidBody {
    pub fn new(p: AirframeParams, initial: BodyState, bus: &mut Bus) -> Self {
        let n = p.mounts.len();
        let body = Self {
            thrust: (0..n).map(|i| bus.signal(&names::prop_thrust(i))).collect(),
            torque: (0..n).map(|i| bus.signal(&names::prop_torque(i))).collect(),
            omega_dot: (0..n).map(|i| bus.signal(&names::motor_omega_dot(i))).collect(),
            pos: bus.signal(names::BODY_POS_NED),
            vel: bus.signal(names::BODY_VEL_NED),
            att: bus.signal(names::BODY_ATT),
            rate: bus.signal(names::BODY_RATE_FRD),
            accel: bus.signal(names::BODY_ACCEL_NED),
            p,
            s: initial,
        };
        body.publish(bus, DVec3::ZERO);
        body
    }

    pub fn state(&self) -> BodyState {
        self.s
    }

    fn publish(&self, bus: &mut Bus, accel_ned: DVec3) {
        bus.set(self.pos, self.s.pos_ned_m);
        bus.set(self.vel, self.s.vel_ned_mps);
        bus.set(self.att, self.s.att);
        bus.set(self.rate, self.s.rate_frd_radps);
        bus.set(self.accel, accel_ned);
    }

    /// Total contact force and torque about the centre of mass, both in NED.
    fn contact(&self, s: &BodyState) -> (DVec3, DVec3) {
        let g = &self.p.ground;
        let mut force = DVec3::ZERO;
        let mut torque = DVec3::ZERO;
        for c in &self.p.contact_points_frd_m {
            let lever = s.att * *c;
            let depth = s.pos_ned_m.z + lever.z; // ground plane is z = 0, NED z points down
            if depth <= 0.0 {
                continue;
            }
            let v = s.vel_ned_mps + s.att * s.rate_frd_radps.cross(*c);
            let normal = (g.stiffness_npm * depth + g.damping_nspm * v.z).max(0.0);
            let v_t = DVec3::new(v.x, v.y, 0.0);
            let friction = -v_t * (g.friction_coeff * normal / v_t.length().max(0.05));
            let f = DVec3::new(0.0, 0.0, -normal) + friction;
            force += f;
            torque += lever.cross(f);
        }
        (force, torque)
    }
}

fn angular_accel(p: &AirframeParams, w: DVec3, torque: DVec3) -> DVec3 {
    let i = p.inertia_kgm2;
    (torque - w.cross(i * w) - p.angular_damping_nms * w) / i
}

impl Model for RigidBody {
    fn name(&self) -> &str {
        "body"
    }

    fn rate_divisor(&self) -> u32 {
        1
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let dt = ctx.dt_s;
        let s = self.s;
        let to_body = s.att.inverse();

        let mut force_body = DVec3::ZERO;
        let mut torque_body = DVec3::ZERO;
        for (k, m) in self.p.mounts.iter().enumerate() {
            let f = DVec3::new(0.0, 0.0, -bus.get(self.thrust[k]));
            force_body += f;
            torque_body += m.position_frd_m.cross(f);
            let rotor_torque = bus.get(self.torque[k]) + self.p.rotor_inertia_kgm2 * bus.get(self.omega_dot[k]);
            torque_body.z -= m.spin * rotor_torque;
        }
        let v_body = to_body * s.vel_ned_mps;
        force_body -= 0.5 * AIR_DENSITY_KGPM3 * self.p.cda_m2 * v_body.abs() * v_body;

        let (contact_force, contact_torque) = self.contact(&s);
        torque_body += to_body * contact_torque;

        // Translation: semi-implicit Euler.
        let accel_ned = (s.att * force_body + contact_force) / self.p.mass_kg + DVec3::new(0.0, 0.0, GRAVITY_MPS2);
        let vel = s.vel_ned_mps + accel_ned * dt;
        let pos = s.pos_ned_m + vel * dt;

        // Rotation: midpoint (RK2) with torque held over the step.
        let k1 = angular_accel(&self.p, s.rate_frd_radps, torque_body);
        let k2 = angular_accel(&self.p, s.rate_frd_radps + k1 * (0.5 * dt), torque_body);
        let rate = s.rate_frd_radps + k2 * dt;
        let w_avg = 0.5 * (s.rate_frd_radps + rate);
        let att = (s.att * DQuat::from_scaled_axis(w_avg * dt)).normalize();

        self.s = BodyState { pos_ned_m: pos, vel_ned_mps: vel, att, rate_frd_radps: rate };
        self.publish(bus, accel_ned);
        Ok(())
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p ofs-physics`
Expected: 5 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/ofs-physics
git commit -m "feat(physics): 6-DOF rigid body with drag and ground contact"
```

---

### Task 4: Propeller model

**Files:**
- Create: `crates/ofs-physics/src/propeller.rs`
- Modify: `crates/ofs-physics/src/lib.rs`
- Test: `crates/ofs-physics/tests/propeller.rs`

**Interfaces:**
- Consumes: `ofs_core::interp::linear`, `names::{motor_omega, prop_thrust, prop_torque}`.
- Produces: `PropParams { diameter_m: f64, ct_table: Vec<(f64, f64)>, cp_table: Vec<(f64, f64)> }` (tables keyed by RPM); `thrust_torque(&PropParams, omega_radps: f64) -> (f64, f64)`; `Propeller::new(index: usize, PropParams, bus: &mut Bus)` — model name `"prop.{index}"`, rate divisor 1.

- [ ] **Step 1: Write the failing tests**

`crates/ofs-physics/tests/propeller.rs`:
```rust
use std::f64::consts::PI;

use ofs_core::{names, Bus, Scheduler};
use ofs_physics::propeller::{thrust_torque, PropParams, Propeller};

fn prop(ct: Vec<(f64, f64)>, cp: Vec<(f64, f64)>) -> PropParams {
    PropParams { diameter_m: 0.127, ct_table: ct, cp_table: cp }
}

#[test]
fn thrust_and_torque_follow_coefficient_formula() {
    let p = prop(vec![(0.0, 0.11)], vec![(0.0, 0.045)]);
    let omega = 24_000.0 * 2.0 * PI / 60.0;
    let n: f64 = 400.0;
    let (t, q) = thrust_torque(&p, omega);
    let t_expected = 0.11 * 1.225 * n * n * 0.127_f64.powi(4);
    let q_expected = 0.045 * 1.225 * n * n * 0.127_f64.powi(5) / (2.0 * PI);
    assert!((t - t_expected).abs() < 1e-9);
    assert!((q - q_expected).abs() < 1e-12);
    assert!(t > 5.0 && t < 6.0, "5-inch prop at 24k rpm makes ~5.6 N, got {t}");
}

#[test]
fn stopped_or_reversed_rotor_makes_nothing() {
    let p = prop(vec![(0.0, 0.11)], vec![(0.0, 0.045)]);
    assert_eq!(thrust_torque(&p, 0.0), (0.0, 0.0));
    assert_eq!(thrust_torque(&p, -100.0), (0.0, 0.0));
}

#[test]
fn coefficients_are_interpolated_by_rpm() {
    let p = prop(vec![(0.0, 0.10), (20_000.0, 0.20)], vec![(0.0, 0.05)]);
    let omega = 10_000.0 * 2.0 * PI / 60.0;
    let n = 10_000.0 / 60.0;
    let (t, _) = thrust_torque(&p, omega);
    assert!((t - 0.15 * 1.225 * n * n * 0.127_f64.powi(4)).abs() < 1e-9);
}

#[test]
fn model_reads_rotor_speed_and_writes_thrust_and_torque() {
    let p = prop(vec![(0.0, 0.11)], vec![(0.0, 0.045)]);
    let mut bus = Bus::new();
    let model = Propeller::new(2, p.clone(), &mut bus);
    let omega = bus.signal::<f64>(&names::motor_omega(2));
    bus.set(omega, 1500.0);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(model));
    s.step().unwrap();
    let (t, q) = thrust_torque(&p, 1500.0);
    let b = s.bus();
    assert_eq!(b.get(b.lookup::<f64>(&names::prop_thrust(2)).unwrap()), t);
    assert_eq!(b.get(b.lookup::<f64>(&names::prop_torque(2)).unwrap()), q);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ofs-physics --test propeller`
Expected: compile error (`propeller` module not found).

- [ ] **Step 3: Implement**

`crates/ofs-physics/src/propeller.rs`:
```rust
//! Static propeller model: T = Ct·ρ·n²·D⁴, Q = Cp·ρ·n²·D⁵ / 2π, with Ct and Cp looked up by RPM.
use std::f64::consts::PI;

use ofs_core::consts::AIR_DENSITY_KGPM3;
use ofs_core::{interp, names, Bus, Model, Signal, SimError, StepCtx};

#[derive(Debug, Clone)]
pub struct PropParams {
    pub diameter_m: f64,
    /// (rpm, Ct) sorted by rpm.
    pub ct_table: Vec<(f64, f64)>,
    /// (rpm, Cp) sorted by rpm.
    pub cp_table: Vec<(f64, f64)>,
}

/// Thrust (N) and drag torque (N·m) magnitudes for a rotor speed in rad/s.
pub fn thrust_torque(p: &PropParams, omega_radps: f64) -> (f64, f64) {
    if omega_radps <= 0.0 {
        return (0.0, 0.0);
    }
    let n = omega_radps / (2.0 * PI);
    let rpm = n * 60.0;
    let ct = interp::linear(&p.ct_table, rpm);
    let cp = interp::linear(&p.cp_table, rpm);
    let d = p.diameter_m;
    let thrust = ct * AIR_DENSITY_KGPM3 * n * n * d.powi(4);
    let torque = cp * AIR_DENSITY_KGPM3 * n * n * d.powi(5) / (2.0 * PI);
    (thrust, torque)
}

pub struct Propeller {
    name: String,
    p: PropParams,
    omega: Signal<f64>,
    thrust: Signal<f64>,
    torque: Signal<f64>,
}

impl Propeller {
    pub fn new(index: usize, p: PropParams, bus: &mut Bus) -> Self {
        Self {
            name: format!("prop.{index}"),
            p,
            omega: bus.signal(&names::motor_omega(index)),
            thrust: bus.signal(&names::prop_thrust(index)),
            torque: bus.signal(&names::prop_torque(index)),
        }
    }
}

impl Model for Propeller {
    fn name(&self) -> &str {
        &self.name
    }

    fn rate_divisor(&self) -> u32 {
        1
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let (t, q) = thrust_torque(&self.p, bus.get(self.omega));
        bus.set(self.thrust, t);
        bus.set(self.torque, q);
        Ok(())
    }
}
```

`crates/ofs-physics/src/lib.rs` (replace whole file):
```rust
//! Flight physics: 6-DOF rigid body, propellers.
pub mod propeller;
pub mod rigid_body;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ofs-physics`
Expected: 9 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-physics
git commit -m "feat(physics): propeller thrust and torque from coefficient tables"
```

---

### Task 5: Battery model

**Files:**
- Create: `crates/ofs-electrical/Cargo.toml`, `crates/ofs-electrical/src/lib.rs`, `crates/ofs-electrical/src/battery.rs`
- Test: `crates/ofs-electrical/tests/battery.rs`

**Interfaces:**
- Consumes: `ofs_core::{interp, names::{esc_bus_current, BATTERY_*}}`.
- Produces: `BatteryParams { cells: u32, capacity_mah: f64, r0_ohm: f64, r1_ohm: f64, c1_f: f64, ocv_table: Vec<(f64, f64)>, initial_soc: f64 }` (`ocv_table` = (state of charge 0..1, cell volts)); `Battery::new(p, motor_count: usize, rate_divisor: u32, bus: &mut Bus)` — model name `"battery"`; reads `esc_bus_current(i)`; writes `BATTERY_VOLTAGE`, `BATTERY_CURRENT`, `BATTERY_SOC`, `BATTERY_CONSUMED`. The initial voltage is published by `new`.

- [ ] **Step 1: Create the crate**

`crates/ofs-electrical/Cargo.toml`:
```toml
[package]
name = "ofs-electrical"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
ofs-core.workspace = true
```

`crates/ofs-electrical/src/lib.rs`:
```rust
//! Electrical models: battery, ESC + brushless motor.
pub mod battery;
```

- [ ] **Step 2: Write the failing tests**

`crates/ofs-electrical/tests/battery.rs`:
```rust
use ofs_core::{interp, names, Bus, Scheduler};
use ofs_electrical::battery::{Battery, BatteryParams};

fn lipo() -> Vec<(f64, f64)> {
    vec![
        (0.0, 3.27), (0.05, 3.61), (0.1, 3.69), (0.2, 3.73), (0.3, 3.77), (0.4, 3.79),
        (0.5, 3.82), (0.6, 3.87), (0.7, 3.92), (0.8, 3.97), (0.9, 4.06), (1.0, 4.20),
    ]
}

fn params() -> BatteryParams {
    BatteryParams {
        cells: 6,
        capacity_mah: 1300.0,
        r0_ohm: 0.024,
        r1_ohm: 0.010,
        c1_f: 2000.0,
        ocv_table: lipo(),
        initial_soc: 1.0,
    }
}

fn sim(p: BatteryParams) -> Scheduler {
    let mut bus = Bus::new();
    let battery = Battery::new(p, 4, 8, &mut bus);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(battery));
    s
}

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

fn load(s: &mut Scheduler, amps: f64) {
    let sig = s.bus().lookup::<f64>(&names::esc_bus_current(0)).unwrap();
    s.bus_mut().set(sig, amps);
}

#[test]
fn open_circuit_voltage_is_cells_times_ocv() {
    let mut s = sim(params());
    assert!((get(&s, names::BATTERY_VOLTAGE) - 25.2).abs() < 1e-12);
    s.run_for(0.01).unwrap();
    assert!((get(&s, names::BATTERY_VOLTAGE) - 25.2).abs() < 1e-12);
}

#[test]
fn load_step_sags_instantly_then_relaxes() {
    let mut s = sim(params());
    load(&mut s, 10.0);
    s.step().unwrap();
    assert!((get(&s, names::BATTERY_VOLTAGE) - (25.2 - 0.24)).abs() < 1e-3);
    s.run_for(100.0).unwrap();
    let soc = 1.0 - 10.0 * 100.0 / (1300.0 * 3.6);
    let expected = 6.0 * interp::linear(&lipo(), soc) - 0.24 - 0.1 * (1.0 - (-5.0f64).exp());
    assert!((get(&s, names::BATTERY_SOC) - soc).abs() < 1e-4);
    assert!((get(&s, names::BATTERY_VOLTAGE) - expected).abs() < 1e-3);
    assert_eq!(get(&s, names::BATTERY_CURRENT), 10.0);
}

#[test]
fn consumed_capacity_integrates_current() {
    let mut s = sim(params());
    load(&mut s, 10.0);
    s.run_for(360.0).unwrap();
    assert!((get(&s, names::BATTERY_CONSUMED) - 1000.0).abs() < 0.01);
}

#[test]
fn empty_battery_clamps_state_of_charge() {
    let mut p = params();
    p.capacity_mah = 1.0;
    let mut s = sim(p);
    load(&mut s, 10.0);
    s.run_for(1.0).unwrap();
    assert_eq!(get(&s, names::BATTERY_SOC), 0.0);
    assert!(get(&s, names::BATTERY_VOLTAGE).is_finite());
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p ofs-electrical`
Expected: compile error (`Battery`, `BatteryParams` not found).

- [ ] **Step 4: Implement**

`crates/ofs-electrical/src/battery.rs`:
```rust
//! LiPo pack as a Thevenin equivalent circuit: OCV(SoC) − I·R0 − V1, with dV1/dt = I/C1 − V1/(R1·C1).
use ofs_core::{interp, names, Bus, Model, Signal, SimError, StepCtx};

#[derive(Debug, Clone)]
pub struct BatteryParams {
    pub cells: u32,
    pub capacity_mah: f64,
    /// Pack ohmic resistance.
    pub r0_ohm: f64,
    /// Pack polarization resistance and capacitance.
    pub r1_ohm: f64,
    pub c1_f: f64,
    /// (state of charge 0..1, cell open-circuit volts), sorted by state of charge.
    pub ocv_table: Vec<(f64, f64)>,
    pub initial_soc: f64,
}

pub struct Battery {
    p: BatteryParams,
    div: u32,
    soc: f64,
    v1: f64,
    consumed_mah: f64,
    bus_currents: Vec<Signal<f64>>,
    voltage: Signal<f64>,
    current: Signal<f64>,
    soc_sig: Signal<f64>,
    consumed: Signal<f64>,
}

impl Battery {
    pub fn new(p: BatteryParams, motor_count: usize, rate_divisor: u32, bus: &mut Bus) -> Self {
        let b = Self {
            bus_currents: (0..motor_count).map(|i| bus.signal(&names::esc_bus_current(i))).collect(),
            voltage: bus.signal(names::BATTERY_VOLTAGE),
            current: bus.signal(names::BATTERY_CURRENT),
            soc_sig: bus.signal(names::BATTERY_SOC),
            consumed: bus.signal(names::BATTERY_CONSUMED),
            div: rate_divisor,
            soc: p.initial_soc,
            v1: 0.0,
            consumed_mah: 0.0,
            p,
        };
        b.publish(bus, 0.0);
        b
    }

    fn publish(&self, bus: &mut Bus, current: f64) {
        let ocv = f64::from(self.p.cells) * interp::linear(&self.p.ocv_table, self.soc);
        bus.set(self.voltage, ocv - current * self.p.r0_ohm - self.v1);
        bus.set(self.current, current);
        bus.set(self.soc_sig, self.soc);
        bus.set(self.consumed, self.consumed_mah);
    }
}

impl Model for Battery {
    fn name(&self) -> &str {
        "battery"
    }

    fn rate_divisor(&self) -> u32 {
        self.div
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let dt = ctx.dt_s;
        let current: f64 = self.bus_currents.iter().map(|s| bus.get(*s)).sum();
        let decay = (-dt / (self.p.r1_ohm * self.p.c1_f)).exp();
        self.v1 = self.v1 * decay + current * self.p.r1_ohm * (1.0 - decay);
        let used_mah = current * dt / 3.6;
        self.consumed_mah += used_mah;
        self.soc = (self.soc - used_mah / self.p.capacity_mah).clamp(0.0, 1.0);
        self.publish(bus, current);
        Ok(())
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p ofs-electrical`
Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/ofs-electrical
git commit -m "feat(electrical): Thevenin battery model"
```

---

### Task 6: ESC + brushless motor model

**Files:**
- Create: `crates/ofs-electrical/src/esc_motor.rs`
- Modify: `crates/ofs-electrical/src/lib.rs`
- Test: `crates/ofs-electrical/tests/esc_motor.rs`

**Interfaces:**
- Consumes: `names::{motor_cmd, BATTERY_VOLTAGE, prop_torque}`.
- Produces: `MotorParams { kv_rpm_per_v, resistance_ohm, no_load_current_a, rotor_inertia_kgm2 }`, `EscParams { response_tau_s, current_limit_a }` (all f64); `EscMotor::new(index: usize, esc: EscParams, motor: MotorParams, prop_inertia_kgm2: f64, bus: &mut Bus)` — model name `"esc_motor.{index}"`, rate divisor 1. Reads `motor_cmd(i)`, `BATTERY_VOLTAGE`, `prop_torque(i)`; writes `motor_omega(i)`, `motor_omega_dot(i)`, `motor_current(i)`, `esc_bus_current(i)`, `esc_duty(i)`.

- [ ] **Step 1: Write the failing tests**

`crates/ofs-electrical/tests/esc_motor.rs`:
```rust
use std::f64::consts::PI;

use ofs_core::{names, Bus, Scheduler};
use ofs_electrical::esc_motor::{EscMotor, EscParams, MotorParams};

fn sim(volts: f64, cmd: f64, no_load_a: f64) -> Scheduler {
    let mut bus = Bus::new();
    let m = EscMotor::new(
        0,
        EscParams { response_tau_s: 0.002, current_limit_a: 40.0 },
        MotorParams { kv_rpm_per_v: 2000.0, resistance_ohm: 0.1, no_load_current_a: no_load_a, rotor_inertia_kgm2: 5e-6 },
        5e-6,
        &mut bus,
    );
    let v = bus.signal::<f64>(names::BATTERY_VOLTAGE);
    bus.set(v, volts);
    let c = bus.signal::<f64>(&names::motor_cmd(0));
    bus.set(c, cmd);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(m));
    s
}

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

#[test]
fn unloaded_motor_reaches_kv_times_voltage_minus_resistive_drop() {
    let mut s = sim(10.0, 1.0, 1.0);
    s.run_for(2.0).unwrap();
    let expected = (10.0 - 1.0 * 0.1) * 2000.0 * 2.0 * PI / 60.0;
    let omega = get(&s, &names::motor_omega(0));
    assert!(((omega - expected) / expected).abs() < 1e-3, "omega {omega} expected {expected}");
    assert!((get(&s, &names::motor_current(0)) - 1.0).abs() < 1e-2);
}

#[test]
fn startup_current_is_limited_by_the_esc() {
    let mut s = sim(16.0, 1.0, 0.0);
    for _ in 0..16 {
        s.step().unwrap();
    }
    assert_eq!(get(&s, &names::motor_current(0)), 40.0);
}

#[test]
fn duty_follows_command_with_first_order_lag() {
    let mut s = sim(10.0, 1.0, 0.0);
    for _ in 0..16 {
        s.step().unwrap(); // 16 ticks = 2 ms = one time constant
    }
    let duty = get(&s, &names::esc_duty(0));
    assert!((duty - (1.0 - (-1.0f64).exp())).abs() < 1e-9, "duty {duty}");
}

#[test]
fn zero_command_brakes_without_reversing() {
    let mut s = sim(10.0, 1.0, 0.5);
    s.run_for(1.0).unwrap();
    let cmd = s.bus().lookup::<f64>(&names::motor_cmd(0)).unwrap();
    s.bus_mut().set(cmd, 0.0);
    s.run_for(0.01).unwrap();
    assert!(get(&s, &names::motor_current(0)) < 0.0, "active braking draws negative current");
    s.run_for(2.0).unwrap();
    assert!(get(&s, &names::motor_omega(0)) >= 0.0);
    assert!(get(&s, &names::motor_omega(0)) < 1.0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ofs-electrical --test esc_motor`
Expected: compile error (`esc_motor` module not found).

- [ ] **Step 3: Implement**

`crates/ofs-electrical/src/esc_motor.rs`:
```rust
//! Averaged ESC + brushless DC motor (winding inductance neglected).
//! duty lags the command; I = (duty·Vbus − Ke·ω)/R clamped to the ESC limit;
//! J·dω/dt = Ke·I − Ke·I0 − Q_prop; bus current = duty·I.
use std::f64::consts::PI;

use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};

#[derive(Debug, Clone)]
pub struct MotorParams {
    pub kv_rpm_per_v: f64,
    pub resistance_ohm: f64,
    pub no_load_current_a: f64,
    pub rotor_inertia_kgm2: f64,
}

#[derive(Debug, Clone)]
pub struct EscParams {
    pub response_tau_s: f64,
    pub current_limit_a: f64,
}

pub struct EscMotor {
    name: String,
    esc: EscParams,
    motor: MotorParams,
    inertia_kgm2: f64,
    ke: f64,
    duty: f64,
    omega: f64,
    cmd: Signal<f64>,
    vbus: Signal<f64>,
    load: Signal<f64>,
    omega_sig: Signal<f64>,
    omega_dot_sig: Signal<f64>,
    current_sig: Signal<f64>,
    bus_current_sig: Signal<f64>,
    duty_sig: Signal<f64>,
}

impl EscMotor {
    pub fn new(index: usize, esc: EscParams, motor: MotorParams, prop_inertia_kgm2: f64, bus: &mut Bus) -> Self {
        Self {
            name: format!("esc_motor.{index}"),
            ke: 60.0 / (2.0 * PI * motor.kv_rpm_per_v),
            inertia_kgm2: motor.rotor_inertia_kgm2 + prop_inertia_kgm2,
            esc,
            motor,
            duty: 0.0,
            omega: 0.0,
            cmd: bus.signal(&names::motor_cmd(index)),
            vbus: bus.signal(names::BATTERY_VOLTAGE),
            load: bus.signal(&names::prop_torque(index)),
            omega_sig: bus.signal(&names::motor_omega(index)),
            omega_dot_sig: bus.signal(&names::motor_omega_dot(index)),
            current_sig: bus.signal(&names::motor_current(index)),
            bus_current_sig: bus.signal(&names::esc_bus_current(index)),
            duty_sig: bus.signal(&names::esc_duty(index)),
        }
    }
}

impl Model for EscMotor {
    fn name(&self) -> &str {
        &self.name
    }

    fn rate_divisor(&self) -> u32 {
        1
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let dt = ctx.dt_s;
        let target = bus.get(self.cmd).clamp(0.0, 1.0);
        self.duty += (target - self.duty) * (1.0 - (-dt / self.esc.response_tau_s).exp());

        let applied_v = self.duty * bus.get(self.vbus);
        let limit = self.esc.current_limit_a;
        let current = ((applied_v - self.ke * self.omega) / self.motor.resistance_ohm).clamp(-limit, limit);
        let friction = if self.omega > 0.0 { self.ke * self.motor.no_load_current_a } else { 0.0 };
        let net_torque = self.ke * current - friction - bus.get(self.load);
        let omega_dot = net_torque / self.inertia_kgm2;
        self.omega = (self.omega + omega_dot * dt).max(0.0); // no reversing in v1 (3D mode unsupported)

        bus.set(self.omega_sig, self.omega);
        bus.set(self.omega_dot_sig, omega_dot);
        bus.set(self.current_sig, current);
        bus.set(self.bus_current_sig, self.duty * current);
        bus.set(self.duty_sig, self.duty);
        Ok(())
    }
}
```

`crates/ofs-electrical/src/lib.rs` (replace whole file):
```rust
//! Electrical models: battery, ESC + brushless motor.
pub mod battery;
pub mod esc_motor;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ofs-electrical`
Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-electrical
git commit -m "feat(electrical): averaged ESC and brushless motor model"
```

---

### Task 7: IMU and barometer models

**Files:**
- Create: `crates/ofs-sensors/Cargo.toml`, `crates/ofs-sensors/src/lib.rs`, `crates/ofs-sensors/src/imu.rs`, `crates/ofs-sensors/src/baro.rs`
- Test: `crates/ofs-sensors/tests/sensors.rs`

**Interfaces:**
- Consumes: `names::{BODY_RATE_FRD, BODY_ATT, BODY_ACCEL_NED, BODY_POS_NED}`, `consts::GRAVITY_MPS2`, `rng::model_rng`.
- Produces: `ImuParams { gyro_noise_std_radps: f64, gyro_bias_radps: DVec3, accel_noise_std_mps2: f64, accel_bias_mps2: DVec3 }`, `Imu::new(p, seed: u64, bus: &mut Bus)` (model `"imu"`, divisor 1, writes `IMU_GYRO`, `IMU_ACCEL`); `BaroParams { noise_std_pa: f64, home_alt_m: f64 }`, `Baro::new(p, seed, bus)` (model `"baro"`, divisor 1, writes `BARO_PRESSURE`); `isa_pressure_pa(alt_m: f64) -> f64`.

- [ ] **Step 1: Create the crate**

`crates/ofs-sensors/Cargo.toml`:
```toml
[package]
name = "ofs-sensors"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
ofs-core.workspace = true
glam.workspace = true
rand.workspace = true
rand_chacha.workspace = true
rand_distr.workspace = true
```

`crates/ofs-sensors/src/lib.rs`:
```rust
//! Sensor models: IMU, barometer.
pub mod baro;
pub mod imu;
```

- [ ] **Step 2: Write the failing tests**

`crates/ofs-sensors/tests/sensors.rs`:
```rust
use std::f64::consts::FRAC_PI_2;

use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Scheduler};
use ofs_sensors::baro::{isa_pressure_pa, Baro, BaroParams};
use ofs_sensors::imu::{Imu, ImuParams};

fn quiet_imu() -> ImuParams {
    ImuParams {
        gyro_noise_std_radps: 0.0,
        gyro_bias_radps: DVec3::ZERO,
        accel_noise_std_mps2: 0.0,
        accel_bias_mps2: DVec3::ZERO,
    }
}

fn imu_sim(p: ImuParams, seed: u64, att: DQuat, rate: DVec3) -> Scheduler {
    let mut bus = Bus::new();
    let imu = Imu::new(p, seed, &mut bus);
    let a = bus.signal::<DQuat>(names::BODY_ATT);
    bus.set(a, att);
    let r = bus.signal::<DVec3>(names::BODY_RATE_FRD);
    bus.set(r, rate);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(imu));
    s
}

fn vec3(s: &Scheduler, name: &str) -> DVec3 {
    s.bus().get(s.bus().lookup::<DVec3>(name).unwrap())
}

#[test]
fn level_at_rest_measures_one_g_up() {
    let mut s = imu_sim(quiet_imu(), 0, DQuat::IDENTITY, DVec3::ZERO);
    s.step().unwrap();
    assert!((vec3(&s, names::IMU_ACCEL) - DVec3::new(0.0, 0.0, -9.80665)).length() < 1e-12);
}

#[test]
fn rolled_right_ninety_degrees_feels_gravity_toward_left_wing() {
    let mut s = imu_sim(quiet_imu(), 0, DQuat::from_rotation_x(FRAC_PI_2), DVec3::ZERO);
    s.step().unwrap();
    assert!((vec3(&s, names::IMU_ACCEL) - DVec3::new(0.0, -9.80665, 0.0)).length() < 1e-9);
}

#[test]
fn gyro_reports_rate_plus_bias() {
    let mut p = quiet_imu();
    p.gyro_bias_radps = DVec3::new(0.01, 0.0, -0.02);
    let mut s = imu_sim(p, 0, DQuat::IDENTITY, DVec3::new(1.0, 2.0, 3.0));
    s.step().unwrap();
    assert!((vec3(&s, names::IMU_GYRO) - DVec3::new(1.01, 2.0, 2.98)).length() < 1e-12);
}

fn gyro_x_samples(seed: u64, n: usize) -> Vec<f64> {
    let mut p = quiet_imu();
    p.gyro_noise_std_radps = 0.01;
    let mut s = imu_sim(p, seed, DQuat::IDENTITY, DVec3::ZERO);
    (0..n)
        .map(|_| {
            s.step().unwrap();
            vec3(&s, names::IMU_GYRO).x
        })
        .collect()
}

#[test]
fn gyro_noise_has_configured_std_and_is_seeded() {
    let xs = gyro_x_samples(7, 20_000);
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    let std = (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64).sqrt();
    assert!((std - 0.01).abs() < 0.0005, "std {std}");
    assert_eq!(gyro_x_samples(7, 10), xs[..10].to_vec());
    assert_ne!(gyro_x_samples(8, 10), xs[..10].to_vec());
}

#[test]
fn barometer_follows_standard_atmosphere() {
    assert!((isa_pressure_pa(0.0) - 101_325.0).abs() < 1e-9);
    assert!((isa_pressure_pa(1000.0) - 89_874.6).abs() < 0.5);
    let mut bus = Bus::new();
    let baro = Baro::new(BaroParams { noise_std_pa: 0.0, home_alt_m: 1000.0 }, 0, &mut bus);
    let pos = bus.signal::<DVec3>(names::BODY_POS_NED);
    bus.set(pos, DVec3::new(0.0, 0.0, -10.0));
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(baro));
    s.step().unwrap();
    let p = s.bus().get(s.bus().lookup::<f64>(names::BARO_PRESSURE).unwrap());
    assert!((p - isa_pressure_pa(1010.0)).abs() < 1e-9);
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p ofs-sensors`
Expected: compile errors (modules empty / items not found).

- [ ] **Step 4: Implement**

`crates/ofs-sensors/src/imu.rs`:
```rust
//! 6-axis IMU: gyro = body rate + bias + noise; accel = specific force R^T(a − g) + bias + noise.
use glam::{DQuat, DVec3};
use ofs_core::consts::GRAVITY_MPS2;
use ofs_core::{names, rng::model_rng, Bus, Model, Signal, SimError, StepCtx};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, Normal};

#[derive(Debug, Clone)]
pub struct ImuParams {
    pub gyro_noise_std_radps: f64,
    pub gyro_bias_radps: DVec3,
    pub accel_noise_std_mps2: f64,
    pub accel_bias_mps2: DVec3,
}

pub struct Imu {
    p: ImuParams,
    rng: ChaCha8Rng,
    gyro_noise: Normal<f64>,
    accel_noise: Normal<f64>,
    rate: Signal<DVec3>,
    att: Signal<DQuat>,
    accel_ned: Signal<DVec3>,
    gyro_out: Signal<DVec3>,
    accel_out: Signal<DVec3>,
}

impl Imu {
    pub fn new(p: ImuParams, seed: u64, bus: &mut Bus) -> Self {
        Self {
            rng: model_rng(seed, "imu"),
            gyro_noise: Normal::new(0.0, p.gyro_noise_std_radps).expect("gyro noise std must be finite and >= 0"),
            accel_noise: Normal::new(0.0, p.accel_noise_std_mps2).expect("accel noise std must be finite and >= 0"),
            rate: bus.signal(names::BODY_RATE_FRD),
            att: bus.signal(names::BODY_ATT),
            accel_ned: bus.signal(names::BODY_ACCEL_NED),
            gyro_out: bus.signal(names::IMU_GYRO),
            accel_out: bus.signal(names::IMU_ACCEL),
            p,
        }
    }

    fn noise3(&mut self, d: Normal<f64>) -> DVec3 {
        DVec3::new(d.sample(&mut self.rng), d.sample(&mut self.rng), d.sample(&mut self.rng))
    }
}

impl Model for Imu {
    fn name(&self) -> &str {
        "imu"
    }

    fn rate_divisor(&self) -> u32 {
        1
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let gravity = DVec3::new(0.0, 0.0, GRAVITY_MPS2);
        let specific_force = bus.get(self.att).inverse() * (bus.get(self.accel_ned) - gravity);
        let gyro = bus.get(self.rate) + self.p.gyro_bias_radps + self.noise3(self.gyro_noise);
        let accel = specific_force + self.p.accel_bias_mps2 + self.noise3(self.accel_noise);
        bus.set(self.gyro_out, gyro);
        bus.set(self.accel_out, accel);
        Ok(())
    }
}
```

`crates/ofs-sensors/src/baro.rs`:
```rust
//! Barometer: ISA pressure at home altitude + height above home, plus noise.
use glam::DVec3;
use ofs_core::{names, rng::model_rng, Bus, Model, Signal, SimError, StepCtx};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, Normal};

#[derive(Debug, Clone)]
pub struct BaroParams {
    pub noise_std_pa: f64,
    pub home_alt_m: f64,
}

/// International Standard Atmosphere pressure (troposphere).
pub fn isa_pressure_pa(alt_m: f64) -> f64 {
    101_325.0 * (1.0 - 2.25577e-5 * alt_m).powf(5.25588)
}

pub struct Baro {
    p: BaroParams,
    rng: ChaCha8Rng,
    noise: Normal<f64>,
    pos: Signal<DVec3>,
    out: Signal<f64>,
}

impl Baro {
    pub fn new(p: BaroParams, seed: u64, bus: &mut Bus) -> Self {
        Self {
            rng: model_rng(seed, "baro"),
            noise: Normal::new(0.0, p.noise_std_pa).expect("baro noise std must be finite and >= 0"),
            pos: bus.signal(names::BODY_POS_NED),
            out: bus.signal(names::BARO_PRESSURE),
            p,
        }
    }
}

impl Model for Baro {
    fn name(&self) -> &str {
        "baro"
    }

    fn rate_divisor(&self) -> u32 {
        1
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let alt = self.p.home_alt_m - bus.get(self.pos).z;
        bus.set(self.out, isa_pressure_pa(alt) + self.noise.sample(&mut self.rng));
        Ok(())
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p ofs-sensors`
Expected: 5 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/ofs-sensors
git commit -m "feat(sensors): IMU and barometer models with seeded noise"
```

---

### Task 8: Quad configuration (TOML) with validation

**Files:**
- Create: `crates/ofs-config/Cargo.toml`, `crates/ofs-config/src/lib.rs`
- Create: `quads/opendrone-5f-freestyle.toml`, `quads/opendrone-5f-freestyle.betaflight.diff`
- Test: `crates/ofs-config/tests/config.rs`

**Interfaces:**
- Produces:
  - `ofs_config::load(path: &Path) -> Result<QuadConfig, ConfigError>`.
  - `QuadConfig` (Clone, Debug) with public sections `sim, frame, ground, motor, prop, esc, battery, imu, baro, home, initial, fc` (field names exactly as in the TOML below), `schema_version: u32`, `name: String`, `source_path: PathBuf` (set by `load`), methods `source_dir(&self) -> &Path`, `resolve(&self, rel: &str) -> PathBuf`, `validate(&self) -> Vec<Problem>`.
  - `FcKind` (Copy, Eq): `Sitl`, `OpenLoop` (TOML `"sitl"`, `"open_loop"`).
  - `Problem { field: String, message: String }`; `ConfigError::{Io, Parse, Invalid}` (Display lists every problem).
  - `pub const SCHEMA_VERSION: u32 = 1`.

- [ ] **Step 1: Create the reference quad files**

`quads/opendrone-5f-freestyle.toml`:
```toml
# OpenDrone 5" freestyle — M1 estimates. Units are in the key suffixes (SI).
# Provenance tracking per parameter arrives with the component library (M4); all values here are estimates.
schema_version = 1
name = "OpenDrone 5F freestyle (M1 estimates)"

[sim]
base_hz = 8000

[frame]
mass_kg = 0.65
inertia_kgm2 = [0.0025, 0.0025, 0.0045]
cda_m2 = [0.010, 0.010, 0.020]
angular_damping_nms = 0.0001
# Betaflight Quad-X order: M1 rear-right, M2 front-right, M3 rear-left, M4 front-left (FRD: x fwd, y right, z down)
motor_positions_frd_m = [[-0.08, 0.08, 0.0], [0.08, 0.08, 0.0], [-0.08, -0.08, 0.0], [0.08, -0.08, 0.0]]
# +1 = clockwise seen from above. Betaflight default: M1 CW, M2 CCW, M3 CCW, M4 CW.
motor_spin = [1, -1, -1, 1]
contact_points_frd_m = [[-0.08, 0.08, 0.03], [0.08, 0.08, 0.03], [-0.08, -0.08, 0.03], [0.08, -0.08, 0.03]]

[ground]
stiffness_npm = 3000.0
damping_nspm = 40.0
friction_coeff = 0.6

[motor]
kv_rpm_per_v = 1950.0
resistance_ohm = 0.07
no_load_current_a = 1.2
rotor_inertia_kgm2 = 6.0e-6

[prop]
diameter_m = 0.127
inertia_kgm2 = 6.0e-6
ct_table = [[0.0, 0.11], [30000.0, 0.11]]
cp_table = [[0.0, 0.045], [30000.0, 0.045]]

[esc]
response_tau_s = 0.002
current_limit_a = 45.0

[battery]
cells = 6
capacity_mah = 1300.0
r0_ohm = 0.024
r1_ohm = 0.010
c1_f = 2000.0
initial_soc = 1.0
rate_hz = 1000
ocv_table = [[0.0, 3.27], [0.05, 3.61], [0.1, 3.69], [0.2, 3.73], [0.3, 3.77], [0.4, 3.79],
             [0.5, 3.82], [0.6, 3.87], [0.7, 3.92], [0.8, 3.97], [0.9, 4.06], [1.0, 4.20]]

[imu]
gyro_noise_std_radps = 0.002
gyro_bias_radps = [0.0, 0.0, 0.0]
accel_noise_std_mps2 = 0.05
accel_bias_mps2 = [0.0, 0.0, 0.0]

[baro]
noise_std_pa = 2.0

[home]
lat_deg = 50.85
lon_deg = 4.35
alt_m = 30.0

[initial]
position_ned_m = [0.0, 0.0, -0.03]
yaw_deg = 0.0

[fc]
kind = "sitl"
exchange_hz = 1000
# Overridden by the OFS_SITL_LAUNCH / OFS_SITL_CLEANUP environment variables (space-separated argv).
# Under WSL (launch[0] = wsl.exe) an empty cleanup defaults to "<wsl prefix> pkill -x betaflight_SITL".
launch = ["betaflight_SITL.elf"]
cleanup = []
betaflight_diff = "opendrone-5f-freestyle.betaflight.diff"
reply_timeout_ms = 500
first_reply_timeout_ms = 5000
startup_timeout_ms = 15000
```

`quads/opendrone-5f-freestyle.betaflight.diff` (replace with M0 §3's final arming diff if it differs):
```
# Applied once, on first boot, with `betaflight_SITL.elf --config`.
feature -GPS
aux 0 0 0 1700 2100 0 0
aux 1 1 1 1700 2100 0 0
set motor_pwm_protocol = PWM
set small_angle = 180
```

- [ ] **Step 2: Create the crate manifest**

`crates/ofs-config/Cargo.toml`:
```toml
[package]
name = "ofs-config"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
serde.workspace = true
toml.workspace = true
thiserror.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

- [ ] **Step 3: Write the failing tests**

`crates/ofs-config/tests/config.rs`:
```rust
use std::fs;
use std::path::Path;

use ofs_config::{load, ConfigError, FcKind};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn quad_text() -> String {
    fs::read_to_string(QUAD).unwrap()
}

fn write_quad(dir: &Path, text: &str) -> std::path::PathBuf {
    let path = dir.join("quad.toml");
    fs::write(&path, text).unwrap();
    fs::write(dir.join("opendrone-5f-freestyle.betaflight.diff"), "feature -GPS\n").unwrap();
    path
}

#[test]
fn reference_quad_loads_and_validates() {
    let cfg = load(Path::new(QUAD)).unwrap();
    assert_eq!(cfg.schema_version, 1);
    assert_eq!(cfg.frame.motor_positions_frd_m.len(), 4);
    assert_eq!(cfg.fc.kind, FcKind::Sitl);
    assert!(cfg.validate().is_empty());
}

#[test]
fn every_problem_is_reported_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text()
        .replace("mass_kg = 0.65", "mass_kg = -1.0")
        .replace("initial_soc = 1.0", "initial_soc = 2.0")
        .replace("motor_spin = [1, -1, -1, 1]", "motor_spin = [1, -1, -1, 2]");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("expected Invalid, got {err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    assert!(fields.contains(&"frame.mass_kg"), "{fields:?}");
    assert!(fields.contains(&"battery.initial_soc"), "{fields:?}");
    assert!(fields.contains(&"frame.motor_spin"), "{fields:?}");
    let shown = err.to_string();
    assert!(shown.contains("frame.mass_kg") && shown.contains("battery.initial_soc"));
}

#[test]
fn unknown_field_is_a_parse_error_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("mass_kg = 0.65", "mass_kg = 0.65\nmas_kg = 1.0");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }));
    assert!(err.to_string().contains("mas_kg"), "{err}");
}

#[test]
fn unsupported_schema_version_is_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("schema_version = 1", "schema_version = 2");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(err.to_string().contains("schema_version 2"), "{err}");
}

#[test]
fn diff_path_resolves_relative_to_quad_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let cfg = load(&path).unwrap();
    assert_eq!(cfg.resolve(&cfg.fc.betaflight_diff), dir.path().join("opendrone-5f-freestyle.betaflight.diff"));
}

#[test]
fn missing_diff_file_is_reported_for_sitl_quads() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    fs::remove_file(dir.path().join("opendrone-5f-freestyle.betaflight.diff")).unwrap();
    let err = load(&path).unwrap_err();
    assert!(err.to_string().contains("fc.betaflight_diff"), "{err}");
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p ofs-config`
Expected: compile error (`load`, `ConfigError`, `FcKind` not found).

- [ ] **Step 5: Implement**

`crates/ofs-config/src/lib.rs`:
```rust
//! Quad description files (TOML): parsing and validation that reports every problem at once.
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuadConfig {
    pub schema_version: u32,
    pub name: String,
    pub sim: SimSection,
    pub frame: FrameSection,
    pub ground: GroundSection,
    pub motor: MotorSection,
    pub prop: PropSection,
    pub esc: EscSection,
    pub battery: BatterySection,
    pub imu: ImuSection,
    pub baro: BaroSection,
    pub home: HomeSection,
    pub initial: InitialSection,
    pub fc: FcSection,
    #[serde(skip)]
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimSection {
    pub base_hz: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameSection {
    pub mass_kg: f64,
    pub inertia_kgm2: [f64; 3],
    pub cda_m2: [f64; 3],
    pub angular_damping_nms: f64,
    pub motor_positions_frd_m: Vec<[f64; 3]>,
    pub motor_spin: Vec<i8>,
    pub contact_points_frd_m: Vec<[f64; 3]>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundSection {
    pub stiffness_npm: f64,
    pub damping_nspm: f64,
    pub friction_coeff: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotorSection {
    pub kv_rpm_per_v: f64,
    pub resistance_ohm: f64,
    pub no_load_current_a: f64,
    pub rotor_inertia_kgm2: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropSection {
    pub diameter_m: f64,
    pub inertia_kgm2: f64,
    pub ct_table: Vec<[f64; 2]>,
    pub cp_table: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EscSection {
    pub response_tau_s: f64,
    pub current_limit_a: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatterySection {
    pub cells: u32,
    pub capacity_mah: f64,
    pub r0_ohm: f64,
    pub r1_ohm: f64,
    pub c1_f: f64,
    pub initial_soc: f64,
    pub rate_hz: u32,
    pub ocv_table: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImuSection {
    pub gyro_noise_std_radps: f64,
    pub gyro_bias_radps: [f64; 3],
    pub accel_noise_std_mps2: f64,
    pub accel_bias_mps2: [f64; 3],
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaroSection {
    pub noise_std_pa: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeSection {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_m: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialSection {
    pub position_ned_m: [f64; 3],
    pub yaw_deg: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FcKind {
    Sitl,
    OpenLoop,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FcSection {
    pub kind: FcKind,
    pub exchange_hz: u32,
    pub launch: Vec<String>,
    #[serde(default)]
    pub cleanup: Vec<String>,
    pub betaflight_diff: String,
    #[serde(default = "default_reply_timeout_ms")]
    pub reply_timeout_ms: u64,
    /// The first exchange waits longer: TCP 5761 accepting does not mean the main loop is running.
    #[serde(default = "default_first_reply_timeout_ms")]
    pub first_reply_timeout_ms: u64,
    #[serde(default = "default_startup_timeout_ms")]
    pub startup_timeout_ms: u64,
}

fn default_reply_timeout_ms() -> u64 {
    500
}

fn default_first_reply_timeout_ms() -> u64 {
    5_000
}

fn default_startup_timeout_ms() -> u64 {
    15_000
}

#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub field: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{path}: cannot read file: {source}", path = .path.display())]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{path}: {message}", path = .path.display())]
    Parse { path: PathBuf, message: String },
    #[error("{path}: {count} problem(s):\n{list}", path = .path.display(), count = .problems.len(), list = format_problems(.problems))]
    Invalid { path: PathBuf, problems: Vec<Problem> },
}

fn format_problems(problems: &[Problem]) -> String {
    let mut out = String::new();
    for p in problems {
        let _ = writeln!(out, "  - {}: {}", p.field, p.message);
    }
    out
}

pub fn load(path: &Path) -> Result<QuadConfig, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io { path: path.to_path_buf(), source })?;
    let parse_err = |message: String| ConfigError::Parse { path: path.to_path_buf(), message };
    let raw: toml::Value = toml::from_str(&text).map_err(|e| parse_err(e.to_string()))?;
    match raw.get("schema_version").and_then(toml::Value::as_integer) {
        Some(v) if v == i64::from(SCHEMA_VERSION) => {}
        Some(v) => return Err(parse_err(format!("unsupported schema_version {v} (this build reads {SCHEMA_VERSION})"))),
        None => return Err(parse_err("missing integer schema_version".into())),
    }
    let mut cfg: QuadConfig = toml::from_str(&text).map_err(|e| parse_err(e.to_string()))?;
    cfg.source_path = path.to_path_buf();
    let problems = cfg.validate();
    if !problems.is_empty() {
        return Err(ConfigError::Invalid { path: path.to_path_buf(), problems });
    }
    Ok(cfg)
}

struct Checker(Vec<Problem>);

impl Checker {
    fn check(&mut self, ok: bool, field: &str, message: impl Into<String>) {
        if !ok {
            self.0.push(Problem { field: field.to_string(), message: message.into() });
        }
    }

    fn positive(&mut self, v: f64, field: &str) {
        self.check(v.is_finite() && v > 0.0, field, format!("must be > 0 (got {v})"));
    }

    fn non_negative(&mut self, v: f64, field: &str) {
        self.check(v.is_finite() && v >= 0.0, field, format!("must be >= 0 (got {v})"));
    }

    fn table(&mut self, t: &[[f64; 2]], field: &str) {
        self.check(!t.is_empty(), field, "must have at least one row");
        self.check(t.windows(2).all(|w| w[1][0] > w[0][0]), field, "first column must be strictly increasing");
        self.check(t.iter().flatten().all(|v| v.is_finite()), field, "values must be finite");
    }

    fn divides(&mut self, base_hz: u32, hz: u32, field: &str) {
        self.check(hz > 0 && base_hz % hz == 0, field, format!("must be > 0 and divide sim.base_hz {base_hz} (got {hz})"));
    }
}

impl QuadConfig {
    pub fn source_dir(&self) -> &Path {
        self.source_path.parent().unwrap_or_else(|| Path::new("."))
    }

    /// Resolves a path written in the quad file relative to the quad file's directory.
    pub fn resolve(&self, rel: &str) -> PathBuf {
        self.source_dir().join(rel)
    }

    pub fn validate(&self) -> Vec<Problem> {
        let mut c = Checker(Vec::new());
        c.check(self.sim.base_hz > 0, "sim.base_hz", "must be > 0");

        let f = &self.frame;
        c.positive(f.mass_kg, "frame.mass_kg");
        f.inertia_kgm2.iter().for_each(|v| c.positive(*v, "frame.inertia_kgm2"));
        f.cda_m2.iter().for_each(|v| c.non_negative(*v, "frame.cda_m2"));
        c.non_negative(f.angular_damping_nms, "frame.angular_damping_nms");
        c.check(
            f.motor_positions_frd_m.len() == 4,
            "frame.motor_positions_frd_m",
            format!("v1 supports exactly 4 motors (Betaflight SITL outputs 4), got {}", f.motor_positions_frd_m.len()),
        );
        c.check(f.motor_spin.len() == f.motor_positions_frd_m.len(), "frame.motor_spin", "needs one entry per motor");
        c.check(f.motor_spin.iter().all(|s| *s == 1 || *s == -1), "frame.motor_spin", "entries must be 1 or -1");
        c.check(!f.contact_points_frd_m.is_empty(), "frame.contact_points_frd_m", "needs at least one point");

        c.positive(self.ground.stiffness_npm, "ground.stiffness_npm");
        c.non_negative(self.ground.damping_nspm, "ground.damping_nspm");
        c.non_negative(self.ground.friction_coeff, "ground.friction_coeff");

        c.positive(self.motor.kv_rpm_per_v, "motor.kv_rpm_per_v");
        c.positive(self.motor.resistance_ohm, "motor.resistance_ohm");
        c.non_negative(self.motor.no_load_current_a, "motor.no_load_current_a");
        c.positive(self.motor.rotor_inertia_kgm2, "motor.rotor_inertia_kgm2");

        c.positive(self.prop.diameter_m, "prop.diameter_m");
        c.non_negative(self.prop.inertia_kgm2, "prop.inertia_kgm2");
        c.table(&self.prop.ct_table, "prop.ct_table");
        c.table(&self.prop.cp_table, "prop.cp_table");

        c.positive(self.esc.response_tau_s, "esc.response_tau_s");
        c.positive(self.esc.current_limit_a, "esc.current_limit_a");

        let b = &self.battery;
        c.check((1..=14).contains(&b.cells), "battery.cells", format!("must be 1..=14 (got {})", b.cells));
        c.positive(b.capacity_mah, "battery.capacity_mah");
        c.non_negative(b.r0_ohm, "battery.r0_ohm");
        c.positive(b.r1_ohm, "battery.r1_ohm");
        c.positive(b.c1_f, "battery.c1_f");
        c.check((0.0..=1.0).contains(&b.initial_soc), "battery.initial_soc", format!("must be in [0, 1] (got {})", b.initial_soc));
        c.table(&b.ocv_table, "battery.ocv_table");
        c.divides(self.sim.base_hz, b.rate_hz, "battery.rate_hz");

        c.non_negative(self.imu.gyro_noise_std_radps, "imu.gyro_noise_std_radps");
        c.non_negative(self.imu.accel_noise_std_mps2, "imu.accel_noise_std_mps2");
        c.non_negative(self.baro.noise_std_pa, "baro.noise_std_pa");

        c.check((-90.0..=90.0).contains(&self.home.lat_deg), "home.lat_deg", "must be in [-90, 90]");
        c.check((-180.0..=180.0).contains(&self.home.lon_deg), "home.lon_deg", "must be in [-180, 180]");

        c.divides(self.sim.base_hz, self.fc.exchange_hz, "fc.exchange_hz");
        if self.fc.kind == FcKind::Sitl {
            c.check(!self.fc.launch.is_empty(), "fc.launch", "must not be empty for kind = \"sitl\"");
            let diff = self.resolve(&self.fc.betaflight_diff);
            c.check(diff.is_file(), "fc.betaflight_diff", format!("file not found: {}", diff.display()));
        }
        c.0
    }
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p ofs-config`
Expected: 6 passed.

- [ ] **Step 7: Commit**

```bash
git add crates/ofs-config quads
git commit -m "feat(config): quad TOML format with all-at-once validation and reference quad"
```

---

### Task 9: Vehicle assembly with open-loop FC

**Files:**
- Create: `crates/ofs-fc/Cargo.toml`, `crates/ofs-fc/src/lib.rs`, `crates/ofs-fc/src/open_loop.rs`
- Create: `crates/ofs-sim/Cargo.toml`, `crates/ofs-sim/src/lib.rs`, `crates/ofs-sim/src/vehicle.rs`
- Test: `crates/ofs-fc/tests/open_loop.rs`, `crates/ofs-sim/tests/open_loop_vehicle.rs`

**Interfaces:**
- Consumes: all Task 1–8 types.
- Produces:
  - `ofs_fc::open_loop::OpenLoopFc::new(motor_count: usize, rate_divisor: u32, bus: &mut Bus)` — model `"fc.open_loop"`; sets every `motor_cmd(i)` = `RC_THROTTLE` clamped to [0, 1].
  - `ofs_sim::vehicle::{build(cfg: &QuadConfig, opts: &BuildOptions) -> Result<Vehicle, SimError>, BuildOptions { seed: u64, data_dir: PathBuf, fc_override: Option<FcKind> }, Sticks { roll, pitch, yaw, throttle: f64, aux: [f64; 4] } (Default: zeros, aux all -1), VehicleState { time_s, pos_ned_m: DVec3, vel_ned_mps: DVec3, att: DQuat, rate_frd_radps: DVec3, battery_voltage_v, battery_current_a: f64, motor_rpm: Vec<f64>, motor_cmd: Vec<f64> }}`.
  - `Vehicle::{set_sticks(&mut self, &Sticks), run_for(&mut self, seconds: f64) -> Result<(), SimError>, state(&self) -> VehicleState, digest(&self) -> u64}`.
  - Model order per tick: 4× `Propeller`, 4× `EscMotor`, `Battery`, `RigidBody`, `Imu`, `Baro`, FC.

- [ ] **Step 1: Create the `ofs-fc` crate with the open-loop FC**

`crates/ofs-fc/Cargo.toml`:
```toml
[package]
name = "ofs-fc"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
ofs-core.workspace = true
glam.workspace = true
thiserror.workspace = true
tracing.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

`crates/ofs-fc/src/lib.rs`:
```rust
//! Flight-controller models: the Betaflight SITL bridge and an open-loop stand-in for tests.
pub mod open_loop;
```

`crates/ofs-fc/tests/open_loop.rs`:
```rust
use ofs_core::{names, Bus, Scheduler};
use ofs_fc::open_loop::OpenLoopFc;

#[test]
fn every_motor_follows_clamped_throttle() {
    let mut bus = Bus::new();
    let fc = OpenLoopFc::new(4, 8, &mut bus);
    let thr = bus.signal::<f64>(names::RC_THROTTLE);
    bus.set(thr, 1.7);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(fc));
    s.step().unwrap();
    for i in 0..4 {
        assert_eq!(s.bus().get(s.bus().lookup::<f64>(&names::motor_cmd(i)).unwrap()), 1.0);
    }
}
```

`crates/ofs-fc/src/open_loop.rs`:
```rust
//! Test stand-in for a flight controller: every motor command = throttle stick. No stabilisation.
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};

pub struct OpenLoopFc {
    div: u32,
    throttle: Signal<f64>,
    cmds: Vec<Signal<f64>>,
}

impl OpenLoopFc {
    pub fn new(motor_count: usize, rate_divisor: u32, bus: &mut Bus) -> Self {
        Self {
            div: rate_divisor,
            throttle: bus.signal(names::RC_THROTTLE),
            cmds: (0..motor_count).map(|i| bus.signal(&names::motor_cmd(i))).collect(),
        }
    }
}

impl Model for OpenLoopFc {
    fn name(&self) -> &str {
        "fc.open_loop"
    }

    fn rate_divisor(&self) -> u32 {
        self.div
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let t = bus.get(self.throttle).clamp(0.0, 1.0);
        for c in &self.cmds {
            bus.set(*c, t);
        }
        Ok(())
    }
}
```

Run: `cargo test -p ofs-fc`
Expected: 1 passed.

- [ ] **Step 2: Create the `ofs-sim` crate manifest and lib**

`crates/ofs-sim/Cargo.toml`:
```toml
[package]
name = "ofs-sim"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
ofs-core.workspace = true
ofs-physics.workspace = true
ofs-electrical.workspace = true
ofs-sensors.workspace = true
ofs-config.workspace = true
ofs-fc.workspace = true
glam.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

`crates/ofs-sim/src/lib.rs`:
```rust
//! Open FPV Sim server library: vehicle assembly (and, from Task 12, the gRPC service).
pub mod vehicle;
```

- [ ] **Step 3: Write the failing vehicle tests**

`crates/ofs-sim/tests/open_loop_vehicle.rs`:
```rust
use std::path::Path;

use ofs_config::{load, FcKind};
use ofs_sim::vehicle::{build, BuildOptions, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn vehicle(seed: u64) -> Vehicle {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop) };
    build(&cfg, &opts).unwrap()
}

fn throttle(t: f64) -> Sticks {
    Sticks { throttle: t, ..Sticks::default() }
}

#[test]
fn resting_quad_stays_put() {
    let mut v = vehicle(1);
    let start = v.state().pos_ned_m;
    v.run_for(3.0).unwrap();
    let s = v.state();
    assert!((s.pos_ned_m - start).length() < 2e-3, "moved to {}", s.pos_ned_m);
    assert!(s.vel_ned_mps.length() < 1e-3);
    assert!((s.time_s - 3.0).abs() < 1e-12);
}

#[test]
fn full_throttle_climbs_and_sags_the_battery() {
    let mut v = vehicle(1);
    v.set_sticks(&throttle(1.0));
    v.run_for(1.0).unwrap();
    let s = v.state();
    assert!(s.pos_ned_m.z < -1.0, "altitude {}", -s.pos_ned_m.z);
    assert!(s.battery_voltage_v < 24.2, "voltage {}", s.battery_voltage_v);
    assert!(s.motor_rpm.iter().all(|r| *r > 20_000.0), "rpm {:?}", s.motor_rpm);
    assert_eq!(s.motor_cmd, vec![1.0; 4]);
}

fn scripted_digest(seed: u64) -> u64 {
    let mut v = vehicle(seed);
    for (t, secs) in [(0.0, 0.5), (0.6, 1.0), (0.2, 0.5)] {
        v.set_sticks(&throttle(t));
        v.run_for(secs).unwrap();
    }
    v.digest()
}

#[test]
fn same_seed_and_inputs_give_identical_runs() {
    assert_eq!(scripted_digest(1), scripted_digest(1));
    assert_ne!(scripted_digest(1), scripted_digest(2));
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p ofs-sim`
Expected: compile error (`vehicle` items not found).

- [ ] **Step 5: Implement the vehicle**

`crates/ofs-sim/src/vehicle.rs`:
```rust
//! Assembles a quad from its config into a scheduler and exposes sticks in, state out.
use std::f64::consts::PI;
use std::path::PathBuf;

use glam::{DQuat, DVec3};
use ofs_config::{FcKind, QuadConfig};
use ofs_core::{names, Bus, Model, Scheduler, Signal, SimError};
use ofs_electrical::battery::{Battery, BatteryParams};
use ofs_electrical::esc_motor::{EscMotor, EscParams, MotorParams};
use ofs_fc::open_loop::OpenLoopFc;
use ofs_physics::propeller::{PropParams, Propeller};
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, MotorMount, RigidBody};
use ofs_sensors::baro::{Baro, BaroParams};
use ofs_sensors::imu::{Imu, ImuParams};

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub seed: u64,
    /// Per-quad firmware working directories live under here.
    pub data_dir: PathBuf,
    pub fc_override: Option<FcKind>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sticks {
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
    pub throttle: f64,
    pub aux: [f64; names::RC_AUX_COUNT],
}

impl Default for Sticks {
    fn default() -> Self {
        Self { roll: 0.0, pitch: 0.0, yaw: 0.0, throttle: 0.0, aux: [-1.0; names::RC_AUX_COUNT] }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VehicleState {
    pub time_s: f64,
    pub pos_ned_m: DVec3,
    pub vel_ned_mps: DVec3,
    pub att: DQuat,
    pub rate_frd_radps: DVec3,
    pub battery_voltage_v: f64,
    pub battery_current_a: f64,
    pub motor_rpm: Vec<f64>,
    pub motor_cmd: Vec<f64>,
}

struct Handles {
    pos: Signal<DVec3>,
    vel: Signal<DVec3>,
    att: Signal<DQuat>,
    rate: Signal<DVec3>,
    vbat: Signal<f64>,
    ibat: Signal<f64>,
    omega: Vec<Signal<f64>>,
    cmd: Vec<Signal<f64>>,
    roll: Signal<f64>,
    pitch: Signal<f64>,
    yaw: Signal<f64>,
    throttle: Signal<f64>,
    aux: Vec<Signal<f64>>,
}

impl Handles {
    fn register(bus: &mut Bus, motors: usize) -> Self {
        Self {
            pos: bus.signal(names::BODY_POS_NED),
            vel: bus.signal(names::BODY_VEL_NED),
            att: bus.signal(names::BODY_ATT),
            rate: bus.signal(names::BODY_RATE_FRD),
            vbat: bus.signal(names::BATTERY_VOLTAGE),
            ibat: bus.signal(names::BATTERY_CURRENT),
            omega: (0..motors).map(|i| bus.signal(&names::motor_omega(i))).collect(),
            cmd: (0..motors).map(|i| bus.signal(&names::motor_cmd(i))).collect(),
            roll: bus.signal(names::RC_ROLL),
            pitch: bus.signal(names::RC_PITCH),
            yaw: bus.signal(names::RC_YAW),
            throttle: bus.signal(names::RC_THROTTLE),
            aux: (0..names::RC_AUX_COUNT).map(|i| bus.signal(&names::rc_aux(i))).collect(),
        }
    }
}

pub struct Vehicle {
    scheduler: Scheduler,
    h: Handles,
}

fn v3(a: [f64; 3]) -> DVec3 {
    DVec3::from_array(a)
}

fn pairs(t: &[[f64; 2]]) -> Vec<(f64, f64)> {
    t.iter().map(|r| (r[0], r[1])).collect()
}

pub fn build(cfg: &QuadConfig, opts: &BuildOptions) -> Result<Vehicle, SimError> {
    let mut bus = Bus::new();
    let n = cfg.frame.motor_positions_frd_m.len();
    let base_hz = cfg.sim.base_hz;
    let mut models: Vec<Box<dyn Model>> = Vec::new();

    let prop = PropParams { diameter_m: cfg.prop.diameter_m, ct_table: pairs(&cfg.prop.ct_table), cp_table: pairs(&cfg.prop.cp_table) };
    for i in 0..n {
        models.push(Box::new(Propeller::new(i, prop.clone(), &mut bus)));
    }

    let motor = MotorParams {
        kv_rpm_per_v: cfg.motor.kv_rpm_per_v,
        resistance_ohm: cfg.motor.resistance_ohm,
        no_load_current_a: cfg.motor.no_load_current_a,
        rotor_inertia_kgm2: cfg.motor.rotor_inertia_kgm2,
    };
    let esc = EscParams { response_tau_s: cfg.esc.response_tau_s, current_limit_a: cfg.esc.current_limit_a };
    for i in 0..n {
        models.push(Box::new(EscMotor::new(i, esc.clone(), motor.clone(), cfg.prop.inertia_kgm2, &mut bus)));
    }

    let battery = BatteryParams {
        cells: cfg.battery.cells,
        capacity_mah: cfg.battery.capacity_mah,
        r0_ohm: cfg.battery.r0_ohm,
        r1_ohm: cfg.battery.r1_ohm,
        c1_f: cfg.battery.c1_f,
        ocv_table: pairs(&cfg.battery.ocv_table),
        initial_soc: cfg.battery.initial_soc,
    };
    models.push(Box::new(Battery::new(battery, n, base_hz / cfg.battery.rate_hz, &mut bus)));

    let f = &cfg.frame;
    let airframe = AirframeParams {
        mass_kg: f.mass_kg,
        inertia_kgm2: v3(f.inertia_kgm2),
        cda_m2: v3(f.cda_m2),
        angular_damping_nms: f.angular_damping_nms,
        rotor_inertia_kgm2: cfg.motor.rotor_inertia_kgm2 + cfg.prop.inertia_kgm2,
        mounts: f
            .motor_positions_frd_m
            .iter()
            .zip(&f.motor_spin)
            .map(|(p, s)| MotorMount { position_frd_m: v3(*p), spin: f64::from(*s) })
            .collect(),
        contact_points_frd_m: f.contact_points_frd_m.iter().map(|p| v3(*p)).collect(),
        ground: GroundParams {
            stiffness_npm: cfg.ground.stiffness_npm,
            damping_nspm: cfg.ground.damping_nspm,
            friction_coeff: cfg.ground.friction_coeff,
        },
    };
    let initial = BodyState {
        pos_ned_m: v3(cfg.initial.position_ned_m),
        vel_ned_mps: DVec3::ZERO,
        att: DQuat::from_rotation_z(cfg.initial.yaw_deg.to_radians()),
        rate_frd_radps: DVec3::ZERO,
    };
    models.push(Box::new(RigidBody::new(airframe, initial, &mut bus)));

    let imu = ImuParams {
        gyro_noise_std_radps: cfg.imu.gyro_noise_std_radps,
        gyro_bias_radps: v3(cfg.imu.gyro_bias_radps),
        accel_noise_std_mps2: cfg.imu.accel_noise_std_mps2,
        accel_bias_mps2: v3(cfg.imu.accel_bias_mps2),
    };
    models.push(Box::new(Imu::new(imu, opts.seed, &mut bus)));
    models.push(Box::new(Baro::new(BaroParams { noise_std_pa: cfg.baro.noise_std_pa, home_alt_m: cfg.home.alt_m }, opts.seed, &mut bus)));

    let fc_divisor = base_hz / cfg.fc.exchange_hz;
    match opts.fc_override.unwrap_or(cfg.fc.kind) {
        FcKind::OpenLoop => models.push(Box::new(OpenLoopFc::new(n, fc_divisor, &mut bus))),
        FcKind::Sitl => return Err(SimError::Other("the Betaflight SITL bridge is added in M1 Task 11".into())),
    }

    let h = Handles::register(&mut bus, n);
    let mut scheduler = Scheduler::new(base_hz, bus);
    for m in models {
        scheduler.add(m);
    }
    Ok(Vehicle { scheduler, h })
}

impl Vehicle {
    pub fn set_sticks(&mut self, s: &Sticks) {
        let h = &self.h;
        let bus = self.scheduler.bus_mut();
        bus.set(h.roll, s.roll);
        bus.set(h.pitch, s.pitch);
        bus.set(h.yaw, s.yaw);
        bus.set(h.throttle, s.throttle);
        for (sig, v) in h.aux.iter().zip(s.aux) {
            bus.set(*sig, v);
        }
    }

    pub fn run_for(&mut self, seconds: f64) -> Result<(), SimError> {
        self.scheduler.run_for(seconds)
    }

    pub fn state(&self) -> VehicleState {
        let b = self.scheduler.bus();
        let h = &self.h;
        VehicleState {
            time_s: self.scheduler.time_s(),
            pos_ned_m: b.get(h.pos),
            vel_ned_mps: b.get(h.vel),
            att: b.get(h.att),
            rate_frd_radps: b.get(h.rate),
            battery_voltage_v: b.get(h.vbat),
            battery_current_a: b.get(h.ibat),
            motor_rpm: h.omega.iter().map(|s| b.get(*s) * 60.0 / (2.0 * PI)).collect(),
            motor_cmd: h.cmd.iter().map(|s| b.get(*s)).collect(),
        }
    }

    pub fn digest(&self) -> u64 {
        self.scheduler.bus().digest()
    }
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: all tests pass (ofs-sim: 3 passed).

- [ ] **Step 7: Commit**

```bash
git add crates/ofs-fc crates/ofs-sim
git commit -m "feat(sim): assemble quad from config with open-loop FC stand-in"
```

---

### Task 10: SITL packet codecs and frame conversion

**M0 mapping (applied below):** gyro sent as FRD rates unchanged, accel as `(−fx, fy, fz)`, verified 8/8 by motor responses (`docs/research/sitl-interface.md` §3). RC travels inside the state datagram (§4).

**Files:**
- Create: `crates/ofs-fc/src/sitl/mod.rs`, `crates/ofs-fc/src/sitl/codec.rs`, `crates/ofs-fc/src/sitl/frames.rs`
- Modify: `crates/ofs-fc/src/lib.rs`
- Test: `crates/ofs-fc/tests/sitl_codec.rs`

**Interfaces:**
- Produces:
  - `ofs_fc::sitl::codec::{PORT_PWM_RAW, PORT_PWM, PORT_STATE, PORT_RC, MSP_TCP_PORT, FdmPacket, RcPacket, ServoPacket}` — `FdmPacket { timestamp_s, gyro_rpy_radps: [f64;3], accel_xyz_mps2: [f64;3], quat_wxyz: [f64;4], velocity_xyz_mps: [f64;3], position_xyz: [f64;3], pressure_pa }` with `SIZE = 144`, `encode() -> [u8; 144]`; `RcPacket { timestamp_s, channels: [u16;16] }` with `SIZE = 40`, `encode()`; `ServoPacket { motor_speed: [f32;4] }` with `SIZE = 16`, `decode(&[u8]) -> Option<Self>`; `STATE_DATAGRAM_SIZE = 184` and `state_datagram(&FdmPacket, &RcPacket) -> [u8; 184]` (fdm ‖ rc, the only thing the bridge sends, to `PORT_STATE`). `PORT_RC` exists for completeness but the bridge never uses it.
  - `ofs_fc::sitl::frames::{Home { lat_deg, lon_deg, alt_m }, SensorFrame { time_s, gyro_frd_radps, accel_frd_mps2, att_ned, vel_ned_mps, pos_ned_m, pressure_pa }, GYRO_SIGN, ACCEL_SIGN, EARTH_RADIUS_M, fdm_packet(&SensorFrame, &Home) -> FdmPacket, attitude_flu_nwu(DQuat) -> DQuat, stick_us(f64) -> u16, throttle_us(f64) -> u16, rc_channels(roll, pitch, yaw, throttle: f64, aux: &[f64]) -> [u16; 16], motor_commands(&ServoPacket) -> [f64; 4]}`.

- [ ] **Step 1: Write the failing tests**

`crates/ofs-fc/tests/sitl_codec.rs`:
```rust
use std::f64::consts::PI;

use glam::{DQuat, DVec3};
use ofs_fc::sitl::codec::{state_datagram, FdmPacket, RcPacket, ServoPacket, STATE_DATAGRAM_SIZE};
use ofs_fc::sitl::frames::{attitude_flu_nwu, fdm_packet, motor_commands, rc_channels, Home, SensorFrame, EARTH_RADIUS_M};

fn f64_at(bytes: &[u8], index: usize) -> f64 {
    f64::from_le_bytes(bytes[index * 8..index * 8 + 8].try_into().unwrap())
}

#[test]
fn fdm_packet_is_18_little_endian_doubles_in_struct_order() {
    let p = FdmPacket {
        timestamp_s: 1.0,
        gyro_rpy_radps: [2.0, 3.0, 4.0],
        accel_xyz_mps2: [5.0, 6.0, 7.0],
        quat_wxyz: [8.0, 9.0, 10.0, 11.0],
        velocity_xyz_mps: [12.0, 13.0, 14.0],
        position_xyz: [15.0, 16.0, 17.0],
        pressure_pa: 18.0,
    };
    let bytes = p.encode();
    assert_eq!(bytes.len(), FdmPacket::SIZE);
    for i in 0..18 {
        assert_eq!(f64_at(&bytes, i), (i + 1) as f64);
    }
}

#[test]
fn rc_packet_is_timestamp_then_16_u16_channels() {
    let mut channels = [1500u16; 16];
    channels[2] = 1000;
    let bytes = RcPacket { timestamp_s: 0.5, channels }.encode();
    assert_eq!(bytes.len(), RcPacket::SIZE);
    assert_eq!(f64_at(&bytes, 0), 0.5);
    assert_eq!(u16::from_le_bytes([bytes[12], bytes[13]]), 1000);
    assert_eq!(u16::from_le_bytes([bytes[38], bytes[39]]), 1500);
}

#[test]
fn state_datagram_is_fdm_then_rc() {
    let fdm = FdmPacket {
        timestamp_s: 1.0,
        gyro_rpy_radps: [0.0; 3],
        accel_xyz_mps2: [0.0; 3],
        quat_wxyz: [1.0, 0.0, 0.0, 0.0],
        velocity_xyz_mps: [0.0; 3],
        position_xyz: [0.0; 3],
        pressure_pa: 101_325.0,
    };
    let rc = RcPacket { timestamp_s: 1.0, channels: [1500; 16] };
    let d = state_datagram(&fdm, &rc);
    assert_eq!(d.len(), STATE_DATAGRAM_SIZE);
    assert_eq!(&d[..FdmPacket::SIZE], &fdm.encode()[..]);
    assert_eq!(&d[FdmPacket::SIZE..], &rc.encode()[..]);
}

#[test]
fn servo_packet_decodes_four_floats_and_rejects_short_input() {
    let mut bytes = Vec::new();
    for v in [0.0f32, 0.25, 0.5, 1.5] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    let p = ServoPacket::decode(&bytes).unwrap();
    assert_eq!(p.motor_speed, [0.0, 0.25, 0.5, 1.5]);
    assert_eq!(motor_commands(&p), [0.0, 0.25, 0.5, 1.0]);
    assert!(ServoPacket::decode(&bytes[..15]).is_none());
}

fn assert_quat(q: DQuat, wxyz: [f64; 4]) {
    let got = [q.w, q.x, q.y, q.z];
    for (g, e) in got.iter().zip(wxyz) {
        assert!((g - e).abs() < 1e-12, "got {got:?}, expected {wxyz:?}");
    }
}

#[test]
fn attitude_converts_frd_ned_to_flu_nwu() {
    let h = |deg: f64| (deg.to_radians() / 2.0).sin();
    let c = |deg: f64| (deg.to_radians() / 2.0).cos();
    assert_quat(attitude_flu_nwu(DQuat::IDENTITY), [1.0, 0.0, 0.0, 0.0]);
    // roll right 10 deg: same rotation about the shared X axis
    assert_quat(attitude_flu_nwu(DQuat::from_rotation_x(10f64.to_radians())), [c(10.0), h(10.0), 0.0, 0.0]);
    // nose up 10 deg: +Y in FRD, -Y in FLU
    assert_quat(attitude_flu_nwu(DQuat::from_rotation_y(10f64.to_radians())), [c(10.0), 0.0, -h(10.0), 0.0]);
    // facing east (yaw right 90 deg): +Z in NED, -Z in NWU
    assert_quat(attitude_flu_nwu(DQuat::from_rotation_z(PI / 2.0)), [c(90.0), 0.0, 0.0, -h(90.0)]);
}

#[test]
fn sensor_frame_maps_to_legacy_bridge_fields() {
    let home = Home { lat_deg: 50.0, lon_deg: 4.0, alt_m: 30.0 };
    let frame = SensorFrame {
        time_s: 2.5,
        gyro_frd_radps: DVec3::new(1.0, 2.0, 3.0),
        accel_frd_mps2: DVec3::new(0.1, 0.2, -9.8),
        att_ned: DQuat::IDENTITY,
        vel_ned_mps: DVec3::new(1.0, 2.0, -3.0),
        pos_ned_m: DVec3::new(100.0, 50.0, -10.0),
        pressure_pa: 101_000.0,
    };
    let p = fdm_packet(&frame, &home);
    assert_eq!(p.timestamp_s, 2.5);
    assert_eq!(p.gyro_rpy_radps, [1.0, 2.0, 3.0]);
    assert_eq!(p.accel_xyz_mps2, [-0.1, 0.2, -9.8]);
    assert_eq!(p.velocity_xyz_mps, [2.0, 1.0, 3.0]);
    let lat = 50.0 + (100.0 / EARTH_RADIUS_M).to_degrees();
    let lon = 4.0 + (50.0 / (EARTH_RADIUS_M * 50f64.to_radians().cos())).to_degrees();
    assert!((p.position_xyz[0] - lon).abs() < 1e-12);
    assert!((p.position_xyz[1] - lat).abs() < 1e-12);
    assert_eq!(p.position_xyz[2], 40.0);
    assert_eq!(p.pressure_pa, 101_000.0);
}

#[test]
fn sticks_map_to_aetr_microseconds() {
    let ch = rc_channels(1.0, -1.0, 0.5, 0.25, &[1.0, -1.0, 0.0, 0.0]);
    assert_eq!(&ch[..8], &[2000, 1000, 1250, 1750, 2000, 1000, 1500, 1500]);
    assert!(ch[8..].iter().all(|c| *c == 1500));
    assert_eq!(rc_channels(5.0, 0.0, 0.0, -3.0, &[])[..4], [2000, 1500, 1000, 1500]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ofs-fc --test sitl_codec`
Expected: compile error (`sitl` module not found).

- [ ] **Step 3: Implement the codec**

`crates/ofs-fc/src/sitl/codec.rs`:
```rust
//! Byte layouts of Betaflight SITL's UDP packets (src/platform/SIMULATOR/target/SITL/target.h).

pub const PORT_PWM_RAW: u16 = 9001;
pub const PORT_PWM: u16 = 9002;
pub const PORT_STATE: u16 = 9003;
pub const PORT_RC: u16 = 9004;
/// UART1 (MSP) is served on TCP 5760 + 1.
pub const MSP_TCP_PORT: u16 = 5761;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FdmPacket {
    pub timestamp_s: f64,
    pub gyro_rpy_radps: [f64; 3],
    pub accel_xyz_mps2: [f64; 3],
    pub quat_wxyz: [f64; 4],
    pub velocity_xyz_mps: [f64; 3],
    pub position_xyz: [f64; 3],
    pub pressure_pa: f64,
}

impl FdmPacket {
    pub const SIZE: usize = 144;

    pub fn encode(&self) -> [u8; Self::SIZE] {
        let fields = std::iter::once(self.timestamp_s)
            .chain(self.gyro_rpy_radps)
            .chain(self.accel_xyz_mps2)
            .chain(self.quat_wxyz)
            .chain(self.velocity_xyz_mps)
            .chain(self.position_xyz)
            .chain(std::iter::once(self.pressure_pa));
        let mut out = [0u8; Self::SIZE];
        for (i, v) in fields.enumerate() {
            out[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RcPacket {
    pub timestamp_s: f64,
    pub channels: [u16; 16],
}

impl RcPacket {
    pub const SIZE: usize = 40;

    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut out = [0u8; Self::SIZE];
        out[..8].copy_from_slice(&self.timestamp_s.to_le_bytes());
        for (i, c) in self.channels.iter().enumerate() {
            out[8 + i * 2..10 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        out
    }
}

/// Lockstep datagram: `fdm_packet` followed by `rc_packet`, sent to `PORT_STATE` (patched SITL applies the RC
/// with the same tick, so stick input is deterministic). See docs/research/sitl-interface.md §4.
pub const STATE_DATAGRAM_SIZE: usize = FdmPacket::SIZE + RcPacket::SIZE;

pub fn state_datagram(fdm: &FdmPacket, rc: &RcPacket) -> [u8; STATE_DATAGRAM_SIZE] {
    let mut out = [0u8; STATE_DATAGRAM_SIZE];
    out[..FdmPacket::SIZE].copy_from_slice(&fdm.encode());
    out[FdmPacket::SIZE..].copy_from_slice(&rc.encode());
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ServoPacket {
    pub motor_speed: [f32; 4],
}

impl ServoPacket {
    pub const SIZE: usize = 16;

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE {
            return None;
        }
        let mut motor_speed = [0f32; 4];
        for (i, m) in motor_speed.iter_mut().enumerate() {
            *m = f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().ok()?);
        }
        Some(Self { motor_speed })
    }
}
```

- [ ] **Step 4: Implement the frame conversion**

`crates/ofs-fc/src/sitl/frames.rs`:
```rust
//! Simulator state (NED/FRD) to SITL packet fields for the legacy bridge (-DENABLE_GAZEBO_BRIDGE=0).
//! Mapping verified in M0: docs/research/sitl-interface.md §3.
use std::f64::consts::PI;

use glam::{DQuat, DVec3};

use super::codec::{FdmPacket, ServoPacket};

pub const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// Applied to FRD body rates before sending (SITL then maps (x, -y, -z), so Betaflight gets FLU rates).
pub const GYRO_SIGN: [f64; 3] = [1.0, 1.0, 1.0];
/// Applied to the FRD specific force before sending (SITL negates all axes, so Betaflight gets FLU).
pub const ACCEL_SIGN: [f64; 3] = [-1.0, 1.0, 1.0];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Home {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_m: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct SensorFrame {
    pub time_s: f64,
    pub gyro_frd_radps: DVec3,
    /// Specific force, FRD.
    pub accel_frd_mps2: DVec3,
    /// FRD body -> NED world.
    pub att_ned: DQuat,
    pub vel_ned_mps: DVec3,
    pub pos_ned_m: DVec3,
    pub pressure_pa: f64,
}

/// FRD->NED attitude expressed as FLU body -> NWU world (both frames are Rx(π) of the originals). w >= 0.
pub fn attitude_flu_nwu(att_ned: DQuat) -> DQuat {
    let x180 = DQuat::from_rotation_x(PI);
    let q = (x180 * att_ned * x180).normalize();
    if q.w < 0.0 {
        -q
    } else {
        q
    }
}

pub fn fdm_packet(f: &SensorFrame, home: &Home) -> FdmPacket {
    let g = f.gyro_frd_radps.to_array();
    let q = attitude_flu_nwu(f.att_ned);
    let lat0 = home.lat_deg.to_radians();
    let lat = home.lat_deg + (f.pos_ned_m.x / EARTH_RADIUS_M).to_degrees();
    let lon = home.lon_deg + (f.pos_ned_m.y / (EARTH_RADIUS_M * lat0.cos())).to_degrees();
    FdmPacket {
        timestamp_s: f.time_s,
        gyro_rpy_radps: [g[0] * GYRO_SIGN[0], g[1] * GYRO_SIGN[1], g[2] * GYRO_SIGN[2]],
        accel_xyz_mps2: {
            let a = f.accel_frd_mps2.to_array();
            [a[0] * ACCEL_SIGN[0], a[1] * ACCEL_SIGN[1], a[2] * ACCEL_SIGN[2]]
        },
        quat_wxyz: [q.w, q.x, q.y, q.z],
        velocity_xyz_mps: [f.vel_ned_mps.y, f.vel_ned_mps.x, -f.vel_ned_mps.z],
        position_xyz: [lon, lat, home.alt_m - f.pos_ned_m.z],
        pressure_pa: f.pressure_pa,
    }
}

pub fn stick_us(x: f64) -> u16 {
    (1500.0 + 500.0 * x.clamp(-1.0, 1.0)).round() as u16
}

pub fn throttle_us(t: f64) -> u16 {
    (1000.0 + 1000.0 * t.clamp(0.0, 1.0)).round() as u16
}

/// Betaflight default channel map AETR, then AUX1.., remaining channels centred.
pub fn rc_channels(roll: f64, pitch: f64, yaw: f64, throttle: f64, aux: &[f64]) -> [u16; 16] {
    let mut ch = [1500u16; 16];
    ch[0] = stick_us(roll);
    ch[1] = stick_us(pitch);
    ch[2] = throttle_us(throttle);
    ch[3] = stick_us(yaw);
    for (slot, a) in ch[4..].iter_mut().zip(aux) {
        *slot = stick_us(*a);
    }
    ch
}

pub fn motor_commands(p: &ServoPacket) -> [f64; 4] {
    p.motor_speed.map(|m| f64::from(m).clamp(0.0, 1.0))
}
```

`crates/ofs-fc/src/sitl/mod.rs`:
```rust
//! Betaflight SITL bridge.
pub mod codec;
pub mod frames;
```

`crates/ofs-fc/src/lib.rs` (replace whole file):
```rust
//! Flight-controller models: the Betaflight SITL bridge and an open-loop stand-in for tests.
pub mod open_loop;
pub mod sitl;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p ofs-fc`
Expected: 8 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/ofs-fc
git commit -m "feat(fc): Betaflight SITL packet codecs and frame conversion"
```

---

### Task 11: SITL process supervisor and lockstep bridge

**M0 results applied:** one 184-byte state datagram to UDP 9003 per exchange, a generous first-reply timeout, `bind … failed` treated as a startup error, and WSL2 addressing and cleanup on Windows (`docs/research/sitl-interface.md` §4, §6, §7).

**Files:**
- Create: `crates/ofs-fc/src/sitl/process.rs`, `crates/ofs-fc/src/sitl/bridge.rs`, `crates/ofs-fc/src/sitl/net.rs`
- Modify: `crates/ofs-fc/src/sitl/mod.rs`, `crates/ofs-sim/src/vehicle.rs` (the `FcKind::Sitl` arm)
- Test: `crates/ofs-fc/tests/sitl_errors.rs`, `crates/ofs-fc/tests/sitl_net.rs`, `crates/ofs-fc/tests/sitl_live.rs` (ignored unless SITL is available)

**Interfaces:**
- Consumes: Task 10 codecs and frames; `names::{IMU_GYRO, IMU_ACCEL, BODY_ATT, BODY_VEL_NED, BODY_POS_NED, BARO_PRESSURE, RC_*, rc_aux, motor_cmd}`.
- Produces:
  - `ofs_fc::sitl::FcError` — `PortInUse { port: u16, hint: &'static str }`, `Launch { command: String, source: io::Error }`, `Config(String)`, `Startup(String, String)`, `Io(io::Error)`.
  - `ofs_fc::sitl::process::{LaunchConfig { launch: Vec<String>, cleanup: Vec<String>, workdir: PathBuf, diff_file: PathBuf, startup_timeout: Duration }, SitlProcess, is_bind_failure(&str) -> bool}` — `SitlProcess::start(&LaunchConfig) -> Result<Self, FcError>` (fails with `FcError::Startup` if SITL prints a `bind port … failed` line), `exit_status(&mut self) -> Option<ExitStatus>`, `log_tail(&self) -> String`; writes `<workdir>/sitl.log`; Drop kills the process and runs `cleanup`.
  - `ofs_fc::sitl::net::{SitlNet { send_ip: Ipv4Addr, bind_ip: Ipv4Addr, sitl_ip_arg: Option<Ipv4Addr> }, SitlNet::loopback(), wsl_prefix(&[String]) -> Option<Vec<String>>, default_cleanup(&[String]) -> Vec<String>, parse_hostname_ips(&str) -> Option<Ipv4Addr>, parse_default_gateway(&str) -> Option<Ipv4Addr>, resolve(launch: &[String], send_override: Option<Ipv4Addr>, reply_override: Option<Ipv4Addr>) -> Result<SitlNet, FcError>}`.
  - `ofs_fc::sitl::bridge::{BridgeConfig { launch: LaunchConfig, net: SitlNet, rate_divisor: u32, first_reply_timeout: Duration, reply_timeout: Duration, home: Home, motor_count: usize }, SitlBridge}` — `SitlBridge::start(BridgeConfig, &mut Bus) -> Result<Self, FcError>` (appends `--ip <sitl_ip_arg>` to the launch argv when set); model `"fc.sitl"`.

- [ ] **Step 1: Write the failing error-path tests (no SITL needed)**

`crates/ofs-fc/tests/sitl_errors.rs`:
```rust
use std::net::UdpSocket;
use std::sync::Mutex;
use std::time::Duration;

use ofs_core::Bus;
use ofs_fc::sitl::bridge::{BridgeConfig, SitlBridge};
use ofs_fc::sitl::frames::Home;
use ofs_fc::sitl::net::SitlNet;
use ofs_fc::sitl::process::{is_bind_failure, LaunchConfig};
use ofs_fc::sitl::FcError;

// These tests bind the fixed SITL UDP ports, so they must not run concurrently.
static PORTS: Mutex<()> = Mutex::new(());

fn config(dir: &std::path::Path, launch: &[&str]) -> BridgeConfig {
    let diff = dir.join("quad.diff");
    std::fs::write(&diff, "feature -GPS\n").unwrap();
    BridgeConfig {
        launch: LaunchConfig {
            launch: launch.iter().map(|s| s.to_string()).collect(),
            cleanup: vec![],
            workdir: dir.join("fc"),
            diff_file: diff,
            startup_timeout: Duration::from_secs(2),
        },
        net: SitlNet::loopback(),
        rate_divisor: 8,
        first_reply_timeout: Duration::from_millis(500),
        reply_timeout: Duration::from_millis(200),
        home: Home { lat_deg: 50.0, lon_deg: 4.0, alt_m: 0.0 },
        motor_count: 4,
    }
}

#[test]
fn bind_failure_lines_are_recognised() {
    // A stale SITL keeps the ports; the new one prints this and keeps running (M0 §6).
    assert!(is_bind_failure("bind port 5761 for UART1 failed!!"));
    assert!(!is_bind_failure("bind port 5761 for UART1"));
    assert!(!is_bind_failure("[SITL] init PwmOut UDP link to gazebo 127.0.0.1:9002...0"));
}

#[test]
fn launch_failure_names_the_command() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let err = SitlBridge::start(config(dir.path(), &["ofs-definitely-missing-binary"]), &mut Bus::new()).err().unwrap();
    assert!(matches!(err, FcError::Launch { .. }), "{err}");
    assert!(err.to_string().contains("ofs-definitely-missing-binary"), "{err}");
}

#[test]
fn busy_pwm_port_is_reported() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let _holder = UdpSocket::bind("127.0.0.1:9002").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let err = SitlBridge::start(config(dir.path(), &["ofs-definitely-missing-binary"]), &mut Bus::new()).err().unwrap();
    assert!(matches!(err, FcError::PortInUse { port: 9002, .. }), "{err}");
    assert!(err.to_string().contains("9002"), "{err}");
}
```

`crates/ofs-fc/tests/sitl_net.rs`:
```rust
use std::net::Ipv4Addr;

use ofs_fc::sitl::net::{default_cleanup, parse_default_gateway, parse_hostname_ips, resolve, wsl_prefix, SitlNet};

fn argv(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[test]
fn wsl_launches_are_detected_with_their_exec_prefix() {
    assert_eq!(wsl_prefix(&argv(&["wsl.exe", "-d", "Ubuntu", "-e", "/home/u/sitl.elf"])), Some(argv(&["wsl.exe", "-d", "Ubuntu", "-e"])));
    assert_eq!(wsl_prefix(&argv(&["C:\\Windows\\System32\\WSL.EXE", "--exec", "/x"])), Some(argv(&["C:\\Windows\\System32\\WSL.EXE", "--exec"])));
    assert_eq!(wsl_prefix(&argv(&["wsl.exe", "/x"])), Some(argv(&["wsl.exe", "-e"])));
    assert_eq!(wsl_prefix(&argv(&["/home/u/betaflight_SITL.elf"])), None);
    assert_eq!(wsl_prefix(&[]), None);
}

#[test]
fn wsl_cleanup_matches_the_process_name_not_the_command_line() {
    assert_eq!(default_cleanup(&argv(&["wsl.exe", "-d", "Ubuntu", "-e", "/x"])), argv(&["wsl.exe", "-d", "Ubuntu", "-e", "pkill", "-x", "betaflight_SITL"]));
    assert!(default_cleanup(&argv(&["/x/betaflight_SITL.elf"])).is_empty());
}

#[test]
fn wsl_addresses_are_parsed() {
    assert_eq!(parse_hostname_ips("172.28.26.115 10.255.255.254 \n"), Some(Ipv4Addr::new(172, 28, 26, 115)));
    assert_eq!(parse_hostname_ips(""), None);
    assert_eq!(parse_default_gateway("default via 172.28.16.1 dev eth0 proto kernel\n"), Some(Ipv4Addr::new(172, 28, 16, 1)));
    assert_eq!(parse_default_gateway("10.0.0.0/8 dev eth0"), None);
}

#[test]
fn native_launch_uses_loopback_and_overrides_win() {
    let native = argv(&["/home/u/betaflight_SITL.elf"]);
    assert_eq!(resolve(&native, None, None).unwrap(), SitlNet::loopback());
    let net = resolve(&native, Some(Ipv4Addr::new(10, 0, 0, 2)), Some(Ipv4Addr::new(10, 0, 0, 1))).unwrap();
    assert_eq!(net, SitlNet { send_ip: Ipv4Addr::new(10, 0, 0, 2), bind_ip: Ipv4Addr::new(10, 0, 0, 1), sitl_ip_arg: Some(Ipv4Addr::new(10, 0, 0, 1)) });
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ofs-fc --test sitl_errors --test sitl_net`
Expected: compile error (`bridge`, `process`, `net`, `FcError` not found).

- [ ] **Step 3: Implement the error type and process supervisor**

`crates/ofs-fc/src/sitl/mod.rs` (replace whole file):
```rust
//! Betaflight SITL bridge.
pub mod bridge;
pub mod codec;
pub mod frames;
pub mod net;
pub mod process;

#[derive(Debug, thiserror::Error)]
pub enum FcError {
    #[error("UDP port {port} is already in use: {hint}")]
    PortInUse { port: u16, hint: &'static str },
    #[error("failed to launch Betaflight SITL `{command}`: {source}")]
    Launch { command: String, source: std::io::Error },
    #[error("Betaflight SITL config step failed: {0}")]
    Config(String),
    #[error("Betaflight SITL {0}; last output:\n{1}")]
    Startup(String, String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
```

`crates/ofs-fc/src/sitl/process.rs`:
```rust
//! Launches and supervises the Betaflight SITL process; applies the CLI diff on first boot.
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::codec::MSP_TCP_PORT;
use super::FcError;

const TAIL_LINES: usize = 200;

#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// argv; on Windows typically `wsl.exe -e /home/<user>/.../betaflight_SITL.elf`.
    pub launch: Vec<String>,
    /// argv run before launch and after stop to remove stray processes; may be empty.
    pub cleanup: Vec<String>,
    /// SITL's working directory: holds eeprom.bin, betaflight.diff and sitl.log.
    pub workdir: PathBuf,
    pub diff_file: PathBuf,
    pub startup_timeout: Duration,
}

/// A stale SITL still holds the ports: a new instance prints this (e.g. `bind port 5761 for UART1 failed!!`)
/// and keeps running, and the simulator would talk to the old one. Treated as a startup error.
pub fn is_bind_failure(line: &str) -> bool {
    line.contains("bind port") && line.contains("failed")
}

#[derive(Default)]
struct LogSink {
    tail: VecDeque<String>,
    file: Option<File>,
    bind_failed: bool,
}

impl LogSink {
    fn push(&mut self, line: String) {
        if is_bind_failure(&line) {
            self.bind_failed = true;
        }
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "{line}");
        }
        if self.tail.len() == TAIL_LINES {
            self.tail.pop_front();
        }
        self.tail.push_back(line);
    }
}

type SharedLog = Arc<Mutex<LogSink>>;

pub struct SitlProcess {
    child: Child,
    log: SharedLog,
    cleanup: Vec<String>,
}

impl SitlProcess {
    pub fn start(cfg: &LaunchConfig) -> Result<Self, FcError> {
        std::fs::create_dir_all(&cfg.workdir)?;
        let workdir = std::path::absolute(&cfg.workdir)?;
        run_cleanup(&cfg.cleanup);
        if !workdir.join("eeprom.bin").exists() {
            apply_diff(cfg, &workdir)?;
        }
        let log: SharedLog = Arc::new(Mutex::new(LogSink { file: File::create(workdir.join("sitl.log")).ok(), ..LogSink::default() }));
        let child = spawn_logged(&cfg.launch, &workdir, &log)?;
        let mut proc = SitlProcess { child, log, cleanup: cfg.cleanup.clone() };
        proc.wait_until_ready(cfg.startup_timeout)?;
        Ok(proc)
    }

    fn wait_until_ready(&mut self, timeout: Duration) -> Result<(), FcError> {
        let addr = SocketAddr::from(([127, 0, 0, 1], MSP_TCP_PORT));
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.exit_status() {
                return Err(FcError::Startup(format!("exited with {status} during startup"), self.log_tail()));
            }
            if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
                // A stale instance also answers on tcp:5761; give the new one time to report bind failures.
                std::thread::sleep(Duration::from_millis(300));
                if self.log.lock().map(|l| l.bind_failed).unwrap_or(false) {
                    let what = "could not bind its ports (a stale SITL is probably still running; see fc.cleanup / OFS_SITL_CLEANUP)";
                    return Err(FcError::Startup(what.into(), self.log_tail()));
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                let what = format!("did not open tcp:{MSP_TCP_PORT} within {} ms", timeout.as_millis());
                return Err(FcError::Startup(what, self.log_tail()));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn exit_status(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    pub fn log_tail(&self) -> String {
        self.log.lock().map(|l| l.tail.iter().cloned().collect::<Vec<_>>().join("\n")).unwrap_or_default()
    }
}

impl Drop for SitlProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        run_cleanup(&self.cleanup);
    }
}

fn command(argv: &[String], workdir: &Path) -> Result<Command, FcError> {
    let (program, args) = argv.split_first().ok_or_else(|| FcError::Config("fc.launch is empty".into()))?;
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(workdir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    Ok(cmd)
}

fn spawn_logged(argv: &[String], workdir: &Path, log: &SharedLog) -> Result<Child, FcError> {
    let mut child = command(argv, workdir)?
        .spawn()
        .map_err(|source| FcError::Launch { command: argv.join(" "), source })?;
    pipe_lines(child.stdout.take().expect("stdout is piped"), log.clone());
    pipe_lines(child.stderr.take().expect("stderr is piped"), log.clone());
    Ok(child)
}

/// First boot: `<launch> --config betaflight.diff` loads the diff, saves eeprom.bin and exits.
fn apply_diff(cfg: &LaunchConfig, workdir: &Path) -> Result<(), FcError> {
    std::fs::copy(&cfg.diff_file, workdir.join("betaflight.diff"))?;
    let mut argv = cfg.launch.clone();
    argv.extend(["--config".to_string(), "betaflight.diff".to_string()]);
    let log: SharedLog = Arc::new(Mutex::new(LogSink::default()));
    let mut child = spawn_logged(&argv, workdir, &log)?;
    let deadline = Instant::now() + cfg.startup_timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(FcError::Config(format!("`{}` did not exit within {} ms", argv.join(" "), cfg.startup_timeout.as_millis())));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if !workdir.join("eeprom.bin").exists() {
        let tail = log.lock().map(|l| l.tail.iter().cloned().collect::<Vec<_>>().join("\n")).unwrap_or_default();
        return Err(FcError::Config(format!("`{}` exited with {status} without writing eeprom.bin; output:\n{tail}", argv.join(" "))));
    }
    Ok(())
}

fn pipe_lines(stream: impl Read + Send + 'static, log: SharedLog) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                    tracing::debug!(target: "sitl", "{line}");
                    if let Ok(mut l) = log.lock() {
                        l.push(line);
                    }
                }
            }
        }
    });
}

fn run_cleanup(argv: &[String]) {
    if let Some((program, args)) = argv.split_first() {
        let _ = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}
```

`crates/ofs-fc/src/sitl/net.rs`:
```rust
//! Where SITL lives on the network: natively on loopback, or inside WSL2 behind NAT (Windows).
//! See docs/research/sitl-interface.md §6.
use std::net::Ipv4Addr;
use std::process::{Command, Stdio};

use super::FcError;

/// Addresses for the lockstep exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SitlNet {
    /// Where state datagrams are sent (SITL's UDP 9003).
    pub send_ip: Ipv4Addr,
    /// Local address the motor socket binds (SITL sends motor packets to it, UDP 9002).
    pub bind_ip: Ipv4Addr,
    /// Passed to SITL as `--ip` when it must reply somewhere other than its own loopback.
    pub sitl_ip_arg: Option<Ipv4Addr>,
}

impl SitlNet {
    pub fn loopback() -> Self {
        Self { send_ip: Ipv4Addr::LOCALHOST, bind_ip: Ipv4Addr::LOCALHOST, sitl_ip_arg: None }
    }
}

/// The `wsl.exe … -e` prefix of a launch argv, if SITL is launched through WSL.
pub fn wsl_prefix(launch: &[String]) -> Option<Vec<String>> {
    let first = launch.first()?;
    let exe = first.rsplit(['/', '\\']).next().unwrap_or(first).to_ascii_lowercase();
    if exe != "wsl.exe" && exe != "wsl" {
        return None;
    }
    match launch.iter().position(|a| a == "-e" || a == "--exec") {
        Some(i) => Some(launch[..=i].to_vec()),
        None => Some(vec![first.clone(), "-e".to_string()]),
    }
}

/// Cleanup argv that removes stray SITL processes. Matches the process *name* (`pkill -x`): `pkill -f`
/// would also match (and kill) any shell whose command line contains the binary path.
pub fn default_cleanup(launch: &[String]) -> Vec<String> {
    match wsl_prefix(launch) {
        Some(mut prefix) => {
            prefix.extend(["pkill", "-x", "betaflight_SITL"].map(String::from));
            prefix
        }
        None => Vec::new(),
    }
}

/// First IPv4 address of `hostname -I` output.
pub fn parse_hostname_ips(out: &str) -> Option<Ipv4Addr> {
    out.split_whitespace().find_map(|w| w.parse().ok())
}

/// Gateway of `ip route show default` output (`default via 172.28.16.1 dev eth0 …`).
pub fn parse_default_gateway(out: &str) -> Option<Ipv4Addr> {
    let mut words = out.split_whitespace();
    while let Some(w) = words.next() {
        if w == "via" {
            return words.next()?.parse().ok();
        }
    }
    None
}

fn run(prefix: &[String], args: &[&str]) -> Result<String, FcError> {
    let (program, rest) = prefix.split_first().expect("wsl prefix is never empty");
    let out = Command::new(program)
        .args(rest)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|source| FcError::Launch { command: format!("{} {}", prefix.join(" "), args.join(" ")), source })?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Addresses for a launch argv. Native: loopback. WSL2 (NAT): send to the VM IP, bind and `--ip` the
/// Windows host IP as seen from WSL. Overrides: `send_override` (OFS_SITL_HOST), `reply_override`
/// (OFS_SITL_REPLY_IP; also passed as `--ip`).
pub fn resolve(launch: &[String], send_override: Option<Ipv4Addr>, reply_override: Option<Ipv4Addr>) -> Result<SitlNet, FcError> {
    let mut net = match wsl_prefix(launch) {
        None => SitlNet::loopback(),
        Some(prefix) => {
            let vm = parse_hostname_ips(&run(&prefix, &["hostname", "-I"])?)
                .ok_or_else(|| FcError::Config("could not read the WSL VM IP (`hostname -I`)".into()))?;
            let host = parse_default_gateway(&run(&prefix, &["ip", "route", "show", "default"])?)
                .ok_or_else(|| FcError::Config("could not read the Windows host IP from WSL (`ip route show default`)".into()))?;
            SitlNet { send_ip: vm, bind_ip: host, sitl_ip_arg: Some(host) }
        }
    };
    if let Some(ip) = send_override {
        net.send_ip = ip;
    }
    if let Some(ip) = reply_override {
        net.bind_ip = ip;
        net.sitl_ip_arg = Some(ip);
    }
    Ok(net)
}
```

- [ ] **Step 4: Implement the bridge model**

`crates/ofs-fc/src/sitl/bridge.rs`:
```rust
//! Lockstep exchange with Betaflight SITL: send one state datagram (sensors + RC), wait for one motor packet.
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};

use super::codec::{state_datagram, RcPacket, ServoPacket, PORT_PWM, PORT_STATE};
use super::frames::{fdm_packet, motor_commands, rc_channels, Home, SensorFrame};
use super::net::SitlNet;
use super::process::{LaunchConfig, SitlProcess};
use super::FcError;

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub launch: LaunchConfig,
    pub net: SitlNet,
    pub rate_divisor: u32,
    /// The first exchange can arrive before SITL's main loop runs; allow seconds.
    pub first_reply_timeout: Duration,
    pub reply_timeout: Duration,
    pub home: Home,
    pub motor_count: usize,
}

struct Inputs {
    gyro: Signal<DVec3>,
    accel: Signal<DVec3>,
    att: Signal<DQuat>,
    vel: Signal<DVec3>,
    pos: Signal<DVec3>,
    pressure: Signal<f64>,
    roll: Signal<f64>,
    pitch: Signal<f64>,
    yaw: Signal<f64>,
    throttle: Signal<f64>,
    aux: Vec<Signal<f64>>,
}

pub struct SitlBridge {
    cfg: BridgeConfig,
    rx: UdpSocket,
    tx: UdpSocket,
    inputs: Inputs,
    cmds: Vec<Signal<f64>>,
    proc: SitlProcess,
    first: bool,
    answered: bool,
}

impl SitlBridge {
    pub fn start(cfg: BridgeConfig, bus: &mut Bus) -> Result<Self, FcError> {
        assert!(cfg.motor_count <= 4, "Betaflight SITL's servo_packet carries 4 motors");
        let net = cfg.net;
        let rx = UdpSocket::bind(SocketAddr::from((net.bind_ip, PORT_PWM)))
            .map_err(|_| FcError::PortInUse { port: PORT_PWM, hint: "another simulator instance may be running" })?;
        if net.send_ip.is_loopback() {
            // Native SITL must be able to bind its state port; probe and release it to fail early.
            // (Under WSL the port lives inside the VM; the cleanup command and bind-failure check cover it.)
            UdpSocket::bind(SocketAddr::from((net.send_ip, PORT_STATE))).map_err(|_| FcError::PortInUse {
                port: PORT_STATE,
                hint: "a Betaflight SITL instance may still be running (see fc.cleanup / OFS_SITL_CLEANUP)",
            })?;
        }
        rx.set_read_timeout(Some(cfg.first_reply_timeout))?;
        let tx = UdpSocket::bind(SocketAddr::from((net.bind_ip, 0)))?;
        let inputs = Inputs {
            gyro: bus.signal(names::IMU_GYRO),
            accel: bus.signal(names::IMU_ACCEL),
            att: bus.signal(names::BODY_ATT),
            vel: bus.signal(names::BODY_VEL_NED),
            pos: bus.signal(names::BODY_POS_NED),
            pressure: bus.signal(names::BARO_PRESSURE),
            roll: bus.signal(names::RC_ROLL),
            pitch: bus.signal(names::RC_PITCH),
            yaw: bus.signal(names::RC_YAW),
            throttle: bus.signal(names::RC_THROTTLE),
            aux: (0..names::RC_AUX_COUNT).map(|i| bus.signal(&names::rc_aux(i))).collect(),
        };
        let cmds = (0..cfg.motor_count).map(|i| bus.signal(&names::motor_cmd(i))).collect();
        let mut launch = cfg.launch.clone();
        if let Some(ip) = net.sitl_ip_arg {
            launch.launch.extend(["--ip".to_string(), ip.to_string()]);
        }
        let proc = SitlProcess::start(&launch)?;
        Ok(Self { cfg, rx, tx, inputs, cmds, proc, first: true, answered: false })
    }

    fn drain(&self) -> std::io::Result<()> {
        self.rx.set_nonblocking(true)?;
        let mut buf = [0u8; 128];
        while self.rx.recv_from(&mut buf).is_ok() {}
        self.rx.set_nonblocking(false)
    }

    fn firmware_error(&mut self, what: &str) -> SimError {
        let state = match self.proc.exit_status() {
            Some(status) => format!("SITL exited with {status}"),
            None => format!("{what} (SITL still running: hung?)"),
        };
        SimError::Firmware(format!("{state}; last output:\n{}", self.proc.log_tail()))
    }
}

fn send(tx: &UdpSocket, bytes: &[u8], to: SocketAddr) {
    // Windows reports an earlier ICMP "port unreachable" as an error on a later send; a missing
    // reply is detected on receive instead, so send errors are ignored here.
    let _ = tx.send_to(bytes, to);
}

impl Model for SitlBridge {
    fn name(&self) -> &str {
        "fc.sitl"
    }

    fn rate_divisor(&self) -> u32 {
        self.cfg.rate_divisor
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        if self.first {
            self.drain().map_err(|e| SimError::Firmware(format!("UDP setup failed: {e}")))?;
            self.first = false;
        }
        let i = &self.inputs;
        let frame = SensorFrame {
            time_s: ctx.time_s,
            gyro_frd_radps: bus.get(i.gyro),
            accel_frd_mps2: bus.get(i.accel),
            att_ned: bus.get(i.att),
            vel_ned_mps: bus.get(i.vel),
            pos_ned_m: bus.get(i.pos),
            pressure_pa: bus.get(i.pressure),
        };
        let aux: Vec<f64> = i.aux.iter().map(|s| bus.get(*s)).collect();
        let channels = rc_channels(bus.get(i.roll), bus.get(i.pitch), bus.get(i.yaw), bus.get(i.throttle), &aux);
        let rc = RcPacket { timestamp_s: ctx.time_s, channels };
        let to = SocketAddr::from((self.cfg.net.send_ip, PORT_STATE));
        send(&self.tx, &state_datagram(&fdm_packet(&frame, &self.cfg.home), &rc), to);

        let mut buf = [0u8; 128];
        match self.rx.recv_from(&mut buf) {
            Ok((n, _)) => {
                let packet = ServoPacket::decode(&buf[..n])
                    .ok_or_else(|| SimError::Firmware(format!("malformed motor packet ({n} bytes)")))?;
                for (sig, v) in self.cmds.iter().zip(motor_commands(&packet)) {
                    bus.set(*sig, v);
                }
                if !self.answered {
                    self.answered = true;
                    self.rx
                        .set_read_timeout(Some(self.cfg.reply_timeout))
                        .map_err(|e| SimError::Firmware(format!("UDP setup failed: {e}")))?;
                }
                Ok(())
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset) => {
                let waited = if self.answered { self.cfg.reply_timeout } else { self.cfg.first_reply_timeout };
                let what = format!("no motor output within {} ms", waited.as_millis());
                Err(self.firmware_error(&what))
            }
            Err(e) => Err(SimError::Firmware(format!("UDP receive failed: {e}"))),
        }
    }
}
```

- [ ] **Step 5: Run the error-path tests**

Run: `cargo test -p ofs-fc --test sitl_errors --test sitl_net`
Expected: sitl_errors 3 passed, sitl_net 4 passed.

- [ ] **Step 6: Wire the bridge into the vehicle**

In `crates/ofs-sim/src/vehicle.rs`, add these imports below the existing `use` lines:
```rust
use std::time::Duration;

use std::net::Ipv4Addr;

use ofs_fc::sitl::bridge::{BridgeConfig, SitlBridge};
use ofs_fc::sitl::frames::Home;
use ofs_fc::sitl::net;
use ofs_fc::sitl::process::LaunchConfig;
```
Replace the line
```rust
        FcKind::Sitl => return Err(SimError::Other("the Betaflight SITL bridge is added in M1 Task 11".into())),
```
with:
```rust
        FcKind::Sitl => {
            let quad_stem = cfg.source_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "quad".into());
            let launch_argv = env_argv("OFS_SITL_LAUNCH").unwrap_or_else(|| cfg.fc.launch.clone());
            let mut cleanup = env_argv("OFS_SITL_CLEANUP").unwrap_or_else(|| cfg.fc.cleanup.clone());
            if cleanup.is_empty() {
                cleanup = net::default_cleanup(&launch_argv); // required under WSL (M0 §6)
            }
            let sitl_net = net::resolve(&launch_argv, env_ip("OFS_SITL_HOST")?, env_ip("OFS_SITL_REPLY_IP")?)
                .map_err(|e| SimError::Firmware(e.to_string()))?;
            let launch = LaunchConfig {
                launch: launch_argv,
                cleanup,
                workdir: opts.data_dir.join(quad_stem),
                diff_file: cfg.resolve(&cfg.fc.betaflight_diff),
                startup_timeout: Duration::from_millis(cfg.fc.startup_timeout_ms),
            };
            let bridge = SitlBridge::start(
                BridgeConfig {
                    launch,
                    net: sitl_net,
                    rate_divisor: fc_divisor,
                    first_reply_timeout: Duration::from_millis(cfg.fc.first_reply_timeout_ms),
                    reply_timeout: Duration::from_millis(cfg.fc.reply_timeout_ms),
                    home: Home { lat_deg: cfg.home.lat_deg, lon_deg: cfg.home.lon_deg, alt_m: cfg.home.alt_m },
                    motor_count: n,
                },
                &mut bus,
            )
            .map_err(|e| SimError::Firmware(e.to_string()))?;
            models.push(Box::new(bridge));
        }
```
and add at the end of the file:
```rust
/// Space-separated argv from an environment variable, if set and non-empty.
fn env_argv(var: &str) -> Option<Vec<String>> {
    std::env::var(var).ok().filter(|s| !s.trim().is_empty()).map(|s| s.split_whitespace().map(String::from).collect())
}

/// IPv4 address from an environment variable, if set and non-empty.
fn env_ip(var: &str) -> Result<Option<Ipv4Addr>, SimError> {
    match std::env::var(var) {
        Ok(v) if !v.trim().is_empty() => v
            .trim()
            .parse()
            .map(Some)
            .map_err(|_| SimError::InvalidArgument(format!("{var}={v} is not an IPv4 address"))),
        _ => Ok(None),
    }
}
```

Run: `cargo test --workspace`
Expected: all tests pass.

- [ ] **Step 7: Write the live SITL smoke test**

`crates/ofs-fc/tests/sitl_live.rs`:
```rust
//! Needs a built Betaflight SITL. Run with:
//!   OFS_SITL_LAUNCH="<path or wsl.exe -e path>" cargo test -p ofs-fc --test sitl_live -- --ignored
use std::time::Duration;

use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Scheduler};
use ofs_fc::sitl::bridge::{BridgeConfig, SitlBridge};
use ofs_fc::sitl::frames::Home;
use ofs_fc::sitl::net;
use ofs_fc::sitl::process::LaunchConfig;

const DIFF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.betaflight.diff");

#[test]
#[ignore]
fn still_quad_exchanges_packets_and_stays_disarmed() {
    let launch: Vec<String> = std::env::var("OFS_SITL_LAUNCH").expect("set OFS_SITL_LAUNCH").split_whitespace().map(String::from).collect();
    let mut cleanup: Vec<String> = std::env::var("OFS_SITL_CLEANUP").unwrap_or_default().split_whitespace().map(String::from).collect();
    if cleanup.is_empty() {
        cleanup = net::default_cleanup(&launch);
    }
    let sitl_net = net::resolve(&launch, None, None).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut bus = Bus::new();
    let accel = bus.signal::<DVec3>(names::IMU_ACCEL);
    bus.set(accel, DVec3::new(0.0, 0.0, -9.80665));
    let att = bus.signal::<DQuat>(names::BODY_ATT);
    bus.set(att, DQuat::IDENTITY);
    let pressure = bus.signal::<f64>(names::BARO_PRESSURE);
    bus.set(pressure, 101_325.0);
    let bridge = SitlBridge::start(
        BridgeConfig {
            launch: LaunchConfig { launch, cleanup, workdir: dir.path().join("fc"), diff_file: DIFF.into(), startup_timeout: Duration::from_secs(15) },
            net: sitl_net,
            rate_divisor: 8,
            first_reply_timeout: Duration::from_secs(5),
            reply_timeout: Duration::from_millis(500),
            home: Home { lat_deg: 50.85, lon_deg: 4.35, alt_m: 30.0 },
            motor_count: 4,
        },
        &mut bus,
    )
    .unwrap();
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(bridge));
    s.run_for(2.0).unwrap();
    for i in 0..4 {
        assert_eq!(s.bus().get(s.bus().lookup::<f64>(&names::motor_cmd(i)).unwrap()), 0.0);
    }
}
```

- [ ] **Step 8: Run the live test**

Build SITL if needed (`bash scripts/build-sitl.sh` in Linux/WSL), then run:
`OFS_SITL_LAUNCH=<launch command> cargo test -p ofs-fc --test sitl_live -- --ignored`
(Linux: the `.elf` path; Windows: `wsl.exe -d Ubuntu -e /home/<user>/ofs/betaflight/obj/main/betaflight_SITL.elf`; addresses and cleanup are resolved automatically.)
Expected: 1 passed. On failure the error contains SITL's last output; the full log is in the temp `fc/sitl.log`.

- [ ] **Step 9: Commit**

```bash
git add crates/ofs-fc crates/ofs-sim
git commit -m "feat(fc): supervised Betaflight SITL process and lockstep bridge"
```

---

### Task 12: gRPC API and `ofs-sim` server binary

**Files:**
- Create: `proto/ofs/v1/sim.proto`, `crates/ofs-sim/build.rs`, `crates/ofs-sim/src/server.rs`, `crates/ofs-sim/src/main.rs`
- Modify: `crates/ofs-sim/Cargo.toml`, `crates/ofs-sim/src/lib.rs`
- Test: `crates/ofs-sim/tests/grpc.rs`

**Interfaces:**
- Consumes: `ofs_sim::vehicle::*`, `ofs_config::load`, `ofs_core::SimError`.
- Produces:
  - gRPC service `ofs.v1.Sim` (proto below); Rust module `ofs_sim::pb`; `ofs_sim::server::{SimService::new(data_dir: PathBuf), PROTOCOL_VERSION: u32 = 1, error(kind, code, message) -> Status}`.
  - Every error status carries metadata `ofs-error-kind` ∈ {`config`, `firmware`, `numerical`, `invalid_argument`, `not_loaded`, `protocol`, `internal`}.
  - Binary `ofs-sim --listen <addr> --data-dir <dir>` (defaults `127.0.0.1:50051`, `.ofs-data`).
  - After a `firmware` or `numerical` error the session is paused: every later `Run` returns the same error until the next `Load`.

- [ ] **Step 1: Write the API definition**

`proto/ofs/v1/sim.proto`:
```proto
syntax = "proto3";

package ofs.v1;

// Headless simulator control. Lockstep only in protocol version 1.
service Sim {
  rpc Handshake(HandshakeRequest) returns (HandshakeReply);
  rpc Load(LoadRequest) returns (LoadReply);
  rpc SetSticks(Sticks) returns (Empty);
  rpc Run(RunRequest) returns (State);
  rpc GetState(Empty) returns (State);
  rpc Unload(Empty) returns (Empty);
}

message Empty {}

message HandshakeRequest { uint32 protocol_version = 1; }
message HandshakeReply {
  uint32 protocol_version = 1;
  string server_version = 2;
}

enum Mode {
  MODE_UNSPECIFIED = 0;  // treated as lockstep
  MODE_LOCKSTEP = 1;
}

message LoadRequest {
  string quad_path = 1;
  uint64 seed = 2;
  Mode mode = 3;
  bool open_loop_fc = 4;  // testing aid: replace the configured FC with throttle-only open loop
}
message LoadReply {
  string quad_name = 1;
  uint32 base_hz = 2;
}

// roll, pitch, yaw, aux in [-1, 1]; throttle in [0, 1]. Up to 4 aux channels; missing ones are -1.
message Sticks {
  double roll = 1;
  double pitch = 2;
  double yaw = 3;
  double throttle = 4;
  repeated double aux = 5;
}

message RunRequest { double seconds = 1; }

message Vec3 {
  double x = 1;
  double y = 2;
  double z = 3;
}
message Quat {
  double w = 1;
  double x = 2;
  double y = 3;
  double z = 4;
}

message State {
  double time_s = 1;
  Vec3 position_ned_m = 2;
  Vec3 velocity_ned_mps = 3;
  Quat attitude = 4;  // FRD body -> NED world
  Vec3 rate_frd_radps = 5;
  double battery_voltage_v = 6;
  double battery_current_a = 7;
  repeated double motor_rpm = 8;
  repeated double motor_cmd = 9;
}
```

- [ ] **Step 2: Add dependencies and the build script**

Replace `crates/ofs-sim/Cargo.toml` with:
```toml
[package]
name = "ofs-sim"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
ofs-core.workspace = true
ofs-physics.workspace = true
ofs-electrical.workspace = true
ofs-sensors.workspace = true
ofs-config.workspace = true
ofs-fc.workspace = true
glam.workspace = true
tonic.workspace = true
prost.workspace = true
tokio.workspace = true
clap.workspace = true
tracing-subscriber.workspace = true

[build-dependencies]
tonic-build.workspace = true
protoc-bin-vendored.workspace = true

[dev-dependencies]
tempfile.workspace = true
tokio-stream.workspace = true
```

`crates/ofs-sim/build.rs`:
```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);
    tonic_build::configure().compile_protos(&["../../proto/ofs/v1/sim.proto"], &["../../proto"])?;
    println!("cargo:rerun-if-changed=../../proto/ofs/v1/sim.proto");
    Ok(())
}
```

`crates/ofs-sim/src/lib.rs` (replace whole file):
```rust
//! Open FPV Sim server library: vehicle assembly and the gRPC service.
pub mod server;
pub mod vehicle;

pub mod pb {
    tonic::include_proto!("ofs.v1");
}
```

- [ ] **Step 3: Write the failing tests**

`crates/ofs-sim/tests/grpc.rs`:
```rust
use ofs_sim::pb::sim_client::SimClient;
use ofs_sim::pb::sim_server::SimServer;
use ofs_sim::pb::{Empty, HandshakeRequest, LoadRequest, RunRequest, Sticks};
use ofs_sim::server::{SimService, PROTOCOL_VERSION};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Channel;
use tonic::{Code, Status};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

async fn start() -> SimClient<Channel> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let data_dir = std::env::temp_dir().join("ofs-grpc-test-data");
    tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(SimServer::new(SimService::new(data_dir)))
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );
    SimClient::connect(format!("http://{addr}")).await.unwrap()
}

fn kind(s: &Status) -> String {
    s.metadata().get("ofs-error-kind").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default()
}

async fn load_open_loop(c: &mut SimClient<Channel>) {
    c.load(LoadRequest { quad_path: QUAD.into(), seed: 1, mode: 0, open_loop_fc: true }).await.unwrap();
}

#[tokio::test]
async fn handshake_checks_protocol_version() {
    let mut c = start().await;
    let ok = c.handshake(HandshakeRequest { protocol_version: PROTOCOL_VERSION }).await.unwrap().into_inner();
    assert_eq!(ok.protocol_version, PROTOCOL_VERSION);
    let err = c.handshake(HandshakeRequest { protocol_version: 999 }).await.unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition);
    assert_eq!(kind(&err), "protocol");
}

#[tokio::test]
async fn run_before_load_is_rejected() {
    let mut c = start().await;
    let err = c.run(RunRequest { seconds: 0.1 }).await.unwrap_err();
    assert_eq!(kind(&err), "not_loaded");
}

#[tokio::test]
async fn bad_quad_path_is_a_config_error() {
    let mut c = start().await;
    let err = c.load(LoadRequest { quad_path: "does/not/exist.toml".into(), seed: 0, mode: 0, open_loop_fc: true }).await.unwrap_err();
    assert_eq!(err.code(), Code::InvalidArgument);
    assert_eq!(kind(&err), "config");
    assert!(err.message().contains("does/not/exist.toml"), "{}", err.message());
}

#[tokio::test]
async fn open_loop_session_runs_and_reports_state() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let s = c.run(RunRequest { seconds: 1.0 }).await.unwrap().into_inner();
    assert!((s.time_s - 1.0).abs() < 1e-9);
    assert!((s.position_ned_m.unwrap().z + 0.0295).abs() < 1e-3);
    assert_eq!(s.motor_rpm.len(), 4);
    let again = c.get_state(Empty {}).await.unwrap().into_inner();
    assert_eq!(again.time_s, s.time_s);
}

#[tokio::test]
async fn invalid_run_durations_do_not_poison_the_session() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    for seconds in [-1.0, f64::NAN] {
        let err = c.run(RunRequest { seconds }).await.unwrap_err();
        assert_eq!(kind(&err), "invalid_argument");
    }
    c.run(RunRequest { seconds: 0.1 }).await.unwrap();
}

#[tokio::test]
async fn non_finite_sticks_are_rejected() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let err = c.set_sticks(Sticks { roll: f64::NAN, ..Default::default() }).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_argument");
    let err = c.set_sticks(Sticks { aux: vec![0.0; 5], ..Default::default() }).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_argument");
    c.set_sticks(Sticks { throttle: 0.5, aux: vec![1.0], ..Default::default() }).await.unwrap();
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p ofs-sim --test grpc`
Expected: compile error (`server` module not found).

- [ ] **Step 5: Implement the service**

`crates/ofs-sim/src/server.rs`:
```rust
//! gRPC service: one vehicle session at a time; all stepping happens on blocking threads.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ofs_config::FcKind;
use ofs_core::{names::RC_AUX_COUNT, SimError};
use tonic::metadata::MetadataMap;
use tonic::{Code, Request, Response, Status};

use crate::pb::{self, sim_server::Sim};
use crate::vehicle::{self, BuildOptions, Sticks, Vehicle, VehicleState};

pub const PROTOCOL_VERSION: u32 = 1;

struct Session {
    vehicle: Vehicle,
    /// Set after a firmware or numerical failure; the session stays paused until the next Load.
    failure: Option<SimError>,
}

type Slot = Option<Session>;

#[derive(Clone)]
pub struct SimService {
    session: Arc<Mutex<Slot>>,
    data_dir: PathBuf,
}

/// A status carrying the machine-readable `ofs-error-kind` metadata that clients map to typed errors.
pub fn error(kind: &'static str, code: Code, message: impl Into<String>) -> Status {
    let mut md = MetadataMap::new();
    md.insert("ofs-error-kind", kind.parse().expect("kind is ASCII"));
    Status::with_metadata(code, message.into(), md)
}

fn sim_error(e: &SimError) -> Status {
    match e {
        SimError::Firmware(m) => error("firmware", Code::Aborted, m.clone()),
        SimError::NonFinite(_) => error("numerical", Code::Aborted, e.to_string()),
        SimError::InvalidArgument(m) => error("invalid_argument", Code::InvalidArgument, m.clone()),
        SimError::Other(m) => error("internal", Code::Internal, m.clone()),
    }
}

fn loaded(slot: &mut Slot) -> Result<&mut Session, Status> {
    slot.as_mut().ok_or_else(|| error("not_loaded", Code::FailedPrecondition, "no quad loaded; call Load first"))
}

fn vec3(v: glam::DVec3) -> Option<pb::Vec3> {
    Some(pb::Vec3 { x: v.x, y: v.y, z: v.z })
}

fn state_msg(s: &VehicleState) -> pb::State {
    pb::State {
        time_s: s.time_s,
        position_ned_m: vec3(s.pos_ned_m),
        velocity_ned_mps: vec3(s.vel_ned_mps),
        attitude: Some(pb::Quat { w: s.att.w, x: s.att.x, y: s.att.y, z: s.att.z }),
        rate_frd_radps: vec3(s.rate_frd_radps),
        battery_voltage_v: s.battery_voltage_v,
        battery_current_a: s.battery_current_a,
        motor_rpm: s.motor_rpm.clone(),
        motor_cmd: s.motor_cmd.clone(),
    }
}

impl SimService {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { session: Arc::new(Mutex::new(None)), data_dir }
    }

    async fn blocking<T, F>(&self, f: F) -> Result<Response<T>, Status>
    where
        T: Send + 'static,
        F: FnOnce(&mut Slot) -> Result<T, Status> + Send + 'static,
    {
        let session = self.session.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = session.lock().map_err(|_| error("internal", Code::Internal, "session lock poisoned"))?;
            f(&mut guard)
        })
        .await
        .map_err(|e| error("internal", Code::Internal, e.to_string()))?
        .map(Response::new)
    }
}

#[tonic::async_trait]
impl Sim for SimService {
    async fn handshake(&self, req: Request<pb::HandshakeRequest>) -> Result<Response<pb::HandshakeReply>, Status> {
        let v = req.into_inner().protocol_version;
        if v != PROTOCOL_VERSION {
            return Err(error("protocol", Code::FailedPrecondition, format!("client speaks protocol {v}, server speaks {PROTOCOL_VERSION}")));
        }
        Ok(Response::new(pb::HandshakeReply { protocol_version: PROTOCOL_VERSION, server_version: env!("CARGO_PKG_VERSION").into() }))
    }

    async fn load(&self, req: Request<pb::LoadRequest>) -> Result<Response<pb::LoadReply>, Status> {
        let req = req.into_inner();
        match pb::Mode::try_from(req.mode) {
            Ok(pb::Mode::Unspecified) | Ok(pb::Mode::Lockstep) => {}
            _ => return Err(error("invalid_argument", Code::InvalidArgument, "only lockstep mode is available in protocol version 1")),
        }
        let cfg = ofs_config::load(Path::new(&req.quad_path)).map_err(|e| error("config", Code::InvalidArgument, e.to_string()))?;
        let opts = BuildOptions { seed: req.seed, data_dir: self.data_dir.clone(), fc_override: req.open_loop_fc.then_some(FcKind::OpenLoop) };
        self.blocking(move |slot| {
            *slot = None; // stop the previous vehicle (and its SITL) before the new one binds the ports
            let vehicle = vehicle::build(&cfg, &opts).map_err(|e| sim_error(&e))?;
            *slot = Some(Session { vehicle, failure: None });
            Ok(pb::LoadReply { quad_name: cfg.name.clone(), base_hz: cfg.sim.base_hz })
        })
        .await
    }

    async fn set_sticks(&self, req: Request<pb::Sticks>) -> Result<Response<pb::Empty>, Status> {
        let s = req.into_inner();
        if s.aux.len() > RC_AUX_COUNT {
            return Err(error("invalid_argument", Code::InvalidArgument, format!("at most {RC_AUX_COUNT} aux channels (got {})", s.aux.len())));
        }
        let mut aux = [-1.0; RC_AUX_COUNT];
        aux[..s.aux.len()].copy_from_slice(&s.aux);
        let sticks = Sticks { roll: s.roll, pitch: s.pitch, yaw: s.yaw, throttle: s.throttle, aux };
        let all_finite = [sticks.roll, sticks.pitch, sticks.yaw, sticks.throttle].iter().chain(aux.iter()).all(|v| v.is_finite());
        if !all_finite {
            return Err(error("invalid_argument", Code::InvalidArgument, "stick values must be finite"));
        }
        self.blocking(move |slot| {
            loaded(slot)?.vehicle.set_sticks(&sticks);
            Ok(pb::Empty {})
        })
        .await
    }

    async fn run(&self, req: Request<pb::RunRequest>) -> Result<Response<pb::State>, Status> {
        let seconds = req.into_inner().seconds;
        self.blocking(move |slot| {
            let s = loaded(slot)?;
            if let Some(f) = &s.failure {
                return Err(sim_error(f));
            }
            if let Err(e) = s.vehicle.run_for(seconds) {
                let status = sim_error(&e);
                if matches!(e, SimError::Firmware(_) | SimError::NonFinite(_)) {
                    s.failure = Some(e);
                }
                return Err(status);
            }
            Ok(state_msg(&s.vehicle.state()))
        })
        .await
    }

    async fn get_state(&self, _req: Request<pb::Empty>) -> Result<Response<pb::State>, Status> {
        self.blocking(|slot| Ok(state_msg(&loaded(slot)?.vehicle.state()))).await
    }

    async fn unload(&self, _req: Request<pb::Empty>) -> Result<Response<pb::Empty>, Status> {
        self.blocking(|slot| {
            *slot = None;
            Ok(pb::Empty {})
        })
        .await
    }
}
```

`crates/ofs-sim/src/main.rs`:
```rust
use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;
use ofs_sim::pb::sim_server::SimServer;
use ofs_sim::server::SimService;

#[derive(Parser)]
#[command(about = "Open FPV Sim headless server")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:50051")]
    listen: SocketAddr,
    /// Per-quad firmware working directories (EEPROM, SITL log) are created here.
    #[arg(long, default_value = ".ofs-data")]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_writer(std::io::stderr).init();
    let args = Args::parse();
    eprintln!("ofs-sim {} listening on {}", env!("CARGO_PKG_VERSION"), args.listen);
    tonic::transport::Server::builder()
        .add_service(SimServer::new(SimService::new(args.data_dir)))
        .serve(args.listen)
        .await?;
    Ok(())
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p ofs-sim`
Expected: grpc 6 passed, open_loop_vehicle 3 passed.

- [ ] **Step 7: Commit**

```bash
git add proto crates/ofs-sim
git commit -m "feat(sim): gRPC API and ofs-sim server binary"
```

---

### Task 13: Python client, tests and hover example

**Files:**
- Create: `python/pyproject.toml`, `python/ofs/__init__.py`, `python/ofs/client.py`, `python/ofs/errors.py`, `python/ofs/v1/__init__.py`
- Generate: `python/ofs/v1/sim_pb2.py`, `python/ofs/v1/sim_pb2.pyi`, `python/ofs/v1/sim_pb2_grpc.py`
- Create: `python/examples/hover.py`
- Test: `python/tests/conftest.py`, `python/tests/test_client.py`, `python/tests/test_sitl_hover.py`

**Interfaces:**
- Consumes: gRPC service from Task 12; `ofs-sim` binary.
- Produces (Python):
  - `ofs.launch(binary=None, headless=True, data_dir=".ofs-data", timeout_s=15.0) -> Sim`, `ofs.connect(address="127.0.0.1:50051") -> Sim`.
  - `Sim.address`, `Sim.load(quad_path, seed=0, mode="lockstep", open_loop_fc=False) -> str`, `Sim.set_sticks(roll=0.0, pitch=0.0, yaw=0.0, throttle=0.0, aux=(-1.0, -1.0, -1.0, -1.0))`, `Sim.run(seconds) -> State`, `Sim.state() -> State`, `Sim.close()`; context manager.
  - `ofs.State` (frozen dataclass: `time_s, position_ned_m, velocity_ned_mps, attitude_wxyz, rate_frd_radps, battery_voltage_v, battery_current_a, motor_rpm, motor_cmd`; `altitude_m` property; `euler_deg() -> (roll, pitch, yaw)`).
  - Exceptions: `OfsError`, `ConfigError`, `FirmwareCrashed`, `NumericalError`, `ProtocolMismatch`, `NotLoaded`, `InvalidArgument` (also a `ValueError`), `ServerUnavailable`.

- [ ] **Step 1: Package metadata**

`python/pyproject.toml`:
```toml
[build-system]
requires = ["setuptools>=68"]
build-backend = "setuptools.build_meta"

[project]
name = "ofs"
version = "0.1.0"
description = "Python client for Open FPV Sim"
license = { text = "GPL-3.0-or-later" }
requires-python = ">=3.10"
dependencies = ["grpcio>=1.66", "protobuf>=5.27"]

[project.optional-dependencies]
dev = ["grpcio-tools>=1.66", "pytest>=8"]

[tool.setuptools.packages.find]
include = ["ofs*"]
```

Create an empty `python/ofs/v1/__init__.py`, then install and generate the stubs (run from the repo root):
```bash
python -m pip install -e "python[dev]"
python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto
```
Expected: `python/ofs/v1/sim_pb2.py`, `sim_pb2.pyi`, `sim_pb2_grpc.py` exist; `sim_pb2_grpc.py` contains `from ofs.v1 import sim_pb2`.

- [ ] **Step 2: Write the failing tests**

`python/tests/conftest.py`:
```python
import os
import pathlib
import sys

import pytest

REPO = pathlib.Path(__file__).resolve().parents[2]
QUAD = str(REPO / "quads" / "opendrone-5f-freestyle.toml")


@pytest.fixture(scope="session")
def sim_bin():
    env = os.environ.get("OFS_SIM_BIN")
    if env:
        return env
    exe = REPO / "target" / "debug" / ("ofs-sim.exe" if sys.platform == "win32" else "ofs-sim")
    if not exe.exists():
        pytest.fail("ofs-sim not built: run `cargo build -p ofs-sim` or set OFS_SIM_BIN")
    return str(exe)


@pytest.fixture
def sim(sim_bin, tmp_path):
    import ofs

    s = ofs.launch(binary=sim_bin, data_dir=str(tmp_path))
    yield s
    s.close()
```

`python/tests/test_client.py`:
```python
import pytest

import ofs
from conftest import QUAD


def test_open_loop_rest_then_climb(sim):
    assert sim.load(QUAD, seed=1, open_loop_fc=True).startswith("OpenDrone")
    s = sim.run(1.0)
    assert abs(s.time_s - 1.0) < 1e-9
    assert abs(s.altitude_m - 0.0295) < 1e-3
    sim.set_sticks(throttle=1.0)
    s = sim.run(1.0)
    assert s.altitude_m > 1.0
    assert s.battery_voltage_v < 24.5
    assert len(s.motor_rpm) == 4


def test_errors_are_typed(sim):
    with pytest.raises(ofs.NotLoaded):
        sim.run(0.1)
    with pytest.raises(ofs.ConfigError):
        sim.load("does/not/exist.toml")
    sim.load(QUAD, open_loop_fc=True)
    with pytest.raises(ofs.InvalidArgument):
        sim.run(-1.0)
    with pytest.raises(ofs.InvalidArgument):
        sim.run(float("nan"))
    sim.run(0.1)


def test_protocol_mismatch_is_typed(sim, monkeypatch):
    import ofs.client

    monkeypatch.setattr(ofs.client, "PROTOCOL_VERSION", 999)
    with pytest.raises(ofs.ProtocolMismatch):
        ofs.connect(sim.address)


def test_euler_of_identity_is_zero():
    s = ofs.State(time_s=0.0, position_ned_m=(0.0, 0.0, 0.0), velocity_ned_mps=(0.0, 0.0, 0.0),
                  attitude_wxyz=(1.0, 0.0, 0.0, 0.0), rate_frd_radps=(0.0, 0.0, 0.0),
                  battery_voltage_v=0.0, battery_current_a=0.0, motor_rpm=(), motor_cmd=())
    assert s.euler_deg() == (0.0, 0.0, 0.0)
```

`python/tests/test_sitl_hover.py`:
```python
"""M1 exit criterion. Needs Betaflight SITL: set OFS_SITL_LAUNCH (cleanup defaults automatically under WSL)."""
import importlib.util
import os

import pytest

from conftest import QUAD, REPO

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")


def _hover_module():
    spec = importlib.util.spec_from_file_location("hover", REPO / "python" / "examples" / "hover.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_betaflight_sitl_hover(sim, tmp_path):
    sim.load(QUAD, seed=1)
    result = _hover_module().fly_hover(sim)
    assert result["armed"], f"motors never spun: read {tmp_path}/opendrone-5f-freestyle/sitl.log for 'Arming disabled'"
    assert result["max_alt_err_m"] < 0.3, result
    assert result["max_tilt_deg"] < 5.0, result
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo build -p ofs-sim` then `python -m pytest python/tests -v`
Expected: collection error (`ofs` has no attribute `launch` / module empty).

- [ ] **Step 4: Implement the package**

`python/ofs/errors.py`:
```python
"""Typed errors mapped from the server's `ofs-error-kind` metadata."""
import grpc


class OfsError(Exception):
    """Base class for Open FPV Sim errors."""


class ConfigError(OfsError):
    """The quad file is missing, unparsable, or invalid."""


class FirmwareCrashed(OfsError):
    """Betaflight SITL exited, hung, or sent garbage. The session is paused until the next load()."""


class NumericalError(OfsError):
    """A simulated signal became NaN or infinite. The session is paused until the next load()."""


class ProtocolMismatch(OfsError):
    """Client and server speak different protocol versions."""


class NotLoaded(OfsError):
    """No quad is loaded; call load() first."""


class InvalidArgument(OfsError, ValueError):
    """A request argument was out of range."""


class ServerUnavailable(OfsError):
    """The server could not be reached."""


_KINDS = {
    "config": ConfigError,
    "firmware": FirmwareCrashed,
    "numerical": NumericalError,
    "protocol": ProtocolMismatch,
    "not_loaded": NotLoaded,
    "invalid_argument": InvalidArgument,
}


def from_rpc_error(e: grpc.RpcError) -> OfsError:
    kind = dict(e.trailing_metadata() or ()).get("ofs-error-kind")
    if kind in _KINDS:
        return _KINDS[kind](e.details())
    if e.code() == grpc.StatusCode.UNAVAILABLE:
        return ServerUnavailable(e.details())
    return OfsError(f"{e.code().name}: {e.details()}")
```

`python/ofs/client.py`:
```python
"""Client for the ofs-sim gRPC server."""
from __future__ import annotations

import math
import os
import shutil
import socket
import subprocess
import time
from dataclasses import dataclass

import grpc

from ofs.v1 import sim_pb2 as pb
from ofs.v1 import sim_pb2_grpc as pbg

from .errors import ProtocolMismatch, from_rpc_error

PROTOCOL_VERSION = 1


@dataclass(frozen=True)
class State:
    time_s: float
    position_ned_m: tuple
    velocity_ned_mps: tuple
    attitude_wxyz: tuple
    rate_frd_radps: tuple
    battery_voltage_v: float
    battery_current_a: float
    motor_rpm: tuple
    motor_cmd: tuple

    @property
    def altitude_m(self) -> float:
        return -self.position_ned_m[2]

    def euler_deg(self) -> tuple:
        """(roll, pitch, yaw) in degrees, aerospace Z-Y-X convention, FRD body / NED world."""
        w, x, y, z = self.attitude_wxyz
        roll = math.degrees(math.atan2(2 * (w * x + y * z), 1 - 2 * (x * x + y * y)))
        pitch = math.degrees(math.asin(max(-1.0, min(1.0, 2 * (w * y - z * x)))))
        yaw = math.degrees(math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z)))
        return roll, pitch, yaw


def _v(m) -> tuple:
    return (m.x, m.y, m.z)


def _state(m) -> State:
    return State(
        time_s=m.time_s,
        position_ned_m=_v(m.position_ned_m),
        velocity_ned_mps=_v(m.velocity_ned_mps),
        attitude_wxyz=(m.attitude.w, m.attitude.x, m.attitude.y, m.attitude.z),
        rate_frd_radps=_v(m.rate_frd_radps),
        battery_voltage_v=m.battery_voltage_v,
        battery_current_a=m.battery_current_a,
        motor_rpm=tuple(m.motor_rpm),
        motor_cmd=tuple(m.motor_cmd),
    )


class Sim:
    """A connection to one ofs-sim server (and the process, if launched by `launch`)."""

    def __init__(self, address: str, process: subprocess.Popen | None = None):
        self.address = address
        self._process = process
        self._channel = grpc.insecure_channel(address)
        self._stub = pbg.SimStub(self._channel)
        try:
            reply = self._call(self._stub.Handshake, pb.HandshakeRequest(protocol_version=PROTOCOL_VERSION))
            if reply.protocol_version != PROTOCOL_VERSION:
                raise ProtocolMismatch(f"server speaks protocol {reply.protocol_version}, client {PROTOCOL_VERSION}")
        except Exception:
            self._channel.close()
            raise

    @staticmethod
    def _call(method, request):
        try:
            return method(request)
        except grpc.RpcError as e:
            raise from_rpc_error(e) from None

    def load(self, quad_path: str, seed: int = 0, mode: str = "lockstep", open_loop_fc: bool = False) -> str:
        if mode != "lockstep":
            raise ValueError("only mode='lockstep' is available in this version")
        reply = self._call(self._stub.Load, pb.LoadRequest(quad_path=str(quad_path), seed=seed,
                                                           mode=pb.MODE_LOCKSTEP, open_loop_fc=open_loop_fc))
        return reply.quad_name

    def set_sticks(self, roll=0.0, pitch=0.0, yaw=0.0, throttle=0.0, aux=(-1.0, -1.0, -1.0, -1.0)) -> None:
        self._call(self._stub.SetSticks, pb.Sticks(roll=roll, pitch=pitch, yaw=yaw, throttle=throttle, aux=list(aux)))

    def run(self, seconds: float) -> State:
        return _state(self._call(self._stub.Run, pb.RunRequest(seconds=seconds)))

    def state(self) -> State:
        return _state(self._call(self._stub.GetState, pb.Empty()))

    def close(self) -> None:
        self._channel.close()
        if self._process is not None:
            self._process.terminate()
            try:
                self._process.wait(5)
            except subprocess.TimeoutExpired:
                self._process.kill()
            self._process = None

    def __enter__(self) -> "Sim":
        return self

    def __exit__(self, *exc) -> None:
        self.close()


def connect(address: str = "127.0.0.1:50051") -> Sim:
    return Sim(address)


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def launch(binary: str | None = None, headless: bool = True, data_dir: str = ".ofs-data",
           timeout_s: float = 15.0) -> Sim:
    """Starts an ofs-sim server on a free local port and connects to it."""
    if not headless:
        raise NotImplementedError("the Godot pilot client arrives in M2; only headless=True is available")
    binary = binary or os.environ.get("OFS_SIM_BIN") or shutil.which("ofs-sim")
    if not binary:
        raise FileNotFoundError("ofs-sim binary not found: set OFS_SIM_BIN or put ofs-sim on PATH")
    address = f"127.0.0.1:{_free_port()}"
    proc = subprocess.Popen([binary, "--listen", address, "--data-dir", data_dir])
    deadline = time.monotonic() + timeout_s
    channel = grpc.insecure_channel(address)
    try:
        while True:
            if proc.poll() is not None:
                raise RuntimeError(f"ofs-sim exited with code {proc.returncode} during startup")
            try:
                grpc.channel_ready_future(channel).result(timeout=0.2)
                break
            except grpc.FutureTimeoutError:
                if time.monotonic() > deadline:
                    proc.kill()
                    raise RuntimeError(f"ofs-sim did not accept connections on {address} within {timeout_s} s")
    finally:
        channel.close()
    return Sim(address, proc)
```

`python/ofs/__init__.py`:
```python
"""Python client for Open FPV Sim."""
from .client import Sim, State, connect, launch
from .errors import (ConfigError, FirmwareCrashed, InvalidArgument, NotLoaded, NumericalError, OfsError,
                     ProtocolMismatch, ServerUnavailable)

__all__ = [
    "Sim", "State", "connect", "launch",
    "OfsError", "ConfigError", "FirmwareCrashed", "NumericalError", "ProtocolMismatch", "NotLoaded",
    "InvalidArgument", "ServerUnavailable",
]
```

- [ ] **Step 5: Write the hover example**

`python/examples/hover.py`:
```python
"""Arms the quad in Betaflight SITL (angle mode) and holds 1 m with a throttle PID.

Usage (repo root): OFS_SITL_LAUNCH="<sitl launch command>" python python/examples/hover.py
"""
import sys

import ofs

ARM_AND_ANGLE = (1.0, 1.0, -1.0, -1.0)  # AUX1 high = ARM, AUX2 high = ANGLE (see the quad's betaflight.diff)


def fly_hover(sim, target_alt=1.0, seconds=8.0):
    sim.set_sticks(throttle=0.0, aux=(-1.0, -1.0, -1.0, -1.0))
    sim.run(4.0)  # boot and gyro calibration, perfectly still
    sim.set_sticks(throttle=0.0, aux=ARM_AND_ANGLE)
    sim.run(1.0)

    hover_guess, kp, ki, kd, dt = 0.35, 0.15, 0.10, 0.10, 0.02
    integral, log = 0.0, []
    s = sim.state()
    t_end = s.time_s + seconds
    while s.time_s < t_end:
        err = target_alt - s.altitude_m
        integral = max(-2.0, min(2.0, integral + err * dt))
        climb = -s.velocity_ned_mps[2]
        throttle = max(0.0, min(1.0, hover_guess + kp * err + ki * integral - kd * climb))
        sim.set_sticks(throttle=throttle, aux=ARM_AND_ANGLE)
        s = sim.run(dt)
        roll, pitch, _ = s.euler_deg()
        log.append((s.time_s, s.altitude_m, roll, pitch, max(s.motor_cmd)))

    tail = [r for r in log if r[0] >= t_end - 5.0]
    return {
        "armed": max(r[4] for r in log) > 0.05,
        "max_alt_err_m": max(abs(r[1] - target_alt) for r in tail),
        "max_tilt_deg": max(max(abs(r[2]), abs(r[3])) for r in tail),
    }


if __name__ == "__main__":
    quad = sys.argv[1] if len(sys.argv) > 1 else "quads/opendrone-5f-freestyle.toml"
    with ofs.launch() as sim:
        sim.load(quad, seed=1)
        print(fly_hover(sim))
```

- [ ] **Step 6: Run the open-loop tests**

Run: `cargo build -p ofs-sim` then `python -m pytest python/tests -v`
Expected: 4 passed, 1 skipped (`test_betaflight_sitl_hover`, unless `OFS_SITL_LAUNCH` is set).

- [ ] **Step 7: Run the M1 exit test against SITL**

Run: `OFS_SITL_LAUNCH=<launch command> python -m pytest python/tests/test_sitl_hover.py -v`
Expected: 1 passed. If `armed` is false, read the SITL log path in the failure message for `Arming disabled:` flags and fix the quad's `.betaflight.diff` (then delete `<data_dir>/opendrone-5f-freestyle/eeprom.bin` so the diff is re-applied). If the quad flips or oscillates, the sign mapping is wrong: re-check M0 §3 against `GYRO_SIGN` in Task 10.

- [ ] **Step 8: Commit**

```bash
git add python
git commit -m "feat(python): ofs client package, typed errors and SITL hover example"
```

---

### Task 14: Developer docs and CI

**Files:**
- Create: `README.md`, `docs/dev-setup.md`, `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: commands from Tasks 1–13 and M0's build script.

- [ ] **Step 1: Write `README.md`**

```markdown
# Open FPV Sim

An open-source FPV drone simulator that runs **real Betaflight** (SITL) against physics, electrical and sensor models,
replicating real protocols so real tools work against it. Inspired by the [OpenDrone](https://opendrone.be/) open-hardware initiative.

Status: **M1 — headless core.** Lockstep physics at 8 kHz, Betaflight SITL bridge, Python scripting over gRPC.

- Design: `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`
- SITL interface findings: `docs/research/sitl-interface.md`
- Developer setup: `docs/dev-setup.md`

## Quick start (open loop, no firmware)

    cargo build -p ofs-sim
    python -m pip install -e "python[dev]"
    python -c "import ofs; s = ofs.launch(binary='target/debug/ofs-sim'); s.load('quads/opendrone-5f-freestyle.toml', open_loop_fc=True); s.set_sticks(throttle=0.6); print(s.run(1.0)); s.close()"

## Hover with real Betaflight

    bash scripts/build-sitl.sh                      # Linux or WSL2
    export OFS_SIM_BIN=target/debug/ofs-sim
    export OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf
    python python/examples/hover.py

License: GPL-3.0-or-later.
```

- [ ] **Step 2: Write `docs/dev-setup.md`**

```markdown
# Developer setup

## Requirements
- Rust stable ≥ 1.80 (`rustup`), Python ≥ 3.10.
- Betaflight SITL: Linux, or WSL2 (Ubuntu) on Windows. Build with `bash scripts/build-sitl.sh` (see the script for env options).

## Everyday commands
| What | Command |
|---|---|
| All Rust tests | `cargo test --workspace` |
| Live SITL bridge test | `OFS_SITL_LAUNCH=<cmd> cargo test -p ofs-fc --test sitl_live -- --ignored` |
| Server | `cargo run -p ofs-sim -- --listen 127.0.0.1:50051 --data-dir .ofs-data` |
| Python tests | `cargo build -p ofs-sim && python -m pytest python/tests -v` |
| Regenerate Python stubs | `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto` |

## Environment variables
- `OFS_SIM_BIN` — path to `ofs-sim` used by `ofs.launch()`.
- `OFS_SITL_LAUNCH` — SITL argv (space-separated), overrides `fc.launch` in quad files.
  Windows: `wsl.exe -d Ubuntu -e /home/<user>/ofs/betaflight/obj/main/betaflight_SITL.elf`. Works with WSL's default NAT networking: the bridge discovers the WSL VM and host IPs and passes `--ip` (see `docs/research/sitl-interface.md` §6).
- `OFS_SITL_CLEANUP` — argv run before launch and after stop to kill stray SITL processes. Under WSL it defaults to `<wsl prefix> pkill -x betaflight_SITL` (required: a stale SITL would otherwise answer instead of the new one).
- `OFS_SITL_HOST`, `OFS_SITL_REPLY_IP` — override the address state datagrams go to, and the address SITL replies to (`--ip`, motor socket bind).

## Firmware state
Each quad gets `<data_dir>/<quad file stem>/` holding `eeprom.bin`, `betaflight.diff` and `sitl.log`.
The diff is applied only on first boot; delete `eeprom.bin` to re-apply it. Betaflight Configurator can connect to `tcp://127.0.0.1:5761` while a session runs.

## Ports
SITL uses fixed ports (UDP 9001–9004, TCP 5760+), so only one SITL session can run per machine and SITL tests must not run in parallel.
```

- [ ] **Step 3: Write the CI workflow**

`.github/workflows/ci.yml`:
```yaml
name: ci
on: [push, pull_request]

jobs:
  core:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/setup-python@v5
        with:
          python-version: "3.12"
      - run: cargo test --workspace
      - run: cargo build -p ofs-sim
      - run: python -m pip install -e "python[dev]"
      - run: python -m pytest python/tests -v

  sitl:
    runs-on: ubuntu-24.04
    env:
      BF_DIR: ${{ github.workspace }}/../betaflight
      OFS_SITL_LAUNCH: ${{ github.workspace }}/../betaflight/obj/main/betaflight_SITL.elf
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/setup-python@v5
        with:
          python-version: "3.12"
      - run: bash scripts/build-sitl.sh
      - run: cargo test -p ofs-fc --test sitl_live -- --ignored
      - run: cargo build -p ofs-sim
      - run: python -m pip install -e "python[dev]"
      - run: python -m pytest python/tests/test_sitl_hover.py -v
```

- [ ] **Step 4: Verify everything locally**

Run: `cargo test --workspace && cargo build -p ofs-sim && python -m pytest python/tests -v`
Expected: all Rust tests pass; Python 4 passed, 1 skipped (or 5 passed with `OFS_SITL_LAUNCH` set).

- [ ] **Step 5: Commit**

```bash
git add README.md docs/dev-setup.md .github/workflows/ci.yml
git commit -m "docs: README, developer setup and CI workflow"
```
