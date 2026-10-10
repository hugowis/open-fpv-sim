//! Lockstep exchange with Betaflight SITL: send one state datagram (sensors, plus bytes for SITL's UARTs),
//! wait for one motor packet. A Betaflight reboot (e.g. a Configurator save) relaunches SITL.
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx, Wire};

use super::codec::{
    parse_reply_serial, state_datagram_with_serial, RcPacket, ReplySerial, ServoPacket, PORT_PWM, PORT_STATE,
    REPLY_MAX, SERIAL_BLOCK_HEADER, SERIAL_SECTION_MAX,
};
use super::frames::{fdm_packet, motor_commands, Home, SensorFrame};
use super::net::SitlNet;
use super::process::{run_cleanup, LaunchConfig, SitlProcess};
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
    /// Bytes for SITL's UARTs, carried in the state datagram (e.g. the receiver's CRSF into UART2).
    pub serial: Vec<SerialLink>,
    /// Betaflight's UART TX bytes, routed to the models that listen (OSD, VTX)
    pub taps: Vec<SerialTap>,
}

/// Bytes the simulator feeds into one of SITL's UARTs.
#[derive(Debug, Clone)]
pub struct SerialLink {
    /// 0-based: UART2 = 1.
    pub uart_index: u8,
    pub rx: Wire,
}

/// Bytes Betaflight writes to one of its UARTs (UART2 and up), as returned in each SITL reply.
#[derive(Debug, Clone)]
pub struct SerialTap {
    /// 0-based: UART4 = 3.
    pub uart_index: u8,
    pub tx: Wire,
}

/// Hands the reply's UART blocks to the taps wired to them; blocks of UARTs nobody listens to are discarded.
pub fn route_reply(taps: &[SerialTap], serial: &ReplySerial) {
    for (uart, bytes) in &serial.blocks {
        if let Some(tap) = taps.iter().find(|t| t.uart_index == *uart) {
            tap.tx.write(bytes);
        }
    }
}

struct Inputs {
    gyro: Signal<DVec3>,
    accel: Signal<DVec3>,
    att: Signal<DQuat>,
    vel: Signal<DVec3>,
    pos: Signal<DVec3>,
    pressure: Signal<f64>,
}

pub struct SitlBridge {
    cfg: BridgeConfig,
    /// What was launched (with `--ip` under WSL); launched again when Betaflight reboots.
    launch: LaunchConfig,
    rx: UdpSocket,
    tx: UdpSocket,
    inputs: Inputs,
    cmds: Vec<Signal<f64>>,
    restarts: Signal<f64>,
    serial_dropped: Signal<f64>,
    proc: Option<SitlProcess>,
    answered: bool,
    restart_budget: RestartBudget,
    /// Bytes the serial links had dropped at the last step (added to `fc.serial_dropped_bytes` as they grow).
    links_dropped: u64,
}

/// How often the first state datagram is resent while SITL has not answered it.
pub const FIRST_RESEND_INTERVAL: Duration = Duration::from_millis(250);
/// After a resent first datagram, how long a second reply (SITL received both copies) may take to arrive.
pub const LATE_REPLY_SETTLE: Duration = Duration::from_millis(100);
/// At most this many Betaflight reboots within [`RESTART_WINDOW`]; more is a reboot loop, reported as an error.
pub const MAX_RESTARTS_IN_WINDOW: usize = 5;
pub const RESTART_WINDOW: Duration = Duration::from_secs(10);

/// Remembers recent restarts to stop a reboot loop.
#[derive(Debug, Default)]
struct RestartBudget(std::collections::VecDeque<Instant>);

impl RestartBudget {
    fn allow(&mut self, now: Instant) -> bool {
        while self.0.front().is_some_and(|t| now.duration_since(*t) > RESTART_WINDOW) {
            self.0.pop_front();
        }
        if self.0.len() >= MAX_RESTARTS_IN_WINDOW {
            return false;
        }
        self.0.push_back(now);
        true
    }
}

impl SitlBridge {
    pub fn start(cfg: BridgeConfig, bus: &mut Bus) -> Result<Self, FcError> {
        if cfg.motor_count > 4 {
            return Err(FcError::Config(format!(
                "Betaflight SITL's servo_packet carries 4 motors; the quad has {}",
                cfg.motor_count
            )));
        }
        let net = cfg.net;
        let rx = UdpSocket::bind(SocketAddr::from((net.bind_ip, PORT_PWM)))
            .map_err(|_| FcError::PortInUse { port: PORT_PWM, hint: "another simulator instance may be running" })?;
        if net.send_ip.is_loopback() {
            // Native SITL must be able to bind its state port; probe and release it to fail early.
            // (Under WSL the port lives inside the VM; the cleanup command and bind-failure check cover it.)
            probe_state_port(net.send_ip, &cfg.launch.cleanup)?;
        }
        let tx = UdpSocket::bind(SocketAddr::from((net.bind_ip, 0)))?;
        let inputs = Inputs {
            gyro: bus.signal(names::IMU_GYRO),
            accel: bus.signal(names::IMU_ACCEL),
            att: bus.signal(names::BODY_ATT),
            vel: bus.signal(names::BODY_VEL_NED),
            pos: bus.signal(names::BODY_POS_NED),
            pressure: bus.signal(names::BARO_PRESSURE),
        };
        let cmds = (0..cfg.motor_count).map(|i| bus.signal(&names::motor_cmd(i))).collect();
        let restarts = bus.signal(names::FC_RESTARTS);
        let serial_dropped = bus.signal(names::FC_SERIAL_DROPPED);
        let mut launch = cfg.launch.clone();
        if let Some(ip) = net.sitl_ip_arg {
            launch.launch.extend(["--ip".to_string(), ip.to_string()]);
        }
        let proc = SitlProcess::start(&launch)?;
        Ok(Self {
            cfg,
            launch,
            rx,
            tx,
            inputs,
            cmds,
            restarts,
            serial_dropped,
            proc: Some(proc),
            answered: false,
            restart_budget: RestartBudget::default(),
            links_dropped: 0,
        })
    }

