//! Race timing: gates the quad flies through, and the lap clock they drive (docs/race-timing.md).
//!
//! A [`Gate`] is a vertical opening; [`Gate::crossing`] tests one step of the quad's path against it. A
//! [`RaceTracker`] applies the course rules to the stream of poses: gate 0 starts the clock, the gates must then be
//! passed in order, and gate 0 again completes a lap. Everything here is pure (no bus, no clock): the scheduler model
//! that feeds it is added around it.
use std::sync::{Arc, Mutex};

use glam::DVec3;
use ofs_config::world::{Course, GateSection, RaceMode};
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};

/// A gate that was just counted counts again (a one-gate course) only after the quad has been this far from its plane.
pub const REARM_M: f64 = 3.0;

/// One gate's geometry in NED metres.
#[derive(Debug, Clone)]
pub struct Gate {
    pub name: String,
    center: DVec3,
    forward: DVec3,
    lateral: DVec3,
    /// The opening's height axis (NED "down" for an upright gate).
    down: DVec3,
    half_width: f64,
    half_height: f64,
    /// The quad's bounding radius: a pass whose centre is closer than this to the opening's edge is clipped.
    clearance_m: f64,
}

/// Where a step went through a gate's opening in the direction of travel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing {
    /// The fraction in (0, 1] of the step.
    pub fraction: f64,
    /// The quad's whole bounding sphere is inside the opening; if not, the pass clipped a post or the bar.
    pub clean: bool,
}

impl Gate {
    /// `clearance_m` is the quad's bounding radius (0: only its centre has to be inside the opening).
    pub fn from_config(g: &GateSection, clearance_m: f64) -> Gate {
        let (forward, lateral, down) = g.axes();
        Gate {
            name: g.name.clone(),
            center: DVec3::from_array(g.center_ned_m),
            forward: DVec3::from_array(forward),
            lateral: DVec3::from_array(lateral),
            down: DVec3::from_array(down),
            half_width: g.width_m / 2.0,
            half_height: g.height_m / 2.0,
            clearance_m,
        }
    }

    /// Where on the step from `p0` to `p1` the path goes through the opening in the direction of travel, or `None`.
    /// A step that starts on the plane does not cross it; one that ends on it does, so a path touching the plane is
    /// counted once. The centre must be inside the opening; `clean` also asks for the quad's radius to fit.
    pub fn crossing(&self, p0: DVec3, p1: DVec3) -> Option<Crossing> {
        let s0 = (p0 - self.center).dot(self.forward);
        let s1 = (p1 - self.center).dot(self.forward);
        if !(s0 < 0.0 && s1 >= 0.0) {
            return None;
        }
        let fraction = -s0 / (s1 - s0);
        let at = p0 + (p1 - p0) * fraction - self.center;
        let (across, up) = (at.dot(self.lateral).abs(), at.dot(self.down).abs());
        if across > self.half_width || up > self.half_height {
            return None;
        }
        let r = self.clearance_m;
        Some(Crossing { fraction, clean: across <= self.half_width - r && up <= self.half_height - r })
    }
}

/// What happened at a crossing. Times are simulated seconds, interpolated inside the step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RaceEvent {
    /// Gate `gate` (its place in the course `course`) was passed; `split_s` is the time since the lap started (0 for
    /// the start itself).
    GatePassed { gate: usize, course: usize, time_s: f64, split_s: f64 },
    /// Lap number `lap` (from 1) took `lap_s`; `best` when it is the fastest so far (the first lap is). `finished`
    /// when it was the world's last lap: the race is over.
    LapCompleted { lap: u32, time_s: f64, lap_s: f64, best: bool, finished: bool },
    /// The quad went through the next gate's opening but its body did not fit: no credit, the gate is still next.
    GateHit { gate: usize, course: usize, time_s: f64 },
    /// A time-attack race ran out of time with `laps` laps completed: the race is over.
    TimeUp { time_s: f64, laps: u32 },
    /// The race was reset (onto course `course`): no clock, no laps, gate 0 next.
    Reset { time_s: f64, course: usize },
}

