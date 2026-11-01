use std::sync::Arc;

use glam::DVec3;
use ofs_core::mesh::{TriMesh, Triangle};
use ofs_core::shape::Shape;
use ofs_core::terrain::Terrain;
use ofs_rf::propagation::*;

fn close(actual: f64, expected: f64, eps: f64, what: &str) {
    assert!((actual - expected).abs() <= eps, "{what}: expected {expected} +- {eps}, got {actual}");
}

fn omni(axis: DVec3, polarization: Polarization) -> Antenna {
    Antenna { kind: AntennaKind::Omni, gain_dbi: 2.0, polarization, axis }
}

#[test]
fn free_space_path_loss_matches_the_formula() {
    close(fspl_db(100.0, 5800.0), 87.72, 0.01, "100 m at 5800 MHz");
    close(fspl_db(1000.0, 5800.0) - fspl_db(100.0, 5800.0), 20.0, 1e-9, "20 dB per decade");
    close(fspl_db(0.1, 5800.0), fspl_db(1.0, 5800.0), 1e-12, "clamped at 1 m");
    close(wavelength_m(5800.0), 0.05169, 1e-5, "wavelength");
}

#[test]
fn power_helpers_round_trip() {
    close(mw_to_dbm(25.0), 13.98, 0.01, "25 mW");
    close(dbm_to_mw(30.0), 1000.0, 1e-9, "30 dBm");
    close(power_sum_dbm([-90.0, -90.0]), -86.99, 0.01, "two equal powers add 3 dB");
}

#[test]
fn an_omni_peaks_across_its_axis_and_nulls_along_it() {
    let a = omni(DVec3::NEG_Z, Polarization::Rhcp);
    close(a.gain_towards(DVec3::X), 2.0, 1e-9, "broadside");
    close(a.gain_towards(DVec3::NEG_Z), 2.0 - PATTERN_FLOOR_DB, 1e-9, "along the axis: the floor");
    close(a.gain_towards(DVec3::new(1.0, 0.0, -1.0).normalize()), 2.0 - 3.0103, 0.001, "45 degrees off the axis: sin^2");
}

#[test]
fn a_patch_is_3_db_down_at_half_its_beamwidth_and_floored_behind() {
    let a = Antenna { kind: AntennaKind::Patch { beamwidth_deg: 60.0 }, gain_dbi: 8.0, polarization: Polarization::Rhcp, axis: DVec3::X };
    close(a.gain_towards(DVec3::X), 8.0, 1e-9, "boresight");
    let half = 30f64.to_radians();
    close(a.gain_towards(DVec3::new(half.cos(), half.sin(), 0.0)), 8.0 - 3.0103, 1e-4, "half power at 30 degrees");
    close(a.gain_towards(DVec3::NEG_X), 8.0 - PATTERN_FLOOR_DB, 1e-9, "behind");
    close(a.gain_towards(DVec3::Y), 8.0 - PATTERN_FLOOR_DB, 1e-9, "at 90 degrees");
}

#[test]
fn polarization_losses_follow_the_table() {
    let path = DVec3::X;
    let rhcp = omni(DVec3::NEG_Z, Polarization::Rhcp);
    let lhcp = omni(DVec3::NEG_Z, Polarization::Lhcp);
    let vertical = omni(DVec3::NEG_Z, Polarization::Linear);
    let horizontal = omni(DVec3::Y, Polarization::Linear);
    let tilted = omni(DVec3::new(0.0, 1.0, -1.0).normalize(), Polarization::Linear);
    close(polarization_loss_db(&rhcp, Polarization::Rhcp, &rhcp, path), 0.0, 1e-12, "same hand");
    close(polarization_loss_db(&rhcp, Polarization::Rhcp, &lhcp, path), CROSS_POLARIZATION_DB, 1e-12, "opposite hands");
    close(polarization_loss_db(&rhcp, Polarization::Rhcp, &vertical, path), CIRCULAR_TO_LINEAR_DB, 1e-12, "circular to linear");
    close(polarization_loss_db(&vertical, Polarization::Linear, &vertical, path), 0.0, 1e-9, "aligned dipoles");
    close(polarization_loss_db(&vertical, Polarization::Linear, &tilted, path), 3.0103, 0.001, "45 degrees apart");
    close(polarization_loss_db(&vertical, Polarization::Linear, &horizontal, path), CROSS_POLARIZATION_DB, 1e-9, "crossed: capped");
    assert_eq!(Polarization::Rhcp.reflected(), Polarization::Lhcp);
    assert_eq!(Polarization::Linear.reflected(), Polarization::Linear);
}