    fn drain(&self) -> std::io::Result<()> {
        self.rx.set_nonblocking(true)?;
        let mut buf = [0u8; REPLY_MAX];
        while self.rx.recv_from(&mut buf).is_ok() {}
        self.rx.set_nonblocking(false)
    }

    fn firmware_error(&mut self, what: &str) -> SimError {
        let Some(proc) = self.proc.as_mut() else {
            return SimError::Firmware(format!("{what} (SITL is not running)"));
        };
        let state = match proc.exit_status() {
            Some(status) => format!("SITL exited with {status}"),
            None => format!("{what} (no reply: datagram lost or SITL hung)"),
        };
        SimError::Firmware(format!("{state}; last output:\n{}", proc.log_tail()))
    }

    /// One send/receive. Until SITL has answered once, the datagram is resent (see [`exchange_with_resend`]).
    fn exchange(&mut self, datagram: &[u8], buf: &mut [u8]) -> std::io::Result<usize> {
        let to = SocketAddr::from((self.cfg.net.send_ip, PORT_STATE));
        if self.answered {
            send(&self.tx, datagram, to);
            return recv_from_ip(&self.rx, buf, to.ip());
        }
        let timeout = self.cfg.first_reply_timeout;
        let (n, resends) = exchange_with_resend(&self.rx, &self.tx, to, datagram, buf, timeout, FIRST_RESEND_INTERVAL)?;
        if resends > 0 {
            // SITL may have received more than one copy and answer each: drop the late replies now, so the next
            // exchange does not take one of them for its own.
            drain_late_replies(&self.rx, LATE_REPLY_SETTLE)?;
            tracing::warn!(
                "SITL did not answer the first state datagram; resent it {resends} time(s). SITL may have run an \
                 extra tick at this instant, so firmware determinism is only claimed for runs without a resend"
            );
        }
        self.answered = true;
        self.rx.set_read_timeout(Some(self.cfg.reply_timeout))?;
        Ok(n)
    }

    /// Adds the bytes the serial links into SITL dropped (a full link, e.g. a stalled exchange) to
    /// `fc.serial_dropped_bytes`, so they are reported like the bytes SITL drops.
    fn count_link_drops(&mut self, bus: &mut Bus) {
        let dropped: u64 = self.cfg.serial.iter().map(|l| l.rx.dropped()).sum();
        if dropped > self.links_dropped {
            let new = dropped - self.links_dropped;
            self.links_dropped = dropped;
            tracing::warn!("{new} byte(s) for Betaflight's UARTs were dropped: a serial link was full");
            bus.set(self.serial_dropped, bus.get(self.serial_dropped) + new as f64);
        }
    }

    fn rebooted(&mut self) -> bool {
        self.proc.as_mut().is_some_and(|p| p.rebooted())
    }

    /// Betaflight rebooted: launch SITL again from its EEPROM. SITL's clock restarts from the next packet.
    fn restart(&mut self) -> Result<(), SimError> {
        if !self.restart_budget.allow(Instant::now()) {
            return Err(SimError::Firmware(format!(
                "Betaflight rebooted more than {MAX_RESTARTS_IN_WINDOW} times within {} s; stopping (a reboot loop)",
                RESTART_WINDOW.as_secs()
            )));
        }
        // Drop the old process first: its cleanup command would kill the new instance.
        self.proc = None;
        let proc = SitlProcess::relaunch(&self.launch)
            .map_err(|e| SimError::Firmware(format!("relaunching Betaflight SITL after a reboot failed: {e}")))?;
        self.proc = Some(proc);
        self.answered = false;
        Ok(())
    }
}

/// Checks that SITL's state port is free. If it is held (typically by a SITL orphaned by a killed
/// simulator), runs the cleanup command and waits briefly for the port to be released.
fn probe_state_port(ip: Ipv4Addr, cleanup: &[String]) -> Result<(), FcError> {
    let free = || UdpSocket::bind(SocketAddr::from((ip, PORT_STATE))).is_ok();
    if free() {
        return Ok(());
    }
    if !cleanup.is_empty() {
        run_cleanup(cleanup);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
            if free() {
                return Ok(());
            }
        }
    }
    Err(FcError::PortInUse {
        port: PORT_STATE,
        hint: "a Betaflight SITL instance may still be running (see fc.cleanup / OFS_SITL_CLEANUP)",
    })
}