/// The race so far. `last_lap_s` and `best_lap_s` are 0 until a lap completes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RaceStatus {
    /// The course being raced (an index into the world's courses).
    pub course: usize,
    /// The clock is running (gate 0 started it and the lap limit is not reached).
    pub running: bool,
    /// The lap limit is reached: the clock has stopped and nothing counts until a reset.
    pub finished: bool,
    /// The gate the quad must fly through next (0 before the start, and again once all gates are passed).
    pub next_gate: usize,
    pub laps: u32,
    /// When the race clock started (gate 0's first pass): a time-attack deadline counts from here.
    pub start_s: f64,
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
    lap_limit: Option<u32>,
    time_limit: Option<f64>,
}

impl RaceTracker {
    pub fn new(gates: Vec<Gate>) -> Self {
        Self { gates, prev: None, status: RaceStatus::default(), last_counted: None, away: false, lap_limit: None, time_limit: None }
    }

    /// The race is over after this many laps (none: it goes on).
    pub fn with_lap_limit(mut self, laps: Option<u32>) -> Self {
        self.lap_limit = laps;
        self
    }

    /// A time-attack race ends this many seconds after gate 0 first starts the clock (none: it goes on).
    pub fn with_time_limit(mut self, seconds: Option<f64>) -> Self {
        self.time_limit = seconds;
        self
    }

    /// Switches to course number `course`: its `gates` in order, its lap and time limits. Starts a fresh race.
    pub fn set_course(&mut self, course: usize, gates: Vec<Gate>, lap_limit: Option<u32>, time_limit: Option<f64>) {
        self.gates = gates;
        self.lap_limit = lap_limit;
        self.time_limit = time_limit;
        self.status.course = course;
        self.reset();
    }

    /// Starts a fresh race on the same course: no clock, no laps, no best time, gate 0 next. The path so far is kept,
    /// so a quad that is already moving through a gate this step still counts.
    pub fn reset(&mut self) {
        self.status = RaceStatus { course: self.status.course, ..RaceStatus::default() };
        self.last_counted = None;
        self.away = false;
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
        if self.gates.is_empty() || self.status.finished {
            return None;
        }
        if let Some(last) = self.last_counted {
            // Measured along the direction of travel: a wide gate's far edge is farther than REARM_M from its centre.
            if (p1 - self.gates[last].center).dot(self.gates[last].forward).abs() > REARM_M {
                self.away = true;
            }
        }
        let deadline = self.status.running.then(|| self.time_limit.map(|t| self.status.start_s + t)).flatten();
        if let Some(end) = deadline.filter(|end| t0_s >= *end) {
            return Some(self.time_up(end));
        }
        let next = self.status.next_gate;
        if self.last_counted == Some(next) && !self.away {
            return None;
        }
        let Crossing { fraction, clean } = self.gates[next].crossing(p0, p1)?;
        let time_s = t0_s + fraction * dt_s;
        if let Some(end) = deadline.filter(|end| time_s > *end) {
            return Some(self.time_up(end));
        }
        let course = self.status.course;
        // A clipped pass also waits for the quad to leave before the gate can be tried again, so hovering at a post
        // does not raise a stream of hits.
        self.last_counted = Some(next);
        self.away = false;
        if !clean {
            return Some(RaceEvent::GateHit { gate: next, course, time_s });
        }
        let s = &mut self.status;
        if !s.running {
            s.running = true;
            s.start_s = time_s;
            s.lap_start_s = time_s;
            s.next_gate = (next + 1) % self.gates.len();
            return Some(RaceEvent::GatePassed { gate: 0, course, time_s, split_s: 0.0 });
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
            let finished = self.lap_limit.is_some_and(|limit| s.laps >= limit);
            if finished {
                s.running = false;
                s.finished = true;
                s.next_gate = 0;
            }
            return Some(RaceEvent::LapCompleted { lap: s.laps, time_s, lap_s, best, finished });
        }
        s.next_gate = (next + 1) % self.gates.len();
        Some(RaceEvent::GatePassed { gate: next, course, time_s, split_s: time_s - s.lap_start_s })
    }

