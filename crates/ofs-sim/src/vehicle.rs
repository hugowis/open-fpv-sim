//! Assembles a quad from its config into a scheduler and exposes sticks in, state out.
use std::f64::consts::PI;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::Duration;

use glam::{DQuat, DVec3};
use ofs_config::{FcKind, QuadConfig};
use ofs_core::{names, Bus, Model, Scheduler, Signal, SimError};
use ofs_electrical::battery::{Battery, BatteryParams};
use ofs_electrical::esc_motor::{EscMotor, EscParams, MotorParams};
use ofs_fc::open_loop::OpenLoopFc;
use ofs_fc::sitl::bridge::{BridgeConfig, SitlBridge};
use ofs_fc::sitl::frames::Home;
use ofs_fc::sitl::net;
use ofs_fc::sitl::process::LaunchConfig;
use ofs_physics::propeller::{PropParams, Propeller};
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, MotorMount, RigidBody};
use ofs_sensors::baro::{Baro, BaroParams};
use ofs_sensors::imu::{Imu, ImuParams};

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
        }
    }
}

pub struct Vehicle {
    scheduler: Scheduler,
    h: Handles,
}

fn v3(a: [f64; 3]) -> DVec3 {
    DVec3::from_array(a)
}

fn pairs(t: &[[f64; 2]]) -> Vec<(f64, f64)> {
    t.iter().map(|r| (r[0], r[1])).collect()
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

    let fc_divisor = base_hz / cfg.fc.exchange_hz;
    match opts.fc_override.unwrap_or(cfg.fc.kind) {
        FcKind::OpenLoop => models.push(Box::new(OpenLoopFc::new(n, fc_divisor, &mut bus))),
        FcKind::Sitl => {
            let quad_stem = cfg.source_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "quad".into());
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
                workdir: opts.data_dir.join(quad_stem),
                diff_file: cfg.resolve(&cfg.fc.betaflight_diff),
                startup_timeout: Duration::from_millis(cfg.fc.startup_timeout_ms),
            };
            let bridge = SitlBridge::start(
                BridgeConfig {
                    launch,
                    net: sitl_net,
                    rate_divisor: fc_divisor,
                    first_reply_timeout: Duration::from_millis(cfg.fc.first_reply_timeout_ms),
                    reply_timeout: Duration::from_millis(cfg.fc.reply_timeout_ms),
                    home: Home { lat_deg: cfg.home.lat_deg, lon_deg: cfg.home.lon_deg, alt_m: cfg.home.alt_m },
                    motor_count: n,
                    serial: vec![],
                },
                &mut bus,
            )
            .map_err(|e| SimError::Firmware(e.to_string()))?;
            models.push(Box::new(bridge));
        }
    }

    let h = Handles::register(&mut bus, n);
    let mut scheduler = Scheduler::new(base_hz, bus);
    for m in models {
        scheduler.add(m);
    }
    let mut vehicle = Vehicle { scheduler, h };
    // The bus starts every signal at zero; aux 0.0 would reach SITL as 1500 us until the first SetSticks.
    vehicle.set_sticks(&Sticks::default());
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

    pub fn run_for(&mut self, seconds: f64) -> Result<(), SimError> {
        self.scheduler.run_for(seconds)
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
