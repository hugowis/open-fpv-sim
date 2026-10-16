//! The first receiver: one CRSF RC frame per delivered packet, LINK_STATISTICS every N delivered packets, and the
//! flight controller's own CRSF telemetry queued for the downlink slots.
use std::collections::VecDeque;

use ofs_core::Wire;

use crate::crsf::{self, Decoder, Frame, LinkStatistics};
use crate::parts::{Delivery, Receiver};

/// How many offered telemetry frames wait for a slot at most; the oldest is dropped when full (stale data yields
/// to fresh, like a radio that cannot retransmit).
pub const TELEMETRY_FIFO_LEN: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverParams {
    /// A LINK_STATISTICS frame follows every this many delivered packets.
    pub link_stats_interval_packets: u32,
}

impl Default for ReceiverParams {
    fn default() -> Self {
        Self { link_stats_interval_packets: 50 }
    }
}

pub struct CrsfReceiver {
    params: ReceiverParams,
    /// The flight controller's UART RX: the receiver writes here.
    uart: Wire,
    /// The flight controller's UART TX: its CRSF telemetry arrives here.
    telemetry_in: Wire,
    decoder: Decoder,
    fifo: VecDeque<(u8, Vec<u8>)>,
    since_stats: u32,
}

impl CrsfReceiver {
    pub fn new(params: ReceiverParams, uart: Wire, telemetry_in: Wire) -> Self {
        Self { params, uart, telemetry_in, decoder: Decoder::default(), fifo: VecDeque::new(), since_stats: 0 }
    }
}

impl Receiver for CrsfReceiver {
    fn ingest_telemetry(&mut self) {
        // Whatever Betaflight wrote this tick joins the FIFO (oldest dropped when full). No random draws: the
        // queue is a pure function of the frames offered and the slots flown.
        for frame in self.decoder.push(&self.telemetry_in.take(usize::MAX)) {
            if let Frame::Other { frame_type, payload, .. } = frame {
                if self.fifo.len() >= TELEMETRY_FIFO_LEN {
                    self.fifo.pop_front();
                }
                self.fifo.push_back((frame_type, payload));
            }
        }
    }

    fn emit(&mut self, delivery: &Delivery) {
        let Some(channels) = delivery.channels else { return };
        self.uart.write(&crsf::rc_channels_frame(&channels));
        self.since_stats += 1;
        if self.since_stats >= self.params.link_stats_interval_packets {
            self.since_stats = 0;
            let r = &delivery.report;
            let dbm_field = |dbm: f64| (-dbm).round().clamp(0.0, 255.0) as u8;
            self.uart.write(&crsf::link_statistics_frame(&LinkStatistics {
                uplink_rssi_1: dbm_field(r.uplink_rssi_dbm[0]),
                uplink_rssi_2: dbm_field(r.uplink_rssi_dbm[1]),
                uplink_lq: r.lq_pct.round().clamp(0.0, 100.0) as u8,
                uplink_snr: r.uplink_snr_db.round().clamp(-128.0, 127.0) as i8,
                active_antenna: r.antenna as u8,
                rf_mode: r.rf_mode,
                uplink_tx_power: r.tx_power_index,
                downlink_rssi: dbm_field(r.downlink_rssi_dbm),
                downlink_lq: r.downlink_lq_pct.round().clamp(0.0, 100.0) as u8,
                downlink_snr: r.downlink_snr_db.round().clamp(-128.0, 127.0) as i8,
            }));
        }
    }

    fn next_telemetry(&mut self) -> Option<(u8, Vec<u8>)> {
        self.fifo.pop_front()
    }
}