#[test]
fn knife_edge_loss_matches_itu_r_p526() {
    close(knife_edge_loss_db(0.0), 6.03, 0.01, "grazing: about 6 dB");
    close(knife_edge_loss_db(-0.78), 0.0, 1e-12, "clear");
    close(knife_edge_loss_db(-2.0), 0.0, 1e-12, "well clear");
    close(knife_edge_loss_db(1.0), 13.96, 0.05, "v = 1");
    close(knife_edge_loss_db(2.4), 20.6, 0.1, "v = 2.4");
}

#[test]
fn signed_distances_of_the_shapes() {
    let b = Shape::Box { center: DVec3::ZERO, half: DVec3::new(2.0, 1.0, 1.0), yaw_rad: 0.0 };
    close(b.signed_distance(DVec3::new(5.0, 0.0, 0.0)), 3.0, 1e-12, "outside along north");
    close(b.signed_distance(DVec3::ZERO), -1.0, 1e-12, "the centre is 1 m from the nearest face");
    let c = Shape::Cylinder { center: DVec3::new(0.0, 0.0, -5.0), radius: 1.0, half_height: 5.0 };
    close(c.signed_distance(DVec3::new(3.0, 0.0, -5.0)), 2.0, 1e-12, "beside it");
    close(c.signed_distance(DVec3::new(0.0, 0.0, -12.0)), 2.0, 1e-12, "above it");
    close(c.signed_distance(DVec3::new(0.0, 0.0, -5.0)), -1.0, 1e-12, "on its axis");
}

#[test]
fn a_shape_standing_on_the_ground_is_rooted_below_it_and_a_floating_one_is_not() {
    let wall = Shape::Box { center: DVec3::new(0.0, 0.0, -10.0), half: DVec3::new(5.0, 5.0, 10.0), yaw_rad: 0.0 };
    let p = DVec3::new(0.0, 0.0, -1.0); // inside the wall, 1 m above the ground and 5 m from its sides
    close(wall.signed_distance(p), -1.0, 1e-12, "unrooted: the bottom face is nearest");
    close(wall.rooted().signed_distance(p), -5.0, 1e-9, "rooted: the side faces are");
    close(wall.rooted().signed_distance(DVec3::new(0.0, 0.0, -25.0)), 5.0, 1e-9, "the top stays where it was");
    let bar = Shape::Box { center: DVec3::new(0.0, 0.0, -2.5), half: DVec3::new(0.1, 1.6, 0.06), yaw_rad: 0.0 };
    assert_eq!(bar.rooted(), bar, "a gate's top bar floats: unchanged");
    let pylon = Shape::Cylinder { center: DVec3::new(0.0, 0.0, -1.5), radius: 0.15, half_height: 1.5 };
    close(pylon.rooted().signed_distance(DVec3::new(0.0, 0.0, -4.0)), 1.0, 1e-9, "a rooted cylinder keeps its top");
    close(pylon.rooted().signed_distance(DVec3::new(0.0, 0.0, -0.5)), -0.15, 1e-9, "and its radius");
}

