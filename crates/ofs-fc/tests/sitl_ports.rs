//! The SITL ports as data: slot 0 is today's ports, every other slot is a block of ten further up.
use std::collections::HashSet;

use ofs_fc::sitl::codec::{PortMap, MSP_TCP_PORT, PORT_PWM, PORT_PWM_RAW, PORT_RC, PORT_STATE};

#[test]
fn the_default_port_map_is_todays_ports() {
    let p = PortMap::default();
    assert_eq!((p.pwm_raw(), p.pwm(), p.state(), p.rc(), p.msp()), (9001, 9002, 9003, 9004, 5761));
    assert_eq!((p.pwm_raw(), p.pwm(), p.state(), p.rc(), p.msp()), (PORT_PWM_RAW, PORT_PWM, PORT_STATE, PORT_RC, MSP_TCP_PORT));
    assert_eq!(p.configurator_address(), "tcp://127.0.0.1:5761");
}

#[test]
fn a_slot_shifts_every_port_by_ten_per_slot() {
    let p = PortMap::slot(2);
    assert_eq!(p.offset, 20);
    assert_eq!((p.pwm_raw(), p.pwm(), p.state(), p.rc(), p.msp()), (9021, 9022, 9023, 9024, 5781));
    assert_eq!(p.configurator_address(), "tcp://127.0.0.1:5781");
}

#[test]
fn a_slots_uart_tcp_block_is_the_msp_port_and_the_seven_above_it() {
    let p = PortMap::slot(3);
    assert_eq!(p.uart_ports().collect::<Vec<_>>(), (5791..=5798).collect::<Vec<_>>());
    // The block stays inside the slot's ten ports (the next slot's MSP port is not probed).
    assert!(!p.uart_ports().contains(&PortMap::slot(4).msp()));
}

#[test]
fn slots_never_share_a_port() {
    let mut udp = HashSet::new();
    let mut tcp = HashSet::new();
    for slot in 0..16 {
        let p = PortMap::slot(slot);
        for port in [p.pwm_raw(), p.pwm(), p.state(), p.rc()] {
            assert!(udp.insert(port), "UDP {port} used twice (slot {slot})");
        }
        // UART1 to UART8 are served on TCP 5761 and the seven above it.
        for uart in 0..8 {
            assert!(tcp.insert(p.msp() + uart), "TCP {} used twice (slot {slot})", p.msp() + uart);
        }
    }
    assert_eq!((udp.len(), tcp.len()), (64, 128));
}
