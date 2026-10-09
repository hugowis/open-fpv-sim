//! The supervisor task: connects (starting the server when allowed), loads the quad, opens the event stream and
//! the pilot link, and keeps flying until it is told to reload or quit.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use glam::{DQuat, DVec3};
use ofs_proto::pb::{self, sim_client::SimClient};
use ofs_proto::PROTOCOL_VERSION;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Channel, Endpoint};

use crate::error::{ClientError, ErrorKind};
use crate::frames::{quat_to_godot, vec_to_godot};
use crate::interp::{Sample, StateBuffer};
use crate::launch::ServerProcess;
use crate::model::{Command, Event, OsdFrame, OverrunPolicy, Phase, Settings, Sticks, Telemetry, Update, World};

/// The server frees a disconnected pilot's slot asynchronously, so a pilot that reconnects at once (a reload) can
/// meet `pilot_busy` for a moment.
const PILOT_RETRIES: u32 = 30;
const PILOT_RETRY_DELAY: Duration = Duration::from_millis(100);
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);
const LAUNCH_POLL: Duration = Duration::from_millis(100);
const UNLOAD_TIMEOUT: Duration = Duration::from_secs(5);

/// State shared between the supervisor and the `Client` handle.
pub(crate) struct Shared {
    pub epoch: Instant,
    pub updates: std::sync::mpsc::Sender<Update>,
    pub sticks: watch::Sender<Sticks>,
    pub buffer: Mutex<StateBuffer>,
    pub telemetry: Mutex<Option<(Telemetry, Instant)>>,
    pub osd: Mutex<Option<OsdFrame>>,
    pub osd_version: AtomicU64,
    pub world: Mutex<Option<World>>,
    pub world_version: AtomicU64,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Shared {
    fn phase(&self, phase: Phase, detail: impl Into<String>) {
        let _ = self.updates.send(Update::Phase { phase, detail: detail.into(), kind: None });
    }

    fn failed(&self, error: &ClientError) {
        let _ = self.updates.send(Update::Phase { phase: Phase::Failed, detail: error.message.clone(), kind: Some(error.kind) });
    }

    fn error(&self, error: ClientError) {
        let _ = self.updates.send(Update::Error(error));
    }

    fn reset_state(&self) {
        lock(&self.buffer).clear();
        *lock(&self.telemetry) = None;
        self.store_osd(None);
        self.store_world(None);
    }

    fn store_world(&self, world: Option<World>) {
        *lock(&self.world) = world;
        self.world_version.fetch_add(1, Ordering::AcqRel);
    }

    fn store_osd(&self, frame: Option<OsdFrame>) {
        *lock(&self.osd) = frame;
        self.osd_version.fetch_add(1, Ordering::AcqRel);
    }

    fn ingest(&self, state: pb::State) {
        let arrival_s = self.epoch.elapsed().as_secs_f64();
        if let (Some(p), Some(q)) = (state.position_ned_m, state.attitude) {
            let sample = Sample {
                sim_time_s: state.time_s,
                pos: vec_to_godot(DVec3::new(p.x, p.y, p.z)),
                att: quat_to_godot(DQuat::from_xyzw(q.x, q.y, q.z, q.w)),
            };
            lock(&self.buffer).push(sample, arrival_s, state.running);
        }
        *lock(&self.telemetry) = Some((Telemetry::from_pb(&state), Instant::now()));
    }
}

fn status_error(status: tonic::Status) -> ClientError {
    ClientError::from_status(&status)
}

/// One attempt to reach the server and shake hands with it.
async fn connect_once(addr: &str, timeout: Duration) -> Result<SimClient<Channel>, ClientError> {
    let unavailable = |e: &dyn std::fmt::Display| ClientError::new(ErrorKind::Unavailable, format!("no ofs-sim answers on {addr} ({e})"));
    let endpoint = Endpoint::from_shared(format!("http://{addr}"))
        .map_err(|e| ClientError::new(ErrorKind::Other, format!("bad server address {addr}: {e}")))?
        .connect_timeout(timeout)
        .tcp_nodelay(true);
    let channel = endpoint.connect().await.map_err(|e| unavailable(&e))?;
    let mut client = SimClient::new(channel);
    let hello = client.handshake(pb::HandshakeRequest { protocol_version: PROTOCOL_VERSION });
    let reply = match tokio::time::timeout(timeout, hello).await {
        Err(_) => return Err(unavailable(&"the handshake timed out")),
        Ok(Err(status)) if status.code() == tonic::Code::Unavailable => return Err(unavailable(&status.message())),
        Ok(Err(status)) => return Err(status_error(status)),
        Ok(Ok(reply)) => reply.into_inner(),
    };
    if reply.protocol_version != PROTOCOL_VERSION {
        let message = format!("the server speaks protocol {}, this client {PROTOCOL_VERSION}", reply.protocol_version);
        return Err(ClientError::new(ErrorKind::Protocol, message));
    }
    Ok(client)
}

async fn connect_or_launch(
    settings: &Settings,
    shared: &Shared,
    server: &mut Option<ServerProcess>,
) -> Result<SimClient<Channel>, ClientError> {
    match connect_once(&settings.server_addr, PROBE_TIMEOUT).await {
        Ok(client) => return Ok(client),
        Err(e) if e.kind != ErrorKind::Unavailable => return Err(e),
        Err(e) => {
            if settings.launch.is_none() {
                let hint = "Start it with `cargo run -p ofs-sim`, or let the client launch it (setting `server_bin`).";
                return Err(ClientError::new(ErrorKind::Unavailable, format!("{} {hint}", e.message)));
            }
        }
    }
    let spec = settings.launch.as_ref().expect("checked above");
    shared.phase(Phase::Connecting, format!("starting {}", spec.program.display()));
    *server = None; // a server of ours that died: stop its leftovers before starting the next one
    let mut process = ServerProcess::spawn(spec, &settings.server_addr)?;
    let deadline = Instant::now() + settings.launch_timeout;
    loop {
        if let Some(status) = process.try_wait() {
            let tail = process.log_tail();
            let log = if tail.is_empty() { String::new() } else { format!("\n{tail}") };
            return Err(ClientError::new(
                ErrorKind::Launch,
                format!("{} exited during startup ({status}). Is the port in {} already taken?{log}", spec.program.display(), settings.server_addr),
            ));
        }
        match connect_once(&settings.server_addr, PROBE_TIMEOUT).await {
            Ok(client) => {
                *server = Some(process);
                return Ok(client);
            }
            Err(e) if e.kind == ErrorKind::Unavailable && Instant::now() < deadline => tokio::time::sleep(LAUNCH_POLL).await,
            Err(e) => return Err(e),
        }
    }
}

/// Forwards the server's events as updates until the stream ends.
fn watch_events(mut stream: tonic::Streaming<pb::Event>, shared: Arc<Shared>) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Ok(Some(event)) = stream.message().await {
            let _ = shared.updates.send(Update::Event(Event::from_pb(event)));
        }
    })
}

