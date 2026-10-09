//! Assembles a quad from its config into a scheduler and exposes sticks in, state out.
use std::f64::consts::PI;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use glam::{DQuat, DVec3};
use ofs_config::{FcKind, QuadConfig};
use ofs_core::rng::fnv1a64;
use ofs_core::{names, Bus, Model, Scheduler, Signal, SimError, Wire};
use ofs_electrical::battery::{Battery, BatteryParams};
use ofs_electrical::esc_motor::{EscMotor, EscParams, MotorParams};
use ofs_fc::open_loop::OpenLoopFc;
use ofs_fc::sitl::bridge::{BridgeConfig, SerialLink, SerialTap, SitlBridge};
use ofs_fc::sitl::codec::MSP_TCP_PORT;
use ofs_fc::sitl::esc_telemetry::EscTelemetry;
use ofs_fc::sitl::frames::Home;
use ofs_fc::sitl::net;
use ofs_fc::sitl::process::LaunchConfig;
use ofs_physics::propeller::{PropParams, Propeller};
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, MotorMount, RigidBody};
use ofs_radio::elrs::{ElrsLink, LinkParams};
use ofs_sensors::baro::{Baro, BaroParams};
use ofs_sensors::imu::{Imu, ImuParams};
use ofs_video::osd::{OsdFrame, OsdHandle, OsdModel};
use ofs_video::vtx::{band_index, VtxModel, VtxParams};

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub seed: u64,
    /// Per-quad firmware working directories live under here.
    pub data_dir: PathBuf,
    pub fc_override: Option<FcKind>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sticks {
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
    pub throttle: f64,
    pub aux: [f64; names::RC_AUX_COUNT],
}

impl Default for Sticks {
    fn default() -> Self {
        Self { roll: 0.0, pitch: 0.0, yaw: 0.0, throttle: 0.0, aux: [-1.0; names::RC_AUX_COUNT] }
    }
}

/// What the radio receiver reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadioState {
    /// The transmitter is on (a pilot or script is connected).
    pub tx_enabled: bool,
    pub link_up: bool,
    pub lq_pct: f64,
    pub rssi_dbm: f64,
}

/// What the VTX model publishes (all zero without a `[vtx]` section or without Betaflight).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VtxInfo {
    pub present: bool,
    /// 1..=6 (A, B, E, F, R, L), 0 in user-frequency mode.
    pub band: u8,
    /// 1..=8, 0 in user-frequency mode.
    pub channel: u8,
    pub freq_mhz: u32,
    pub power_mw: u32,
    pub pit_mode: bool,
}

/// Faults a script can inject (spec §6.2). The v1 catalog completes in M4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Every radio uplink packet is lost: the receiver goes silent and Betaflight fails safe.
    RadioLinkLoss,
}

/// The receiver's UART buffer. With no flight controller draining it (open-loop FC), the oldest bytes are
/// dropped like a UART overrun.
const RECEIVER_UART_CAPACITY: usize = 4096;

/// Buffers between Betaflight's UARTs and the video models. Large enough for a few ticks of bursts; when a consumer
/// falls behind, the oldest bytes are dropped like a UART overrun.
const ESC_UART_CAPACITY: usize = 1024;
const VIDEO_TAP_CAPACITY: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct VehicleState {
    pub time_s: f64,
    pub pos_ned_m: DVec3,
    pub vel_ned_mps: DVec3,
    pub att: DQuat,
    pub rate_frd_radps: DVec3,
    pub battery_voltage_v: f64,
    pub battery_current_a: f64,
    pub motor_rpm: Vec<f64>,
    pub motor_cmd: Vec<f64>,
    pub radio: RadioState,
    /// Betaflight reboots so far (e.g. after a Configurator save); always 0 without SITL.
    pub fc_restarts: u32,
    pub vtx: VtxInfo,
    /// TX bytes Betaflight's UART capture dropped because a consumer fell behind.
    pub serial_dropped_bytes: u64,
}

