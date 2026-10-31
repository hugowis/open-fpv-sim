use std::sync::Arc;

use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Model, Scheduler, StepCtx};
use ofs_video::link::*;
use ofs_core::shape::Shape;
use ofs_core::terrain::Terrain;
use ofs_rf::propagation::{fspl_db, power_sum_dbm, terrain_loss_db, wavelength_m, Antenna, AntennaKind, Obstacle, Polarization};

fn close(actual: f64, expected: f64, eps: f64, what: &str) {
    assert!((actual - expected).abs() <= eps, "{what}: expected {expected} +- {eps}, got {actual}");
}

fn omni(axis: DVec3) -> Antenna {
    Antenna { kind: AntennaKind::Omni, gain_dbi: 2.0, polarization: Polarization::Rhcp, axis }
}

/// The open field: the goggles 1.7 m up at home with one upright 2 dBi omni, no objects, no emitters.
fn open_field() -> LinkWorld {
    LinkWorld {
        pilot_position: DVec3::new(0.0, 0.0, -1.7),
        antennas: vec![ReceiverAntenna { name: "omni".into(), antenna: omni(DVec3::NEG_Z) }],
        noise_floor_dbm: -93.0,
        diversity: true,
        obstacles: Vec::new(),
        emitters: Vec::new(),
        terrain: None,
    }
}

/// A VTX omni standing straight up on the quad (FRD -z is up); bounce and fading off: the bare link budget.
fn bare(world: LinkWorld) -> LinkParams {
    LinkParams {
        world,
        vtx_antenna: omni(DVec3::NEG_Z),
        pit_power_mw: 0.1,
        fading: false,
        ground_bounce: false,
        thresholds: ReceiverThresholds::default(),
    }
}

#[test]
fn the_default_thresholds_are_the_published_constants() {
    let d = ReceiverThresholds::default();
    assert_eq!(
        (d.clean_snr_db, d.sparkle_snr_db, d.sparkle_full_snr_db, d.color_fade_snr_db, d.color_lost_snr_db),
        (CLEAN_SNR_DB, SPARKLE_SNR_DB, SPARKLE_FULL_SNR_DB, COLOR_FADE_SNR_DB, COLOR_LOST_SNR_DB)
    );
    assert_eq!((d.unstable_snr_db, d.lost_snr_db), (UNSTABLE_SNR_DB, LOST_SNR_DB));
    assert_eq!((d.lost_after_fields, d.relock_after_fields), (LOST_AFTER_FIELDS, RELOCK_AFTER_FIELDS));
    assert_eq!(d.field_drop_p_max, FIELD_DROP_P_MAX);
    for step in -10..=70 {
        let snr = f64::from(step) * 0.5;
        assert_eq!(picture_with(&d, snr), picture(snr), "picture at {snr} dB");
        assert_eq!(field_drop_probability_with(&d, snr), field_drop_probability(snr), "drops at {snr} dB");
    }
}

#[test]
fn a_raised_clean_threshold_adds_grain_to_a_strong_signal() {
    let t = ReceiverThresholds { clean_snr_db: 40.0, ..ReceiverThresholds::default() };
    assert!(picture_with(&t, 30.0).noise > 0.0);
    assert_eq!(picture(30.0).noise, 0.0);
}

#[test]
fn a_raised_lost_threshold_loses_sync_on_a_weaker_signal() {
    let t = ReceiverThresholds { unstable_snr_db: 12.0, lost_snr_db: 9.0, ..ReceiverThresholds::default() };
    let mut raised = SyncTracker::with_thresholds(t);
    let mut stock = SyncTracker::new();
    let (mut a, mut b) = (VideoSync::Locked, VideoSync::Locked);
    for _ in 0..4 {
        a = raised.update(8.0);
        b = stock.update(8.0);
    }
    assert_eq!(a, VideoSync::Lost);
    assert_eq!(b, VideoSync::Locked);
}

