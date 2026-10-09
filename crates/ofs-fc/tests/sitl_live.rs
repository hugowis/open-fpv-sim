//! Needs a built Betaflight SITL. Run with:
//!   OFS_SITL_LAUNCH="<path or wsl.exe -e path>" cargo test -p ofs-fc --test sitl_live -- --ignored --test-threads=1
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Model, Scheduler, Signal, SimError, StepCtx, Wire};
use ofs_fc::msp::{rc_channels_us, MspClient, MspParser, MSP_BATTERY_STATE, MSP_RC};
use ofs_fc::sitl::bridge::{BridgeConfig, SerialLink, SerialTap, SitlBridge};
use ofs_fc::sitl::codec::MSP_TCP_PORT;
use ofs_fc::sitl::esc_telemetry::EscTelemetry;
use ofs_fc::sitl::frames::Home;
use ofs_fc::sitl::net;
use ofs_fc::sitl::process::LaunchConfig;
use ofs_radio::crsf;

const QUAD_DIFF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.betaflight.diff");
const CRSF_DIFF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/crsf.betaflight.diff");
const OSD_DIFF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/osd.betaflight.diff");
const ESC_DIFF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/esc.betaflight.diff");

fn argv(var: &str) -> Vec<String> {
    std::env::var(var).unwrap_or_default().split_whitespace().map(String::from).collect()
}

/// A level, still quad on the bus and a bridge to a fresh SITL (working directory under `dir`).
/// `source`, if any, runs before the bridge in every tick.
fn still_quad(
    dir: &Path,
    diff: &str,
    serial: Vec<SerialLink>,
    taps: Vec<SerialTap>,
    source: impl FnOnce(&mut Bus) -> Option<Box<dyn Model>>,
) -> Scheduler {
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
    let source = source(&mut bus);
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
    let mut s = still_quad(dir.path(), QUAD_DIFF, vec![], vec![], |_| None);
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
    let mut s = still_quad(dir.path(), CRSF_DIFF, vec![link], vec![], |_| Some(Box::new(CrsfSource { uart, channels })));
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
    let mut s = still_quad(dir.path(), OSD_DIFF, vec![], vec![tap], |_| None);
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

/// Holds constant battery readings on the bus, like a battery model under a steady load.
struct FixedBattery {
    voltage: Signal<f64>,
    current: Signal<f64>,
    consumed: Signal<f64>,
}

impl Model for FixedBattery {
    fn name(&self) -> &str {
        "test.fixed_battery"
    }

    fn rate_divisor(&self) -> u32 {
        1
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        bus.set(self.voltage, 24.6);
        bus.set(self.current, 8.0);
        bus.set(self.consumed, 120.0);
        Ok(())
    }
}

/// Runs several models in order (a scheduler slot takes one `source`), each at its own rate divisor.
struct Both(Vec<Box<dyn Model>>);

impl Model for Both {
    fn name(&self) -> &str {
        "test.both"
    }

    fn rate_divisor(&self) -> u32 {
        1
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        for m in &mut self.0 {
            if ctx.tick % u64::from(m.rate_divisor()) == 0 {
                m.step(ctx, bus)?;
            }
        }
        Ok(())
    }
}

#[test]
#[ignore]
fn esc_telemetry_frames_become_battery_state() {
    let dir = tempfile::tempdir().unwrap();
    let uart = Wire::new(4096);
    let link = SerialLink { uart_index: 2, rx: uart.clone() }; // UART3
    let mut s = still_quad(dir.path(), ESC_DIFF, vec![link], vec![], |bus| {
        let battery = FixedBattery {
            voltage: bus.signal(names::BATTERY_VOLTAGE),
            current: bus.signal(names::BATTERY_CURRENT),
            consumed: bus.signal(names::BATTERY_CONSUMED),
        };
        let esc = EscTelemetry::new(uart.clone(), 4, 80, bus);
        Some(Box::new(Both(vec![Box::new(battery), Box::new(esc)])))
    });
    s.run_for(4.0).unwrap();
    let mut msp = MspClient::connect(SocketAddr::from(([127, 0, 0, 1], MSP_TCP_PORT)), Duration::from_secs(5)).unwrap();
    let reply = msp.request(MSP_BATTERY_STATE, &[], 500, || s.run_for(0.01)).unwrap();
    // MSP_BATTERY_STATE: cells u8, capacity u16, legacy volts u8, mAh drawn u16, amperage i16 (0.01 A), state u8,
    // voltage u16 (0.01 V).
    let p = &reply.payload;
    let (cells, mah) = (p[0], u16::from_le_bytes([p[4], p[5]]));
    let (amps, volts) = (f64::from(i16::from_le_bytes([p[6], p[7]])) / 100.0, f64::from(u16::from_le_bytes([p[9], p[10]])) / 100.0);
    println!("cells {cells} mAh {mah} A {amps} V {volts}");
    assert_eq!(cells, 6, "6S detected");
    assert!((volts - 24.6).abs() < 0.15, "voltage {volts}");
    assert!((amps - 8.0).abs() < 0.4, "current {amps}");
    assert!((i32::from(mah) - 120).abs() <= 6, "consumed {mah} mAh");
}
