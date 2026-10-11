//! ExpressLRS 2.4 GHz on the shared propagation model. Every packet the handset samples the sticks; the
//! packet's RSSI comes from the geometry — antenna patterns, range, obstruction, polarization, ground bounce,
//! fading, body shadow — and a logistic curve around the mode's sensitivity decides loss. The receiver writes
//! one CRSF RC frame per packet it receives (and LINK_STATISTICS every N received packets) to the flight
//! controller's UART; when packets stop it goes silent, as ExpressLRS does by default, and Betaflight's own
//! failsafe takes over. Deterministic: every packet draws the same count of numbers from the model's own
//! seeded stream, whatever the inputs.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use glam::{DQuat, DVec3};
use ofs_core::rng::model_rng;
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx, Wire};
use ofs_rf::fading::{body_shadow_db, Diversity, Fader, LOS_K_DB};
use ofs_rf::propagation::{mw_to_dbm, path_gain, wavelength_m, Antenna, AntennaKind, Endpoint, Obstacle, Polarization};
use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::crsf::{self, Decoder, Frame, LinkStatistics, CHANNEL_COUNT};

pub const MODEL_NAME: &str = "radio.elrs";
/// ExpressLRS reports link quality as the share of the last 100 packets received.
pub const LQ_WINDOW: usize = 100;
/// RSSI published while no packet is heard.
pub const NO_SIGNAL_RSSI_DBM: f64 = -130.0;
/// The middle of the 2.4 GHz band; frequency hopping is not modelled.
pub const ELRS_FREQ_MHZ: f64 = 2440.0;
/// LoRa bandwidth 812.5 kHz, noise figure 6 dB (estimated): `-174 + 10*log10(812_500) + 6`.
pub const NOISE_FLOOR_DBM: f64 = -108.90176630349089;
/// The receiver transmits the downlink at this power (estimated).
pub const DOWNLINK_TX_POWER_MW: f64 = 100.0;
/// The logistic PER curve's width in dB (an estimate).
pub const PER_SLOPE_DB: f64 = 1.0;
/// Every this-many downlink packets carries one telemetry frame (an ExpressLRS telemetry-ratio
/// estimate: 125 slots/s at the 500 Hz packet rate).
pub const TELEMETRY_SLOT_DIVISOR: u32 = 4;
/// How many offered telemetry frames wait for a slot at most; the oldest is dropped when full
/// (stale data yields to fresh, like a radio that cannot retransmit).
pub const TELEMETRY_FIFO_LEN: usize = 16;

/// The RSSI at which half the packets decode, by packet rate: ExpressLRS's published sensitivities for its
/// 2.4 GHz LoRa modes.
pub fn sensitivity_dbm(packet_rate_hz: u32) -> Option<f64> {
    match packet_rate_hz {
        50 => Some(-117.0),
        150 => Some(-112.0),
        250 => Some(-108.0),
        500 => Some(-105.0),
        _ => None,
    }
}

/// The CRSF rf_mode index of a packet rate (the CRSF table: 0 = 4 Hz, 1 = 50, 2 = 150, 3 = 250, 4 = 500).
pub fn rf_mode_index(packet_rate_hz: u32) -> Option<u8> {
    match packet_rate_hz {
        50 => Some(1),
        150 => Some(2),
        250 => Some(3),
        500 => Some(4),
        _ => None,
    }
}

/// The CRSF power index of a TX power in mW (the CRSF power table).
pub fn tx_power_index_mw(power_mw: u32) -> u8 {
    match power_mw {
        0..=9 => 0,
        10..=24 => 1,
        25..=49 => 6,
        50..=99 => 10,
        100..=249 => 13,
        250..=499 => 17,
        500..=999 => 20,
        _ => 23,
    }
}

