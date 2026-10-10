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

#[test]
fn non_finite_scalars_and_quaternions_are_found_first_by_name() {
    let mut bus = Bus::new();
    let q = bus.signal::<DQuat>("b.att");
    let s = bus.signal::<f64>("c.speed");
    let ok = bus.signal::<DVec3>("a.pos");
    bus.set(ok, DVec3::ONE);
    bus.set(s, f64::INFINITY);
    assert_eq!(bus.first_non_finite(), Some("c.speed"));
    bus.set(q, DQuat::from_xyzw(f64::NAN, 0.0, 0.0, 1.0));
    assert_eq!(bus.first_non_finite(), Some("b.att"), "name order, not registration order");
    bus.set(q, DQuat::IDENTITY);
    bus.set(s, 1.0);
    assert_eq!(bus.first_non_finite(), None);
}

#[test]
fn digest_covers_every_component_of_vectors_and_quaternions() {
    let base = || {
        let mut bus = Bus::new();
        let v = bus.signal::<DVec3>("v");
        let q = bus.signal::<DQuat>("q");
        (bus, v, q)
    };
    let (reference, _, _) = base();
    for component in 0..3 {
        let (mut bus, v, _) = base();
        let mut a = [0.0; 3];
        a[component] = 1e-12;
        bus.set(v, DVec3::from_array(a));
        assert_ne!(bus.digest(), reference.digest(), "vec3 component {component}");
    }
    for component in 0..4 {
        let (mut bus, _, q) = base();
        let mut a = DQuat::default().to_array();
        a[component] += 1e-12;
        bus.set(q, DQuat::from_array(a));
        assert_ne!(bus.digest(), reference.digest(), "quat component {component}");
    }
}

#[test]
fn tables_must_be_non_empty_and_strictly_increasing() {
    assert!(interp::check_table(&[(0.0, 1.0), (1.0, 2.0)], "t").is_ok());
    assert!(interp::check_table(&[], "t").unwrap_err().contains("empty"));
    assert!(interp::check_table(&[(1.0, 1.0), (1.0, 2.0)], "t").unwrap_err().contains("strictly increasing"));
    assert!(interp::check_table(&[(0.0, f64::NAN)], "t").unwrap_err().contains("finite"));
}
