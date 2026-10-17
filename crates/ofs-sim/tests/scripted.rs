//! Scheduled faults (`at=`) and scripted inputs (spec §5, §6.2): applied on their tick, whatever the Run chunking.
use std::path::Path;

use ofs_config::{load, FcKind, WorldConfig};
use ofs_core::SimError;
use ofs_sim::inputs;
use ofs_sim::vehicle::{build, BuildOptions, Fault, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn vehicle() -> Vehicle {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    build(&cfg, &opts).unwrap()
}

fn dead_motor_2() -> Fault {
    Fault::MotorFailure { motor: 2, scale: 0.0 }
}

/// Full throttle, motor 2 scheduled to die at 0.5 s, run for 1 s in the given chunks.
fn chunked(chunks: &[f64]) -> Vehicle {
    let mut v = vehicle();
    v.set_sticks(&Sticks { throttle: 1.0, ..Sticks::default() }).unwrap();
    v.schedule_fault(dead_motor_2(), 0.5).unwrap();
    for c in chunks {
        v.run_for(*c).unwrap();
    }
    v
}

#[test]
fn a_scheduled_fault_lands_on_its_tick_however_the_run_is_cut() {
    let whole = chunked(&[1.0]);
    let cut = chunked(&[0.3, 0.2, 0.05, 0.45]);
    assert_eq!(whole.digest(), cut.digest());
    let mut never = vehicle();
    never.set_sticks(&Sticks { throttle: 1.0, ..Sticks::default() }).unwrap();
    never.run_for(1.0).unwrap();
    assert_ne!(whole.digest(), never.digest(), "the fault changed the flight");

    let mut v = vehicle();
    v.set_sticks(&Sticks { throttle: 1.0, ..Sticks::default() }).unwrap();
    v.schedule_fault(dead_motor_2(), 0.5).unwrap();
    v.run_for(0.499_875).unwrap(); // tick 3999
    assert!(v.active_fault_names().is_empty(), "not yet");
    v.step_ticks(1).unwrap(); // 0.5 s: the state at 0.5 s shows the fault
    assert_eq!(v.active_fault_names(), ["motor_failure"]);
}

#[test]
fn a_scheduled_fault_is_validated_now_and_a_passed_time_is_refused() {
    let mut v = vehicle();
    v.run_for(1.0).unwrap();
    let passed = v.schedule_fault(dead_motor_2(), 0.5).unwrap_err();
    assert_eq!(passed, SimError::InvalidArgument("fault time 0.5 s has passed (the session is at 1 s)".into()));
    let bad = v.schedule_fault(Fault::MotorFailure { motor: 9, scale: 0.0 }, 2.0).unwrap_err();
    assert_eq!(bad, SimError::InvalidArgument("motor 9 does not exist (the quad has 4)".into()));
    assert!(matches!(v.schedule_fault(dead_motor_2(), f64::NAN), Err(SimError::InvalidArgument(_))));
    v.schedule_fault(dead_motor_2(), 1.0).unwrap();
    assert_eq!(v.active_fault_names(), ["motor_failure"], "now applies at once");
}

#[test]
fn clearing_faults_drops_the_scheduled_ones() {
    let mut v = vehicle();
    v.schedule_fault(dead_motor_2(), 0.2).unwrap();
    v.clear_faults();
    v.run_for(0.5).unwrap();
    assert!(v.active_fault_names().is_empty());
}

fn csv(text: &str) -> inputs::Inputs {
    inputs::parse("test.csv", text).unwrap()
}

#[test]
fn inputs_rows_apply_on_their_tick_counted_from_the_start_of_playback() {
    let mut v = vehicle();
    v.run_for(0.2).unwrap();
    v.play_inputs(csv("t_s,throttle\n0,0\n0.5,1\n")).unwrap();
    assert_eq!(v.inputs_playing(), Some("test.csv"));
    v.run_for(0.49).unwrap();
    assert_eq!(v.state().motor_cmd, [0.0; 4], "row 0.5 is due at 0.7 s");
    v.run_for(0.02).unwrap();
    assert_eq!(v.state().motor_cmd, [1.0; 4]);
    assert_eq!(v.inputs_playing(), None, "the last row applied: playback is over");
    assert_eq!(v.take_finished_inputs().as_deref(), Some("test.csv"));
    assert_eq!(v.take_finished_inputs(), None, "reported once");
    v.run_for(0.2).unwrap();
    assert_eq!(v.state().motor_cmd, [1.0; 4], "the last row holds");
}

#[test]
fn playback_is_the_same_however_the_run_is_cut_and_equals_setting_the_sticks() {
    let text = "t_s,throttle,roll\n0,0.4,0\n0.25,0.7,0.2\n0.6,0.5,-0.1\n";
    let mut whole = vehicle();
    whole.play_inputs(csv(text)).unwrap();
    whole.run_for(1.0).unwrap();
    let mut cut = vehicle();
    cut.play_inputs(csv(text)).unwrap();
    for _ in 0..10 {
        cut.run_for(0.1).unwrap();
    }
    assert_eq!(whole.digest(), cut.digest());

    let mut by_hand = vehicle();
    for (sticks, secs) in [((0.4, 0.0), 0.25), ((0.7, 0.2), 0.35), ((0.5, -0.1), 0.4)] {
        by_hand.set_sticks(&Sticks { throttle: sticks.0, roll: sticks.1, ..Sticks::default() }).unwrap();
        by_hand.run_for(secs).unwrap();
    }
    assert_eq!(whole.digest(), by_hand.digest(), "a file is the same as the SetSticks calls it stands for");
}

#[test]
fn columns_the_file_leaves_out_keep_their_value() {
    let armed = Sticks { aux: [1.0, 1.0, -1.0, -1.0], ..Sticks::default() };
    let mut played = vehicle();
    played.set_sticks(&armed).unwrap();
    played.play_inputs(csv("t_s,throttle\n0,0.5\n")).unwrap();
    played.run_for(0.3).unwrap();
    let mut by_hand = vehicle();
    by_hand.set_sticks(&Sticks { throttle: 0.5, ..armed }).unwrap();
    by_hand.run_for(0.3).unwrap();
    assert_eq!(played.digest(), by_hand.digest());
}

#[test]
fn a_second_file_waits_for_the_first_and_stop_keeps_the_sticks() {
    let mut v = vehicle();
    v.play_inputs(csv("t_s,throttle\n0,1\n5,0\n")).unwrap();
    let busy = v.play_inputs(csv("t_s,yaw\n0,0\n")).unwrap_err();
    assert_eq!(busy, SimError::InvalidArgument("test.csv is still playing; stop it first".into()));
    v.run_for(0.1).unwrap();
    assert_eq!(v.stop_inputs().as_deref(), Some("test.csv"));
    assert_eq!(v.stop_inputs(), None);
    assert_eq!(v.take_finished_inputs(), None, "stopped, not finished");
    v.run_for(0.1).unwrap();
    assert_eq!(v.state().motor_cmd, [1.0; 4], "the sticks keep the last applied row");
}
