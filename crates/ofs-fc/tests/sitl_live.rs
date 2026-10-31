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
            ports: ofs_fc::sitl::codec::PortMap::default(),
            rate_divisor: 8,
            first_reply_timeout: Duration::from_secs(5),
            reply_timeout: Duration::from_millis(500),
            home: Home { lat_deg: 50.85, lon_deg: 4.35, alt_m: 30.0 },
            motor_count: 4,
            serial,
            erpm: Vec::new(),
            pole_pairs: Vec::new(),
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

/// `--port-offset N` shifts every port SITL binds by N (UDP 9001 to 9004, TCP 5761 and up), so a second SITL can run
/// beside the first. The process is launched alone: the printed bind lines are the evidence.
#[test]
#[ignore]
fn a_sitl_launched_with_a_port_offset_binds_the_shifted_ports() {
    use ofs_fc::sitl::process::SitlProcess;
    let dir = tempfile::tempdir().unwrap();
    let mut launch = argv("OFS_SITL_LAUNCH");
    assert!(!launch.is_empty(), "set OFS_SITL_LAUNCH");
    launch.extend(["--port-offset".to_string(), "10".to_string()]);
    let cleanup = net::default_cleanup(&launch);
    let process = SitlProcess::start(&LaunchConfig {
        launch,
        cleanup,
        workdir: dir.path().join("fc"),
        diff_file: QUAD_DIFF.into(),
        startup_timeout: Duration::from_secs(15),
    })
    .unwrap();
    let log = process.log_tail();
    for expected in ["bind port 5771 for UART1", "start UDP server @9013", "start UDP server for RC input @9014"] {
        assert!(log.contains(expected), "{expected:?} missing from:\n{log}");
    }
    assert!(!log.contains("port 5761") && !log.contains("@9003"), "nothing binds the unshifted ports:\n{log}");
}

