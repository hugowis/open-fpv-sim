//! Godot 4 extension: `OfsClient`, a node that connects the pilot's game to `ofs-sim`.
//!
//! All the logic lives in `ofs-client`; this file converts between its plain types and Godot's. The client runs
//! on its own threads and never touches a Godot object: the node polls it once per frame (`process`) and turns
//! what happened into signals, so everything Godot sees happens on the main thread.
use std::path::PathBuf;
use std::time::Duration;

use godot::classes::{INode, Node};
use godot::prelude::*;
use ofs_client::{Client, Command, LaunchSpec, OverrunPolicy, Settings, Sticks, Update};

struct OfsExtension;

#[gdextension]
unsafe impl ExtensionLibrary for OfsExtension {}

/// The pilot's link to the simulator. Call `start(settings)`, feed it `set_sticks` every frame, read `get_pose`
/// and `get_telemetry`, and react to its signals. See `godot/scripts/app.gd` for the whole loop.
#[derive(GodotClass)]
#[class(base = Node)]
pub struct OfsClient {
    base: Base<Node>,
    client: Option<Client>,
}

#[godot_api]
impl INode for OfsClient {
    fn init(base: Base<Node>) -> Self {
        Self { base, client: None }
    }

    fn process(&mut self, _delta: f64) {
        let updates = match &self.client {
            Some(client) => client.poll(),
            None => return,
        };
        for update in updates {
            match update {
                Update::Phase { phase, detail, kind } => {
                    let kind = kind.map(|k| k.as_str()).unwrap_or("");
                    self.signals().phase_changed().emit(&GString::from(phase.as_str()), &GString::from(detail.as_str()), &GString::from(kind));
                }
                Update::Session { quad_name, configurator_address } => {
                    self.signals().session_ready().emit(&GString::from(quad_name.as_str()), &GString::from(configurator_address.as_str()));
                }
                Update::Event(event) => {
                    self.signals().event_received().emit(&GString::from(event.kind.as_str()), &GString::from(event.message.as_str()), event.time_s);
                }
                Update::Error(error) => {
                    self.signals().request_failed().emit(&GString::from(error.kind.as_str()), &GString::from(error.message.as_str()));
                }
            }
        }
    }

    fn exit_tree(&mut self) {
        self.stop();
    }
}

#[godot_api]
impl OfsClient {
    /// The phase changed: "connecting", "loading", "flying", "paused", "failed" or "stopped". `detail` says more
    /// (for "failed", the error message) and `kind` is the error kind for "failed", empty otherwise.
    #[signal]
    fn phase_changed(phase: GString, detail: GString, kind: GString);

    /// The quad is loaded. `configurator_address` (like `tcp://127.0.0.1:5761`) is empty without Betaflight.
    #[signal]
    fn session_ready(quad_name: GString, configurator_address: GString);

    /// A simulator event: "link_down", "link_up", "overrun", "firmware_restarted", "sim_error", ...
    #[signal]
    fn event_received(kind: GString, message: GString, time_s: f64);

    /// A request failed without ending the flight (for example a pause the server refused).
    #[signal]
    fn request_failed(kind: GString, message: GString);

    /// Starts connecting; returns at once. Returns false (and logs why) when already started or the settings are
    /// invalid. Keys: `quad_path` (required), `server_addr`, `server_bin` (a program to start when nothing
    /// answers), `data_dir`, `log_file`, `env` (Dictionary of String to String for the started server), `seed`,
    /// `open_loop`, `overrun_policy` ("warn" or "slow"), `state_rate_hz`, `stick_rate_hz`.
    #[func]
    fn start(&mut self, settings: VarDictionary) -> bool {
        if self.client.is_some() {
            godot_error!("OfsClient.start: already started");
            return false;
        }
        let settings = match parse_settings(&settings) {
            Ok(settings) => settings,
            Err(message) => {
                godot_error!("OfsClient.start: {message}");
                return false;
            }
        };
        match Client::start(settings) {
            Ok(client) => {
                self.client = Some(client);
                true
            }
            Err(error) => {
                godot_error!("OfsClient.start: {error}");
                false
            }
        }
    }

