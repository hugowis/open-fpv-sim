use ofs_core::{Bus, Model, Scheduler, Signal, SimError, StepCtx};

struct Counter {
    name: String,
    div: u32,
    count: Signal<f64>,
    last_dt: Signal<f64>,
    last_time: Signal<f64>,
}

impl Counter {
    fn new(name: &str, div: u32, bus: &mut Bus) -> Self {
        Self {
            name: name.to_string(),
            div,
            count: bus.signal(&format!("{name}.count")),
            last_dt: bus.signal(&format!("{name}.dt")),
            last_time: bus.signal(&format!("{name}.time")),
        }
    }
}

impl Model for Counter {
    fn name(&self) -> &str {
        &self.name
    }
    fn rate_divisor(&self) -> u32 {
        self.div
    }
    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let c = bus.get(self.count);
        bus.set(self.count, c + 1.0);
        bus.set(self.last_dt, ctx.dt_s);
        bus.set(self.last_time, ctx.time_s);
        Ok(())
    }
}

struct Writer(Signal<f64>);
impl Model for Writer {
    fn name(&self) -> &str {
        "writer"
    }
    fn rate_divisor(&self) -> u32 {
        1
    }
    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        bus.set(self.0, ctx.tick as f64 + 1.0);
        Ok(())
    }
}

struct Reader(Signal<f64>, Signal<f64>);
impl Model for Reader {
    fn name(&self) -> &str {
        "reader"
    }
    fn rate_divisor(&self) -> u32 {
        1
    }
    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let v = bus.get(self.0);
        bus.set(self.1, v);
        Ok(())
    }
}

struct NanAt(u64, Signal<f64>);
impl Model for NanAt {
    fn name(&self) -> &str {
        "nan"
    }
    fn rate_divisor(&self) -> u32 {
        1
    }
    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        if ctx.tick == self.0 {
            bus.set(self.1, f64::NAN);
        }
        Ok(())
    }
}

fn value(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

#[test]
fn runs_models_at_their_divisors() {
    let mut bus = Bus::new();
    let fast = Counter::new("fast", 1, &mut bus);
    let slow = Counter::new("slow", 4, &mut bus);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(fast));
    s.add(Box::new(slow));
    for _ in 0..8 {
        s.step().unwrap();
    }
    assert_eq!(value(&s, "fast.count"), 8.0);
    assert_eq!(value(&s, "slow.count"), 2.0);
    assert_eq!(value(&s, "slow.dt"), 4.0 / 8000.0);
    assert_eq!(value(&s, "slow.time"), 4.0 / 8000.0);
    assert_eq!(s.tick(), 8);
    assert_eq!(s.time_s(), 0.001);
}

#[test]
fn models_run_in_registration_order_within_a_tick() {
    let mut bus = Bus::new();
    let w = bus.signal("w");
    let r = bus.signal("r");
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(Writer(w)));
    s.add(Box::new(Reader(w, r)));
    s.step().unwrap();
    assert_eq!(value(&s, "r"), 1.0);
}

#[test]
fn non_finite_value_stops_the_run_without_advancing() {
    let mut bus = Bus::new();
    let bad = bus.signal("bad");
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(NanAt(3, bad)));
    for _ in 0..3 {
        s.step().unwrap();
    }
    assert_eq!(s.step(), Err(SimError::NonFinite("bad".into())));
    assert_eq!(s.tick(), 3);
}

#[test]
fn run_for_steps_whole_ticks_and_rejects_bad_durations() {
    let mut s = Scheduler::new(8000, Bus::new());
    s.run_for(0.5).unwrap();
    assert_eq!(s.tick(), 4000);
    assert!(matches!(s.run_for(-1.0), Err(SimError::InvalidArgument(_))));
    assert!(matches!(s.run_for(f64::NAN), Err(SimError::InvalidArgument(_))));
    assert_eq!(s.tick(), 4000);
}

#[test]
#[should_panic(expected = "model name 'imu' is already registered")]
fn two_models_cannot_share_a_name() {
    // Model RNG streams are seeded from the model name: a duplicate would make two models draw the same noise.
    let mut bus = Bus::new();
    let a = Counter::new("imu", 1, &mut bus);
    let b = Counter::new("imu", 2, &mut bus);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(a));
    s.add(Box::new(b));
}

#[test]
fn run_for_refuses_durations_longer_than_a_simulated_day() {
    let mut s = Scheduler::new(8000, Bus::new());
    let err = s.run_for(86_400.0 + 1.0).unwrap_err();
    assert!(matches!(&err, SimError::InvalidArgument(m) if m.contains("86400")), "{err}");
    assert!(matches!(s.run_for(1e300), Err(SimError::InvalidArgument(_))));
    assert_eq!(s.tick(), 0);
}
