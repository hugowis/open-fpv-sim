//! Needs a built Betaflight SITL. Run with:
//!   OFS_SITL_LAUNCH="<path or wsl.exe -e path>" cargo test -p ofs-video --test vtx_live -- --ignored --test-threads=1
mod common;

use std::net::SocketAddr;
use std::time::Duration;

use common::still_quad;
use ofs_core::{names, Scheduler, Wire};
use ofs_fc::msp::MspClient;
use ofs_fc::sitl::bridge::{SerialLink, SerialTap};
use ofs_fc::sitl::codec::MSP_TCP_PORT;
use ofs_video::vtx::{band_index, VtxModel, VtxParams};

const VTX_DIFF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/vtx.betaflight.diff");
const MSP_VTX_CONFIG: u8 = 88;
const MSP_SET_VTX_CONFIG: u8 = 89;

fn signal(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

#[test]
#[ignore]
fn betaflight_accepts_the_smartaudio_vtx_and_follows_msp_changes() {
    let dir = tempfile::tempdir().unwrap();
    let to_vtx = Wire::new(4096); // Betaflight's UART5 TX (its requests)
    let from_vtx = Wire::new(4096); // the VTX's replies into Betaflight's UART5 RX
    let tap = SerialTap { uart_index: 4, tx: to_vtx.clone() };
    let link = SerialLink { uart_index: 4, rx: from_vtx.clone() };
    let mut s = still_quad(dir.path(), VTX_DIFF, vec![link], vec![tap], |bus| {
        let params = VtxParams {
            power_levels_mw: vec![25, 200, 600, 1000],
            power_levels_dbm: vec![14, 23, 28, 30],
            default_band: band_index("R").unwrap(),
            default_channel: 1,
            default_power_index: 1,
            reply_latency_s: 0.005,
        };
        Some(Box::new(VtxModel::new(params, to_vtx, from_vtx, 1, bus)))
    });
    s.run_for(5.0).unwrap();

    let mut msp = MspClient::connect(SocketAddr::from(([127, 0, 0, 1], MSP_TCP_PORT)), Duration::from_secs(5)).unwrap();
    let cfg = msp.request(MSP_VTX_CONFIG, &[], 500, || s.run_for(0.01)).unwrap();
    let p = &cfg.payload;
    println!("MSP_VTX_CONFIG: {p:?}");
    assert!(p.len() >= 15, "short MSP_VTX_CONFIG reply: {p:?}");
    assert_eq!(p[7], 1, "Betaflight does not consider the SmartAudio device ready (reply side not accepted): {p:?}");
    assert_eq!((p[1], p[2], p[3]), (5, 1, 2), "band, channel, power index of the shipped diff: {p:?}");
    assert_eq!(u16::from_le_bytes([p[5], p[6]]), 5658, "settings frequency of R1: {p:?}");

    // What the Configurator VTX tab sends: band R (5) channel 3 = (5 - 1) * 8 + (3 - 1) = 34, power level 3, pit off.
    msp.request(MSP_SET_VTX_CONFIG, &[34, 0, 3, 0], 500, || s.run_for(0.01)).unwrap();
    s.run_for(2.0).unwrap();
    assert_eq!(signal(&s, names::VTX_FREQ_MHZ), 5732.0, "R3");
    assert_eq!((signal(&s, names::VTX_BAND), signal(&s, names::VTX_CHANNEL)), (5.0, 3.0));
    assert_eq!(signal(&s, names::VTX_POWER_MW), 600.0);

    msp.request(MSP_SET_VTX_CONFIG, &[34, 0, 3, 1], 500, || s.run_for(0.01)).unwrap();
    s.run_for(2.0).unwrap();
    assert_eq!(signal(&s, names::VTX_PIT), 1.0, "pit mode on");
    msp.request(MSP_SET_VTX_CONFIG, &[34, 0, 3, 0], 500, || s.run_for(0.01)).unwrap();
    s.run_for(2.0).unwrap();
    assert_eq!(signal(&s, names::VTX_PIT), 0.0, "pit mode off");
}
