//! Lockstep exchange with Betaflight SITL: send one state datagram (sensors + RC), wait for one motor packet.
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};

use super::codec::{state_datagram, RcPacket, ServoPacket, PORT_PWM, PORT_STATE};
use super::frames::{fdm_packet, motor_commands, rc_channels, Home, SensorFrame};
use super::net::SitlNet;
use super::process::{LaunchConfig, SitlProcess};
use super::FcError;

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub launch: LaunchConfig,
    pub net: SitlNet,
    pub rate_divisor: u32,
    /// The first exchange can arrive before SITL's main loop runs; allow seconds. The first datagram is
    /// resent every [`FIRST_RESEND_INTERVAL`] until SITL answers or this elapses.
    pub first_reply_timeout: Duration,
    pub reply_timeout: Duration,
    pub home: Home,
    pub motor_count: usize,
}

struct Inputs {
    gyro: Signal<DVec3>,
    accel: Signal<DVec3>,
    att: Signal<DQuat>,
    vel: Signal<DVec3>,
    pos: Signal<DVec3>,
    pressure: Signal<f64>,
    roll: Signal<f64>,
    pitch: Signal<f64>,
    yaw: Signal<f64>,
    throttle: Signal<f64>,
    aux: Vec<Signal<f64>>,
}

pub struct SitlBridge {
    cfg: BridgeConfig,
    rx: UdpSocket,
    tx: UdpSocket,
    inputs: Inputs,
    cmds: Vec<Signal<f64>>,
    proc: SitlProcess,
    answered: bool,
}

/// How often the first state datagram is resent while SITL has not answered it.
pub const FIRST_RESEND_INTERVAL: Duration = Duration::from_millis(250);

impl SitlBridge {
    pub fn start(cfg: BridgeConfig, bus: &mut Bus) -> Result<Self, FcError> {
        assert!(cfg.motor_count <= 4, "Betaflight SITL's servo_packet carries 4 motors");
        let net = cfg.net;
        let rx = UdpSocket::bind(SocketAddr::from((net.bind_ip, PORT_PWM)))
            .map_err(|_| FcError::PortInUse { port: PORT_PWM, hint: "another simulator instance may be running" })?;
        if net.send_ip.is_loopback() {
            // Native SITL must be able to bind its state port; probe and release it to fail early.
            // (Under WSL the port lives inside the VM; the cleanup command and bind-failure check cover it.)
            UdpSocket::bind(SocketAddr::from((net.send_ip, PORT_STATE))).map_err(|_| FcError::PortInUse {
                port: PORT_STATE,
                hint: "a Betaflight SITL instance may still be running (see fc.cleanup / OFS_SITL_CLEANUP)",
            })?;
        }
        let tx = UdpSocket::bind(SocketAddr::from((net.bind_ip, 0)))?;
        let inputs = Inputs {
            gyro: bus.signal(names::IMU_GYRO),
            accel: bus.signal(names::IMU_ACCEL),
            att: bus.signal(names::BODY_ATT),
            vel: bus.signal(names::BODY_VEL_NED),
            pos: bus.signal(names::BODY_POS_NED),
            pressure: bus.signal(names::BARO_PRESSURE),
            roll: bus.signal(names::RC_ROLL),
            pitch: bus.signal(names::RC_PITCH),
            yaw: bus.signal(names::RC_YAW),
            throttle: bus.signal(names::RC_THROTTLE),
            aux: (0..names::RC_AUX_COUNT).map(|i| bus.signal(&names::rc_aux(i))).collect(),
        };
        let cmds = (0..cfg.motor_count).map(|i| bus.signal(&names::motor_cmd(i))).collect();
        let mut launch = cfg.launch.clone();
        if let Some(ip) = net.sitl_ip_arg {
            launch.launch.extend(["--ip".to_string(), ip.to_string()]);
        }
        let proc = SitlProcess::start(&launch)?;
        Ok(Self { cfg, rx, tx, inputs, cmds, proc, answered: false })
    }

    fn drain(&self) -> std::io::Result<()> {
        self.rx.set_nonblocking(true)?;
        let mut buf = [0u8; 128];
        while self.rx.recv_from(&mut buf).is_ok() {}
        self.rx.set_nonblocking(false)
    }

    fn firmware_error(&mut self, what: &str) -> SimError {
        let state = match self.proc.exit_status() {
            Some(status) => format!("SITL exited with {status}"),
            None => format!("{what} (no reply: datagram lost or SITL hung)"),
        };
        SimError::Firmware(format!("{state}; last output:\n{}", self.proc.log_tail()))
    }
}

fn send(tx: &UdpSocket, bytes: &[u8], to: SocketAddr) {
    // Windows reports an earlier ICMP "port unreachable" as an error on a later send; a missing
    // reply is detected on receive instead, so send errors are ignored here.
    let _ = tx.send_to(bytes, to);
}

