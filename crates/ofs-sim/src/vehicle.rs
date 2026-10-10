//! Assembles a quad from its config into a scheduler and exposes sticks in, state out.
use std::f64::consts::PI;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use glam::{DQuat, DVec3};
use ofs_config::world::{self as world_cfg, WorldConfig};
use ofs_config::{FcKind, QuadConfig, VtxSection};
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
use ofs_video::link::{Emitter, LinkParams as VideoLinkParams, LinkWorld, ReceiverAntenna, VideoLink, VideoSync, FIELD_RATE_HZ};
use ofs_video::osd::{OsdFrame, OsdHandle, OsdModel};
use ofs_core::shape::Shape;
use ofs_rf::propagation::{Antenna, AntennaKind, Obstacle, Polarization};
use ofs_video::vtx::{band_index, VtxModel, VtxParams, FREQUENCIES_MHZ};

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub seed: u64,
    /// Per-quad firmware working directories live under here.
    pub data_dir: PathBuf,
    pub fc_override: Option<FcKind>,
    /// The field the quad flies in: the pilot's goggles, objects and other transmitters (the video link's world).
    pub world: WorldConfig,
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

/// The analog video link as the goggles see it (`present = false` and a clean picture without a VTX).
#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    pub present: bool,
    pub snr_db: f64,
    pub interference_dbm: f64,
    /// Received power at each goggle antenna, in the world file's order.
    pub rssi_dbm: Vec<(String, f64)>,
    /// The antenna the receiver uses.
    pub active_antenna: String,
    pub noise: f64,
    pub sparkles: f64,
    pub chroma: f64,
    pub sync: VideoSync,
}

impl Default for VideoInfo {
    fn default() -> Self {
        Self {
            present: false,
            snr_db: 0.0,
            interference_dbm: 0.0,
            rssi_dbm: Vec::new(),
            active_antenna: String::new(),
            noise: 0.0,
            sparkles: 0.0,
            chroma: 1.0,
            sync: VideoSync::Locked,
        }
    }
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
    pub video: VideoInfo,
}

struct VideoHandles {
    present: Signal<f64>,
    snr: Signal<f64>,
    interference: Signal<f64>,
    rssi: Vec<(String, Signal<f64>)>,
    antenna: Signal<f64>,
    noise: Signal<f64>,
    sparkles: Signal<f64>,
    chroma: Signal<f64>,
    sync: Signal<f64>,
}

impl VideoHandles {
    fn register(bus: &mut Bus, world: &WorldConfig) -> Self {
        Self {
            present: bus.signal(names::VIDEO_PRESENT),
            snr: bus.signal(names::VIDEO_SNR),
            interference: bus.signal(names::VIDEO_INTERFERENCE),
            rssi: world.receiver.antennas.iter().map(|a| (a.name.clone(), bus.signal(&names::video_rssi(&a.name)))).collect(),
            antenna: bus.signal(names::VIDEO_ANTENNA),
            noise: bus.signal(names::VIDEO_NOISE),
            sparkles: bus.signal(names::VIDEO_SPARKLES),
            chroma: bus.signal(names::VIDEO_CHROMA),
            sync: bus.signal(names::VIDEO_SYNC),
        }
    }

    fn read(&self, b: &Bus) -> VideoInfo {
        if b.get(self.present) < 0.5 {
            return VideoInfo::default();
        }
        let active = b.get(self.antenna) as usize;
        VideoInfo {
            present: true,
            snr_db: b.get(self.snr),
            interference_dbm: b.get(self.interference),
            rssi_dbm: self.rssi.iter().map(|(name, s)| (name.clone(), b.get(*s))).collect(),
            active_antenna: self.rssi.get(active).map(|(name, _)| name.clone()).unwrap_or_default(),
            noise: b.get(self.noise),
            sparkles: b.get(self.sparkles),
            chroma: b.get(self.chroma),
            sync: VideoSync::from_signal(b.get(self.sync)),
        }
    }
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
    video: VideoHandles,
}

impl Handles {
    fn register(bus: &mut Bus, motors: usize, world: &WorldConfig) -> Self {
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
            video: VideoHandles::register(bus, world),
        }
    }
}

