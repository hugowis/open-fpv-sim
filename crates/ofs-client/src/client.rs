//! The client handle: a supervisor running on its own threads, polled without blocking from a game loop.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch};

use crate::dvr::{self, DvrFeed, DvrSummary, DvrWriter};
use crate::dvr_audio::AudioLog;
use crate::error::{ClientError, ErrorKind};
use crate::fpv::{self, FpvBuffer, FpvView};
use crate::interp::{Pose, StateBuffer};
use crate::model::{Command, EventKind, OsdFrame, Phase, Settings, Sticks, Telemetry, Update, World};
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
    dvr: Mutex<[Option<(DvrWriter, AudioLog)>; 2]>, // indexed by DvrFeed
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
            fpv: Mutex::new(FpvBuffer::default()),
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
            dvr: Mutex::new([None, None]),
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
        drop(seen);
        self.log_collisions(&updates);
        updates
    }

    /// Every collision goes to the audio log of each recording feed (the lock is taken after `seen` is released).
    fn log_collisions(&self, updates: &[Update]) {
        let mut dvr = lock(&self.dvr);
        for update in updates {
            if let Update::Event(e) = update {
                if e.kind == EventKind::Collision {
                    for (_, audio) in dvr.iter_mut().flatten() {
                        audio.collision(e.time_s);
                    }
                }
            }
        }
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

    /// The FPV picture for this frame (the goggles' pipeline): the newest field boundary at or before
    /// `sim_now − latency` with the pose, video uniforms and OSD all sampled there; a dropped field holds the
    /// previous one. None before the first state, while paused, or when every buffered field was dropped —
    /// the caller keeps the last picture.
    pub fn fpv_view(&self) -> Option<FpvView> {
        let now_s = self.shared.epoch.elapsed().as_secs_f64();
        let buffer = lock(&self.shared.buffer);
        let sim_now = buffer.sim_now(now_s)?;
        let latency_ms = lock(&self.shared.telemetry).clone().map_or(0, |(t, _)| t.video.latency_ms);
        let target = fpv::field_floor(sim_now - f64::from(latency_ms) / 1000.0);
        let (render_time_s, uniforms, field_valid) = lock(&self.shared.fpv).field_at(target)?;
        let pose = buffer.pose_at_sim(render_time_s)?;
        let osd = lock(&self.shared.fpv).osd_at(render_time_s);
        Some(FpvView { pose, uniforms, field_valid, osd, render_time_s, held_fields: fpv::held_fields(target, render_time_s), latency_ms })
    }

    /// The world of the loaded session, in Godot's frame; `None` before the first load and while one is under way.
    pub fn world(&self) -> Option<World> {
        lock(&self.shared.world).clone()
    }

    /// Counts world updates (and clears): a changed value means [`world`](Self::world) has something new to build.
    pub fn world_version(&self) -> u64 {
        self.shared.world_version.load(Ordering::Acquire)
    }

    /// Starts a DVR recording of `feed` to `path` at the field rate with `width` × `height` pixels.
    pub fn dvr_start(&self, feed: DvrFeed, path: String, width: u32, height: u32) -> Result<(), String> {
        let mut dvr = lock(&self.dvr);
        let slot = &mut dvr[feed as usize];
        if slot.is_some() {
            return Err("already recording".into());
        }
        let avi = PathBuf::from(path);
        if avi.extension().is_some_and(|e| e.eq_ignore_ascii_case("audio")) {
            return Err(format!("{}: the .audio extension is reserved for the audio sidecar", avi.display()));
        }
        let writer = DvrWriter::start(avi.clone(), dvr::DVR_FPS, width, height)?; // refuses an existing AVI
        match AudioLog::create(&Path::new(&avi).with_extension("audio")) {
            Ok(audio) => *slot = Some((writer, audio)),
            Err(e) => {
                let _ = writer.stop();
                let _ = std::fs::remove_file(&avi);
                return Err(e);
            }
        }
        Ok(())
    }

    /// Hands one frame to the writer; a frame it accepts gets its audio record from the newest telemetry.
    pub fn dvr_write_frame(&self, feed: DvrFeed, jpeg: Vec<u8>) {
        // The telemetry is read before the dvr lock is taken: the two are never held together.
        let input = lock(&self.shared.telemetry).as_ref().map(|(t, _)| (t.audio_input(true), t.time_s, t.collision_speed_mps));
        if let Some((writer, audio)) = lock(&self.dvr)[feed as usize].as_mut() {
            if writer.write_frame(jpeg) {
                audio.frame(input.as_ref().map(|(i, t, v)| (i, *t, *v)));
            }
        }
    }

    /// Stops the `feed` recording; the summary reports frames, drops and bytes.
    pub fn dvr_stop(&self, feed: DvrFeed) -> Result<DvrSummary, String> {
        let (writer, audio) = lock(&self.dvr)[feed as usize].take().ok_or_else(|| "not recording".to_string())?;
        let summary = writer.stop();
        let audio = audio.finish();
        let summary = summary?;
        audio?;
        Ok(summary)
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

    #[test]
    fn an_avi_path_with_the_audio_extension_is_refused_and_creates_nothing() {
        let dir = std::env::temp_dir().join(format!("ofs-client-reserved-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let client = Client::start(Settings { server_addr: "127.0.0.1:1".into(), ..Settings::new("quad.toml") }).unwrap();
        for name in ["rec.audio", "rec.AUDIO"] {
            let err = client.dvr_start(DvrFeed::Goggles, dir.join(name).display().to_string(), 4, 2).unwrap_err();
            assert!(err.contains("reserved for the audio sidecar"), "{err}");
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        assert!(client.dvr_stop(DvrFeed::Goggles).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_collision_goes_to_every_recording_feed_and_other_events_to_none() {
        let dir = std::env::temp_dir().join(format!("ofs-client-collisions-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let client = Client::start(Settings { server_addr: "127.0.0.1:1".into(), ..Settings::new("quad.toml") }).unwrap();
        let (a, b) = (dir.join("a.avi"), dir.join("b.avi"));
        client.dvr_start(DvrFeed::Goggles, a.display().to_string(), 4, 2).unwrap();
        client.dvr_start(DvrFeed::Clean, b.display().to_string(), 4, 2).unwrap();
        let event = |time_s, kind| Update::Event(crate::model::Event { time_s, kind, message: String::new() });
        client.log_collisions(&[event(1.5, crate::model::EventKind::Collision), event(2.0, crate::model::EventKind::LinkDown)]);
        client.dvr_write_frame(DvrFeed::Goggles, vec![0xFF; 8]);
        client.dvr_write_frame(DvrFeed::Clean, vec![0xFF; 8]);
        client.dvr_stop(DvrFeed::Goggles).unwrap();
        client.dvr_stop(DvrFeed::Clean).unwrap();
        for avi in [a, b] {
            let log = std::fs::read_to_string(avi.with_extension("audio")).unwrap();
            assert_eq!(log, "ofs-dvr-audio 1
C 1.5
F NaN 0 0 0 0
");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