/// The default cleanup of a slot finds and stops a SITL that is really running: the production path behind "a simulator
/// was killed and left its SITL" (on Linux the script reads `/proc`, on macOS it uses `ps`), against the real binary
/// and its real process name rather than a decoy. The test starts the SITL itself and runs the cleanup by hand.
#[cfg(unix)]
#[test]
#[ignore]
fn the_slot_cleanup_stops_a_running_sitl() {
    use ofs_fc::sitl::process::{run_cleanup, SitlProcess};
    let dir = tempfile::tempdir().unwrap();
    let mut launch = argv("OFS_SITL_LAUNCH");
    assert!(!launch.is_empty(), "set OFS_SITL_LAUNCH");
    launch.extend(["--port-offset".to_string(), "20".to_string()]);
    let cleanup = net::default_cleanup(&launch);
    let mut process = SitlProcess::start(&LaunchConfig {
        launch,
        cleanup: Vec::new(), // the cleanup under test must be the only thing that stops it
        workdir: dir.path().join("fc"),
        diff_file: QUAD_DIFF.into(),
        startup_timeout: Duration::from_secs(15),
    })
    .unwrap();
    assert!(process.exit_status().is_none(), "the SITL is running");
    run_cleanup(&cleanup);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while process.exit_status().is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(process.exit_status().is_some(), "the slot's cleanup stopped the running SITL");
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
    assert_eq!(&rc_channels_us(&reply).unwrap()[..4], &[1600, 1400, 1500, 1000]);
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

/// m3a debt: `serial_overflow` had only ever fired from a counter. Betaflight's OSD writes hundreds of DisplayPort
/// bytes a second into UART4; a 64-byte tap nobody drains must overflow, and every byte it drops is counted in
/// `fc.serial_dropped_bytes` (which the session turns into the `serial_overflow` event: ofs-sim's
/// `vtx_changes_and_dropped_bytes_become_events`).
#[test]
#[ignore]
fn a_full_tap_counts_the_bytes_betaflight_lost() {
    let dir = tempfile::tempdir().unwrap();
    let osd = Wire::new(64);
    let tap = SerialTap { uart_index: 3, tx: osd.clone() }; // UART4, never drained
    let mut s = still_quad(dir.path(), OSD_DIFF, vec![], vec![tap], |_| None);
    s.run_for(3.0).unwrap();
    let dropped = s.bus().get(s.bus().lookup::<f64>(names::FC_SERIAL_DROPPED).unwrap());
    assert!(osd.dropped() > 0, "3 s of DisplayPort cannot fit in 64 bytes");
    // The bridge counts a wire's drops at the start of each step, before routing that step's reply, so the newest
    // reply's drops are counted one step later: the count trails the wire by at most one exchange.
    assert!(dropped > 0.0, "the drops reach fc.serial_dropped_bytes");
    assert!(dropped as u64 <= osd.dropped(), "never more than the wire lost: {dropped} > {}", osd.dropped());
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
        let esc = EscTelemetry::new(uart.clone(), 4, 80, 7.0, bus);
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

/// The port-bind flake (docs/superpowers/history/closeout-evidence.md): serial_tcp.c bound each UART once and
/// kept booting on failure. The patch retries for about half a second and names the errno, so a port released
/// within the window self-heals and a permanently held one fails the start with the attempts on the record.
#[cfg(not(target_os = "macos"))]
#[test]
#[ignore]
fn a_held_serial_port_is_retried_then_refused_with_the_errno() {
    let launch = argv("OFS_SITL_LAUNCH");
    assert!(!launch.is_empty(), "set OFS_SITL_LAUNCH");
    let dir = tempfile::tempdir().unwrap();
    let cfg = LaunchConfig {
        launch,
        cleanup: vec![], // the holder is this test, not a stray SITL
        workdir: dir.path().join("fc"),
        diff_file: OSD_DIFF.into(),
        startup_timeout: Duration::from_secs(15),
    };
    std::fs::create_dir_all(&cfg.workdir).unwrap();
    std::fs::copy(OSD_DIFF, cfg.workdir.join("betaflight.diff")).unwrap();
    let _holder = std::net::TcpListener::bind("127.0.0.1:5761").unwrap();
    let err = ofs_fc::sitl::process::SitlProcess::start(&cfg).err().expect("a held MSP port fails the start");
    let text = err.to_string();
    assert!(text.contains("bind port 5761 for UART1 failed!! after 20 attempts"), "the spent retries are on the record: {text}");
    assert!(text.contains("errno"), "the failure names the errno: {text}");
}

/// macOS's BSD `SO_REUSEADDR` lets SITL's wildcard bind coexist with a listener on 127.0.0.1:5761; the
/// documented behaviour is that the real SITL still starts. Pinning that here makes the platform difference
/// visible instead of returning early and showing a pass. The simulator's own UDP 9003 probe, and the fake
/// SITL error test, cover a genuinely stale process.
#[cfg(target_os = "macos")]
#[test]
#[ignore]
fn a_held_tcp_port_does_not_stop_sitl_on_macos() {
    let launch = argv("OFS_SITL_LAUNCH");
    assert!(!launch.is_empty(), "set OFS_SITL_LAUNCH");
    let dir = tempfile::tempdir().unwrap();
    let cfg = LaunchConfig {
        launch,
        cleanup: vec![],
        workdir: dir.path().join("fc"),
        diff_file: OSD_DIFF.into(),
        startup_timeout: Duration::from_secs(15),
    };
    std::fs::create_dir_all(&cfg.workdir).unwrap();
    std::fs::copy(OSD_DIFF, cfg.workdir.join("betaflight.diff")).unwrap();
    let _holder = std::net::TcpListener::bind("127.0.0.1:5761").unwrap();
    let started = ofs_fc::sitl::process::SitlProcess::start(&cfg);
    assert!(
        started.is_ok(),
        "macOS lets SITL bind alongside the holder: {:?}",
        started.err().map(|e| e.to_string())
    );
}

/// Writes one iBUS RC frame every 7 ms (56 base steps at 8 kHz), like a FlySky receiver.
struct IbusSource {
    uart: Wire,
    channels: [u16; crsf::CHANNEL_COUNT],
}

impl Model for IbusSource {
    fn name(&self) -> &str {
        "test.ibus_source"
    }

    fn rate_divisor(&self) -> u32 {
        56
    }

    fn step(&mut self, _ctx: &StepCtx, _bus: &mut Bus) -> Result<(), SimError> {
        self.uart.write(&ofs_radio::ibus::rc_frame(&self.channels));
        Ok(())
    }
}

/// Needs a SITL built from a patch that defines `USE_SERIALRX_IBUS` (the default `ofs-sitl.patch` since #8).
#[test]
#[ignore]
fn ibus_bytes_in_the_state_datagram_reach_betaflight() {
    let dir = tempfile::tempdir().unwrap();
    let uart = Wire::new(4096);
    let mut channels = [crsf::stick_ticks(0.0); crsf::CHANNEL_COUNT];
    channels[0] = crsf::stick_ticks(0.2); // roll -> 1600 us
    channels[1] = crsf::stick_ticks(-0.2); // pitch -> 1400 us
    channels[2] = crsf::throttle_ticks(0.0); // throttle -> 1000 us
    channels[3] = crsf::stick_ticks(0.0); // yaw -> 1500 us
    let link = SerialLink { uart_index: 1, rx: uart.clone() }; // UART2
    let ibus_diff = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/ibus.betaflight.diff");
    let mut s = still_quad(dir.path(), ibus_diff, vec![link], vec![], |_| Some(Box::new(IbusSource { uart, channels })));
    s.run_for(2.0).unwrap();
    let mut msp = MspClient::connect(SocketAddr::from(([127, 0, 0, 1], MSP_TCP_PORT)), Duration::from_secs(5)).unwrap();
    let reply = msp.request(MSP_RC, &[], 500, || s.run_for(0.01)).unwrap();
    println!("MSP_RC over iBUS: {:?}", &rc_channels_us(&reply).unwrap()[..8]);
    assert_eq!(&rc_channels_us(&reply).unwrap()[..4], &[1600, 1400, 1500, 1000]);
}
