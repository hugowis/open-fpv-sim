//! Canonical bus signal names shared by all crates.

pub const BODY_POS_NED: &str = "body.pos_ned_m";
pub const BODY_VEL_NED: &str = "body.vel_ned_mps";
/// FRD body -> NED world.
pub const BODY_ATT: &str = "body.att_q";
pub const BODY_RATE_FRD: &str = "body.rate_frd_radps";
/// Kinematic acceleration (not specific force), NED.
pub const BODY_ACCEL_NED: &str = "body.accel_ned_mps2";

pub const BATTERY_VOLTAGE: &str = "battery.voltage_v";
pub const BATTERY_CURRENT: &str = "battery.current_a";
pub const BATTERY_SOC: &str = "battery.soc";
pub const BATTERY_CONSUMED: &str = "battery.consumed_mah";

pub const IMU_GYRO: &str = "imu.gyro_frd_radps";
/// Specific force, FRD.
pub const IMU_ACCEL: &str = "imu.accel_frd_mps2";
pub const BARO_PRESSURE: &str = "baro.pressure_pa";

/// Sticks in [-1, 1]; throttle in [0, 1]; aux in [-1, 1].
pub const RC_ROLL: &str = "rc.roll";
pub const RC_PITCH: &str = "rc.pitch";
pub const RC_YAW: &str = "rc.yaw";
pub const RC_THROTTLE: &str = "rc.throttle";
pub const RC_AUX_COUNT: usize = 4;

pub fn rc_aux(i: usize) -> String {
    format!("rc.aux.{i}")
}

/// Flight-controller motor command in [0, 1].
pub fn motor_cmd(i: usize) -> String {
    format!("fc.motor.{i}.cmd")
}

pub fn esc_duty(i: usize) -> String {
    format!("esc.{i}.duty")
}

pub fn esc_bus_current(i: usize) -> String {
    format!("esc.{i}.bus_current_a")
}

pub fn motor_current(i: usize) -> String {
    format!("motor.{i}.current_a")
}

/// Rotor speed magnitude (>= 0).
pub fn motor_omega(i: usize) -> String {
    format!("motor.{i}.omega_radps")
}

pub fn motor_omega_dot(i: usize) -> String {
    format!("motor.{i}.omega_dot_radps2")
}

/// Thrust magnitude along body -Z.
pub fn prop_thrust(i: usize) -> String {
    format!("prop.{i}.thrust_n")
}

/// Aerodynamic drag torque magnitude on the rotor.
pub fn prop_torque(i: usize) -> String {
    format!("prop.{i}.torque_nm")
}

/// Transmitter (handset) power: 1 = on, 0 = off (no pilot connected). Read by the radio link.
pub const RADIO_TX_ENABLED: &str = "radio.tx_enabled";
/// 1 while the receiver has heard at least one packet in its link-quality window, else 0.
pub const RADIO_LINK_UP: &str = "radio.link_up";
/// Uplink link quality: percent of the last 100 packets received.
pub const RADIO_LQ: &str = "radio.lq_pct";
/// Uplink RSSI as the receiver reports it.
pub const RADIO_RSSI: &str = "radio.rssi_dbm";
/// Fault: 1 = every uplink packet is lost.
pub const FAULT_RADIO_LINK_LOSS: &str = "fault.radio.link_loss";
/// Number of firmware restarts so far (e.g. Betaflight rebooting after a Configurator save).
pub const FC_RESTARTS: &str = "fc.restarts";
/// Cumulative serial bytes lost on the way to or from Betaflight: those SITL reported dropping from its UART TX
/// capture buffers, plus those our own full wires dropped (into SITL's UARTs, or from them to a model that fell
/// behind).
pub const FC_SERIAL_DROPPED: &str = "fc.serial_dropped_bytes";

/// 1.0 when the quad has a VTX (it transmits from load on, with or without Betaflight).
pub const VTX_PRESENT: &str = "vtx.present";
/// Band 1..=6 (A, B, E, F, R, L); 0 while the VTX is in user-frequency mode.
pub const VTX_BAND: &str = "vtx.band";
/// Channel 1..=8; 0 in user-frequency mode.
pub const VTX_CHANNEL: &str = "vtx.channel";
pub const VTX_FREQ_MHZ: &str = "vtx.freq_mhz";
/// Output power of the current power level in mW (see `vtx.pit_mode`).
pub const VTX_POWER_MW: &str = "vtx.power_mw";
/// 1.0 while the VTX is in pit mode.
pub const VTX_PIT: &str = "vtx.pit_mode";

/// 1.0 when the quad has a VTX, so the video link model runs.
pub const VIDEO_PRESENT: &str = "video.present";
/// Signal-to-noise ratio in dB at the receiver antenna in use.
pub const VIDEO_SNR: &str = "video.snr_db";
/// Index (into the world's receiver antennas) of the antenna in use.
pub const VIDEO_ANTENNA: &str = "video.antenna";
/// Other emitters' power after the receiver's channel filter, at the antenna in use, in dBm.
pub const VIDEO_INTERFERENCE: &str = "video.interference_dbm";
/// Picture grain, 0 (clean) to 1 (static).
pub const VIDEO_NOISE: &str = "video.noise";
/// FM threshold sparkles, 0 to 1.
pub const VIDEO_SPARKLES: &str = "video.sparkles";
/// Colour saturation, 1 (full colour) to 0 (black and white).
pub const VIDEO_CHROMA: &str = "video.chroma";
/// 0 locked, 1 unstable (tearing), 2 lost (rolling, static).
pub const VIDEO_SYNC: &str = "video.sync";

/// Received power in dBm at one receiver antenna, by its name in the world file.
pub fn video_rssi(antenna: &str) -> String {
    format!("video.rssi_dbm.{antenna}")
}
