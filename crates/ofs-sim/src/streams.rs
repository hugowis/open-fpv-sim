//! Streaming RPCs: state at a client rate, the pilot's transmitter link, and session watchers.
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Code, Status, Streaming};

use crate::pb;
use crate::server::{error, loaded, not_loaded, parse_sticks};
use crate::session::{event, Shared};

pub const DEFAULT_STATE_RATE_HZ: u32 = 60;
/// Spec §5.2: client state streams run at up to 240 Hz.
pub const MAX_STATE_RATE_HZ: u32 = 240;

pub fn state_rate(hz: u32) -> Result<u32, Status> {
    match hz {
        0 => Ok(DEFAULT_STATE_RATE_HZ),
        1..=MAX_STATE_RATE_HZ => Ok(hz),
        _ => Err(error("invalid_argument", Code::InvalidArgument, format!("state rate must be 1..={MAX_STATE_RATE_HZ} Hz (got {hz})"))),
    }
}

/// Sends the session state every 1/rate s from a plain thread, until the client goes away, the server stops,
/// or the session is unloaded (then the stream ends with a not_loaded error).
pub fn state_feed(shared: Arc<Shared>, rate_hz: u32) -> ReceiverStream<Result<pb::State, Status>> {
    let (tx, rx) = mpsc::channel(4);
    let period = Duration::from_secs_f64(1.0 / f64::from(rate_hz));
    std::thread::spawn(move || loop {
        std::thread::sleep(period);
        if tx.is_closed() || shared.stopping() {
            break;
        }
        let msg = shared.lock().as_ref().map(|s| s.state_msg()).ok_or_else(not_loaded);
        let last = msg.is_err();
        if tx.blocking_send(msg).is_err() || last {
            break;
        }
    });
    ReceiverStream::new(rx)
}

/// Runs `f` off the async runtime (it locks the session, which can wait for a runner step).
fn off_runtime(f: impl FnOnce() + Send + 'static) {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => drop(handle.spawn_blocking(f)),
        Err(_) => f(),
    }
}

/// Held while a pilot is connected. Dropping it (the input stream ended or failed) turns the transmitter off,
/// so Betaflight fails safe as with a real radio switched off, and frees the pilot slot.
struct PilotGuard(Arc<Shared>);

impl Drop for PilotGuard {
    fn drop(&mut self) {
        let shared = self.0.clone();
        off_runtime(move || {
            let time_s = shared.lock().as_mut().map(|s| {
                s.vehicle.set_transmitter(false);
                s.vehicle.time_s()
            });
            shared.pilot.store(false, Ordering::Release);
            if let Some(t) = time_s {
                shared.publish([event(t, pb::EventKind::PilotDisconnected, "pilot disconnected: transmitter off")]);
            }
        });
    }
}

pub async fn pilot(
    shared: Arc<Shared>,
    mut inbound: Streaming<pb::PilotInput>,
) -> Result<ReceiverStream<Result<pb::State, Status>>, Status> {
    let first = inbound
        .message()
        .await?
        .ok_or_else(|| error("invalid_argument", Code::InvalidArgument, "send a PilotInput to start piloting"))?;
    let rate_hz = state_rate(first.state_rate_hz)?;
    let first_sticks = parse_sticks(first.sticks.unwrap_or_default())?;
    if shared.pilot.swap(true, Ordering::AcqRel) {
        return Err(error("pilot_busy", Code::AlreadyExists, "a pilot is already connected"));
    }
    let guard = PilotGuard(shared.clone());
    let s = shared.clone();
    tokio::task::spawn_blocking(move || -> Result<(), Status> {
        let mut slot = s.lock();
        let session = loaded(&mut slot)?;
        session.vehicle.set_sticks(&first_sticks);
        session.vehicle.set_transmitter(true);
        let t = session.vehicle.time_s();
        drop(slot);
        s.publish([event(t, pb::EventKind::PilotConnected, "pilot connected: transmitter on")]);
        Ok(())
    })
    .await
    .map_err(|e| error("internal", Code::Internal, e.to_string()))??;

    let input_shared = shared.clone();
    tokio::spawn(async move {
        let _guard = guard;
        while let Ok(Some(input)) = inbound.message().await {
            // A malformed input is dropped, like a corrupted radio packet.
            let Ok(sticks) = parse_sticks(input.sticks.unwrap_or_default()) else { continue };
            let s = input_shared.clone();
            let applied = tokio::task::spawn_blocking(move || {
                if let Some(session) = s.lock().as_mut() {
                    session.vehicle.set_sticks(&sticks);
                }
            })
            .await;
            if applied.is_err() {
                break;
            }
        }
    });
    Ok(state_feed(shared, rate_hz))
}

/// Streams session events. When the last watcher goes away, a watched session without keep_alive ends.
pub async fn watch(shared: Arc<Shared>) -> ReceiverStream<Result<pb::Event, Status>> {
    let mut events = shared.events.subscribe();
    shared.watchers.fetch_add(1, Ordering::AcqRel);
    let s = shared.clone();
    let _ = tokio::task::spawn_blocking(move || {
        if let Some(session) = s.lock().as_mut() {
            session.watched = true;
        }
    })
    .await;
    let (tx, rx) = mpsc::channel(64);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                received = events.recv() => match received {
                    Ok(e) => {
                        if tx.send(Ok(e)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                _ = tx.closed() => break,
            }
        }
        let _ = tokio::task::spawn_blocking(move || unwatch(&shared)).await;
    });
    ReceiverStream::new(rx)
}

fn unwatch(shared: &Shared) {
    if shared.watchers.fetch_sub(1, Ordering::AcqRel) != 1 {
        return;
    }
    let mut slot = shared.lock();
    let Some(s) = slot.as_ref() else { return };
    if !s.watched || s.keep_alive {
        return;
    }
    let t = s.vehicle.time_s();
    *slot = None; // stops the vehicle, and Betaflight SITL with it
    drop(slot);
    shared.publish([event(t, pb::EventKind::SessionEnded, "the last watcher disconnected and keep_alive is off")]);
}
