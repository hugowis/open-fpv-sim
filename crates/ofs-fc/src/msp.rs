//! MSP v1 over TCP: enough to talk to Betaflight SITL's MSP port (UART1, tcp:5761) from tests and tools.
//! SITL services MSP only while simulated time advances, so [`MspClient::request`] pumps the simulation
//! while it waits for the reply.
use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use ofs_core::SimError;

pub const MSP_API_VERSION: u8 = 1;
pub const MSP_REBOOT: u8 = 68;
pub const MSP_STATUS: u8 = 101;
pub const MSP_RC: u8 = 105;
pub const MSP_BATTERY_STATE: u8 = 130;

fn checksum(len: u8, cmd: u8, payload: &[u8]) -> u8 {
    payload.iter().fold(len ^ cmd, |c, b| c ^ b)
}

/// `$M<` request frame: length, command, payload, XOR checksum of length, command and payload.
pub fn encode_request(cmd: u8, payload: &[u8]) -> Vec<u8> {
    let len = u8::try_from(payload.len()).expect("MSP v1 payloads are at most 255 bytes");
    let mut out = b"$M<".to_vec();
    out.push(len);
    out.push(cmd);
    out.extend_from_slice(payload);
    out.push(checksum(len, cmd, payload));
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MspReply {
    pub cmd: u8,
    pub payload: Vec<u8>,
    /// `$M!`: Betaflight did not accept the command.
    pub error: bool,
}

/// Parses `$M>` and `$M!` frames from a byte stream; skips noise and frames with bad checksums.
#[derive(Debug, Default)]
pub struct MspParser {
    buf: Vec<u8>,
}

impl MspParser {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<MspReply> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            match self.buf.iter().position(|b| *b == b'$') {
                Some(start) => {
                    self.buf.drain(..start);
                }
                None => {
                    self.buf.clear();
                    break;
                }
            }
            if self.buf.len() < 5 {
                break;
            }
            let error = match &self.buf[1..3] {
                b"M>" => false,
                b"M!" => true,
                _ => {
                    self.buf.remove(0);
                    continue;
                }
            };
            let len = usize::from(self.buf[3]);
            if self.buf.len() < 6 + len {
                break;
            }
            let cmd = self.buf[4];
            let payload = self.buf[5..5 + len].to_vec();
            if checksum(self.buf[3], cmd, &payload) == self.buf[5 + len] {
                out.push(MspReply { cmd, payload, error });
                self.buf.drain(..6 + len);
            } else {
                self.buf.remove(0);
            }
        }
        out
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MspError {
    #[error("MSP I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("no MSP reply to command {cmd} after {pumps} simulation steps")]
    Timeout { cmd: u8, pumps: usize },
    #[error(transparent)]
    Sim(#[from] SimError),
}

pub struct MspClient {
    stream: TcpStream,
    parser: MspParser,
    replies: VecDeque<MspReply>,
}

impl MspClient {
    pub fn connect(addr: SocketAddr, timeout: Duration) -> Result<Self, MspError> {
        let stream = TcpStream::connect_timeout(&addr, timeout)?;
        stream.set_nodelay(true)?;
        stream.set_nonblocking(true)?;
        Ok(Self { stream, parser: MspParser::default(), replies: VecDeque::new() })
    }

    pub fn send(&mut self, cmd: u8, payload: &[u8]) -> Result<(), MspError> {
        let bytes = encode_request(cmd, payload);
        self.stream.set_nonblocking(false)?;
        let written = self.stream.write_all(&bytes);
        self.stream.set_nonblocking(true)?;
        Ok(written?)
    }

    /// Reads whatever has arrived, without blocking; returns the oldest reply to `cmd`, if any. A reply that
    /// arrived just before the connection closed (e.g. MSP_REBOOT's) is still returned.
    pub fn poll(&mut self, cmd: u8) -> Result<Option<MspReply>, MspError> {
        let mut buf = [0u8; 1024];
        let mut closed = false;
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => {
                    closed = true;
                    break;
                }
                Ok(n) => {
                    let replies = self.parser.push(&buf[..n]);
                    self.replies.extend(replies);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
        if let Some(i) = self.replies.iter().position(|r| r.cmd == cmd) {
            return Ok(self.replies.remove(i));
        }
        if closed {
            return Err(std::io::Error::new(ErrorKind::UnexpectedEof, "MSP connection closed").into());
        }
        Ok(None)
    }

    /// Sends `cmd`, then calls `pump` (which must advance simulated time) until the reply arrives or
    /// `max_pumps` calls have passed.
    pub fn request(
        &mut self,
        cmd: u8,
        payload: &[u8],
        max_pumps: usize,
        mut pump: impl FnMut() -> Result<(), SimError>,
    ) -> Result<MspReply, MspError> {
        self.send(cmd, payload)?;
        for _ in 0..max_pumps {
            pump()?;
            if let Some(reply) = self.poll(cmd)? {
                return Ok(reply);
            }
        }
        Err(MspError::Timeout { cmd, pumps: max_pumps })
    }
}

/// MSP_API_VERSION: (MSP protocol, API major, API minor).
pub fn api_version(reply: &MspReply) -> Option<(u8, u8, u8)> {
    match reply.payload[..] {
        [protocol, major, minor, ..] => Some((protocol, major, minor)),
        _ => None,
    }
}

/// MSP_STATUS: Betaflight's ARM box is bit 0 of the flight-mode flags (u32 at offset 6).
pub fn armed(reply: &MspReply) -> Option<bool> {
    let flags = reply.payload.get(6..10)?;
    Some(u32::from_le_bytes(flags.try_into().ok()?) & 1 == 1)
}

/// MSP_RC: channel values in microseconds, in Betaflight's internal order (roll, pitch, yaw, throttle, aux...).
pub fn rc_channels_us(reply: &MspReply) -> Vec<u16> {
    reply.payload.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}