#[test]
fn a_building_on_the_path_costs_its_loss_and_a_clear_one_costs_nothing() {
    let building = Obstacle { shape: Shape::Box { center: DVec3::new(50.0, 0.0, -10.0), half: DVec3::new(5.0, 5.0, 10.0), yaw_rad: 0.0 }.rooted(), rf_loss_db: 25.0 };
    let lambda = wavelength_m(5800.0);
    let (a, b) = (DVec3::new(0.0, 0.0, -2.0), DVec3::new(100.0, 0.0, -2.0));
    close(obstruction_loss_db(&building, a, b, lambda), 25.0, 1e-9, "straight through: capped at rf_loss_db");
    let beside = DVec3::new(100.0, 20.0, -2.0);
    let past = DVec3::new(0.0, 20.0, -2.0);
    close(obstruction_loss_db(&building, past, beside, lambda), 0.0, 1e-9, "15 m clear of it");
    let transparent = Obstacle { rf_loss_db: 0.0, ..building };
    close(obstruction_loss_db(&transparent, a, b, lambda), 0.0, 1e-12, "a 0 dB object never blocks");
}

#[test]
fn the_loss_fades_in_over_a_few_metres_at_a_buildings_edge() {
    // The receiver is 100 m south of a 10 m wide building; the quad moves east behind it, 100 m north of it.
    let building = Obstacle { shape: Shape::Box { center: DVec3::new(100.0, 0.0, -10.0), half: DVec3::new(5.0, 5.0, 10.0), yaw_rad: 0.0 }.rooted(), rf_loss_db: 30.0 };
    let lambda = wavelength_m(5800.0);
    let rx = DVec3::new(0.0, 0.0, -2.0);
    let loss_at = |east: f64| obstruction_loss_db(&building, rx, DVec3::new(200.0, east, -2.0), lambda);
    let losses: Vec<f64> = [30.0, 14.0, 10.0, 6.0, 0.0].iter().map(|e| loss_at(*e)).collect();
    assert!(losses[0] < 0.5, "well clear: {losses:?}");
    assert!(losses.windows(2).all(|w| w[1] >= w[0]), "rises steadily as the path goes in: {losses:?}");
    assert!(losses[2] > 3.0 && losses[2] < 9.0, "the line just grazes the edge: about 6 dB: {losses:?}");
    // Directly behind, the path is 5 m from the nearest side: v = 4.4, J(v) = 25.7 dB, under the 30 dB cap.
    close(losses[4], 25.70, 0.01, "fully behind: diffraction around the sides");
}

#[test]
fn a_thin_object_on_a_long_path_is_still_found() {
    // A 0.5 m post exactly between two ends 4 km apart: a fixed sampling would step over it.
    let post = Obstacle { shape: Shape::Box { center: DVec3::new(2000.0, 0.0, -5.0), half: DVec3::new(0.25, 0.25, 5.0), yaw_rad: 0.0 }.rooted(), rf_loss_db: 10.0 };
    let loss = obstruction_loss_db(&post, DVec3::new(0.0, 0.0, -2.0), DVec3::new(4000.0, 0.0, -2.0), wavelength_m(5800.0));
    assert!(loss > 5.0, "the post is in the way: {loss}");
}

#[test]
fn circular_antennas_reject_most_of_the_ground_bounce() {
    let freq = 5800.0;
    let spread = |polarization: Polarization| {
        let mut gains = Vec::new();
        for i in 0..400 {
            let d = 100.0 + f64::from(i) * 0.5;
            let tx = Endpoint { position: DVec3::new(d, 0.0, -10.0), antenna: omni(DVec3::NEG_Z, polarization) };
            let rx = Endpoint { position: DVec3::new(0.0, 0.0, -1.7), antenna: omni(DVec3::NEG_Z, polarization) };
            let with = path_gain(&tx, &rx, freq, &[], None, &[], true).gain_db;
            let without = path_gain(&tx, &rx, freq, &[], None, &[], false).gain_db;
            gains.push(with - without);
        }
        let max = gains.iter().cloned().fold(f64::MIN, f64::max);
        let min = gains.iter().cloned().fold(f64::MAX, f64::min);
        (max, min)
    };
    let (circular_max, circular_min) = spread(Polarization::Rhcp);
    let (linear_max, linear_min) = spread(Polarization::Linear);
    assert!(circular_max < 1.5 && circular_min > -1.5, "circular: a ripple under 1.5 dB ({circular_min}..{circular_max})");
    assert!(linear_min < -10.0 && linear_max > 4.0, "linear: deep fades and peaks ({linear_min}..{linear_max})");
}

