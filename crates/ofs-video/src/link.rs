//! The analog video link, from the quad's VTX to the pilot's goggles, one PAL field (50 Hz) at a time: received
//! power at each goggle antenna, interference from other emitters, diversity, then what the receiver makes of the
//! signal (grain, sparkles, colour, sync). Deterministic: the fading draws from the model's own seeded stream.
//!
//! Every threshold below is an estimate for a typical analog 5.8 GHz receiver; docs/research/video-link.md lists
//! them with their provenance.
use glam::{DQuat, DVec3};
use ofs_core::rng::model_rng;
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};
use ofs_rf::fading::{body_shadow_db, Diversity, Fader, LOS_K_DB};
use ofs_rf::propagation::{
    adjacent_channel_rejection_db, mw_to_dbm, obstruction_db, power_sum_dbm, unobstructed_gain_db, wavelength_m, Antenna,
    Endpoint, Obstacle, PathGain,
};
use rand_chacha::ChaCha8Rng;

pub const MODEL_NAME: &str = "video.link";
/// One PAL field.
pub use ofs_core::consts::VIDEO_FIELD_RATE_HZ as FIELD_RATE_HZ;
/// Received power reported when nothing arrives (no transmitter, or no other emitters).
pub const NO_SIGNAL_DBM: f64 = -150.0;

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

/// The chance the receiver fails to complete a field at the lost edge of the marginal band; the probability
/// ramps linearly from 0 at `UNSTABLE_SNR_DB` to this at `LOST_SNR_DB`, and is 0 outside the band (a locked
/// link never drops, a lost link outputs static — drops there would mean nothing). An estimate; see
/// docs/research/video-link.md.
pub const FIELD_DROP_P_MAX: f64 = 0.5;

/// The receiver's thresholds, as a value (the world file's `[video]` table sets them). `Default` is the constants
/// above, which are what a world without a `[video]` table gets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReceiverThresholds {
    pub clean_snr_db: f64,
    pub sparkle_snr_db: f64,
    pub sparkle_full_snr_db: f64,
    pub color_fade_snr_db: f64,
    pub color_lost_snr_db: f64,
    pub unstable_snr_db: f64,
    pub lost_snr_db: f64,
    pub lost_after_fields: u32,
    pub relock_after_fields: u32,
    pub field_drop_p_max: f64,
}

impl Default for ReceiverThresholds {
    fn default() -> Self {
        Self {
            clean_snr_db: CLEAN_SNR_DB,
            sparkle_snr_db: SPARKLE_SNR_DB,
            sparkle_full_snr_db: SPARKLE_FULL_SNR_DB,
            color_fade_snr_db: COLOR_FADE_SNR_DB,
            color_lost_snr_db: COLOR_LOST_SNR_DB,
            unstable_snr_db: UNSTABLE_SNR_DB,
            lost_snr_db: LOST_SNR_DB,
            lost_after_fields: LOST_AFTER_FIELDS,
            relock_after_fields: RELOCK_AFTER_FIELDS,
            field_drop_p_max: FIELD_DROP_P_MAX,
        }
    }
}

/// The per-field drop probability at `snr_db` (M3e spec §2).
pub fn field_drop_probability(snr_db: f64) -> f64 {
    field_drop_probability_with(&ReceiverThresholds::default(), snr_db)
}

/// `field_drop_probability` with the given thresholds.
pub fn field_drop_probability_with(t: &ReceiverThresholds, snr_db: f64) -> f64 {
    if snr_db >= t.unstable_snr_db || snr_db < t.lost_snr_db {
        return 0.0;
    }
    t.field_drop_p_max * ramp(snr_db, t.unstable_snr_db, t.lost_snr_db)
}

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
    picture_with(&ReceiverThresholds::default(), snr_db)
}

