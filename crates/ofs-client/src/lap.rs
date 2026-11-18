//! The session's fastest clean lap, captured from the state stream.
//!
//! The server's `best_lap_s` is the fastest lap, dirty or not; the ghost must be a lap the pilot can actually
//! race, so this module keeps its own `best_clean_s`. A lap completes when the state's `laps` rises; clean means
//! no collision event time lies inside its window. The capture is held for one more state so a collision event
//! that arrives after the completing state can still veto it.
use std::collections::VecDeque;

use crate::interp::Sample;
use crate::model::RaceInfo;

/// The stored track's sample rate; the state stream is up to 240 Hz.
pub const STORE_HZ: f64 = 60.0;
/// How many collision times the worker keeps for the veto.
pub const COLLISION_MEMORY: usize = 8;

/// A clean lap's poses, in Godot's frame, with times normalised to the first stored pose.
#[derive(Debug, Clone, PartialEq)]
pub struct LapTrack {
    pub world: String,
    pub course: String,
    pub lap_s: f64,
    pub times: Vec<f64>,
    pub positions: Vec<[f64; 3]>,
    pub rotations: Vec<[f64; 4]>,
}

struct Pending {
    track: LapTrack,
    end_s: f64,
    previous_best: Option<f64>,
}

/// The capture state: the current lap's poses and the best clean lap of the session.
#[derive(Default)]
pub struct LapCapture {
    poses: VecDeque<Sample>,
    prev_laps: u32,
    prev_course: String,
    best_clean_s: Option<f64>,
    pending: Option<Pending>,
}

impl LapCapture {
    /// Feeds one state. `collisions` are the recent collision event times (the worker's memory).
    ///
    /// A new capture is held until the next call; the call that completes the lap returns `None` (or a previous
    /// held capture), and the next call returns it unless a late collision vetoes it.
    pub fn ingest(&mut self, world: &str, sample: Sample, race: &RaceInfo, collisions: &[f64]) -> Option<LapTrack> {
        // A reset: no race, back to the start, a course switch, or the lap count dropping. A faster lap lowers
        // the server's `best_lap_s` too, so the best alone is not a reset.
        let reset = !race.present
            || (!race.running && race.laps == 0 && !race.finished)
            || race.laps < self.prev_laps
            || race.course != self.prev_course;
        if reset {
            self.clear();
            self.prev_laps = race.laps;
            self.prev_course = race.course.clone();
            return None;
        }

        // The capture held from the previous state: a collision that arrived since then vetoes it.
        let mut result = None;
        if let Some(pending) = self.pending.take() {
            let window_start = pending.end_s - pending.track.lap_s;
            if collisions.iter().any(|&t| t >= window_start && t <= pending.end_s) {
                self.best_clean_s = pending.previous_best;
            } else {
                result = Some(pending.track);
            }
        }

        // A completed lap: the state's laps rose. The frame is the crossing, so its pose is not in the buffer yet.
        if race.laps > self.prev_laps && race.last_lap_s > 0.0 {
            let lap_s = race.last_lap_s;
            let window_start = sample.sim_time_s - lap_s;
            let clean = !collisions.iter().any(|&t| t >= window_start && t <= sample.sim_time_s);
            let faster = self.best_clean_s.is_none_or(|best| lap_s < best);
            if clean && faster {
                if let Some(track) = self.track(world, race, lap_s, window_start) {
                    let previous_best = self.best_clean_s;
                    self.best_clean_s = Some(lap_s);
                    self.pending = Some(Pending { track, end_s: sample.sim_time_s, previous_best });
                }
            }
        }

        // Buffer the current pose while the race runs. The completing state already runs the next lap's clock,
        // so the trim follows the detection; the lap's own window is taken from the poses before this state.
        if race.running {
            self.poses.push_back(sample);
            let keep_from = sample.sim_time_s - race.current_lap_s;
            while self.poses.front().is_some_and(|p| p.sim_time_s < keep_from) {
                self.poses.pop_front();
            }
        }

        self.prev_laps = race.laps;
        self.prev_course = race.course.clone();
        result
    }