/// The packet error rate at `rssi_dbm` for a mode whose sensitivity is `sensitivity_dbm`: a logistic curve,
/// 50 % at the sensitivity (the slope is an estimate).
pub fn packet_error_rate(rssi_dbm: f64, sensitivity_dbm: f64) -> f64 {
    1.0 / (1.0 + ((rssi_dbm - sensitivity_dbm) / PER_SLOPE_DB).exp())
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinkParams {
    pub packet_rate_hz: u32,
    /// Packets between the handset sampling the sticks and the receiver outputting them.
    pub latency_packets: u32,
    /// The handset's TX power.
    pub tx_power_mw: f64,
    /// A LINK_STATISTICS frame follows every this many received packets.
    pub link_stats_interval_packets: u32,
    /// Where the handset is (NED) and its antennas (axes already in the world frame).
    pub handset_position: DVec3,
    pub handset_antennas: Vec<Antenna>,
    /// The receiver's antennas on the quad (axes in the body frame, FRD).
    pub quad_antennas: Vec<Antenna>,
    pub obstacles: Vec<Obstacle>,
    /// Fading and the ground bounce; tests that check the bare link budget turn them off.
    pub fading: bool,
    pub ground_bounce: bool,
}

impl LinkParams {
    /// A strong link at `packet_rate_hz`: one packet of latency, both ends a vertical 2 dBi linear dipole, the
    /// handset 2 m from the quad's home and level with the pad (a horizontal path: the dipoles are broadside),
    /// no obstacles, no fading, no bounce.
    pub fn ideal(packet_rate_hz: u32) -> Self {
        let dipole = Antenna { kind: AntennaKind::Omni, gain_dbi: 2.0, polarization: Polarization::Linear, axis: DVec3::NEG_Z };
        Self {
            packet_rate_hz,
            latency_packets: 1,
            tx_power_mw: 250.0,
            link_stats_interval_packets: 50,
            handset_position: DVec3::new(-2.0, 0.0, -1.7),
            handset_antennas: vec![dipole],
            quad_antennas: vec![dipole],
            obstacles: Vec::new(),
            fading: false,
            ground_bounce: false,
        }
    }
}

/// What Betaflight told the handset, decoded from the CRSF telemetry frames that flew a downlink
/// slot. Units are Betaflight's own CRSF encodings (see docs/research/elrs-link.md): the battery
/// voltage in 0.1 V and current in 0.1 A (`getLegacyBatteryVoltage()`/`getAmperage()/10`), attitude
/// in radians x 10 000 with yaw wrapped to ±180°, the flight mode verbatim.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FcTelemetry {
    pub battery_v: f64,
    pub battery_a: f64,
    pub battery_mah: f64,
    pub roll_deg: f64,
    pub pitch_deg: f64,
    pub yaw_deg: f64,
    pub flight_mode: String,
    /// Simulated time of the last frame that flew a slot (0 until one arrives).
    pub last_frame_s: f64,
    /// Frames that flew a slot so far (a monotonic "anything arrived yet" marker).
    pub frame_count: u64,
}

/// The latest [`FcTelemetry`], shared with the server's state snapshot (the `OsdHandle` pattern).
#[derive(Debug, Clone, Default)]
pub struct FcTelemetryHandle(Arc<Mutex<FcTelemetry>>);

impl FcTelemetryHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn latest(&self) -> FcTelemetry {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn store(&self, t: FcTelemetry) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = t;
    }
}

struct Inputs {
    roll: Signal<f64>,
    pitch: Signal<f64>,
    yaw: Signal<f64>,
    throttle: Signal<f64>,
    aux: Vec<Signal<f64>>,
    tx_enabled: Signal<f64>,
    fault_loss: Signal<f64>,
}

struct Outputs {
    link_up: Signal<f64>,
    lq: Signal<f64>,
    rssi: Signal<f64>,
    snr: Signal<f64>,
    antenna: Signal<f64>,
    downlink_lq: Signal<f64>,
}

pub struct ElrsLink {
    params: LinkParams,
    div: u32,
    rng: ChaCha8Rng,
    uart: Wire,
    inputs: Inputs,
    outputs: Outputs,
    pos: Signal<DVec3>,
    att: Signal<DQuat>,
    history: VecDeque<bool>,
    downlink_history: VecDeque<bool>,
    pipeline: VecDeque<[u16; CHANNEL_COUNT]>,
    since_stats: u32,
    quad_diversity: Diversity,
    handset_diversity: Diversity,
    quad_faders: Vec<Fader>,
    handset_faders: Vec<Fader>,
    /// Reused per-packet scratch: the per-antenna RSSI vectors (no allocation in the hot loop).
    rssi: Vec<f64>,
    downlink: Vec<f64>,
    last_pos: Option<DVec3>,
    telemetry_in: Wire,
    decoder: Decoder,
    telemetry_fifo: VecDeque<(u8, Vec<u8>)>,
    downlink_packets: u32,
    fc_telemetry: FcTelemetryHandle,
}

