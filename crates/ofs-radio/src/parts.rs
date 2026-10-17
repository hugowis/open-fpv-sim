//! The radio's three swappable parts and what flows between them.
//!
//! One packet slot flows `Handset -> Link -> Receiver`: the handset samples the sticks, the link decides what
//! the air does to the packet and what comes back, the receiver turns what arrived into the bytes the flight
//! controller reads. Only a [`Link`] may draw random numbers: handsets and receivers are pure, so a seeded run
//! never depends on which parts are swapped in around a link.
use std::collections::VecDeque;

use glam::DVec3;
use ofs_core::{names, Bus, Signal, StepCtx};
use ofs_rf::propagation::Antenna;

use crate::crsf::CHANNEL_COUNT;

/// The 16 CRSF channels as 11-bit ticks (the currency between the parts: Betaflight reads CRSF).
pub type Channels = [u16; CHANNEL_COUNT];

/// ExpressLRS reports link quality as the share of the last 100 packets received.
pub const LQ_WINDOW: usize = 100;

/// What the handset sent in one packet slot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TxFrame {
    pub channels: Channels,
    /// The transmitter's switch is on (`radio.tx_enabled`: a pilot or script is connected and the radio was not
    /// switched off). The link-loss fault's cut is not applied here: each link applies it (`FaultSignals::link_cut`).
    pub transmitting: bool,
}

/// The handset's radio end: where it is, its antennas (axes already in the world frame) and its TX power.
#[derive(Debug, Clone)]
pub struct RfFrontEnd {
    pub position: DVec3,
    pub antennas: Vec<Antenna>,
    pub tx_power_mw: f64,
}

/// The controller: stick and switch signals in, one [`TxFrame`] per packet slot out.
pub trait Handset: Send {
    /// Where it transmits from. The chain builder fills the link's transmitter end from the same values; nothing calls
    /// this yet.
    fn rf_front_end(&self) -> &RfFrontEnd;
    fn sample(&mut self, bus: &Bus) -> TxFrame;
}

/// What a link tells the rest of the sim about a packet slot: the six bus outputs, and what a receiver reports
/// in LINK_STATISTICS.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkReport {
    // Published on the bus (the HUD, the state stream and the recorder read these).
    pub link_up: bool,
    pub lq_pct: f64,
    /// The active antenna's RSSI; the no-signal floor while the link is down.
    pub rssi_dbm: f64,
    pub snr_db: f64,
    pub antenna: usize,
    pub downlink_lq_pct: f64,
    // What the receiver reports in LINK_STATISTICS (raw, whether or not the link is up).
    /// Per receiver antenna; with one antenna both entries hold its RSSI.
    pub uplink_rssi_dbm: [f64; 2],
    pub uplink_snr_db: f64,
    pub downlink_rssi_dbm: f64,
    pub downlink_snr_db: f64,
    pub rf_mode: u8,
    pub tx_power_index: u8,
}

/// One packet slot's outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct Delivery {
    /// The channels the receiver heard (after the link's latency); `None` when the packet was lost.
    pub channels: Option<Channels>,
    /// A downlink telemetry slot flew and the handset heard it: one queued frame reaches the handset.
    pub downlink_slot: bool,
    pub report: LinkReport,
}

/// The air between handset and receiver. Implementations register the bus signals they read (the quad's pose,
/// the radio faults) at construction.
pub trait Link: Send {
    /// The model's name: it names the chain and seeds the link's random stream.
    fn name(&self) -> &str;
    fn packet_rate_hz(&self) -> u32;
    fn deliver(&mut self, ctx: &StepCtx, bus: &Bus, tx: &TxFrame) -> Delivery;
}

/// The receiver on the quad: what arrives becomes CRSF bytes on the flight controller's UART.
pub trait Receiver: Send {
    /// Reads what the flight controller sent on its UART TX and queues the telemetry frames for downlink slots.
    fn ingest_telemetry(&mut self);
    /// Writes what the packet slot delivered to the flight controller.
    fn emit(&mut self, delivery: &Delivery);
    /// The next queued telemetry frame (type, payload), when a downlink slot flies.
    fn next_telemetry(&mut self) -> Option<(u8, Vec<u8>)>;
}