struct Handles {
    pos: Signal<DVec3>,
    vel: Signal<DVec3>,
    att: Signal<DQuat>,
    rate: Signal<DVec3>,
    vbat: Signal<f64>,
    ibat: Signal<f64>,
    omega: Vec<Signal<f64>>,
    cmd: Vec<Signal<f64>>,
    roll: Signal<f64>,
    pitch: Signal<f64>,
    yaw: Signal<f64>,
    throttle: Signal<f64>,
    aux: Vec<Signal<f64>>,
    tx_enabled: Signal<f64>,
    link_up: Signal<f64>,
    lq: Signal<f64>,
    rssi: Signal<f64>,
    fault_radio_loss: Signal<f64>,
    fc_restarts: Signal<f64>,
    vtx_present: Signal<f64>,
    vtx_band: Signal<f64>,
    vtx_channel: Signal<f64>,
    vtx_freq: Signal<f64>,
    vtx_power: Signal<f64>,
    vtx_pit: Signal<f64>,
    serial_dropped: Signal<f64>,
}

impl Handles {
    fn register(bus: &mut Bus, motors: usize) -> Self {
        Self {
            pos: bus.signal(names::BODY_POS_NED),
            vel: bus.signal(names::BODY_VEL_NED),
            att: bus.signal(names::BODY_ATT),
            rate: bus.signal(names::BODY_RATE_FRD),
            vbat: bus.signal(names::BATTERY_VOLTAGE),
            ibat: bus.signal(names::BATTERY_CURRENT),
            omega: (0..motors).map(|i| bus.signal(&names::motor_omega(i))).collect(),
            cmd: (0..motors).map(|i| bus.signal(&names::motor_cmd(i))).collect(),
            roll: bus.signal(names::RC_ROLL),
            pitch: bus.signal(names::RC_PITCH),
            yaw: bus.signal(names::RC_YAW),
            throttle: bus.signal(names::RC_THROTTLE),
            aux: (0..names::RC_AUX_COUNT).map(|i| bus.signal(&names::rc_aux(i))).collect(),
            tx_enabled: bus.signal(names::RADIO_TX_ENABLED),
            link_up: bus.signal(names::RADIO_LINK_UP),
            lq: bus.signal(names::RADIO_LQ),
            rssi: bus.signal(names::RADIO_RSSI),
            fault_radio_loss: bus.signal(names::FAULT_RADIO_LINK_LOSS),
            fc_restarts: bus.signal(names::FC_RESTARTS),
            vtx_present: bus.signal(names::VTX_PRESENT),
            vtx_band: bus.signal(names::VTX_BAND),
            vtx_channel: bus.signal(names::VTX_CHANNEL),
            vtx_freq: bus.signal(names::VTX_FREQ_MHZ),
            vtx_power: bus.signal(names::VTX_POWER_MW),
            vtx_pit: bus.signal(names::VTX_PIT),
            serial_dropped: bus.signal(names::FC_SERIAL_DROPPED),
        }
    }
}

pub struct Vehicle {
    scheduler: Scheduler,
    h: Handles,
    sitl: bool,
    osd: Option<OsdHandle>,
}

/// Per-quad firmware directory: `<data_dir>/<quad file stem>-<hash of the quad file's path>`, so quads with
/// the same file name in different directories never share an EEPROM.
pub fn firmware_dir(data_dir: &Path, quad_path: &Path) -> PathBuf {
    let full = std::fs::canonicalize(quad_path)
        .or_else(|_| std::path::absolute(quad_path))
        .unwrap_or_else(|_| quad_path.to_path_buf());
    let stem = quad_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "quad".into());
    let hash = fnv1a64(full.to_string_lossy().as_bytes()) as u32;
    data_dir.join(format!("{stem}-{hash:08x}"))
}

fn v3(a: [f64; 3]) -> DVec3 {
    DVec3::from_array(a)
}

fn pairs(t: &[[f64; 2]]) -> Vec<(f64, f64)> {
    t.iter().map(|r| (r[0], r[1])).collect()
}

/// The video devices a quad file asks for, wired to Betaflight's UARTs.
struct VideoDevices {
    /// Run before the flight controller each tick (they write bytes it reads in this tick's exchange).
    before_fc: Vec<Box<dyn Model>>,
    /// Run after it (they read what it wrote in this tick's exchange).
    after_fc: Vec<Box<dyn Model>>,
    /// Bytes into Betaflight's UARTs (0-based UART indexes), besides the receiver's CRSF.
    serial: Vec<SerialLink>,
    /// Bytes out of Betaflight's UARTs.
    taps: Vec<SerialTap>,
    osd: Option<OsdHandle>,
}

