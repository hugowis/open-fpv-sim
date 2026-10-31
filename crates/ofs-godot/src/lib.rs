//! Godot 4 extension: `OfsClient`, a node that connects the pilot's game to `ofs-sim`.
//!
//! All the logic lives in `ofs-client`; this file converts between its plain types and Godot's. The client runs
//! on its own threads and never touches a Godot object: the node polls it once per frame (`process`) and turns
//! what happened into signals, so everything Godot sees happens on the main thread.
use std::path::PathBuf;
use std::time::Duration;

use godot::classes::{INode, Node};
use godot::prelude::*;
use ofs_client::dvr::DvrFeed;
use ofs_client::dvr_export::Exporter;
use ofs_client::{Client, Command, LaunchSpec, OsdFrame, OverrunPolicy, Settings, Sticks, Update};

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
    /// DVR export jobs; independent of any session, so a recording made before a reload still finishes exporting.
    exporter: Exporter,
}

#[godot_api]
impl INode for OfsClient {
    fn init(base: Base<Node>) -> Self {
        Self { base, client: None, exporter: Exporter::new() }
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

    /// Starts connecting; returns at once. Returns an empty string when it started, otherwise why it did not
    /// ("already started", or which setting is invalid). Keys: `quad_path` (required), `world_path` (empty or absent:
    /// the open field), `server_addr`, `server_bin` (a program to start when nothing answers), `data_dir`, `log_file`,
    /// `env` (Dictionary of String to String for the started server), `seed`, `open_loop`, `overrun_policy` ("warn" or
    /// "slow"), `state_rate_hz`, `stick_rate_hz`.
    #[func]
    fn start(&mut self, settings: VarDictionary) -> GString {
        if self.client.is_some() {
            return GString::from("already started");
        }
        let settings = match parse_settings(&settings) {
            Ok(settings) => settings,
            Err(message) => return GString::from(message.as_str()),
        };
        match Client::start(settings) {
            Ok(client) => {
                self.client = Some(client);
                GString::new()
            }
            Err(error) => GString::from(error.to_string().as_str()),
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

    /// Starts (true) or stops (false) recording the sticks and link state to a replayable CSV the server names;
    /// the `inputs_recording_started` / `inputs_recording_stopped` events say where it went.
    #[func]
    fn set_inputs_recording(&self, on: bool) {
        self.send(Command::SetInputsRecording(on));
    }

    /// Starts a fresh race: no clock, no laps, gate 0 next (a `race_reset` event confirms it), on the world's course
    /// called `course` (an empty String: the one being raced). An error without gates or for an unknown course.
    #[func]
    fn reset_race(&self, course: GString) {
        self.send(Command::ResetRace(course.to_string()));
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
    /// `tx_enabled`, `link_up`, `lq_pct`, `rssi_dbm`, `radio_snr_db`, `radio_antenna`, `downlink_lq_pct`,
    /// `fc_battery_v`, `fc_battery_a`, `fc_battery_mah`, `fc_roll_deg`, `fc_pitch_deg`, `fc_yaw_deg`,
    /// `fc_flight_mode`, `fc_frame_count`, `fc_telemetry_age_s`,
    /// `collision_speed_mps`, `running`, `overruns`, `fc_restarts`, `age_s`, and the VTX's
    /// `vtx_present`, `vtx_band`, `vtx_channel`, `vtx_channel_name` (a String, empty without a VTX), `vtx_freq_mhz`,
    /// `vtx_power_mw` and `vtx_pit_mode`, and the video link's `video_present`, `video_snr_db`,
    /// `video_interference_dbm`, `video_sync` ("locked", "unstable", "lost"; empty without a VTX), `video_noise`,
    /// `video_sparkles`, `video_chroma`, `video_field_valid`, `video_latency_ms`, `video_antenna`
    /// (the antenna in use) and `video_rssi` (a Dictionary of dBm by antenna name), and the lap clock's
    /// `race_present` (the world has gates), `race_running`, `race_finished` (the lap limit is reached), `race_lap_limit`
    /// (0 = none), `race_next_gate`, `race_gate_count`, `race_laps`,
    /// `race_lap_s` (the current lap so far), `race_last_lap_s` and `race_best_lap_s` (both 0 until a lap completes),
    /// `race_course` (a String, empty for a world without courses), `race_course_index`, `race_mode` ("laps" or
    /// "time_attack"), `race_time_limit_s` and `race_time_left_s` (time attack: the clock, and what is left of it).
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
        d.set("radio_snr_db", t.radio_snr_db);
        d.set("radio_antenna", &GString::from(t.radio_antenna.as_str()));
        d.set("downlink_lq_pct", t.downlink_lq_pct);
        let fc = &t.fc_telemetry;
        d.set("fc_battery_v", fc.battery_v);
        d.set("fc_battery_a", fc.battery_a);
        d.set("fc_battery_mah", fc.battery_mah);
        d.set("fc_roll_deg", fc.roll_deg);
        d.set("fc_pitch_deg", fc.pitch_deg);
        d.set("fc_yaw_deg", fc.yaw_deg);
        d.set("fc_flight_mode", &GString::from(fc.flight_mode.as_str()));
        d.set("fc_frame_count", fc.frame_count as i64);
        d.set("fc_telemetry_age_s", fc.telemetry_age_s);
        d.set("collision_speed_mps", t.collision_speed_mps);
        d.set("running", t.running);
        d.set("overruns", t.overruns as i64);
        d.set("fc_restarts", i64::from(t.fc_restarts));
        d.set("vtx_present", t.vtx.present);
        d.set("vtx_band", i64::from(t.vtx.band));
        d.set("vtx_channel", i64::from(t.vtx.channel));
        d.set("vtx_channel_name", &GString::from(t.vtx.channel_name().as_str()));
        d.set("vtx_freq_mhz", i64::from(t.vtx.freq_mhz));
        d.set("vtx_power_mw", i64::from(t.vtx.power_mw));
        d.set("vtx_pit_mode", t.vtx.pit_mode);
        let v = &t.video;
        d.set("video_present", v.present);
        d.set("video_snr_db", v.snr_db);
        d.set("video_interference_dbm", v.interference_dbm);
        d.set("video_sync", &GString::from(v.sync.as_str()));
        d.set("video_noise", v.noise);
        d.set("video_sparkles", v.sparkles);
        d.set("video_chroma", v.chroma);
        d.set("video_field_valid", v.field_valid);
        d.set("video_latency_ms", v.latency_ms);
        d.set("video_antenna", &GString::from(v.active_antenna.as_str()));
        let mut rssi = VarDictionary::new();
        for (name, dbm) in &v.rssi_dbm {
            rssi.set(&GString::from(name.as_str()), *dbm);
        }
        d.set("video_rssi", &rssi);
        let r = &t.race;
        d.set("race_present", r.present);
        d.set("race_running", r.running);
        d.set("race_finished", r.finished);
        d.set("race_lap_limit", i64::from(r.lap_limit));
        d.set("race_next_gate", i64::from(r.next_gate));
        d.set("race_gate_count", i64::from(r.gate_count));
        d.set("race_laps", i64::from(r.laps));
        d.set("race_lap_s", r.current_lap_s);
        d.set("race_last_lap_s", r.last_lap_s);
        d.set("race_best_lap_s", r.best_lap_s);
        d.set("race_course", &GString::from(r.course.as_str()));
        d.set("race_course_index", i64::from(r.course_index));
        d.set("race_mode", &GString::from(r.mode.as_str()));
        d.set("race_time_limit_s", r.time_limit_s);
        d.set("race_time_left_s", r.time_left_s);
        d.set("age_s", t.age_s);
        d
    }

    /// The FPV picture for this frame — the goggles' pipeline: the pose, video uniforms and OSD of one delayed,
    /// field-quantized instant; a dropped field holds the previous one. Empty before the first field: `pose`
    /// (Transform3D), `render_time_s`, `video_present`, `video_noise`, `video_sparkles`, `video_chroma`,
    /// `video_sync` ("locked"/"unstable"/"lost"), `field_valid`, `held_fields` (how many field boundaries the
    /// shown field lags the render target by; 0 is current, ≥ 1 means held fields), `target_field` (the integer
    /// index of the field boundary the picture renders for — what the DVR dedups on), `latency_ms`, and `osd`
    /// (the Dictionary `get_osd` returns, or empty).
    #[func]
    fn get_fpv_view(&self) -> VarDictionary {
        let mut d = VarDictionary::new();
        let Some(v) = self.client.as_ref().and_then(|c| c.fpv_view()) else { return d };
        let rotation = Quaternion::new(v.pose.att.x as f32, v.pose.att.y as f32, v.pose.att.z as f32, v.pose.att.w as f32);
        let origin = Vector3::new(v.pose.pos.x as f32, v.pose.pos.y as f32, v.pose.pos.z as f32);
        d.set("pose", Transform3D::new(Basis::from_quaternion(rotation), origin));
        d.set("render_time_s", v.render_time_s);
        d.set("video_present", v.uniforms.present);
        d.set("video_noise", v.uniforms.noise);
        d.set("video_sparkles", v.uniforms.sparkles);
        d.set("video_chroma", v.uniforms.chroma);
        d.set("video_sync", &GString::from(v.uniforms.sync.as_str()));
        d.set("field_valid", v.field_valid);
        d.set("held_fields", i64::from(v.held_fields));
        d.set("target_field", ofs_client::fpv::target_field(v.render_time_s, v.held_fields));
        d.set("latency_ms", v.latency_ms);
        match &v.osd {
            Some(frame) => d.set("osd", &osd_dict(frame)),
            None => d.set("osd", &VarDictionary::new()),
        }
        d
    }

    /// Starts a DVR recording of `feed` ("goggles" or "clean") to `path` (an absolute .avi path) at the given pixel
    /// size; "" or why not.
    #[func]
    fn dvr_start(&self, feed: GString, path: GString, width: i64, height: i64) -> GString {
        let Some(client) = self.client.as_ref() else { return GString::from("no session") };
        let name = feed.to_string();
        let Some(feed) = DvrFeed::parse(&name) else { return GString::from(format!("unknown feed {name}").as_str()) };
        match client.dvr_start(feed, path.to_string(), width.max(1) as u32, height.max(1) as u32) {
            Ok(()) => GString::new(),
            Err(e) => GString::from(e.as_str()),
        }
    }

    #[func]
    fn dvr_write_frame(&self, feed: GString, jpeg: PackedByteArray) {
        if let (Some(client), Some(feed)) = (self.client.as_ref(), DvrFeed::parse(&feed.to_string())) {
            client.dvr_write_frame(feed, jpeg.to_vec());
        }
    }

    /// Stops the `feed` recording: `{frames, dropped_frames, bytes}`, or empty when it was not recording.
    #[func]
    fn dvr_stop(&self, feed: GString) -> VarDictionary {
        let mut d = VarDictionary::new();
        let Some(client) = self.client.as_ref() else { return d };
        let Some(feed) = DvrFeed::parse(&feed.to_string()) else { return d };
        let Ok(summary) = client.dvr_stop(feed) else { return d };
        d.set("frames", summary.frames as i64);
        d.set("dropped_frames", summary.dropped_frames as i64);
        d.set("bytes", summary.bytes as i64);
        d
    }

    /// Exports the finished recording `path` (an absolute .avi path) in the background: `mode` is "frames" (an image
    /// sequence next to it), "mp4" (needs ffmpeg on the PATH) or "both". "" when it started, otherwise why not.
    /// Results arrive through `dvr_export_poll`.
    #[func]
    fn dvr_export(&self, path: GString, mode: GString) -> GString {
        let (frames, mp4) = match mode.to_string().as_str() {
            "frames" => (true, false),
            "mp4" => (false, true),
            "both" => (true, true),
            other => return GString::from(format!("unknown export mode {other}").as_str()),
        };
        self.exporter.spawn(PathBuf::from(path.to_string()), frames, mp4);
        GString::new()
    }

    /// The exports that finished since the last call: an Array of `{path, kind ("frames" or "mp4"), ok, detail}`
    /// (`detail` is the output path, or the error).
    #[func]
    fn dvr_export_poll(&self) -> VarArray {
        let mut out = VarArray::new();
        for done in self.exporter.poll() {
            let mut d = VarDictionary::new();
            d.set("path", &GString::from(done.avi.to_string_lossy().as_ref()));
            d.set("kind", &GString::from(done.kind));
            d.set("ok", done.result.is_ok());
            let detail = match &done.result {
                Ok(path) => path.to_string_lossy().into_owned(),
                Err(e) => e.clone(),
            };
            d.set("detail", &GString::from(detail.as_str()));
            out.push(&d.to_variant());
        }
        out
    }

    /// The newest OSD frame: `seq`, `time_s`, `present`, `cols`, `rows` and `cells` (a PackedInt32Array, row-major,
    /// each `char | font_page << 8 | blink << 10`); an empty Dictionary before the first frame.
    #[func]
    fn get_osd(&self) -> VarDictionary {
        let Some(frame) = self.client.as_ref().and_then(|c| c.osd()) else { return VarDictionary::new() };
        osd_dict(&frame)
    }

    /// Counts OSD updates; when it changes, `get_osd()` has something new to draw.
    #[func]
    fn get_osd_version(&self) -> i64 {
        self.client.as_ref().map_or(0, |c| c.osd_version() as i64)
    }

    /// The loaded session's world in Godot's frame, or an empty Dictionary before it arrives: `name`,
    /// `pilot_position` (Vector3), `antennas` (Array of {`name`, `kind`, `aim`: Vector3}), `objects` (Array of {`name`,
    /// `shape` ("box" or "cylinder"), `center`: Vector3, `size`: Vector3 (a box's), `radius`, `height` (a cylinder's),
    /// `color`: Color, `rf_loss_db`}), `emitters` (Array of {`name`, `position`: Vector3, `freq_mhz`, `power_mw`}) and
    /// `gates` (Array of {`name`, `center`: Vector3 (the middle of the opening), `heading_deg` (the direction of travel,
    /// clockwise from north), `width`, `height`, `pitch_deg`, `roll_deg` (the tilt after the heading)}) as declared,
    /// `courses` (Array of {`name`, `gates`: Array of gate names in course order, `mode` ("laps" or "time_attack"),
    /// `laps` (0 = none), `time_limit_s`}; empty for a world without `[[courses]]`, which races all `gates` in order)
    /// and `default_course` (the index a fresh session starts in).
    #[func]
    fn get_world(&self) -> VarDictionary {
        let mut d = VarDictionary::new();
        let Some(world) = self.client.as_ref().and_then(|c| c.world()) else { return d };
        let mut antennas = VarArray::new();
        for a in &world.antennas {
            let mut e = VarDictionary::new();
            e.set("name", &GString::from(a.name.as_str()));
            e.set("kind", &GString::from(a.kind.as_str()));
            e.set("aim", vector3(a.aim.x, a.aim.y, a.aim.z));
            antennas.push(&e.to_variant());
        }
        let mut objects = VarArray::new();
        for o in &world.objects {
            let mut e = VarDictionary::new();
            e.set("name", &GString::from(o.name.as_str()));
            e.set("shape", &GString::from(o.shape.as_str()));
            e.set("center", vector3(o.center.x, o.center.y, o.center.z));
            e.set("size", vector3(o.size.x, o.size.y, o.size.z));
            e.set("radius", o.radius_m);
            e.set("height", o.height_m);
            e.set("color", Color::from_rgb(o.color[0] as f32, o.color[1] as f32, o.color[2] as f32));
            e.set("rf_loss_db", o.rf_loss_db);
            objects.push(&e.to_variant());
        }
        let mut emitters = VarArray::new();
        for x in &world.emitters {
            let mut e = VarDictionary::new();
            e.set("name", &GString::from(x.name.as_str()));
            e.set("position", vector3(x.position.x, x.position.y, x.position.z));
            e.set("freq_mhz", x.freq_mhz);
            e.set("power_mw", x.power_mw);
            emitters.push(&e.to_variant());
        }
        let mut gates = VarArray::new();
        for g in &world.gates {
            let mut e = VarDictionary::new();
            e.set("name", &GString::from(g.name.as_str()));
            e.set("center", vector3(g.center.x, g.center.y, g.center.z));
            e.set("heading_deg", g.heading_deg);
            e.set("width", g.width_m);
            e.set("height", g.height_m);
            e.set("pitch_deg", g.pitch_deg);
            e.set("roll_deg", g.roll_deg);
            gates.push(&e.to_variant());
        }
        let mut handset_antennas = VarArray::new();
        for a in &world.handset_antennas {
            let mut e = VarDictionary::new();
            e.set("name", &GString::from(a.name.as_str()));
            e.set("kind", &GString::from(a.kind.as_str()));
            e.set("aim", vector3(a.aim.x, a.aim.y, a.aim.z));
            handset_antennas.push(&e.to_variant());
        }
        d.set("name", &GString::from(world.name.as_str()));
        d.set("pilot_position", vector3(world.pilot_position.x, world.pilot_position.y, world.pilot_position.z));
        d.set("antennas", &antennas);
        d.set("handset_position", vector3(world.handset_position.x, world.handset_position.y, world.handset_position.z));
        d.set("handset_antennas", &handset_antennas);
        d.set("objects", &objects);
        d.set("emitters", &emitters);
        d.set("gates", &gates);
        let mut courses = VarArray::new();
        for c in &world.courses {
            let mut e = VarDictionary::new();
            let mut names = VarArray::new();
            for g in &c.gates {
                names.push(&GString::from(g.as_str()).to_variant());
            }
            e.set("name", &GString::from(c.name.as_str()));
            e.set("gates", &names);
            e.set("mode", &GString::from(c.mode.as_str()));
            e.set("laps", i64::from(c.laps));
            e.set("time_limit_s", c.time_limit_s);
            courses.push(&e.to_variant());
        }
        d.set("courses", &courses);
        d.set("default_course", i64::from(world.default_course));
        d
    }

    /// Counts world updates; when it changes, `get_world()` has a new world (or none, while a load is under way).
    #[func]
    fn get_world_version(&self) -> i64 {
        self.client.as_ref().map_or(0, |c| c.world_version() as i64)
    }
}

impl OfsClient {
    fn send(&self, command: Command) {
        if let Some(client) = &self.client {
            client.send(command);
        }
    }
}

fn osd_dict(frame: &OsdFrame) -> VarDictionary {
    let cells: Vec<i32> = frame.cells.iter().map(|c| *c as i32).collect();
    let mut d = VarDictionary::new();
    d.set("seq", frame.seq as i64);
    d.set("time_s", frame.time_s);
    d.set("present", frame.present);
    d.set("cols", i64::from(frame.cols));
    d.set("rows", i64::from(frame.rows));
    d.set("cells", &PackedInt32Array::from(cells.as_slice()));
    d
}

fn vector3(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x as f32, y as f32, z as f32)
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
    if let Some(world) = string_key(d, "world_path") {
        settings.world_path = world;
    }
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