impl ElrsLink {
    /// `uart` receives the receiver's CRSF output (the flight controller's UART RX); `telemetry_in`
    /// carries the flight controller's own CRSF telemetry frames (its UART TX) into the downlink slots.
    pub fn new(params: LinkParams, rate_divisor: u32, seed: u64, uart: Wire, telemetry_in: Wire, bus: &mut Bus) -> Self {
        assert!(!params.quad_antennas.is_empty() && !params.handset_antennas.is_empty(), "both ends need at least one antenna");
        assert!(sensitivity_dbm(params.packet_rate_hz).is_some(), "packet_rate_hz must be one of 50, 150, 250, 500");
        let inputs = Inputs {
            roll: bus.signal(names::RC_ROLL),
            pitch: bus.signal(names::RC_PITCH),
            yaw: bus.signal(names::RC_YAW),
            throttle: bus.signal(names::RC_THROTTLE),
            aux: (0..names::RC_AUX_COUNT).map(|i| bus.signal(&names::rc_aux(i))).collect(),
            tx_enabled: bus.signal(names::RADIO_TX_ENABLED),
            fault_loss: bus.signal(names::FAULT_RADIO_LINK_LOSS),
        };
        let outputs = Outputs {
            link_up: bus.signal(names::RADIO_LINK_UP),
            lq: bus.signal(names::RADIO_LQ),
            rssi: bus.signal(names::RADIO_RSSI),
            snr: bus.signal(names::RADIO_SNR),
            antenna: bus.signal(names::RADIO_ANTENNA),
            downlink_lq: bus.signal(names::RADIO_DOWNLINK_LQ),
        };
        let mut rng = model_rng(seed, MODEL_NAME);
        let mut quad_faders = vec![Fader::default(); params.quad_antennas.len()];
        let mut handset_faders = vec![Fader::default(); params.handset_antennas.len()];
        for f in quad_faders.iter_mut().chain(handset_faders.iter_mut()) {
            f.next(&mut rng, 0.0, LOS_K_DB); // a fresh scatter state for the first packet
        }
        let (quad_antennas, handset_antennas) = (params.quad_antennas.len(), params.handset_antennas.len());
        Self {
            params,
            div: rate_divisor,
            rng,
            uart,
            inputs,
            outputs,
            pos: bus.signal(names::BODY_POS_NED),
            att: bus.signal(names::BODY_ATT),
            history: VecDeque::with_capacity(LQ_WINDOW + 1),
            downlink_history: VecDeque::with_capacity(LQ_WINDOW + 1),
            pipeline: VecDeque::new(),
            since_stats: 0,
            quad_diversity: Diversity::default(),
            handset_diversity: Diversity::default(),
            quad_faders,
            handset_faders,
            rssi: vec![NO_SIGNAL_RSSI_DBM; quad_antennas],
            downlink: vec![NO_SIGNAL_RSSI_DBM; handset_antennas],
            last_pos: None,
            telemetry_in,
            decoder: Decoder::default(),
            telemetry_fifo: VecDeque::new(),
            downlink_packets: 0,
            fc_telemetry: FcTelemetryHandle::new(),
        }
    }

    /// The handset's view of Betaflight's telemetry, updated as frames fly downlink slots.
    pub fn fc_telemetry_handle(&self) -> FcTelemetryHandle {
        self.fc_telemetry.clone()
    }

    /// Handset: sticks to channels in Betaflight's default AETR order, then AUX1..4, the rest centred.
    fn sample(&self, bus: &Bus) -> [u16; CHANNEL_COUNT] {
        let i = &self.inputs;
        let mut ch = [crsf::stick_ticks(0.0); CHANNEL_COUNT];
        ch[0] = crsf::stick_ticks(bus.get(i.roll));
        ch[1] = crsf::stick_ticks(bus.get(i.pitch));
        ch[2] = crsf::throttle_ticks(bus.get(i.throttle));
        ch[3] = crsf::stick_ticks(bus.get(i.yaw));
        for (slot, s) in ch[4..].iter_mut().zip(&i.aux) {
            *slot = crsf::stick_ticks(bus.get(*s));
        }
        ch
    }

    fn lq_of(history: &VecDeque<bool>) -> f64 {
        if history.is_empty() {
            return 0.0;
        }
        100.0 * history.iter().filter(|r| **r).count() as f64 / history.len() as f64
    }

