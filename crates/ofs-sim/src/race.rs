//! Race timing: gates the quad flies through, and the lap clock they drive (docs/race-timing.md).
//!
//! A [`Gate`] is a vertical opening; [`Gate::crossing`] tests one step of the quad's path against it. A
//! [`RaceTracker`] applies the course rules to the stream of poses: gate 0 starts the clock, the gates must then be
//! passed in order, and gate 0 again completes a lap. Everything here is pure (no bus, no clock): the scheduler model
//! that feeds it is added around it.
use std::sync::{Arc, Mutex};

use glam::DVec3;
use ofs_config::world::GateSection;
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};

/// A gate that was just counted counts again (a one-gate course) only after the quad has been this far from its centre.
pub const REARM_M: f64 = 3.0;

/// One gate's geometry in NED metres.
#[derive(Debug, Clone)]
pub struct Gate {
    pub name: String,
    center: DVec3,
    forward: DVec3,
    lateral: DVec3,
    half_width: f64,
    half_height: f64,
}

impl Gate {
    pub fn from_config(g: &GateSection) -> Gate {
        let (sin, cos) = g.heading_deg.to_radians().sin_cos();
        Gate {
            name: g.name.clone(),
            center: DVec3::from_array(g.center_ned_m),
            forward: DVec3::new(cos, sin, 0.0),
            lateral: DVec3::new(-sin, cos, 0.0),
            half_width: g.width_m / 2.0,
            half_height: g.height_m / 2.0,
        }
    }

    /// Where on the step from `p0` to `p1` the path goes through the opening in the direction of travel: the
    /// fraction in (0, 1] of the step, or `None`. A step that starts on the plane does not cross it; one that ends on
    /// it does, so a path touching the plane is counted once.
    pub fn crossing(&self, p0: DVec3, p1: DVec3) -> Option<f64> {
        let s0 = (p0 - self.center).dot(self.forward);
        let s1 = (p1 - self.center).dot(self.forward);
        if !(s0 < 0.0 && s1 >= 0.0) {
            return None;
        }
        let fraction = -s0 / (s1 - s0);
        let at = p0 + (p1 - p0) * fraction - self.center;
        (at.dot(self.lateral).abs() <= self.half_width && at.z.abs() <= self.half_height).then_some(fraction)
    }
}

/// What happened at a crossing. Times are simulated seconds, interpolated inside the step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RaceEvent {
    /// Gate `gate` was passed; `split_s` is the time since the lap started (0 for the start itself).
    GatePassed { gate: usize, time_s: f64, split_s: f64 },
    /// Lap number `lap` (from 1) took `lap_s`; `best` when it is the fastest so far (the first lap is).
    LapCompleted { lap: u32, time_s: f64, lap_s: f64, best: bool },
}

/// The race so far. `last_lap_s` and `best_lap_s` are 0 until a lap completes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RaceStatus {
    pub running: bool,
    /// The gate the quad must fly through next (0 before the start, and again once all gates are passed).
    pub next_gate: usize,
    pub laps: u32,
    pub lap_start_s: f64,
    pub last_lap_s: f64,
    pub best_lap_s: f64,
}

#[derive(Debug, Clone)]
pub struct RaceTracker {
    gates: Vec<Gate>,
    prev: Option<DVec3>,
    status: RaceStatus,
    last_counted: Option<usize>,
    away: bool,
}

impl RaceTracker {
    pub fn new(gates: Vec<Gate>) -> Self {
        Self { gates, prev: None, status: RaceStatus::default(), last_counted: None, away: false }
    }

    pub fn status(&self) -> RaceStatus {
        self.status
    }

    pub fn gate_count(&self) -> usize {
        self.gates.len()
    }

    pub fn gate_name(&self, i: usize) -> &str {
        &self.gates[i].name
    }

