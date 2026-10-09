use glam::DVec3;
use ofs_video::propagation::*;

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
    let b = Shape::Box { center: DVec3::ZERO, half: DVec3::new(2.0, 1.0, 1.0) };
    close(b.signed_distance(DVec3::new(5.0, 0.0, 0.0)), 3.0, 1e-12, "outside along north");
    close(b.signed_distance(DVec3::ZERO), -1.0, 1e-12, "the centre is 1 m from the nearest face");
    let c = Shape::Cylinder { center: DVec3::new(0.0, 0.0, -5.0), radius: 1.0, half_height: 5.0 };
    close(c.signed_distance(DVec3::new(3.0, 0.0, -5.0)), 2.0, 1e-12, "beside it");
    close(c.signed_distance(DVec3::new(0.0, 0.0, -12.0)), 2.0, 1e-12, "above it");
    close(c.signed_distance(DVec3::new(0.0, 0.0, -5.0)), -1.0, 1e-12, "on its axis");
}

#[test]
fn a_shape_standing_on_the_ground_is_rooted_below_it_and_a_floating_one_is_not() {
    let wall = Shape::Box { center: DVec3::new(0.0, 0.0, -10.0), half: DVec3::new(5.0, 5.0, 10.0) };
    let p = DVec3::new(0.0, 0.0, -1.0); // inside the wall, 1 m above the ground and 5 m from its sides
    close(wall.signed_distance(p), -1.0, 1e-12, "unrooted: the bottom face is nearest");
    close(wall.rooted().signed_distance(p), -5.0, 1e-9, "rooted: the side faces are");
    close(wall.rooted().signed_distance(DVec3::new(0.0, 0.0, -25.0)), 5.0, 1e-9, "the top stays where it was");
    let bar = Shape::Box { center: DVec3::new(0.0, 0.0, -2.5), half: DVec3::new(0.1, 1.6, 0.06) };
    assert_eq!(bar.rooted(), bar, "a gate's top bar floats: unchanged");
    let pylon = Shape::Cylinder { center: DVec3::new(0.0, 0.0, -1.5), radius: 0.15, half_height: 1.5 };
    close(pylon.rooted().signed_distance(DVec3::new(0.0, 0.0, -4.0)), 1.0, 1e-9, "a rooted cylinder keeps its top");
    close(pylon.rooted().signed_distance(DVec3::new(0.0, 0.0, -0.5)), -0.15, 1e-9, "and its radius");
}

#[test]
fn a_building_on_the_path_costs_its_loss_and_a_clear_one_costs_nothing() {
    let building = Obstacle { shape: Shape::Box { center: DVec3::new(50.0, 0.0, -10.0), half: DVec3::new(5.0, 5.0, 10.0) }.rooted(), rf_loss_db: 25.0 };
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
    let building = Obstacle { shape: Shape::Box { center: DVec3::new(100.0, 0.0, -10.0), half: DVec3::new(5.0, 5.0, 10.0) }.rooted(), rf_loss_db: 30.0 };
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
    let post = Obstacle { shape: Shape::Box { center: DVec3::new(2000.0, 0.0, -5.0), half: DVec3::new(0.25, 0.25, 5.0) }.rooted(), rf_loss_db: 10.0 };
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
            let with = path_gain(&tx, &rx, freq, &[], true).gain_db;
            let without = path_gain(&tx, &rx, freq, &[], false).gain_db;
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
    let p = path_gain(&tx, &rx, 5800.0, &[], false);
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
