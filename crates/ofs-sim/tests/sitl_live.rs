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

/// A SITL vehicle whose firmware directory is under `data_dir` (shared with other vehicles, as two sessions of one
/// simulator machine share a data directory).
fn sitl_in(data_dir: &Path) -> Vehicle {
    assert!(std::env::var("OFS_SITL_LAUNCH").is_ok(), "set OFS_SITL_LAUNCH");
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed: 1, data_dir: data_dir.to_path_buf(), fc_override: None, world: ofs_config::WorldConfig::open_field() };
    build(&cfg, &opts).unwrap()
}

fn msp_at(address: &str) -> MspClient {
    let port: u16 = address.rsplit(':').next().unwrap().parse().unwrap();
    MspClient::connect(SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_secs(5)).unwrap()
}

/// A second session started while the first runs takes the next slot: its own ports, firmware directory and
/// Configurator address, and both fly (different throttles give different motor commands) without disturbing each other.
#[test]
#[ignore]
fn a_second_session_gets_its_own_slot_and_both_fly() {
    let data = tempfile::tempdir().unwrap();
    let mut a = sitl_in(data.path());
    let mut b = sitl_in(data.path());
    assert_eq!(a.configurator_address().as_deref(), Some("tcp://127.0.0.1:5761"));
    assert_eq!(b.configurator_address().as_deref(), Some("tcp://127.0.0.1:5771"));
    let (dir_a, dir_b) = (a.firmware_dir().unwrap(), b.firmware_dir().unwrap());
    assert_ne!(dir_a, dir_b);
    assert_eq!(dir_b.file_name().unwrap().to_string_lossy(), format!("{}-s1", dir_a.file_name().unwrap().to_string_lossy()));
    for v in [&mut a, &mut b] {
        v.run_for(4.0).unwrap(); // boot and gyro calibration
        v.set_sticks(&Sticks { aux: ARM_AND_ANGLE, ..Sticks::default() }).unwrap();
        v.run_for(1.0).unwrap();
        assert!(armed(v), "never armed: {:?}", v.state().motor_cmd);
    }
    a.set_sticks(&Sticks { throttle: 0.6, aux: ARM_AND_ANGLE, ..Sticks::default() }).unwrap();
    b.set_sticks(&Sticks { throttle: 0.4, aux: ARM_AND_ANGLE, ..Sticks::default() }).unwrap();
    for _ in 0..100 {
        a.run_for(0.01).unwrap();
        b.run_for(0.01).unwrap();
    }
    let (ma, mb) = (a.state().motor_cmd[0], b.state().motor_cmd[0]);
    assert!(ma > mb + 0.05, "each session follows its own sticks: A {ma:.3}, B {mb:.3}");
    assert_eq!((a.state().fc_restarts, b.state().fc_restarts), (0, 0));
    // Each Betaflight answers on its own MSP port.
    let (addr_a, addr_b) = (a.configurator_address().unwrap(), b.configurator_address().unwrap());
    let reply_a = msp_at(&addr_a).request(MSP_API_VERSION, &[], 500, || a.run_for(0.01)).unwrap();
    let reply_b = msp_at(&addr_b).request(MSP_API_VERSION, &[], 500, || b.run_for(0.01)).unwrap();
    assert_eq!((api_version(&reply_a).map(|v| v.1), api_version(&reply_b).map(|v| v.1)), (Some(1), Some(1)));
}