    /// Decodes one telemetry frame that flew a downlink slot into the shared handle. Frame types we
    /// do not expose still consume their slot (the radio forwarded them; nothing more is modelled).
    fn apply_telemetry_frame(&mut self, frame_type: u8, payload: &[u8], now_s: f64) {
        let mut t = self.fc_telemetry.latest();
        match frame_type {
            crsf::FRAMETYPE_BATTERY_SENSOR if payload.len() >= 7 => {
                t.battery_v = f64::from(u16::from_be_bytes([payload[0], payload[1]])) / 10.0;
                t.battery_a = f64::from(u16::from_be_bytes([payload[2], payload[3]])) / 10.0;
                t.battery_mah = f64::from(u32::from_be_bytes([0, payload[4], payload[5], payload[6]]));
            }
            crsf::FRAMETYPE_ATTITUDE if payload.len() >= 6 => {
                let deg = |raw: [u8; 2]| f64::from(i16::from_be_bytes(raw)) * 180.0 / (10_000.0 * std::f64::consts::PI);
                t.pitch_deg = deg([payload[0], payload[1]]);
                t.roll_deg = deg([payload[2], payload[3]]);
                t.yaw_deg = deg([payload[4], payload[5]]);
            }
            crsf::FRAMETYPE_FLIGHT_MODE => {
                t.flight_mode = String::from_utf8_lossy(payload).split('\0').next().unwrap_or("").to_owned();
            }
            _ => {}
        }
        t.frame_count += 1;
        t.last_frame_s = now_s;
        self.fc_telemetry.store(t);
    }
}

impl Model for ElrsLink {
    fn name(&self) -> &str {
        MODEL_NAME
    }

