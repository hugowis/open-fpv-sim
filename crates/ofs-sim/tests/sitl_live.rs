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
    v.set_fault(Fault::RadioLinkLoss, true).unwrap();
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

    // The link comes back: Betaflight recovers once the arm switch is off, and arms again.
    v.clear_faults();
    v.set_sticks(&Sticks::default()); // arm switch off
    v.run_for(2.0).unwrap();
    assert!(v.state().radio.link_up, "the link is back");
    v.set_sticks(&Sticks { aux: ARM_AND_ANGLE, ..Sticks::default() });
    v.run_for(1.0).unwrap();
    assert!(armed(v), "no re-arm after the failsafe: {:?}", v.state().motor_cmd);
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

/// A real crash, end to end: SITL is killed from outside during a lockstep run served over gRPC, so the failure goes
/// through `Session::step` and its failure hook (the test above builds the bundle by hand). Linux only: it kills SITL
/// with `pkill -x`, which takes down every Betaflight SITL on the machine, so run it alone (CI does, one test at a time).
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore]
async fn a_sitl_crash_through_the_session_leaves_a_replayable_bundle() {
    use ofs_sim::pb::sim_client::SimClient;
    use ofs_sim::pb::{EventKind, LoadRequest, Mode, RunRequest, Sticks as PbSticks};
    use ofs_sim::repro::replay;
    use ofs_sim::server::{sim_server, SimService};
    use tokio_stream::wrappers::TcpListenerStream;

    assert!(std::env::var("OFS_SITL_LAUNCH").is_ok(), "set OFS_SITL_LAUNCH");
    let data_dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = SimService::new(data_dir.path().to_path_buf());
    tokio::spawn(tonic::transport::Server::builder().add_service(sim_server(service.clone())).serve_with_incoming(TcpListenerStream::new(listener)));
    let mut c = SimClient::connect(format!("http://{addr}")).await.unwrap();
    let mut events = service.subscribe();

    let quad = std::path::absolute(QUAD).unwrap();
    c.load(LoadRequest { quad_path: quad.to_string_lossy().into(), seed: 1, mode: Mode::Lockstep as i32, open_loop_fc: false, ..Default::default() }).await.unwrap();
    // Betaflight refuses to arm with the switch already up at boot: let it boot and calibrate with the switch low
    // (about 4 s), then raise it. The crash must hit an armed quad.
    c.run(RunRequest { seconds: 4.0 }).await.unwrap();
    c.set_sticks(PbSticks { aux: ARM_AND_ANGLE.to_vec(), ..Default::default() }).await.unwrap();
    let flying = c.run(RunRequest { seconds: 2.0 }).await.unwrap().into_inner();
    assert!(flying.motor_cmd.iter().all(|m| *m > 0.0), "not armed before the kill: {:?}", flying.motor_cmd);

    let killed = std::process::Command::new("pkill").args(["-9", "-x", "betaflight_SITL"]).status().unwrap();
    assert!(killed.success(), "no SITL to kill");
    let err = c.run(RunRequest { seconds: 2.0 }).await.expect_err("a run with a dead SITL must fail");

    let mut message = None;
    while let Ok(e) = events.try_recv() {
        if e.kind() == EventKind::SimError {
            message = Some(e.message);
        }
    }
    let message = message.unwrap_or_else(|| panic!("no sim_error event after {err:?}"));
    let folder = message.split("(repro bundle: ").nth(1).and_then(|rest| rest.strip_suffix(')')).unwrap_or_else(|| panic!("no bundle in: {message}"));
    let folder = std::path::PathBuf::from(folder);
    for wanted in ["bundle.toml", "journal.txt", "tail.mcap", "firmware/eeprom.bin", "firmware/sitl.log"] {
        assert!(folder.join(wanted).is_file(), "{wanted} missing from {}", folder.display());
    }

    c.unload(ofs_sim::pb::Empty {}).await.unwrap(); // the session's SITL must stop before the replay starts its own
    let scratch = tempfile::tempdir().unwrap();
    // The kill came from outside and is not in the journal, so it does not recur: the replay runs the journal to the
    // failing tick and says so.
    let outcome = replay(&folder, scratch.path()).unwrap();
    assert!(!outcome.reproduced && outcome.failure.is_none(), "{outcome:?}");
}

#[test]
#[ignore]
fn a_sitl_sessions_bundle_carries_its_firmware_and_replays() {
    use ofs_sim::repro::{replay, snapshot_firmware, write_bundle, BundleSource};
    let mut rig = Rig::new();
    let data_dir = rig._dir.path().to_path_buf();
    let quad = std::path::absolute(QUAD).unwrap();
    let firmware = ofs_sim::vehicle::firmware_dir(&data_dir, &quad);
    let cfg = load(&quad).unwrap();
    let mut source = BundleSource::new(&cfg, None, None, 1, false, false, data_dir.join("logs"), Some(firmware.clone())).unwrap();
    source.firmware_initial = snapshot_firmware(&firmware);
    assert!(source.firmware_initial.iter().any(|(n, _)| n == "eeprom.bin"), "the first boot saved an EEPROM");
    rig.v.set_sticks(&Sticks { aux: ARM_AND_ANGLE, ..Sticks::default() });
    rig.v.run_for(2.0).unwrap();
    let error = ofs_core::SimError::Firmware("simulated failure for the bundle test".into());
    let bundle = write_bundle(&source, &rig.v, &error).unwrap();
    for wanted in ["firmware/eeprom.bin", "firmware/betaflight.diff", "firmware/sitl.log", "tail.mcap", "journal.txt", "bundle.toml"] {
        assert!(bundle.join(wanted).is_file(), "{wanted} missing from the bundle");
    }
    drop(rig.v); // SITL must stop before the replay starts its own
    let scratch = tempfile::tempdir().unwrap();
    // A crash that never happened does not recur: the replay runs the journal to the failing tick and says it did
    // not reproduce, but it must run (the bundle's EEPROM boots SITL).
    let outcome = replay(&bundle, scratch.path()).unwrap();
    assert!(!outcome.reproduced, "{outcome:?}");
    assert!(outcome.failure.is_none(), "{outcome:?}");
    assert_eq!(outcome.reached_tick, outcome.expected_tick + 1, "{outcome:?}");
}
