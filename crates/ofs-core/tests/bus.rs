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