#[test]
fn the_direct_path_adds_gains_and_subtracts_the_path_loss() {
    let tx = Endpoint { position: DVec3::new(100.0, 0.0, -2.0), antenna: omni(DVec3::NEG_Z, Polarization::Rhcp) };
    let rx = Endpoint { position: DVec3::new(0.0, 0.0, -2.0), antenna: omni(DVec3::NEG_Z, Polarization::Rhcp) };
    let p = path_gain(&tx, &rx, 5800.0, &[], None, &[], false);
    close(p.gain_db, 2.0 + 2.0 - fspl_db(100.0, 5800.0), 1e-9, "two broadside omnis");
    assert_eq!(p.obstruction_db, 0.0);
}

#[test]
fn adjacent_channel_rejection_follows_its_curve() {
    close(adjacent_channel_rejection_db(0.0), 0.0, 1e-12, "same channel");
    close(adjacent_channel_rejection_db(10.0), 5.0, 1e-12, "halfway to 20 MHz");
    close(adjacent_channel_rejection_db(-20.0), 10.0, 1e-12, "either side");
    close(adjacent_channel_rejection_db(40.0), 25.0, 1e-12, "40 MHz");
    close(adjacent_channel_rejection_db(37.0), 22.75, 1e-12, "37 MHz (the next Raceband channel)");
    close(adjacent_channel_rejection_db(500.0), 40.0, 1e-12, "far away");
}

// Terrain: a 41 x 41 grid, 1 m cells, north 0..40 and east -20..20; the fixtures are constant along east, so the
// crest line is a north row.
fn grid(row_height: impl Fn(usize) -> f32) -> Terrain {
    Terrain::from_grid([0.0, -20.0], 1.0, 41, 41, (0..41 * 41).map(|k| row_height(k / 41)).collect()).unwrap()
}

fn tent(row: usize, crest_row: usize, crest_m: f32) -> f32 {
    (crest_m * (1.0 - row.abs_diff(crest_row) as f32 / 10.0)).max(0.0)
}

fn ridge(crest_row: usize, crest_m: f32) -> Terrain {
    grid(|r| tent(r, crest_row, crest_m))
}

fn v_at(terrain: &Terrain, a: DVec3, b: DVec3, lambda: f64, t: f64) -> f64 {
    let (p, length) = (a.lerp(b, t), a.distance(b));
    fresnel_v(p.z - terrain.ground_d(p.x, p.y), t * length, (1.0 - t) * length, lambda)
}

/// The loss from a dense scan of the whole ray: no early-out, no refinement.
fn scanned_loss(terrain: &Terrain, a: DVec3, b: DVec3, lambda: f64, points: usize) -> f64 {
    let v = (0..=points).map(|k| v_at(terrain, a, b, lambda, k as f64 / points as f64)).fold(f64::NEG_INFINITY, f64::max);
    knife_edge_loss_db(v).min(TERRAIN_RF_LOSS_DB)
}

fn along_north(z: f64) -> (DVec3, DVec3) {
    (DVec3::new(0.0, 0.0, z), DVec3::new(40.0, 0.0, z))
}

#[test]
fn a_ray_well_above_the_ridge_loses_nothing() {
    let (a, b) = along_north(-40.0);
    assert_eq!(terrain_loss_db(&ridge(20, 10.0), a, b, wavelength_m(5800.0)), 0.0);
}

#[test]
fn grazing_the_crest_costs_about_6_db() {
    let (a, b) = along_north(-10.0);
    close(terrain_loss_db(&ridge(20, 10.0), a, b, wavelength_m(5800.0)), 6.03, 0.3, "grazing the crest");
}

#[test]
fn a_ray_below_the_crest_matches_the_knife_edge_formula() {
    let lambda = wavelength_m(5800.0);
    let (a, b) = along_north(-2.0);
    let expected = knife_edge_loss_db(fresnel_v(8.0, 20.0, 20.0, lambda));
    close(terrain_loss_db(&ridge(20, 10.0), a, b, lambda), expected, 0.05, "8 m into the path, midway");
}

