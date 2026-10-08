//! Needs a built Betaflight SITL. Run with:
//!   OFS_SITL_LAUNCH="<path or wsl.exe -e path>" cargo test -p ofs-fc --test sitl_live -- --ignored --test-threads=1
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Model, Scheduler, SimError, StepCtx, Wire};
use ofs_fc::msp::{rc_channels_us, MspClient, MspParser, MSP_RC};
use ofs_fc::sitl::bridge::{BridgeConfig, SerialLink, SerialTap, SitlBridge};
use ofs_fc::sitl::codec::MSP_TCP_PORT;
use ofs_fc::sitl::frames::Home;
use ofs_fc::sitl::net;
use ofs_fc::sitl::process::LaunchConfig;
use ofs_radio::crsf;

const QUAD_DIFF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.betaflight.diff");
const CRSF_DIFF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/crsf.betaflight.diff");
const OSD_DIFF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/osd.betaflight.diff");

fn argv(var: &str) -> Vec<String> {
    std::env::var(var).unwrap_or_default().split_whitespace().map(String::from).collect()
}

/// A level, still quad on the bus and a bridge to a fresh SITL (working directory under `dir`).
/// `source`, if any, runs before the bridge in every tick.
fn still_quad(dir: &Path, diff: &str, serial: Vec<SerialLink>, taps: Vec<SerialTap>, source: Option<Box<dyn Model>>) -> Scheduler {
    let launch = argv("OFS_SITL_LAUNCH");
    assert!(!launch.is_empty(), "set OFS_SITL_LAUNCH");
    let mut cleanup = argv("OFS_SITL_CLEANUP");
    if cleanup.is_empty() {
        cleanup = net::default_cleanup(&launch);
    }
    let sitl_net = net::resolve(&launch, None, None).unwrap();
    let mut bus = Bus::new();
    let accel = bus.signal::<DVec3>(names::IMU_ACCEL);
    bus.set(accel, DVec3::new(0.0, 0.0, -9.80665));
    let att = bus.signal::<DQuat>(names::BODY_ATT);
    bus.set(att, DQuat::IDENTITY);
    let pressure = bus.signal::<f64>(names::BARO_PRESSURE);
    bus.set(pressure, 101_325.0);
    let bridge = SitlBridge::start(
        BridgeConfig {
            launch: LaunchConfig {
                launch,
                cleanup,
                workdir: dir.join("fc"),
                diff_file: diff.into(),
                startup_timeout: Duration::from_secs(15),
            },
            net: sitl_net,
            rate_divisor: 8,
            first_reply_timeout: Duration::from_secs(5),
            reply_timeout: Duration::from_millis(500),
            home: Home { lat_deg: 50.85, lon_deg: 4.35, alt_m: 30.0 },
            motor_count: 4,
            serial,
            taps,
        },
        &mut bus,
    )
    .unwrap();
    let mut s = Scheduler::new(8000, bus);
    if let Some(m) = source {
        s.add(m);
    }
    s.add(Box::new(bridge));
    s
}

#[test]
#[ignore]
fn still_quad_exchanges_packets_and_stays_disarmed() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = still_quad(dir.path(), QUAD_DIFF, vec![], vec![], None);
    s.run_for(2.0).unwrap();
    for i in 0..4 {
        assert_eq!(s.bus().get(s.bus().lookup::<f64>(&names::motor_cmd(i)).unwrap()), 0.0);
    }
}

/// Writes one CRSF RC frame per step, like a receiver at 500 Hz.
struct CrsfSource {
    uart: Wire,
    channels: [u16; crsf::CHANNEL_COUNT],
}

impl Model for CrsfSource {
    fn name(&self) -> &str {
        "test.crsf_source"
    }

    fn rate_divisor(&self) -> u32 {
        16
    }

    fn step(&mut self, _ctx: &StepCtx, _bus: &mut Bus) -> Result<(), SimError> {
        self.uart.write(&crsf::rc_channels_frame(&self.channels));
        Ok(())
    }
}

#[test]
#[ignore]
fn crsf_bytes_in_the_state_datagram_reach_betaflight() {
    let dir = tempfile::tempdir().unwrap();
    let uart = Wire::new(4096);
    let mut channels = [crsf::stick_ticks(0.0); crsf::CHANNEL_COUNT];
    channels[0] = crsf::stick_ticks(0.2); // roll -> 1600 us
    channels[1] = crsf::stick_ticks(-0.2); // pitch -> 1400 us
    channels[2] = crsf::throttle_ticks(0.0); // throttle -> 1000 us
    channels[3] = crsf::stick_ticks(0.0); // yaw -> 1500 us
    let link = SerialLink { uart_index: 1, rx: uart.clone() }; // UART2
    let mut s = still_quad(dir.path(), CRSF_DIFF, vec![link], vec![], Some(Box::new(CrsfSource { uart, channels })));
    s.run_for(2.0).unwrap();
    let mut msp = MspClient::connect(SocketAddr::from(([127, 0, 0, 1], MSP_TCP_PORT)), Duration::from_secs(5)).unwrap();
    let reply = msp.request(MSP_RC, &[], 500, || s.run_for(0.01)).unwrap();
    // MSP_RC lists roll, pitch, yaw, throttle (Betaflight's internal order).
    assert_eq!(&rc_channels_us(&reply)[..4], &[1600, 1400, 1500, 1000]);
}

#[test]
#[ignore]
fn displayport_frames_come_back_in_the_reply() {
    let dir = tempfile::tempdir().unwrap();
    let osd = Wire::new(65536);
    let tap = SerialTap { uart_index: 3, tx: osd.clone() }; // UART4
    let mut s = still_quad(dir.path(), OSD_DIFF, vec![], vec![tap], None);
    s.run_for(3.0).unwrap();
    let bytes = osd.take(usize::MAX);
    let frames = MspParser::default().push(&bytes);
    let displayport: Vec<_> = frames.iter().filter(|f| f.cmd == 182 && !f.error).collect();
    assert!(displayport.len() >= 20, "only {} DisplayPort frames in 3 s ({} bytes)", displayport.len(), bytes.len());
    assert!(displayport.iter().any(|f| f.payload.first() == Some(&3)), "no write-string frame");
    assert!(displayport.iter().any(|f| f.payload.first() == Some(&4)), "no draw-screen frame");
    let dropped = s.bus().lookup::<f64>(names::FC_SERIAL_DROPPED).map(|sig| s.bus().get(sig));
    assert_eq!(dropped, Some(0.0), "SITL dropped TX bytes");
    println!("DisplayPort: {} frames, {} bytes in 3 s", displayport.len(), bytes.len());
}
