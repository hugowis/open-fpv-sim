//! A geometry-free link: a fixed loss fraction, optional bursts and a whole number of packets of latency, with no
//! RF budget. It is the worked example of a [`Link`] in `docs/writing-a-radio-model.md`, and the second,
//! structurally different link that keeps the trait honest.
use std::collections::VecDeque;

use ofs_core::rng::model_rng;
use ofs_core::{Bus, StepCtx};
use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::elrs::{NOISE_FLOOR_DBM, NO_SIGNAL_RSSI_DBM, TELEMETRY_SLOT_DIVISOR};
use crate::parts::{Channels, Delivery, FaultSignals, Link, LinkReport, LqWindow, TxFrame};

pub const MODEL_NAME: &str = "radio.fixed";
/// What a fixed link reports while it is up: there is no geometry, so nominal numbers (estimated).
pub const NOMINAL_RSSI_DBM: f64 = -50.0;
pub const NOMINAL_SNR_DB: f64 = 40.0;

#[derive(Debug, Clone, PartialEq)]
pub struct FixedParams {
    /// Packets per second; it must divide the base rate.
    pub packet_rate_hz: u32,
    /// Packets between the handset sampling the sticks and the receiver outputting them.
    pub latency_packets: u32,
    /// The share of packets lost, in [0, 1].
    pub loss_fraction: f64,
    /// Every packet is lost for `burst_on_s`, then delivered (subject to `loss_fraction`) for `burst_off_s`,
    /// repeating. `burst_on_s = 0` means no bursts.
    pub burst_on_s: f64,
    pub burst_off_s: f64,
}

pub struct FixedLink {
    params: FixedParams,
    rng: ChaCha8Rng,
    fault_rng: ChaCha8Rng,
    faults: FaultSignals,
    pipeline: VecDeque<Channels>,
    history: LqWindow,
    downlink_packets: u32,
}

impl FixedLink {
    pub fn new(params: FixedParams, seed: u64, bus: &mut Bus) -> Self {
        assert!(params.packet_rate_hz > 0, "packet_rate_hz must be positive");
        assert!((0.0..=1.0).contains(&params.loss_fraction), "loss_fraction must be in [0, 1]");
        assert!(
            params.burst_on_s >= 0.0 && params.burst_off_s >= 0.0 && (params.burst_on_s == 0.0 || params.burst_off_s > 0.0),
            "burst windows must be >= 0, and a burst needs an off window"
        );
        Self {
            rng: model_rng(seed, MODEL_NAME),
            fault_rng: model_rng(seed, "fixed-faults"),
            faults: FaultSignals::register(bus),
            pipeline: VecDeque::new(),
            history: LqWindow::new(),
            downlink_packets: 0,
            params,
        }
    }
}

impl Link for FixedLink {
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

        // Both draws happen every packet, whatever the inputs, so a seeded run's stream never shifts.
        let draw: f64 = self.rng.gen();
        let fault_draw: f64 = self.fault_rng.gen();
        let p = &self.params;
        let in_burst = p.burst_on_s > 0.0 && ctx.time_s % (p.burst_on_s + p.burst_off_s) < p.burst_on_s;
        let transmitting = tx.transmitting && !self.faults.link_cut(bus);
        let received = transmitting && draw >= p.loss_fraction && !in_burst && self.faults.survives(bus, ctx.time_s, fault_draw);

        // The downlink is heard whenever the uplink was: one packet in TELEMETRY_SLOT_DIVISOR carries a frame.
        let mut downlink_slot = false;
        if transmitting {
            self.downlink_packets += 1;
            downlink_slot = received && self.downlink_packets % TELEMETRY_SLOT_DIVISOR == 0;
        }

        let lq = self.history.push(received);
        let up = lq > 0.0;
        Delivery {
            channels: received.then_some(channels),
            downlink_slot,
            report: LinkReport {
                link_up: up,
                lq_pct: lq,
                rssi_dbm: if up { NOMINAL_RSSI_DBM } else { NO_SIGNAL_RSSI_DBM },
                snr_db: if up { NOMINAL_SNR_DB } else { NO_SIGNAL_RSSI_DBM - NOISE_FLOOR_DBM },
                antenna: 0,
                downlink_lq_pct: lq,
                uplink_rssi_dbm: [NOMINAL_RSSI_DBM; 2],
                uplink_snr_db: NOMINAL_SNR_DB,
                downlink_rssi_dbm: NOMINAL_RSSI_DBM,
                downlink_snr_db: NOMINAL_SNR_DB,
                rf_mode: 0,
                tx_power_index: 0,
            },
        }
    }
}