#[test]
fn the_loss_rises_gradually_as_the_ray_sinks() {
    let (ridge, lambda) = (ridge(20, 10.0), wavelength_m(5800.0));
    let loss_at = |z: f64| {
        let (a, b) = along_north(z);
        terrain_loss_db(&ridge, a, b, lambda)
    };
    // At 5.8 GHz the first Fresnel zone is half a metre wide, so the transition is a metre deep: whole metres
    // jump, tenths of a metre do not.
    let metres: Vec<f64> = (-14..=-2).map(|z| loss_at(f64::from(z))).collect();
    assert!(metres.windows(2).all(|w| w[1] >= w[0]), "monotone: {metres:?}");
    let tenths: Vec<f64> = (0..=40).map(|k| loss_at(-12.0 + f64::from(k) * 0.1)).collect();
    assert!(tenths.windows(2).all(|w| w[1] >= w[0] && w[1] - w[0] < 3.0), "gradual: {tenths:?}");
    let (above, at, below) = (loss_at(-10.25), loss_at(-10.0), loss_at(-9.75));
    assert!(above < at && at < below, "grazing sits between 0.25 m clear and 0.25 m in: {above} {at} {below}");
}

#[test]
fn a_ray_that_never_meets_the_ridge_loses_nothing() {
    let (a, b) = (DVec3::new(30.0, 0.0, -2.0), DVec3::new(40.0, 0.0, -2.0));
    assert_eq!(terrain_loss_db(&ridge(20, 10.0), a, b, wavelength_m(5800.0)), 0.0);
}

#[test]
fn the_dominant_of_two_ridges_sets_the_loss() {
    let lambda = wavelength_m(5800.0);
    let two = grid(|r| tent(r, 12, 6.0).max(tent(r, 28, 10.0)));
    let (a, b) = along_north(-2.0);
    let first = knife_edge_loss_db(fresnel_v(4.0, 12.0, 28.0, lambda));
    let second = knife_edge_loss_db(fresnel_v(8.0, 28.0, 12.0, lambda));
    assert!(first < second, "the fixture has a clear winner: {first} {second}");
    close(terrain_loss_db(&two, a, b, lambda), first.max(second), 0.05, "the larger edge, not their sum");
}

#[test]
fn a_flat_terrain_changes_nothing_for_a_raised_ray() {
    let flat = grid(|_| 0.0);
    let (a, b) = along_north(-2.0);
    assert_eq!(terrain_loss_db(&flat, a, b, wavelength_m(5800.0)), 0.0);
    let antenna = omni(DVec3::NEG_Z, Polarization::Rhcp);
    let (tx, rx) = (Endpoint { position: b, antenna }, Endpoint { position: a, antenna });
    assert_eq!(path_gain(&tx, &rx, 5800.0, &[], Some(&flat), &[], true), path_gain(&tx, &rx, 5800.0, &[], None, &[], true));
}

#[test]
fn a_ray_outside_the_footprint_loses_nothing() {
    let (a, b) = (DVec3::new(0.0, 500.0, -2.0), DVec3::new(40.0, 500.0, -2.0));
    assert_eq!(terrain_loss_db(&ridge(20, 10.0), a, b, wavelength_m(5800.0)), 0.0);
}

#[test]
fn a_thin_crest_between_samples_is_found() {
    let lambda = wavelength_m(5800.0);
    let spike = grid(|r| if r == 20 { 10.0 } else { 0.0 });
    let (a, b) = (DVec3::new(0.0, 0.5, -2.0), DVec3::new(40.0, 0.5, -2.0));
    let expected = scanned_loss(&spike, a, b, lambda, 100_000);
    assert!(expected > 20.0, "the scan sees the crest: {expected}");
    close(terrain_loss_db(&spike, a, b, lambda), expected, 0.5, "a 1 m wide tent");
}