pub struct Vehicle {
    scheduler: Scheduler,
    h: Handles,
    sitl: bool,
    osd: Option<OsdHandle>,
    world: WorldConfig,
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
        v.taps.push(SerialTap { uart_index: x.uart - 1, tx: requests.clone() });
        v.serial.push(SerialLink { uart_index: x.uart - 1, rx: replies.clone() });
        v.after_fc.push(Box::new(vtx_model(x, requests, replies, fc_divisor, bus)?));
    }
    Ok(v)
}

/// The VTX: it answers SmartAudio on `requests`/`replies` and transmits its power-up channel and power from the start.
fn vtx_model(x: &VtxSection, requests: Wire, replies: Wire, divisor: u32, bus: &mut Bus) -> Result<VtxModel, SimError> {
    let params = VtxParams {
        power_levels_mw: x.power_levels_mw.clone(),
        power_levels_dbm: x.power_levels_dbm.clone(),
        default_band: band_index(&x.default_band)
            .ok_or_else(|| SimError::InvalidArgument(format!("vtx.default_band {:?} is not a band letter", x.default_band)))?,
        default_channel: x.default_channel,
        default_power_index: x.default_power_index,
        reply_latency_s: x.reply_latency_ms / 1000.0,
    };
    Ok(VtxModel::new(params, requests, replies, divisor, bus))
}

fn polarization(p: world_cfg::Polarization) -> Polarization {
    match p {
        world_cfg::Polarization::Rhcp => Polarization::Rhcp,
        world_cfg::Polarization::Lhcp => Polarization::Lhcp,
        world_cfg::Polarization::Linear => Polarization::Linear,
    }
}

fn antenna_kind(kind: world_cfg::AntennaKind, beamwidth_deg: Option<f64>) -> AntennaKind {
    match kind {
        world_cfg::AntennaKind::Omni => AntennaKind::Omni,
        world_cfg::AntennaKind::Patch => AntennaKind::Patch { beamwidth_deg: beamwidth_deg.unwrap_or(60.0) },
    }
}

/// A direction in NED from a heading (degrees clockwise from north) and an elevation (degrees up).
pub fn aim_ned(heading_deg: f64, elevation_deg: f64) -> DVec3 {
    DVec3::from_array(ofs_proto::aim_ned(heading_deg, elevation_deg))
}

/// An emitter's frequency: its `freq_mhz`, or its band and channel in the factory table.
pub(crate) fn emitter_freq_mhz(e: &world_cfg::EmitterSection) -> Result<f64, SimError> {
    if let Some(f) = e.freq_mhz {
        return Ok(f);
    }
    let band = e.band.as_deref().and_then(band_index);
    match (band, e.channel) {
        (Some(b), Some(c @ 1..=8)) => Ok(f64::from(FREQUENCIES_MHZ[b][usize::from(c) - 1])),
        _ => Err(SimError::InvalidArgument(format!("emitter {:?} has no valid frequency", e.name))),
    }
}

