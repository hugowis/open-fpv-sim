use ofs_sim::pacer::{OverrunPolicy, Pacer, Plan, MAX_SLEEP_S, YIELD_S};

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
