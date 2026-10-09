//! The analog video link, from the quad's VTX to the pilot's goggles, one PAL field (50 Hz) at a time: received
//! power at each goggle antenna, interference from other emitters, diversity, then what the receiver makes of the
//! signal (grain, sparkles, colour, sync). Deterministic: the fading draws from the model's own seeded stream.
//!
//! Every threshold below is an estimate for a typical analog 5.8 GHz receiver; docs/research/video-link.md lists
//! them with their provenance.
use glam::{DQuat, DVec3};
use ofs_core::rng::model_rng;
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, StandardNormal};

use crate::propagation::{
    adjacent_channel_rejection_db, mw_to_dbm, path_gain, power_sum_dbm, wavelength_m, Antenna, Endpoint, Obstacle,
    PathGain,
};

pub const MODEL_NAME: &str = "video.link";
/// One PAL field.
pub const FIELD_RATE_HZ: u32 = 50;
/// Received power reported when nothing arrives (no transmitter, or no other emitters).
pub const NO_SIGNAL_DBM: f64 = -150.0;
/// The most the frame, battery and stack take from the VTX signal when they sit between its antenna and the pilot.
pub const BODY_SHADOW_DB: f64 = 8.0;
/// The body direction (FRD, unit) the frame shadows most: forward and down, through the stack and the battery.
const SHADOW_DIRECTION: DVec3 = DVec3::new(0.6, 0.0, 0.8);
/// The receiver changes antenna only when another one is better by this much.
pub const DIVERSITY_HYSTERESIS_DB: f64 = 2.0;
/// Rician K-factor with a clear line of sight; obstruction lowers it dB for dB (towards Rayleigh fading).
pub const LOS_K_DB: f64 = 10.0;
/// Lowest K-factor (deep behind an obstacle: practically Rayleigh).
pub const MIN_K_DB: f64 = -20.0;

/// At or above this SNR the picture is clean.
pub const CLEAN_SNR_DB: f64 = 25.0;
/// Grain reaches half of full static here, where sparkles start.
pub const SPARKLE_SNR_DB: f64 = 12.0;
/// Sparkles cover the picture at or below this SNR.
pub const SPARKLE_FULL_SNR_DB: f64 = 4.0;
/// Colour starts to fade below this SNR and is gone at `COLOR_LOST_SNR_DB`.
pub const COLOR_FADE_SNR_DB: f64 = 8.0;
pub const COLOR_LOST_SNR_DB: f64 = 5.0;
/// Below this SNR sync is unstable (tearing); a lost sync relocks only above it.
pub const UNSTABLE_SNR_DB: f64 = 6.0;
/// Below this SNR for `LOST_AFTER_FIELDS` fields in a row, sync is lost.
pub const LOST_SNR_DB: f64 = 3.0;
pub const LOST_AFTER_FIELDS: u32 = 3;
/// Fields in a row at or above `UNSTABLE_SNR_DB` that a lost sync needs to relock.
pub const RELOCK_AFTER_FIELDS: u32 = 5;

/// The receiver's hold on the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoSync {
    Locked = 0,
    /// Tearing and line jitter.
    Unstable = 1,
    /// Rolling, then static.
    Lost = 2,
}

impl VideoSync {
    pub fn from_signal(value: f64) -> VideoSync {
        match value.round() as i64 {
            0 => VideoSync::Locked,
            1 => VideoSync::Unstable,
            _ => VideoSync::Lost,
        }
    }
}

/// What the picture looks like, each value 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Picture {
    /// Grain; 1 is full static.
    pub noise: f64,
    pub sparkles: f64,
    /// Colour saturation; 0 is black and white.
    pub chroma: f64,
}

/// How far `x` has gone from `from` towards `to` (`from > to`), clamped to 0..=1.
fn ramp(x: f64, from: f64, to: f64) -> f64 {
    ((from - x) / (from - to)).clamp(0.0, 1.0)
}

/// The picture at a given SNR: grain rises from 25 dB (0.5 at 12 dB, full static at 0 dB), sparkles from 12 dB
/// (full at 4 dB), colour fades from 8 dB (gone at 5 dB).
pub fn picture(snr_db: f64) -> Picture {
    let noise = 0.5 * ramp(snr_db, CLEAN_SNR_DB, SPARKLE_SNR_DB) + 0.5 * ramp(snr_db, SPARKLE_SNR_DB, 0.0);
    Picture {
        noise,
        sparkles: ramp(snr_db, SPARKLE_SNR_DB, SPARKLE_FULL_SNR_DB),
        chroma: 1.0 - ramp(snr_db, COLOR_FADE_SNR_DB, COLOR_LOST_SNR_DB),
    }
}

