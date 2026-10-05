//! CRSF framing, byte-compatible with Betaflight's serial RX (src/main/rx/crsf.c, crsf_protocol.h):
//! `[address][length][type][payload...][crc]`, where length counts type + payload + CRC and the CRC is
//! CRC-8/DVB-S2 over type + payload.

pub const ADDRESS_FLIGHT_CONTROLLER: u8 = 0xC8;
/// Addresses a frame can start with (flight controller, radio transmitter, receiver, TX module).
pub const SYNC_ADDRESSES: [u8; 4] = [0xC8, 0xEA, 0xEC, 0xEE];
pub const FRAMETYPE_LINK_STATISTICS: u8 = 0x14;
pub const FRAMETYPE_RC_CHANNELS_PACKED: u8 = 0x16;
pub const CHANNEL_COUNT: usize = 16;
pub const RC_CHANNELS_PAYLOAD_SIZE: usize = 22;
pub const LINK_STATISTICS_PAYLOAD_SIZE: usize = 10;
/// Whole frame including the address and length bytes (Betaflight's CRSF_FRAME_SIZE_MAX).
pub const FRAME_SIZE_MAX: usize = 64;

/// CRC-8/DVB-S2: polynomial 0xD5, initial value 0.
pub fn crc8_dvb_s2(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for b in bytes {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0xD5 } else { crc << 1 };
        }
    }
    crc
}

/// Stick position in [-1, 1] to CRSF ticks: 1000..2000 us maps to 192..1792 ticks (1500 us = 992).
/// Betaflight turns ticks back into microseconds with us = 0.62477 * ticks + 881 (crsfReadRawRC);
/// this scale was verified against SITL in M0 (docs/research/sitl-interface.md §5).
pub fn stick_ticks(x: f64) -> u16 {
    (992.0 + 800.0 * x.clamp(-1.0, 1.0)).round() as u16
}

/// Throttle in [0, 1] to CRSF ticks (0 = 1000 us = 192 ticks, 1 = 2000 us = 1792 ticks).
pub fn throttle_ticks(t: f64) -> u16 {
    (192.0 + 1600.0 * t.clamp(0.0, 1.0)).round() as u16
}

/// Betaflight's conversion back to microseconds, truncated as MSP_RC reports it.
pub fn ticks_to_us(ticks: u16) -> u16 {
    (0.624_771_201_952_41 * f64::from(ticks) + 881.0) as u16
}

/// Packs 16 channels of 11 bits each, little-endian bit order (channel 0 in the lowest bits).
pub fn pack_channels(channels: &[u16; CHANNEL_COUNT]) -> [u8; RC_CHANNELS_PAYLOAD_SIZE] {
    let mut out = [0u8; RC_CHANNELS_PAYLOAD_SIZE];
    for (i, &ch) in channels.iter().enumerate() {
        let value = ch & 0x7FF;
        for bit in 0..11 {
            if (value >> bit) & 1 == 1 {
                let pos = i * 11 + bit;
                out[pos / 8] |= 1u8 << (pos % 8);
            }
        }
    }
    out
}

pub fn unpack_channels(payload: &[u8; RC_CHANNELS_PAYLOAD_SIZE]) -> [u16; CHANNEL_COUNT] {
    let mut out = [0u16; CHANNEL_COUNT];
    for (i, ch) in out.iter_mut().enumerate() {
        for bit in 0..11 {
            let pos = i * 11 + bit;
            if (payload[pos / 8] >> (pos % 8)) & 1 == 1 {
                *ch |= 1u16 << bit;
            }
        }
    }
    out
}

fn frame(frame_type: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(payload.len() + 4);
    f.push(ADDRESS_FLIGHT_CONTROLLER);
    f.push(u8::try_from(payload.len() + 2).expect("CRSF payload too long"));
    f.push(frame_type);
    f.extend_from_slice(payload);
    let crc = crc8_dvb_s2(&f[2..]);
    f.push(crc);
    f
}

pub fn rc_channels_frame(channels: &[u16; CHANNEL_COUNT]) -> Vec<u8> {
    frame(FRAMETYPE_RC_CHANNELS_PACKED, &pack_channels(channels))
}