/// `picture` with the given thresholds.
pub fn picture_with(t: &ReceiverThresholds, snr_db: f64) -> Picture {
    let noise = 0.5 * ramp(snr_db, t.clean_snr_db, t.sparkle_snr_db) + 0.5 * ramp(snr_db, t.sparkle_snr_db, 0.0);
    Picture {
        noise,
        sparkles: ramp(snr_db, t.sparkle_snr_db, t.sparkle_full_snr_db),
        chroma: 1.0 - ramp(snr_db, t.color_fade_snr_db, t.color_lost_snr_db),
    }
}

/// The receiver's sync, field by field: unstable below 6 dB, lost after 3 fields below 3 dB, relocked after 5 fields
/// at 6 dB or more. The first field decides the starting state without delay.
#[derive(Debug, Clone, Default)]
pub struct SyncTracker {
    state: Option<VideoSync>,
    low_fields: u32,
    good_fields: u32,
    thresholds: ReceiverThresholds,
}

impl SyncTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_thresholds(thresholds: ReceiverThresholds) -> Self {
        Self { thresholds, ..Self::default() }
    }

    pub fn update(&mut self, snr_db: f64) -> VideoSync {
        let t = self.thresholds;
        let live = if snr_db < t.unstable_snr_db { VideoSync::Unstable } else { VideoSync::Locked };
        let next = match self.state {
            None if snr_db < t.lost_snr_db => VideoSync::Lost,
            None => live,
            Some(VideoSync::Lost) => {
                self.good_fields = if snr_db >= t.unstable_snr_db { self.good_fields + 1 } else { 0 };
                if self.good_fields >= t.relock_after_fields { live } else { VideoSync::Lost }
            }
            Some(_) => {
                self.low_fields = if snr_db < t.lost_snr_db { self.low_fields + 1 } else { 0 };
                if self.low_fields >= t.lost_after_fields { VideoSync::Lost } else { live }
            }
        };
        if (next == VideoSync::Lost) != (self.state == Some(VideoSync::Lost)) {
            self.low_fields = 0;
            self.good_fields = 0;
        }
        self.state = Some(next);
        next
    }

    /// The current sync state without updating it (the drop decision reads it).
    pub fn state(&self) -> Option<VideoSync> {
        self.state
    }
}