fn video_devices(cfg: &QuadConfig, motors: usize, fc_divisor: u32, bus: &mut Bus) -> Result<VideoDevices, SimError> {
    let mut v = VideoDevices { before_fc: Vec::new(), after_fc: Vec::new(), serial: Vec::new(), taps: Vec::new(), osd: None };
    if let Some(e) = &cfg.esc_telemetry {
        let uart = Wire::new(ESC_UART_CAPACITY);
        v.before_fc.push(Box::new(EscTelemetry::new(uart.clone(), motors, cfg.sim.base_hz / e.rate_hz, bus)));
        v.serial.push(SerialLink { uart_index: e.uart - 1, rx: uart });
    }
    if let Some(o) = &cfg.osd {
        let tap = Wire::new(VIDEO_TAP_CAPACITY);
        let model = OsdModel::new(o.cols as usize, o.rows as usize, tap.clone(), fc_divisor);
        v.osd = Some(model.handle());
        v.taps.push(SerialTap { uart_index: o.uart - 1, tx: tap });
        v.after_fc.push(Box::new(model));
    }
    if let Some(x) = &cfg.vtx {
        let requests = Wire::new(VIDEO_TAP_CAPACITY);
        let replies = Wire::new(VIDEO_TAP_CAPACITY);
        let params = VtxParams {
            power_levels_mw: x.power_levels_mw.clone(),
            power_levels_dbm: x.power_levels_dbm.clone(),
            default_band: band_index(&x.default_band)
                .ok_or_else(|| SimError::InvalidArgument(format!("vtx.default_band {:?} is not a band letter", x.default_band)))?,
            default_channel: x.default_channel,
            default_power_index: x.default_power_index,
            reply_latency_s: x.reply_latency_ms / 1000.0,
        };
        v.taps.push(SerialTap { uart_index: x.uart - 1, tx: requests.clone() });
        v.serial.push(SerialLink { uart_index: x.uart - 1, rx: replies.clone() });
        v.after_fc.push(Box::new(VtxModel::new(params, requests, replies, fc_divisor, bus)));
    }
    Ok(v)
}

