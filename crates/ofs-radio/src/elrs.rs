//! The ExpressLRS link: the air between the handset and the receiver, on the shared propagation model. Every packet
//! slot it takes the handset's [`TxFrame`] and works out what the air does to it: the RF budget of both
//! directions (antenna patterns, range, obstruction, polarization, ground bounce, fading, body shadow) in the
//! band's profile (2.4 GHz or 900 MHz, `band`), a logistic packet-error curve around the packet rate's
//! sensitivity that decides loss, the latency pipeline that delays the channels by whole packets, link quality
//! (the share received of the last 100 packets, per direction) and the downlink telemetry slots. It returns a
//! [`Delivery`]: the channels after latency (or none when the packet was lost) and the quality figures the
//! receiver reports and the bus publishes. The CRSF bytes are the receiver's work (`receiver.rs`), the sticks the
//! handset's (`joystick.rs`). When packets stop the receiver goes silent, as ExpressLRS does by default, and
//! Betaflight's own failsafe takes over. Deterministic: every packet draws the same count of numbers from the
//! model's own seeded stream, whatever the inputs.
use std::collections::VecDeque;
use std::sync::Arc;

use glam::{DQuat, DVec3};
use ofs_core::rng::model_rng;
use ofs_core::terrain::Terrain;
use ofs_core::{names, Bus, Signal, StepCtx};
use ofs_rf::fading::{body_shadow_db, Diversity, Fader, LOS_K_DB};
use ofs_rf::propagation::{mw_to_dbm, path_gain, wavelength_m, Antenna, AntennaKind, Endpoint, Obstacle, Polarization};
use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::band::Band;
use crate::parts::{Channels, Delivery, FaultSignals, Link, LinkReport, LqWindow, TxFrame};

pub const MODEL_NAME: &str = "radio.elrs";
/// RSSI published while no packet is heard.
pub const NO_SIGNAL_RSSI_DBM: f64 = -130.0;
/// The middle of the 2.4 GHz band; frequency hopping is not modelled (the 900 MHz band is in `band`).
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

/// The telemetry ratios a link accepts: 1, 2, 4, ..., 128. `0` (off) is not modelled.
pub(crate) fn valid_telemetry_slot_divisor(divisor: u32) -> bool {
    matches!(divisor, 1 | 2 | 4 | 8 | 16 | 32 | 64 | 128)
}

/// The RSSI at which half the packets decode, by packet rate: ExpressLRS's published sensitivities for its
/// 2.4 GHz LoRa modes.
pub fn sensitivity_dbm(packet_rate_hz: u32) -> Option<f64> {
    Band::G2_4.sensitivity_dbm(packet_rate_hz)
}

/// The CRSF rf_mode index of a packet rate (the CRSF table: 0 = 4 Hz, 1 = 50, 2 = 150, 3 = 250, 4 = 500).
pub fn rf_mode_index(packet_rate_hz: u32) -> Option<u8> {
    Band::G2_4.rf_mode_index(packet_rate_hz)
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
    /// One downlink packet in this many carries a telemetry frame; see [`TELEMETRY_SLOT_DIVISOR`].
    pub telemetry_slot_divisor: u32,
    /// The ExpressLRS band: carrier, noise floor and the packet rates and sensitivities of its modes.
    pub band: Band,
    /// Packets between the handset sampling the sticks and the receiver outputting them.
    pub latency_packets: u32,
    /// The handset's TX power.
    pub tx_power_mw: f64,
    /// Where the handset is (NED) and its antennas (axes already in the world frame).
    pub handset_position: DVec3,
    pub handset_antennas: Vec<Antenna>,
    /// The receiver's antennas on the quad (axes in the body frame, FRD).
    pub quad_antennas: Vec<Antenna>,
    pub obstacles: Vec<Obstacle>,
    /// The ground that can hide the quad from the handset: the physics' own grid, shared: never a copy of the heights.
    pub terrain: Option<Arc<Terrain>>,
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
            telemetry_slot_divisor: TELEMETRY_SLOT_DIVISOR,
            band: Band::G2_4,
            latency_packets: 1,
            tx_power_mw: 250.0,
            handset_position: DVec3::new(-2.0, 0.0, -1.7),
            handset_antennas: vec![dipole],
            quad_antennas: vec![dipole],
            obstacles: Vec::new(),
            terrain: None,
            fading: false,
            ground_bounce: false,
        }
    }
}