/// Keeps the newest OSD frame until the stream ends (the session was unloaded or the server went away).
fn watch_osd(mut stream: tonic::Streaming<pb::OsdFrame>, shared: Arc<Shared>) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Ok(Some(frame)) = stream.message().await {
            shared.store_osd(Some(OsdFrame::from_pb(frame)));
        }
    })
}

/// The open pilot link: a task sending the latest sticks at a fixed rate and a task receiving states.
struct PilotLink {
    sender: JoinHandle<()>,
    receiver: JoinHandle<Result<(), ClientError>>,
}

impl PilotLink {
    /// Ends both tasks (dropping the streams, which turns the transmitter off) and waits until they are gone.
    async fn stop(self) {
        self.sender.abort();
        let _ = self.sender.await;
        if !self.receiver.is_finished() {
            self.receiver.abort();
            let _ = self.receiver.await;
        }
    }
}

async fn open_pilot(client: &mut SimClient<Channel>, shared: &Arc<Shared>, settings: &Settings) -> Result<PilotLink, ClientError> {
    let period = Duration::from_secs_f64(1.0 / f64::from(settings.stick_rate_hz.max(1)));
    for attempt in 1..=PILOT_RETRIES {
        let (tx, rx) = mpsc::channel::<pb::PilotInput>(8);
        let first = pb::PilotInput { sticks: Some(shared.sticks.borrow().to_pb()), state_rate_hz: settings.state_rate_hz };
        let _ = tx.send(first).await; // queued before the call: the server reads it to set the stream up
        match client.pilot(ReceiverStream::new(rx)).await {
            Ok(response) => {
                let mut stream = response.into_inner();
                let mut sticks = shared.sticks.subscribe();
                let sender = tokio::spawn(async move {
                    let mut tick = tokio::time::interval(period);
                    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
                    loop {
                        tick.tick().await;
                        let latest = sticks.borrow_and_update().to_pb();
                        if tx.send(pb::PilotInput { sticks: Some(latest), state_rate_hz: 0 }).await.is_err() {
                            break; // the stream closed
                        }
                    }
                });
                let shared = shared.clone();
                let receiver = tokio::spawn(async move {
                    loop {
                        match stream.message().await {
                            Ok(Some(state)) => shared.ingest(state),
                            Ok(None) => return Ok(()),
                            Err(status) => return Err(status_error(status)),
                        }
                    }
                });
                return Ok(PilotLink { sender, receiver });
            }
            Err(status) => {
                let error = status_error(status);
                if error.kind == ErrorKind::PilotBusy && attempt < PILOT_RETRIES {
                    tokio::time::sleep(PILOT_RETRY_DELAY).await;
                    continue;
                }
                return Err(error);
            }
        }
    }
    unreachable!("the last attempt returns")
}