#[test]
fn one_bad_field_loses_sync_when_lost_after_fields_is_one() {
    let t = ReceiverThresholds { lost_after_fields: 1, ..ReceiverThresholds::default() };
    let mut quick = SyncTracker::with_thresholds(t);
    let mut stock = SyncTracker::new();
    quick.update(20.0);
    stock.update(20.0);
    assert_eq!(quick.update(2.0), VideoSync::Lost);
    assert_ne!(stock.update(2.0), VideoSync::Lost, "the default needs three fields");
}

#[test]
fn a_zero_drop_probability_never_drops_a_field() {
    let t = ReceiverThresholds { field_drop_p_max: 0.0, ..ReceiverThresholds::default() };
    assert_eq!(field_drop_probability_with(&t, 4.5), 0.0);
}

fn snr_at(params: &LinkParams, power_mw: f64, north_m: f64) -> f64 {
    let path = vtx_paths(params, DVec3::new(north_m, 0.0, -1.7), DQuat::IDENTITY, 5800.0)[0];
    snr_db(10.0 * power_mw.log10() + path.gain_db, params.world.noise_floor_dbm, NO_SIGNAL_DBM)
}

#[test]
fn twenty_five_milliwatts_reach_8_db_snr_at_580_m_in_the_open() {
    let params = bare(open_field());
    close(snr_at(&params, 25.0, 580.0), 8.0, 0.1, "25 mW at 580 m");
    let reach_600 = 580.0 * 10f64.powf((10.0 * (600.0f64 / 25.0).log10()) / 20.0);
    assert!(reach_600 > 2700.0 && reach_600 < 3000.0, "600 mW reaches about 3 km: {reach_600}");
    close(snr_at(&params, 600.0, reach_600), 8.0, 0.1, "600 mW at its reach");
    close(snr_at(&params, 25.0, 58.0) - snr_at(&params, 25.0, 580.0), 20.0, 1e-6, "ten times closer: 20 dB better");
    close(fspl_db(580.0, 5800.0), 102.99, 0.01, "the path loss behind it");
}

#[test]
fn the_picture_follows_the_receiver_thresholds() {
    assert_eq!(picture(30.0), Picture { noise: 0.0, sparkles: 0.0, chroma: 1.0 }, "clean");
    let p = picture(12.0);
    close(p.noise, 0.5, 1e-12, "grain at 12 dB");
    close(p.sparkles, 0.0, 1e-12, "sparkles start at 12 dB");
    close(picture(8.0).sparkles, 0.5, 1e-12, "half way to 4 dB");
    close(picture(4.0).sparkles, 1.0, 1e-12, "full at 4 dB");
    close(picture(8.0).chroma, 1.0, 1e-12, "colour holds at 8 dB");
    close(picture(6.5).chroma, 0.5, 1e-12, "half gone at 6.5 dB");
    close(picture(5.0).chroma, 0.0, 1e-12, "gone at 5 dB");
    close(picture(0.0).noise, 1.0, 1e-12, "static at 0 dB");
    close(picture(-20.0).noise, 1.0, 1e-12, "and below");
    let mut last = picture(40.0);
    for tenth in (-100..400).rev() {
        let p = picture(f64::from(tenth) / 10.0);
        assert!(p.noise >= last.noise && p.sparkles >= last.sparkles && p.chroma <= last.chroma, "monotonic at {tenth}");
        last = p;
    }
}

#[test]
fn sync_is_lost_after_three_bad_fields_and_relocks_after_five_good_ones() {
    let mut t = SyncTracker::new();
    assert_eq!(t.update(20.0), VideoSync::Locked);
    assert_eq!(t.update(5.0), VideoSync::Unstable, "below 6 dB: tearing");
    assert_eq!(t.update(2.0), VideoSync::Unstable, "one field below 3 dB");
    assert_eq!(t.update(2.0), VideoSync::Unstable, "two");
    assert_eq!(t.update(2.0), VideoSync::Lost, "three in a row: lost");
    for i in 0..4 {
        assert_eq!(t.update(10.0), VideoSync::Lost, "good field {i}: still relocking");
    }
    assert_eq!(t.update(10.0), VideoSync::Locked, "the fifth good field relocks");
    assert_eq!(t.update(2.0), VideoSync::Unstable);
    assert_eq!(t.update(20.0), VideoSync::Locked, "a good field resets the count");
    assert_eq!(t.update(2.0), VideoSync::Unstable);
    assert_eq!(t.update(2.0), VideoSync::Unstable, "only two in a row");
}

