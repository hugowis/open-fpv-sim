use ofs_core::{names, Bus, Model, StepCtx, Wire};
use ofs_video::smartaudio::*;
use ofs_video::vtx::{band_index, VtxModel, VtxParams, FREQUENCIES_MHZ};

fn params() -> VtxParams {
    VtxParams {
        power_levels_mw: vec![25, 200, 600, 1000],
        power_levels_dbm: vec![14, 23, 28, 30],
        default_band: band_index("R").unwrap(),
        default_channel: 1,
        default_power_index: 1,
        reply_latency_s: 0.005,
    }
}

struct Rig {
    model: VtxModel,
    bus: Bus,
    requests: Wire,
    replies: Wire,
}

impl Rig {
    fn new() -> Rig {
        let mut bus = Bus::new();
        let requests = Wire::new(1024);
        let replies = Wire::new(1024);
        let model = VtxModel::new(params(), requests.clone(), replies.clone(), 1, &mut bus);
        Rig { model, bus, requests, replies }
    }

    fn step(&mut self, t: f64) {
        self.model.step(&StepCtx { tick: 0, time_s: t, dt_s: 0.001 }, &mut self.bus).unwrap();
    }

    fn signal(&self, name: &str) -> f64 {
        self.bus.get(self.bus.lookup::<f64>(name).unwrap())
    }

    /// Sends a request frame at `t`, steps past the reply latency and returns the reply bytes.
    fn ask(&mut self, t: f64, frame: &[u8]) -> Vec<u8> {
        self.requests.write(frame);
        self.step(t);
        self.step(t + 0.006);
        self.replies.take(1024)
    }
}

const GOLDEN_DEFAULT_SETTINGS: [u8; 17] =
    [0xAA, 0x55, 0x11, 0x0C, 0x20, 0x17, 0x10, 0x16, 0x1A, 0x17, 0x04, 0x00, 0x0E, 0x17, 0x1C, 0x1E, 0xB5];

#[test]
fn the_defaults_are_published_on_the_bus() {
    let mut rig = Rig::new();
    rig.step(0.0);
    assert_eq!(rig.signal(names::VTX_PRESENT), 1.0);
    assert_eq!((rig.signal(names::VTX_BAND), rig.signal(names::VTX_CHANNEL)), (5.0, 1.0));
    assert_eq!(rig.signal(names::VTX_FREQ_MHZ), 5658.0);
    assert_eq!(rig.signal(names::VTX_POWER_MW), 200.0);
    assert_eq!(rig.signal(names::VTX_PIT), 0.0);
}

#[test]
fn get_settings_is_answered_after_the_latency_with_the_golden_frame() {
    let mut rig = Rig::new();
    rig.requests.write(&request_frame(CMD_GET_SETTINGS, &[]));
    rig.step(0.0);
    assert!(rig.replies.take(1024).is_empty(), "the VTX has not answered yet");
    rig.step(0.006);
    assert_eq!(rig.replies.take(1024), GOLDEN_DEFAULT_SETTINGS);
}

#[test]
fn set_channel_moves_the_vtx_and_echoes_the_channel() {
    let mut rig = Rig::new();
    let reply = rig.ask(0.0, &request_frame(CMD_SET_CHANNEL, &[34])); // band R (index 4), channel 3
    assert_eq!(reply, response_frame(CMD_SET_CHANNEL, &[34]));
    assert_eq!(rig.signal(names::VTX_FREQ_MHZ), f64::from(FREQUENCIES_MHZ[4][2]));
    assert_eq!((rig.signal(names::VTX_BAND), rig.signal(names::VTX_CHANNEL)), (5.0, 3.0));
}

#[test]
fn set_power_in_dbm_selects_the_level() {
    let mut rig = Rig::new();
    let reply = rig.ask(0.0, &request_frame(CMD_SET_POWER, &[0x80 | 28]));
    assert_eq!(reply, response_frame(CMD_SET_POWER, &[0x80 | 28]));
    assert_eq!(rig.signal(names::VTX_POWER_MW), 600.0);
    assert_eq!(rig.signal(names::VTX_PIT), 0.0);
}

#[test]
fn zero_dbm_is_pit_mode_and_clear_pit_leaves_it() {
    let mut rig = Rig::new();
    rig.ask(0.0, &request_frame(CMD_SET_POWER, &[0x80]));
    assert_eq!(rig.signal(names::VTX_PIT), 1.0);
    assert_eq!(rig.signal(names::VTX_POWER_MW), 200.0, "the reported level does not change");
    let settings = rig.ask(0.1, &request_frame(CMD_GET_SETTINGS, &[]));
    assert_eq!(settings[6] & MODE_PIT, MODE_PIT, "settings report pit mode: {settings:02x?}");
    rig.ask(0.2, &request_frame(CMD_SET_MODE, &[SET_MODE_CLEAR_PIT | SET_MODE_UNLOCK]));
    assert_eq!(rig.signal(names::VTX_PIT), 0.0);
}

#[test]
fn set_frequency_enters_user_frequency_mode_and_a_channel_leaves_it() {
    let mut rig = Rig::new();
    rig.ask(0.0, &request_frame(CMD_SET_FREQUENCY, &[0x16, 0xA8])); // 5800 MHz
    assert_eq!(rig.signal(names::VTX_FREQ_MHZ), 5800.0);
    assert_eq!((rig.signal(names::VTX_BAND), rig.signal(names::VTX_CHANNEL)), (0.0, 0.0));
    let settings = rig.ask(0.1, &request_frame(CMD_GET_SETTINGS, &[]));
    assert_eq!(settings[6] & MODE_FREQUENCY, MODE_FREQUENCY, "frequency-mode bit: {settings:02x?}");
    rig.ask(0.2, &request_frame(CMD_SET_CHANNEL, &[0]));
    assert_eq!(rig.signal(names::VTX_FREQ_MHZ), 5865.0, "band A channel 1");
}

#[test]
fn an_out_of_range_frequency_is_ignored_but_still_answered() {
    let mut rig = Rig::new();
    let reply = rig.ask(0.0, &request_frame(CMD_SET_FREQUENCY, &[0x0F, 0xA0])); // 4000 MHz
    assert!(!reply.is_empty());
    assert_eq!(rig.signal(names::VTX_FREQ_MHZ), 5658.0);
}

#[test]
fn locking_clears_the_unlocked_bit() {
    let mut rig = Rig::new();
    rig.ask(0.0, &request_frame(CMD_SET_MODE, &[0]));
    let settings = rig.ask(0.1, &request_frame(CMD_GET_SETTINGS, &[]));
    assert_eq!(settings[6] & MODE_UNLOCKED, 0, "{settings:02x?}");
}

#[test]
fn a_corrupted_request_gets_no_answer() {
    let mut rig = Rig::new();
    let mut frame = request_frame(CMD_SET_CHANNEL, &[3]);
    *frame.last_mut().unwrap() ^= 0xFF;
    assert!(rig.ask(0.0, &frame).is_empty());
    assert_eq!(rig.model.bad_frames(), 1);
    assert_eq!(rig.signal(names::VTX_CHANNEL), 1.0, "unchanged");
}
