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