    /// Feeds the pose `p1` reached at the end of a step that began at `t0_s` and lasted `dt_s`. The step runs from
    /// the pose the previous call was given (the first call only records its pose). At most one gate counts per step.
    pub fn step(&mut self, t0_s: f64, dt_s: f64, p1: DVec3) -> Option<RaceEvent> {
        let p0 = self.prev.replace(p1)?;
        if self.gates.is_empty() {
            return None;
        }
        if let Some(last) = self.last_counted {
            if (p1 - self.gates[last].center).length() > REARM_M {
                self.away = true;
            }
        }
        let next = self.status.next_gate;
        if self.last_counted == Some(next) && !self.away {
            return None;
        }
        let fraction = self.gates[next].crossing(p0, p1)?;
        let time_s = t0_s + fraction * dt_s;
        self.last_counted = Some(next);
        self.away = false;
        let s = &mut self.status;
        if !s.running {
            s.running = true;
            s.lap_start_s = time_s;
            s.next_gate = (next + 1) % self.gates.len();
            return Some(RaceEvent::GatePassed { gate: 0, time_s, split_s: 0.0 });
        }
        if next == 0 {
            let lap_s = time_s - s.lap_start_s;
            let best = s.laps == 0 || lap_s < s.best_lap_s;
            s.laps += 1;
            s.last_lap_s = lap_s;
            if best {
                s.best_lap_s = lap_s;
            }
            s.lap_start_s = time_s;
            s.next_gate = 1 % self.gates.len();
            return Some(RaceEvent::LapCompleted { lap: s.laps, time_s, lap_s, best });
        }
        s.next_gate = (next + 1) % self.gates.len();
        Some(RaceEvent::GatePassed { gate: next, time_s, split_s: time_s - s.lap_start_s })
    }
}

/// The race as a client sees it. All zero/false when the world has no gates.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RaceInfo {
    /// The world has gates.
    pub present: bool,
    pub running: bool,
    pub next_gate: u32,
    pub gate_count: u32,
    pub laps: u32,
    /// Time since the current lap started (0 until the clock runs).
    pub current_lap_s: f64,
    pub last_lap_s: f64,
    pub best_lap_s: f64,
}

/// The `race.*` bus signals.
#[derive(Debug, Clone, Copy)]
pub struct RaceSignals {
    running: Signal<f64>,
    next_gate: Signal<f64>,
    laps: Signal<f64>,
    lap_start_s: Signal<f64>,
    last_lap_s: Signal<f64>,
    best_lap_s: Signal<f64>,
}

impl RaceSignals {
    /// Creates (or finds) the signals; call it only for a world with gates, so other recordings never see them.
    pub fn register(bus: &mut Bus) -> Self {
        Self {
            running: bus.signal(names::RACE_RUNNING),
            next_gate: bus.signal(names::RACE_NEXT_GATE),
            laps: bus.signal(names::RACE_LAPS),
            lap_start_s: bus.signal(names::RACE_LAP_START_S),
            last_lap_s: bus.signal(names::RACE_LAST_LAP_S),
            best_lap_s: bus.signal(names::RACE_BEST_LAP_S),
        }
    }

    fn write(&self, bus: &mut Bus, s: &RaceStatus) {
        bus.set(self.running, f64::from(u8::from(s.running)));
        bus.set(self.next_gate, s.next_gate as f64);
        bus.set(self.laps, f64::from(s.laps));
        bus.set(self.lap_start_s, s.lap_start_s);
        bus.set(self.last_lap_s, s.last_lap_s);
        bus.set(self.best_lap_s, s.best_lap_s);
    }

    /// The race as of simulated time `time_s`, with `gate_count` gates in the course.
    pub fn info(&self, bus: &Bus, time_s: f64, gate_count: usize) -> RaceInfo {
        let running = bus.get(self.running) > 0.5;
        RaceInfo {
            present: true,
            running,
            next_gate: bus.get(self.next_gate) as u32,
            gate_count: gate_count as u32,
            laps: bus.get(self.laps) as u32,
            current_lap_s: if running { time_s - bus.get(self.lap_start_s) } else { 0.0 },
            last_lap_s: bus.get(self.last_lap_s),
            best_lap_s: bus.get(self.best_lap_s),
        }
    }
}

/// The events the model has raised and nobody has taken yet; clones share one queue.
#[derive(Debug, Clone, Default)]
pub struct RaceHandle(Arc<Mutex<Vec<RaceEvent>>>);

impl RaceHandle {
    pub fn take_events(&self) -> Vec<RaceEvent> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn push(&self, event: RaceEvent) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push(event);
    }
}

