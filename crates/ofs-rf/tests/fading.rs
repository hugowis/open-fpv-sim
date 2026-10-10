use glam::{DQuat, DVec3};
use ofs_rf::fading::{body_shadow_db, Diversity, BODY_SHADOW_DB};

fn close(actual: f64, expected: f64, eps: f64, what: &str) {
    assert!((actual - expected).abs() <= eps, "{what}: expected {expected} +- {eps}, got {actual}");
}

#[test]
fn diversity_switches_only_for_a_clearly_better_antenna() {
    let mut d = Diversity::default();
    assert_eq!(d.choose(&[10.0, 11.0]), 0, "1 dB better is not enough");
    assert_eq!(d.choose(&[10.0, 12.5]), 1, "2.5 dB better: switch");
    assert_eq!(d.choose(&[11.0, 10.0]), 1, "and stay while the other is only 1 dB better");
    assert_eq!(d.choose(&[13.0, 10.0]), 0, "back when it is 3 dB better");
    assert_eq!(d.choose(&[5.0]), 0, "a single antenna");
    assert_eq!(d.active(), 0);
}

#[test]
fn the_frame_shadows_the_antenna_when_the_other_end_is_ahead_and_below() {
    let level = DQuat::IDENTITY;
    let quad = DVec3::new(0.0, 0.0, -20.0);
    close(body_shadow_db(level, quad, DVec3::new(0.0, 0.0, 0.0)), BODY_SHADOW_DB, 1e-9, "straight below");
    close(body_shadow_db(level, quad, DVec3::new(-100.0, 0.0, -20.0)), 0.0, 1e-12, "behind, level");
    close(body_shadow_db(level, quad, DVec3::new(0.0, 0.0, -40.0)), 0.0, 1e-12, "above");
    let ahead = body_shadow_db(level, quad, DVec3::new(100.0, 0.0, -20.0));
    assert!(ahead > 4.0 && ahead < BODY_SHADOW_DB, "ahead, level: most of it ({ahead})");
    let turned = DQuat::from_rotation_z(std::f64::consts::PI);
    close(body_shadow_db(turned, quad, DVec3::new(100.0, 0.0, -20.0)), 0.0, 1e-9, "turned away: the antenna sees the other end");
}