/// The ExpressLRS link: the RF budget of both directions on the shared propagation model, a logistic packet-loss
/// curve around the mode's sensitivity, the handset-to-receiver latency pipeline, link quality, and the downlink
/// telemetry slots. Deterministic: every packet draws the same count of numbers from the model's own seeded
/// stream, whatever the inputs.
pub struct ElrsLink {
    params: LinkParams,
    rng: ChaCha8Rng,
    fault_rng: ChaCha8Rng,
    downlink_fault_rng: ChaCha8Rng,
    faults: FaultSignals,
    pos: Signal<DVec3>,
    att: Signal<DQuat>,
    history: LqWindow,
    downlink_history: LqWindow,
    pipeline: VecDeque<Channels>,
    quad_diversity: Diversity,
    handset_diversity: Diversity,
    quad_faders: Vec<Fader>,
    handset_faders: Vec<Fader>,
    /// Reused per-packet scratch: the per-antenna RSSI vectors (no allocation in the hot loop).
    rssi: Vec<f64>,
    downlink: Vec<f64>,
    last_pos: Option<DVec3>,
    downlink_packets: u32,
}

impl ElrsLink {
    pub fn new(params: LinkParams, seed: u64, bus: &mut Bus) -> Self {
        debug_assert!(
            valid_telemetry_slot_divisor(params.telemetry_slot_divisor),
            "telemetry_slot_divisor must be one of 1, 2, 4, 8, 16, 32, 64, 128 (got {})",
            params.telemetry_slot_divisor
        );
        assert!(!params.quad_antennas.is_empty() && !params.handset_antennas.is_empty(), "both ends need at least one antenna");
        assert!(
            params.band.sensitivity_dbm(params.packet_rate_hz).is_some(),
            "packet_rate_hz {} is not a {} mode: use one of {:?}",
            params.packet_rate_hz,
            params.band.name(),
            params.band.packet_rates()
        );
        let mut rng = model_rng(seed, MODEL_NAME);
        let fault_rng = model_rng(seed, "elrs-faults");
        let downlink_fault_rng = model_rng(seed, "elrs-faults-downlink");
        let mut quad_faders = vec![Fader::default(); params.quad_antennas.len()];
        let mut handset_faders = vec![Fader::default(); params.handset_antennas.len()];
        for f in quad_faders.iter_mut().chain(handset_faders.iter_mut()) {
            f.next(&mut rng, 0.0, LOS_K_DB); // a fresh scatter state for the first packet
        }
        let (quad_antennas, handset_antennas) = (params.quad_antennas.len(), params.handset_antennas.len());
        Self {
            params,
            rng,
            fault_rng,
            downlink_fault_rng,
            faults: FaultSignals::register(bus),
            pos: bus.signal(names::BODY_POS_NED),
            att: bus.signal(names::BODY_ATT),
            history: LqWindow::new(),
            downlink_history: LqWindow::new(),
            pipeline: VecDeque::new(),
            quad_diversity: Diversity::default(),
            handset_diversity: Diversity::default(),
            quad_faders,
            handset_faders,
            rssi: vec![NO_SIGNAL_RSSI_DBM; quad_antennas],
            downlink: vec![NO_SIGNAL_RSSI_DBM; handset_antennas],
            last_pos: None,
            downlink_packets: 0,
        }
    }
}

impl Link for ElrsLink {
    fn name(&self) -> &str {
        MODEL_NAME
    }