#[test]
fn a_relock_needs_five_good_fields_in_a_row() {
    let mut t = SyncTracker::new();
    assert_eq!(t.update(-5.0), VideoSync::Lost, "the first field starts lost at once");
    for _ in 0..4 {
        t.update(10.0);
    }
    assert_eq!(t.update(4.0), VideoSync::Lost, "a weak field breaks the run");
    for _ in 0..4 {
        assert_eq!(t.update(10.0), VideoSync::Lost);
    }
    assert_eq!(t.update(5.0 + UNSTABLE_SNR_DB), VideoSync::Locked);
    assert_eq!(VideoSync::from_signal(2.0), VideoSync::Lost);
    assert_eq!(VideoSync::from_signal(1.0), VideoSync::Unstable);
    assert_eq!(VideoSync::from_signal(0.0), VideoSync::Locked);
}

fn emitter_world(offset_mhz: f64) -> LinkWorld {
    let mut world = open_field();
    world.emitters.push(Emitter {
        position: DVec3::new(0.0, 150.0, -1.0),
        freq_mhz: 5800.0 + offset_mhz,
        power_mw: 25.0,
        antenna: omni(DVec3::NEG_Z),
    });
    world
}

#[test]
fn an_emitter_interferes_less_the_further_its_channel_is() {
    let same = interference_dbm(&emitter_world(0.0), 5800.0, false)[0];
    let next = interference_dbm(&emitter_world(37.0), 5800.0, false)[0];
    let far = interference_dbm(&emitter_world(100.0), 5800.0, false)[0];
    close(same, 13.98 + 4.0 - fspl_db(150.0, 5800.0), 0.05, "same channel: its whole power");
    // 22.75 dB of rejection, plus 0.06 dB more path loss at the higher frequency.
    close(same - next, 22.75 + 20.0 * (5837.0f64 / 5800.0).log10(), 1e-3, "37 MHz away (the next Raceband channel)");
    close(same - far, 40.0 + 20.0 * (5900.0f64 / 5800.0).log10(), 1e-3, "far away");
    close(interference_dbm(&open_field(), 5800.0, false)[0], NO_SIGNAL_DBM, 1e-9, "no emitters");
    let quad_snr = |world: LinkWorld| {
        let params = bare(world);
        let path = vtx_paths(&params, DVec3::new(300.0, 0.0, -1.7), DQuat::IDENTITY, 5800.0)[0];
        snr_db(10.0 * 25f64.log10() + path.gain_db, -93.0, interference_dbm(&params.world, 5800.0, false)[0])
    };
    assert!(quad_snr(emitter_world(0.0)) < 0.0, "a same-channel emitter close to the pilot swamps a quad 300 m out");
    assert!(quad_snr(emitter_world(100.0)) > quad_snr(open_field()) - 1.0, "a far channel costs under a dB");
}

#[test]
fn a_building_between_the_quad_and_the_pilot_costs_its_loss() {
    let mut world = open_field();
    let building = Shape::Box { center: DVec3::new(100.0, 0.0, -10.0), half: DVec3::new(5.0, 5.0, 10.0) };
    world.obstacles.push(Obstacle { shape: building.rooted(), rf_loss_db: 25.0 });
    let params = bare(world);
    let open = bare(open_field());
    let at = |p: &LinkParams, east: f64| vtx_paths(p, DVec3::new(200.0, east, -1.7), DQuat::IDENTITY, 5800.0)[0];
    close(at(&open, 0.0).gain_db - at(&params, 0.0).gain_db, 25.0, 1e-6, "behind it");
    close(at(&params, 0.0).obstruction_db, 25.0, 1e-9, "reported as obstruction");
    close(at(&open, 40.0).gain_db - at(&params, 40.0).gain_db, 0.0, 1e-9, "beside it");
}