enum Fly {
    Reload,
    Quit,
    Failed(ClientError),
}

/// Loads the quad, starts it, opens the pilot link and serves commands until something ends the flight.
async fn fly(client: &mut SimClient<Channel>, shared: &Arc<Shared>, settings: &Settings, commands: &mut mpsc::UnboundedReceiver<Command>) -> Fly {
    shared.reset_state();
    shared.phase(Phase::Loading, format!("loading {} (Betaflight takes a few seconds to boot)", settings.quad_path));
    let policy = match settings.overrun_policy {
        OverrunPolicy::Warn => pb::OverrunPolicy::Warn,
        OverrunPolicy::Slow => pb::OverrunPolicy::Slow,
    };
    let load = pb::LoadRequest {
        quad_path: settings.quad_path.clone(),
        seed: settings.seed,
        mode: pb::Mode::Realtime as i32,
        open_loop_fc: settings.open_loop_fc,
        overrun_policy: policy as i32,
        keep_alive: false,
        world_path: settings.world_path.clone(),
    };
    let reply = match client.load(load).await {
        Ok(reply) => reply.into_inner(),
        Err(status) => return Fly::Failed(status_error(status)),
    };
    match client.get_world(pb::Empty {}).await {
        Ok(world) => shared.store_world(Some(World::from_pb(world.into_inner()))),
        Err(status) => return Fly::Failed(status_error(status)),
    }
    let _ = shared.updates.send(Update::Session { quad_name: reply.quad_name, configurator_address: reply.configurator_address });
    if let Err(status) = client.start(pb::Empty {}).await {
        return Fly::Failed(status_error(status));
    }
    let mut link = match open_pilot(client, shared, settings).await {
        Ok(link) => link,
        Err(error) => return Fly::Failed(error),
    };
    shared.phase(Phase::Flying, "");
    let osd_task = match client.stream_osd(pb::StreamRequest { rate_hz: 60 }).await {
        Ok(stream) => Some(watch_osd(stream.into_inner(), shared.clone())),
        Err(status) => {
            shared.error(status_error(status)); // the flight goes on without an OSD
            None
        }
    };
    let outcome = loop {
        tokio::select! {
            command = commands.recv() => match command {
                None => break Fly::Quit,
                Some(Command::Reload) => break Fly::Reload,
                Some(Command::Pause) => match client.pause(pb::Empty {}).await {
                    Ok(_) => shared.phase(Phase::Paused, ""),
                    Err(status) => shared.error(status_error(status)),
                },
                Some(Command::Resume) => match client.start(pb::Empty {}).await {
                    Ok(_) => shared.phase(Phase::Flying, ""),
                    Err(status) => shared.error(status_error(status)),
                },
                Some(Command::SetRadioLoss(on)) => {
                    let result = if on {
                        let fault = pb::Fault { kind: Some(pb::fault::Kind::RadioLinkLoss(pb::RadioLinkLoss {})) };
                        client.inject_fault(fault).await.map(|_| ())
                    } else {
                        client.clear_faults(pb::Empty {}).await.map(|_| ())
                    };
                    if let Err(status) = result {
                        shared.error(status_error(status));
                    }
                }
            },
            ended = &mut link.receiver => {
                break Fly::Failed(match ended {
                    Ok(Ok(())) => ClientError::new(ErrorKind::NotLoaded, "the server closed the pilot link"),
                    Ok(Err(error)) => error,
                    Err(join) => ClientError::new(ErrorKind::Internal, format!("the state receiver stopped: {join}")),
                });
            }
        }
    };
    if let Some(task) = osd_task {
        task.abort();
    }
    link.stop().await;
    outcome
}

