//! SmartAudio v2.1 from the VTX's side: parses the requests Betaflight's driver sends and builds the responses it
//! accepts (checked against `vtx_smartaudio.c` of the pinned Betaflight, 2026.6.2).

/// CRC-8, polynomial 0xD5, initial value 0. Requests carry it over the whole frame before the CRC byte
/// (`AA 55 <cmd> <len> <payload>`); responses over `<code> <len> <payload>` only (Betaflight's `saReceiveFrame`).
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for byte in data {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0xD5 } else { crc << 1 };
        }
    }
    crc
}

pub const CMD_GET_SETTINGS: u8 = 0x01;
pub const CMD_SET_POWER: u8 = 0x02;
pub const CMD_SET_CHANNEL: u8 = 0x03;
pub const CMD_SET_FREQUENCY: u8 = 0x04;
pub const CMD_SET_MODE: u8 = 0x05;
/// Response code of a v2.1 settings frame.
pub const RESP_SETTINGS_V21: u8 = 0x11;

/// Mode byte of a settings response.
pub const MODE_FREQUENCY: u8 = 0x01;
pub const MODE_PIT: u8 = 0x02;
pub const MODE_UNLOCKED: u8 = 0x10;
/// SET_MODE request bits.
pub const SET_MODE_PIT_IN_RANGE: u8 = 0x01;
pub const SET_MODE_PIT_OUT_OF_RANGE: u8 = 0x02;
pub const SET_MODE_CLEAR_PIT: u8 = 0x04;
pub const SET_MODE_UNLOCK: u8 = 0x08;
/// SET_FREQUENCY: bit 14 asks for the pit-mode frequency, bit 15 sets it.
pub const FREQ_GET_PIT: u16 = 1 << 14;
pub const FREQ_SET_PIT: u16 = 1 << 15;
pub const FREQ_MASK: u16 = !(FREQ_GET_PIT | FREQ_SET_PIT);
/// SET_POWER: bit 7 set means the low bits are dBm (v2.1).
pub const POWER_IS_DBM: u8 = 0x80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    GetSettings,
    SetPower(u8),
    SetChannel(u8),
    SetFrequency(u16),
    SetMode(u8),
}

/// A request frame as Betaflight writes it (tests and fake flight controllers).
pub fn request_frame(code: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![0xAA, 0x55, (code << 1) | 1, payload.len() as u8];
    frame.extend_from_slice(payload);
    frame.push(crc8(&frame));
    frame
}

/// A response frame as Betaflight's receive framer accepts it.
pub fn response_frame(code: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![0xAA, 0x55, code, payload.len() as u8];
    frame.extend_from_slice(payload);
    let crc = crc8(&frame[2..]);
    frame.push(crc);
    frame
}

/// The payload of a v2.1 settings response: `[channel, power(dBm), mode, freq hi, freq lo, current dBm, N, 0,
/// the N dBm levels]`. Betaflight skips the leading zero level (`vtx_smartaudio.c`).
pub fn settings_payload(channel: u8, mode: u8, freq_mhz: u16, current_dbm: u8, levels_dbm: &[u8]) -> Vec<u8> {
    let mut p = vec![channel, current_dbm, mode];
    p.extend_from_slice(&freq_mhz.to_be_bytes());
    p.push(current_dbm);
    p.push(levels_dbm.len() as u8);
    p.push(0);
    p.extend_from_slice(levels_dbm);
    p
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Preamble1,
    Preamble2,
    Command,
    Length,
    Payload,
    Crc,
}

const MAX_PAYLOAD: usize = 16;

/// Reassembles request frames from the bytes Betaflight writes to the VTX UART.
#[derive(Debug, Default)]
pub struct RequestParser {
    state: State,
    command: u8,
    length: usize,
    payload: Vec<u8>,
    bad_frames: u32,
}

impl RequestParser {
    /// Feeds bytes; returns the complete, valid requests in order. Frames with a bad CRC, an unknown command or the
    /// wrong payload length are ignored (as a real VTX would) and counted in [`bad_frames`](Self::bad_frames).
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Request> {
        let mut out = Vec::new();
        for &b in bytes {
            match self.state {
                State::Preamble1 => {
                    if b == 0xAA {
                        self.state = State::Preamble2;
                    }
                }
                State::Preamble2 => {
                    self.state = match b {
                        0x55 => State::Command,
                        0xAA => State::Preamble2,
                        _ => State::Preamble1,
                    };
                }
                State::Command => {
                    self.command = b;
                    self.state = State::Length;
                }
                State::Length => {
                    self.length = usize::from(b);
                    self.payload.clear();
                    self.state = if self.length > MAX_PAYLOAD {
                        self.bad_frames += 1;
                        State::Preamble1
                    } else if self.length == 0 {
                        State::Crc
                    } else {
                        State::Payload
                    };
                }
                State::Payload => {
                    self.payload.push(b);
                    if self.payload.len() == self.length {
                        self.state = State::Crc;
                    }
                }
                State::Crc => {
                    let mut frame = vec![0xAA, 0x55, self.command, self.length as u8];
                    frame.extend_from_slice(&self.payload);
                    match (crc8(&frame) == b).then(|| self.decode()).flatten() {
                        Some(request) => out.push(request),
                        None => self.bad_frames += 1,
                    }
                    self.state = State::Preamble1;
                }
            }
        }
        out
    }

    pub fn bad_frames(&self) -> u32 {
        self.bad_frames
    }

    fn decode(&self) -> Option<Request> {
        if self.command & 1 == 0 {
            return None; // an even code is a response (or an echo of one), not a request
        }
        match (self.command >> 1, self.payload.as_slice()) {
            (CMD_GET_SETTINGS, []) => Some(Request::GetSettings),
            (CMD_SET_POWER, [value]) => Some(Request::SetPower(*value)),
            (CMD_SET_CHANNEL, [index]) => Some(Request::SetChannel(*index)),
            (CMD_SET_FREQUENCY, [hi, lo]) => Some(Request::SetFrequency(u16::from_be_bytes([*hi, *lo]))),
            (CMD_SET_MODE, [mode]) => Some(Request::SetMode(*mode)),
            _ => None,
        }
    }
}
