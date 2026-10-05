use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use ofs_fc::msp::*;

fn reply(cmd: u8, payload: &[u8], error: bool) -> Vec<u8> {
    let mut f = vec![b'$', b'M', if error { b'!' } else { b'>' }, payload.len() as u8, cmd];
    f.extend_from_slice(payload);
    f.push(payload.iter().fold(payload.len() as u8 ^ cmd, |c, b| c ^ b));
    f
}

#[test]
fn requests_are_framed_with_an_xor_checksum() {
    assert_eq!(encode_request(MSP_API_VERSION, &[]), b"$M<\x00\x01\x01".to_vec());
    assert_eq!(encode_request(200, &[1, 2]), vec![b'$', b'M', b'<', 2, 200, 1, 2, 2 ^ 200 ^ 1 ^ 2]);
}

#[test]
fn parser_skips_noise_and_bad_checksums() {
    let mut bytes = b"noise$M".to_vec();
    let mut bad = reply(MSP_RC, &[1, 2], false);
    *bad.last_mut().unwrap() ^= 0x55;
    bytes.extend(bad);
    bytes.extend(reply(MSP_API_VERSION, &[0, 1, 48], false));
    bytes.extend(reply(MSP_REBOOT, &[], true));
    let mut p = MspParser::default();
    let (a, b) = bytes.split_at(9);
    let mut got = p.push(a);
    got.extend(p.push(b));
    assert_eq!(
        got,
        vec![
            MspReply { cmd: MSP_API_VERSION, payload: vec![0, 1, 48], error: false },
            MspReply { cmd: MSP_REBOOT, payload: vec![], error: true },
        ]
    );
}

#[test]
fn request_pumps_until_the_reply_arrives() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut req = [0u8; 6];
        s.read_exact(&mut req).unwrap();
        assert_eq!(&req, b"$M<\x00\x01\x01");
        s.write_all(&reply(MSP_API_VERSION, &[0, 1, 48], false)).unwrap();
        let mut sink = [0u8; 1];
        let _ = s.read(&mut sink); // hold the connection until the client hangs up
    });
    let mut c = MspClient::connect(addr, Duration::from_secs(2)).unwrap();
    let mut pumps = 0;
    let r = c
        .request(MSP_API_VERSION, &[], 1000, || {
            pumps += 1;
            std::thread::sleep(Duration::from_millis(1));
            Ok(())
        })
        .unwrap();
    assert_eq!(api_version(&r), Some((0, 1, 48)));
    assert!(pumps >= 1);
    drop(c);
    server.join().unwrap();
}

#[test]
fn request_times_out_after_max_pumps() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut sink = [0u8; 64];
        while let Ok(n) = s.read(&mut sink) {
            if n == 0 {
                break;
            }
        }
    });
    let mut c = MspClient::connect(addr, Duration::from_secs(2)).unwrap();
    let err = c.request(MSP_RC, &[], 5, || Ok(())).unwrap_err();
    assert!(matches!(err, MspError::Timeout { cmd: MSP_RC, pumps: 5 }), "{err}");
    drop(c);
    server.join().unwrap();
}

#[test]
fn status_and_rc_decoders() {
    let mut status = vec![0u8; 22];
    status[6] = 1;
    assert_eq!(armed(&MspReply { cmd: MSP_STATUS, payload: status, error: false }), Some(true));
    assert_eq!(armed(&MspReply { cmd: MSP_STATUS, payload: vec![0; 4], error: false }), None);
    let rc = MspReply { cmd: MSP_RC, payload: vec![0x40, 0x06, 0x78, 0x05], error: false };
    assert_eq!(rc_channels_us(&rc), vec![1600, 1400]);
}
