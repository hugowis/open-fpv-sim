//! The journal records what steers a run, and replaying it reproduces the run (repro bundle, spec §7).
use std::path::Path;

use ofs_config::{load, FcKind, WorldConfig};
use ofs_sim::inputs;
use ofs_sim::journal::Action;
use ofs_sim::vehicle::{build, BuildOptions, Fault, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn vehicle() -> Vehicle {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed: 7, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    build(&cfg, &opts).unwrap()
}

fn climb() -> inputs::Inputs {
    inputs::parse("climb.csv", "t_s,throttle\n0,0.5\n0.02,0.9\n").unwrap()
}

/// A script that touches every steering call, stepping between them.
fn drive(v: &mut Vehicle) {
    let hover = Sticks { throttle: 0.3, ..Sticks::default() };
    v.set_sticks(&hover);
    v.set_sticks(&hover); // the same values again: nothing new to journal
    v.step_ticks(100).unwrap();
    v.set_sticks(&Sticks { throttle: 0.6, roll: 0.2, ..Sticks::default() });
    v.set_transmitter(false);
    v.step_ticks(100).unwrap();
    v.set_transmitter(true);
    v.set_fault(Fault::MotorFailure { motor: 2, scale: 0.0 }, true).unwrap();
    v.schedule_fault(Fault::GyroFault { bias_radps: glam::DVec3::new(0.1, 0.0, 0.0), noise_scale: 2.0 }, 0.05).unwrap(); // tick 400
    v.step_ticks(300).unwrap(); // passes tick 400: the activation adds no entry
    v.clear_faults();
    v.play_inputs(climb()).unwrap();
    v.step_ticks(50).unwrap();
    v.stop_inputs();
    v.step_ticks(50).unwrap();
}

#[test]
fn a_vehicle_journals_what_steers_it() {
    let mut v = vehicle();
    assert!(v.journal().entries().is_empty(), "the initial setup in build is not journaled");
    drive(&mut v);
    let seen: Vec<(u64, &str)> = v
        .journal()
        .entries()
        .iter()
        .map(|e| {
            (
                e.tick,
                match &e.action {
                    Action::Sticks(_) => "sticks",
                    Action::Transmitter(_) => "tx",
                    Action::Fault { .. } => "fault",
                    Action::ScheduleFault { .. } => "schedule",
                    Action::ClearFaults => "clear",
                    Action::PlayInputs(_) => "play",
                    Action::StopInputs => "stop",
                },
            )
        })
        .collect();
    assert_eq!(
        seen,
        [(0, "sticks"), (100, "sticks"), (100, "tx"), (200, "tx"), (200, "fault"), (200, "schedule"), (500, "clear"), (500, "play"), (550, "stop")]
    );
}

#[test]
fn a_refused_call_is_not_journaled() {
    let mut v = vehicle();
    assert!(v.set_fault(Fault::MotorFailure { motor: 9, scale: 0.0 }, true).is_err());
    assert!(v.schedule_fault(Fault::RadioLinkLoss, f64::NAN).is_err());
    v.step_ticks(10).unwrap();
    assert!(v.schedule_fault(Fault::RadioLinkLoss, 0.0).is_err(), "in the past");
    assert!(v.journal().entries().is_empty(), "{:?}", v.journal().entries());
}

#[test]
fn replaying_a_journal_reproduces_the_run() {
    let mut a = vehicle();
    drive(&mut a);
    let mut b = vehicle();
    for e in a.journal().entries() {
        let behind = e.tick - b.tick();
        b.step_ticks(behind).unwrap();
        b.replay_action(&e.action).unwrap();
    }
    b.step_ticks(a.tick() - b.tick()).unwrap();
    assert_eq!(a.tick(), b.tick());
    assert_eq!(a.digest(), b.digest());
    assert_eq!(a.state(), b.state());
    // And the journal a replay leaves is the one it replayed.
    assert_eq!(a.journal().entries().len(), b.journal().entries().len());
}