/// The receiver's sync, field by field: unstable below 6 dB, lost after 3 fields below 3 dB, relocked after 5 fields
/// at 6 dB or more. The first field decides the starting state without delay.
#[derive(Debug, Clone, Default)]
pub struct SyncTracker {
    state: Option<VideoSync>,
    low_fields: u32,
    good_fields: u32,
}

impl SyncTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, snr_db: f64) -> VideoSync {
        let live = if snr_db < UNSTABLE_SNR_DB { VideoSync::Unstable } else { VideoSync::Locked };
        let next = match self.state {
            None if snr_db < LOST_SNR_DB => VideoSync::Lost,
            None => live,
            Some(VideoSync::Lost) => {
                self.good_fields = if snr_db >= UNSTABLE_SNR_DB { self.good_fields + 1 } else { 0 };
                if self.good_fields >= RELOCK_AFTER_FIELDS { live } else { VideoSync::Lost }
            }
            Some(_) => {
                self.low_fields = if snr_db < LOST_SNR_DB { self.low_fields + 1 } else { 0 };
                if self.low_fields >= LOST_AFTER_FIELDS { VideoSync::Lost } else { live }
            }
        };
        if (next == VideoSync::Lost) != (self.state == Some(VideoSync::Lost)) {
            self.low_fields = 0;
            self.good_fields = 0;
        }
        self.state = Some(next);
        next
    }
}

/// Diversity: the antenna with the best SNR, changed only for a 2 dB better one.
#[derive(Debug, Clone, Default)]
pub struct Diversity {
    active: usize,
}

impl Diversity {
    /// The antenna to use, given each antenna's SNR (at least one).
    pub fn choose(&mut self, snr_db: &[f64]) -> usize {
        self.active = self.active.min(snr_db.len().saturating_sub(1));
        let best = (0..snr_db.len()).max_by(|a, b| snr_db[*a].total_cmp(&snr_db[*b])).unwrap_or(0);
        if snr_db[best] > snr_db[self.active] + DIVERSITY_HYSTERESIS_DB {
            self.active = best;
        }
        self.active
    }
}

/// `SNR = signal - (noise floor + interference)`, powers added in mW.
pub fn snr_db(signal_dbm: f64, noise_floor_dbm: f64, interference_dbm: f64) -> f64 {
    signal_dbm - power_sum_dbm([noise_floor_dbm, interference_dbm])
}

/// How much of the frame, battery and stack lies between the VTX antenna and the receiver, as a loss in dB:
/// up to `BODY_SHADOW_DB` when the receiver is forward and below the quad, nothing when it is behind or above.
pub fn body_shadow_db(att: DQuat, quad_pos: DVec3, receiver_pos: DVec3) -> f64 {
    let Some(dir_world) = (receiver_pos - quad_pos).try_normalize() else { return 0.0 };
    let dir_body = att.inverse() * dir_world;
    let x = (dir_body.dot(SHADOW_DIRECTION.normalize()) / 0.8).clamp(0.0, 1.0);
    BODY_SHADOW_DB * x * x * (3.0 - 2.0 * x)
}

/// A goggle antenna, placed in the world (its axis in NED).
#[derive(Debug, Clone, PartialEq)]
pub struct ReceiverAntenna {
    pub name: String,
    pub antenna: Antenna,
}

/// Another transmitter on the field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Emitter {
    pub position: DVec3,
    pub freq_mhz: f64,
    pub power_mw: f64,
    pub antenna: Antenna,
}

/// Everything the link needs to know about the world.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkWorld {
    /// Where the goggles are.
    pub pilot_position: DVec3,
    pub antennas: Vec<ReceiverAntenna>,
    pub noise_floor_dbm: f64,
    pub diversity: bool,
    pub obstacles: Vec<Obstacle>,
    pub emitters: Vec<Emitter>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinkParams {
    pub world: LinkWorld,
    /// The VTX antenna on the quad: its axis in the body frame (FRD).
    pub vtx_antenna: Antenna,
    /// Output power in pit mode.
    pub pit_power_mw: f64,
    /// Fading and the ground bounce; tests that check the bare link budget turn them off.
    pub fading: bool,
    pub ground_bounce: bool,
}