#[test]
fn a_ray_sampled_with_the_early_out_agrees_with_without() {
    let (ridge, lambda) = (ridge(20, 10.0), wavelength_m(5800.0));
    let mut previous = f64::INFINITY;
    let mut first_clear = None;
    for k in 0..=84 {
        let z = -9.0 - f64::from(k) * 0.25;
        let (a, b) = along_north(z);
        let loss = terrain_loss_db(&ridge, a, b, lambda);
        close(loss, scanned_loss(&ridge, a, b, lambda, 20_000), 0.05, &format!("z = {z}"));
        assert!(loss <= previous, "monotone non-increasing at z = {z}: {loss} after {previous}");
        previous = loss;
        if loss == 0.0 && first_clear.is_none() {
            first_clear = Some(z);
        }
    }
    assert_eq!(previous, 0.0, "clear at -30 m");
    let z = first_clear.expect("a clear altitude");
    assert!(fresnel_v(z + 10.0, 20.0, 20.0, lambda) <= -0.78, "the first clear altitude clears the crest's first Fresnel zone");
}

#[test]
fn a_ray_starting_underground_is_finite_and_capped() {
    let (ridge, lambda) = (ridge(20, 10.0), wavelength_m(5800.0));
    let loss = terrain_loss_db(&ridge, DVec3::new(10.0, 0.0, 0.5), DVec3::new(40.0, 0.0, -2.0), lambda);
    assert!(loss.is_finite() && (0.0..=TERRAIN_RF_LOSS_DB).contains(&loss), "{loss}");
    let p = DVec3::new(20.0, 0.0, -2.0);
    assert_eq!(terrain_loss_db(&ridge, p, p, lambda), 0.0);
}

#[test]
fn a_vertical_ray_over_the_ridge_is_finite() {
    let loss = terrain_loss_db(&ridge(20, 10.0), DVec3::new(20.0, 0.0, -1.0), DVec3::new(20.0, 0.0, -30.0), wavelength_m(5800.0));
    assert!(loss.is_finite() && (0.0..=TERRAIN_RF_LOSS_DB).contains(&loss), "{loss}");
}

#[test]
fn obstruction_db_adds_terrain_to_the_objects() {
    let building = Obstacle { shape: Shape::Box { center: DVec3::new(10.0, 0.0, -10.0), half: DVec3::new(2.0, 5.0, 10.0), yaw_rad: 0.0 }.rooted(), rf_loss_db: 25.0 };
    let (ridge, lambda) = (ridge(20, 10.0), wavelength_m(5800.0));
    let (a, b) = along_north(-2.0);
    let objects = obstruction_db(&[building], None, &[], a, b, 5800.0);
    assert!(objects > 1.0, "the building is in the way: {objects}");
    let terrain = terrain_loss_db(&ridge, a, b, lambda);
    assert!(terrain > 1.0, "so is the ridge: {terrain}");
    assert_eq!(obstruction_db(&[building], Some(&ridge), &[], a, b, 5800.0), objects + terrain);
    assert_eq!(obstruction_db(&[building], None, &[], a, b, 5800.0), objects);
    assert_eq!(objects, obstruction_loss_db(&building, a, b, lambda), "the objects alone are the one building's loss");
}

#[test]
fn terrain_loss_with_a_nan_position_is_zero() {
    let (ridge, lambda) = (ridge(20, 10.0), wavelength_m(5800.0));
    let ok = DVec3::new(40.0, 0.0, -2.0);
    for bad in [DVec3::new(f64::NAN, 0.0, -2.0), DVec3::new(0.0, f64::NAN, -2.0), DVec3::new(0.0, 0.0, f64::NAN)] {
        assert_eq!(terrain_loss_db(&ridge, bad, ok, lambda), 0.0);
        assert_eq!(terrain_loss_db(&ridge, ok, bad, lambda), 0.0);
    }
}

#[test]
fn a_path_shorter_than_3_m_is_not_cleared_by_the_early_out_too_soon() {
    // Half a metre long, 5 cm under the crest: fresnel_v clamps d1 and d2 to 1 m, so v is only about -0.44.
    let (ridge, lambda) = (ridge(20, 10.0), wavelength_m(5800.0));
    let (a, b) = (DVec3::new(20.0, 0.0, -10.05), DVec3::new(20.5, 0.0, -10.05));
    let scanned = scanned_loss(&ridge, a, b, lambda, 20_000);
    assert!(scanned > 1.0, "the scan sees a loss: {scanned}");
    close(terrain_loss_db(&ridge, a, b, lambda), scanned, 0.05, "a short path just under the crest");
}