pub fn build(cfg: &QuadConfig, opts: &BuildOptions) -> Result<Vehicle, SimError> {
    let mut bus = Bus::new();
    let n = cfg.frame.motor_positions_frd_m.len();
    let base_hz = cfg.sim.base_hz;
    let mut models: Vec<Box<dyn Model>> = Vec::new();

    let prop = PropParams { diameter_m: cfg.prop.diameter_m, ct_table: pairs(&cfg.prop.ct_table), cp_table: pairs(&cfg.prop.cp_table) };
    for i in 0..n {
        models.push(Box::new(Propeller::new(i, prop.clone(), &mut bus)));
    }

    let motor = MotorParams {
        kv_rpm_per_v: cfg.motor.kv_rpm_per_v,
        resistance_ohm: cfg.motor.resistance_ohm,
        no_load_current_a: cfg.motor.no_load_current_a,
        rotor_inertia_kgm2: cfg.motor.rotor_inertia_kgm2,
    };
    let esc = EscParams { response_tau_s: cfg.esc.response_tau_s, current_limit_a: cfg.esc.current_limit_a };
    for i in 0..n {
        models.push(Box::new(EscMotor::new(i, esc.clone(), motor.clone(), cfg.prop.inertia_kgm2, &mut bus)));
    }

    let battery = BatteryParams {
        cells: cfg.battery.cells,
        capacity_mah: cfg.battery.capacity_mah,
        r0_ohm: cfg.battery.r0_ohm,
        r1_ohm: cfg.battery.r1_ohm,
        c1_f: cfg.battery.c1_f,
        ocv_table: pairs(&cfg.battery.ocv_table),
        initial_soc: cfg.battery.initial_soc,
    };
    models.push(Box::new(Battery::new(battery, n, base_hz / cfg.battery.rate_hz, &mut bus)));

    let f = &cfg.frame;
    let airframe = AirframeParams {
        mass_kg: f.mass_kg,
        inertia_kgm2: v3(f.inertia_kgm2),
        cda_m2: v3(f.cda_m2),
        angular_damping_nms: f.angular_damping_nms,
        rotor_inertia_kgm2: cfg.motor.rotor_inertia_kgm2 + cfg.prop.inertia_kgm2,
        mounts: f
            .motor_positions_frd_m
            .iter()
            .zip(&f.motor_spin)
            .map(|(p, s)| MotorMount { position_frd_m: v3(*p), spin: f64::from(*s) })
            .collect(),
        contact_points_frd_m: f.contact_points_frd_m.iter().map(|p| v3(*p)).collect(),
        ground: GroundParams {
            stiffness_npm: cfg.ground.stiffness_npm,
            damping_nspm: cfg.ground.damping_nspm,
            friction_coeff: cfg.ground.friction_coeff,
        },
    };
    let initial = BodyState {
        pos_ned_m: v3(cfg.initial.position_ned_m),
        vel_ned_mps: DVec3::ZERO,
        att: DQuat::from_rotation_z(cfg.initial.yaw_deg.to_radians()),
        rate_frd_radps: DVec3::ZERO,
    };
    models.push(Box::new(RigidBody::new(airframe, initial, &mut bus)));

    let imu = ImuParams {
        gyro_noise_std_radps: cfg.imu.gyro_noise_std_radps,
        gyro_bias_radps: v3(cfg.imu.gyro_bias_radps),
        accel_noise_std_mps2: cfg.imu.accel_noise_std_mps2,
        accel_bias_mps2: v3(cfg.imu.accel_bias_mps2),
    };
    models.push(Box::new(Imu::new(imu, opts.seed, &mut bus)));
    models.push(Box::new(Baro::new(BaroParams { noise_std_pa: cfg.baro.noise_std_pa, home_alt_m: cfg.home.alt_m }, opts.seed, &mut bus)));

    let r = &cfg.radio;
    let link = LinkParams {
        packet_rate_hz: r.packet_rate_hz,
        latency_packets: r.latency_packets,
        loss_good: r.loss_good,
        loss_bad: r.loss_bad,
        p_good_to_bad: r.p_good_to_bad,
        p_bad_to_good: r.p_bad_to_good,
        rssi_dbm: r.rssi_dbm,
        snr_db: r.snr_db,
        link_stats_interval_packets: r.link_stats_interval_packets,
        rf_mode: r.rf_mode,
        tx_power: r.tx_power,
    };
    let receiver_uart = Wire::new(RECEIVER_UART_CAPACITY);
    // Before the FC: a frame received on a tick reaches Betaflight in that tick's exchange.
    models.push(Box::new(ElrsLink::new(link, base_hz / r.packet_rate_hz, opts.seed, receiver_uart.clone(), &mut bus)));

    let fc_divisor = base_hz / cfg.fc.exchange_hz;
    let fc_kind = opts.fc_override.unwrap_or(cfg.fc.kind);
    let mut osd: Option<OsdHandle> = None;
    match fc_kind {
        FcKind::OpenLoop => models.push(Box::new(OpenLoopFc::new(n, fc_divisor, &mut bus))),
        FcKind::Sitl => {
            let video = video_devices(cfg, n, fc_divisor, &mut bus)?;
            let launch_argv = env_argv("OFS_SITL_LAUNCH").unwrap_or_else(|| cfg.fc.launch.clone());
            let mut cleanup = env_argv("OFS_SITL_CLEANUP").unwrap_or_else(|| cfg.fc.cleanup.clone());
            if cleanup.is_empty() {
                cleanup = net::default_cleanup(&launch_argv); // required under WSL (M0 §6)
            }
            let sitl_net = net::resolve(&launch_argv, env_ip("OFS_SITL_HOST")?, env_ip("OFS_SITL_REPLY_IP")?)
                .map_err(|e| SimError::Firmware(e.to_string()))?;
            let launch = LaunchConfig {
                launch: launch_argv,
                cleanup,
                workdir: firmware_dir(&opts.data_dir, &cfg.source_path),
                diff_file: cfg.resolve(&cfg.fc.betaflight_diff),
                startup_timeout: Duration::from_millis(cfg.fc.startup_timeout_ms),
            };
            let mut serial = vec![SerialLink { uart_index: r.uart - 1, rx: receiver_uart }];
            serial.extend(video.serial);
            let bridge = SitlBridge::start(
                BridgeConfig {
                    launch,
                    net: sitl_net,
                    rate_divisor: fc_divisor,
                    first_reply_timeout: Duration::from_millis(cfg.fc.first_reply_timeout_ms),
                    reply_timeout: Duration::from_millis(cfg.fc.reply_timeout_ms),
                    home: Home { lat_deg: cfg.home.lat_deg, lon_deg: cfg.home.lon_deg, alt_m: cfg.home.alt_m },
                    motor_count: n,
                    serial,
                    taps: video.taps,
                },
                &mut bus,
            )
            .map_err(|e| SimError::Firmware(e.to_string()))?;
            models.extend(video.before_fc);
            models.push(Box::new(bridge));
            models.extend(video.after_fc);
            osd = video.osd;
        }
    }

    let h = Handles::register(&mut bus, n);
    let mut scheduler = Scheduler::new(base_hz, bus);
    for m in models {
        scheduler.add(m);
    }
    let mut vehicle = Vehicle { scheduler, h, sitl: fc_kind == FcKind::Sitl, osd };
    // The bus starts every signal at zero; aux 0.0 would reach Betaflight as 1500 us until the first SetSticks.
    vehicle.set_sticks(&Sticks::default());
    vehicle.set_transmitter(true);
    Ok(vehicle)
}