/// The VTX signal's path to each receiver antenna, without fading: body shadow included.
pub fn vtx_paths(params: &LinkParams, quad_pos: DVec3, att: DQuat, freq_mhz: f64) -> Vec<PathGain> {
    let tx = Endpoint { position: quad_pos, antenna: Antenna { axis: (att * params.vtx_antenna.axis).normalize(), ..params.vtx_antenna } };
    let shadow = body_shadow_db(att, quad_pos, params.world.pilot_position);
    params
        .world
        .antennas
        .iter()
        .map(|a| {
            let rx = Endpoint { position: params.world.pilot_position, antenna: a.antenna };
            let p = path_gain(&tx, &rx, freq_mhz, &params.world.obstacles, params.ground_bounce);
            PathGain { gain_db: p.gain_db - shadow, obstruction_db: p.obstruction_db }
        })
        .collect()
}

/// Interference at each receiver antenna for a receiver tuned to `freq_mhz`, in dBm.
pub fn interference_dbm(world: &LinkWorld, freq_mhz: f64, ground_bounce: bool) -> Vec<f64> {
    world
        .antennas
        .iter()
        .map(|a| {
            let rx = Endpoint { position: world.pilot_position, antenna: a.antenna };
            let powers = world.emitters.iter().map(|e| {
                let tx = Endpoint { position: e.position, antenna: e.antenna };
                mw_to_dbm(e.power_mw) + path_gain(&tx, &rx, e.freq_mhz, &world.obstacles, ground_bounce).gain_db
                    - adjacent_channel_rejection_db(e.freq_mhz - freq_mhz)
            });
            power_sum_dbm(powers.chain([NO_SIGNAL_DBM]))
        })
        .collect()
}

/// Rician fading of one antenna: an AR(1) complex Gaussian scatter (unit mean power) added to the line of sight.
#[derive(Debug, Clone, Copy, Default)]
struct Fader {
    re: f64,
    im: f64,
}

impl Fader {
    /// The fade in dB for this field. `rho` is the correlation with the previous field (1 = the quad stood still).
    fn next(&mut self, rng: &mut ChaCha8Rng, rho: f64, k_db: f64) -> f64 {
        let fresh = ((1.0 - rho * rho).max(0.0) * 0.5).sqrt();
        let (n1, n2): (f64, f64) = (StandardNormal.sample(rng), StandardNormal.sample(rng));
        self.re = rho * self.re + fresh * n1;
        self.im = rho * self.im + fresh * n2;
        let k = 10f64.powf(k_db.max(MIN_K_DB) / 10.0);
        let (los, scatter) = ((k / (k + 1.0)).sqrt(), (1.0 / (k + 1.0)).sqrt());
        let (re, im) = (los + scatter * self.re, scatter * self.im);
        10.0 * (re * re + im * im).max(1e-6).log10()
    }
}

struct Outputs {
    present: Signal<f64>,
    snr: Signal<f64>,
    rssi: Vec<Signal<f64>>,
    antenna: Signal<f64>,
    interference: Signal<f64>,
    noise: Signal<f64>,
    sparkles: Signal<f64>,
    chroma: Signal<f64>,
    sync: Signal<f64>,
}

pub struct VideoLink {
    params: LinkParams,
    divisor: u32,
    rng: ChaCha8Rng,
    faders: Vec<Fader>,
    last_pos: Option<DVec3>,
    tracker: SyncTracker,
    diversity: Diversity,
    /// Interference per antenna for the frequency it was computed for (the emitters and the goggles do not move).
    interference: Option<(f64, Vec<f64>)>,
    pos: Signal<DVec3>,
    att: Signal<DQuat>,
    vtx_present: Signal<f64>,
    vtx_freq: Signal<f64>,
    vtx_power: Signal<f64>,
    vtx_pit: Signal<f64>,
    out: Outputs,
}

