//! iBUS (FlySky) framing and receiver, byte-compatible with Betaflight's serial RX (src/main/rx/ibus.c): a 32-byte
//! frame `[0x20][0x40][14 channels, 16-bit little-endian, microseconds][checksum, 16-bit little-endian]`, where the
//! checksum is 0xFFFF minus the sum of the first 30 bytes. 115200 baud, 8N1, about every 7 ms
//! (docs/research/ibus-receiver.md has the sources and which figures are estimated).
use ofs_core::Wire;

use crate::parts::{Channels, Delivery, Receiver};

pub const FRAME_LEN: usize = 32;
pub const CHANNEL_COUNT: usize = 14;
/// The first two bytes: the frame length and the channel-data command.
pub const HEADER: [u8; 2] = [0x20, 0x40];
/// The microsecond range a FlySky receiver sends (estimated, docs/research/ibus-receiver.md).
pub const US_MIN: u16 = 1000;
pub const US_MAX: u16 = 2000;
/// Seconds between frames (estimated: the issue says about 7 ms).
pub const FRAME_PERIOD_S: f64 = 0.007;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IbusError {
    Length(usize),
    Header([u8; 2]),
    Checksum,
}

/// 0xFFFF minus the sum of the bytes before the checksum.
pub fn checksum(bytes: &[u8; FRAME_LEN - 2]) -> u16 {
    0xFFFF - bytes.iter().map(|b| u16::from(*b)).sum::<u16>()
}

pub fn frame(us: &[u16; CHANNEL_COUNT]) -> [u8; FRAME_LEN] {
    let mut f = [0u8; FRAME_LEN];
    f[..2].copy_from_slice(&HEADER);
    for (i, v) in us.iter().enumerate() {
        // The high nibble belongs to channels 15 to 18 (Betaflight keeps 12 bits); the clamp keeps it clear.
        f[2 + 2 * i..4 + 2 * i].copy_from_slice(&(*v).clamp(US_MIN, US_MAX).to_le_bytes());
    }
    let sum = checksum(f[..FRAME_LEN - 2].try_into().expect("30 bytes"));
    f[FRAME_LEN - 2..].copy_from_slice(&sum.to_le_bytes());
    f
}

/// The channels of a checked frame, 12 bits each as Betaflight reads them.
pub fn decode(bytes: &[u8]) -> Result<[u16; CHANNEL_COUNT], IbusError> {
    let f: &[u8; FRAME_LEN] = bytes
        .try_into()
        .map_err(|_| IbusError::Length(bytes.len()))?;
    if f[..2] != HEADER {
        return Err(IbusError::Header([f[0], f[1]]));
    }
    if checksum(f[..FRAME_LEN - 2].try_into().expect("30 bytes"))
        != u16::from_le_bytes([f[30], f[31]])
    {
        return Err(IbusError::Checksum);
    }
    let mut us = [0u16; CHANNEL_COUNT];
    for (i, v) in us.iter_mut().enumerate() {
        *v = u16::from_le_bytes([f[2 + 2 * i], f[3 + 2 * i] & 0x0F]);
    }
    Ok(us)
}

/// The sim's CRSF ticks (1000 us = 192, 2000 us = 1792) to microseconds, clamped to the FlySky range.
pub fn ticks_to_us(ticks: u16) -> u16 {
    (f64::from(US_MIN) + (f64::from(ticks) - 192.0) * 0.625)
        .round()
        .clamp(f64::from(US_MIN), f64::from(US_MAX)) as u16
}

/// Microseconds (clamped to the FlySky range) back to the sim's ticks.
pub fn us_to_ticks(us: u16) -> u16 {
    (192.0 + (f64::from(us.clamp(US_MIN, US_MAX)) - f64::from(US_MIN)) * 1.6).round() as u16
}

/// A frame for the first 14 of the sim's channels (iBUS has no more).
pub fn rc_frame(channels: &Channels) -> [u8; FRAME_LEN] {
    let mut us = [0u16; CHANNEL_COUNT];
    for (v, t) in us.iter_mut().zip(channels) {
        *v = ticks_to_us(*t);
    }
    frame(&us)
}

/// The receiver on the quad: the last delivered channels as an iBUS frame about every 7 ms of packet time, none while
/// the link delivers nothing. No random draws and no downlink.
pub struct IbusReceiver {
    /// Seconds one packet slot lasts.
    slot_s: f64,
    /// The flight controller's UART RX: the receiver writes here.
    uart: Wire,
    /// The flight controller's UART TX: iBUS has no telemetry here, so what arrives is dropped.
    telemetry_in: Wire,
    /// Packet time since the last frame, at most one period (so a frame follows the first fresh packet at once).
    since_frame_s: f64,
    latest: Option<Channels>,
    fresh: bool,
}

impl IbusReceiver {
    pub fn new(packet_rate_hz: u32, uart: Wire, telemetry_in: Wire) -> Self {
        assert!(packet_rate_hz > 0, "the packet rate must be positive");
        Self {
            slot_s: 1.0 / f64::from(packet_rate_hz),
            uart,
            telemetry_in,
            since_frame_s: FRAME_PERIOD_S,
            latest: None,
            fresh: false,
        }
    }
}

impl Receiver for IbusReceiver {
    fn ingest_telemetry(&mut self) {
        self.telemetry_in.take(usize::MAX);
    }

    fn emit(&mut self, delivery: &Delivery) {
        if let Some(channels) = delivery.channels {
            self.latest = Some(channels);
            self.fresh = true;
        }
        self.since_frame_s += self.slot_s;
        // Epsilon: 7 ms is not a whole number of slots, so the sum can land a few ulps short of the period.
        if self.since_frame_s < FRAME_PERIOD_S - 1e-9 {
            return;
        }
        match self.latest.filter(|_| self.fresh) {
            Some(channels) => {
                self.uart.write(&rc_frame(&channels));
                self.fresh = false;
                self.since_frame_s = (self.since_frame_s - FRAME_PERIOD_S).min(FRAME_PERIOD_S);
            }
            None => self.since_frame_s = FRAME_PERIOD_S,
        }
    }

    fn next_telemetry(&mut self) -> Option<(u8, Vec<u8>)> {
        None
    }
}
