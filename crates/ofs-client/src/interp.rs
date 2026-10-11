//! Smooth vehicle poses from a jittery state stream.
//!
//! The server sends states at a fixed rate with their simulated time; they reach the client with irregular
//! delays, and the display runs at its own rate. `StateBuffer` keeps the recent samples and answers "where was
//! the vehicle at this moment on the client's clock": it learns the offset between the two clocks (the smallest
//! arrival delay seen lately, since delays only add) and renders a fixed `delay_s` in the past, so the answer is
//! interpolated between two real samples instead of jumping from sample to sample.
use std::collections::VecDeque;

use glam::{DQuat, DVec3};

/// How far back poses are kept: the 1000 ms `latency_ms` validation cap (`ofs-config`) + display delay + margin.
/// The composite samples the pose and the picture at one instant, and `FpvBuffer` keeps 1.28 s of picture, so the
/// pose history must reach at least the latency cap back or the two would disagree about "when".
const POSE_SPAN_S: f64 = 1.2;
/// A backstop on top of the time-based span (about 2.1 s of samples at the default 240 Hz state rate).
const MAX_SAMPLES: usize = 512;
/// How many recent arrival offsets the clock estimate looks at (a second at 240 Hz).
const OFFSET_WINDOW: usize = 240;

/// A vehicle pose in Godot's frame at a simulated time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub sim_time_s: f64,
    pub pos: DVec3,
    pub att: DQuat,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub pos: DVec3,
    pub att: DQuat,
}

impl From<Sample> for Pose {
    fn from(s: Sample) -> Pose {
        Pose { pos: s.pos, att: s.att }
    }
}

#[derive(Debug, Clone)]
pub struct StateBuffer {
    samples: VecDeque<Sample>,
    offsets: VecDeque<f64>,
    delay_s: f64,
}

impl StateBuffer {
    /// `delay_s`: how far in the past poses are rendered; about one state period plus a little jitter margin.
    pub fn new(delay_s: f64) -> StateBuffer {
        StateBuffer { samples: VecDeque::new(), offsets: VecDeque::new(), delay_s }
    }

    pub fn clear(&mut self) {
        self.samples.clear();
        self.offsets.clear();
    }

    /// Adds a sample that arrived at `arrival_s` on the client's monotonic clock. `running` is false while the
    /// session is paused: simulated time then stands still, so the clock offset is forgotten until it runs again.
    /// A sample that is not finite is ignored; a sample from before the newest one means the session restarted,
    /// so the old samples are dropped.
    pub fn push(&mut self, mut s: Sample, arrival_s: f64, running: bool) {
        let len = s.att.length();
        if !(s.sim_time_s.is_finite() && s.pos.is_finite() && s.att.is_finite() && arrival_s.is_finite() && len > 1e-9) {
            return;
        }
        s.att = s.att / len;
        if let Some(last) = self.samples.back() {
            if s.sim_time_s < last.sim_time_s {
                self.clear();
            } else if s.sim_time_s == last.sim_time_s {
                if !running {
                    self.offsets.clear();
                }
                return;
            }
        }
        if running {
            self.offsets.push_back(arrival_s - s.sim_time_s);
            if self.offsets.len() > OFFSET_WINDOW {
                self.offsets.pop_front();
            }
        } else {
            self.offsets.clear();
        }
        self.samples.push_back(s);
        // Time-based retention: the composite asks for the pose up to the latency cap in the past (a second or
        // more of samples at the state rate), so the span, not a count, must bound what is kept.
        let cutoff = s.sim_time_s - POSE_SPAN_S;
        while self.samples.front().is_some_and(|first| first.sim_time_s < cutoff) {
            self.samples.pop_front();
        }
        if self.samples.len() > MAX_SAMPLES {
            self.samples.pop_front();
        }
    }

    /// The pose to show at `now_s` on the client's clock. Before the first sample there is none; when the stream
    /// stalls, or while paused, the newest pose is held rather than extrapolated.
    pub fn pose_at(&self, now_s: f64) -> Option<Pose> {
        let latest = *self.samples.back()?;
        let Some(offset) = self.offsets.iter().copied().reduce(f64::min) else {
            return Some(latest.into());
        };
        let target = now_s - offset - self.delay_s;
        if target >= latest.sim_time_s {
            return Some(latest.into());
        }
        let first = *self.samples.front()?;
        if target <= first.sim_time_s {
            return Some(first.into());
        }
        let after = self.samples.partition_point(|s| s.sim_time_s <= target);
        let (a, b) = (self.samples[after - 1], self.samples[after]);
        let t = (target - a.sim_time_s) / (b.sim_time_s - a.sim_time_s);
        Some(Pose { pos: a.pos.lerp(b.pos, t), att: a.att.slerp(b.att, t) })
    }

    /// The simulator's idea of "now" for `client_now_s`: the least-delayed recent arrival's clock offset
    /// (delays only add, so the minimum is the best estimate). None before the first sample and while paused
    /// (the offsets are forgotten then) — the caller keeps the last picture.
    pub fn sim_now(&self, client_now_s: f64) -> Option<f64> {
        let latest = *self.samples.back()?;
        let offset = self.offsets.iter().copied().reduce(f64::min)?;
        Some((client_now_s - offset).max(latest.sim_time_s))
    }

    /// The pose at an exact simulated time (no clock offset, no display delay): interpolated between the two
    /// samples that bracket it; the oldest or newest pose held outside them.
    pub fn pose_at_sim(&self, sim_time_s: f64) -> Option<Pose> {
        if self.samples.is_empty() {
            return None;
        }
        if sim_time_s <= self.samples.front()?.sim_time_s {
            return Some((*self.samples.front()?).into());
        }
        let latest = *self.samples.back()?;
        if sim_time_s >= latest.sim_time_s {
            return Some(latest.into());
        }
        let after = self.samples.partition_point(|s| s.sim_time_s <= sim_time_s);
        let (a, b) = (self.samples[after - 1], self.samples[after]);
        let t = (sim_time_s - a.sim_time_s) / (b.sim_time_s - a.sim_time_s);
        Some(Pose { pos: a.pos.lerp(b.pos, t), att: a.att.slerp(b.att, t) })
    }
}
