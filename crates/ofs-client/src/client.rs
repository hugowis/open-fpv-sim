//! The client handle: a supervisor running on its own threads, polled without blocking from a game loop.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch};

use crate::error::{ClientError, ErrorKind};
use crate::interp::{Pose, StateBuffer};
use crate::model::{Command, OsdFrame, Phase, Settings, Sticks, Telemetry, Update, World};
use crate::worker::{lock, run, Shared};

/// How long `shutdown` waits for the supervisor (it unloads the session of a server it launched).
const SHUTDOWN_WAIT: Duration = Duration::from_secs(10);

/// What `poll` has seen so far, for the accessors.
struct Seen {
    phase: Phase,
    detail: String,
    kind: Option<ErrorKind>,
    configurator_address: String,
    quad_name: String,
}

pub struct Client {
    shared: Arc<Shared>,
    commands: Option<mpsc::UnboundedSender<Command>>,
    updates: Mutex<std_mpsc::Receiver<Update>>,
    finished: Mutex<std_mpsc::Receiver<()>>,
    runtime: Option<tokio::runtime::Runtime>,
    seen: Mutex<Seen>,
}

/// Upper bound of the stick rate: a mistyped setting must not make the worker spin or flood the server.
pub const MAX_STICK_RATE_HZ: u32 = 1000;

/// Keeps the two rates inside what the server and the worker handle.
fn clamp_rates(settings: &mut Settings) {
    settings.state_rate_hz = settings.state_rate_hz.clamp(1, 240);
    settings.stick_rate_hz = settings.stick_rate_hz.clamp(1, MAX_STICK_RATE_HZ);
}

impl Client {
    /// Starts the supervisor and returns at once; progress arrives through `poll`.
    pub fn start(mut settings: Settings) -> Result<Client, ClientError> {
        clamp_rates(&mut settings);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("ofs-client")
            .enable_all()
            .build()
            .map_err(|e| ClientError::new(ErrorKind::Other, format!("cannot start the client runtime: {e}")))?;
        let (updates_tx, updates_rx) = std_mpsc::channel();
        let (sticks_tx, _) = watch::channel(Sticks::default());
        let delay_s = 1.0 / f64::from(settings.state_rate_hz) + 0.002;
        let shared = Arc::new(Shared {
            epoch: Instant::now(),
            updates: updates_tx,
            sticks: sticks_tx,
            buffer: Mutex::new(StateBuffer::new(delay_s)),
            telemetry: Mutex::new(None),
            osd: Mutex::new(None),
            osd_version: AtomicU64::new(0),
            world: Mutex::new(None),
            world_version: AtomicU64::new(0),
        });
        let (commands_tx, commands_rx) = mpsc::unbounded_channel();
        let (finished_tx, finished_rx) = std_mpsc::channel();
        let worker_shared = shared.clone();
        runtime.spawn(async move {
            run(settings, worker_shared, commands_rx).await;
            let _ = finished_tx.send(());
        });
        Ok(Client {
            shared,
            commands: Some(commands_tx),
            updates: Mutex::new(updates_rx),
            finished: Mutex::new(finished_rx),
            runtime: Some(runtime),
            seen: Mutex::new(Seen {
                phase: Phase::Connecting,
                detail: String::new(),
                kind: None,
                configurator_address: String::new(),
                quad_name: String::new(),
            }),
        })
    }

    /// Replaces the transmitter's sticks (clamped to their ranges); the pilot link sends the latest at its own rate.
    pub fn set_sticks(&self, sticks: Sticks) {
        self.shared.sticks.send_replace(sticks.sanitized());
    }

    pub fn send(&self, command: Command) {
        if let Some(tx) = &self.commands {
            let _ = tx.send(command);
        }
    }