/// Reads the pose the physics wrote this tick and feeds the [`RaceTracker`]; register it after the physics.
pub struct GateTimer {
    tracker: RaceTracker,
    pos: Signal<DVec3>,
    signals: RaceSignals,
    handle: RaceHandle,
}

impl GateTimer {
    pub fn new(gates: &[GateSection], bus: &mut Bus) -> (GateTimer, RaceHandle) {
        let handle = RaceHandle::default();
        let signals = RaceSignals::register(bus);
        let tracker = RaceTracker::new(gates.iter().map(Gate::from_config).collect());
        signals.write(bus, &tracker.status());
        let timer = GateTimer { tracker, pos: bus.signal(names::BODY_POS_NED), signals, handle: handle.clone() };
        (timer, handle)
    }
}

impl Model for GateTimer {
    fn name(&self) -> &str {
        "race.gates"
    }

    fn rate_divisor(&self) -> u32 {
        1
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        if let Some(event) = self.tracker.step(ctx.time_s, ctx.dt_s, bus.get(self.pos)) {
            self.handle.push(event);
            self.signals.write(bus, &self.tracker.status());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A gate on the north line at `north`, `heading` degrees, 3 m wide and 2 m tall, opening from d = -2.5 to -0.5.
    fn section(name: &str, north: f64, heading: f64) -> GateSection {
        GateSection { name: name.into(), center_ned_m: [north, 0.0, -1.5], heading_deg: heading, width_m: 3.0, height_m: 2.0 }
    }

    fn gate(north: f64) -> Gate {
        Gate::from_config(&section("G", north, 0.0))
    }

    fn at(north: f64, east: f64, down: f64) -> DVec3 {
        DVec3::new(north, east, down)
    }

    fn course(n: usize) -> RaceTracker {
        RaceTracker::new((0..n).map(|i| Gate::from_config(&section(&format!("Gate{i}"), 30.0 * (i + 1) as f64, 0.0))).collect())
    }

    /// Flies straight at 10 m/s along the centre line from `from` to `to` north, in 0.1 s steps, starting at time
    /// `t`, and returns the events with the time after the last step.
    fn fly(r: &mut RaceTracker, t: f64, from: f64, to: f64) -> (Vec<RaceEvent>, f64) {
        let mut events = Vec::new();
        let mut north = from;
        let mut t = t;
        r.step(t, 0.0, at(north, 0.0, -1.5)); // positions the tracker without moving
        while north < to - 1e-9 {
            north += 1.0;
            if let Some(e) = r.step(t, 0.1, at(north, 0.0, -1.5)) {
                events.push(e);
            }
            t += 0.1;
        }
        (events, t)
    }

    #[test]
    fn a_step_through_the_opening_crosses_at_the_right_fraction() {
        let f = gate(30.0).crossing(at(29.0, 0.0, -1.5), at(33.0, 0.0, -1.5)).unwrap();
        assert!((f - 0.25).abs() < 1e-12, "{f}");
    }

    #[test]
    fn a_step_beside_above_or_below_the_opening_does_not_cross() {
        let g = gate(30.0);
        assert_eq!(g.crossing(at(29.0, 1.6, -1.5), at(31.0, 1.6, -1.5)), None, "beside");
        assert_eq!(g.crossing(at(29.0, -1.6, -1.5), at(31.0, -1.6, -1.5)), None, "beside, the other way");
        assert_eq!(g.crossing(at(29.0, 0.0, -2.6), at(31.0, 0.0, -2.6)), None, "above");
        assert_eq!(g.crossing(at(29.0, 0.0, -0.4), at(31.0, 0.0, -0.4)), None, "below");
        assert_eq!(g.crossing(at(29.0, 0.0, -1.5), at(29.5, 0.0, -1.5)), None, "short of the plane");
    }

    #[test]
    fn a_step_exactly_at_the_opening_edge_counts_and_one_just_outside_does_not() {
        let g = gate(30.0);
        assert!(g.crossing(at(29.0, 1.5, -2.5), at(31.0, 1.5, -2.5)).is_some(), "the top corner");
        assert!(g.crossing(at(29.0, -1.5, -0.5), at(31.0, -1.5, -0.5)).is_some(), "the bottom corner");
        assert_eq!(g.crossing(at(29.0, 1.5 + 1e-9, -1.5), at(31.0, 1.5 + 1e-9, -1.5)), None);
    }

    #[test]
    fn a_step_against_the_heading_does_not_cross() {
        assert_eq!(gate(30.0).crossing(at(31.0, 0.0, -1.5), at(29.0, 0.0, -1.5)), None);
    }

    #[test]
    fn a_step_ending_on_the_plane_crosses_and_the_next_step_does_not_cross_again() {
        let g = gate(30.0);
        assert_eq!(g.crossing(at(29.0, 0.0, -1.5), at(30.0, 0.0, -1.5)), Some(1.0));
        assert_eq!(g.crossing(at(30.0, 0.0, -1.5), at(31.0, 0.0, -1.5)), None);
    }

    #[test]
    fn a_gate_headed_east_is_crossed_eastward_only() {
        let g = Gate::from_config(&GateSection { name: "E".into(), center_ned_m: [0.0, 50.0, -1.5], heading_deg: 90.0, width_m: 3.0, height_m: 2.0 });
        assert!(g.crossing(at(0.0, 49.0, -1.5), at(0.0, 51.0, -1.5)).is_some(), "east");
        assert_eq!(g.crossing(at(0.0, 51.0, -1.5), at(0.0, 49.0, -1.5)), None, "west");
        assert_eq!(g.crossing(at(1.6, 49.0, -1.5), at(1.6, 51.0, -1.5)), None, "beside: the width runs north-south");
        assert!(g.crossing(at(-1.4, 49.0, -1.5), at(-1.4, 51.0, -1.5)).is_some(), "inside, to the south of the centre");
    }

    #[test]
    fn the_clock_starts_at_gate_zero_and_gates_are_counted_in_order() {
        let mut r = course(3);
        let (events, _) = fly(&mut r, 0.0, 0.0, 100.0);
        assert_eq!(events.len(), 3, "{events:?}");
        assert!(matches!(events[0], RaceEvent::GatePassed { gate: 0, split_s, .. } if split_s == 0.0));
        let RaceEvent::GatePassed { gate: 1, split_s, time_s } = events[1] else { panic!("{:?}", events[1]) };
        assert!((split_s - 3.0).abs() < 1e-9 && (time_s - 6.0).abs() < 1e-9, "{split_s} {time_s}");
        assert!(matches!(events[2], RaceEvent::GatePassed { gate: 2, .. }));
        let s = r.status();
        assert!(s.running && s.next_gate == 0 && s.laps == 0, "{s:?}");
    }

    #[test]
    fn a_gate_out_of_order_is_ignored() {
        let mut r = course(3);
        let (events, t) = fly(&mut r, 0.0, 0.0, 35.0); // gate 0 only
        assert_eq!(events.len(), 1);
        // Skip past gate 1 beside its opening, then fly through gate 2 (90 m): not the next gate.
        r.step(t, 0.1, at(89.0, 10.0, -1.5));
        r.step(t + 0.1, 0.1, at(89.0, 0.0, -1.5));
        assert_eq!(r.step(t + 0.2, 0.1, at(91.0, 0.0, -1.5)), None);
        assert_eq!(r.status().next_gate, 1);
        // Gate 1 still works afterwards.
        r.step(t + 0.3, 0.1, at(59.0, 10.0, -1.5));
        r.step(t + 0.4, 0.1, at(59.0, 0.0, -1.5));
        assert!(matches!(r.step(t + 0.5, 0.1, at(61.0, 0.0, -1.5)), Some(RaceEvent::GatePassed { gate: 1, .. })));
    }

    #[test]
    fn a_lap_completes_at_gate_zero_after_the_last_gate() {
        let mut r = course(3);
        let (_, t) = fly(&mut r, 0.0, 0.0, 100.0);
        // Turn around outside the course and come back through gate 0 northwards.
        r.step(t, 0.1, at(10.0, 40.0, -1.5));
        let (events, _) = fly(&mut r, t + 20.0, 10.0, 35.0);
        let Some(RaceEvent::LapCompleted { lap: 1, lap_s, best: true, .. }) = events.first().copied() else { panic!("{events:?}") };
        let s = r.status();
        assert_eq!((s.laps, s.next_gate), (1, 1));
        assert_eq!((s.last_lap_s, s.best_lap_s), (lap_s, lap_s));
        assert!(lap_s > 20.0, "{lap_s}");
    }

    #[test]
    fn only_a_faster_lap_is_best() {
        let mut r = course(2);
        let mut t = 0.0;
        let mut laps = Vec::new();
        // Four flights through gates 0 and 1, each followed by a wait off the course: the first only starts the
        // clock, the others close laps of about 16.5, 26.5 and 11.5 s.
        for pause in [10.0, 20.0, 5.0, 0.0] {
            let (events, t1) = fly(&mut r, t, 0.0, 65.0);
            laps.extend(events.into_iter().filter_map(|e| match e {
                RaceEvent::LapCompleted { lap_s, best, .. } => Some((lap_s, best)),
                _ => None,
            }));
            t = t1 + pause;
        }
        let best: Vec<bool> = laps.iter().map(|l| l.1).collect();
        assert_eq!(best, [true, false, true], "{laps:?}");
        let s = r.status();
        assert_eq!(s.laps, 3);
        assert_eq!(s.best_lap_s, laps[2].0);
        assert_eq!(s.last_lap_s, laps[2].0);
    }

    #[test]
    fn crossing_times_are_interpolated_inside_the_step() {
        let mut r = course(1);
        r.step(5.0, 0.0, at(29.0, 0.0, -1.5));
        let Some(RaceEvent::GatePassed { time_s, .. }) = r.step(5.0, 1.0, at(33.3, 0.0, -1.5)) else { panic!() };
        assert!((time_s - (5.0 + 1.0 / 4.3)).abs() < 1e-9, "{time_s}");
    }

    #[test]
    fn a_reverse_crossing_after_the_start_changes_nothing() {
        let mut r = course(3);
        let (_, t) = fly(&mut r, 0.0, 0.0, 35.0);
        let before = r.status();
        r.step(t, 0.1, at(31.0, 0.0, -1.5));
        assert_eq!(r.step(t + 0.1, 0.1, at(29.0, 0.0, -1.5)), None);
        assert_eq!(r.status(), before);
        // And forward again through gate 0: not the next gate, so still nothing.
        assert_eq!(r.step(t + 0.2, 0.1, at(31.0, 0.0, -1.5)), None);
        assert_eq!(r.status(), before);
    }

    #[test]
    fn a_very_fast_step_over_the_plane_counts_once() {
        let mut r = course(1);
        r.step(0.0, 0.0, at(10.0, 0.0, -1.5));
        assert!(r.step(0.0, 1.0, at(50.0, 0.0, -1.5)).is_some());
        r.step(1.0, 1.0, at(90.0, 0.0, -1.5));
        assert_eq!(r.status().laps, 0, "gate 0 behind the quad: nothing more");
    }

    #[test]
    fn a_single_gate_course_counts_a_lap_only_after_leaving_and_coming_back() {
        let mut r = course(1);
        r.step(0.0, 0.0, at(29.9, 0.0, -1.5));
        assert!(matches!(r.step(0.0, 0.1, at(30.1, 0.0, -1.5)), Some(RaceEvent::GatePassed { gate: 0, .. })));
        // Jitter through the plane while hovering at the gate.
        for i in 0..10 {
            let wobble = if i % 2 == 0 { 29.9 } else { 30.1 };
            assert_eq!(r.step(1.0 + f64::from(i), 1.0, at(wobble, 0.0, -1.5)), None, "jitter {i}");
        }
        assert_eq!(r.status().laps, 0);
        // Go well away, come back, cross.
        r.step(20.0, 1.0, at(25.0, 0.0, -1.5));
        r.step(21.0, 1.0, at(29.0, 0.0, -1.5));
        let Some(RaceEvent::LapCompleted { lap: 1, .. }) = r.step(22.0, 1.0, at(31.0, 0.0, -1.5)) else { panic!() };
    }

    #[test]
    fn a_course_with_no_gates_never_raises_anything() {
        let mut r = RaceTracker::new(Vec::new());
        r.step(0.0, 0.0, at(0.0, 0.0, 0.0));
        assert_eq!(r.step(0.0, 1.0, at(100.0, 0.0, 0.0)), None);
        assert_eq!(r.status(), RaceStatus::default());
    }
}