enum End {
    Quit,
    Failed(ClientError),
}

/// One connection: reach the server, watch its events, then fly (reloading as often as asked).
async fn cycle(
    settings: &Settings,
    shared: &Arc<Shared>,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    server: &mut Option<ServerProcess>,
) -> End {
    shared.phase(Phase::Connecting, format!("connecting to {}", settings.server_addr));
    let mut client = match connect_or_launch(settings, shared, server).await {
        Ok(client) => client,
        Err(error) => return End::Failed(error),
    };
    // Watch first: a session the server sees watched ends when this client goes away.
    let watcher = match client.watch(pb::Empty {}).await {
        Ok(stream) => watch_events(stream.into_inner(), shared.clone()),
        Err(status) => return End::Failed(status_error(status)),
    };
    let end = loop {
        match fly(&mut client, shared, settings, commands).await {
            Fly::Reload => continue,
            Fly::Quit => break End::Quit,
            Fly::Failed(error) => break End::Failed(error),
        }
    };
    if server.is_some() {
        // Unload stops Betaflight SITL cleanly; killing a launched server first would orphan it on Windows.
        let _ = tokio::time::timeout(UNLOAD_TIMEOUT, client.unload(pb::Empty {})).await;
    }
    watcher.abort();
    end
}

/// The supervisor's whole life: cycles until the commands channel closes.
pub(crate) async fn run(settings: Settings, shared: Arc<Shared>, mut commands: mpsc::UnboundedReceiver<Command>) {
    let mut server: Option<ServerProcess> = None;
    loop {
        match cycle(&settings, &shared, &mut commands, &mut server).await {
            End::Quit => break,
            End::Failed(error) => {
                shared.failed(&error);
                // Wait for a reload (reconnect and try again) or for the end.
                loop {
                    match commands.recv().await {
                        Some(Command::Reload) => break,
                        Some(_) => {}
                        None => {
                            drop(server);
                            shared.phase(Phase::Stopped, "");
                            return;
                        }
                    }
                }
            }
        }
    }
    drop(server);
    shared.phase(Phase::Stopped, "");
}
