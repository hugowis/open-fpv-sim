//! M2 exit checks against real Betaflight SITL (spec §8.3). Needs a built SITL; run one at a time:
//!   OFS_SITL_LAUNCH="<path or wsl.exe -e path>" cargo test -p ofs-sim --test sitl_live -- --ignored --test-threads=1
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use ofs_config::load;
use ofs_fc::msp::{api_version, rc_channels_us, MspClient, MSP_API_VERSION, MSP_RC, MSP_REBOOT};
use ofs_fc::sitl::codec::MSP_TCP_PORT;
use ofs_sim::vehicle::{build, BuildOptions, Fault, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");
/// AUX1 high = ARM, AUX2 high = ANGLE (the quad's betaflight.diff).
const ARM_AND_ANGLE: [f64; 4] = [1.0, 1.0, -1.0, -1.0];

/// A quad flying Betaflight SITL from a fresh firmware directory. Fields drop in order: SITL stops before
/// its directory is removed.
struct Rig {
    v: Vehicle,
    _dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Self {
        assert!(std::env::var("OFS_SITL_LAUNCH").is_ok(), "set OFS_SITL_LAUNCH");
        let dir = tempfile::tempdir().unwrap();
        let cfg = load(Path::new(QUAD)).unwrap();
        let opts = BuildOptions { seed: 1, data_dir: dir.path().to_path_buf(), fc_override: None, world: ofs_config::WorldConfig::open_field() };
        let v = build(&cfg, &opts).unwrap();
        Self { v, _dir: dir }
    }
}

fn msp() -> MspClient {
    MspClient::connect(SocketAddr::from(([127, 0, 0, 1], MSP_TCP_PORT)), Duration::from_secs(5)).unwrap()
}

#[test]
#[ignore]
fn msp_reports_the_api_version_over_tcp() {
    let mut rig = Rig::new();
    let v = &mut rig.v;
    v.run_for(1.0).unwrap();
    let reply = msp().request(MSP_API_VERSION, &[], 500, || v.run_for(0.01)).unwrap();
    let (protocol, major, minor) = api_version(&reply).unwrap();
    assert_eq!((protocol, major), (0, 1), "MSP API {major}.{minor}");
}

#[test]
#[ignore]
fn crsf_sticks_reach_betaflight() {
    let mut rig = Rig::new();
    let v = &mut rig.v;
    v.set_sticks(&Sticks { roll: 0.2, pitch: -0.2, ..Sticks::default() });
    v.run_for(2.0).unwrap();
    let reply = msp().request(MSP_RC, &[], 500, || v.run_for(0.01)).unwrap();
    // MSP_RC lists roll, pitch, yaw, throttle (Betaflight's internal order).
    assert_eq!(&rc_channels_us(&reply).unwrap()[..4], &[1600, 1400, 1500, 1000]);
}

fn armed(v: &Vehicle) -> bool {
    v.state().motor_cmd.iter().all(|m| *m > 0.0)
}

#[test]
#[ignore]
fn a_radio_cut_fails_safe_on_betaflight_timing() {
    let mut rig = Rig::new();
    let v = &mut rig.v;
    v.run_for(4.0).unwrap(); // boot and gyro calibration
    v.set_sticks(&Sticks { aux: ARM_AND_ANGLE, ..Sticks::default() });
    v.run_for(1.0).unwrap();
    assert!(armed(v), "never armed: {:?}", v.state().motor_cmd);
    v.set_fault(Fault::RadioLinkLoss, true);
    let cut = v.state().time_s;
    let mut disarmed_after = None;
    while v.state().time_s < cut + 4.0 {
        v.run_for(0.01).unwrap();
        if v.state().motor_cmd.iter().all(|m| *m == 0.0) {
            disarmed_after = Some(v.state().time_s - cut);
            break;
        }
    }
    let dt = disarmed_after.expect("Betaflight never disarmed after the radio cut");
    // Betaflight declares RX loss once frames stop for failsafe_delay (1.5 s by default) and then disarms
    // (src/main/flight/failsafe.c); stage-1 failsafe holds idle until then.
    assert!((1.4..=2.2).contains(&dt), "disarmed {dt:.3} s after the cut");
}

#[test]
#[ignore]
fn a_betaflight_reboot_is_a_firmware_restart_not_a_crash() {
    let mut rig = Rig::new();
    let v = &mut rig.v;
    v.run_for(2.0).unwrap();
    // Betaflight resets after MSP_REBOOT: SITL prints "[system]Reset!" and exits with status 0. (Its reply can
    // be lost with the connection, so the test only sends the command.)
    let mut client = msp();
    client.send(MSP_REBOOT, &[]).unwrap();
    let give_up = v.state().time_s + 5.0;
    while v.state().fc_restarts == 0 && v.state().time_s < give_up {
        v.run_for(0.01).unwrap();
    }
    assert_eq!(v.state().fc_restarts, 1);
    v.run_for(2.0).unwrap();
    let reply = msp().request(MSP_API_VERSION, &[], 500, || v.run_for(0.01)).unwrap();
    assert_eq!(api_version(&reply).map(|(_, major, _)| major), Some(1));
}