    /// Everything that happened since the last call, oldest first. Never blocks.
    pub fn poll(&self) -> Vec<Update> {
        let updates: Vec<Update> = lock_receiver(&self.updates).try_iter().collect();
        let mut seen = lock(&self.seen);
        for update in &updates {
            match update {
                Update::Phase { phase, detail, kind } => {
                    seen.phase = *phase;
                    seen.detail = detail.clone();
                    seen.kind = *kind;
                }
                Update::Session { quad_name, configurator_address } => {
                    seen.quad_name = quad_name.clone();
                    seen.configurator_address = configurator_address.clone();
                }
                Update::Event(_) | Update::Error(_) => {}
            }
        }
        updates
    }

    /// The phase as of the last `poll`, with its detail and, for `Failed`, the error kind.
    pub fn phase(&self) -> (Phase, String, Option<ErrorKind>) {
        let seen = lock(&self.seen);
        (seen.phase, seen.detail.clone(), seen.kind)
    }

    /// As of the last `poll`: empty without Betaflight, or before the quad is loaded.
    pub fn configurator_address(&self) -> String {
        lock(&self.seen).configurator_address.clone()
    }

    pub fn quad_name(&self) -> String {
        lock(&self.seen).quad_name.clone()
    }

    /// Where to draw the vehicle now (Godot frame), interpolated between the last states received.
    pub fn pose(&self) -> Option<Pose> {
        lock(&self.shared.buffer).pose_at(self.shared.epoch.elapsed().as_secs_f64())
    }

    /// The newest telemetry, with its age.
    pub fn telemetry(&self) -> Option<Telemetry> {
        let (telemetry, received) = lock(&self.shared.telemetry).clone()?;
        Some(Telemetry { age_s: received.elapsed().as_secs_f64(), ..telemetry })
    }

    /// The newest OSD frame; `None` before the first one arrives and right after a reload.
    pub fn osd(&self) -> Option<OsdFrame> {
        lock(&self.shared.osd).clone()
    }

    /// Counts OSD updates (and clears): a changed value means [`osd`](Self::osd) has something new to draw.
    pub fn osd_version(&self) -> u64 {
        self.shared.osd_version.load(Ordering::Acquire)
    }

    /// The world of the loaded session, in Godot's frame; `None` before the first load and while one is under way.
    pub fn world(&self) -> Option<World> {
        lock(&self.shared.world).clone()
    }

    /// Counts world updates (and clears): a changed value means [`world`](Self::world) has something new to build.
    pub fn world_version(&self) -> u64 {
        self.shared.world_version.load(Ordering::Acquire)
    }

    /// Ends the session (unloading it first when this client launched the server) and stops the supervisor.
    /// Also done on drop.
    pub fn shutdown(&mut self) {
        self.commands = None; // closing the channel is the quit signal
        if let Some(runtime) = self.runtime.take() {
            let _ = lock_receiver(&self.finished).recv_timeout(SHUTDOWN_WAIT);
            runtime.shutdown_timeout(Duration::from_secs(2));
        }
        lock(&self.seen).phase = Phase::Stopped;
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn lock_receiver<T>(m: &Mutex<std_mpsc::Receiver<T>>) -> std::sync::MutexGuard<'_, std_mpsc::Receiver<T>> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_are_clamped_to_what_the_server_and_worker_handle() {
        let mut settings = Settings::new("quad.toml");
        settings.state_rate_hz = 100_000;
        settings.stick_rate_hz = 4_000_000_000;
        clamp_rates(&mut settings);
        assert_eq!((settings.state_rate_hz, settings.stick_rate_hz), (240, MAX_STICK_RATE_HZ));
        settings.state_rate_hz = 0;
        settings.stick_rate_hz = 0;
        clamp_rates(&mut settings);
        assert_eq!((settings.state_rate_hz, settings.stick_rate_hz), (1, 1));
        let mut defaults = Settings::new("quad.toml");
        clamp_rates(&mut defaults);
        assert_eq!((defaults.state_rate_hz, defaults.stick_rate_hz), (Settings::new("quad.toml").state_rate_hz, 250));
    }
}