    /// Ends the session and stops a server this node started. Also done when the node leaves the tree.
    #[func]
    fn stop(&mut self) {
        if let Some(mut client) = self.client.take() {
            client.shutdown();
        }
    }

    /// The transmitter's sticks: roll, pitch, yaw in [-1, 1], throttle in [0, 1], up to 4 aux switches in [-1, 1].
    #[func]
    fn set_sticks(&self, roll: f64, pitch: f64, yaw: f64, throttle: f64, aux: PackedFloat32Array) {
        let Some(client) = &self.client else { return };
        let mut sticks = Sticks { roll, pitch, yaw, throttle, ..Sticks::default() };
        for (slot, value) in sticks.aux.iter_mut().zip(aux.as_slice()) {
            *slot = f64::from(*value);
        }
        client.set_sticks(sticks);
    }

    #[func]
    fn pause(&self) {
        self.send(Command::Pause);
    }

    #[func]
    fn resume(&self) {
        self.send(Command::Resume);
    }

    /// Loads the quad again (the drone is back at its start), or retries after a failure.
    #[func]
    fn reload(&self) {
        self.send(Command::Reload);
    }

    /// Cuts (true) or restores (false) the radio link: the failsafe test.
    #[func]
    fn set_radio_loss(&self, on: bool) {
        self.send(Command::SetRadioLoss(on));
    }

    /// The current phase (see `phase_changed`).
    #[func]
    fn get_phase(&self) -> GString {
        GString::from(self.client.as_ref().map_or("stopped", |c| c.phase().0.as_str()))
    }

    #[func]
    fn get_phase_detail(&self) -> GString {
        GString::from(self.client.as_ref().map(|c| c.phase().1).unwrap_or_default().as_str())
    }

    #[func]
    fn get_configurator_address(&self) -> GString {
        GString::from(self.client.as_ref().map(|c| c.configurator_address()).unwrap_or_default().as_str())
    }

    #[func]
    fn get_quad_name(&self) -> GString {
        GString::from(self.client.as_ref().map(|c| c.quad_name()).unwrap_or_default().as_str())
    }

    /// True once the first vehicle state has arrived.
    #[func]
    fn has_pose(&self) -> bool {
        self.client.as_ref().is_some_and(|c| c.pose().is_some())
    }

    /// Where to draw the vehicle now, in Godot's frame (+Y up, forward is -Z), smoothed between state messages.
    /// The identity transform before the first state arrives.
    #[func]
    fn get_pose(&self) -> Transform3D {
        let Some(pose) = self.client.as_ref().and_then(|c| c.pose()) else { return Transform3D::IDENTITY };
        let rotation = Quaternion::new(pose.att.x as f32, pose.att.y as f32, pose.att.z as f32, pose.att.w as f32);
        let origin = Vector3::new(pose.pos.x as f32, pose.pos.y as f32, pose.pos.z as f32);
        Transform3D::new(Basis::from_quaternion(rotation), origin)
    }