    /// The clock ran out at `end`: the lap in progress is dropped.
    fn time_up(&mut self, end: f64) -> RaceEvent {
        let s = &mut self.status;
        s.running = false;
        s.finished = true;
        s.next_gate = 0;
        RaceEvent::TimeUp { time_s: end, laps: s.laps }
    }
}

/// The race as a client sees it. All zero/false when the world has no gates.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RaceInfo {
    /// The world has gates.
    pub present: bool,
    /// The course being raced (an index into the world's courses) and how it is won.
    pub course: u32,
    pub mode: RaceMode,
    /// The time-attack clock (0 in a lap race) and what is left of it: the whole limit before the start, 0 once
    /// the race is over.
    pub time_limit_s: f64,
    pub time_left_s: f64,
    pub running: bool,
    /// The world's lap limit is reached: the clock has stopped.
    pub finished: bool,
    pub next_gate: u32,
    pub gate_count: u32,
    pub laps: u32,
    /// The world's lap limit (0 = none).
    pub lap_limit: u32,
    /// Time since the current lap started (0 until the clock runs).
    pub current_lap_s: f64,
    pub last_lap_s: f64,
    pub best_lap_s: f64,
}

/// The `race.*` bus signals.
#[derive(Debug, Clone, Copy)]
pub struct RaceSignals {
    running: Signal<f64>,
    finished: Signal<f64>,
    next_gate: Signal<f64>,
    course: Signal<f64>,
    laps: Signal<f64>,
    start_s: Signal<f64>,
    lap_start_s: Signal<f64>,
    last_lap_s: Signal<f64>,
    best_lap_s: Signal<f64>,
}

impl RaceSignals {
    /// Creates (or finds) the signals; call it only for a world with gates, so other recordings never see them.
    pub fn register(bus: &mut Bus) -> Self {
        Self {
            running: bus.signal(names::RACE_RUNNING),
            finished: bus.signal(names::RACE_FINISHED),
            next_gate: bus.signal(names::RACE_NEXT_GATE),
            course: bus.signal(names::RACE_COURSE),
            laps: bus.signal(names::RACE_LAPS),
            start_s: bus.signal(names::RACE_START_S),
            lap_start_s: bus.signal(names::RACE_LAP_START_S),
            last_lap_s: bus.signal(names::RACE_LAST_LAP_S),
            best_lap_s: bus.signal(names::RACE_BEST_LAP_S),
        }
    }

    fn write(&self, bus: &mut Bus, s: &RaceStatus) {
        bus.set(self.running, f64::from(u8::from(s.running)));
        bus.set(self.finished, f64::from(u8::from(s.finished)));
        bus.set(self.next_gate, s.next_gate as f64);
        bus.set(self.course, s.course as f64);
        bus.set(self.laps, f64::from(s.laps));
        bus.set(self.start_s, s.start_s);
        bus.set(self.lap_start_s, s.lap_start_s);
        bus.set(self.last_lap_s, s.last_lap_s);
        bus.set(self.best_lap_s, s.best_lap_s);
    }

    /// The race as of simulated time `time_s`, on whichever of the world's `courses` the bus says is being raced.
    pub fn info(&self, bus: &Bus, time_s: f64, courses: &[Course]) -> RaceInfo {
        let index = (bus.get(self.course) as usize).min(courses.len().saturating_sub(1));
        let Some(course) = courses.get(index) else { return RaceInfo::default() };
        let (running, finished) = (bus.get(self.running) > 0.5, bus.get(self.finished) > 0.5);
        let limit = course.time_limit_s.unwrap_or(0.0);
        let time_left_s = match (course.mode, running, finished) {
            (RaceMode::Laps, ..) | (_, _, true) => 0.0,
            (RaceMode::TimeAttack, true, false) => (bus.get(self.start_s) + limit - time_s).clamp(0.0, limit),
            (RaceMode::TimeAttack, false, false) => limit,
        };
        RaceInfo {
            present: true,
            course: index as u32,
            mode: course.mode,
            time_limit_s: limit,
            time_left_s,
            running,
            finished,
            next_gate: bus.get(self.next_gate) as u32,
            gate_count: course.gates.len() as u32,
            laps: bus.get(self.laps) as u32,
            lap_limit: course.laps.unwrap_or(0),
            current_lap_s: if running { time_s - bus.get(self.lap_start_s) } else { 0.0 },
            last_lap_s: bus.get(self.last_lap_s),
            best_lap_s: bus.get(self.best_lap_s),
        }
    }
}

