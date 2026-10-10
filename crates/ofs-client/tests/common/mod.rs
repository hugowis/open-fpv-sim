//! An in-process `ofs-sim` server (open-loop sessions, no Betaflight) and helpers for driving a `Client` in tests.
#![allow(dead_code)]
#![allow(clippy::result_large_err)] // tonic's `Status`, fixed by the gRPC traits
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ofs_client::{Client, EventKind, Phase, Settings, Update};
use ofs_proto::pb::sim_client::SimClient;
use ofs_proto::pb::sim_server::SimServer;
use ofs_proto::pb::{self, Event};
use ofs_sim::server::SimService;
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::transport::Channel;

pub const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

static COUNTER: AtomicUsize = AtomicUsize::new(0);

pub struct TestServer {
    pub addr: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl TestServer {
    pub fn start() -> TestServer {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let data_dir = std::env::temp_dir().join(format!("ofs-client-test-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
            let service = runtime.block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                ready_tx.send(listener.local_addr().unwrap().to_string()).unwrap();
                let service = SimService::new(data_dir);
                let server = tonic::transport::Server::builder()
                    .add_service(SimServer::new(service.clone()))
                    .serve_with_incoming(TcpListenerStream::new(listener));
                tokio::select! {
                    _ = server => {}
                    _ = stop_rx => {}
                }
                service
            });
            // Abrupt, like a crashed server: open streams die with the runtime. Only then is the session stopped; the
            // other order let a client see its session unloaded (not_loaded) before the connection dropped.
            runtime.shutdown_background();
            service.shutdown();
        });
        TestServer { addr: ready_rx.recv().unwrap(), stop: Some(stop_tx), thread: Some(thread) }
    }

    /// Settings for an open-loop session on this server.
    pub fn settings(&self) -> Settings {
        Settings { server_addr: self.addr.clone(), open_loop_fc: true, ..Settings::new(QUAD) }
    }

    /// Takes the server down at once.
    pub fn kill(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }

    pub fn raw(&self) -> Raw {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let client = runtime.block_on(SimClient::connect(format!("http://{}", self.addr))).unwrap();
        Raw { runtime, client }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.kill();
    }
}

/// A plain gRPC client for checks the `Client` does not expose.
pub struct Raw {
    runtime: tokio::runtime::Runtime,
    pub client: SimClient<Channel>,
}

impl Raw {
    pub fn get_state(&mut self) -> Result<pb::State, tonic::Status> {
        self.runtime.block_on(self.client.get_state(pb::Empty {})).map(|r| r.into_inner())
    }

    pub fn unload(&mut self) {
        self.runtime.block_on(self.client.unload(pb::Empty {})).unwrap();
    }

    /// Opens an event stream (a watcher keeps a session alive when the pilot's client goes away).
    pub fn watch(&mut self) -> tonic::Streaming<Event> {
        self.runtime.block_on(self.client.watch(pb::Empty {})).unwrap().into_inner()
    }

    /// Tries to become the pilot.
    pub fn try_pilot(&mut self) -> Result<(), tonic::Status> {
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        tx.try_send(pb::PilotInput { sticks: Some(pb::Sticks::default()), state_rate_hz: 60 }).unwrap();
        self.runtime.block_on(self.client.pilot(ReceiverStream::new(rx))).map(|_| ())
    }
}

/// A client plus everything it has reported.
pub struct Probe {
    pub client: Client,
    pub log: Vec<Update>,
}

impl Probe {
    pub fn start(settings: Settings) -> Probe {
        Probe { client: Client::start(settings).unwrap(), log: Vec::new() }
    }

    pub fn pump(&mut self) {
        let updates = self.client.poll();
        self.log.extend(updates);
    }

    /// Polls until `condition` holds; panics with the update log when it does not within `timeout`.
    pub fn wait(&mut self, what: &str, timeout: Duration, condition: impl Fn(&Probe) -> bool) {
        let deadline = Instant::now() + timeout;
        loop {
            self.pump();
            if condition(self) {
                return;
            }
            if Instant::now() > deadline {
                panic!("timed out after {timeout:?} waiting for {what}\nphase: {:?}\nlog: {:#?}", self.client.phase(), self.log);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn wait_phase(&mut self, phase: Phase, timeout: Duration) {
        self.wait(&format!("phase {phase:?}"), timeout, |p| p.client.phase().0 == phase);
    }

    pub fn event_kinds(&self) -> Vec<EventKind> {
        self.log.iter().filter_map(|u| if let Update::Event(e) = u { Some(e.kind) } else { None }).collect()
    }

    pub fn phases(&self) -> Vec<Phase> {
        self.log.iter().filter_map(|u| if let Update::Phase { phase, .. } = u { Some(*phase) } else { None }).collect()
    }
}

pub const LONG: Duration = Duration::from_secs(15);
pub const SHORT: Duration = Duration::from_secs(5);
