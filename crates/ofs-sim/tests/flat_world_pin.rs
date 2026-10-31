//! Pins the bus digest of the flat world, recorded on `main` before the terrain heightfield work, so a world without a
//! `[terrain]` table can be shown to run bit-identically afterwards. The scenario climbs, then falls, lands, bounces and
//! rests (0.3 throttle for 0.5 s, then none for 3 s: the race quad is too strong for 0.7 to ever come back down), so the contact spheres, landing points and friction all feed the digest.
//!
//! The Linux pin was recorded on Linux/WSL and the Windows one natively; macOS is not pinned.
#![cfg(any(windows, target_os = "linux"))]
use std::path::Path;

use ofs_config::{load, FcKind, QuadConfig, WorldConfig};
use ofs_sim::vehicle::{build, BuildOptions, Sticks};

// libm differs per OS, so the digest does too: Windows has its own pin, Linux/WSL the other.
#[cfg(target_os = "linux")]
const PIN_CLIMB: u64 = 2546269408358881885;
#[cfg(target_os = "linux")]
const PIN_REST: u64 = 6789808347882939318;
#[cfg(windows)]
const PIN_CLIMB: u64 = 1897724010399850095;
#[cfg(windows)]
const PIN_REST: u64 = 11299260126608164184;

#[test]
fn flat_world_digest_is_pinned() {
    let cfg: QuadConfig = load(Path::new(&format!("{}/../../quads/opendrone-5r-race.toml", env!("CARGO_MANIFEST_DIR")))).unwrap();
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    let mut v = build(&cfg, &opts, None).unwrap();
    v.set_sticks(&Sticks { throttle: 0.3, ..Sticks::default() }).unwrap();
    v.run_for(0.5).unwrap();
    let climb = v.digest();
    v.set_sticks(&Sticks { throttle: 0.0, ..Sticks::default() }).unwrap();
    v.run_for(3.0).unwrap();
    let rest = v.digest();
    println!("climb={climb} rest={rest}");
    assert!(v.state().pos_ned_m.z.abs() < 0.1, "the quad should rest on the ground, z = {}", v.state().pos_ned_m.z);
    assert_eq!(climb, PIN_CLIMB, "digest after the climb");
    assert_eq!(rest, PIN_REST, "digest after the fall and rest");
}