impl Vehicle {
    pub fn set_sticks(&mut self, s: &Sticks) {
        let h = &self.h;
        let bus = self.scheduler.bus_mut();
        bus.set(h.roll, s.roll);
        bus.set(h.pitch, s.pitch);
        bus.set(h.yaw, s.yaw);
        bus.set(h.throttle, s.throttle);
        for (sig, v) in h.aux.iter().zip(s.aux) {
            bus.set(*sig, v);
        }
    }

    /// Transmitter on (pilot or script connected) or off (the receiver hears nothing).
    pub fn set_transmitter(&mut self, on: bool) {
        let s = self.h.tx_enabled;
        self.scheduler.bus_mut().set(s, if on { 1.0 } else { 0.0 });
    }

    pub fn set_fault(&mut self, fault: Fault, active: bool) {
        let s = match fault {
            Fault::RadioLinkLoss => self.h.fault_radio_loss,
        };
        self.scheduler.bus_mut().set(s, if active { 1.0 } else { 0.0 });
    }

    pub fn clear_faults(&mut self) {
        self.set_fault(Fault::RadioLinkLoss, false);
    }

    pub fn run_for(&mut self, seconds: f64) -> Result<(), SimError> {
        self.scheduler.run_for(seconds)
    }

    /// Steps `ticks` base ticks. On error the failing tick is not counted.
    pub fn step_ticks(&mut self, ticks: u64) -> Result<(), SimError> {
        for _ in 0..ticks {
            self.scheduler.step()?;
        }
        Ok(())
    }

    pub fn time_s(&self) -> f64 {
        self.scheduler.time_s()
    }

    pub fn base_hz(&self) -> u32 {
        self.scheduler.base_hz()
    }

    /// Where Betaflight Configurator can connect (SITL's MSP UART), when this vehicle runs Betaflight SITL.
    pub fn configurator_address(&self) -> Option<String> {
        self.sitl.then(|| format!("tcp://127.0.0.1:{MSP_TCP_PORT}"))
    }

    /// The latest OSD frame, when the quad has an `[osd]` section and runs Betaflight.
    pub fn osd_frame(&self) -> Option<OsdFrame> {
        self.osd.as_ref().map(OsdHandle::latest)
    }

