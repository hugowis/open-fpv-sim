//! What Betaflight told the handset, decoded from the CRSF telemetry frames that flew a downlink slot.
use std::sync::{Arc, Mutex};

use crate::crsf;

/// What Betaflight told the handset, decoded from the CRSF telemetry frames that flew a downlink
/// slot. Units are Betaflight's own CRSF encodings (see docs/research/elrs-link.md): the battery
/// voltage in 0.1 V and current in 0.1 A (`getLegacyBatteryVoltage()`/`getAmperage()/10`), attitude
/// in radians x 10 000 with yaw wrapped to ±180°, the flight mode verbatim.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FcTelemetry {
    pub battery_v: f64,
    pub battery_a: f64,
    pub battery_mah: f64,
    pub roll_deg: f64,
    pub pitch_deg: f64,
    pub yaw_deg: f64,
    pub flight_mode: String,
    /// Simulated time of the last frame that flew a slot (0 until one arrives).
    pub last_frame_s: f64,
    /// Frames that flew a slot so far (a monotonic "anything arrived yet" marker).
    pub frame_count: u64,
}

/// The latest [`FcTelemetry`], shared with the server's state snapshot (the `OsdHandle` pattern).
#[derive(Debug, Clone, Default)]
pub struct FcTelemetryHandle(Arc<Mutex<FcTelemetry>>);

impl FcTelemetryHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn latest(&self) -> FcTelemetry {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn store(&self, t: FcTelemetry) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = t;
    }
}

/// Decodes one telemetry frame that flew a downlink slot into the shared handle. Frame types we do not expose
/// still consume their slot (the radio forwarded them; nothing more is modelled).
pub fn apply_frame(fc: &FcTelemetryHandle, frame_type: u8, payload: &[u8], now_s: f64) {
    let mut t = fc.latest();
    match frame_type {
        crsf::FRAMETYPE_BATTERY_SENSOR if payload.len() >= 7 => {
            t.battery_v = f64::from(u16::from_be_bytes([payload[0], payload[1]])) / 10.0;
            t.battery_a = f64::from(u16::from_be_bytes([payload[2], payload[3]])) / 10.0;
            t.battery_mah = f64::from(u32::from_be_bytes([0, payload[4], payload[5], payload[6]]));
        }
        crsf::FRAMETYPE_ATTITUDE if payload.len() >= 6 => {
            let deg = |raw: [u8; 2]| f64::from(i16::from_be_bytes(raw)) * 180.0 / (10_000.0 * std::f64::consts::PI);
            t.pitch_deg = deg([payload[0], payload[1]]);
            t.roll_deg = deg([payload[2], payload[3]]);
            t.yaw_deg = deg([payload[4], payload[5]]);
        }
        crsf::FRAMETYPE_FLIGHT_MODE => {
            t.flight_mode = String::from_utf8_lossy(payload).split('\0').next().unwrap_or("").to_owned();
        }
        _ => {}
    }
    t.frame_count += 1;
    t.last_frame_s = now_s;
    fc.store(t);
}
