//! Characterization of the ELRS radio: the bytes it writes to the flight controller's UART, the signals it
//! publishes and the telemetry the handset decodes, hashed per scenario. The digests are per OS
//! (`tests/golden/elrs.windows.digests`, `elrs.linux.digests`) because libm's f64 maths differs between Windows and
//! Linux. Each was generated from the pre-split `ElrsLink` (M5 Task 1); the split into handset / link / receiver
//! must reproduce each OS's own file exactly. Regenerate only on purpose, on the OS whose file you mean to change:
//! `OFS_GOLDEN_WRITE=1 cargo test -p ofs-radio --test golden`.
use std::path::PathBuf;

use glam::DVec3;
use ofs_core::shape::Shape;
use ofs_core::{names, Bus, Scheduler, Wire};
use ofs_radio::crsf::{
    crc8_dvb_s2, FRAMETYPE_ATTITUDE, FRAMETYPE_BATTERY_SENSOR, FRAMETYPE_FLIGHT_MODE,
};
use ofs_radio::chain::RadioChain;
use ofs_radio::elrs::LinkParams;
use ofs_radio::receiver::ReceiverParams;
use ofs_radio::telemetry::FcTelemetryHandle;
use ofs_rf::propagation::{Antenna, AntennaKind, Obstacle, Polarization};

const BASE_HZ: u32 = 8000;
/// 0.4 s: 200 packets at 500 Hz.
const PHASE_TICKS: u64 = 3200;

/// FNV-1a 64 over everything a scenario produces.
struct Digest(u64);

impl Digest {
    fn new() -> Self {
        Digest(0xcbf2_9ce4_8422_2325)
    }
    fn bytes(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn f64(&mut self, v: f64) {
        self.bytes(&v.to_bits().to_le_bytes());
    }
}

fn rig(
    params: LinkParams,
    stats_interval: u32,
    seed: u64,
) -> (Scheduler, Wire, Wire, FcTelemetryHandle) {
    let mut bus = Bus::new();
    let uart = Wire::new(1 << 20);
    let telemetry_in = Wire::new(1 << 16);
    let receiver = ReceiverParams { link_stats_interval_packets: stats_interval };
    let chain = RadioChain::elrs(params, receiver, BASE_HZ, seed, uart.clone(), telemetry_in.clone(), &mut bus);
    let handle = chain.fc_telemetry_handle();
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let mut s = Scheduler::new(BASE_HZ, bus);
    s.add(Box::new(chain));
    (s, uart, telemetry_in, handle)
}

fn set(s: &mut Scheduler, name: &str, v: f64) {
    let sig = s.bus_mut().signal::<f64>(name);
    s.bus_mut().set(sig, v);
}

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

fn set_pos(s: &mut Scheduler, pos: DVec3) {
    let sig = s.bus_mut().lookup::<DVec3>(names::BODY_POS_NED).unwrap();
    s.bus_mut().set(sig, pos);
}

/// One CRC-valid CRSF frame: [address 0xC8][len][type][payload][crc8 poly 0xD5].
fn frame(frame_type: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = vec![0xC8, (payload.len() + 2) as u8, frame_type];
    f.extend_from_slice(payload);
    f.push(crc8_dvb_s2(&f[2..]));
    f
}

fn battery_frame(voltage_v: f64, current_a: f64, index: u8) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&((voltage_v * 10.0) as u16).to_be_bytes());
    p.extend_from_slice(&((current_a * 10.0) as u16).to_be_bytes());
    p.extend_from_slice(&[0, 0, index]);
    p.push(100);
    frame(FRAMETYPE_BATTERY_SENSOR, &p)
}

fn attitude_frame(pitch_deg: f64, roll_deg: f64, yaw_deg: f64) -> Vec<u8> {
    let rad = |d: f64| (d.to_radians() * 10_000.0).round() as i16;
    let mut p = Vec::new();
    for v in [rad(pitch_deg), rad(roll_deg), rad(yaw_deg)] {
        p.extend_from_slice(&v.to_be_bytes());
    }
    frame(FRAMETYPE_ATTITUDE, &p)
}

