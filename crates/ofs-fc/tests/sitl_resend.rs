//! First-exchange resend against a fake SITL on ephemeral loopback ports (no Betaflight needed).
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ofs_fc::sitl::bridge::exchange_with_resend;

const RESEND: Duration = Duration::from_millis(50);
const TIMEOUT: Duration = Duration::from_secs(2);

struct Link {
    rx: UdpSocket,
    tx: UdpSocket,
    sitl: UdpSocket,
}

impl Link {
    fn new() -> Self {
        let bind = || UdpSocket::bind("127.0.0.1:0").unwrap();
        Self { rx: bind(), tx: bind(), sitl: bind() }
    }

    fn sitl_addr(&self) -> SocketAddr {
        self.sitl.local_addr().unwrap()
    }

    /// Fake lockstep SITL: drops the first `lose` datagrams, then answers each later one with its first byte,
    /// until `answers` replies were sent. Returns how many datagrams it saw.
    fn serve(&self, lose: usize, answers: usize) -> JoinHandle<usize> {
        let sitl = self.sitl.try_clone().unwrap();
        let reply_to = self.rx.local_addr().unwrap();
        sitl.set_read_timeout(Some(TIMEOUT * 2)).unwrap();
        std::thread::spawn(move || {
            let (mut seen, mut sent) = (0, 0);
            let mut buf = [0u8; 256];
            while sent < answers {
                let Ok((n, _)) = sitl.recv_from(&mut buf) else { break };
                assert!(n > 0);
                seen += 1;
                if seen > lose {
                    sitl.send_to(&buf[..1], reply_to).unwrap();
                    sent += 1;
                }
            }
            seen
        })
    }
}

#[test]
fn answered_first_datagram_is_not_resent() {
    let link = Link::new();
    let sitl = link.serve(0, 1);
    let mut buf = [0u8; 16];
    let (n, resends) = exchange_with_resend(&link.rx, &link.tx, link.sitl_addr(), &[7; 184], &mut buf, TIMEOUT, RESEND).unwrap();
    assert_eq!((n, buf[0], resends), (1, 7, 0));
    assert_eq!(sitl.join().unwrap(), 1);
}

#[test]
fn lost_first_datagram_is_resent_until_answered() {
    let link = Link::new();
    let sitl = link.serve(2, 1);
    let mut buf = [0u8; 16];
    let started = Instant::now();
    let (n, resends) = exchange_with_resend(&link.rx, &link.tx, link.sitl_addr(), &[9; 184], &mut buf, TIMEOUT, RESEND).unwrap();
    assert_eq!((n, buf[0]), (1, 9));
    assert_eq!(resends, 2, "two datagrams were dropped");
    assert!(started.elapsed() < TIMEOUT / 2, "took {:?}", started.elapsed());
    assert_eq!(sitl.join().unwrap(), 3);
}

#[test]
fn silent_sitl_times_out_after_the_first_reply_timeout() {
    let link = Link::new();
    let mut buf = [0u8; 16];
    let timeout = Duration::from_millis(300);
    let started = Instant::now();
    let err = exchange_with_resend(&link.rx, &link.tx, link.sitl_addr(), &[1; 184], &mut buf, timeout, RESEND).unwrap_err();
    let waited = started.elapsed();
    assert_eq!(err.kind(), ErrorKind::TimedOut, "{err}");
    assert!(waited >= timeout && waited < timeout * 3, "waited {waited:?}");
    // Every resend reached the (silent) peer.
    link.sitl.set_nonblocking(true).unwrap();
    let mut got = 0;
    while link.sitl.recv_from(&mut [0u8; 256]).is_ok() {
        got += 1;
    }
    assert!(got >= 4, "only {got} datagrams sent in {waited:?}");
}