/// A finished session frees its slot: the next one takes the lowest free slot, not a higher one.
#[test]
#[ignore]
fn dropping_the_first_session_lets_the_next_take_slot_zero_again() {
    let data = tempfile::tempdir().unwrap();
    let first = sitl_in(data.path());
    let second = sitl_in(data.path());
    assert_eq!(second.configurator_address().as_deref(), Some("tcp://127.0.0.1:5771"));
    drop(first);
    let third = sitl_in(data.path());
    assert_eq!(third.configurator_address().as_deref(), Some("tcp://127.0.0.1:5761"), "slot 0 was free again");
    drop(second);
    drop(third);
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
    v.set_sticks(&Sticks { roll: 0.2, pitch: -0.2, ..Sticks::default() }).unwrap();
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
    v.set_sticks(&Sticks { aux: ARM_AND_ANGLE, ..Sticks::default() }).unwrap();
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
    v.set_sticks(&Sticks::default()).unwrap(); // arm switch off
    v.run_for(2.0).unwrap();
    assert!(v.state().radio.link_up, "the link is back");
    v.set_sticks(&Sticks { aux: ARM_AND_ANGLE, ..Sticks::default() }).unwrap();
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

/// #57: a client that closes its MSP connection right after MSP_REBOOT used to leave Betaflight waiting to send the
/// reply for good: it never rebooted and the session failed. The patched SITL drops the bytes nobody is listening for.
#[test]
#[ignore]
fn a_client_that_hangs_up_right_after_msp_reboot_does_not_stop_the_reboot() {
    let mut rig = Rig::new();
    let v = &mut rig.v;
    v.run_for(2.0).unwrap();
    let mut client = msp();
    client.send(MSP_REBOOT, &[]).unwrap();
    drop(client); // hang up at once, without reading the reply
    let give_up = v.state().time_s + 5.0;
    while v.state().fc_restarts == 0 && v.state().time_s < give_up {
        v.run_for(0.01).unwrap();
    }
    assert_eq!(v.state().fc_restarts, 1, "Betaflight rebooted");
    v.run_for(2.0).unwrap();
    let reply = msp().request(MSP_API_VERSION, &[], 500, || v.run_for(0.01)).unwrap();
    assert_eq!(api_version(&reply).map(|(_, major, _)| major), Some(1));
}

/// A SITL session served over gRPC (lockstep, Betaflight), booted and armed: the client, the service and the data
/// directory that holds its firmware and logs. Linux only (the live tests need a real Betaflight in the VM).
#[cfg(target_os = "linux")]
async fn serve_an_armed_quad() -> (
    ofs_sim::pb::sim_client::SimClient<tonic::transport::Channel>,
    ofs_sim::server::SimService,
    tempfile::TempDir,
    String, // the session's Configurator address (its slot's), so a test can scope its cleanup to the slot
) {
    use ofs_sim::pb::sim_client::SimClient;
    use ofs_sim::pb::{LoadRequest, Mode, RunRequest, Sticks as PbSticks};
    use ofs_sim::server::{sim_server, SimService};
    use tokio_stream::wrappers::TcpListenerStream;

    assert!(std::env::var("OFS_SITL_LAUNCH").is_ok(), "set OFS_SITL_LAUNCH");
    let data_dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = SimService::new(data_dir.path().to_path_buf());
    tokio::spawn(tonic::transport::Server::builder().add_service(sim_server(service.clone())).serve_with_incoming(TcpListenerStream::new(listener)));
    let mut c = SimClient::connect(format!("http://{addr}")).await.unwrap();
    let quad = std::path::absolute(QUAD).unwrap();
    let reply = c.load(LoadRequest { quad_path: quad.to_string_lossy().into(), seed: 1, mode: Mode::Lockstep as i32, open_loop_fc: false, ..Default::default() }).await.unwrap().into_inner();
    // Betaflight refuses to arm with the switch already up at boot: let it boot and calibrate with the switch low
    // (about 4 s), then raise it.
    c.run(RunRequest { seconds: 4.0 }).await.unwrap();
    c.set_sticks(PbSticks { aux: ARM_AND_ANGLE.to_vec(), ..Default::default() }).await.unwrap();
    let flying = c.run(RunRequest { seconds: 2.0 }).await.unwrap().into_inner();
    assert!(flying.motor_cmd.iter().all(|m| *m > 0.0), "not armed: {:?}", flying.motor_cmd);
    (c, service, data_dir, reply.configurator_address)
}

/// Blackbox ends a log with an "End of log" event when logging stops (on disarm); a log cut short lacks it.
#[cfg(target_os = "linux")]
fn finished_blackbox_logs(logs_dir: &Path) -> (usize, usize) {
    let logs: Vec<Vec<u8>> = std::fs::read_dir(logs_dir)
        .map(|d| d.filter_map(Result::ok).filter(|e| e.file_name().to_string_lossy().ends_with(".BFL")).map(|e| std::fs::read(e.path()).unwrap()).collect())
        .unwrap_or_default();
    let finished = logs.iter().filter(|bytes| bytes.windows(10).any(|w| w == b"End of log")).count();
    (logs.len(), finished)
}

/// #58's "done when" also says the log's *last second* is present. The repo has no Blackbox frame decoder, so the
/// content is pinned at the mechanism level: the log grows with the armed flight's length (logging ran throughout)
/// and ends with Blackbox's own End of log event (it stopped on the disarm, not mid-write) — together that is the
/// flight's final second.
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore]
async fn a_blackbox_log_grows_with_the_armed_flight_it_recorded() {
    use ofs_sim::pb::RunRequest;
    let armed = |seconds: f64| async move {
        let (mut c, service, _data_dir, _address) = serve_an_armed_quad().await;
        c.run(RunRequest { seconds }).await.unwrap();
        c.unload(ofs_sim::pb::Empty {}).await.unwrap();
        let mut size = 0;
        for entry in std::fs::read_dir(service.logs_dir()).unwrap().flatten() {
            if entry.file_name().to_string_lossy().ends_with(".BFL") {
                size += entry.metadata().unwrap().len();
            }
        }
        size
    };
    let one_second = armed(1.0).await;
    let four_seconds = armed(4.0).await;
    // The log carries a fixed header, so the bar is the slope: three extra armed seconds add well over 4 KB/s of
    // frames (an earlier run measured about 9.5), proving frames were written throughout the flight, not once.
    assert!(four_seconds > one_second, "the log grows with the flight it recorded: 1 s {one_second}, 4 s {four_seconds}");
    assert!(
        (four_seconds - one_second) as f64 / 3.0 > 4000.0,
        "frames are written throughout the armed flight, not once: 1 s {one_second}, 4 s {four_seconds}"
    );
}