/// A 41 x 41 terrain, constant along east, with a tent-shaped ridge (10 rows a side) whose crest is `crest_m` high
/// along north row 20; with 10 m cells the crest sits at north 100 m, mid-way between the goggles and a point 200 m out.
fn ridge(crest_m: f32, cell_m: f64) -> Arc<Terrain> {
    let height = |row: usize| (crest_m * (1.0 - row.abs_diff(20) as f32 / 10.0)).max(0.0);
    let heights = (0..41 * 41).map(|k| height(k / 41)).collect();
    Arc::new(Terrain::from_grid([-10.0 * cell_m, -20.0 * cell_m], cell_m, 41, 41, heights).unwrap())
}

fn over_the_ridge(terrain: Option<Arc<Terrain>>) -> LinkWorld {
    LinkWorld { terrain, ..open_field() }
}

#[test]
fn a_ridge_between_the_quad_and_the_goggles_lowers_the_snr() {
    let terrain = ridge(10.0, 10.0);
    let (open, hidden) = (bare(open_field()), bare(over_the_ridge(Some(terrain.clone()))));
    let quad = DVec3::new(200.0, 0.0, -1.7);
    let (a, b) = (vtx_paths(&open, quad, DQuat::IDENTITY, 5800.0)[0], vtx_paths(&hidden, quad, DQuat::IDENTITY, 5800.0)[0]);
    let expected = terrain_loss_db(&terrain, quad, open.world.pilot_position, wavelength_m(5800.0));
    assert!(expected > 20.0, "the 10 m crest is a deep knife edge over a 2 m path: {expected}");
    close(a.gain_db - b.gain_db, expected, 1e-9, "the ridge's knife-edge loss");
    close(b.obstruction_db, expected, 1e-9, "reported as obstruction");
    close(snr_at(&open, 25.0, 200.0) - snr_at(&hidden, 25.0, 200.0), expected, 1e-9, "and in the SNR");
    let published = |params: LinkParams| {
        let mut rig = Rig::new(params, 1);
        rig.set_pose(quad);
        rig.field();
        rig.get(names::VIDEO_SNR)
    };
    assert!(published(hidden) < published(open) - 20.0, "the published SNR drops with it");
}

#[test]
fn a_ridge_between_an_emitter_and_the_goggles_lowers_the_interference() {
    let mut world = emitter_world(0.0);
    world.emitters[0].position = DVec3::new(200.0, 0.0, -2.0);
    let terrain = ridge(10.0, 10.0);
    let hidden = LinkWorld { terrain: Some(terrain.clone()), ..world.clone() };
    let expected = terrain_loss_db(&terrain, world.emitters[0].position, world.pilot_position, wavelength_m(5800.0));
    assert!(expected > 20.0, "{expected}");
    close(interference_dbm(&world, 5800.0, false)[0] - interference_dbm(&hidden, 5800.0, false)[0], expected, 1e-3, "the ridge hides the emitter (the -150 dBm floor adds a hair)");
}

#[test]
fn a_clear_path_over_the_ridge_is_unchanged() {
    // 60 m up the ray clears the 10 m crest by 20 m, far beyond its first Fresnel zone.
    let (open, ridged) = (bare(open_field()), bare(over_the_ridge(Some(ridge(10.0, 10.0)))));
    let quad = DVec3::new(200.0, 0.0, -60.0);
    assert_eq!(vtx_paths(&open, quad, DQuat::IDENTITY, 5800.0), vtx_paths(&ridged, quad, DQuat::IDENTITY, 5800.0));
    let mut world = emitter_world(0.0);
    world.emitters[0].position = DVec3::new(200.0, 0.0, -60.0);
    let ridged_world = LinkWorld { terrain: Some(ridge(10.0, 10.0)), ..world.clone() };
    assert_eq!(interference_dbm(&world, 5800.0, false), interference_dbm(&ridged_world, 5800.0, false));
}

struct Rig {
    bus: Bus,
    link: VideoLink,
    tick: u64,
}

impl Rig {
    fn new(params: LinkParams, seed: u64) -> Rig {
        let mut bus = Bus::new();
        let link = VideoLink::new(params, 160, seed, VtxInterferenceHandle::default(), &mut bus);
        let mut rig = Rig { bus, link, tick: 0 };
        rig.set_vtx(true, 5800.0, 25.0, false);
        rig.set_pose(DVec3::new(50.0, 0.0, -10.0));
        rig
    }