/// `SNR = signal - (noise floor + interference)`, powers added in mW.
pub fn snr_db(signal_dbm: f64, noise_floor_dbm: f64, interference_dbm: f64) -> f64 {
    signal_dbm - power_sum_dbm([noise_floor_dbm, interference_dbm])
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

/// A runtime emitter injected by the VtxInterference fault (the OsdHandle pattern: shared state the
/// vehicle writes and the model reads). Default = no injected emitter.
#[derive(Clone, Default)]
pub struct VtxInterferenceHandle(std::sync::Arc<std::sync::Mutex<Option<Emitter>>>);

impl VtxInterferenceHandle {
    pub fn set(&self, emitter: Option<Emitter>) {
        *self.0.lock().expect("vtx interference handle poisoned") = emitter;
    }
    fn emitter(&self) -> Option<Emitter> {
        *self.0.lock().expect("vtx interference handle poisoned")
    }
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
    /// What the receiver makes of the SNR (the world file's `[video]` table).
    pub thresholds: ReceiverThresholds,
}

/// The VTX signal's path to each receiver antenna, without fading: body shadow included.
pub fn vtx_paths(params: &LinkParams, quad_pos: DVec3, att: DQuat, freq_mhz: f64) -> Vec<PathGain> {
    let tx = Endpoint { position: quad_pos, antenna: Antenna { axis: (att * params.vtx_antenna.axis).normalize(), ..params.vtx_antenna } };
    let shadow = body_shadow_db(att, quad_pos, params.world.pilot_position);
    // Every goggle antenna is at the pilot: one obstruction for all of them.
    let obstruction = obstruction_db(&params.world.obstacles, quad_pos, params.world.pilot_position, freq_mhz);
    params
        .world
        .antennas
        .iter()
        .map(|a| {
            let rx = Endpoint { position: params.world.pilot_position, antenna: a.antenna };
            let gain_db = unobstructed_gain_db(&tx, &rx, freq_mhz, params.ground_bounce) - obstruction - shadow;
            PathGain { gain_db, obstruction_db: obstruction }
        })
        .collect()
}

/// Interference at each receiver antenna for a receiver tuned to `freq_mhz`, in dBm.
pub fn interference_dbm(world: &LinkWorld, freq_mhz: f64, ground_bounce: bool) -> Vec<f64> {
    let obstructions: Vec<f64> =
        world.emitters.iter().map(|e| obstruction_db(&world.obstacles, e.position, world.pilot_position, e.freq_mhz)).collect();
    world
        .antennas
        .iter()
        .map(|a| {
            let rx = Endpoint { position: world.pilot_position, antenna: a.antenna };
            let powers = world.emitters.iter().zip(&obstructions).map(|(e, obstruction)| {
                let tx = Endpoint { position: e.position, antenna: e.antenna };
                mw_to_dbm(e.power_mw) + unobstructed_gain_db(&tx, &rx, e.freq_mhz, ground_bounce) - obstruction
                    - adjacent_channel_rejection_db(e.freq_mhz - freq_mhz)
            });
            power_sum_dbm(powers.chain([NO_SIGNAL_DBM]))
        })
        .collect()
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
    field_valid: Signal<f64>,
}

pub struct VideoLink {
    params: LinkParams,
    handle: VtxInterferenceHandle,
    divisor: u32,
    rng: ChaCha8Rng,
    faders: Vec<Fader>,
    last_pos: Option<DVec3>,
    tracker: SyncTracker,
    diversity: Diversity,
    /// Interference per antenna for the frequency it was computed for (the emitters and the goggles do not
    /// move), keyed on the injected emitter too: a runtime injection or clear must take effect on its next
    /// field even though the frequency is unchanged.
    interference: Option<(f64, Option<Emitter>, Vec<f64>)>,
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
    pub fn new(params: LinkParams, divisor: u32, seed: u64, handle: VtxInterferenceHandle, bus: &mut Bus) -> Self {
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
            field_valid: bus.signal(names::VIDEO_FIELD_VALID),
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
            tracker: SyncTracker::with_thresholds(params.thresholds),
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
            handle,
        }
    }

    fn interference_for(&mut self, freq_mhz: f64) -> Vec<f64> {
        let injected = self.handle.emitter();
        // Exact equality on purpose: the cache holds only for the very same frequency and injected emitter, and any
        // difference recomputes. Fault validation rejects non-finite payloads at inject, so NaN never reaches the key.
        match &self.interference {
            Some((f, e, values)) if *f == freq_mhz && *e == injected => values.clone(),
            _ => {
                let mut emitters = self.params.world.emitters.clone();
                if let Some(e) = injected {
                    emitters.push(e);
                }
                let values = interference_dbm(&LinkWorld { emitters, ..self.params.world.clone() }, freq_mhz, self.params.ground_bounce);
                self.interference = Some((freq_mhz, injected, values.clone()));
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
        // At a marginal SNR the receiver may fail to complete the field: one seeded draw per field, after the
        // fading draws. A lost sync outputs static rather than held fields, so no drops are drawn there.
        let mut field_valid = transmitting;
        if transmitting
            && self.tracker.state() != Some(VideoSync::Lost)
            && rand::Rng::gen::<f64>(&mut self.rng) < field_drop_probability_with(&self.params.thresholds, snr[active])
        {
            field_valid = false;
        }
        let o = &self.out;
        // Without a transmitter the receiver still outputs static (published as before); only a dropped field
        // repeats the previous one, so sync and the picture keep their last values and field_valid marks the miss.
        if field_valid || !transmitting {
            let sync = self.tracker.update(snr[active]);
            let picture = picture_with(&self.params.thresholds, snr[active]);
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
        } else {
            bus.set(o.present, 1.0);
        }
        bus.set(o.field_valid, if field_valid { 1.0 } else { 0.0 });
        Ok(())
    }
}
