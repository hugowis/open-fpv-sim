use ofs_sim::pacer::{OverrunPolicy, Pacer, Plan, MAX_SLEEP_S, MIN_BATCH_S, YIELD_S};

const HZ: u32 = 8000;

fn pacer(policy: OverrunPolicy) -> Pacer {
    let mut p = Pacer::new(policy, HZ);
    p.restart(100.0, 0.0); // wall clock at 100 s, simulation at 0 s
    p
}

#[test]
fn on_schedule_it_runs_the_elapsed_ticks() {
    let mut p = pacer(OverrunPolicy::Warn);
    assert_eq!(p.plan(100.0101, 0.0), Plan { ticks: 80, sleep_s: YIELD_S, overrun: false });
}

#[test]
fn ahead_of_the_wall_clock_it_sleeps() {
    let mut p = pacer(OverrunPolicy::Warn);
    let plan = p.plan(100.010, 0.0105);
    assert_eq!(plan.ticks, 0);
    assert!(plan.sleep_s >= YIELD_S && plan.sleep_s <= MAX_SLEEP_S, "{plan:?}");
}

#[test]
fn warn_catches_up_in_chunks_within_the_allowed_lag() {
    let mut p = pacer(OverrunPolicy::Warn);
    assert_eq!(p.plan(100.0801, 0.0), Plan { ticks: 400, sleep_s: YIELD_S, overrun: false });
    assert_eq!(p.overruns(), 0);
}

#[test]
fn warn_drops_a_backlog_beyond_the_allowed_lag_and_counts_an_overrun() {
    let mut p = pacer(OverrunPolicy::Warn);
    let plan = p.plan(100.5, 0.0);
    assert!(plan.overrun);
    assert_eq!(plan.ticks, 400);
    assert_eq!(p.overruns(), 1);
    // The backlog is now 0.1 s: catching up continues without new overruns.
    let plan = p.plan(100.5, 0.05);
    assert_eq!((plan.ticks, plan.overrun), (400, false));
    assert_eq!(p.overruns(), 1);
}

#[test]
fn slow_never_bursts_and_counts_an_overrun_per_100_ms_of_stretch() {
    let mut p = pacer(OverrunPolicy::Slow);
    let mut sim = 0.0;
    let mut now = 100.0;
    let mut overruns = Vec::new();
    for _ in 0..5 {
        now += 0.08; // the simulation only manages 0.05 s per 0.08 s of wall time
        let plan = p.plan(now, sim);
        assert_eq!(plan.ticks, 400, "at most one chunk");
        sim += plan.ticks as f64 / f64::from(HZ);
        overruns.push(plan.overrun);
    }
    // 0.03 s of stretch per step: the 4th step crosses 0.1 s.
    assert_eq!(overruns, vec![false, false, false, true, false]);
    assert_eq!(p.overruns(), 1);
}

#[test]
fn restart_re_anchors_after_a_pause() {
    let mut p = pacer(OverrunPolicy::Warn);
    p.restart(500.0, 0.5); // paused for minutes
    let plan = p.plan(500.0011, 0.5);
    assert_eq!((plan.ticks, plan.overrun), (8, false));
}

#[test]
fn slow_counts_one_overrun_per_max_lag_of_large_stall() {
    // After a large stall, Slow should count multiple overruns.
    // excess = 10.0 - CHUNK_S = 10.0 - 0.05 = 9.95
    // Expected overruns = floor(9.95 / MAX_LAG_S) = floor(9.95 / 0.1) = 99
    let mut p = Pacer::new(OverrunPolicy::Slow, HZ);
    p.restart(0.0, 0.0);
    let plan = p.plan(10.0, 0.0);
    // The plan should run one chunk and report overrun=true
    assert_eq!(plan.ticks, 400);
    assert_eq!(plan.overrun, true);
    // Should have counted 99 overruns from the excess
    assert_eq!(p.overruns(), 99);
    // Following in-time plan adds no more overruns
    let sim_advance = 400.0 / f64::from(HZ); // 0.05 s
    let plan2 = p.plan(10.0 + sim_advance, sim_advance);
    assert_eq!(plan2.overrun, false);
    assert_eq!(p.overruns(), 99);
}

#[test]
fn a_slow_base_rate_still_runs_at_least_one_tick_per_step() {
    // 0.05 s chunks hold no whole tick below 20 Hz; the simulation must not stall.
    let mut p = Pacer::new(OverrunPolicy::Warn, 5);
    p.restart(0.0, 0.0);
    assert_eq!(p.plan(0.5, 0.0).ticks, 1);
}

#[test]
fn a_non_finite_clock_runs_nothing() {
    let mut p = pacer(OverrunPolicy::Warn);
    for (now, sim) in [(f64::NAN, 0.0), (f64::INFINITY, 0.0), (100.5, f64::NAN)] {
        let plan = p.plan(now, sim);
        assert_eq!((plan.ticks, plan.overrun), (0, false), "{now} {sim}");
        assert!(plan.sleep_s.is_finite() && plan.sleep_s > 0.0, "{plan:?}");
    }
    assert_eq!(p.overruns(), 0);
    assert_eq!(p.plan(100.0101, 0.0).ticks, 80, "the pacer is unharmed");
}

#[test]
fn a_clock_that_goes_backwards_waits() {
    let mut p = pacer(OverrunPolicy::Warn);
    let plan = p.plan(99.0, 0.0);
    assert_eq!((plan.ticks, plan.overrun), (0, false));
    assert!(plan.sleep_s <= MAX_SLEEP_S, "{plan:?}");
}

#[test]
fn a_backlog_of_exactly_one_batch_runs() {
    let mut p = pacer(OverrunPolicy::Warn);
    assert_eq!(p.plan(100.0 + MIN_BATCH_S, 0.0).ticks, 8);
    let mut p = pacer(OverrunPolicy::Warn);
    assert_eq!(p.plan(100.0 + MIN_BATCH_S * 0.99, 0.0).ticks, 0);
}

#[test]
fn restart_forgets_the_stretch_of_a_slow_session() {
    let mut p = pacer(OverrunPolicy::Slow);
    p.plan(100.13, 0.0); // 0.08 s of stretch, below one overrun
    p.restart(200.0, 0.05);
    p.plan(200.13, 0.05); // another 0.08 s: still none, had the stretch been kept it would be one
    assert_eq!(p.overruns(), 0);
}