    fn set_vtx(&mut self, present: bool, freq_mhz: f64, power_mw: f64, pit: bool) {
        let b = &mut self.bus;
        let s = |b: &mut Bus, name: &str, v: f64| {
            let sig = b.signal::<f64>(name);
            b.set(sig, v);
        };
        s(b, names::VTX_PRESENT, if present { 1.0 } else { 0.0 });
        s(b, names::VTX_FREQ_MHZ, freq_mhz);
        s(b, names::VTX_POWER_MW, power_mw);
        s(b, names::VTX_PIT, if pit { 1.0 } else { 0.0 });
    }

    fn set_pose(&mut self, pos: DVec3) {
        let p = self.bus.signal::<DVec3>(names::BODY_POS_NED);
        self.bus.set(p, pos);
        let a = self.bus.signal::<DQuat>(names::BODY_ATT);
        self.bus.set(a, DQuat::IDENTITY);
    }

    fn field(&mut self) {
        let ctx = StepCtx { tick: self.tick, time_s: self.tick as f64 / 8000.0, dt_s: 0.02 };
        self.link.step(&ctx, &mut self.bus).unwrap();
        self.tick += 160;
    }

    fn get(&self, name: &str) -> f64 {
        self.bus.get(self.bus.lookup::<f64>(name).unwrap())
    }
}

fn faded(world: LinkWorld) -> LinkParams {
    LinkParams { fading: true, ground_bounce: true, ..bare(world) }
}

#[test]
fn a_hovering_quad_sees_a_steady_signal_and_a_moving_one_a_fading_one() {
    let mut rig = Rig::new(faded(open_field()), 7);
    let mut still = Vec::new();
    for _ in 0..20 {
        rig.field();
        still.push(rig.get(&names::video_rssi("omni")));
    }
    assert!(still.windows(2).all(|w| w[0] == w[1]), "standing still: {still:?}");
    let mut moving = Vec::new();
    for i in 0..50 {
        rig.set_pose(DVec3::new(100.0 + f64::from(i) * 0.4, 0.0, -10.0)); // 20 m/s
        rig.field();
        moving.push(rig.get(&names::video_rssi("omni")));
    }
    let max = moving.iter().cloned().fold(f64::MIN, f64::max);
    let min = moving.iter().cloned().fold(f64::MAX, f64::min);
    assert!(max - min > 3.0, "fast flight flickers: {min}..{max}");
}

#[test]
fn the_same_seed_gives_the_same_fades() {
    let run = |seed: u64| {
        let mut rig = Rig::new(faded(open_field()), seed);
        (0..30)
            .map(|i| {
                rig.set_pose(DVec3::new(100.0 + f64::from(i), 0.0, -10.0));
                rig.field();
                rig.get(names::VIDEO_SNR)
            })
            .collect::<Vec<f64>>()
    };
    assert_eq!(run(3), run(3));
    assert_ne!(run(3), run(4));
}

#[test]
fn the_model_publishes_the_link_and_follows_the_vtx() {
    let mut rig = Rig::new(bare(open_field()), 1);
    rig.field();
    assert_eq!(rig.get(names::VIDEO_PRESENT), 1.0);
    let full = rig.get(names::VIDEO_SNR);
    assert!(full > CLEAN_SNR_DB, "25 mW at 50 m is clean: {full}");
    assert_eq!(rig.get(names::VIDEO_SYNC), 0.0, "locked");
    assert_eq!(rig.get(names::VIDEO_NOISE), 0.0);
    assert_eq!(rig.get(names::VIDEO_CHROMA), 1.0);
    assert_eq!(rig.get(names::VIDEO_ANTENNA), 0.0);
    close(rig.get(names::VIDEO_INTERFERENCE), NO_SIGNAL_DBM, 1e-9, "no emitters");
    rig.set_vtx(true, 5800.0, 25.0, true);
    rig.field();
    close(full - rig.get(names::VIDEO_SNR), 10.0 * (25.0f64 / 0.1).log10(), 1e-6, "pit mode: 0.1 mW");
    rig.set_vtx(false, 0.0, 0.0, false);
    for _ in 0..3 {
        rig.field();
    }
    close(rig.get(&names::video_rssi("omni")), NO_SIGNAL_DBM, 1e-9, "no VTX: nothing received");
    assert_eq!(rig.get(names::VIDEO_SYNC), 2.0, "and the sync is lost");
    assert_eq!(rig.get(names::VIDEO_NOISE), 1.0, "static");
}

