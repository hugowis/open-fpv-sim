//! Slots: how several simulator processes share one machine's Betaflight SITL ports (issue #15).
//!
//! Every SITL instance runs on a block of ten ports (`--port-offset` = slot x 10, see [`PortMap`]); slot 0 is the
//! stock ports. A simulator takes the first free slot by *holding sockets*, which the OS releases when the process
//! exits however it exits, so a crashed simulator frees its slot with no stale lock file to clean up:
//!
//! 1. a UDP "lock" socket on `127.0.0.1:21600 + slot`: if it cannot be bound, another simulator (or anything else)
//!    owns the slot;
//! 2. the motor socket on `bind_ip:9002 + offset`, the one the simulator needs anyway: if it cannot be bound, some
//!    foreign process holds the slot's ports, and the slot is skipped.
//!
//! Two simulators on the same operating system share this lock domain. A Windows simulator and a simulator running
//! inside WSL do not, so launching both into one WSL VM can pick the same slot, and the second one's cleanup removes
//! the first one's SITL there. Run all simulators from one side of the machine (`docs/dev-setup.md`).
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};

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
        for index in 0..MAX_SLOTS {
            let ports = PortMap { offset: port_shift + index * SLOT_PORT_STRIDE };
            let Ok(lock) = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, lock_base + index))) else { continue };
            // A failed motor bind drops the lock again, so the slot stays free for whoever can use it.
            let Ok(rx) = UdpSocket::bind(SocketAddr::from((bind_ip, ports.pwm()))) else { continue };
            return Ok(SitlSlot { index, ports, rx, _lock: lock });
        }
        Err(FcError::PortInUse {
            port: PORT_PWM + port_shift,
            hint: "every simulator slot is taken (16 simulators, or other programs on those ports, are running)",
        })
    }

    /// Splits the motor socket off, for its user to own outright (no duplicated handle: on Windows a duplicate is a
    /// second handle to the socket, which a child process can inherit and keep alive after the session is gone); what
    /// remains is the claim on the slot's number, to be held for the session's life.
    pub fn into_parts(self) -> (UdpSocket, SlotClaim) {
        (self.rx, SlotClaim { index: self.index, ports: self.ports, _lock: self._lock })
    }
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

    const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;

    /// A port space of its own per test (the tests run in threads of one process): locks at 47_000 + 100 n and
    /// motor ports 9_002 + 200 n + 10 slot.
    fn space(n: u16) -> (u16, u16) {
        (LOCK_PORT_BASE + 100 * n, 200 * n)
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
        let third = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap();
        assert_eq!((second.index, third.index), (1, 0), "the lowest free slot is reused");
    }

    /// The bridge owns the motor socket and holds the claim: the slot stays taken while either lives, and is free once
    /// both are gone.
    #[test]
    fn a_split_slot_stays_taken_until_its_socket_and_claim_are_both_dropped() {
        let (lock, shift) = space(7);
        let (rx, claim) = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap().into_parts();
        assert_eq!((claim.index, claim.ports.offset, rx.local_addr().unwrap().port()), (0, shift, PORT_PWM + shift));
        assert_eq!(SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap().index, 1, "slot 0 is taken by the parts");
        drop(claim);
        assert_eq!(SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap().index, 1, "the socket alone still holds the motor port");
        drop(rx);
        assert_eq!(SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap().index, 0, "both gone: slot 0 is free again");
    }

    #[test]
    fn a_foreign_holder_of_the_motor_port_skips_the_slot_and_leaves_it_free() {
        let (lock, shift) = space(4);
        let foreign = UdpSocket::bind(SocketAddr::from((LOOPBACK, PORT_PWM + shift))).unwrap();
        let slot = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap();
        assert_eq!(slot.index, 1, "slot 0's motor port is taken by someone else");
        drop(foreign);
        let next = SitlSlot::acquire_in(LOOPBACK, lock, shift).unwrap();
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
}
