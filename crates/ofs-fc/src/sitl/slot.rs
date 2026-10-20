//! Slots: how several simulator processes share one machine's Betaflight SITL ports (issue #15).
//!
//! Every SITL instance runs on a block of ten ports (`--port-offset` = slot x 10, see [`PortMap`]); slot 0 is the
//! stock ports. A simulator takes the first free slot by *holding sockets*, which the OS releases when the process
//! exits however it exits, so a crashed simulator frees its slot with no stale lock file to clean up:
//!
//! 1. a UDP "lock" socket on `127.0.0.1:21600 + slot`: if it cannot be bound, another simulator (or anything else)
//!    owns the slot;
//! 2. the motor socket on `bind_ip:9002 + offset`, the one the simulator needs anyway: if it cannot be bound, some
//!    foreign process holds the slot's ports, and the slot is skipped;
//! 3. the slot's TCP UART block (`5761 + offset` and the seven above it, [`PortMap::uart_ports`]), which Betaflight
//!    binds at launch: each port is probed bind-and-release on `bind_ip` — the address this session's own sockets
//!    use — so a foreign holder skips the slot before it can turn the launch into a confusing bind failure. Under
//!    WSL that address is the Windows side of the NAT, deliberately not loopback: the OS's own loopback relay for
//!    the VM's listeners (`wslrelay.exe`) binds the loopback ports of SITL's UARTs and outlives the SITL, which a
//!    loopback probe once misread as every slot taken.
//!
//! A bind that fails for a reason *other* than the port being taken (permissions, socket exhaustion) skips the slot
//! too, but is remembered and named in the error when no slot could be taken ([`FcError::Acquire`]).
//!
//! A session rebuilt on the same machine (a reload) keeps its slot instead of racing another simulator's acquire for
//! the first free one: it carries its [`SlotClaim`] across the rebuild and retakes it through
//! [`SitlSlot::from_claim`], so the lock is never released and the firmware directory stays the session's own.
//!
//! Two simulators on the same operating system share this lock domain. A Windows simulator and a simulator running
//! inside WSL do not, so launching both into one WSL VM can pick the same slot, and the second one's cleanup removes
//! the first one's SITL there. Run all simulators from one side of the machine (`docs/dev-setup.md`).
use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, UdpSocket};

use super::codec::{PortMap, PORT_PWM, SLOT_PORT_STRIDE};
use super::FcError;

/// How many simulators can hold a slot at once (offsets 0 to 150).
pub const MAX_SLOTS: u16 = 16;
/// The loopback UDP port of slot 0's lock; slot n locks `LOCK_PORT_BASE + n`. Below every OS's default ephemeral
/// range (Linux 32768–60999, Windows and macOS 49152–65535): the OS must not hand a lock port to an outbound socket.
pub const LOCK_PORT_BASE: u16 = 21600;

/// A slot held by this process: its ports, the motor socket bound on them and the lock. Dropping it frees the slot.
#[derive(Debug)]
pub struct SitlSlot {
    pub index: u16,
    pub ports: PortMap,
    /// The motor socket (`ports.pwm()`), already bound: SITL's motor packets arrive here.
    pub rx: UdpSocket,
    _lock: UdpSocket,
}

impl SitlSlot {
    /// Takes the first free slot (see the module docs). `bind_ip` is where the motor socket binds (the Windows host
    /// address under WSL's NAT, loopback otherwise).
    pub fn acquire(bind_ip: Ipv4Addr) -> Result<SitlSlot, FcError> {
        Self::acquire_in(bind_ip, LOCK_PORT_BASE, 0)
    }