    /// The newest telemetry, or an empty Dictionary before the first state: `time_s`, `altitude_m`, `speed_mps`,
    /// `climb_mps`, `battery_voltage_v`, `battery_current_a`, `motor_cmd` (PackedFloat32Array), `motors_spinning`,
    /// `tx_enabled`, `link_up`, `lq_pct`, `rssi_dbm`, `running`, `overruns`, `fc_restarts` and `age_s`.
    #[func]
    fn get_telemetry(&self) -> VarDictionary {
        let mut d = VarDictionary::new();
        let Some(t) = self.client.as_ref().and_then(|c| c.telemetry()) else { return d };
        let motors: Vec<f32> = t.motor_cmd.iter().map(|m| *m as f32).collect();
        d.set("time_s", t.time_s);
        d.set("altitude_m", t.altitude_m);
        d.set("speed_mps", t.speed_mps);
        d.set("climb_mps", t.climb_mps);
        d.set("battery_voltage_v", t.battery_voltage_v);
        d.set("battery_current_a", t.battery_current_a);
        d.set("motor_cmd", &PackedFloat32Array::from(motors.as_slice()));
        d.set("motors_spinning", t.motors_spinning);
        d.set("tx_enabled", t.tx_enabled);
        d.set("link_up", t.link_up);
        d.set("lq_pct", t.lq_pct);
        d.set("rssi_dbm", t.rssi_dbm);
        d.set("running", t.running);
        d.set("overruns", t.overruns as i64);
        d.set("fc_restarts", i64::from(t.fc_restarts));
        d.set("age_s", t.age_s);
        d
    }
}

impl OfsClient {
    fn send(&self, command: Command) {
        if let Some(client) = &self.client {
            client.send(command);
        }
    }
}

fn string_key(d: &VarDictionary, key: &str) -> Option<String> {
    d.get(key).and_then(|v| v.try_to::<GString>().ok()).map(|s| s.to_string()).filter(|s| !s.is_empty())
}

fn number_key(d: &VarDictionary, key: &str) -> Option<i64> {
    d.get(key).and_then(|v| v.try_to::<i64>().ok())
}

fn parse_settings(d: &VarDictionary) -> Result<Settings, String> {
    let quad_path = string_key(d, "quad_path").ok_or("`quad_path` is required")?;
    let mut settings = Settings::new(quad_path);
    if let Some(addr) = string_key(d, "server_addr") {
        settings.server_addr = addr;
    }
    if let Some(program) = string_key(d, "server_bin") {
        let mut env = Vec::new();
        if let Some(vars) = d.get("env").and_then(|v| v.try_to::<VarDictionary>().ok()) {
            for (key, value) in vars.iter_shared() {
                let (Ok(key), Ok(value)) = (key.try_to::<GString>(), value.try_to::<GString>()) else {
                    return Err("`env` must map strings to strings".into());
                };
                env.push((key.to_string(), value.to_string()));
            }
        }
        settings.launch = Some(LaunchSpec {
            program: PathBuf::from(program),
            data_dir: PathBuf::from(string_key(d, "data_dir").unwrap_or_else(|| ".ofs-data".into())),
            env,
            log_file: string_key(d, "log_file").map(PathBuf::from),
        });
    }
    if let Some(seed) = number_key(d, "seed") {
        settings.seed = u64::try_from(seed).map_err(|_| "`seed` must not be negative")?;
    }
    if let Some(open_loop) = d.get("open_loop").and_then(|v| v.try_to::<bool>().ok()) {
        settings.open_loop_fc = open_loop;
    }
    match string_key(d, "overrun_policy").as_deref() {
        None | Some("warn") => settings.overrun_policy = OverrunPolicy::Warn,
        Some("slow") => settings.overrun_policy = OverrunPolicy::Slow,
        Some(other) => return Err(format!("`overrun_policy` must be \"warn\" or \"slow\", not \"{other}\"")),
    }
    if let Some(hz) = number_key(d, "state_rate_hz") {
        settings.state_rate_hz = u32::try_from(hz).map_err(|_| "`state_rate_hz` is out of range")?;
    }
    if let Some(hz) = number_key(d, "stick_rate_hz") {
        settings.stick_rate_hz = u32::try_from(hz).map_err(|_| "`stick_rate_hz` is out of range")?;
    }
    if let Some(seconds) = number_key(d, "launch_timeout_s") {
        settings.launch_timeout = Duration::from_secs(u64::try_from(seconds).map_err(|_| "`launch_timeout_s` is out of range")?);
    }
    Ok(settings)
}
