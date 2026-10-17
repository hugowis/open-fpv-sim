//! The radio inside a whole vehicle: two lockstep flights recorded to MCAP, hashed. The digests are per OS
//! (`tests/golden/radio-flight.windows.digests`, `radio-flight.linux.digests`) because libm's f64 maths differs
//! between Windows and Linux. Each was generated from the pre-split code (M5 Task 1); the radio chain (M5 Task 2's
//! split) must not change a byte of what the vehicle records, on each OS against its own file. Regenerate only on
//! purpose, on the OS whose file you mean to change: `OFS_GOLDEN_WRITE=1 cargo test -p ofs-sim --test radio_golden`.
use std::path::{Path, PathBuf};

use ofs_config::{load, FcKind, WorldConfig};
use ofs_sim::vehicle::{build, BuildOptions, Fault, Sticks};

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// A 2 s open-loop flight (throttle and roll steps, a 30 % radio-loss fault from 1 s) recorded with the default
/// table; returns the digest of the MCAP file.
fn flight(quad: &str, seed: u64, path: &Path) -> u64 {
    let quad_path = format!("{}/../../quads/{quad}", env!("CARGO_MANIFEST_DIR"));
    let cfg = load(Path::new(&quad_path)).unwrap();
    let opts = BuildOptions {
        seed,
        data_dir: std::env::temp_dir().join("ofs-test-data"),
        fc_override: Some(FcKind::OpenLoop),
        world: WorldConfig::open_field(),
    };
    let mut v = build(&cfg, &opts).unwrap();
    v.start_recording(&[], path).unwrap();
    v.schedule_fault(Fault::RadioLoss { fraction: 0.3 }, 1.0)
        .unwrap();
    for (throttle, roll, secs) in [(0.0, 0.0, 0.5), (0.6, 0.0, 0.75), (0.4, 0.5, 0.75)] {
        v.set_sticks(&Sticks {
            throttle,
            roll,
            ..Sticks::default()
        })
        .unwrap();
        v.run_for(secs).unwrap();
    }
    v.stop_recording().unwrap();
    fnv1a(&std::fs::read(path).unwrap())
}

/// The golden file for this OS, or None where none was recorded.
fn golden_path() -> Option<PathBuf> {
    let name = if cfg!(target_os = "windows") {
        "radio-flight.windows.digests"
    } else if cfg!(target_os = "linux") {
        "radio-flight.linux.digests"
    } else {
        return None;
    };
    Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name))
}

#[test]
fn a_recorded_vehicle_flight_reproduces_its_digests() {
    let Some(golden) = golden_path() else {
        println!("radio goldens exist for Windows and Linux only; skipping on this OS");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let current = [
        (
            "freestyle",
            flight("opendrone-5f-freestyle.toml", 1, &dir.path().join("a.mcap")),
        ),
        (
            "weak-link",
            flight("opendrone-5f-weak-link.toml", 3, &dir.path().join("b.mcap")),
        ),
    ];
    let text: String = current
        .iter()
        .map(|(name, d)| format!("{name} {d:016x}\n"))
        .collect();
    if std::env::var_os("OFS_GOLDEN_WRITE").is_some() {
        std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
        std::fs::write(&golden, &text).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&golden)
        .expect("tests/golden/radio-flight.<os>.digests (run once with OFS_GOLDEN_WRITE=1)");
    assert_eq!(
        text,
        expected.replace("\r\n", "\n"),
        "the vehicle's recording changed; the radio chain must be byte-identical. If the change is deliberate (a new channel, a rate-table row), regenerate both OS files with OFS_GOLDEN_WRITE=1 and say why in the commit; if it is the radio, regenerate only from the pre-split commit 1d9f204"
    );
}