#[test]
fn diversity_picks_the_patch_when_the_quad_is_out_in_front() {
    let mut world = open_field();
    world.antennas.push(ReceiverAntenna {
        name: "patch".into(),
        antenna: Antenna { kind: AntennaKind::Patch { beamwidth_deg: 60.0 }, gain_dbi: 8.0, polarization: Polarization::Rhcp, axis: DVec3::X },
    });
    let mut rig = Rig::new(bare(world), 1);
    rig.set_pose(DVec3::new(300.0, 0.0, -10.0));
    rig.field();
    assert_eq!(rig.get(names::VIDEO_ANTENNA), 1.0, "in front: the 8 dBi patch");
    let gain = rig.get(&names::video_rssi("patch")) - rig.get(&names::video_rssi("omni"));
    assert!(gain > 5.0, "the patch hears it {gain} dB better");
    rig.set_pose(DVec3::new(-300.0, 0.0, -10.0));
    rig.field();
    assert_eq!(rig.get(names::VIDEO_ANTENNA), 0.0, "behind the pilot: the omni");
    let mut single = rig_without_diversity();
    single.set_pose(DVec3::new(300.0, 0.0, -10.0));
    single.field();
    assert_eq!(single.get(names::VIDEO_ANTENNA), 0.0, "no diversity: always the first antenna");
}

fn rig_without_diversity() -> Rig {
    let mut world = open_field();
    world.diversity = false;
    world.antennas.push(ReceiverAntenna { name: "patch".into(), antenna: omni(DVec3::NEG_Z) });
    Rig::new(bare(world), 1)
}

#[test]
fn the_field_drop_probability_peaks_between_unstable_and_lost() {
    close(field_drop_probability(25.0), 0.0, 1e-12, "a locked link never drops");
    close(field_drop_probability(6.0), 0.0, 1e-12, "at the unstable edge: none");
    close(field_drop_probability(4.5), FIELD_DROP_P_MAX * 0.5, 1e-12, "half way down the band");
    close(field_drop_probability(3.0), FIELD_DROP_P_MAX, 1e-12, "at the lost edge: the peak");
    close(field_drop_probability(2.0), 0.0, 1e-12, "a lost link is static, not held: no drops");
}

#[test]
fn the_same_seed_drops_the_same_fields() {
    for seed in [1u64, 7, 42] {
        let drops = run_fields(seed);
        assert_eq!(drops, run_fields(seed), "seed {seed}: the same seed, the same drops");
        assert!(drops.iter().any(|v| !*v), "seed {seed}: the band actually drops some fields");
        assert!(drops.iter().any(|v| *v), "seed {seed}: the band does not drop everything");
    }
}

/// Steps a link in place for 200 fields with the quad parked 866 m out (25 mW: about 4.5 dB — the middle of
/// the drop band, since the bare link budget gives 8 dB at 580 m and 20 dB a decade) and reports each field's
/// `video.field_valid`.
fn run_fields(seed: u64) -> Vec<bool> {
    let params = bare(open_field());
    let mut bus = Bus::new();
    let pos = bus.signal::<DVec3>(names::BODY_POS_NED);
    bus.set(pos, DVec3::new(866.0, 0.0, -1.7));
    let att = bus.signal::<DQuat>(names::BODY_ATT);
    bus.set(att, DQuat::IDENTITY);
    let mut link = VideoLink::new(params, 1, seed, VtxInterferenceHandle::default(), &mut bus);
    let vtx_present = bus.signal::<f64>(names::VTX_PRESENT);
    let vtx_freq = bus.signal::<f64>(names::VTX_FREQ_MHZ);
    let vtx_power = bus.signal::<f64>(names::VTX_POWER_MW);
    bus.set(vtx_present, 1.0);
    bus.set(vtx_freq, 5800.0);
    bus.set(vtx_power, 25.0);
    let field_valid = bus.signal::<f64>(names::VIDEO_FIELD_VALID);
    (0..200)
        .map(|i| {
            link.step(&StepCtx { tick: i, time_s: i as f64 * 0.02, dt_s: 0.02 }, &mut bus).unwrap();
            bus.get(field_valid) > 0.5
        })
        .collect()
}