impl VideoLink {
    /// `divisor` is `base_hz / FIELD_RATE_HZ`.
    pub fn new(params: LinkParams, divisor: u32, seed: u64, bus: &mut Bus) -> Self {
        assert!(!params.world.antennas.is_empty(), "the receiver needs at least one antenna");
        let out = Outputs {
            present: bus.signal(names::VIDEO_PRESENT),
            snr: bus.signal(names::VIDEO_SNR),
            rssi: params.world.antennas.iter().map(|a| bus.signal(&names::video_rssi(&a.name))).collect(),
            antenna: bus.signal(names::VIDEO_ANTENNA),
            interference: bus.signal(names::VIDEO_INTERFERENCE),
            noise: bus.signal(names::VIDEO_NOISE),
            sparkles: bus.signal(names::VIDEO_SPARKLES),
            chroma: bus.signal(names::VIDEO_CHROMA),
            sync: bus.signal(names::VIDEO_SYNC),
        };
        let mut rng = model_rng(seed, MODEL_NAME);
        let faders = params
            .world
            .antennas
            .iter()
            .map(|_| {
                let mut f = Fader::default();
                f.next(&mut rng, 0.0, LOS_K_DB); // a fresh scatter state for the first field
                f
            })
            .collect();
        Self {
            divisor,
            rng,
            faders,
            last_pos: None,
            tracker: SyncTracker::new(),
            diversity: Diversity::default(),
            interference: None,
            pos: bus.signal(names::BODY_POS_NED),
            att: bus.signal(names::BODY_ATT),
            vtx_present: bus.signal(names::VTX_PRESENT),
            vtx_freq: bus.signal(names::VTX_FREQ_MHZ),
            vtx_power: bus.signal(names::VTX_POWER_MW),
            vtx_pit: bus.signal(names::VTX_PIT),
            out,
            params,
        }
    }

    fn interference_for(&mut self, freq_mhz: f64) -> Vec<f64> {
        match &self.interference {
            Some((f, values)) if *f == freq_mhz => values.clone(),
            _ => {
                let values = interference_dbm(&self.params.world, freq_mhz, self.params.ground_bounce);
                self.interference = Some((freq_mhz, values.clone()));
                values
            }
        }
    }
}

impl Model for VideoLink {
    fn name(&self) -> &str {
        MODEL_NAME
    }

    fn rate_divisor(&self) -> u32 {
        self.divisor
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let pos = bus.get(self.pos);
        let att = bus.get(self.att);
        let freq_mhz = bus.get(self.vtx_freq);
        let power_mw = if bus.get(self.vtx_pit) > 0.5 { self.params.pit_power_mw } else { bus.get(self.vtx_power) };
        let transmitting = bus.get(self.vtx_present) > 0.5 && power_mw > 0.0 && freq_mhz > 0.0;
        let att = if att.length_squared() > 0.0 { att.normalize() } else { DQuat::IDENTITY };
        let moved = self.last_pos.map_or(0.0, |p| p.distance(pos));
        self.last_pos = Some(pos);
        let interference = if freq_mhz > 0.0 { self.interference_for(freq_mhz) } else { vec![NO_SIGNAL_DBM; self.faders.len()] };
        let mut rssi = vec![NO_SIGNAL_DBM; self.faders.len()];
        if transmitting {
            let rho = (-moved / (wavelength_m(freq_mhz) * 0.5)).exp();
            for (i, path) in vtx_paths(&self.params, pos, att, freq_mhz).into_iter().enumerate() {
                let fade = if self.params.fading {
                    self.faders[i].next(&mut self.rng, rho, LOS_K_DB - path.obstruction_db)
                } else {
                    0.0
                };
                rssi[i] = (mw_to_dbm(power_mw) + path.gain_db + fade).max(NO_SIGNAL_DBM);
            }
        }
        let snr: Vec<f64> = rssi
            .iter()
            .zip(&interference)
            .map(|(s, i)| snr_db(*s, self.params.world.noise_floor_dbm, *i))
            .collect();
        let active = if self.params.world.diversity { self.diversity.choose(&snr) } else { 0 };
        let sync = self.tracker.update(snr[active]);
        let picture = picture(snr[active]);
        let o = &self.out;
        bus.set(o.present, 1.0);
        bus.set(o.snr, snr[active]);
        for (signal, value) in o.rssi.iter().zip(&rssi) {
            bus.set(*signal, *value);
        }
        bus.set(o.antenna, active as f64);
        bus.set(o.interference, interference[active]);
        bus.set(o.noise, picture.noise);
        bus.set(o.sparkles, picture.sparkles);
        bus.set(o.chroma, picture.chroma);
        bus.set(o.sync, sync as i32 as f64);
        Ok(())
    }
}
