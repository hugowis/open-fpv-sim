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