    fn packet_rate_hz(&self) -> u32 {
        self.params.packet_rate_hz
    }

    fn deliver(&mut self, ctx: &StepCtx, bus: &Bus, tx: &TxFrame) -> Delivery {
        // The pipeline delays samples by `latency_packets` packets.
        if self.pipeline.is_empty() {
            self.pipeline.extend(std::iter::repeat_n(tx.channels, self.params.latency_packets as usize));
        }
        self.pipeline.push_back(tx.channels);
        let channels = self.pipeline.pop_front().expect("the pipeline holds at least this sample");

        let pos = bus.get(self.pos);
        let att = bus.get(self.att);
        let att = if att.length_squared() > 0.0 { att.normalize() } else { DQuat::IDENTITY };
        let moved = self.last_pos.map_or(0.0, |p| p.distance(pos));
        self.last_pos = Some(pos);
        let sensitivity = self.params.band.sensitivity_dbm(self.params.packet_rate_hz).expect("checked at construction");
        let transmitting = tx.transmitting && !self.faults.link_cut(bus);
        let freq = self.params.band.freq_mhz();
        let noise = self.params.band.noise_floor_dbm();

        // Draw order is fixed: every packet draws the same count of numbers whatever the inputs — the uplink
        // fades (two normals per quad antenna), the uplink loss draw, the downlink fades, the downlink draw.
        let rho = (-moved / (wavelength_m(freq) * 0.5)).exp();
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
                freq,
                &self.params.obstacles,
                self.params.terrain.as_deref(),
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
        // Drawn every packet from its own stream, fault or not, so the model's own draws (fading, loss, downlink)
        // never shift with a fault (tests/elrs.rs: the_models_own_stream_ignores_the_fault_fraction).
        let fault_draw: f64 = self.fault_rng.gen();
        let received = transmitting
            && draw >= packet_error_rate(rssi[active], sensitivity)
            && self.faults.survives(bus, ctx.time_s, fault_draw);

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
                freq,
                &self.params.obstacles,
                self.params.terrain.as_deref(),
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
        // One number per packet from the dedicated downlink-fault stream, fault or not, so toggling the fault never
        // moves the model's own draws or the uplink fault stream.
        let downlink_fault_draw: f64 = self.downlink_fault_rng.gen();
        let downlink_received = transmitting
            && downlink_draw >= packet_error_rate(downlink[downlink_active], sensitivity)
            && self.faults.downlink_survives(bus, downlink_fault_draw);

        // One downlink packet in `telemetry_slot_divisor` carries a telemetry frame, if it is heard.
        let mut downlink_slot = false;
        if transmitting {
            self.downlink_packets += 1;
            downlink_slot = downlink_received && self.downlink_packets % self.params.telemetry_slot_divisor == 0;
        }

        // Link quality over the last 100 packets, per direction; silence on loss; Betaflight fails safe.
        let lq = self.history.push(received);
        let downlink_lq = self.downlink_history.push(downlink_received);
        let link_up = lq > 0.0;
        let snr = rssi[active] - noise;

        Delivery {
            channels: received.then_some(channels),
            downlink_slot,
            report: LinkReport {
                link_up,
                lq_pct: lq,
                rssi_dbm: if link_up { rssi[active] } else { NO_SIGNAL_RSSI_DBM },
                snr_db: if link_up { snr } else { NO_SIGNAL_RSSI_DBM - noise },
                antenna: active,
                downlink_lq_pct: downlink_lq,
                uplink_rssi_dbm: [rssi[0], rssi.get(1).copied().unwrap_or(rssi[0])],
                uplink_snr_db: snr,
                downlink_rssi_dbm: downlink[downlink_active],
                downlink_snr_db: downlink[downlink_active] - noise,
                rf_mode: self.params.band.rf_mode_index(self.params.packet_rate_hz).unwrap_or(0),
                tx_power_index: tx_power_index_mw(self.params.tx_power_mw as u32),
            },
        }
    }
}
