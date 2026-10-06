//! Paces real-time sessions to the wall clock on a background thread.
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::pacer::MAX_LAG_S;
use crate::pb;
use crate::session::{event, panic_message, RunMode, Shared};
use ofs_core::SimError;

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
        // A panic in a step must not kill this thread (every later real-time session would freeze): poison the session.
        match catch_unwind(AssertUnwindSafe(|| s.step(plan.ticks))) {
            Ok(Ok(())) => events.extend(s.changes()),
            Ok(Err(e)) => {
                tracing::error!("real-time session stopped: {e}");
                events.push(event(s.vehicle.time_s(), pb::EventKind::SimError, e.to_string()));
                events.extend(s.changes());
            }
            Err(payload) => {
                let e = SimError::Other(format!("internal panic: {}", panic_message(payload.as_ref())));
                tracing::error!("real-time session stopped: {e}");
                events.push(event(s.vehicle.time_s(), pb::EventKind::SimError, e.to_string()));
                s.poison(e);
            }
        }
    }
    drop(slot);
    shared.publish(events);
    Duration::from_secs_f64(plan.sleep_s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::testing::open_loop_session;

    #[test]
    fn a_panicking_step_poisons_the_session_and_keeps_the_runner_alive() {
        let shared = Shared::new();
        let mut events = shared.events.subscribe();
        {
            let mut s = open_loop_session(RunMode::Realtime);
            s.running = true;
            s.panic_on_step = true;
            *shared.lock() = Some(s);
        }
        std::thread::sleep(Duration::from_millis(30)); // let the pacer owe a few ticks
        pace_once(&shared); // must not unwind
        let slot = shared.lock();
        let s = slot.as_ref().unwrap();
        assert!(!s.running);
        assert!(matches!(&s.failure, Some(SimError::Other(m)) if m.contains("internal panic: test panic in step")), "{:?}", s.failure);
        drop(slot);
        let e = events.try_recv().unwrap();
        assert_eq!(e.kind, pb::EventKind::SimError as i32);
        assert!(e.message.contains("test panic in step"), "{}", e.message);
        assert_eq!(pace_once(&shared), IDLE, "a poisoned session is left alone");
    }
}
