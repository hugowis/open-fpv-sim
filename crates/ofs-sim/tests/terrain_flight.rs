//! Flying over the shipped hills world (#153): it loads, a seeded flight replays exactly, the terrain signal exists only
//! with a terrain, and a quad that falls onto a slope comes to rest on the surface, not on the plane d = 0.
use std::path::Path;
use std::time::Instant;

use glam::DVec3;

use ofs_config::{load, FcKind, QuadConfig, WorldConfig};
use ofs_sim::vehicle::{build, BuildOptions, Sticks, Vehicle};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn hills() -> WorldConfig {
    ofs_config::world::load(Path::new(&format!("{ROOT}/worlds/hills.toml"))).unwrap()
}

fn race() -> QuadConfig {
    load(Path::new(&format!("{ROOT}/quads/opendrone-5r-race.toml"))).unwrap()
}

fn vehicle_with(cfg: &QuadConfig, world: WorldConfig, fc: FcKind) -> Vehicle {
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(fc), world };
    build(cfg, &opts, None).unwrap()
}

fn vehicle(cfg: &QuadConfig, world: WorldConfig) -> Vehicle {
    vehicle_with(cfg, world, FcKind::OpenLoop)
}

fn sticks(throttle: f64, pitch: f64) -> Sticks {
    Sticks { throttle, pitch, ..Sticks::default() }
}

/// The same stick script on a vehicle (the plain open-loop FC ignores the pitch stick, so this one holds the angle):
/// fly off the pad over the rising ground, then fall onto it. Returns the digest after each phase.
fn fly(cfg: &QuadConfig, world: WorldConfig) -> (u64, u64) {
    let mut v = vehicle_with(cfg, world, FcKind::OpenLoopStabilized);
    v.set_sticks(&sticks(0.45, 0.6)).unwrap();
    v.run_for(4.0).unwrap();
    let flown = v.digest();
    v.set_sticks(&sticks(0.0, 0.0)).unwrap();
    v.run_for(3.0).unwrap();
    (flown, v.digest())
}

#[test]
fn hills_world_loads_and_is_valid() {
    let w = hills();
    assert_eq!(w.name, "Rolling hills");
    assert!(w.terrain.is_some());
}

#[test]
fn seeded_flight_over_hills_replays_exactly() {
    let cfg = race();
    let a = fly(&cfg, hills());
    assert_eq!(a, fly(&cfg, hills()), "the same script over the same hills replays exactly");

    let mut other = hills();
    other.terrain.as_mut().unwrap().seed += 1;
    assert_ne!(a.1, fly(&cfg, other).1, "another seed is another landscape");
    assert_ne!(a.1, fly(&cfg, WorldConfig::open_field()).1, "the hills are not the flat field");
}

#[test]
fn terrain_signal_only_with_terrain() {
    let cfg = race();
    assert!(vehicle(&cfg, hills()).signal("body.terrain_d_m").is_some());
    assert!(vehicle(&cfg, WorldConfig::open_field()).signal("body.terrain_d_m").is_none());
}

/// Dropped from 3 m onto a 10 degree flank 100 m out (beyond the 30 m flat disc), the quad rests on the surface, not on the plane d = 0.
#[test]
fn quad_lands_on_a_hillside() {
    let ground = hills().terrain.unwrap().build();
    let (n, e) = (-20.0, -100.0);
    let mut cfg = race();
    cfg.initial.position_ned_m = [n, e, ground.ground_d(n, e) - 3.0];
    let mut v = vehicle(&cfg, hills());
    v.run_for(6.0).unwrap();
    let s = v.state();
    let (ground_d, normal) = ground.surface(s.pos_ned_m.x, s.pos_ned_m.y);
    assert!(normal.z.abs().acos().to_degrees() > 5.0, "the spot is sloped");
    assert!(ground_d < -1.0, "the spot is well above the plane, ground_d = {ground_d}");
    assert!(s.vel_ned_mps.length() < 0.01, "at rest, speed {}", s.vel_ned_mps.length());
    assert!((s.pos_ned_m.z - ground_d).abs() < 0.1, "rests on the surface: z = {}, ground_d = {ground_d}", s.pos_ned_m.z);
    // The spherical body can rest tilted a few degrees off the slope; level would be as far off as the slope (~10 degrees) plus this.
    let up = s.att * DVec3::new(0.0, 0.0, -1.0);
    let off = up.angle_between(normal).to_degrees();
    assert!(off < 12.0, "lies roughly along the slope, {off} degrees off the normal");
}

/// Wall time per simulated second of 10 s of hops (a short climb, then a fall and bounces, repeated) at the quad's
/// base rate; run with `--release -- --ignored --nocapture`.
#[test]
#[ignore = "timing: run in release"]
fn terrain_costs_little_in_release() {
    let mut files: Vec<_> = std::fs::read_dir(format!("{ROOT}/quads")).unwrap().map(|f| f.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect();
    files.sort();
    // The best of five fresh vehicles, so a cold cache or a scheduler hiccup does not read as a cost.
    let ms_per_s = |cfg: &QuadConfig, world: &WorldConfig| {
        (0..5)
            .map(|_| {
                let mut v = vehicle(cfg, world.clone());
                let t = Instant::now();
                for _ in 0..10 {
                    v.set_sticks(&sticks(0.3, 0.0)).unwrap();
                    v.run_for(0.3).unwrap();
                    v.set_sticks(&sticks(0.0, 0.0)).unwrap();
                    v.run_for(0.7).unwrap();
                }
                t.elapsed().as_secs_f64() * 1000.0 / 10.0
            })
            .fold(f64::MAX, f64::min)
    };
    let mut slow = Vec::new();
    for f in files {
        let cfg: QuadConfig = load(&f).unwrap();
        let flat = ms_per_s(&cfg, &WorldConfig::open_field());
        let hilly = ms_per_s(&cfg, &hills());
        eprintln!("{}: flat {flat:.1} ms/s, hills {hilly:.1} ms/s, ratio {:.2}", f.file_name().unwrap().to_string_lossy(), hilly / flat);
        assert!(flat < 1000.0 && hilly < 1000.0, "slower than real time");
        // A vehicle step is a few ms a simulated second, so a ratio alone is noise-prone: the terrain may cost more than
        // half as much again only if that stays under 10 ms a second (1% of real time).
        if hilly / flat >= 1.5 && hilly - flat >= 10.0 {
            slow.push(f.file_name().unwrap().to_string_lossy().into_owned());
        }
    }
    assert!(slow.is_empty(), "the terrain costs too much on {slow:?}");
}