/// Takes pending UART bytes for one datagram, within SITL's serial section limit; what does not fit stays in the
/// links for the next datagram.
pub fn take_serial_blocks(links: &[SerialLink]) -> Vec<(u8, Vec<u8>)> {
    let mut blocks = Vec::new();
    let mut room = SERIAL_SECTION_MAX;
    for link in links {
        if room <= SERIAL_BLOCK_HEADER {
            break;
        }
        let bytes = link.rx.take(room - SERIAL_BLOCK_HEADER);
        if !bytes.is_empty() {
            room -= SERIAL_BLOCK_HEADER + bytes.len();
            blocks.push((link.uart_index, bytes));
        }
    }
    blocks
}

/// Receives one datagram from `from`; datagrams from any other address (not SITL) are discarded.
fn recv_from_ip(rx: &UdpSocket, buf: &mut [u8], from: IpAddr) -> std::io::Result<usize> {
    loop {
        let (n, sender) = rx.recv_from(buf)?;
        if sender.ip() == from {
            return Ok(n);
        }
        tracing::warn!("ignored a datagram from {sender} on the motor port (SITL is at {from})");
    }
}

/// Discards the replies that arrive on `rx` within `settle`. Returns how many there were.
pub fn drain_late_replies(rx: &UdpSocket, settle: Duration) -> std::io::Result<usize> {
    let deadline = Instant::now() + settle;
    let mut buf = [0u8; REPLY_MAX];
    let mut drained = 0;
    let previous = rx.read_timeout()?;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        rx.set_read_timeout(Some(left))?;
        match rx.recv_from(&mut buf) {
            Ok(_) => drained += 1,
            Err(e) if no_reply(&e) => {}
            Err(e) => {
                rx.set_read_timeout(previous)?;
                return Err(e);
            }
        }
    }
    rx.set_read_timeout(previous)?;
    Ok(drained)
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
            Ok((n, sender)) if sender.ip() == to.ip() => return Ok((n, resends)),
            Ok((_, sender)) => tracing::warn!("ignored a datagram from {sender} on the motor port (SITL is at {})", to.ip()),
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
        // Pilot input reaches Betaflight only as CRSF on its receiver UART (spec §1.5). The UDP RC channels are
        // zero: invalid pulses, so a SITL whose EEPROM still selects the UDP receiver fails safe instead of flying.
        let rc = RcPacket { timestamp_s: ctx.time_s, channels: [0; 16] };
        let fdm = fdm_packet(&frame, &self.cfg.home);
        // A first datagram may be resent, and SITL may receive more than one copy: it carries no UART bytes, so none
        // can reach Betaflight twice. They wait in the links for the next exchange.
        let serial = if self.answered { take_serial_blocks(&self.cfg.serial) } else { Vec::new() };
        let datagram = state_datagram_with_serial(&fdm, &rc, &serial);
        self.count_link_drops(bus);

        let mut buf = [0u8; REPLY_MAX];
        let mut received = self.exchange(&datagram, &mut buf);
        if matches!(&received, Err(e) if no_reply(e)) && self.rebooted() {
            self.restart()?;
            bus.set(self.restarts, bus.get(self.restarts) + 1.0);
            tracing::info!("Betaflight rebooted; SITL relaunched from its EEPROM");
            self.drain().map_err(|e| SimError::Firmware(format!("UDP setup failed: {e}")))?;
            // The UART bytes of the lost exchange went down with the old process, as on a real reboot.
            let first = state_datagram_with_serial(&fdm, &rc, &[]);
            received = self.exchange(&first, &mut buf);
        }
        match received {
            Ok(n) => {
                let packet = ServoPacket::decode(&buf[..n])
                    .ok_or_else(|| SimError::Firmware(format!("malformed motor packet ({n} bytes)")))?;
                for (sig, v) in self.cmds.iter().zip(motor_commands(&packet)) {
                    bus.set(*sig, v);
                }
                let serial = parse_reply_serial(&buf[ServoPacket::SIZE..n])
                    .map_err(|e| SimError::Firmware(format!("malformed SITL reply: {e}")))?;
                route_reply(&self.cfg.taps, &serial);
                if serial.dropped > 0 {
                    bus.set(self.serial_dropped, bus.get(self.serial_dropped) + f64::from(serial.dropped));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reboots_in_quick_succession_are_capped() {
        let mut budget = RestartBudget::default();
        let t0 = Instant::now();
        for i in 0..MAX_RESTARTS_IN_WINDOW {
            assert!(budget.allow(t0 + Duration::from_millis(100 * i as u64)), "restart {i}");
        }
        assert!(!budget.allow(t0 + Duration::from_secs(1)), "a reboot loop must stop");
        assert!(budget.allow(t0 + RESTART_WINDOW + Duration::from_secs(1)), "reboots spread over time are fine");
    }
}
