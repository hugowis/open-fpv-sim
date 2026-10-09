//! Byte layouts of Betaflight SITL's UDP packets (src/platform/SIMULATOR/target/SITL/target.h).

pub const PORT_PWM_RAW: u16 = 9001;
pub const PORT_PWM: u16 = 9002;
pub const PORT_STATE: u16 = 9003;
pub const PORT_RC: u16 = 9004;
/// UART1 (MSP) is served on TCP 5760 + 1.
pub const MSP_TCP_PORT: u16 = 5761;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FdmPacket {
    pub timestamp_s: f64,
    pub gyro_rpy_radps: [f64; 3],
    pub accel_xyz_mps2: [f64; 3],
    pub quat_wxyz: [f64; 4],
    pub velocity_xyz_mps: [f64; 3],
    pub position_xyz: [f64; 3],
    pub pressure_pa: f64,
}

impl FdmPacket {
    pub const SIZE: usize = 144;

    pub fn encode(&self) -> [u8; Self::SIZE] {
        let fields = std::iter::once(self.timestamp_s)
            .chain(self.gyro_rpy_radps)
            .chain(self.accel_xyz_mps2)
            .chain(self.quat_wxyz)
            .chain(self.velocity_xyz_mps)
            .chain(self.position_xyz)
            .chain(std::iter::once(self.pressure_pa));
        let mut out = [0u8; Self::SIZE];
        for (i, v) in fields.enumerate() {
            out[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RcPacket {
    pub timestamp_s: f64,
    pub channels: [u16; 16],
}

impl RcPacket {
    pub const SIZE: usize = 40;

    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut out = [0u8; Self::SIZE];
        out[..8].copy_from_slice(&self.timestamp_s.to_le_bytes());
        for (i, c) in self.channels.iter().enumerate() {
            out[8 + i * 2..10 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        out
    }
}

/// Lockstep datagram: `fdm_packet` followed by `rc_packet`, sent to `PORT_STATE` (patched SITL applies the RC
/// with the same tick, so stick input is deterministic). See docs/research/sitl-interface.md §4.
pub const STATE_DATAGRAM_SIZE: usize = FdmPacket::SIZE + RcPacket::SIZE;

pub fn state_datagram(fdm: &FdmPacket, rc: &RcPacket) -> [u8; STATE_DATAGRAM_SIZE] {
    let mut out = [0u8; STATE_DATAGRAM_SIZE];
    out[..FdmPacket::SIZE].copy_from_slice(&fdm.encode());
    out[FdmPacket::SIZE..].copy_from_slice(&rc.encode());
    out
}

/// Bytes for SITL's UARTs may follow the 184-byte state datagram, as blocks of
/// `[uart index (0-based)][length, u16 little-endian][bytes]`. The patched SITL hands them to the UART on the
/// tick that applies the packet, so receiver traffic is deterministic. At most this many bytes of blocks
/// (headers included) per datagram: `EXT_SERIAL_MAX` in third_party/betaflight/ofs-sitl.patch.
pub const SERIAL_SECTION_MAX: usize = 512;
pub const SERIAL_BLOCK_HEADER: usize = 3;

/// The state datagram followed by serial blocks `(uart index, bytes)`.
pub fn state_datagram_with_serial(fdm: &FdmPacket, rc: &RcPacket, blocks: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut out = state_datagram(fdm, rc).to_vec();
    for (uart, bytes) in blocks {
        let len = u16::try_from(bytes.len()).expect("serial block too long");
        out.push(*uart);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(bytes);
    }
    assert!(out.len() - STATE_DATAGRAM_SIZE <= SERIAL_SECTION_MAX, "serial section exceeds SITL's EXT_SERIAL_MAX");
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ServoPacket {
    pub motor_speed: [f32; 4],
}

impl ServoPacket {
    pub const SIZE: usize = 16;

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE {
            return None;
        }
        let mut motor_speed = [0f32; 4];
        for (i, m) in motor_speed.iter_mut().enumerate() {
            *m = f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().ok()?);
        }
        Some(Self { motor_speed })
    }
}

/// A SITL reply is the 16-byte servo packet, then `[dropped: u16 LE]` (TX bytes SITL had to drop since its
/// previous reply because a capture buffer overflowed), then the TX bytes of UART2 and up as blocks
/// `[uart index (0-based)][length, u16 LE][bytes]`, at most [`SERIAL_SECTION_MAX`] bytes of blocks. UART1 (MSP) is
/// never captured. See `third_party/betaflight/tools/add_serial_out_datagram.py`.
pub const REPLY_MAX: usize = ServoPacket::SIZE + 2 + SERIAL_SECTION_MAX;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplySerial {
    /// TX bytes SITL dropped since its previous reply.
    pub dropped: u16,
    /// `(uart index (0-based), bytes)`, in the order SITL sent them.
    pub blocks: Vec<(u8, Vec<u8>)>,
}

/// Parses the part of a reply after the servo packet. An empty trailer (a SITL built without the TX capture)
/// is a reply with no serial data.
pub fn parse_reply_serial(trailer: &[u8]) -> Result<ReplySerial, String> {
    if trailer.is_empty() {
        return Ok(ReplySerial::default());
    }
    if trailer.len() < 2 {
        return Err(format!("reply trailer is {} byte(s); expected 0 or at least 2", trailer.len()));
    }
    let dropped = u16::from_le_bytes([trailer[0], trailer[1]]);
    let mut blocks = Vec::new();
    let mut i = 2;
    while i < trailer.len() {
        if i + SERIAL_BLOCK_HEADER > trailer.len() {
            return Err(format!("truncated block header at offset {i}"));
        }
        let uart = trailer[i];
        let len = usize::from(u16::from_le_bytes([trailer[i + 1], trailer[i + 2]]));
        i += SERIAL_BLOCK_HEADER;
        if i + len > trailer.len() {
            return Err(format!("block for UART{} claims {len} byte(s), {} left", u16::from(uart) + 1, trailer.len() - i));
        }
        blocks.push((uart, trailer[i..i + len].to_vec()));
        i += len;
    }
    Ok(ReplySerial { dropped, blocks })
}

/// A reply datagram as the patched SITL sends it (fake SITLs in tests).
pub fn encode_reply(motors: [f32; 4], dropped: u16, blocks: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(REPLY_MAX);
    for m in motors {
        out.extend_from_slice(&m.to_le_bytes());
    }
    out.extend_from_slice(&dropped.to_le_bytes());
    for (uart, bytes) in blocks {
        let len = u16::try_from(bytes.len()).expect("serial block too long");
        out.push(*uart);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(bytes);
    }
    assert!(out.len() <= REPLY_MAX, "serial section exceeds SITL's EXT_SERIAL_MAX");
    out
}
