//! The Tramp protocol from the VTX's side (M3e spec §6), checked against `vtx_tramp.c` of the pinned
//! Betaflight (2026.6.2; see the plan's ruling 1): 16-byte frames `0x0F cmd <12-byte payload> crc 0x00`, the
//! CRC a u8 sum of bytes 1..=13 (the 0x0F excluded — `trampChecksum`). The FC asks with 'r' (limits), 'v'
//! (settings), 'F'/'P'/'I' (frequency / power in mW / pit state) and only accepts 'r', 'v' and 's' back; 'F',
//! 'P' and 'I' are confirmed by the next 'v' at least 200 ms later.

/// The payload's u16 fields are little endian (the FC composes `param & 0xff`, `(param >> 8) & 0xff`).
pub const TRAMP_FRAME_LENGTH: usize = 16;
pub const CMD_RESET: u8 = b'r';
pub const CMD_GET_SETTINGS: u8 = b'v';
/// The status query; the FC's `trampQuery('s')` sends a `'s'`-commanded frame, and its answer carries the same code.
pub const CMD_GET_STATUS: u8 = b's';
pub const CMD_SET_FREQ: u8 = b'F';
pub const CMD_SET_POWER: u8 = b'P';
pub const CMD_SET_ACTIVE: u8 = b'I';
pub const RESP_STATUS: u8 = b's';

/// The CRC Betaflight's Tramp driver recomputes: bytes 1..=13, the sync byte excluded.
pub fn crc(frame: &[u8]) -> u8 {
    frame[1..14].iter().fold(0u8, |sum, b| sum.wrapping_add(*b))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Reset,
    GetSettings,
    /// The status query ('s'): the FC reads the temperature out of the answer.
    GetStatus,
    SetFrequency(u16),
    SetPowerMw(u16),
    /// true: transmit (leave pit mode); false: enter pit mode.
    SetActive(bool),
}

/// A response frame as the FC's receive framer accepts it: any 16-byte frame with a valid CRC; the FC itself
/// reads only 'r', 'v' and 's' codes.
pub fn response_frame(code: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = vec![0x0F, code];
    f.extend_from_slice(payload);
    f.resize(14, 0);
    f.push(crc(&f));
    f.push(0x00);
    f
}

#[derive(Debug, Default)]
pub struct Parser {
    buf: [u8; TRAMP_FRAME_LENGTH],
    pos: usize,
    bad_frames: u32,
}

impl Parser {
    /// Feeds bytes; returns the complete, valid requests in order. A frame with a bad CRC or an unknown
    /// command is ignored (as a real VTX would ignore it) and counted in [`bad_frames`](Self::bad_frames).
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Request> {
        let mut out = Vec::new();
        for &b in bytes {
            // Resync: hunt the 0x0F sync byte at a frame boundary, then fill 15 more.
            if self.pos == 0 && b != 0x0F {
                continue;
            }
            self.buf[self.pos] = b;
            self.pos += 1;
            if self.pos < TRAMP_FRAME_LENGTH {
                continue;
            }
            self.pos = 0;
            let frame = &self.buf;
            if crc(frame) == frame[14] && frame[15] == 0x00 {
                match Request::decode(frame) {
                    Some(request) => out.push(request),
                    None => self.bad_frames += 1,
                }
            } else {
                self.bad_frames += 1;
            }
        }
        out
    }

    pub fn bad_frames(&self) -> u32 {
        self.bad_frames
    }
}

impl Request {
    fn decode(frame: &[u8; TRAMP_FRAME_LENGTH]) -> Option<Request> {
        let param = u16::from_le_bytes([frame[2], frame[3]]);
        let rest_zero = frame[4..14].iter().all(|b| *b == 0);
        match frame[1] {
            CMD_RESET if rest_zero => Some(Request::Reset),
            CMD_GET_SETTINGS if rest_zero => Some(Request::GetSettings),
            CMD_GET_STATUS if rest_zero => Some(Request::GetStatus),
            CMD_SET_FREQ if rest_zero => Some(Request::SetFrequency(param)),
            CMD_SET_POWER if rest_zero => Some(Request::SetPowerMw(param)),
            CMD_SET_ACTIVE if rest_zero => Some(Request::SetActive(param != 0)),
            _ => None,
        }
    }
}
