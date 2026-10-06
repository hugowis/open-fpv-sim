use std::f64::consts::PI;

use glam::{DQuat, DVec3};
use ofs_fc::sitl::codec::{
    state_datagram, state_datagram_with_serial, FdmPacket, RcPacket, ServoPacket, SERIAL_SECTION_MAX, STATE_DATAGRAM_SIZE,
};
use ofs_fc::sitl::frames::{attitude_flu_nwu, fdm_packet, motor_commands, rc_channels, Home, SensorFrame, EARTH_RADIUS_M};

fn f64_at(bytes: &[u8], index: usize) -> f64 {
    f64::from_le_bytes(bytes[index * 8..index * 8 + 8].try_into().unwrap())
}

#[test]
fn fdm_packet_is_18_little_endian_doubles_in_struct_order() {
    let p = FdmPacket {
        timestamp_s: 1.0,
        gyro_rpy_radps: [2.0, 3.0, 4.0],
        accel_xyz_mps2: [5.0, 6.0, 7.0],
        quat_wxyz: [8.0, 9.0, 10.0, 11.0],
        velocity_xyz_mps: [12.0, 13.0, 14.0],
        position_xyz: [15.0, 16.0, 17.0],
        pressure_pa: 18.0,
    };
    let bytes = p.encode();
    assert_eq!(bytes.len(), FdmPacket::SIZE);
    for i in 0..18 {
        assert_eq!(f64_at(&bytes, i), (i + 1) as f64);
    }
}

#[test]
fn rc_packet_is_timestamp_then_16_u16_channels() {
    let mut channels = [1500u16; 16];
    channels[2] = 1000;
    let bytes = RcPacket { timestamp_s: 0.5, channels }.encode();
    assert_eq!(bytes.len(), RcPacket::SIZE);
    assert_eq!(f64_at(&bytes, 0), 0.5);
    assert_eq!(u16::from_le_bytes([bytes[12], bytes[13]]), 1000);
    assert_eq!(u16::from_le_bytes([bytes[38], bytes[39]]), 1500);
}

#[test]
fn state_datagram_is_fdm_then_rc() {
    let fdm = FdmPacket {
        timestamp_s: 1.0,
        gyro_rpy_radps: [0.0; 3],
        accel_xyz_mps2: [0.0; 3],
        quat_wxyz: [1.0, 0.0, 0.0, 0.0],
        velocity_xyz_mps: [0.0; 3],
        position_xyz: [0.0; 3],
        pressure_pa: 101_325.0,
    };
    let rc = RcPacket { timestamp_s: 1.0, channels: [1500; 16] };
    let d = state_datagram(&fdm, &rc);
    assert_eq!(d.len(), STATE_DATAGRAM_SIZE);
    assert_eq!(&d[..FdmPacket::SIZE], &fdm.encode()[..]);
    assert_eq!(&d[FdmPacket::SIZE..], &rc.encode()[..]);
}

#[test]
fn servo_packet_decodes_four_floats_and_rejects_short_input() {
    let mut bytes = Vec::new();
    for v in [0.0f32, 0.25, 0.5, 1.5] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    let p = ServoPacket::decode(&bytes).unwrap();
    assert_eq!(p.motor_speed, [0.0, 0.25, 0.5, 1.5]);
    assert_eq!(motor_commands(&p), [0.0, 0.25, 0.5, 1.0]);
    assert!(ServoPacket::decode(&bytes[..15]).is_none());
}

fn assert_quat(q: DQuat, wxyz: [f64; 4]) {
    let got = [q.w, q.x, q.y, q.z];
    for (g, e) in got.iter().zip(wxyz) {
        assert!((g - e).abs() < 1e-12, "got {got:?}, expected {wxyz:?}");
    }
}