/// The video link's view of the world file and the quad's VTX antenna.
pub fn link_params(vtx: &VtxSection, world: &WorldConfig) -> Result<VideoLinkParams, SimError> {
    let facing = world.pilot.facing_deg;
    let antennas = world
        .receiver
        .antennas
        .iter()
        .map(|a| ReceiverAntenna {
            name: a.name.clone(),
            antenna: Antenna {
                kind: antenna_kind(a.kind, a.beamwidth_deg),
                gain_dbi: a.gain_dbi,
                polarization: polarization(a.polarization),
                axis: aim_ned(facing + a.aim_az_deg, a.aim_el()),
            },
        })
        .collect();
    let obstacles = world
        .objects
        .iter()
        .filter(|o| o.rf_loss_db > 0.0)
        .map(|o| {
            let center = v3(o.center_ned_m);
            let shape = match o.shape {
                world_cfg::Shape::Box => {
                    let s = o.size_m.unwrap_or_default();
                    Shape::Box { center, half: DVec3::new(s[0], s[1], s[2]) * 0.5 }
                }
                world_cfg::Shape::Cylinder => {
                    Shape::Cylinder { center, radius: o.radius_m.unwrap_or_default(), half_height: o.height_m.unwrap_or_default() * 0.5 }
                }
            };
            Obstacle { shape: shape.rooted(), rf_loss_db: o.rf_loss_db }
        })
        .collect();
    let emitters = world
        .emitters
        .iter()
        .map(|e| {
            Ok(Emitter {
                position: v3(e.position_ned_m),
                freq_mhz: emitter_freq_mhz(e)?,
                power_mw: e.power_mw,
                antenna: Antenna { kind: AntennaKind::Omni, gain_dbi: e.gain_dbi, polarization: polarization(e.polarization), axis: DVec3::NEG_Z },
            })
        })
        .collect::<Result<Vec<_>, SimError>>()?;
    let a = &vtx.antenna;
    Ok(VideoLinkParams {
        world: LinkWorld {
            pilot_position: v3(world.pilot.position_ned_m),
            antennas,
            noise_floor_dbm: world.receiver.noise_floor_dbm,
            diversity: world.receiver.diversity,
            obstacles,
            emitters,
        },
        vtx_antenna: Antenna {
            kind: antenna_kind(a.kind, a.beamwidth_deg),
            gain_dbi: a.gain_dbi,
            polarization: polarization(a.polarization),
            axis: v3(a.mount_frd).normalize(),
        },
        pit_power_mw: vtx.pit_power_mw,
        fading: true,
        ground_bounce: true,
    })
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
        objects: Vec::new(), // the world's objects are wired in with the collision milestone
        collision: Default::default(),
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
        FcKind::OpenLoop => {
            models.push(Box::new(OpenLoopFc::new(n, fc_divisor, &mut bus)));
            // No Betaflight to talk to, but the VTX is on the quad all the same: it transmits its power-up channel.
            if let Some(x) = &cfg.vtx {
                models.push(Box::new(vtx_model(x, Wire::new(VIDEO_TAP_CAPACITY), Wire::new(VIDEO_TAP_CAPACITY), fc_divisor, &mut bus)?));
            }
        }
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
    if let Some(x) = &cfg.vtx {
        // Last in the tick: it reads the pose the physics wrote and the channel the VTX published.
        let params = link_params(x, &opts.world)?;
        models.push(Box::new(VideoLink::new(params, base_hz / FIELD_RATE_HZ, opts.seed, &mut bus)));
    }

    let h = Handles::register(&mut bus, n, &opts.world);
    let mut scheduler = Scheduler::new(base_hz, bus);
    for m in models {
        scheduler.add(m);
    }
    let mut vehicle = Vehicle { scheduler, h, sitl: fc_kind == FcKind::Sitl, osd, world: opts.world.clone() };
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

    /// The world the vehicle flies in (the open field when the session named none).
    pub fn world(&self) -> &WorldConfig {
        &self.world
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
            video: h.video.read(b),
        }
    }

    pub fn digest(&self) -> u64 {
        self.scheduler.bus().digest()
    }
}

/// Space-separated argv from an environment variable, if set and non-empty.
fn env_argv(var: &str) -> Option<Vec<String>> {
    std::env::var(var).ok().filter(|s| !s.trim().is_empty()).map(|s| split_argv(&s))
}

