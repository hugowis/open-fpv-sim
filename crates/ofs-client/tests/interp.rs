use glam::{DQuat, DVec3};
use ofs_client::interp::{Sample, StateBuffer};

const RATE_HZ: f64 = 240.0;
const LATENCY_S: f64 = 0.010;
/// Arrival jitter pattern (seconds): includes 0, so the smallest delay seen is exactly `LATENCY_S`.
const JITTER_S: [f64; 7] = [0.0, 0.0013, 0.0004, 0.0021, 0.0, 0.0009, 0.0017];
const YAW_RATE: f64 = std::f64::consts::FRAC_PI_2; // rad/s about +Y

fn sample(k: usize) -> Sample {
    let t = k as f64 / RATE_HZ;
    Sample { sim_time_s: t, pos: DVec3::new(10.0 * t, 0.0, 0.0), att: DQuat::from_rotation_y(YAW_RATE * t) }
}

fn arrival(k: usize) -> f64 {
    k as f64 / RATE_HZ + LATENCY_S + JITTER_S[k % JITTER_S.len()]
}

fn filled(delay_s: f64, count: usize) -> StateBuffer {
    let mut b = StateBuffer::new(delay_s);
    for k in 0..count {
        b.push(sample(k), arrival(k), true);
    }
    b
}

#[test]
fn poses_follow_the_simulation_smoothly_despite_arrival_jitter() {
    let delay = 1.0 / RATE_HZ + 0.002;
    let b = filled(delay, 120);
    let latest = 119.0 / RATE_HZ;
    // The display asks at its own rate (144 Hz) over the last 40 ms the buffer can answer.
    let mut now = LATENCY_S + delay + latest - 0.040;
    while now < LATENCY_S + delay + latest {
        let want = now - LATENCY_S - delay;
        let pose = b.pose_at(now).unwrap();
        assert!((pose.pos.x - 10.0 * want).abs() < 1e-9, "x {} vs {}", pose.pos.x, 10.0 * want);
        let yaw_q = DQuat::from_rotation_y(YAW_RATE * want);
        assert!(pose.att.angle_between(yaw_q) < 1e-6, "attitude lags or jumps at now = {now}");
        now += 1.0 / 144.0;
    }
}

#[test]
fn a_stalled_stream_holds_the_newest_pose() {
    let b = filled(0.006, 50);
    let newest = sample(49);
    let pose = b.pose_at(arrival(49) + 5.0).unwrap();
    assert_eq!(pose.pos, newest.pos);
    assert!(pose.att.angle_between(newest.att) < 1e-12);
}

#[test]
fn the_oldest_pose_is_held_before_the_buffer_reaches_back_that_far() {
    let b = filled(0.006, 3);
    assert_eq!(b.pose_at(-100.0).unwrap().pos, sample(0).pos);
}

#[test]
fn there_is_no_pose_before_the_first_sample() {
    assert!(StateBuffer::new(0.006).pose_at(1.0).is_none());
}

#[test]
fn a_restarted_session_drops_the_old_samples() {
    let mut b = filled(0.006, 50);
    let fresh = Sample { sim_time_s: 0.0, pos: DVec3::new(0.0, 5.0, 0.0), att: DQuat::IDENTITY };
    b.push(fresh, arrival(49) + 3.0, true);
    assert_eq!(b.pose_at(arrival(49) + 3.0).unwrap().pos, fresh.pos);
    assert_eq!(b.pose_at(0.0).unwrap().pos, fresh.pos, "no trace of the previous session remains");
}

#[test]
fn a_paused_session_holds_its_pose_and_relearns_the_clock_when_it_runs_again() {
    let delay = 0.006;
    let mut b = filled(delay, 50);
    // Paused: the server keeps sending the same state; simulated time stands still.
    for i in 0..10 {
        b.push(sample(49), arrival(49) + 0.1 + i as f64 * 0.004, false);
    }
    assert_eq!(b.pose_at(arrival(49) + 100.0).unwrap().pos, sample(49).pos);
    // Running again a second later. The new clock offset is the arrival delay of sample 50 onwards.
    let resume_offset = 1.0 + LATENCY_S;
    for k in 50..80 {
        b.push(sample(k), k as f64 / RATE_HZ + resume_offset + JITTER_S[k % JITTER_S.len()], true);
    }
    let now = 70.0 / RATE_HZ + resume_offset + delay;
    let pose = b.pose_at(now).unwrap();
    assert!((pose.pos.x - 10.0 * (70.0 / RATE_HZ)).abs() < 1e-9, "poses follow the new clock: {}", pose.pos.x);
}

#[test]
fn non_finite_samples_are_ignored() {
    let mut b = filled(0.006, 10);
    let bad = Sample { sim_time_s: 1.0, pos: DVec3::new(f64::NAN, 0.0, 0.0), att: DQuat::IDENTITY };
    b.push(bad, 1.0, true);
    let bad = Sample { sim_time_s: 1.0, pos: DVec3::ZERO, att: DQuat::from_xyzw(0.0, 0.0, 0.0, 0.0) };
    b.push(bad, 1.0, true);
    let bad = Sample { sim_time_s: f64::INFINITY, pos: DVec3::ZERO, att: DQuat::IDENTITY };
    b.push(bad, 1.0, true);
    assert_eq!(b.pose_at(5.0).unwrap().pos, sample(9).pos, "the newest good sample is still the newest");
}

#[test]
fn attitudes_are_renormalised() {
    let mut b = StateBuffer::new(0.006);
    b.push(Sample { sim_time_s: 0.0, pos: DVec3::ZERO, att: DQuat::from_xyzw(0.0, 0.0, 0.0, 2.0) }, 0.0, true);
    assert!((b.pose_at(1.0).unwrap().att.length() - 1.0).abs() < 1e-12);
}