    fn rate_divisor(&self) -> u32 {
        self.div
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        // Handset: the pipeline delays samples by `latency_packets` packets.
        let sample = self.sample(bus);
        if self.pipeline.is_empty() {
            self.pipeline.extend(std::iter::repeat_n(sample, self.params.latency_packets as usize));
        }
        self.pipeline.push_back(sample);
        let channels = self.pipeline.pop_front().expect("the pipeline holds at least this sample");

        let pos = bus.get(self.pos);
        let att = bus.get(self.att);
        let att = if att.length_squared() > 0.0 { att.normalize() } else { DQuat::IDENTITY };
        let moved = self.last_pos.map_or(0.0, |p| p.distance(pos));
        self.last_pos = Some(pos);
        let sensitivity = sensitivity_dbm(self.params.packet_rate_hz).expect("checked at construction");
        let transmitting = bus.get(self.inputs.tx_enabled) > 0.5 && bus.get(self.inputs.fault_loss) < 0.5;

        // Draw order is fixed: every packet draws the same count of numbers whatever the inputs — the uplink
        // fades (two normals per quad antenna), the uplink loss draw, the downlink fades, the downlink draw.
        let rho = (-moved / (wavelength_m(ELRS_FREQ_MHZ) * 0.5)).exp();
        let shadow = body_shadow_db(att, pos, self.params.handset_position);
        // The handset transmits on its active antenna: invariant across the quad's antennas.
        let tx_ant = self.params.handset_antennas[self.handset_diversity.active().min(self.params.handset_antennas.len() - 1)];
        self.rssi.clear();
        self.rssi.resize(self.params.quad_antennas.len(), NO_SIGNAL_RSSI_DBM);
        let rssi = &mut self.rssi;
        for (j, configured) in self.params.quad_antennas.iter().enumerate() {
            let rx_ant = Antenna { axis: (att * configured.axis).normalize(), ..*configured };
            let path = path_gain(
                &Endpoint { position: self.params.handset_position, antenna: tx_ant },
                &Endpoint { position: pos, antenna: rx_ant },
                ELRS_FREQ_MHZ,
                &self.params.obstacles,
                self.params.ground_bounce,
            );
            let fade = if self.params.fading {
                self.quad_faders[j].next(&mut self.rng, rho, LOS_K_DB - path.obstruction_db)
            } else {
                0.0
            };
            rssi[j] = (mw_to_dbm(self.params.tx_power_mw) + path.gain_db + fade - shadow).max(NO_SIGNAL_RSSI_DBM);
        }
        let active = self.quad_diversity.choose(rssi);
        let draw: f64 = self.rng.gen();
        let received = transmitting && draw >= packet_error_rate(rssi[active], sensitivity);

        // Downlink: the reversed path of the active antenna, the receiver transmitting at DOWNLINK_TX_POWER_MW,
        // received on the handset's best antenna.
        let configured = self.params.quad_antennas[active];
        let tx_ant = Antenna { axis: (att * configured.axis).normalize(), ..configured };
        self.downlink.clear();
        self.downlink.resize(self.params.handset_antennas.len(), NO_SIGNAL_RSSI_DBM);
        let downlink = &mut self.downlink;
        for (a, rx_ant) in self.params.handset_antennas.iter().enumerate() {
            let path = path_gain(
                &Endpoint { position: pos, antenna: tx_ant },
                &Endpoint { position: self.params.handset_position, antenna: *rx_ant },
                ELRS_FREQ_MHZ,
                &self.params.obstacles,
                self.params.ground_bounce,
            );
            let fade = if self.params.fading {
                self.handset_faders[a].next(&mut self.rng, rho, LOS_K_DB - path.obstruction_db)
            } else {
                0.0
            };
            downlink[a] = (mw_to_dbm(DOWNLINK_TX_POWER_MW) + path.gain_db + fade - shadow).max(NO_SIGNAL_RSSI_DBM);
        }
        let downlink_active = self.handset_diversity.choose(downlink);
        let downlink_draw: f64 = self.rng.gen();
        let downlink_received = transmitting && downlink_draw >= packet_error_rate(downlink[downlink_active], sensitivity);

        // Telemetry ingress: whatever Betaflight wrote this tick joins the FIFO (oldest dropped when
        // full). No new random draws: the queue is a pure function of the frames offered and the
        // slots flown.
        for frame in self.decoder.push(&self.telemetry_in.take(usize::MAX)) {
            if let Frame::Other { frame_type, payload, .. } = frame {
                if self.telemetry_fifo.len() >= TELEMETRY_FIFO_LEN {
                    self.telemetry_fifo.pop_front();
                }
                self.telemetry_fifo.push_back((frame_type, payload));
            }
        }
        if transmitting {
            self.downlink_packets += 1;
            if downlink_received && self.downlink_packets % TELEMETRY_SLOT_DIVISOR == 0 {
                if let Some((frame_type, payload)) = self.telemetry_fifo.pop_front() {
                    self.apply_telemetry_frame(frame_type, &payload, ctx.time_s);
                }
            }
        }

        // Link quality over the last 100 packets, per direction; silence on loss; Betaflight fails safe.
        self.history.push_back(received);
        if self.history.len() > LQ_WINDOW {
            self.history.pop_front();
        }
        self.downlink_history.push_back(downlink_received);
        if self.downlink_history.len() > LQ_WINDOW {
            self.downlink_history.pop_front();
        }
        let lq = Self::lq_of(&self.history);
        let downlink_lq = Self::lq_of(&self.downlink_history);
        let link_up = lq > 0.0;
        let snr = rssi[active] - NOISE_FLOOR_DBM;

        // Receiver: one RC frame per received packet, link statistics every N received packets.
        if received {
            self.uart.write(&crsf::rc_channels_frame(&channels));
            self.since_stats += 1;
            if self.since_stats >= self.params.link_stats_interval_packets {
                self.since_stats = 0;
                let dbm_field = |dbm: f64| (-dbm).round().clamp(0.0, 255.0) as u8;
                self.uart.write(&crsf::link_statistics_frame(&LinkStatistics {
                    uplink_rssi_1: dbm_field(rssi[0]),
                    uplink_rssi_2: rssi.get(1).map_or(dbm_field(rssi[0]), |v| dbm_field(*v)),
                    uplink_lq: lq.round().clamp(0.0, 100.0) as u8,
                    uplink_snr: snr.round().clamp(-128.0, 127.0) as i8,
                    active_antenna: active as u8,
                    rf_mode: rf_mode_index(self.params.packet_rate_hz).unwrap_or(0),
                    uplink_tx_power: tx_power_index_mw(self.params.tx_power_mw as u32),
                    downlink_rssi: dbm_field(downlink[downlink_active]),
                    downlink_lq: downlink_lq.round().clamp(0.0, 100.0) as u8,
                    downlink_snr: (downlink[downlink_active] - NOISE_FLOOR_DBM).round().clamp(-128.0, 127.0) as i8,
                }));
            }
        }
        bus.set(self.outputs.link_up, if link_up { 1.0 } else { 0.0 });
        bus.set(self.outputs.lq, lq);
        bus.set(self.outputs.rssi, if link_up { rssi[active] } else { NO_SIGNAL_RSSI_DBM });
        bus.set(self.outputs.snr, if link_up { snr } else { NO_SIGNAL_RSSI_DBM - NOISE_FLOOR_DBM });
        bus.set(self.outputs.antenna, active as f64);
        bus.set(self.outputs.downlink_lq, downlink_lq);
        Ok(())
    }
}
