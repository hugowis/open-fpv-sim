//! The GateTimer model on a bare scheduler: a test model flies the quad along a path, the timer reads its poses.
use glam::DVec3;
use ofs_config::world::GateSection;
use ofs_core::{names, Bus, Model, Scheduler, SimError, StepCtx};
use ofs_sim::race::{GateTimer, RaceEvent, RaceHandle};

/// Writes `body.pos_ned_m` for the end of every tick from a function of the time.
struct Mover(Box<dyn FnMut(f64) -> DVec3 + Send>);

impl Model for Mover {
    fn name(&self) -> &str {
        "test.mover"
    }
    fn rate_divisor(&self) -> u32 {
        1
    }
    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let pos = bus.signal::<DVec3>(names::BODY_POS_NED);
        bus.set(pos, (self.0)(ctx.time_s + ctx.dt_s));
        Ok(())
    }
}

fn course() -> Vec<GateSection> {
    (1..=3)
        .map(|i| GateSection {
            name: format!("Gate{}", i - 1),
            center_ned_m: [30.0 * f64::from(i), 0.0, -1.5],
            heading_deg: 0.0,
            width_m: 3.0,
            height_m: 2.0,
        })
        .collect()
}

/// A quad flying north along the centre line at `speed` from 5 m south of home to 100 m, and starting over again
/// (the jump back is a backwards crossing of every gate, which counts for nothing).
fn rig(base_hz: u32, speed: f64) -> (Scheduler, RaceHandle) {
    let mut bus = Bus::new();
    let (timer, handle) = GateTimer::new(&course(), &mut bus);
    let lap_s = 105.0 / speed;
    let mut s = Scheduler::new(base_hz, bus);
    s.add(Box::new(Mover(Box::new(move |t| DVec3::new(-5.0 + speed * (t % lap_s), 0.0, -1.5)))));
    s.add(Box::new(timer));
    (s, handle)
}

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

#[test]
fn a_course_flown_in_order_raises_start_splits_and_a_lap() {
    let (mut s, handle) = rig(8000, 15.0);
    s.run_for(15.0).unwrap();
    let events = handle.take_events();
    assert_eq!(events.len(), 6, "{events:?}");
    assert!(matches!(events[0], RaceEvent::GatePassed { gate: 0, split_s, .. } if split_s == 0.0));
    let RaceEvent::GatePassed { gate: 1, split_s, .. } = events[1] else { panic!("{:?}", events[1]) };
    assert!((split_s - 2.0).abs() < 1e-6, "{split_s}");
    let RaceEvent::LapCompleted { lap: 1, lap_s, best: true, time_s } = events[3] else { panic!("{:?}", events[3]) };
    assert!((lap_s - 7.0).abs() < 1e-6 && (time_s - (7.0 + 35.0 / 15.0)).abs() < 1e-6, "{lap_s} {time_s}");
    assert!(handle.take_events().is_empty(), "taking the events empties the queue");
    assert_eq!((get(&s, names::RACE_RUNNING), get(&s, names::RACE_LAPS)), (1.0, 1.0));
    assert!((get(&s, names::RACE_LAST_LAP_S) - 7.0).abs() < 1e-6);
    assert_eq!(get(&s, names::RACE_BEST_LAP_S), get(&s, names::RACE_LAST_LAP_S));
    assert_eq!(get(&s, names::RACE_NEXT_GATE), 0.0, "gates 1 and 2 of the second lap are passed: gate 0 is next");
}

#[test]
fn a_quad_at_100_mps_is_counted_once_per_gate_at_any_tick_rate() {
    for base_hz in [8000, 100] {
        let (mut s, handle) = rig(base_hz, 100.0);
        s.run_for(1.0).unwrap();
        let events = handle.take_events();
        assert_eq!(events.len(), 3, "{base_hz} Hz: {events:?}");
        let RaceEvent::GatePassed { gate: 2, split_s, .. } = events[2] else { panic!("{:?}", events[2]) };
        assert!((split_s - 0.6).abs() < 1e-6, "{base_hz} Hz: {split_s}");
    }
}