#[test]
fn a_ray_crossing_the_ridge_from_outside_the_footprint_uses_the_true_distances() {
    let lambda = wavelength_m(5800.0);
    let (a, b) = (DVec3::new(-40.0, 0.0, -2.0), DVec3::new(80.0, 0.0, -2.0));
    let expected = knife_edge_loss_db(fresnel_v(8.0, 60.0, 60.0, lambda));
    close(terrain_loss_db(&ridge(20, 10.0), a, b, lambda), expected, 0.05, "d1 = d2 = 60 m from the real ends");
}

#[test]
fn the_terrain_loss_is_capped() {
    // An end 80 m underground: v is about 500, far over the cap's 225.
    let (a, b) = (DVec3::new(0.0, 0.0, 80.0), DVec3::new(40.0, 0.0, -2.0));
    assert_eq!(terrain_loss_db(&ridge(20, 10.0), a, b, wavelength_m(5800.0)), TERRAIN_RF_LOSS_DB);
}

/// A square wall of two triangles in the plane north = `north`, spanning east and down within +-`half`, of material `material`.
fn wall(north: f64, half: f64, material: u32) -> Triangle {
    let p = |e: f64, d: f64| DVec3::new(north, e, d);
    Triangle { a: p(-half, -half), b: p(half, -half), c: p(0.0, half), material }
}

fn mesh_of(triangles: Vec<Triangle>, loss_db: Vec<f64>) -> MeshObstacle {
    MeshObstacle { mesh: Arc::new(TriMesh::new(triangles).unwrap()), loss_db }
}

#[test]
fn a_path_crossing_two_walls_loses_the_sum() {
    let m = mesh_of(vec![wall(10.0, 5.0, 0), wall(20.0, 5.0, 1)], vec![18.0, 40.0]);
    close(mesh_loss_db(&m, DVec3::new(0.0, 0.0, -1.0), DVec3::new(30.0, 0.0, -1.0)), 58.0, 1e-12, "both walls");
    close(mesh_loss_db(&m, DVec3::new(0.0, 0.0, -1.0), DVec3::new(15.0, 0.0, -1.0)), 18.0, 1e-12, "the first only");
    assert_eq!(mesh_loss_db(&m, DVec3::new(0.0, 50.0, -1.0), DVec3::new(30.0, 50.0, -1.0)), 0.0, "past the walls' edge");
    let (a, b) = (DVec3::new(0.0, 0.0, -1.0), DVec3::new(30.0, 0.0, -1.0));
    close(obstruction_db(&[], None, std::slice::from_ref(&m), a, b, 2400.0), 58.0, 1e-12, "obstruction_db carries it");
    close(obstruction_db(&[], None, &[m], a, b, 5800.0), 58.0, 1e-12, "at any frequency");
}

#[test]
fn a_material_past_the_end_of_the_table_loses_nothing() {
    let m = mesh_of(vec![wall(10.0, 5.0, 3)], vec![18.0]);
    assert_eq!(mesh_loss_db(&m, DVec3::new(0.0, 0.0, -1.0), DVec3::new(30.0, 0.0, -1.0)), 0.0);
}

#[test]
fn no_meshes_changes_nothing() {
    let building = Obstacle { shape: Shape::Box { center: DVec3::new(10.0, 0.0, -10.0), half: DVec3::new(2.0, 5.0, 10.0), yaw_rad: 0.0 }.rooted(), rf_loss_db: 25.0 };
    let (ridge, lambda) = (ridge(20, 10.0), wavelength_m(5800.0));
    let (a, b) = along_north(-2.0);
    let objects = obstruction_db(&[building], None, &[], a, b, 5800.0);
    assert_eq!(objects, obstruction_loss_db(&building, a, b, lambda));
    assert_eq!(obstruction_db(&[building], Some(&ridge), &[], a, b, 5800.0), objects + terrain_loss_db(&ridge, a, b, lambda));
}