/// A receive error that only means "nothing arrived (yet)".
fn no_reply(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset)
}

/// Sends `datagram` to `to` and waits on `rx` for one reply, resending the same datagram every
/// `resend_every` until a reply arrives or `timeout` elapses (then `ErrorKind::TimedOut`).
/// Returns the reply length in `buf` and how many times the datagram was resent.
///
/// Lockstep SITL only replies to datagrams it received, so a lost first datagram (state port not yet
/// bound, first packet through the WSL NAT dropped) is never recovered by waiting alone.
pub fn exchange_with_resend(
    rx: &UdpSocket,
    tx: &UdpSocket,
    to: SocketAddr,
    datagram: &[u8],
    buf: &mut [u8],
    timeout: Duration,
    resend_every: Duration,
) -> std::io::Result<(usize, u32)> {
    let deadline = Instant::now() + timeout;
    send(tx, datagram, to);
    let mut next_send = Instant::now() + resend_every;
    let mut resends = 0;
    loop {
        let mut now = Instant::now();
        if now >= deadline {
            return Err(ErrorKind::TimedOut.into());
        }
        if now >= next_send {
            send(tx, datagram, to);
            resends += 1;
            now = Instant::now();
            next_send = now + resend_every;
        }
        let wait = next_send.min(deadline).saturating_duration_since(now).max(Duration::from_millis(1));
        rx.set_read_timeout(Some(wait))?;
        match rx.recv_from(buf) {
            Ok((n, _)) => return Ok((n, resends)),
            Err(e) if no_reply(&e) => {}
            Err(e) => return Err(e),
        }
    }
}

impl Model for SitlBridge {
    fn name(&self) -> &str {
        "fc.sitl"
    }

    fn rate_divisor(&self) -> u32 {
        self.cfg.rate_divisor
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        // Exchanges stay paired: drop any stray reply (e.g. SITL answering both copies of a resent first
        // datagram) before sending (M0 §7).
        self.drain().map_err(|e| SimError::Firmware(format!("UDP setup failed: {e}")))?;
        let i = &self.inputs;
        let frame = SensorFrame {
            time_s: ctx.time_s,
            gyro_frd_radps: bus.get(i.gyro),
            accel_frd_mps2: bus.get(i.accel),
            att_ned: bus.get(i.att),
            vel_ned_mps: bus.get(i.vel),
            pos_ned_m: bus.get(i.pos),
            pressure_pa: bus.get(i.pressure),
        };
        let aux: Vec<f64> = i.aux.iter().map(|s| bus.get(*s)).collect();
        let channels = rc_channels(bus.get(i.roll), bus.get(i.pitch), bus.get(i.yaw), bus.get(i.throttle), &aux);
        let rc = RcPacket { timestamp_s: ctx.time_s, channels };
        let to = SocketAddr::from((self.cfg.net.send_ip, PORT_STATE));
        let datagram = state_datagram(&fdm_packet(&frame, &self.cfg.home), &rc);

        let mut buf = [0u8; 128];
        let received = if self.answered {
            send(&self.tx, &datagram, to);
            self.rx.recv_from(&mut buf).map(|(n, _)| n)
        } else {
            let timeout = self.cfg.first_reply_timeout;
            exchange_with_resend(&self.rx, &self.tx, to, &datagram, &mut buf, timeout, FIRST_RESEND_INTERVAL).map(|(n, resends)| {
                if resends > 0 {
                    tracing::warn!(
                        "SITL did not answer the first state datagram; resent it {resends} time(s). SITL may have run an \
                         extra t=0 tick, so firmware determinism is only claimed for runs without a resend"
                    );
                }
                n
            })
        };
        match received {
            Ok(n) => {
                let packet = ServoPacket::decode(&buf[..n])
                    .ok_or_else(|| SimError::Firmware(format!("malformed motor packet ({n} bytes)")))?;
                for (sig, v) in self.cmds.iter().zip(motor_commands(&packet)) {
                    bus.set(*sig, v);
                }
                if !self.answered {
                    self.answered = true;
                    self.rx
                        .set_read_timeout(Some(self.cfg.reply_timeout))
                        .map_err(|e| SimError::Firmware(format!("UDP setup failed: {e}")))?;
                }
                Ok(())
            }
            Err(e) if no_reply(&e) => {
                let waited = if self.answered { self.cfg.reply_timeout } else { self.cfg.first_reply_timeout };
                let what = format!("no motor output within {} ms", waited.as_millis());
                Err(self.firmware_error(&what))
            }
            Err(e) => Err(SimError::Firmware(format!("UDP receive failed: {e}"))),
        }
    }
}