#[test]
fn odd_positions_give_finite_values() {
    // At the goggles (zero distance), inside a building, below the ground plane (a crash can dip there): the
    // simulator's per-step check stops on any non-finite signal, so the link must never produce one.
    let mut world = open_field();
    let building = Shape::Box { center: DVec3::new(100.0, 0.0, -10.0), half: DVec3::new(5.0, 5.0, 10.0) };
    world.obstacles.push(Obstacle { shape: building.rooted(), rf_loss_db: 25.0 });
    let mut rig = Rig::new(faded(world), 1);
    for pos in [DVec3::new(0.0, 0.0, -1.7), DVec3::new(100.0, 0.0, -5.0), DVec3::new(10.0, 0.0, 0.5), DVec3::new(1e5, 0.0, -10.0)] {
        rig.set_pose(pos);
        rig.field();
        for name in [names::VIDEO_SNR, names::VIDEO_NOISE, names::VIDEO_SPARKLES, names::VIDEO_CHROMA, names::VIDEO_INTERFERENCE] {
            assert!(rig.get(name).is_finite(), "{name} at {pos}: {}", rig.get(name));
        }
        assert!(rig.get(&names::video_rssi("omni")).is_finite(), "rssi at {pos}");
    }
}

fn emitter() -> Emitter {
    Emitter {
        position: DVec3::new(5.0, 0.0, -1.7),
        freq_mhz: 5700.0,
        power_mw: 500.0,
        antenna: omni(DVec3::NEG_Z),
    }
}

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

/// Builds the link over the bare open field (the clean-picture rig: the quad parked 50 m north, transmitting
/// 25 mW on 5800), runs 0.2 s (ten fields), and returns (snr_db, interference_dbm) as published.
fn run_link(emitter: Option<Emitter>) -> (f64, f64) {
    let params = bare(open_field());
    let handle = VtxInterferenceHandle::default();
    handle.set(emitter);
    let mut bus = Bus::new();
    let pos = bus.signal::<DVec3>(names::BODY_POS_NED);
    bus.set(pos, DVec3::new(50.0, 0.0, -10.0));
    let att = bus.signal::<DQuat>(names::BODY_ATT);
    bus.set(att, DQuat::IDENTITY);
    for (name, value) in [(names::VTX_PRESENT, 1.0), (names::VTX_FREQ_MHZ, 5800.0), (names::VTX_POWER_MW, 25.0)] {
        let sig = bus.signal::<f64>(name);
        bus.set(sig, value);
    }
    let link = VideoLink::new(params, 160, 7, handle, &mut bus);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(link));
    s.run_for(0.2).unwrap();
    (get(&s, names::VIDEO_SNR), get(&s, names::VIDEO_INTERFERENCE))
}

#[test]
fn the_injected_emitter_degrades_the_picture() {
    let noisy = run_link(Some(emitter()));
    let clean = run_link(None);
    assert!(noisy.1 > clean.1 + 3.0, "interference {} -> {}", clean.1, noisy.1);
    // The receiver sums the interference with its noise floor in the power domain, so the SNR falls by
    // exactly that sum's rise. The clean run publishes NO_SIGNAL_DBM (no emitters at all), which sits under
    // the floor, so the rise is measured floor to interference, not clean-published to noisy-published.
    let floor = bare(open_field()).world.noise_floor_dbm;
    let rise = power_sum_dbm([floor, noisy.1]) - power_sum_dbm([floor, clean.1]);
    assert!(
        (clean.0 - noisy.0 - rise).abs() < 1e-6,
        "SNR fell {} dB, the floor-plus-interference sum rose {} dB",
        clean.0 - noisy.0,
        rise
    );
}