/// #58: a session that ends armed (the Unload comes with the quad still armed) used to leave the Blackbox log cut
/// short; the session now disarms Betaflight first.
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore]
async fn a_session_that_ends_armed_leaves_a_finished_blackbox_log() {
    use ofs_sim::pb::{Empty, RunRequest};
    let (mut c, service, _data_dir, _address) = serve_an_armed_quad().await;
    // An inputs recording is running when the session ends: the shutdown disarm must not write a row into it.
    let recording = c.start_inputs_recording(Empty {}).await.unwrap().into_inner();
    c.run(RunRequest { seconds: 0.2 }).await.unwrap();
    c.unload(Empty {}).await.unwrap();
    let (logs, finished) = finished_blackbox_logs(&service.logs_dir());
    assert_eq!(logs, 1, "the session wrote one Blackbox log into {}", service.logs_dir().display());
    assert_eq!(finished, 1, "and it ends with Blackbox's End of log event");
    let csv = std::fs::read_to_string(&recording.path).unwrap();
    let last = csv.lines().last().unwrap();
    assert_eq!(csv.lines().count(), 2, "the header and the one row of the stick state at the start: {csv}");
    assert!(last.contains(",1,1,-1,-1"), "that row still says armed (aux1 high); the disarm is not in the file: {last}");
}

/// The pilot's link is cut when the session ends (a `RadioLinkLoss` fault, or out of range): Betaflight is in failsafe
/// stage 1 and holds its last aux values, so the shutdown must restore the link before the disarm can reach it.
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore]
async fn a_session_that_ends_with_its_link_cut_still_leaves_a_finished_blackbox_log() {
    use ofs_sim::pb::{fault, Fault, RadioLinkLoss, RunRequest};
    let (mut c, service, _data_dir, _address) = serve_an_armed_quad().await;
    c.inject_fault(Fault { kind: Some(fault::Kind::RadioLinkLoss(RadioLinkLoss {})), at_s: None }).await.unwrap();
    c.run(RunRequest { seconds: 0.3 }).await.unwrap();
    c.unload(ofs_sim::pb::Empty {}).await.unwrap();
    let (logs, finished) = finished_blackbox_logs(&service.logs_dir());
    assert_eq!((logs, finished), (1, 1), "one log, finished, though the link was cut when the session ended");
}

