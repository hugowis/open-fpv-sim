//! Pins the bus digest of a world with a box in the flight path, recorded on `main` before the rotated-boxes work,
//! so a world that writes no yaw can be shown to run bit-identically afterwards. The quad climbs into a slab and
//! rests against it (spheres and landing points touch the box, the RF loss runs through it), so the whole object
//! chain feeds the digest. libm differs per OS: like flat_world_pin.rs, Windows and Linux/WSL have their own pins.
#![cfg(any(windows, target_os = "linux"))]
use std::path::Path;

use ofs_config::{load, FcKind, QuadConfig, WorldConfig};
use ofs_sim::vehicle::{build, BuildOptions, Sticks};

#[cfg(target_os = "linux")]
const PIN_HIT: u64 = 12011186114502832584;
#[cfg(target_os = "linux")]
const PIN_REST: u64 = 21131293073156954;
#[cfg(windows)]
const PIN_HIT: u64 = 6566639466238057605;
#[cfg(windows)]
const PIN_REST: u64 = 21131293073156954;

/// The open field with one 10 x 10 m slab hanging 3 m over home: the climbing quad meets its underside.
pub fn object_world() -> WorldConfig {
    let mut w = WorldConfig::open_field();
    w.objects.push(ofs_config::world::ObjectSection {
        name: "Slab".into(),
        shape: ofs_config::world::Shape::Box,
        center_ned_m: [0.0, 0.0, -23.0],
        size_m: Some([10.0, 10.0, 40.0]),
        radius_m: None,
        height_m: None,
        yaw_deg: None,
        color: [0.6, 0.6, 0.6],
        rf_loss_db: 25.0,
    });
    w
}

#[test]
fn an_unrotated_object_world_is_pinned() {
    let cfg: QuadConfig = load(Path::new(&format!("{}/../../quads/opendrone-5r-race.toml", env!("CARGO_MANIFEST_DIR")))).unwrap();
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: object_world() };
    let mut v = build(&cfg, &opts, None).unwrap();
    v.set_sticks(&Sticks { throttle: 0.3, ..Sticks::default() }).unwrap();
    v.run_for(1.0).unwrap();
    let hit = v.digest();
    assert!(v.state().collision_event_count >= 1, "the flight must touch the slab, else the pin proves nothing");
    v.set_sticks(&Sticks { throttle: 0.0, ..Sticks::default() }).unwrap();
    v.run_for(1.0).unwrap();
    let rest = v.digest();
    println!("hit={hit} rest={rest}");
    assert_eq!(hit, PIN_HIT, "digest after the climb into the slab");
    assert_eq!(rest, PIN_REST, "digest after resting against it");
}

#[test]
fn a_yawed_object_world_replays_exactly_and_differs_from_the_unyawed() {
    // A slab offset 6 m east misses the climbing quad: its footprint starts 1 m east of the column, and the
    // flight sails through empty air. The same slab yawed 45 degrees swings its south corner 1.07 m past
    // the column, and the climb meets the rotated underside instead. (A slab centred on the column cannot
    // tell the two apart: the column runs down the slab's centre axis, where the signed distance is
    // -min(half) at every yaw, so the plan's centred variant moved no bit — the offsets above were tried.)
    // The digest comparisons are same-machine flights against each other, never a yawed value against a
    // cross-OS constant (trig in the SDF is libm, whose last bits differ per OS).
    let fly = |world: WorldConfig| {
        let cfg: QuadConfig = load(Path::new(&format!("{}/../../quads/opendrone-5r-race.toml", env!("CARGO_MANIFEST_DIR")))).unwrap();
        let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world };
        let mut v = build(&cfg, &opts, None).unwrap();
        v.set_sticks(&Sticks { throttle: 0.3, ..Sticks::default() }).unwrap();
        v.run_for(1.0).unwrap();
        let hit = v.digest();
        v.set_sticks(&Sticks { throttle: 0.0, ..Sticks::default() }).unwrap();
        v.run_for(1.0).unwrap();
        (hit, v.digest(), v.state().collision_event_count)
    };
    let world = |yaw_deg: Option<f64>| {
        let mut w = object_world();
        w.objects[0].center_ned_m = [0.0, 6.0, -23.0];
        w.objects[0].yaw_deg = yaw_deg;
        w
    };
    let yawed_once = fly(world(Some(45.0)));
    let yawed_again = fly(world(Some(45.0)));
    assert_eq!(yawed_once, yawed_again, "the yawed world replays to the same digests");
    assert!(yawed_once.2 >= 1, "the corner of the rotated slab stands over the column, else the test proves nothing");
    let unrotated = fly(world(None));
    assert_eq!(unrotated.2, 0, "the axis-aligned slab starts a metre east of the column");
    assert_ne!(yawed_once.0, unrotated.0, "the rotation puts the slab's corner in the climb");
    assert_ne!(yawed_once.1, unrotated.1, "the rest differs too");
}
