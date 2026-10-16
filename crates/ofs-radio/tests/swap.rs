//! The swap proof (spec §7.4): a `Link` defined in this file, with no change to the core, builds a chain and
//! drives it through failsafe and recovery.
use glam::DVec3;
use ofs_core::{names, Bus, Scheduler, StepCtx, Wire};
use ofs_radio::chain::RadioChain;
use ofs_radio::crsf::{Decoder, Frame};
use ofs_radio::joystick::JoystickHandset;
use ofs_radio::parts::{Delivery, Link, LinkReport, RfFrontEnd, TxFrame};
use ofs_radio::receiver::{CrsfReceiver, ReceiverParams};

const BASE_HZ: u32 = 8000;

/// Loses every packet for the first `dead_packets`, then delivers every one: the simplest possible outage.
struct OutageLink {
    packets: u32,
    dead_packets: u32,
}

impl Link for OutageLink {
    fn name(&self) -> &str {
        "radio.test-outage"
    }

    fn packet_rate_hz(&self) -> u32 {
        500
    }

    fn deliver(&mut self, _ctx: &StepCtx, _bus: &Bus, tx: &TxFrame) -> Delivery {
        self.packets += 1;
        let up = tx.transmitting && self.packets > self.dead_packets;
        Delivery {
            channels: up.then_some(tx.channels),
            downlink_slot: false,
            report: LinkReport {
                link_up: up,
                lq_pct: if up { 100.0 } else { 0.0 },
                rssi_dbm: -60.0,
                snr_db: 30.0,
                antenna: 0,
                downlink_lq_pct: if up { 100.0 } else { 0.0 },
                uplink_rssi_dbm: [-60.0; 2],
                uplink_snr_db: 30.0,
                downlink_rssi_dbm: -60.0,
                downlink_snr_db: 30.0,
                rf_mode: 0,
                tx_power_index: 0,
            },
        }
    }
}

#[test]
fn a_link_written_outside_the_crate_drives_the_chain_through_an_outage_and_back() {
    let mut bus = Bus::new();
    let uart = Wire::new(1 << 20);
    let front_end = RfFrontEnd { position: DVec3::ZERO, antennas: Vec::new(), tx_power_mw: 250.0 };
    let handset = JoystickHandset::new(front_end, &mut bus);
    let receiver = CrsfReceiver::new(ReceiverParams::default(), uart.clone(), Wire::new(1 << 16));
    let chain = RadioChain::new(Box::new(handset), Box::new(OutageLink { packets: 0, dead_packets: 100 }), Box::new(receiver), BASE_HZ, &mut bus);
    assert_eq!(ofs_core::Model::name(&chain), "radio.test-outage", "the chain takes its link's name");
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let mut s = Scheduler::new(BASE_HZ, bus);
    s.add(Box::new(chain));

    s.run_for(0.2).unwrap(); // 100 packets of outage
    assert!(Decoder::default().push(&uart.take(usize::MAX)).is_empty(), "failsafe: the receiver went silent");
    assert_eq!(s.bus().get(s.bus().lookup::<f64>(names::RADIO_LINK_UP).unwrap()), 0.0);

    s.run_for(0.2).unwrap(); // recovery
    let frames = Decoder::default().push(&uart.take(usize::MAX));
    assert_eq!(frames.iter().filter(|f| matches!(f, Frame::RcChannels(_))).count(), 100, "one frame per packet again");
    assert_eq!(s.bus().get(s.bus().lookup::<f64>(names::RADIO_LINK_UP).unwrap()), 1.0);
}