/// Six phases of 0.4 s: the quad at 30 m, 3 km, then three distances at the sensitivity edge of the link (where
/// packets drop part of the time; `edge`), 100 m between them, with changing sticks, the link-loss fault in phase 1,
/// the transmitter off in phase 2, a 30 % loss fault in phase 3, a burst fault in phase 5, and telemetry frames
/// offered every phase (twenty batteries in phase 4, to overflow the 16-frame queue).
fn run_scenario(params: LinkParams, stats_interval: u32, seed: u64, edge: [f64; 3]) -> u64 {
    let (mut s, uart, telemetry_in, fc) = rig(params, stats_interval, seed);
    let mut d = Digest::new();
    let norths = [30.0, 3_000.0, edge[0], edge[1], 100.0, edge[2]];
    for (phase, north) in norths.iter().enumerate() {
        let p = phase as f64;
        set_pos(&mut s, DVec3::new(*north, 0.0, -1.7));
        set(&mut s, names::RC_ROLL, 0.1 * p - 0.25);
        set(&mut s, names::RC_PITCH, 0.2 - 0.07 * p);
        set(&mut s, names::RC_YAW, -0.3 + 0.05 * p);
        set(&mut s, names::RC_THROTTLE, 0.15 * p);
        set(
            &mut s,
            &names::rc_aux(0),
            if phase % 2 == 0 { -1.0 } else { 1.0 },
        );
        set(
            &mut s,
            names::FAULT_RADIO_LINK_LOSS,
            if phase == 1 { 1.0 } else { 0.0 },
        );
        set(
            &mut s,
            names::RADIO_TX_ENABLED,
            if phase == 2 { 0.0 } else { 1.0 },
        );
        set(
            &mut s,
            names::FAULT_RADIO_LOSS_FRACTION,
            if phase == 3 { 0.3 } else { 0.0 },
        );
        set(
            &mut s,
            names::FAULT_RADIO_BURST_ON_S,
            if phase == 5 { 0.05 } else { 0.0 },
        );
        set(
            &mut s,
            names::FAULT_RADIO_BURST_OFF_S,
            if phase == 5 { 0.05 } else { 0.0 },
        );
        if phase == 4 {
            for i in 0..20u8 {
                telemetry_in.write(&battery_frame(20.0 + f64::from(i), 1.0, i));
            }
        } else {
            telemetry_in.write(&battery_frame(24.0 + p, p, phase as u8));
            telemetry_in.write(&attitude_frame(p, -p, 10.0 * p));
            telemetry_in.write(&frame(FRAMETYPE_FLIGHT_MODE, b"ACRO\0"));
        }
        for _ in 0..PHASE_TICKS {
            s.step().unwrap();
            d.bytes(&uart.take(usize::MAX));
            for name in [
                names::RADIO_LINK_UP,
                names::RADIO_LQ,
                names::RADIO_RSSI,
                names::RADIO_SNR,
                names::RADIO_ANTENNA,
                names::RADIO_DOWNLINK_LQ,
            ] {
                d.f64(get(&s, name));
            }
        }
    }
    let t = fc.latest();
    for v in [
        t.battery_v,
        t.battery_a,
        t.battery_mah,
        t.roll_deg,
        t.pitch_deg,
        t.yaw_deg,
        t.last_frame_s,
    ] {
        d.f64(v);
    }
    d.bytes(t.flight_mode.as_bytes());
    d.bytes(&t.frame_count.to_le_bytes());
    d.0
}

fn dipole(axis: DVec3) -> Antenna {
    Antenna {
        kind: AntennaKind::Omni,
        gain_dbi: 2.0,
        polarization: Polarization::Linear,
        axis,
    }
}