    /// As [`SitlSlot::acquire`] in a shifted port space: the locks start at `lock_base` and every slot's ports are
    /// `port_shift` higher. Tests use it to stay away from a real SITL's ports and from each other.
    pub fn acquire_in(bind_ip: Ipv4Addr, lock_base: u16, port_shift: u16) -> Result<SitlSlot, FcError> {
        // Binds and probes that failed a way other than "taken" (issue #90): each is named when no slot can be had.
        let mut failures: Vec<String> = Vec::new();
        for index in 0..MAX_SLOTS {
            let ports = PortMap { offset: port_shift + index * SLOT_PORT_STRIDE };
            // A failed bind drops what the slot already holds, so it stays free for whoever can use it.
            let lock = match bind_udp(Ipv4Addr::LOCALHOST, lock_base + index) {
                Ok(lock) => lock,
                Err(None) => continue,
                Err(Some(failure)) => {
                    failures.push(failure);
                    continue;
                }
            };
            let rx = match bind_udp(bind_ip, ports.pwm()) {
                Ok(rx) => rx,
                Err(None) => continue,
                Err(Some(failure)) => {
                    failures.push(failure);
                    continue;
                }
            };
            match probe_tcp_block(bind_ip, ports) {
                Ok(()) => {}
                Err(None) => continue, // a port of the block is taken: drops the lock and the motor socket
                Err(Some(failure)) => {
                    failures.push(failure);
                    continue;
                }
            }
            return Ok(SitlSlot { index, ports, rx, _lock: lock });
        }
        if !failures.is_empty() {
            return Err(FcError::Acquire { details: failures.join("; ") });
        }
        Err(FcError::PortInUse {
            port: PORT_PWM + port_shift,
            hint: "every simulator slot is taken (16 simulators, or other programs on those ports, are running)",
        })
    }

    /// Retakes the slot of a claim kept across a session rebuild (a reload; see [`SitlSlot::into_parts`]): the lock
    /// never left this process, so no other simulator's acquire can interleave with the rebuild and it cannot land
    /// in another slot's firmware directory. Only the motor socket is bound again — the old session's was released
    /// with its bridge — so a process that took it in that window is a [`FcError::PortInUse`].
    pub fn from_claim(claim: SlotClaim, bind_ip: Ipv4Addr) -> Result<SitlSlot, FcError> {
        let SlotClaim { index, ports, _lock } = claim;
        let rx = UdpSocket::bind(SocketAddr::from((bind_ip, ports.pwm()))).map_err(|_| FcError::PortInUse {
            port: ports.pwm(),
            hint: "the slot kept across the reload had its motor port taken by another process",
        })?;
        Ok(SitlSlot { index, ports, rx, _lock })
    }

    /// Splits the motor socket off, for its user to own outright (no duplicated handle: on Windows a duplicate is a
    /// second handle to the socket, which a child process can inherit and keep alive after the session is gone); what
    /// remains is the claim on the slot's number, to be held for the session's life.
    pub fn into_parts(self) -> (UdpSocket, SlotClaim) {
        (self.rx, SlotClaim { index: self.index, ports: self.ports, _lock: self._lock })
    }
}

/// One UDP bind of the acquire: `Err(None)` — the port is taken, skip the slot — or `Err(Some(what))` — the bind
/// failed a way other than that; the slot is still skipped, but the failure is named if no slot can be taken.
fn bind_udp(ip: Ipv4Addr, port: u16) -> Result<UdpSocket, Option<String>> {
    UdpSocket::bind(SocketAddr::from((ip, port)))
        .map_err(|e| (e.kind() != ErrorKind::AddrInUse).then(|| format!("binding UDP port {port} failed: {e}")))
}

/// Probes every TCP port of the slot's UART block bind-and-release on `bind_ip`, the address this session's own
/// sockets use (the slot is skipped while the probe holds nothing): `Err(None)` — a port is taken — or
/// `Err(Some(what))` — a probe failed a way other than that (named like the UDP binds). Natively `bind_ip` is
/// loopback, so the probe sees every holder; under WSL it is the Windows side of the NAT, where SITL's UARTs really
/// live in the VM — and where the OS's loopback relay for them must not count as a holder (see the module docs).
fn probe_tcp_block(bind_ip: Ipv4Addr, ports: PortMap) -> Result<(), Option<String>> {
    for port in ports.uart_ports() {
        match TcpListener::bind(SocketAddr::from((bind_ip, port))) {
            Ok(listener) => drop(listener), // free: released at once; Betaflight binds it when it launches
            Err(e) if e.kind() == ErrorKind::AddrInUse => return Err(None),
            Err(e) => return Err(Some(format!("probing TCP port {port} failed: {e}"))),
        }
    }
    Ok(())
}

/// A slot's lock without its motor socket (see [`SitlSlot::into_parts`]). Dropping it frees the slot's number.
#[derive(Debug)]
pub struct SlotClaim {
    pub index: u16,
    pub ports: PortMap,
    _lock: UdpSocket,
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::codec::MSP_TCP_PORT;

    const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;