/// The Godot client's quit order: the pilot's stream ends first (the transmitter goes off), then Unload.
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore]
async fn a_session_whose_pilot_left_first_still_leaves_a_finished_blackbox_log() {
    use ofs_sim::pb::{PilotInput, Sticks as PbSticks};
    use tokio_stream::wrappers::ReceiverStream;
    use tokio_stream::StreamExt;
    let (mut c, service, _data_dir, _address) = serve_an_armed_quad().await;
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    let armed = PbSticks { aux: ARM_AND_ANGLE.to_vec(), ..Default::default() };
    tx.send(PilotInput { sticks: Some(armed), state_rate_hz: 50 }).await.unwrap();
    let mut states = c.pilot(ReceiverStream::new(rx)).await.unwrap().into_inner();
    assert!(states.next().await.unwrap().unwrap().radio.unwrap().tx_enabled);
    drop(tx);
    drop(states); // the pilot is gone: the server switches the transmitter off
    tokio::time::sleep(Duration::from_millis(500)).await;
    c.unload(ofs_sim::pb::Empty {}).await.unwrap();
    let (logs, finished) = finished_blackbox_logs(&service.logs_dir());
    assert_eq!((logs, finished), (1, 1), "one log, finished, though the transmitter was off when the session ended");
}


#[cfg(target_os = "linux")]
/// This session's SITL processes are killed by their slot (the same selection the product's cleanup makes, from the
/// session's Configurator address): SIGTERM first, then SIGKILL for whoever survived. Never a machine-wide match by
/// process name: another simulator's SITL must not lose its session to this test.
fn kill_this_sessions_sitl(address: &str) {
    let slot = (address.rsplit(':').next().unwrap().parse::<u16>().unwrap() - MSP_TCP_PORT) / 10;
    std::process::Command::new("sh").args(["-c", ofs_fc::sitl::net::SLOT_KILL_SCRIPT, "sh", &slot.to_string()]).status().unwrap();
    for _ in 0..40 {
        let alive = std::process::Command::new("sh").arg("-c").arg(format!("pgrep -x betaflight_SITL | while read p; do tr '\\0' '\\n' < /proc/$p/cmdline | grep -q -- --port-offset {slot} && echo x; done")).status().unwrap().success();
        if !alive {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    std::process::Command::new("pkill").args(["-9", "-f", "--", &format!("--port-offset {slot}")]).status().unwrap();
}

/// A session whose SITL died is not made to wait for a disarm that cannot happen.
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore]
async fn a_session_whose_sitl_died_still_ends_promptly() {
    let (mut c, _service, _data_dir, address) = serve_an_armed_quad().await;
    kill_this_sessions_sitl(&address);
    let started = std::time::Instant::now();
    c.unload(ofs_sim::pb::Empty {}).await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(5), "unload took {:?}", started.elapsed());
}