/// The six signals every link publishes.
pub struct LinkOutputs {
    link_up: Signal<f64>,
    lq: Signal<f64>,
    rssi: Signal<f64>,
    snr: Signal<f64>,
    antenna: Signal<f64>,
    downlink_lq: Signal<f64>,
}

impl LinkOutputs {
    pub fn register(bus: &mut Bus) -> Self {
        Self {
            link_up: bus.signal(names::RADIO_LINK_UP),
            lq: bus.signal(names::RADIO_LQ),
            rssi: bus.signal(names::RADIO_RSSI),
            snr: bus.signal(names::RADIO_SNR),
            antenna: bus.signal(names::RADIO_ANTENNA),
            downlink_lq: bus.signal(names::RADIO_DOWNLINK_LQ),
        }
    }

    pub fn publish(&self, bus: &mut Bus, report: &LinkReport) {
        bus.set(self.link_up, if report.link_up { 1.0 } else { 0.0 });
        bus.set(self.lq, report.lq_pct);
        bus.set(self.rssi, report.rssi_dbm);
        bus.set(self.snr, report.snr_db);
        bus.set(self.antenna, report.antenna as f64);
        bus.set(self.downlink_lq, report.downlink_lq_pct);
    }
}

/// How close to a burst-window edge a packet's time must be to count as being on it, s.
const BURST_EDGE_S: f64 = 1e-9;

/// The radio faults every link honours (the fault catalog is link-agnostic).
pub struct FaultSignals {
    link_loss: Signal<f64>,
    loss_fraction: Signal<f64>,
    burst_on: Signal<f64>,
    burst_off: Signal<f64>,
}

impl FaultSignals {
    pub fn register(bus: &mut Bus) -> Self {
        Self {
            link_loss: bus.signal(names::FAULT_RADIO_LINK_LOSS),
            loss_fraction: bus.signal(names::FAULT_RADIO_LOSS_FRACTION),
            burst_on: bus.signal(names::FAULT_RADIO_BURST_ON_S),
            burst_off: bus.signal(names::FAULT_RADIO_BURST_OFF_S),
        }
    }

    /// The link-loss fault is active: no packet reaches anyone. A NaN signal counts as cut (it fails safe, as the
    /// pre-split `< 0.5` test did).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn link_cut(&self, bus: &Bus) -> bool {
        !(bus.get(self.link_loss) < 0.5)
    }

    /// The packet survives the extra-loss and burst faults. `fault_draw` is the link's own per-packet draw from its
    /// fault stream (drawn every packet, fault or not, so the link's other draws never shift with a fault).
    pub fn survives(&self, bus: &Bus, time_s: f64, fault_draw: f64) -> bool {
        let fraction = bus.get(self.loss_fraction);
        let burst_on = bus.get(self.burst_on);
        let burst_off = bus.get(self.burst_off);
        let in_burst = burst_on > 0.0 && {
            let period = burst_on + burst_off;
            let phase = time_s % period;
            // A packet within BURST_EDGE_S of an edge belongs to the window that starts there: `time_s % period` can
            // land a few ulps on the wrong side of an edge (0.3 % 0.1 is 0.0999...), which lost or kept one extra packet.
            let phase = if period - phase < BURST_EDGE_S { 0.0 } else { phase };
            phase < burst_on - BURST_EDGE_S
        };
        fault_draw >= fraction && !in_burst
    }
}

/// Link quality: the share received of the last [`LQ_WINDOW`] packets, in percent.
#[derive(Debug)]
pub struct LqWindow(VecDeque<bool>);

impl LqWindow {
    pub fn new() -> Self {
        Self(VecDeque::with_capacity(LQ_WINDOW + 1))
    }

    /// Records one packet and returns the link quality including it.
    pub fn push(&mut self, received: bool) -> f64 {
        self.0.push_back(received);
        if self.0.len() > LQ_WINDOW {
            self.0.pop_front();
        }
        100.0 * self.0.iter().filter(|r| **r).count() as f64 / self.0.len() as f64
    }
}

impl Default for LqWindow {
    fn default() -> Self {
        Self::new()
    }
}