#[test]
fn attitude_converts_frd_ned_to_flu_nwu() {
    let h = |deg: f64| (deg.to_radians() / 2.0).sin();
    let c = |deg: f64| (deg.to_radians() / 2.0).cos();
    assert_quat(attitude_flu_nwu(DQuat::IDENTITY), [1.0, 0.0, 0.0, 0.0]);
    // roll right 10 deg: same rotation about the shared X axis
    assert_quat(attitude_flu_nwu(DQuat::from_rotation_x(10f64.to_radians())), [c(10.0), h(10.0), 0.0, 0.0]);
    // nose up 10 deg: +Y in FRD, -Y in FLU
    assert_quat(attitude_flu_nwu(DQuat::from_rotation_y(10f64.to_radians())), [c(10.0), 0.0, -h(10.0), 0.0]);
    // facing east (yaw right 90 deg): +Z in NED, -Z in NWU
    assert_quat(attitude_flu_nwu(DQuat::from_rotation_z(PI / 2.0)), [c(90.0), 0.0, 0.0, -h(90.0)]);
}

#[test]
fn sensor_frame_maps_to_legacy_bridge_fields() {
    let home = Home { lat_deg: 50.0, lon_deg: 4.0, alt_m: 30.0 };
    let frame = SensorFrame {
        time_s: 2.5,
        gyro_frd_radps: DVec3::new(1.0, 2.0, 3.0),
        accel_frd_mps2: DVec3::new(0.1, 0.2, -9.8),
        att_ned: DQuat::IDENTITY,
        vel_ned_mps: DVec3::new(1.0, 2.0, -3.0),
        pos_ned_m: DVec3::new(100.0, 50.0, -10.0),
        pressure_pa: 101_000.0,
    };
    let p = fdm_packet(&frame, &home);
    assert_eq!(p.timestamp_s, 2.5);
    assert_eq!(p.gyro_rpy_radps, [1.0, 2.0, 3.0]);
    assert_eq!(p.accel_xyz_mps2, [-0.1, 0.2, -9.8]);
    assert_eq!(p.velocity_xyz_mps, [2.0, 1.0, 3.0]);
    let lat = 50.0 + (100.0 / EARTH_RADIUS_M).to_degrees();
    let lon = 4.0 + (50.0 / (EARTH_RADIUS_M * 50f64.to_radians().cos())).to_degrees();
    assert!((p.position_xyz[0] - lon).abs() < 1e-12);
    assert!((p.position_xyz[1] - lat).abs() < 1e-12);
    assert_eq!(p.position_xyz[2], 40.0);
    assert_eq!(p.pressure_pa, 101_000.0);
}

#[test]
fn sticks_map_to_aetr_microseconds() {
    let ch = rc_channels(1.0, -1.0, 0.5, 0.25, &[1.0, -1.0, 0.0, 0.0]);
    assert_eq!(&ch[..8], &[2000, 1000, 1250, 1750, 2000, 1000, 1500, 1500]);
    assert!(ch[8..].iter().all(|c| *c == 1500));
    assert_eq!(rc_channels(5.0, 0.0, 0.0, -3.0, &[])[..4], [2000, 1500, 1000, 1500]);
}

fn zero_fdm() -> FdmPacket {
    FdmPacket {
        timestamp_s: 0.25,
        gyro_rpy_radps: [0.0; 3],
        accel_xyz_mps2: [0.0; 3],
        quat_wxyz: [1.0, 0.0, 0.0, 0.0],
        velocity_xyz_mps: [0.0; 3],
        position_xyz: [0.0; 3],
        pressure_pa: 101_325.0,
    }
}

#[test]
fn serial_blocks_follow_the_state_datagram() {
    let fdm = zero_fdm();
    let rc = RcPacket { timestamp_s: 0.25, channels: [0; 16] };
    assert_eq!(state_datagram_with_serial(&fdm, &rc, &[]), state_datagram(&fdm, &rc).to_vec());
    let d = state_datagram_with_serial(&fdm, &rc, &[(1, vec![0xC8, 0x18, 0x16]), (4, vec![0xAA])]);
    assert_eq!(&d[..STATE_DATAGRAM_SIZE], &state_datagram(&fdm, &rc)[..]);
    assert_eq!(&d[STATE_DATAGRAM_SIZE..], &[1, 3, 0, 0xC8, 0x18, 0x16, 4, 1, 0, 0xAA]);
}

#[test]
#[should_panic(expected = "EXT_SERIAL_MAX")]
fn oversized_serial_sections_are_refused() {
    let rc = RcPacket { timestamp_s: 0.0, channels: [0; 16] };
    state_datagram_with_serial(&zero_fdm(), &rc, &[(1, vec![0; SERIAL_SECTION_MAX])]);
}
