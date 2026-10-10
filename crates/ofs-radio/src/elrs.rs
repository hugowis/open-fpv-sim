//! ExpressLRS link, behavioural (fidelity level 1). Every packet the handset samples the sticks; the packet
//! crosses a channel with Gilbert-Elliott burst loss; the receiver writes one CRSF RC frame per packet it
//! receives (and LINK_STATISTICS every N received packets) to the flight controller's UART. When packets
//! stop, the receiver goes silent, as ExpressLRS does by default, and Betaflight's own failsafe takes over.
use std::collections::VecDeque;

use ofs_core::rng::model_rng;
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx, Wire};
use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::crsf::{self, LinkStatistics, CHANNEL_COUNT};

pub const MODEL_NAME: &str = "radio.elrs";
/// ExpressLRS reports link quality as the share of the last 100 packets received.
pub const LQ_WINDOW: usize = 100;
/// RSSI published while no packet is heard.
pub const NO_SIGNAL_RSSI_DBM: f64 = -130.0;

#[derive(Debug, Clone, PartialEq)]
pub struct LinkParams {
    pub packet_rate_hz: u32,
    /// Packets between the handset sampling the sticks and the receiver outputting them.
    pub latency_packets: u32,
    /// Loss probability per packet in the good and the bad (burst) channel state.
    pub loss_good: f64,
    pub loss_bad: f64,
    /// Per-packet probability of entering and of leaving the bad state.
    pub p_good_to_bad: f64,
    pub p_bad_to_good: f64,
    pub rssi_dbm: f64,
    pub snr_db: f64,
    /// A LINK_STATISTICS frame follows every this many received packets.
    pub link_stats_interval_packets: u32,
    pub rf_mode: u8,
    pub tx_power: u8,
}

impl LinkParams {
    /// A perfect link at `packet_rate_hz`: no loss, one packet of latency.
    pub fn ideal(packet_rate_hz: u32) -> Self {
        Self {
            packet_rate_hz,
            latency_packets: 1,
            loss_good: 0.0,
            loss_bad: 0.0,
            p_good_to_bad: 0.0,
            p_bad_to_good: 1.0,
            rssi_dbm: -50.0,
            snr_db: 10.0,
            link_stats_interval_packets: 50,
            rf_mode: 0,
            tx_power: 0,
        }
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
}

pub struct ElrsLink {
    params: LinkParams,
    div: u32,
    rng: ChaCha8Rng,
    uart: Wire,
    inputs: Inputs,
    outputs: Outputs,
    bad: bool,
    history: VecDeque<bool>,
    pipeline: VecDeque<[u16; CHANNEL_COUNT]>,
    since_stats: u32,
}

impl ElrsLink {
    /// `uart` receives the receiver's CRSF output (the flight controller's UART RX).
    pub fn new(params: LinkParams, rate_divisor: u32, seed: u64, uart: Wire, bus: &mut Bus) -> Self {
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
        };
        Self {
            params,
            div: rate_divisor,
            rng: model_rng(seed, MODEL_NAME),
            uart,
            inputs,
            outputs,
            bad: false,
            history: VecDeque::with_capacity(LQ_WINDOW + 1),
            pipeline: VecDeque::new(),
            since_stats: 0,
        }
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

    fn link_quality(&self) -> f64 {
        if self.history.is_empty() {
            return 0.0;
        }
        let received = self.history.iter().filter(|r| **r).count();
        100.0 * received as f64 / self.history.len() as f64
    }

    fn statistics(&self, lq: f64) -> LinkStatistics {
        let rssi = (-self.params.rssi_dbm).round().clamp(0.0, 255.0) as u8;
        let snr = self.params.snr_db.round().clamp(-128.0, 127.0) as i8;
        let lq = lq.round().clamp(0.0, 100.0) as u8;
        LinkStatistics {
            uplink_rssi_1: rssi,
            uplink_rssi_2: rssi,
            uplink_lq: lq,
            uplink_snr: snr,
            active_antenna: 0,
            rf_mode: self.params.rf_mode,
            uplink_tx_power: self.params.tx_power,
            downlink_rssi: rssi,
            downlink_lq: lq,
            downlink_snr: snr,
        }
    }
}

impl Model for ElrsLink {
    fn name(&self) -> &str {
        MODEL_NAME
    }

    fn rate_divisor(&self) -> u32 {
        self.div
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        // Handset: the pipeline delays samples by `latency_packets` packets.
        let sample = self.sample(bus);
        if self.pipeline.is_empty() {
            self.pipeline.extend(std::iter::repeat_n(sample, self.params.latency_packets as usize));
        }
        self.pipeline.push_back(sample);
        let channels = self.pipeline.pop_front().expect("the pipeline holds at least this sample");

        // Channel: both numbers are drawn every packet, so the random stream never depends on the inputs.
        let switch: f64 = self.rng.gen();
        let draw: f64 = self.rng.gen();
        self.bad = if self.bad { switch >= self.params.p_bad_to_good } else { switch < self.params.p_good_to_bad };
        let loss = if self.bad { self.params.loss_bad } else { self.params.loss_good };
        let transmitting = bus.get(self.inputs.tx_enabled) > 0.5 && bus.get(self.inputs.fault_loss) < 0.5;
        let received = transmitting && draw >= loss;

        self.history.push_back(received);
        if self.history.len() > LQ_WINDOW {
            self.history.pop_front();
        }
        let lq = self.link_quality();
        let link_up = lq > 0.0;

        // Receiver: one RC frame per received packet, link statistics every N received packets.
        if received {
            self.uart.write(&crsf::rc_channels_frame(&channels));
            self.since_stats += 1;
            if self.since_stats >= self.params.link_stats_interval_packets {
                self.since_stats = 0;
                self.uart.write(&crsf::link_statistics_frame(&self.statistics(lq)));
            }
        }
        bus.set(self.outputs.link_up, if link_up { 1.0 } else { 0.0 });
        bus.set(self.outputs.lq, lq);
        bus.set(self.outputs.rssi, if link_up { self.params.rssi_dbm } else { NO_SIGNAL_RSSI_DBM });
        Ok(())
    }
}
