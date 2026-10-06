//! Paces real-time sessions to the wall clock on a background thread.
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::pacer::MAX_LAG_S;
use crate::pb;
use crate::session::{event, RunMode, Shared};

/// Poll interval while no real-time session is running.
const IDLE: Duration = Duration::from_millis(5);

pub fn spawn(shared: Arc<Shared>) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("ofs-realtime".into())
        .spawn(move || {
            while !shared.stopping() {
                let sleep = pace_once(&shared);
                std::thread::sleep(sleep);
            }
        })
        .expect("failed to spawn the real-time runner thread")
}

/// One pacing step of the loaded session, if it is a running real-time session. Returns how long to sleep.
pub fn pace_once(shared: &Shared) -> Duration {
    let mut slot = shared.lock();
    let Some(s) = slot.as_mut() else { return IDLE };
    if s.mode != RunMode::Realtime || !s.running {
        return IDLE;
    }
    let plan = s.pacer.plan(shared.wall_s(), s.vehicle.time_s());
    let mut events = Vec::new();
    if plan.overrun {
        let message = format!("fell more than {} ms behind the wall clock", (MAX_LAG_S * 1000.0) as u32);
        events.push(event(s.vehicle.time_s(), pb::EventKind::Overrun, message));
    }
    if plan.ticks > 0 {
        if let Err(e) = s.step(plan.ticks) {
            tracing::error!("real-time session stopped: {e}");
            events.push(event(s.vehicle.time_s(), pb::EventKind::SimError, e.to_string()));
        }
        events.extend(s.changes());
    }
    drop(slot);
    shared.publish(events);
    Duration::from_secs_f64(plan.sleep_s)
}
