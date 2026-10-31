use ofs_audio::{Audio, AudioInput, SAMPLE_RATE};

const RATE: f64 = SAMPLE_RATE as f64;

fn frames(seconds: f64) -> usize {
    (seconds * RATE) as usize
}

fn rms(s: &[f32]) -> f64 {
    (s.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / s.len() as f64).sqrt()
}

fn spinning(active: bool) -> AudioInput {
    AudioInput {
        motor_rpm: vec![20000.0; 4],
        motor_cmd: vec![0.6; 4],
        airspeed_mps: 10.0,
        descent_mps: 1.0,
        active,
    }
}

#[test]
fn a_collision_waits_for_the_state_that_carries_it() {
    let mut a = Audio::new();
    let idle = AudioInput::default();
    a.frame(&idle, 4.0, 0.0, frames(0.1));
    a.collision_event(5.0);
    let early = a.frame(&idle, 4.9, 0.0, frames(0.1));
    assert!(early.iter().all(|s| s.abs() < 1e-6));

    let hit = a.frame(&idle, 5.0, 8.0, frames(0.1));
    assert!(rms(&hit[..frames(0.05)]) > 0.02);

    let later = a.frame(&idle, 5.1, 8.0, frames(0.5));
    let tail = a.frame(&idle, 5.2, 8.0, frames(0.1));
    assert!(rms(&tail) < 0.002 && later.len() == frames(0.5));
}

#[test]
fn reset_drops_a_pending_collision() {
    let mut a = Audio::new();
    let idle = AudioInput::default();
    a.collision_event(1.0);
    a.reset();
    let out = a.frame(&idle, 2.0, 8.0, frames(0.1));
    assert!(out.iter().all(|&s| s == 0.0));
}

#[test]
fn frame_is_deterministic() {
    let run = || {
        let mut a = Audio::new();
        let mut all = Vec::new();
        a.collision_event(0.2);
        for (i, n) in [441, 735, 1024, 500].into_iter().enumerate() {
            let mut input = spinning(true);
            input.motor_rpm[0] += 100.0 * i as f64;
            all.extend(a.frame(&input, 0.1 * i as f64, if i == 2 { 6.0 } else { 0.0 }, n));
        }
        all
    };
    let (x, y) = (run(), run());
    assert_eq!(x.len(), y.len());
    assert!(x.iter().zip(&y).all(|(p, q)| p.to_bits() == q.to_bits()));
}

#[test]
fn inactive_input_fades_to_silence() {
    let mut a = Audio::new();
    let on = a.frame(&spinning(true), 0.0, 0.0, frames(0.3));
    assert!(rms(&on) > 0.01);
    let off = a.frame(&spinning(false), 0.3, 0.0, frames(0.2));
    assert!(off[off.len() - frames(0.05)..].iter().all(|&s| s == 0.0));
}

#[test]
fn pausing_keeps_the_motors_so_the_tones_fade_without_a_click() {
    let mut a = Audio::new();
    let on = a.frame(&spinning(true), 0.0, 0.0, frames(0.3));
    let off = a.frame(&spinning(false), f64::NAN, 0.0, frames(0.3));
    let max_jump = |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    // The tone's own slope is the bound: the seam may not be steeper than the steady sound before it.
    let steady = max_jump(&on[on.len() - frames(0.1)..]);
    let seam = [&on[on.len() - 1..], &off[..]].concat();
    assert!(max_jump(&seam) <= steady * 1.05, "no click across the pause: {} vs {steady}", max_jump(&seam));
    // And it is a fade: the tone is still there a few milliseconds in (a cut would be silent at once).
    assert!(rms(&off[frames(0.002)..frames(0.012)]) > 0.5 * rms(&on[on.len() - frames(0.01)..]));
    assert!(off[off.len() - frames(0.05)..].iter().all(|&s| s == 0.0));
}

#[test]
fn a_collision_far_ahead_of_its_state_is_dropped() {
    let mut a = Audio::new();
    let idle = AudioInput::default();
    a.collision_event(100.0);
    let early = a.frame(&idle, 5.0, 8.0, frames(0.1));
    assert!(early.iter().all(|&s| s == 0.0));
    let later = a.frame(&idle, 100.0, 8.0, frames(0.1));
    assert!(later.iter().all(|&s| s == 0.0), "the stale event never plays");
}
