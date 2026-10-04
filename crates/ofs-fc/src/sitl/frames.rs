//! Simulator state (NED/FRD) to SITL packet fields for the legacy bridge (-DENABLE_GAZEBO_BRIDGE=0).
//! Mapping verified in M0: docs/research/sitl-interface.md §3.
use std::f64::consts::PI;

use glam::{DQuat, DVec3};

use super::codec::{FdmPacket, ServoPacket};

pub const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// Applied to FRD body rates before sending (SITL then maps (x, -y, -z), so Betaflight gets FLU rates).
pub const GYRO_SIGN: [f64; 3] = [1.0, 1.0, 1.0];
/// Applied to the FRD specific force before sending (SITL negates all axes, so Betaflight gets FLU).
pub const ACCEL_SIGN: [f64; 3] = [-1.0, 1.0, 1.0];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Home {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_m: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct SensorFrame {
    pub time_s: f64,
    pub gyro_frd_radps: DVec3,
    /// Specific force, FRD.
    pub accel_frd_mps2: DVec3,
    /// FRD body -> NED world.
    pub att_ned: DQuat,
    pub vel_ned_mps: DVec3,
    pub pos_ned_m: DVec3,
    pub pressure_pa: f64,
}

/// FRD->NED attitude expressed as FLU body -> NWU world (both frames are Rx(π) of the originals). w >= 0.
pub fn attitude_flu_nwu(att_ned: DQuat) -> DQuat {
    let x180 = DQuat::from_rotation_x(PI);
    let q = (x180 * att_ned * x180).normalize();
    if q.w < 0.0 {
        -q
    } else {
        q
    }
}

pub fn fdm_packet(f: &SensorFrame, home: &Home) -> FdmPacket {
    let g = f.gyro_frd_radps.to_array();
    let q = attitude_flu_nwu(f.att_ned);
    let lat0 = home.lat_deg.to_radians();
    let lat = home.lat_deg + (f.pos_ned_m.x / EARTH_RADIUS_M).to_degrees();
    let lon = home.lon_deg + (f.pos_ned_m.y / (EARTH_RADIUS_M * lat0.cos())).to_degrees();
    FdmPacket {
        timestamp_s: f.time_s,
        gyro_rpy_radps: [g[0] * GYRO_SIGN[0], g[1] * GYRO_SIGN[1], g[2] * GYRO_SIGN[2]],
        accel_xyz_mps2: {
            let a = f.accel_frd_mps2.to_array();
            [a[0] * ACCEL_SIGN[0], a[1] * ACCEL_SIGN[1], a[2] * ACCEL_SIGN[2]]
        },
        quat_wxyz: [q.w, q.x, q.y, q.z],
        velocity_xyz_mps: [f.vel_ned_mps.y, f.vel_ned_mps.x, -f.vel_ned_mps.z],
        position_xyz: [lon, lat, home.alt_m - f.pos_ned_m.z],
        pressure_pa: f.pressure_pa,
    }
}

pub fn stick_us(x: f64) -> u16 {
    (1500.0 + 500.0 * x.clamp(-1.0, 1.0)).round() as u16
}

pub fn throttle_us(t: f64) -> u16 {
    (1000.0 + 1000.0 * t.clamp(0.0, 1.0)).round() as u16
}

/// Betaflight default channel map AETR, then AUX1.., remaining channels centred.
pub fn rc_channels(roll: f64, pitch: f64, yaw: f64, throttle: f64, aux: &[f64]) -> [u16; 16] {
    let mut ch = [1500u16; 16];
    ch[0] = stick_us(roll);
    ch[1] = stick_us(pitch);
    ch[2] = throttle_us(throttle);
    ch[3] = stick_us(yaw);
    for (slot, a) in ch[4..].iter_mut().zip(aux) {
        *slot = stick_us(*a);
    }
    ch
}

pub fn motor_commands(p: &ServoPacket) -> [f64; 4] {
    p.motor_speed.map(|m| f64::from(m).clamp(0.0, 1.0))
}