/// LINK_STATISTICS payload (Betaflight's crsfLinkStatistics_t). RSSI fields hold -dBm (60 = -60 dBm).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LinkStatistics {
    pub uplink_rssi_1: u8,
    pub uplink_rssi_2: u8,
    pub uplink_lq: u8,
    pub uplink_snr: i8,
    pub active_antenna: u8,
    pub rf_mode: u8,
    pub uplink_tx_power: u8,
    pub downlink_rssi: u8,
    pub downlink_lq: u8,
    pub downlink_snr: i8,
}

impl LinkStatistics {
    pub fn encode(&self) -> [u8; LINK_STATISTICS_PAYLOAD_SIZE] {
        [
            self.uplink_rssi_1,
            self.uplink_rssi_2,
            self.uplink_lq,
            self.uplink_snr as u8,
            self.active_antenna,
            self.rf_mode,
            self.uplink_tx_power,
            self.downlink_rssi,
            self.downlink_lq,
            self.downlink_snr as u8,
        ]
    }

    pub fn decode(p: &[u8; LINK_STATISTICS_PAYLOAD_SIZE]) -> Self {
        Self {
            uplink_rssi_1: p[0],
            uplink_rssi_2: p[1],
            uplink_lq: p[2],
            uplink_snr: p[3] as i8,
            active_antenna: p[4],
            rf_mode: p[5],
            uplink_tx_power: p[6],
            downlink_rssi: p[7],
            downlink_lq: p[8],
            downlink_snr: p[9] as i8,
        }
    }
}

pub fn link_statistics_frame(stats: &LinkStatistics) -> Vec<u8> {
    frame(FRAMETYPE_LINK_STATISTICS, &stats.encode())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    RcChannels([u16; CHANNEL_COUNT]),
    LinkStatistics(LinkStatistics),
    Other { address: u8, frame_type: u8, payload: Vec<u8> },
}

/// Streaming CRSF parser: feed it bytes in any chunking. It skips to the next sync address after a bad
/// length or CRC, never buffers more than one frame, and no input makes it panic.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
    crc_errors: u64,
}

impl Decoder {
    pub fn crc_errors(&self) -> u64 {
        self.crc_errors
    }

    /// Bytes held while waiting for the rest of a frame.
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<Frame> {
        self.buf.extend_from_slice(bytes);
        let mut frames = Vec::new();
        loop {
            match self.buf.iter().position(|b| SYNC_ADDRESSES.contains(b)) {
                Some(start) => {
                    self.buf.drain(..start);
                }
                None => {
                    self.buf.clear();
                    break;
                }
            }
            if self.buf.len() < 2 {
                break;
            }
            let len = usize::from(self.buf[1]); // type + payload + CRC
            if !(2..=FRAME_SIZE_MAX - 2).contains(&len) {
                self.buf.remove(0);
                continue;
            }
            if self.buf.len() < len + 2 {
                break;
            }
            let body = &self.buf[2..len + 1];
            if crc8_dvb_s2(body) != self.buf[len + 1] {
                self.crc_errors += 1;
                self.buf.remove(0);
                continue;
            }
            frames.push(parse(self.buf[0], body[0], &body[1..]));
            self.buf.drain(..len + 2);
        }
        frames
    }
}

fn parse(address: u8, frame_type: u8, payload: &[u8]) -> Frame {
    match frame_type {
        FRAMETYPE_RC_CHANNELS_PACKED => {
            if let Ok(p) = <&[u8; RC_CHANNELS_PAYLOAD_SIZE]>::try_from(payload) {
                return Frame::RcChannels(unpack_channels(p));
            }
        }
        FRAMETYPE_LINK_STATISTICS => {
            if let Ok(p) = <&[u8; LINK_STATISTICS_PAYLOAD_SIZE]>::try_from(payload) {
                return Frame::LinkStatistics(LinkStatistics::decode(p));
            }
        }
        _ => {}
    }
    Frame::Other { address, frame_type, payload: payload.to_vec() }
}