    /// The stored, decimated track for a lap window, or `None` when the stream had no pose inside it.
    fn track(&self, world: &str, race: &RaceInfo, lap_s: f64, window_start: f64) -> Option<LapTrack> {
        let selected: Vec<&Sample> = self
            .poses
            .iter()
            .filter(|p| p.sim_time_s >= window_start && p.sim_time_s <= window_start + lap_s)
            .collect();
        if selected.is_empty() {
            return None;
        }
        let first = selected[0].sim_time_s;
        let target = (STORE_HZ * lap_s).max(1.0);
        let step = ((selected.len() as f64 / target).ceil() as usize).max(1);
        let last = selected.len() - 1;
        let chosen: Vec<&Sample> = selected
            .iter()
            .enumerate()
            .filter(|(i, _)| i % step == 0 || *i == last)
            .map(|(_, sample)| *sample)
            .collect();
        Some(LapTrack {
            world: world.to_string(),
            course: race.course.clone(),
            lap_s,
            times: chosen.iter().map(|p| p.sim_time_s - first).collect(),
            positions: chosen.iter().map(|p| [p.pos.x, p.pos.y, p.pos.z]).collect(),
            rotations: chosen.iter().map(|p| [p.att.x, p.att.y, p.att.z, p.att.w]).collect(),
        })
    }