/// (name, params, stats interval in packets, seed). Covers latency 0, latency > 1, one and two antennas, fading
/// and ground bounce, an obstacle, the packet rates that divide the 8 kHz base (50, 250, 500), and a weak link.
fn scenarios() -> Vec<(&'static str, LinkParams, u32, u64, [f64; 3])> {
    let faded = |rate: u32| LinkParams {
        fading: true,
        ground_bounce: true,
        ..LinkParams::ideal(rate)
    };
    let two_antennas = {
        let mut p = faded(500);
        p.quad_antennas.push(Antenna {
            kind: AntennaKind::Patch {
                beamwidth_deg: 60.0,
            },
            gain_dbi: 8.0,
            polarization: Polarization::Linear,
            axis: DVec3::NEG_X,
        });
        p
    };
    let obstructed = {
        let mut p = LinkParams {
            ground_bounce: true,
            ..LinkParams::ideal(50)
        };
        let shape = Shape::Box {
            center: DVec3::new(50.0, 0.0, -10.0),
            half: DVec3::new(10.0, 10.0, 10.0),
        };
        p.obstacles = vec![Obstacle {
            shape: shape.rooted(),
            rf_loss_db: 25.0,
        }];
        p
    };
    // Phase 2, 3 and 5 distances (m): at the sensitivity edge of each link, where packets drop part of the time.
    let far = [42_000.0, 42_800.0, 41_500.0];
    vec![
        ("ideal-500", LinkParams::ideal(500), 50, 1, far),
        (
            "latency-0-250",
            LinkParams {
                latency_packets: 0,
                ..LinkParams::ideal(250)
            },
            10,
            2,
            far,
        ),
        (
            "latency-5-500",
            LinkParams {
                latency_packets: 5,
                ..LinkParams::ideal(500)
            },
            25,
            3,
            far,
        ),
        ("fading-bounce-500", faded(500), 50, 4, [3_500.0, 3_600.0, 3_400.0]),
        ("two-antennas-fading", two_antennas, 20, 5, [5_000.0, 5_100.0, 4_900.0]),
        ("obstacle-50hz", obstructed, 5, 6, [1_700.0, 1_750.0, 1_650.0]),
        (
            "weak-10mw",
            LinkParams {
                tx_power_mw: 10.0,
                ..faded(250)
            },
            50,
            7,
            [1_600.0, 1_650.0, 1_550.0],
        ),
        (
            "crossed-dipole",
            LinkParams {
                quad_antennas: vec![dipole(DVec3::Y)],
                ..faded(500)
            },
            50,
            8,
            [1_000.0, 1_050.0, 950.0],
        ),
    ]
}

/// The golden file for this OS, or None where none was recorded.
fn golden_path() -> Option<PathBuf> {
    let name = if cfg!(target_os = "windows") {
        "elrs.windows.digests"
    } else if cfg!(target_os = "linux") {
        "elrs.linux.digests"
    } else {
        return None;
    };
    Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name))
}

#[test]
fn the_elrs_radio_reproduces_its_recorded_digests() {
    let Some(golden) = golden_path() else {
        println!("radio goldens exist for Windows and Linux only; skipping on this OS");
        return;
    };
    let current: Vec<String> = scenarios()
        .into_iter()
        .map(|(name, params, interval, seed, edge)| {
            format!("{name} {:016x}", run_scenario(params, interval, seed, edge))
        })
        .collect();
    let text = current.join("\n") + "\n";
    if std::env::var_os("OFS_GOLDEN_WRITE").is_some() {
        std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
        std::fs::write(&golden, &text).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&golden)
        .expect("tests/golden/elrs.<os>.digests (run once with OFS_GOLDEN_WRITE=1)");
    let expected = expected.replace("\r\n", "\n");
    assert_eq!(
        text, expected,
        "the radio's output changed; the split must be byte-identical"
    );
}
