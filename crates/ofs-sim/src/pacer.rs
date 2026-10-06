//! Real-time pacing (spec §5.1): how many base ticks to run now so simulated time follows the wall clock.
//! Pure arithmetic on seconds, so it is tested with synthetic clocks; the runner thread feeds it real time.
//!
//! When the simulation falls behind:
//! - `Warn` catches up in bursts while the backlog stays within [`MAX_LAG_S`]; beyond that the backlog is
//!   dropped (simulated time slips against the wall clock) and one overrun is counted.
//! - `Slow` never bursts more than one chunk: simulated time stretches instead, and one overrun is counted
//!   for every [`MAX_LAG_S`] of accumulated stretch.
//!
//! Either way the simulation itself stays exact (it is stepped tick by tick); only its alignment with the
//! wall clock suffers, and the overrun count says so.

/// Backlog `Warn` catches up on before dropping it; also the stretch that counts as one `Slow` overrun.
pub const MAX_LAG_S: f64 = 0.1;
/// Most simulated time run per pacing step, so other threads get the session lock regularly.
pub const CHUNK_S: f64 = 0.05;
/// Smallest backlog worth a pacing step (one Betaflight exchange at 1 kHz).
pub const MIN_BATCH_S: f64 = 0.001;
/// Sleep after a pacing step that ran ticks: lets other threads take the session lock.
pub const YIELD_S: f64 = 0.001;
/// Longest sleep while ahead of the wall clock, so pause/resume and new work are picked up quickly.
pub const MAX_SLEEP_S: f64 = 0.005;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverrunPolicy {
    Warn,
    Slow,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plan {
    /// Base ticks to run now.
    pub ticks: u64,
    /// Seconds to sleep after running them.
    pub sleep_s: f64,
    /// True when this step counted an overrun.
    pub overrun: bool,
}

#[derive(Debug, Clone)]
pub struct Pacer {
    policy: OverrunPolicy,
    base_hz: u32,
    anchor_wall_s: f64,
    anchor_sim_s: f64,
    stretched_s: f64,
    overruns: u64,
}

impl Pacer {
    pub fn new(policy: OverrunPolicy, base_hz: u32) -> Self {
        assert!(base_hz > 0, "base_hz must be > 0");
        Self { policy, base_hz, anchor_wall_s: 0.0, anchor_sim_s: 0.0, stretched_s: 0.0, overruns: 0 }
    }

    /// Aligns simulated time `sim_s` with wall time `now_s`; call when (re)starting real-time running.
    pub fn restart(&mut self, now_s: f64, sim_s: f64) {
        self.anchor_wall_s = now_s;
        self.anchor_sim_s = sim_s;
        self.stretched_s = 0.0;
    }

    pub fn overruns(&self) -> u64 {
        self.overruns
    }

    fn chunk_ticks(&self) -> u64 {
        (CHUNK_S * f64::from(self.base_hz)).round() as u64
    }

    pub fn plan(&mut self, now_s: f64, sim_s: f64) -> Plan {
        let mut lag = self.anchor_sim_s + (now_s - self.anchor_wall_s) - sim_s;
        let allowed = match self.policy {
            OverrunPolicy::Warn => MAX_LAG_S,
            OverrunPolicy::Slow => CHUNK_S,
        };
        let mut overrun = false;
        if lag > allowed {
            let excess = lag - allowed;
            self.anchor_wall_s = now_s;
            self.anchor_sim_s = sim_s + allowed;
            lag = allowed;
            overrun = match self.policy {
                OverrunPolicy::Warn => true,
                OverrunPolicy::Slow => {
                    self.stretched_s += excess;
                    if self.stretched_s >= MAX_LAG_S {
                        self.stretched_s -= MAX_LAG_S;
                        true
                    } else {
                        false
                    }
                }
            };
            if overrun {
                self.overruns += 1;
            }
        }
        if lag < MIN_BATCH_S {
            return Plan { ticks: 0, sleep_s: (MIN_BATCH_S - lag).clamp(YIELD_S, MAX_SLEEP_S), overrun };
        }
        let ticks = ((lag * f64::from(self.base_hz)).floor() as u64).min(self.chunk_ticks());
        Plan { ticks, sleep_s: YIELD_S, overrun }
    }
}