/// A real crash, end to end: SITL is killed from outside during a lockstep run served over gRPC, so the failure goes
/// through `Session::step` and its failure hook (the test above builds the bundle by hand). Linux only; the kill is
/// this session's slot's, and the test proves a same-named outsider survives it.
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore]
async fn a_sitl_crash_through_the_session_leaves_a_replayable_bundle() {
    use ofs_sim::pb::{EventKind, RunRequest};
    use ofs_sim::repro::replay;

    let (mut c, service, _data_dir, address) = serve_an_armed_quad().await;
    let mut events = service.subscribe();

    // A decoy SITL in another slot must survive the scoped kill: a copy of /bin/sh named betaflight_SITL (comm is
    // the file's name, which is what the selection matches) whose command line reads like another slot's SITL —
    // the same trick sitl_errors' fakes use, since a sleep binary cannot carry SITL's arguments.
    let decoy_dir = tempfile::tempdir().unwrap();
    let decoy_path = decoy_dir.path().join("betaflight_SITL");
    std::fs::copy("/bin/sh", &decoy_path).unwrap();
    let mut decoy = std::process::Command::new(&decoy_path).args(["-c", "sleep 60", "--port-offset", "5"]).spawn().unwrap();
    kill_this_sessions_sitl(&address);
    let decoy_lives = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{} slot_pids 5 | grep -q .", ofs_fc::sitl::net::SLOT_PIDS_SH))
        .status()
        .unwrap()
        .success();
    assert!(decoy_lives, "the kill stayed in this session's slot: another slot's SITL survives");
    decoy.kill().unwrap();
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

/// A bundle written from a SITL session that has flown for two seconds, with the session already gone (so SITL is
/// stopped and its slot free). The directory holding the bundle is returned to keep it alive.
fn a_bundle_of_a_flown_session() -> (std::path::PathBuf, tempfile::TempDir) {
    use ofs_sim::repro::{snapshot_firmware, write_bundle, BundleSource};
    let mut rig = Rig::new();
    let data_dir = rig._dir.path().to_path_buf();
    let quad = std::path::absolute(QUAD).unwrap();
    let firmware = ofs_sim::vehicle::firmware_dir(&data_dir, &quad);
    let cfg = load(&quad).unwrap();
    let mut source = BundleSource::new(&cfg, None, None, 1, false, false, data_dir.join("logs"), Some(firmware.clone())).unwrap();
    source.firmware_initial = snapshot_firmware(&firmware);
    assert!(source.firmware_initial.iter().any(|(n, _)| n == "eeprom.bin"), "the first boot saved an EEPROM");
    rig.v.set_sticks(&Sticks { aux: ARM_AND_ANGLE, ..Sticks::default() }).unwrap();
    rig.v.run_for(2.0).unwrap();
    let error = ofs_core::SimError::Firmware("simulated failure for the bundle test".into());
    let bundle = write_bundle(&source, &rig.v, &error).unwrap();
    for wanted in ["firmware/eeprom.bin", "firmware/betaflight.diff", "firmware/sitl.log", "tail.mcap", "journal.txt", "bundle.toml"] {
        assert!(bundle.join(wanted).is_file(), "{wanted} missing from the bundle");
    }
    let Rig { v, _dir } = rig;
    drop(v); // SITL must stop before the replay starts its own
    (bundle, _dir)
}

#[test]
#[ignore]
fn a_sitl_sessions_bundle_carries_its_firmware_and_replays() {
    use ofs_sim::repro::replay;
    let (bundle, _keep) = a_bundle_of_a_flown_session();
    let scratch = tempfile::tempdir().unwrap();
    // A crash that never happened does not recur: the replay runs the journal to the failing tick and says it did
    // not reproduce, but it must run (the bundle's EEPROM boots SITL).
    let outcome = replay(&bundle, scratch.path()).unwrap();
    assert!(!outcome.reproduced, "{outcome:?}");
    assert!(outcome.failure.is_none(), "{outcome:?}");
    assert_eq!(outcome.reached_tick, outcome.expected_tick + 1, "{outcome:?}");
}

/// A replay in the data directory of a running simulator takes the next free slot, so its SITL runs from that slot's
/// firmware directory: the bundle's EEPROM must be seeded there, not into the slot-0 directory the replay does not use
/// (which would also be a directory another replay of the same bundle could be flying from).
#[test]
#[ignore]
fn a_replay_beside_a_running_session_seeds_the_slot_it_takes() {
    use ofs_sim::repro::replay;
    let (bundle, _keep) = a_bundle_of_a_flown_session();
    let data = tempfile::tempdir().unwrap();
    let running = sitl_in(data.path()); // takes slot 0
    let outcome = replay(&bundle, data.path()).unwrap();
    assert!(!outcome.reproduced && outcome.failure.is_none(), "{outcome:?}");
    assert_eq!(outcome.reached_tick, outcome.expected_tick + 1, "{outcome:?}");
    // The replay's quad is the bundle's own copy, so its firmware directory is named after that path.
    let text = std::fs::read_to_string(bundle.join("bundle.toml")).unwrap();
    let quad = text.lines().find_map(|l| l.strip_prefix("quad = ")).expect("bundle.toml names its quad").trim_matches('"');
    let replay_dir = ofs_sim::vehicle::firmware_dir(data.path(), &bundle.join(quad));
    let slot_1 = ofs_sim::vehicle::slot_firmware_dir(&replay_dir, 1);
    assert!(slot_1.join("eeprom.bin").is_file(), "the replay flew from slot 1's directory: {}", slot_1.display());
    assert!(!replay_dir.exists(), "nothing was seeded into the slot-0 directory the replay did not use: {}", replay_dir.display());
    drop(running);
}

/// Something that is not a simulator — any process holding a slot's loopback lock — pushes the session to the next
/// slot all the same: the product path skips the taken slot, and the session reports the next slot's address. The
/// lock is held on loopback in every launch mode (native Linux, and Windows against SITL in WSL), so this works in
/// both, which a held motor port would not (that binds the gateway IP under WSL's NAT).
#[test]
#[ignore]
fn a_foreign_holder_of_the_slot_lock_pushes_the_session_to_the_next_slot() {
    let _holder = std::net::UdpSocket::bind(("127.0.0.1", ofs_fc::sitl::slot::LOCK_PORT_BASE)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let v = sitl_in(dir.path());
    assert_eq!(v.configurator_address().as_deref(), Some("tcp://127.0.0.1:5771"), "slot 0's lock was held, so the session took slot 1");
}

/// A bundle recorded by a session that flew in slot 1 (slot 0's lock held by a foreign socket) replays like any
/// other: nothing in the bundle path assumes the stock ports.
#[test]
#[ignore]
fn a_bundle_from_a_session_in_slot_n_replays() {
    use ofs_sim::repro::{replay, snapshot_firmware, write_bundle, BundleSource};

    let _holder = std::net::UdpSocket::bind(("127.0.0.1", ofs_fc::sitl::slot::LOCK_PORT_BASE)).unwrap();
    let mut rig = Rig::new();
    assert_eq!(rig.v.configurator_address().as_deref(), Some("tcp://127.0.0.1:5771"), "flying in slot 1");
    let data_dir = rig._dir.path().to_path_buf();
    let quad = std::path::absolute(QUAD).unwrap();
    let cfg = load(&quad).unwrap();
    let firmware = rig.v.firmware_dir().unwrap();
    assert!(firmware.file_name().unwrap().to_string_lossy().ends_with("-s1"), "{firmware:?}");
    let mut source = BundleSource::new(&cfg, None, None, 1, false, false, data_dir.join("logs"), Some(firmware.clone())).unwrap();
    source.firmware_initial = snapshot_firmware(&firmware);
    rig.v.set_sticks(&Sticks { aux: ARM_AND_ANGLE, ..Sticks::default() }).unwrap();
    rig.v.run_for(2.0).unwrap();
    let error = ofs_core::SimError::Firmware("simulated failure for the slot-n bundle test".into());
    let bundle = write_bundle(&source, &rig.v, &error).unwrap();
    let Rig { v, _dir } = rig;
    drop(v); // SITL must stop before the replay starts its own; _dir keeps the bundle alive
    drop(_holder); // the replay takes the lowest free slot, as a plain replay does
    let scratch = tempfile::tempdir().unwrap();
    let outcome = replay(&bundle, scratch.path()).unwrap();
    assert!(!outcome.reproduced, "{outcome:?}");
    assert!(outcome.failure.is_none(), "{outcome:?}");
    assert_eq!(outcome.reached_tick, outcome.expected_tick + 1, "{outcome:?}");
}
