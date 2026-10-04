//! Lockstep exchange with Betaflight SITL: send one state datagram (sensors + RC), wait for one motor packet.
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

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
    /// The first exchange can arrive before SITL's main loop runs; allow seconds.
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
    first: bool,
    answered: bool,
}

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
        rx.set_read_timeout(Some(cfg.first_reply_timeout))?;
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
        Ok(Self { cfg, rx, tx, inputs, cmds, proc, first: true, answered: false })
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
            None => format!("{what} (SITL still running: hung?)"),
        };
        SimError::Firmware(format!("{state}; last output:\n{}", self.proc.log_tail()))
    }
}

fn send(tx: &UdpSocket, bytes: &[u8], to: SocketAddr) {
    // Windows reports an earlier ICMP "port unreachable" as an error on a later send; a missing
    // reply is detected on receive instead, so send errors are ignored here.
    let _ = tx.send_to(bytes, to);
}

impl Model for SitlBridge {
    fn name(&self) -> &str {
        "fc.sitl"
    }

    fn rate_divisor(&self) -> u32 {
        self.cfg.rate_divisor
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        if self.first {
            self.drain().map_err(|e| SimError::Firmware(format!("UDP setup failed: {e}")))?;
            self.first = false;
        }
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
        send(&self.tx, &state_datagram(&fdm_packet(&frame, &self.cfg.home), &rc), to);

        let mut buf = [0u8; 128];
        match self.rx.recv_from(&mut buf) {
            Ok((n, _)) => {
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
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset) => {
                let waited = if self.answered { self.cfg.reply_timeout } else { self.cfg.first_reply_timeout };
                let what = format!("no motor output within {} ms", waited.as_millis());
                Err(self.firmware_error(&what))
            }
            Err(e) => Err(SimError::Firmware(format!("UDP receive failed: {e}"))),
        }
    }
}