/// The race path end to end through a `Session`: a stabilized quad steered through Gate0 of the circuit world raises
/// a GatePassed event in `changes()` with the same text the state message's formatter produces, and every batch the
/// client sees is time-ordered (the circuit example's pilot, in miniature: altitude hold, then two waypoints).
#[test]
fn a_gate_event_arrives_through_session_changes() {
    use ofs_config::{load, FcKind};
    use std::path::Path;
    use ofs_sim::pacer::OverrunPolicy;
    use ofs_sim::session::{RunMode, Session};
    use ofs_sim::vehicle::{build, BuildOptions, Sticks};

    let quad = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml"));
    let world = ofs_config::world::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/circuit.toml"))).unwrap();
    let cfg = load(quad).unwrap();
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-gates-session"), fc_override: Some(FcKind::OpenLoopStabilized), world };
    let mut session = Session::new(build(&cfg, &opts, None).unwrap(), RunMode::Lockstep, OverrunPolicy::Warn, false);

    const DT: f64 = 0.02;
    let mut integral = 0.0_f64;
    let sticks = |s: &ofs_sim::vehicle::VehicleState, integral: &mut f64, target: (f64, f64)| {
        let (north, east) = (s.pos_ned_m.x, s.pos_ned_m.y);
        let (vn, ve) = (s.vel_ned_mps.x, s.vel_ned_mps.y);
        let (to_n, to_e) = (target.0 - north, target.1 - east);
        let dist = to_n.hypot(to_e);
        let scale = if dist > 1e-6 { (5.0 / (0.8 * dist)).min(1.0) } else { 0.0 };
        let (an, ae) = ((2.0 * (0.8 * to_n * scale - vn)).clamp(-3.0, 3.0), (2.0 * (0.8 * to_e * scale - ve)).clamp(-3.0, 3.0));
        let pitch = an.atan2(9.81).to_degrees() / 60.0; // a lean of atan(a/g) degrees, one stick = 60 degrees
        let roll = ae.atan2(9.81).to_degrees() / 60.0;
        let err = 2.0 + s.pos_ned_m.z; // 2 m up: down is negative
        *integral = (*integral + err * DT).clamp(-2.0, 2.0);
        let climb = -s.vel_ned_mps.z;
        let throttle = (0.42 + 0.15 * err + 0.10 * *integral - 0.10 * climb).clamp(0.0, 1.0);
        Sticks { roll: roll.clamp(-1.0, 1.0), pitch: pitch.clamp(-1.0, 1.0), yaw: 0.0, throttle, aux: [-1.0; 4] }
    };

    let ticks = (DT * f64::from(session.vehicle.base_hz())) as u64;
    let mut gate_events: Vec<(f64, String)> = Vec::new();
    // Gate0 stands at (north 20, east 0), opening facing east, 3 m to a side: settle west of it at north 20, then
    // sweep east through the opening.
    for target in [(20.0, -8.0), (20.0, -3.5), (20.0, 8.0)] {
        for _ in 0..700 {
            let s = session.vehicle.state();
            session.vehicle.set_sticks(&sticks(&s, &mut integral, target)).unwrap();
            session.step(ticks).unwrap();
            for e in session.changes() {
                if e.kind == ofs_sim::pb::EventKind::GatePassed as i32 {
                    gate_events.push((e.time_s, e.message));
                }
            }
            let s = session.vehicle.state();
            let arrived = (target.0 - s.pos_ned_m.x).hypot(target.1 - s.pos_ned_m.y) <= 0.7;
            if arrived {
                break; // this waypoint is made; the next one steers from here
            }
        }
    }
    assert!(
        gate_events.iter().any(|(_, m)| m == "Gate0 (gate 0) start"),
        "Gate0's start crossing arrives through changes(): {gate_events:?}"
    );
    // The flight raises one race event, so batch order is not exercisable here; the unit test
    // `a_batch_of_events_is_time_ordered_with_ties_kept_in_insertion_order` pins it.
}
