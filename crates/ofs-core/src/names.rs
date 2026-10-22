//! Canonical bus signal names shared by all crates.

pub const BODY_POS_NED: &str = "body.pos_ned_m";
pub const BODY_VEL_NED: &str = "body.vel_ned_mps";
/// FRD body -> NED world.
pub const BODY_ATT: &str = "body.att_q";
pub const BODY_RATE_FRD: &str = "body.rate_frd_radps";
/// Kinematic acceleration (not specific force), NED.
pub const BODY_ACCEL_NED: &str = "body.accel_ned_mps2";
/// Inward speed of the last collision event, m/s (0 before the first one).
pub const BODY_COLLISION_SPEED: &str = "body.collision_speed";
/// Object index of the last collision event; -1 is the ground plane.
pub const BODY_COLLISION_OBJECT: &str = "body.collision_object";
/// Collision events raised so far; the server detects the event's edge on this counter.
pub const BODY_COLLISION_COUNT: &str = "body.collision_count";

/// The race (only in worlds with gates): 1 once gate 0 has started the clock.
pub const RACE_RUNNING: &str = "race.running";
/// The gate to fly through next (0 before the start and again once every gate is passed).
pub const RACE_NEXT_GATE: &str = "race.next_gate";
/// Laps completed.
pub const RACE_LAPS: &str = "race.laps";
/// Simulated time the current lap started.
pub const RACE_LAP_START_S: &str = "race.lap_start_s";
/// The last completed lap's time, and the fastest; both 0 until a lap completes.
pub const RACE_LAST_LAP_S: &str = "race.last_lap_s";
pub const RACE_BEST_LAP_S: &str = "race.best_lap_s";

pub const BATTERY_VOLTAGE: &str = "battery.voltage_v";
pub const BATTERY_CURRENT: &str = "battery.current_a";
pub const BATTERY_SOC: &str = "battery.soc";
pub const BATTERY_CONSUMED: &str = "battery.consumed_mah";

/// One cell of the series string: its terminal voltage (V).
pub fn battery_cell_voltage(i: usize) -> String {
    format!("battery.cell.{i}.voltage_v")
}

/// One cell's state of charge, 0..1.
pub fn battery_cell_soc(i: usize) -> String {
    format!("battery.cell.{i}.soc")
}

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
/// Uplink RSSI as the receiver reports it, at the antenna in use.
pub const RADIO_RSSI: &str = "radio.rssi_dbm";
/// Uplink SNR in dB at the antenna in use (LoRa decodes below the noise, so it may be negative).
pub const RADIO_SNR: &str = "radio.snr_db";
/// Index (into the quad file's radio antennas) of the antenna in use.
pub const RADIO_ANTENNA: &str = "radio.antenna";
/// Downlink link quality: percent of the last 100 packets the handset received.
pub const RADIO_DOWNLINK_LQ: &str = "radio.downlink_lq";
/// Fault: 1 = every packet is lost, uplink and downlink alike (the receiver and the handset both go silent).
pub const FAULT_RADIO_LINK_LOSS: &str = "fault.radio.link_loss";
/// Fault seams (M4b): the vehicle writes them once per set_fault; models read them each step.
pub const FAULT_RADIO_LOSS_FRACTION: &str = "fault.radio.loss_fraction";
/// Fault: the share of downlink packets the handset loses, in [0, 1]. The uplink is untouched.
pub const FAULT_RADIO_DOWNLINK_LOSS_FRACTION: &str = "fault.radio.downlink_loss_fraction";
pub const FAULT_RADIO_BURST_ON_S: &str = "fault.radio.burst_on_s";
pub const FAULT_RADIO_BURST_OFF_S: &str = "fault.radio.burst_off_s";

pub fn fault_motor_scale(i: usize) -> String {
    format!("fault.motor.{i}.scale")
}

pub fn fault_prop_thrust_scale(i: usize) -> String {
    format!("fault.prop.{i}.thrust_scale")
}

/// Per-motor static imbalance (part data, overwritten by PropDamage while active).
pub fn prop_imbalance(i: usize) -> String {
    format!("prop.{i}.imbalance_kgm")
}

/// Air coming up through the prop's disc (m/s): the mount's speed along body +Z, positive while the aircraft
/// descends. Published by the rigid body, read by the propeller a tick later.
pub fn prop_inflow(i: usize) -> String {
    format!("prop.{i}.inflow_mps")
}

/// The ESC's temperature (°C), from the thermal model; read by the KISS telemetry frames.
pub fn esc_temp(i: usize) -> String {
    format!("esc.{i}.temp_c")
}

/// The motor winding's temperature (°C).
pub fn motor_temp(i: usize) -> String {
    format!("motor.{i}.temp_c")
}

/// The video transmitter's temperature (°C); Tramp's status reply reports it.
pub const VTX_TEMP_C: &str = "vtx.temp_c";

/// Integrated rotor phase, wrapped to [0, 2π) — the vibration model's phase input.
pub fn motor_theta(i: usize) -> String {
    format!("motor.{i}.theta_rad")
}

pub const FAULT_BATTERY_R_OFFSET_OHM: &str = "fault.battery.r_offset_ohm";
pub const FAULT_BATTERY_V_OFFSET_V: &str = "fault.battery.v_offset_v";
/// Cell index + 1; 0 = no weak cell.
pub const FAULT_BATTERY_WEAK_CELL: &str = "fault.battery.weak_cell";

pub const FAULT_IMU_GYRO_BIAS: &str = "fault.imu.gyro_bias_radps";
pub const FAULT_IMU_NOISE_SCALE: &str = "fault.imu.noise_scale";
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
/// 1.0 when the receiver completed the field, 0.0 when it dropped it (M3e); 0.0 while nothing transmits.
pub const VIDEO_FIELD_VALID: &str = "video.field_valid";

/// Received power in dBm at one receiver antenna, by its name in the world file.
pub fn video_rssi(antenna: &str) -> String {
    format!("video.rssi_dbm.{antenna}")
}