#[derive(Debug, Default)]
struct Shared {
    events: Vec<RaceEvent>,
    /// A reset waiting for the next tick, onto the given course (`None`: the current one).
    reset: Option<Option<usize>>,
}

/// The events the model has raised and nobody has taken yet, and a reset waiting for the next tick; clones share one
/// queue.
#[derive(Debug, Clone, Default)]
pub struct RaceHandle(Arc<Mutex<Shared>>);

impl RaceHandle {
    pub fn take_events(&self) -> Vec<RaceEvent> {
        std::mem::take(&mut self.lock().events)
    }

    /// Asks for a fresh race, on course number `course` or the current one; the model applies it at the start of its
    /// next tick (and raises [`RaceEvent::Reset`]).
    pub fn request_reset(&self, course: Option<usize>) {
        self.lock().reset = Some(course);
    }

    fn take_reset(&self) -> Option<Option<usize>> {
        std::mem::take(&mut self.lock().reset)
    }

    fn push(&self, event: RaceEvent) {
        self.lock().events.push(event);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Reads the pose the physics wrote this tick and feeds the [`RaceTracker`]; register it after the physics.
pub struct GateTimer {
    tracker: RaceTracker,
    /// Every declared gate; a course picks from them by index.
    gates: Vec<Gate>,
    courses: Vec<Course>,
    pos: Signal<DVec3>,
    signals: RaceSignals,
    handle: RaceHandle,
}

impl GateTimer {
    /// The implicit course: all the gates in file order. `clearance_m`: the quad's bounding radius; `lap_limit`: the
    /// race ends after this many laps.
    pub fn new(gates: &[GateSection], clearance_m: f64, lap_limit: Option<u32>, bus: &mut Bus) -> (GateTimer, RaceHandle) {
        let course = Course { name: String::new(), gates: (0..gates.len()).collect(), mode: RaceMode::Laps, laps: lap_limit, time_limit_s: None };
        GateTimer::with_courses(gates, vec![course], 0, clearance_m, bus)
    }

    /// `courses` (never empty) pick from `gates`; the race starts on course `first`.
    pub fn with_courses(gates: &[GateSection], courses: Vec<Course>, first: usize, clearance_m: f64, bus: &mut Bus) -> (GateTimer, RaceHandle) {
        let handle = RaceHandle::default();
        let signals = RaceSignals::register(bus);
        let gates: Vec<Gate> = gates.iter().map(|g| Gate::from_config(g, clearance_m)).collect();
        let mut tracker = RaceTracker::new(Vec::new());
        let c = &courses[first];
        tracker.set_course(first, c.gates.iter().map(|&i| gates[i].clone()).collect(), c.laps, c.time_limit_s);
        signals.write(bus, &tracker.status());
        let timer = GateTimer { tracker, gates, courses, pos: bus.signal(names::BODY_POS_NED), signals, handle: handle.clone() };
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
        if let Some(course) = self.handle.take_reset() {
            let course = course.filter(|&c| c < self.courses.len()).unwrap_or(self.tracker.status().course);
            let c = &self.courses[course];
            self.tracker.set_course(course, c.gates.iter().map(|&i| self.gates[i].clone()).collect(), c.laps, c.time_limit_s);
            self.handle.push(RaceEvent::Reset { time_s: ctx.time_s, course });
            self.signals.write(bus, &self.tracker.status());
        }
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
        GateSection { name: name.into(), center_ned_m: [north, 0.0, -1.5], heading_deg: heading, width_m: 3.0, height_m: 2.0, pitch_deg: 0.0, roll_deg: 0.0 }
    }

    fn gate(north: f64) -> Gate {
        Gate::from_config(&section("G", north, 0.0), 0.0)
    }

    fn at(north: f64, east: f64, down: f64) -> DVec3 {
        DVec3::new(north, east, down)
    }

    fn course(n: usize) -> RaceTracker {
        RaceTracker::new((0..n).map(|i| Gate::from_config(&section(&format!("Gate{i}"), 30.0 * (i + 1) as f64, 0.0), 0.0)).collect())
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
        let f = gate(30.0).crossing(at(29.0, 0.0, -1.5), at(33.0, 0.0, -1.5)).unwrap().fraction;
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
        assert_eq!(g.crossing(at(29.0, 0.0, -1.5), at(30.0, 0.0, -1.5)).map(|c| c.fraction), Some(1.0));
        assert_eq!(g.crossing(at(30.0, 0.0, -1.5), at(31.0, 0.0, -1.5)), None);
    }

    #[test]
    fn a_gate_headed_east_is_crossed_eastward_only() {
        let g = Gate::from_config(&GateSection { name: "E".into(), center_ned_m: [0.0, 50.0, -1.5], heading_deg: 90.0, width_m: 3.0, height_m: 2.0, pitch_deg: 0.0, roll_deg: 0.0 }, 0.0);
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
        let RaceEvent::GatePassed { gate: 1, split_s, time_s, .. } = events[1] else { panic!("{:?}", events[1]) };
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
    fn jitter_at_the_edge_of_a_wide_single_gate_does_not_run_up_laps() {
        // 8 m wide: hovering 3.5 m off the centre line is inside the opening yet more than REARM_M from its centre.
        let mut r = RaceTracker::new(vec![Gate::from_config(&GateSection {
            name: "Wide".into(),
            center_ned_m: [30.0, 0.0, -1.5],
            heading_deg: 0.0,
            width_m: 8.0,
            height_m: 2.0,
            pitch_deg: 0.0,
            roll_deg: 0.0,
        }, 0.0)]);
        r.step(0.0, 0.0, at(29.9, 3.5, -1.5));
        assert!(matches!(r.step(0.0, 0.1, at(30.1, 3.5, -1.5)), Some(RaceEvent::GatePassed { gate: 0, .. })));
        for i in 0..10 {
            let wobble = if i % 2 == 0 { 29.9 } else { 30.1 };
            assert_eq!(r.step(1.0 + f64::from(i), 1.0, at(wobble, 3.5, -1.5)), None, "jitter {i}");
        }
        assert_eq!(r.status().laps, 0);
    }

    /// Flies the three-gate course through to gate 0 again, `laps` times over (the first pass only starts the clock),
    /// turning around outside the course between flights.
    fn fly_laps(r: &mut RaceTracker, laps: u32) -> (Vec<RaceEvent>, f64) {
        let (mut events, mut t) = fly(r, 0.0, 0.0, 100.0);
        for _ in 0..laps {
            r.step(t, 0.1, at(10.0, 40.0, -1.5));
            let (e, t1) = fly(r, t + 20.0, 10.0, 100.0);
            events.extend(e);
            t = t1;
        }
        (events, t)
    }

    #[test]
    fn the_race_ends_at_the_lap_limit_and_nothing_more_counts() {
        let mut r = course(3).with_lap_limit(Some(2));
        let (events, t) = fly_laps(&mut r, 2);
        let laps: Vec<(u32, bool)> = events
            .iter()
            .filter_map(|e| match *e {
                RaceEvent::LapCompleted { lap, finished, .. } => Some((lap, finished)),
                _ => None,
            })
            .collect();
        assert_eq!(laps, [(1, false), (2, true)], "{events:?}");
        let s = r.status();
        assert!(s.finished && !s.running && s.laps == 2 && s.next_gate == 0, "{s:?}");
        assert!(s.best_lap_s > 0.0 && s.last_lap_s > 0.0, "the times survive the finish");
        let (more, _) = fly(&mut r, t + 20.0, 0.0, 100.0);
        assert!(more.is_empty(), "a flight after the finish counts for nothing: {more:?}");
        assert_eq!(r.status(), s);
    }

    #[test]
    fn without_a_limit_the_race_goes_on() {
        let mut r = course(3);
        let (events, _) = fly_laps(&mut r, 4);
        assert_eq!(events.iter().filter(|e| matches!(e, RaceEvent::LapCompleted { finished: false, .. })).count(), 4);
        assert!(!r.status().finished);
    }

    #[test]
    fn a_one_lap_limit_finishes_on_the_first_lap() {
        let mut r = course(2).with_lap_limit(Some(1));
        let (events, _) = fly_laps(&mut r, 1);
        assert!(matches!(events.last(), Some(RaceEvent::LapCompleted { lap: 1, finished: true, .. })), "{events:?}");
    }

    #[test]
    fn a_reset_starts_a_fresh_race() {
        let mut r = course(3).with_lap_limit(Some(1));
        let (_, t) = fly_laps(&mut r, 1);
        assert!(r.status().finished);
        r.reset();
        assert_eq!(r.status(), RaceStatus::default(), "no clock, laps, best or last time, not finished, gate 0 next");
        // The same course can be raced again from the start.
        r.step(t, 0.1, at(10.0, 40.0, -1.5));
        let (events, _) = fly(&mut r, t + 20.0, 0.0, 100.0);
        assert!(matches!(events[0], RaceEvent::GatePassed { gate: 0, split_s, .. } if split_s == 0.0), "{events:?}");
        assert!(r.status().running && !r.status().finished);
    }

    #[test]
    fn a_reset_mid_race_forgets_the_laps_and_the_gate_order() {
        let mut r = course(3);
        let (_, t) = fly_laps(&mut r, 1);
        let (_, t) = fly(&mut r, t, 0.0, 65.0); // closes lap 2, then gate 1
        assert!(r.status().next_gate == 2 && r.status().laps == 2, "{:?}", r.status());
        r.reset();
        r.step(t, 0.1, at(10.0, 40.0, -1.5));
        // Gate 2 is no longer next: gate 0 is.
        let (events, _) = fly(&mut r, t + 20.0, 50.0, 100.0);
        assert!(events.is_empty(), "gate 1 and 2 first: nothing counts before the start: {events:?}");
    }

    fn tilted(pitch: f64, roll: f64, clearance: f64) -> Gate {
        let g = GateSection { pitch_deg: pitch, roll_deg: roll, ..section("T", 30.0, 0.0) };
        Gate::from_config(&g, clearance)
    }

    #[test]
    fn a_pass_that_does_not_fit_the_quad_is_clipped() {
        // 3 x 2 m opening, a 0.2 m quad: the centre may be at most 1.3 m across and 0.8 m up or down.
        let g = Gate::from_config(&section("G", 30.0, 0.0), 0.2);
        let through = |east: f64, down: f64| g.crossing(at(29.0, east, down), at(31.0, east, down)).map(|c| c.clean);
        assert_eq!(through(0.0, -1.5), Some(true), "centre");
        assert_eq!(through(1.3, -1.5), Some(true), "exactly clear of the post");
        assert_eq!(through(1.4, -1.5), Some(false), "inside the opening but over the post");
        assert_eq!(through(-1.4, -1.5), Some(false), "the other post");
        assert_eq!(through(0.0, -2.4), Some(false), "over the bar");
        assert_eq!(through(0.0, -0.6), Some(false), "over the sill");
        assert_eq!(through(1.4, -2.4), Some(false), "the corner");
        assert_eq!(through(1.6, -1.5), None, "outside the opening: a miss, not a hit");
        let small = Gate::from_config(&GateSection { width_m: 0.3, ..section("S", 30.0, 0.0) }, 0.2);
        assert_eq!(small.crossing(at(29.0, 0.0, -1.5), at(31.0, 0.0, -1.5)).map(|c| c.clean), Some(false), "a quad wider than the opening never fits");
    }

    #[test]
    fn a_clipped_pass_raises_a_hit_and_leaves_the_race_as_it_was() {
        let mut r = RaceTracker::new(vec![Gate::from_config(&section("G0", 30.0, 0.0), 0.2), Gate::from_config(&section("G1", 60.0, 0.0), 0.2)]);
        r.step(0.0, 0.0, at(29.0, 1.4, -1.5));
        assert_eq!(r.step(0.0, 1.0, at(31.0, 1.4, -1.5)), Some(RaceEvent::GateHit { gate: 0, course: 0, time_s: 0.5 }));
        assert_eq!(r.status(), RaceStatus::default(), "the clock did not start");
        // Hovering at the post, back and forth through the plane, raises nothing more.
        for i in 0..6 {
            let north = if i % 2 == 0 { 29.0 } else { 31.0 };
            assert_eq!(r.step(1.0 + f64::from(i), 1.0, at(north, 1.4, -1.5)), None, "jitter {i}");
        }
        // Away and back through the middle: the gate counts.
        r.step(10.0, 1.0, at(20.0, 0.0, -1.5));
        r.step(11.0, 1.0, at(29.0, 0.0, -1.5));
        assert!(matches!(r.step(12.0, 1.0, at(31.0, 0.0, -1.5)), Some(RaceEvent::GatePassed { gate: 0, split_s, .. }) if split_s == 0.0));
        // A clipped gate 1 is a hit and gate 1 stays next.
        r.step(13.0, 1.0, at(59.0, -1.4, -1.5));
        assert!(matches!(r.step(14.0, 1.0, at(61.0, -1.4, -1.5)), Some(RaceEvent::GateHit { gate: 1, .. })));
        assert_eq!((r.status().next_gate, r.status().laps), (1, 0));
    }

    #[test]
    fn a_pitched_gate_is_crossed_on_its_tilted_plane() {
        // Pitched up 45 degrees: the plane leans back, so a level path 0.5 m above the centre reaches it 0.5 m early.
        let g = tilted(45.0, 0.0, 0.0);
        let c = g.crossing(at(28.0, 0.0, -2.0), at(31.0, 0.0, -2.0)).unwrap();
        assert!((c.fraction - 0.5).abs() < 1e-12, "{c:?}");
        // Along the opening's own height axis that is 0.5 * sqrt(2) up, inside; 0.9 m up it is 1.27 m, outside the 1 m
        // half-height, though the upright gate takes it.
        assert!(g.crossing(at(28.0, 0.0, -2.4), at(31.0, 0.0, -2.4)).is_none());
        assert!(tilted(0.0, 0.0, 0.0).crossing(at(28.0, 0.0, -2.4), at(31.0, 0.0, -2.4)).is_some());
        // A path along the gate's normal through its centre is dead centre.
        let n = std::f64::consts::FRAC_1_SQRT_2;
        let on_axis = g.crossing(at(30.0 - n, 0.0, -1.5 + n), at(30.0 + n, 0.0, -1.5 - n)).unwrap();
        assert!((on_axis.fraction - 0.5).abs() < 1e-12 && on_axis.clean);
        // The wrong way along the normal does not cross.
        assert_eq!(g.crossing(at(30.0 + n, 0.0, -1.5 - n), at(30.0 - n, 0.0, -1.5 + n)), None);
    }

    #[test]
    fn a_rolled_gate_swaps_which_edge_is_which() {
        // Rolled a quarter turn the 3 m width is vertical and the 2 m height horizontal.
        let g = tilted(0.0, 90.0, 0.0);
        let through = |east: f64, down: f64| g.crossing(at(29.0, east, down), at(31.0, east, down)).is_some();
        assert!(!through(1.2, -1.5), "1.2 m to the side is outside the now 2 m wide opening");
        assert!(through(0.0, -2.7), "1.2 m up is inside the now 3 m tall one");
        assert!(through(0.9, -1.5));
        let upright = tilted(0.0, 0.0, 0.0);
        assert!(upright.crossing(at(29.0, 1.2, -1.5), at(31.0, 1.2, -1.5)).is_some());
        assert!(upright.crossing(at(29.0, 0.0, -2.7), at(31.0, 0.0, -2.7)).is_none());
    }

    #[test]
    fn a_gates_axes_are_orthonormal_at_any_tilt() {
        for (h, p, r) in [(0.0, 0.0, 0.0), (37.0, 25.0, -60.0), (200.0, -80.0, 170.0), (-90.0, 89.0, 5.0)] {
            let (f, l, d) = GateSection { heading_deg: h, pitch_deg: p, roll_deg: r, ..section("A", 0.0, 0.0) }.axes();
            let (f, l, d) = (DVec3::from_array(f), DVec3::from_array(l), DVec3::from_array(d));
            for v in [f, l, d] {
                assert!((v.length() - 1.0).abs() < 1e-12, "{h} {p} {r}");
            }
            assert!(f.dot(l).abs() < 1e-12 && f.dot(d).abs() < 1e-12 && l.dot(d).abs() < 1e-12, "{h} {p} {r}");
            assert!(f.cross(l).dot(d) > 0.999_999, "right-handed (forward, across, down): {h} {p} {r}");
        }
    }

    #[test]
    fn a_course_with_no_gates_never_raises_anything() {
        let mut r = RaceTracker::new(Vec::new());
        r.step(0.0, 0.0, at(0.0, 0.0, 0.0));
        assert_eq!(r.step(0.0, 1.0, at(100.0, 0.0, 0.0)), None);
        assert_eq!(r.status(), RaceStatus::default());
    }

    #[test]
    fn a_time_attack_race_ends_at_the_deadline_with_the_laps_so_far() {
        // Gates at 30, 60 and 90 m flown at 10 m/s: the start is at 3 s, the second gate at 6 s, the third at 9 s.
        let mut r = course(3).with_time_limit(Some(5.0));
        let (events, _) = fly(&mut r, 0.0, 0.0, 100.0);
        assert_eq!(events.len(), 3, "{events:?}");
        assert!(matches!(events[0], RaceEvent::GatePassed { gate: 0, .. }));
        assert!(matches!(events[1], RaceEvent::GatePassed { gate: 1, .. }));
        let RaceEvent::TimeUp { time_s, laps } = events[2] else { panic!("{:?}", events[2]) };
        assert!((time_s - 8.0).abs() < 1e-6 && laps == 0, "{time_s} {laps}");
        let s = r.status();
        assert!(s.finished && !s.running && s.next_gate == 0, "{s:?}");
        let (more, _) = fly(&mut r, 20.0, 0.0, 100.0);
        assert!(more.is_empty(), "nothing counts after the deadline: {more:?}");
    }

    #[test]
    fn a_crossing_after_the_deadline_inside_the_same_step_does_not_count() {
        let mut r = course(3).with_time_limit(Some(2.95)); // start at 3.0 s, deadline 5.95 s
        let (_, t) = fly(&mut r, 0.0, 0.0, 59.0);
        let e = r.step(t, 0.2, at(61.0, 0.0, -1.5)); // crosses gate 1 at 6.0 s
        assert!(matches!(e, Some(RaceEvent::TimeUp { time_s, laps: 0 }) if (time_s - 5.95).abs() < 1e-6), "{e:?}");
    }

    #[test]
    fn time_attack_counts_laps_before_the_deadline() {
        let mut r = course(3).with_time_limit(Some(100.0)); // a flown lap takes about 29 s here
        let (events, _) = fly_laps(&mut r, 2);
        assert_eq!(events.iter().filter(|e| matches!(e, RaceEvent::LapCompleted { finished: false, .. })).count(), 2, "{events:?}");
        assert!(!r.status().finished && r.status().running, "the clock has not run out");
    }

    #[test]
    fn a_course_is_a_choice_of_gates_and_laps_are_counted_on_it() {
        let mut r = course(3); // gates at 30, 60, 90
        let gates = r.gates.clone();
        r.set_course(1, vec![gates[2].clone(), gates[0].clone()], Some(1), None);
        let (events, t) = fly(&mut r, 0.0, 0.0, 100.0);
        assert_eq!(events.len(), 1, "gates 30 and 60 are not next: {events:?}");
        assert!(matches!(events[0], RaceEvent::GatePassed { gate: 0, course: 1, .. }), "{events:?}");
        r.step(t, 0.1, at(10.0, 40.0, -1.5));
        let (events, _) = fly(&mut r, t + 20.0, 10.0, 100.0);
        assert!(matches!(events[0], RaceEvent::GatePassed { gate: 1, course: 1, .. }), "{events:?}");
        assert!(matches!(events[1], RaceEvent::LapCompleted { lap: 1, finished: true, .. }), "{events:?}");
        assert_eq!((r.gate_count(), r.status().course), (2, 1));
        r.reset();
        assert_eq!((r.status().course, r.status().laps, r.status().running), (1, 0, false), "a reset keeps the course");
    }
}
