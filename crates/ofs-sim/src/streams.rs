//! Streaming RPCs: state at a client rate, the pilot's transmitter link, and session watchers.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, mpsc, Notify};
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
    feed(shared, rate_hz, None).0
}

/// `state_feed`, optionally bound to one session (a different loaded session counts as unloaded). The `Notify`
/// is signalled once the feed has ended.
fn feed(shared: Arc<Shared>, rate_hz: u32, bind: Option<u64>) -> (ReceiverStream<Result<pb::State, Status>>, Arc<Notify>) {
    let (tx, rx) = mpsc::channel(4);
    let ended = Arc::new(Notify::new());
    let signal = ended.clone();
    let period = Duration::from_secs_f64(1.0 / f64::from(rate_hz));
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(period);
            if tx.is_closed() || shared.stopping() {
                break;
            }
            let msg = match shared.lock().as_ref() {
                Some(s) if bind.map_or(true, |id| s.id == id) => Ok(s.state_msg()),
                _ => Err(not_loaded()),
            };
            let last = msg.is_err();
            if tx.blocking_send(msg).is_err() || last {
                break;
            }
        }
        signal.notify_one();
    });
    (ReceiverStream::new(rx), ended)
}

/// Runs `f` off the async runtime (it locks the session, which can wait for a runner step).
fn off_runtime(f: impl FnOnce() + Send + 'static) {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => drop(handle.spawn_blocking(f)),
        Err(_) => f(),
    }
}

/// One Pilot connection's state, shared between its call, its input task and the guard's cleanup.
struct PilotLink {
    /// False once the connection is gone; the setup step then does nothing, whenever it gets the session lock.
    alive: AtomicBool,
    /// The session this pilot switched the transmitter on for (0 = none). Only touched under the session lock.
    session: AtomicU64,
}

/// Held while a pilot is connected. Dropping it (the input stream ended or failed, or the call was cancelled)
/// turns the transmitter off, so Betaflight fails safe as with a real radio switched off, and frees the pilot slot.
/// It only ever touches the session this pilot connected to.
struct PilotGuard {
    shared: Arc<Shared>,
    link: Arc<PilotLink>,
}

impl Drop for PilotGuard {
    fn drop(&mut self) {
        self.link.alive.store(false, Ordering::Release); // before anything is queued: a late setup step sees it
        let shared = self.shared.clone();
        let link = self.link.clone();
        off_runtime(move || {
            let time_s = match shared.lock().as_mut() {
                Some(s) if s.id == link.session.load(Ordering::Acquire) => {
                    s.vehicle.set_transmitter(false);
                    Some(s.vehicle.time_s())
                }
                _ => None,
            };
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
    let link = Arc::new(PilotLink { alive: AtomicBool::new(true), session: AtomicU64::new(0) });
    let guard = PilotGuard { shared: shared.clone(), link: link.clone() };
    let s = shared.clone();
    let setup_link = link.clone();
    let session_id = tokio::task::spawn_blocking(move || -> Result<u64, Status> {
        let mut slot = s.lock();
        let session = loaded(&mut slot)?;
        if !setup_link.alive.load(Ordering::Acquire) {
            return Err(Status::cancelled("the pilot went away before connecting"));
        }
        session.vehicle.set_sticks(&first_sticks);
        session.vehicle.set_transmitter(true);
        setup_link.session.store(session.id, Ordering::Release);
        let (id, t) = (session.id, session.vehicle.time_s());
        drop(slot);
        s.publish([event(t, pb::EventKind::PilotConnected, "pilot connected: transmitter on")]);
        Ok(id)
    })
    .await
    .map_err(|e| error("internal", Code::Internal, e.to_string()))??;

    let (states, feed_ended) = feed(shared.clone(), rate_hz, Some(session_id));
    let input_shared = shared.clone();
    tokio::spawn(async move {
        let _guard = guard;
        loop {
            // The pilot is over when its input ends or its session (and so its state feed) does.
            let input = tokio::select! {
                received = inbound.message() => match received {
                    Ok(Some(input)) => input,
                    _ => break,
                },
                _ = feed_ended.notified() => break,
            };
            // A malformed input is dropped, like a corrupted radio packet.
            let Ok(sticks) = parse_sticks(input.sticks.unwrap_or_default()) else { continue };
            let s = input_shared.clone();
            let applied = tokio::task::spawn_blocking(move || match s.lock().as_mut() {
                Some(session) if session.id == session_id => {
                    session.vehicle.set_sticks(&sticks);
                    true
                }
                _ => false, // the session this pilot connected to is gone
            })
            .await;
            if !matches!(applied, Ok(true)) {
                break;
            }
        }
    });
    Ok(states)
}

/// One Watch call's state, shared between the call and the guard's cleanup.
struct WatchLink {
    /// False once the call is gone; the registration step then does nothing, whenever it gets the session lock.
    alive: AtomicBool,
    /// This watcher is in `Shared::watchers`. Only touched under the session lock.
    counted: AtomicBool,
}

/// Held while a Watch call is open. Dropping it (the stream ended, or the call was cancelled) unregisters the
/// watcher; when it was the last one, a watched session without keep_alive ends.
struct WatchGuard {
    shared: Arc<Shared>,
    link: Arc<WatchLink>,
}

impl Drop for WatchGuard {
    fn drop(&mut self) {
        self.link.alive.store(false, Ordering::Release); // before anything is queued: a late registration sees it
        let shared = self.shared.clone();
        let link = self.link.clone();
        off_runtime(move || unwatch(&shared, &link));
    }
}

/// Streams session events. When the last watcher goes away, a watched session without keep_alive ends.
pub async fn watch(shared: Arc<Shared>) -> ReceiverStream<Result<pb::Event, Status>> {
    let mut events = shared.events.subscribe();
    let link = Arc::new(WatchLink { alive: AtomicBool::new(true), counted: AtomicBool::new(false) });
    let guard = WatchGuard { shared: shared.clone(), link: link.clone() };
    let s = shared.clone();
    let l = link.clone();
    let _ = tokio::task::spawn_blocking(move || {
        // Registering and marking the session happen under the session lock, as do Load's read of the count
        // and the last watcher's unload decision, so none of them can interleave.
        let mut slot = s.lock();
        if !l.alive.load(Ordering::Acquire) {
            return;
        }
        s.watchers.fetch_add(1, Ordering::AcqRel);
        l.counted.store(true, Ordering::Release);
        if let Some(session) = slot.as_mut() {
            session.watched = true;
        }
    })
    .await;
    let (tx, rx) = mpsc::channel(64);
    tokio::spawn(async move {
        let _guard = guard;
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
    });
    ReceiverStream::new(rx)
}

fn unwatch(shared: &Shared, link: &WatchLink) {
    let mut slot = shared.lock();
    if !link.counted.swap(false, Ordering::AcqRel) {
        return; // never registered
    }
    if shared.watchers.fetch_sub(1, Ordering::AcqRel) != 1 || shared.watchers.load(Ordering::Acquire) != 0 {
        return;
    }
    let Some(s) = slot.as_ref() else { return };
    if !s.watched || s.keep_alive {
        return;
    }
    let t = s.vehicle.time_s();
    *slot = None; // stops the vehicle, and Betaflight SITL with it
    drop(slot);
    shared.publish([event(t, pb::EventKind::SessionEnded, "the last watcher disconnected and keep_alive is off")]);
}