/// Splits a command line on whitespace; `"..."` or `'...'` keeps a path with spaces in one argument. Backslashes are
/// ordinary characters (Windows paths).
fn split_argv(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current: Option<String> = None;
    let mut quote = None;
    for c in line.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.get_or_insert_with(String::new).push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                current.get_or_insert_with(String::new);
            }
            None if c.is_whitespace() => args.extend(current.take()),
            None => current.get_or_insert_with(String::new).push(c),
        }
    }
    args.extend(current);
    args
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

    #[test]
    fn launch_argv_from_the_environment_keeps_quoted_paths_whole() {
        let argv = split_argv(r#"wsl.exe -d Ubuntu -e "/home/me/my builds/betaflight_SITL.elf" --x 'a b'"#);
        assert_eq!(argv, ["wsl.exe", "-d", "Ubuntu", "-e", "/home/me/my builds/betaflight_SITL.elf", "--x", "a b"]);
        assert_eq!(split_argv("  plain   words "), ["plain", "words"]);
        assert_eq!(split_argv(r#"C:\tools\sitl.exe "#), [r"C:\tools\sitl.exe"], "backslashes are kept (Windows paths)");
    }

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

    fn flat_world() -> WorldConfig {
        world_cfg::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/flat.toml"))).unwrap()
    }

    fn open_loop(cfg: &QuadConfig, world: WorldConfig) -> Vehicle {
        let opts = BuildOptions {
            seed: 1,
            data_dir: std::env::temp_dir().join("ofs-unit-test-data"),
            fc_override: Some(FcKind::OpenLoop),
            world,
        };
        build(cfg, &opts).unwrap()
    }

    #[test]
    fn an_open_loop_vehicle_has_its_vtx_at_the_power_up_channel_but_no_osd() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        vehicle.run_for(0.05).unwrap();
        let state = vehicle.state();
        assert_eq!(state.vtx, VtxInfo { present: true, band: 5, channel: 1, freq_mhz: 5658, power_mw: 200, pit_mode: false });
        assert_eq!(state.serial_dropped_bytes, 0);
        assert!(vehicle.osd_frame().is_none());
        assert!(state.video.present);
        assert_eq!(state.video.sync, VideoSync::Locked, "{:?}", state.video);
        assert_eq!(state.video.active_antenna, "omni");
        assert_eq!(vehicle.world().name, "open field");
    }

    #[test]
    fn a_quad_without_a_vtx_has_no_video_link() {
        let mut cfg = shipped_quad();
        cfg.vtx = None;
        let mut vehicle = open_loop(&cfg, flat_world());
        vehicle.run_for(0.05).unwrap();
        let state = vehicle.state();
        assert!(!state.vtx.present);
        assert_eq!(state.video, VideoInfo::default());
    }

    #[test]
    fn the_link_parameters_follow_the_world_file() {
        let world = flat_world();
        let params = link_params(shipped_quad().vtx.as_ref().unwrap(), &world).unwrap();
        let w = &params.world;
        assert_eq!(w.pilot_position, DVec3::new(-3.0, 2.0, -1.7));
        assert_eq!(w.antennas.len(), 2);
        assert!((w.antennas[0].antenna.axis - DVec3::NEG_Z).length() < 1e-12, "the omni stands upright");
        let patch = w.antennas[1].antenna;
        assert_eq!(patch.kind, AntennaKind::Patch { beamwidth_deg: 60.0 });
        let expected = DVec3::new(10f64.to_radians().cos(), 0.0, -10f64.to_radians().sin());
        assert!((patch.axis - expected).length() < 1e-12, "the patch looks north, 10 degrees up: {}", patch.axis);
        assert_eq!(w.obstacles.len(), 4, "only the buildings take signal");
        assert_eq!(w.emitters.len(), 1);
        assert_eq!(w.emitters[0].freq_mhz, 5695.0, "R2");
        assert!((params.vtx_antenna.axis - DVec3::new(-0.5, 0.0, -1.0).normalize()).length() < 1e-12);
        assert_eq!(params.pit_power_mw, 0.1);
        assert!(params.fading && params.ground_bounce);
        assert!((aim_ned(90.0, 0.0) - DVec3::Y).length() < 1e-12, "heading 90 is east");
    }

    /// The quad resting on the ground at `north`, `east` in the flat world (or the same world without objects).
    fn snr_at(north: f64, east: f64, objects: bool, power_index: usize) -> VideoInfo {
        let mut cfg = shipped_quad();
        cfg.initial.position_ned_m = [north, east, -0.03];
        cfg.vtx.as_mut().unwrap().default_power_index = power_index;
        let mut world = flat_world();
        if !objects {
            world.objects.clear();
        }
        let mut vehicle = open_loop(&cfg, world);
        vehicle.run_for(0.2).unwrap();
        vehicle.state().video
    }

    #[test]
    fn the_picture_is_clean_near_the_pilot_and_lost_far_away() {
        let near = snr_at(10.0, 0.0, true, 1);
        assert_eq!((near.sync, near.noise, near.chroma), (VideoSync::Locked, 0.0, 1.0), "10 m out at 200 mW: {near:?}");
        let far = snr_at(4000.0, 0.0, true, 0);
        assert_eq!(far.sync, VideoSync::Lost, "4 km out at 25 mW: {far:?}");
        assert!(far.noise > 0.9, "static: {far:?}");
        assert_eq!(far.active_antenna, "patch", "out in front the patch hears it best");
    }

    #[test]
    fn building_b_shadows_the_quad_behind_it() {
        // Twice as far from the pilot as building B's centre, on the same bearing: the building is in the way.
        let (north, east) = (-3.0 + 2.0 * 78.0, 2.0 + 2.0 * 36.0);
        let open = snr_at(north, east, false, 1);
        let shadowed = snr_at(north, east, true, 1);
        assert!(open.snr_db - shadowed.snr_db > 15.0, "open {} dB, behind the building {} dB", open.snr_db, shadowed.snr_db);
    }
}