    pub fn state(&self) -> VehicleState {
        let b = self.scheduler.bus();
        let h = &self.h;
        VehicleState {
            time_s: self.scheduler.time_s(),
            pos_ned_m: b.get(h.pos),
            vel_ned_mps: b.get(h.vel),
            att: b.get(h.att),
            rate_frd_radps: b.get(h.rate),
            battery_voltage_v: b.get(h.vbat),
            battery_current_a: b.get(h.ibat),
            motor_rpm: h.omega.iter().map(|s| b.get(*s) * 60.0 / (2.0 * PI)).collect(),
            motor_cmd: h.cmd.iter().map(|s| b.get(*s)).collect(),
            radio: RadioState {
                tx_enabled: b.get(h.tx_enabled) > 0.5,
                link_up: b.get(h.link_up) > 0.5,
                lq_pct: b.get(h.lq),
                rssi_dbm: b.get(h.rssi),
            },
            fc_restarts: b.get(h.fc_restarts) as u32,
            vtx: VtxInfo {
                present: b.get(h.vtx_present) > 0.5,
                band: b.get(h.vtx_band) as u8,
                channel: b.get(h.vtx_channel) as u8,
                freq_mhz: b.get(h.vtx_freq) as u32,
                power_mw: b.get(h.vtx_power) as u32,
                pit_mode: b.get(h.vtx_pit) > 0.5,
            },
            serial_dropped_bytes: b.get(h.serial_dropped) as u64,
        }
    }

    pub fn digest(&self) -> u64 {
        self.scheduler.bus().digest()
    }
}

/// Space-separated argv from an environment variable, if set and non-empty.
fn env_argv(var: &str) -> Option<Vec<String>> {
    std::env::var(var).ok().filter(|s| !s.trim().is_empty()).map(|s| s.split_whitespace().map(String::from).collect())
}

/// IPv4 address from an environment variable, if set and non-empty.
fn env_ip(var: &str) -> Result<Option<Ipv4Addr>, SimError> {
    match std::env::var(var) {
        Ok(v) if !v.trim().is_empty() => v
            .trim()
            .parse()
            .map(Some)
            .map_err(|_| SimError::InvalidArgument(format!("{var}={v} is not an IPv4 address"))),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped_quad() -> QuadConfig {
        ofs_config::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml"))).unwrap()
    }

    #[test]
    fn the_shipped_quad_wires_esc_telemetry_osd_and_vtx_to_their_uarts() {
        let cfg = shipped_quad();
        let mut bus = Bus::new();
        let video = video_devices(&cfg, 4, 8, &mut bus).unwrap();
        let serial: Vec<u8> = video.serial.iter().map(|l| l.uart_index).collect();
        let taps: Vec<u8> = video.taps.iter().map(|t| t.uart_index).collect();
        assert_eq!(serial, vec![2, 4], "ESC telemetry on UART3 and the VTX replies on UART5");
        assert_eq!(taps, vec![3, 4], "DisplayPort from UART4 and SmartAudio requests from UART5");
        assert_eq!(video.before_fc.len(), 1, "the ESC telemetry runs before the flight controller");
        assert_eq!(video.after_fc.len(), 2, "the OSD and the VTX run after it");
        assert!(video.osd.is_some());
    }

    #[test]
    fn a_quad_without_video_sections_wires_nothing() {
        let mut cfg = shipped_quad();
        (cfg.esc_telemetry, cfg.osd, cfg.vtx) = (None, None, None);
        let mut bus = Bus::new();
        let video = video_devices(&cfg, 4, 8, &mut bus).unwrap();
        assert!(video.serial.is_empty() && video.taps.is_empty() && video.osd.is_none());
        assert!(video.before_fc.is_empty() && video.after_fc.is_empty());
    }

    #[test]
    fn an_open_loop_vehicle_has_no_osd_and_no_vtx() {
        let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-unit-test-data"), fc_override: Some(FcKind::OpenLoop) };
        let mut vehicle = build(&shipped_quad(), &opts).unwrap();
        vehicle.run_for(0.05).unwrap();
        let state = vehicle.state();
        assert_eq!(state.vtx, VtxInfo { present: false, band: 0, channel: 0, freq_mhz: 0, power_mw: 0, pit_mode: false });
        assert_eq!(state.serial_dropped_bytes, 0);
        assert!(vehicle.osd_frame().is_none());
    }
}