    fn clear(&mut self) {
        self.poses.clear();
        self.best_clean_s = None;
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RaceInfo;
    use glam::{DQuat, DVec3};

    const DT: f64 = 1.0 / 240.0;
    const WORLD: &str = "Flat field";

    fn state(t: f64, x: f64) -> Sample {
        Sample { sim_time_s: t, pos: DVec3::new(x, 0.0, -5.0), att: DQuat::IDENTITY }
    }

    fn race(laps: u32, running: bool, current: f64, last: f64, best: f64) -> RaceInfo {
        RaceInfo {
            present: true,
            course: "Diamond".into(),
            running,
            laps,
            current_lap_s: current,
            last_lap_s: last,
            best_lap_s: best,
            ..RaceInfo::default()
        }
    }

    fn feed(capture: &mut LapCapture, from: f64, to: f64) {
        let steps = ((to - from) / DT).round() as usize;
        for i in 0..=steps {
            let t = from + i as f64 * DT;
            capture.ingest(WORLD, state(t, t), &race(0, true, t, 0.0, 0.0), &[]);
        }
    }

    #[test]
    fn states_without_a_running_race_capture_nothing() {
        let mut capture = LapCapture::default();
        let world = WORLD;
        let mut absent = race(0, false, 0.0, 0.0, 0.0);
        absent.present = false;
        assert!(capture.ingest(world, state(0.0, 0.0), &absent, &[]).is_none());
        assert!(capture.ingest(world, state(DT, 1.0), &race(0, false, 0.0, 0.0, 0.0), &[]).is_none());
        assert!(capture.poses.is_empty());
    }

    #[test]
    fn a_first_lap_is_captured_as_the_fastest_clean_lap() {
        let mut capture = LapCapture::default();
        feed(&mut capture, 0.0, 10.0 - DT);
        assert!(capture.ingest(WORLD, state(10.0, 10.0), &race(1, true, 0.0, 10.0, 10.0), &[]).is_none(), "held one state");
        let track = capture.ingest(WORLD, state(10.0 + DT, 10.1), &race(1, true, DT, 10.0, 10.0), &[]).expect("the held lap");
        assert_eq!(track.world, WORLD);
        assert_eq!(track.course, "Diamond");
        assert_eq!(track.lap_s, 10.0);
        assert_eq!(track.times[0], 0.0);
        assert!(track.times.last().is_some_and(|t| *t <= 10.0 + 1e-9));
    }

    #[test]
    fn a_dirty_session_best_does_not_block_a_slower_clean_lap() {
        let mut capture = LapCapture::default();
        feed(&mut capture, 0.0, 10.0 - DT);
        let dirty = race(1, true, 0.0, 10.0, 10.0);
        assert!(capture.ingest(WORLD, state(10.0, 10.0), &dirty, &[5.0]).is_none());
        assert!(capture.ingest(WORLD, state(10.0 + DT, 10.1), &race(1, true, DT, 10.0, 10.0), &[5.0]).is_none());
        // Lap 2 is slower than the dirty session best (10 s), but it is the fastest clean lap.
        for i in 2..=(11.0 / DT) as usize {
            let t = 10.0 + i as f64 * DT;
            if t >= 21.0 {
                break;
            }
            capture.ingest(WORLD, state(t, t), &race(1, true, t - 10.0, 0.0, 10.0), &[]);
        }
        assert!(capture.ingest(WORLD, state(21.0, 21.0), &race(2, true, 0.0, 11.0, 10.0), &[]).is_none());
        let track = capture.ingest(WORLD, state(21.0 + DT, 21.1), &race(2, true, DT, 11.0, 10.0), &[]).expect("the clean lap");
        assert_eq!(track.lap_s, 11.0);
    }

    #[test]
    fn the_fastest_clean_lap_replaces_the_previous() {
        let mut capture = LapCapture::default();
        feed(&mut capture, 0.0, 10.0 - DT);
        assert!(capture.ingest(WORLD, state(10.0, 10.0), &race(1, true, 0.0, 10.0, 10.0), &[]).is_none());
        assert!(capture.ingest(WORLD, state(10.0 + DT, 10.1), &race(1, true, DT, 10.0, 10.0), &[]).is_some());

        for i in 2..=(9.0 / DT) as usize {
            let t = 10.0 + i as f64 * DT;
            if t >= 19.0 {
                break;
            }
            capture.ingest(WORLD, state(t, t), &race(1, true, t - 10.0, 0.0, 10.0), &[]);
        }
        assert!(capture.ingest(WORLD, state(19.0, 19.0), &race(2, true, 0.0, 9.0, 9.0), &[]).is_none());
        let track = capture.ingest(WORLD, state(19.0 + DT, 19.1), &race(2, true, DT, 9.0, 9.0), &[]).expect("faster");
        assert_eq!(track.lap_s, 9.0);

        // A slower clean lap now leaves the best alone.
        for i in 2..=(9.5 / DT) as usize {
            let t = 19.0 + i as f64 * DT;
            if t >= 28.5 {
                break;
            }
            capture.ingest(WORLD, state(t, t), &race(2, true, t - 19.0, 0.0, 9.0), &[]);
        }
        assert!(capture.ingest(WORLD, state(28.5, 28.5), &race(3, true, 0.0, 9.5, 9.0), &[]).is_none());
        assert!(capture.ingest(WORLD, state(28.5 + DT, 28.6), &race(3, true, DT, 9.5, 9.0), &[]).is_none());
    }

    #[test]
    fn the_track_spans_the_lap_window_decimated_to_60_hz() {
        let mut capture = LapCapture::default();
        feed(&mut capture, 0.0, 2.0 - DT);
        capture.ingest(WORLD, state(2.0, 2.0), &race(1, true, 0.0, 2.0, 2.0), &[]);
        let track = capture.ingest(WORLD, state(2.0 + DT, 2.1), &race(1, true, DT, 2.0, 2.0), &[]).unwrap();
        assert!(track.times.len() <= 2 * 60 + 2, "{} samples", track.times.len());
        assert_eq!(track.times[0], 0.0);
        assert_eq!(track.positions.len(), track.times.len());
        assert_eq!(track.rotations.len(), track.times.len());
        assert!(track.positions[1][0] > track.positions[0][0], "the fed poses are kept in order");
    }

    #[test]
    fn an_uneven_state_stream_still_yields_the_right_window() {
        let mut capture = LapCapture::default();
        for i in 0..=(10.0 / DT) as usize {
            if i % 7 == 0 {
                continue; // a dropped state
            }
            let t = i as f64 * DT;
            if i == (10.0 / DT) as usize {
                break;
            }
            capture.ingest(WORLD, state(t, t), &race(0, true, t, 0.0, 0.0), &[]);
        }
        capture.ingest(WORLD, state(10.0, 10.0), &race(1, true, 0.0, 10.0, 10.0), &[]);
        let track = capture.ingest(WORLD, state(10.0 + DT, 10.1), &race(1, true, DT, 10.0, 10.0), &[]).unwrap();
        assert_eq!(track.times[0], 0.0);
        assert!(track.times.last().is_some_and(|t| *t <= 10.0));
        assert!(track.positions.iter().all(|p| p[0] >= 0.0 && p[0] <= 10.0));
    }

    #[test]
    fn a_collision_inside_the_window_vetoes_the_capture() {
        let mut capture = LapCapture::default();
        feed(&mut capture, 0.0, 10.0 - DT);
        capture.ingest(WORLD, state(10.0, 10.0), &race(1, true, 0.0, 10.0, 10.0), &[5.0]);
        assert!(capture.ingest(WORLD, state(10.0 + DT, 10.1), &race(1, true, DT, 10.0, 10.0), &[5.0]).is_none());
        assert_eq!(capture.best_clean_s, None);
    }

    #[test]
    fn a_late_collision_vetoes_the_held_capture() {
        let mut capture = LapCapture::default();
        feed(&mut capture, 0.0, 10.0 - DT);
        assert!(capture.ingest(WORLD, state(10.0, 10.0), &race(1, true, 0.0, 10.0, 10.0), &[]).is_none());
        assert!(capture.ingest(WORLD, state(10.0 + DT, 10.1), &race(1, true, DT, 10.0, 10.0), &[9.999]).is_none());
        assert_eq!(capture.best_clean_s, None);
    }

    #[test]
    fn a_collision_outside_the_window_does_not_veto() {
        let mut capture = LapCapture::default();
        feed(&mut capture, 0.0, 10.0 - DT);
        capture.ingest(WORLD, state(10.0, 10.0), &race(1, true, 0.0, 10.0, 10.0), &[-1.0, 10.1]);
        assert!(capture.ingest(WORLD, state(10.0 + DT, 10.1), &race(1, true, DT, 10.0, 10.0), &[-1.0, 10.1]).is_some());
    }

    #[test]
    fn a_reset_before_the_first_lap_clears_the_window() {
        let mut capture = LapCapture::default();
        feed(&mut capture, 0.0, 5.0 - DT);
        // A reset after the clock started but before any lap: the state goes back to laps 0, not running.
        capture.ingest(WORLD, state(5.0, 5.0), &race(0, false, 0.0, 0.0, 0.0), &[]);
        assert!(capture.poses.is_empty());
        for i in 0..=(3.0 / DT) as usize {
            let t = 6.0 + i as f64 * DT;
            capture.ingest(WORLD, state(t, 100.0 + t), &race(0, true, t - 6.0, 0.0, 0.0), &[]);
        }
        capture.ingest(WORLD, state(9.0, 109.0), &race(1, true, 0.0, 3.0, 3.0), &[]);
        let track = capture.ingest(WORLD, state(9.0 + DT, 109.1), &race(1, true, DT, 3.0, 3.0), &[]).unwrap();
        assert!(track.positions.iter().all(|p| p[0] >= 100.0), "no pose from before the reset survives");
    }

    #[test]
    fn a_reset_clears_the_window_and_the_best_reference() {
        let mut capture = LapCapture::default();
        feed(&mut capture, 0.0, 10.0 - DT);
        capture.ingest(WORLD, state(10.0, 10.0), &race(1, true, 0.0, 10.0, 10.0), &[]);
        assert!(capture.ingest(WORLD, state(10.0 + DT, 10.1), &race(1, true, DT, 10.0, 10.0), &[]).is_some());
        capture.ingest(WORLD, state(10.0 + 2.0 * DT, 10.2), &race(0, false, 0.0, 0.0, 0.0), &[]);
        assert_eq!(capture.best_clean_s, None);
        assert!(capture.poses.is_empty());
    }

    #[test]
    fn a_world_name_travels_with_the_track() {
        let mut capture = LapCapture::default();
        feed(&mut capture, 0.0, 10.0 - DT);
        capture.ingest(WORLD, state(10.0, 10.0), &race(1, true, 0.0, 10.0, 10.0), &[]);
        let track = capture.ingest(WORLD, state(10.0 + DT, 10.1), &race(1, true, DT, 10.0, 10.0), &[]).unwrap();
        assert_eq!(track.world, WORLD);
    }

    #[test]
    fn a_lap_with_no_pose_in_its_window_is_not_captured() {
        let mut capture = LapCapture::default();
        assert!(capture.ingest(WORLD, state(10.0, 10.0), &race(1, true, 0.0, 10.0, 10.0), &[]).is_none());
        assert!(capture.ingest(WORLD, state(10.0 + DT, 10.1), &race(1, true, DT, 10.0, 10.0), &[]).is_none());
        assert_eq!(capture.best_clean_s, None);
    }
}