    /// A port space of its own per test (the tests run in threads of one process): locks at `LOCK_PORT_BASE + 100 n`
    /// and motor ports 9_002 + 200 n + 10 slot.
    fn space(n: u16) -> (u16, u16) {
        (LOCK_PORT_BASE + 100 * n, 200 * n)
    }

    /// Acquires the slot that should be free. On macOS (both runners) a closed socket's port was sometimes still held a
    /// moment later in these tests (most likely a child process another test thread was spawning at that instant
    /// inherited the descriptor before it was marked close-on-exec, and kept it until it exited): there the slot is
    /// retried for up to two seconds. Everywhere else a slot that is not free at once is a failure.
    fn acquire_until(index: u16, lock: u16, shift: u16) -> SitlSlot {
        let patience = if cfg!(target_os = "macos") { std::time::Duration::from_secs(2) } else { std::time::Duration::ZERO };
        let deadline = std::time::Instant::now() + patience;
        loop {
            let slot = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap();
            if slot.index == index || std::time::Instant::now() >= deadline {
                return slot;
            }
            drop(slot);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn the_first_acquire_takes_slot_zero_and_its_motor_port() {
        let (lock, shift) = space(1);
        let slot = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap();
        assert_eq!((slot.index, slot.ports.offset), (0, shift));
        assert_eq!(slot.rx.local_addr().unwrap().port(), PORT_PWM + shift);
    }

    #[test]
    fn a_second_acquire_while_the_first_lives_takes_the_next_slot() {
        let (lock, shift) = space(2);
        let first = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap();
        let second = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap();
        assert_eq!((first.index, second.index), (0, 1));
        assert_eq!(second.ports.offset, first.ports.offset + SLOT_PORT_STRIDE);
        assert_eq!(second.rx.local_addr().unwrap().port(), first.rx.local_addr().unwrap().port() + SLOT_PORT_STRIDE);
    }

    #[test]
    fn dropping_a_slot_frees_it_for_the_next_acquire() {
        let (lock, shift) = space(3);
        let first = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap();
        let second = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap();
        drop(first);
        let third = acquire_until(0, lock, shift);
        assert_eq!((second.index, third.index), (1, 0), "the lowest free slot is reused");
    }

    /// The bridge owns the motor socket and holds the claim: the slot stays taken while either lives, and is free once
    /// both are gone.
    #[test]
    fn a_split_slot_stays_taken_until_its_socket_and_claim_are_both_dropped() {
        let (lock, shift) = space(7);
        let (rx, claim) = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap().into_parts();
        assert_eq!((claim.index, claim.ports.offset, rx.local_addr().unwrap().port()), (0, shift, PORT_PWM + shift));
        assert_eq!(acquire_until(1, lock, shift).index, 1, "slot 0 is taken by the parts");
        drop(claim);
        assert_eq!(acquire_until(1, lock, shift).index, 1, "the socket alone still holds the motor port");
        drop(rx);
        assert_eq!(acquire_until(0, lock, shift).index, 0, "both gone: slot 0 is free again");
    }

    #[test]
    fn a_foreign_holder_of_the_motor_port_skips_the_slot_and_leaves_it_free() {
        let (lock, shift) = space(4);
        let foreign = UdpSocket::bind(SocketAddr::from((LOOPBACK, PORT_PWM + shift))).unwrap();
        let slot = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap();
        assert_eq!(slot.index, 1, "slot 0's motor port is taken by someone else");
        drop(foreign);
        let next = acquire_until(0, lock, shift);
        assert_eq!(next.index, 0, "the failed attempt did not keep slot 0's lock");
    }

    #[test]
    fn a_foreign_holder_of_the_lock_port_skips_the_slot() {
        let (lock, shift) = space(5);
        let _foreign = UdpSocket::bind(SocketAddr::from((LOOPBACK, lock))).unwrap();
        let slot = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap();
        assert_eq!(slot.index, 1);
    }

    #[test]
    fn all_slots_taken_is_a_port_in_use_error_naming_a_motor_port() {
        let (lock, shift) = space(6);
        let held: Vec<SitlSlot> = (0..MAX_SLOTS).map(|_| SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap()).collect();
        assert_eq!(held.iter().map(|s| s.index).collect::<Vec<_>>(), (0..MAX_SLOTS).collect::<Vec<_>>());
        let err = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap_err();
        assert!(matches!(err, FcError::PortInUse { port, .. } if port == PORT_PWM + shift), "{err}");
        assert!(err.to_string().contains("every simulator slot is taken"), "{err}");
    }

    #[test]
    fn a_foreign_holder_of_a_tcp_port_of_the_slots_block_skips_the_slot() {
        let (lock, shift) = space(9);
        let foreign = std::net::TcpListener::bind(SocketAddr::from((LOOPBACK, MSP_TCP_PORT + shift + 3))).unwrap();
        let slot = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap();
        assert_eq!(slot.index, 1, "slot 0's TCP block is held by someone else");
        drop(foreign);
        let next = acquire_until(0, lock, shift);
        assert_eq!(next.index, 0, "the skip left slot 0 free for whoever can use it");
    }

    /// Under WSL the OS's loopback relay for SITL's VM listeners (`wslrelay.exe`) binds the UART ports on loopback
    /// and outlives the SITL; a live run's acquire once skipped every slot because of it. The probe watches the
    /// session's own address, so a loopback holder — the relay — never pushes a WSL session off its slot. 127.0.0.2
    /// stands in for the WSL gateway address here.
    #[test]
    #[cfg_attr(target_os = "macos", ignore = "macOS has no 127.0.0.2 on lo0 (a second local address needs `sudo ifconfig lo0 alias 127.0.0.2`)")]
    fn a_loopback_holder_does_not_push_a_session_on_another_address_off_its_slot() {
        let (lock, shift) = space(13);
        let _relay = std::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, MSP_TCP_PORT + shift))).unwrap();
        let slot = SitlSlot::acquire_in(Ipv4Addr::new(127, 0, 0, 2), lock, shift).unwrap();
        assert_eq!(slot.index, 0, "the probe watches the session's own address, not loopback from elsewhere");
    }

    /// #90's reload rule at the slot layer: a rebuild through the kept claim retakes *its* slot — and so its
    /// firmware directory — even when a lower slot has freed up meanwhile (a fresh acquire would take that one).
    #[test]
    fn a_kept_claim_retakes_its_own_slot_not_the_first_free_one() {
        let (lock, shift) = space(10);
        let other = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap(); // another simulator: slot 0
        let mine = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap(); // this one: slot 1
        let (rx, claim) = mine.into_parts();
        drop(rx); // the old session's bridge went with it
        drop(other); // and the other simulator quit mid-reload: slot 0 is free again
        let reloaded = SitlSlot::from_claim(claim, LOOPBACK).unwrap();
        assert_eq!(reloaded.index, 1, "the reload keeps slot 1");
        assert_eq!(reloaded.rx.local_addr().unwrap().port(), PORT_PWM + shift + SLOT_PORT_STRIDE);
    }

    #[test]
    fn a_kept_claim_whose_motor_port_was_taken_in_the_rebuild_window_is_an_error() {
        let (lock, shift) = space(12);
        let (rx, claim) = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap().into_parts();
        drop(rx);
        // The thief takes the port the moment it is free (on macOS that was sometimes a moment after the drop).
        let thief_addr = SocketAddr::from((LOOPBACK, PORT_PWM + shift));
        let patience = if cfg!(target_os = "macos") { std::time::Duration::from_secs(2) } else { std::time::Duration::ZERO };
        let deadline = std::time::Instant::now() + patience;
        let _thief = loop {
            match UdpSocket::bind(thief_addr) {
                Ok(socket) => break socket,
                Err(e) if std::time::Instant::now() >= deadline => panic!("the motor port was not released: {e}"),
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        };
        let err = SitlSlot::from_claim(claim, LOOPBACK).unwrap_err();
        assert!(matches!(err, FcError::PortInUse { port, .. } if port == PORT_PWM + shift), "{err}");
    }

    /// A bind that fails for a reason other than the port being taken (here: the motor bind to an address the
    /// machine does not have) must be named in the error, not reported as sixteen taken slots.
    #[test]
    fn a_bind_failure_that_is_not_in_use_is_named_not_every_slot_taken() {
        let (lock, shift) = space(11);
        let err = SitlSlot::acquire_in(Ipv4Addr::new(192, 0, 2, 1), lock, shift).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("no simulator slot could be taken"), "{text}");
        assert!(text.contains(&format!("UDP port {}", PORT_PWM + shift)), "{text}");
        assert!(!text.contains("every simulator slot is taken"), "{text}");
    }
}
