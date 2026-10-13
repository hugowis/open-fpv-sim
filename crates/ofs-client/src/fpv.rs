//! The FPV picture as the goggles compose it (M3e spec §3): pose, video quality and OSD all sampled at one
//! delayed, field-quantized instant, so the picture ages as a unit at the field rate and a dropped field holds
//! the previous one. The render instant is the newest field boundary at or before `sim_now − latency`.
use std::collections::VecDeque;

use crate::interp::Pose;
use crate::model::{OsdFrame, VideoSync};

/// One analog video field: 20 ms of simulated time (50 fields a second), the same constant the video link runs at.
pub use ofs_core::consts::VIDEO_FIELD_PERIOD_S as FIELD_PERIOD_S;
/// Fields kept (1.28 s of picture) and OSD frames kept (0.64 s of OSD): ample for a goggle's latency.
const MAX_FIELDS: usize = 64;
const MAX_OSD_FRAMES: usize = 32;

/// What a received field carries for the shader (the state's picture values).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldUniforms {
    pub present: bool,
    pub noise: f64,
    pub sparkles: f64,
    pub chroma: f64,
    pub sync: VideoSync,
}

/// What to draw this frame: the pose, the picture and the OSD of one received field.
#[derive(Debug, Clone, PartialEq)]
pub struct FpvView {
    pub pose: Pose,
    pub uniforms: FieldUniforms,
    /// Whether the shown field arrived (false: it is a held older field).
    pub field_valid: bool,
    pub osd: Option<OsdFrame>,
    /// Simulated time of the field the picture shows (a field boundary).
    pub render_time_s: f64,
    /// How many field boundaries the shown field lags the render target by: 0 when the shown field is
    /// current, ≥ 1 when dropped fields held it back (the caller repeats the held picture).
    pub held_fields: u32,
    /// The goggles' glass-to-glass latency the instant was computed with.
    pub latency_ms: u32,
}

/// The newest field boundary at or before `sim_time_s`.
pub fn field_floor(sim_time_s: f64) -> f64 {
    (sim_time_s / FIELD_PERIOD_S).floor() * FIELD_PERIOD_S
}

/// How many field boundaries `render_time_s` (the field the picture shows) lags the render target `target_s`
/// by: 0 when the shown field is current, ≥ 1 when dropped fields held it back. Both are field boundaries, so
/// the target is exactly recoverable as `render_time_s + held_fields × FIELD_PERIOD_S`.
pub fn held_fields(target_s: f64, render_time_s: f64) -> u32 {
    ((target_s - render_time_s) / FIELD_PERIOD_S).round() as u32
}

/// The integer index of the field boundary nearest `time_s` (boundary k is at k × FIELD_PERIOD_S). Boundaries are
/// computed in floating point, so anything that dedups on fields keys on this index, never on the boundary time.
pub fn field_index(time_s: f64) -> i64 {
    (time_s / FIELD_PERIOD_S).round() as i64
}

/// The field the picture renders for: the shown field plus the boundaries dropped fields held it back by. The DVR
/// writes one frame per target field.
pub fn target_field(render_time_s: f64, held_fields: u32) -> i64 {
    field_index(render_time_s) + i64::from(held_fields)
}

#[derive(Debug, Default)]
pub struct FpvBuffer {
    /// One entry per received field: (field boundary, uniforms, the field arrived).
    fields: VecDeque<(f64, FieldUniforms, bool)>,
    osd: VecDeque<OsdFrame>,
}

impl FpvBuffer {
    /// Adds a state sample's field. The state stream is faster than the video, so samples inside one field
    /// replace that field's entry; a sample from before the newest one clears the buffer (the session restarted).
    pub fn push_field(&mut self, time_s: f64, uniforms: FieldUniforms, valid: bool) {
        if !(time_s.is_finite() && uniforms.noise.is_finite() && uniforms.sparkles.is_finite() && uniforms.chroma.is_finite()) {
            return;
        }
        let boundary = field_floor(time_s);
        if let Some(&(last, _, _)) = self.fields.back() {
            if boundary == last {
                let entry = self.fields.back_mut().unwrap();
                entry.1 = uniforms;
                entry.2 = valid;
                return;
            }
            if boundary < last {
                self.clear();
            }
        }
        self.fields.push_back((boundary, uniforms, valid));
        if self.fields.len() > MAX_FIELDS {
            self.fields.pop_front();
        }
    }

    pub fn push_osd(&mut self, frame: OsdFrame) {
        if !frame.time_s.is_finite() {
            return;
        }
        self.osd.push_back(frame);
        if self.osd.len() > MAX_OSD_FRAMES {
            self.osd.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.fields.clear();
        self.osd.clear();
    }

    /// The field whose picture to show for the render instant `target_s` (a field boundary): the newest field at
    /// or before it that arrived — a dropped field holds the previous one. The bool is whether that field
    /// arrived. None when every buffered field before the instant was dropped; the caller keeps the last picture.
    pub fn field_at(&self, target_s: f64) -> Option<(f64, FieldUniforms, bool)> {
        let mut i = self.fields.iter().rposition(|(t, _, _)| *t <= target_s + 1e-9)?;
        while !self.fields[i].2 {
            i = i.checked_sub(1)?;
        }
        Some(self.fields[i])
    }

    /// The newest OSD frame drawn no later than `target_s` (an exactly-at-the-instant frame counts as drawn).
    pub fn osd_at(&self, target_s: f64) -> Option<OsdFrame> {
        self.osd.iter().rev().find(|f| f.time_s <= target_s + 1e-9).cloned()
    }
}
