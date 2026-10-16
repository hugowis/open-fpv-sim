use ofs_core::{names, Bus, Model, StepCtx, Wire};
use ofs_video::smartaudio::*;
use ofs_video::tramp::{self, Parser, Request as TrampRequest};
use ofs_video::vtx::{band_index, VtxKind, VtxModel, VtxParams, FREQUENCIES_MHZ};
use proptest::prelude::*;

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
    fn new(params: VtxParams, kind: VtxKind) -> Rig {
        let mut bus = Bus::new();
        let requests = Wire::new(1024);
        let replies = Wire::new(1024);
        let model = VtxModel::new(kind, params, requests.clone(), replies.clone(), 1, &mut bus);
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
    let mut rig = Rig::new(params(), VtxKind::Smartaudio);
    rig.step(0.0);
    assert_eq!(rig.signal(names::VTX_PRESENT), 1.0);
    assert_eq!((rig.signal(names::VTX_BAND), rig.signal(names::VTX_CHANNEL)), (5.0, 1.0));
    assert_eq!(rig.signal(names::VTX_FREQ_MHZ), 5658.0);
    assert_eq!(rig.signal(names::VTX_POWER_MW), 200.0);
    assert_eq!(rig.signal(names::VTX_PIT), 0.0);
}

#[test]
fn get_settings_is_answered_after_the_latency_with_the_golden_frame() {
    let mut rig = Rig::new(params(), VtxKind::Smartaudio);
    rig.requests.write(&request_frame(CMD_GET_SETTINGS, &[]));
    rig.step(0.0);
    assert!(rig.replies.take(1024).is_empty(), "the VTX has not answered yet");
    rig.step(0.006);
    assert_eq!(rig.replies.take(1024), GOLDEN_DEFAULT_SETTINGS);
}

#[test]
fn set_channel_moves_the_vtx_and_echoes_the_channel() {
    let mut rig = Rig::new(params(), VtxKind::Smartaudio);
    let reply = rig.ask(0.0, &request_frame(CMD_SET_CHANNEL, &[34])); // band R (index 4), channel 3
    assert_eq!(reply, response_frame(CMD_SET_CHANNEL, &[34]));
    assert_eq!(rig.signal(names::VTX_FREQ_MHZ), f64::from(FREQUENCIES_MHZ[4][2]));
    assert_eq!((rig.signal(names::VTX_BAND), rig.signal(names::VTX_CHANNEL)), (5.0, 3.0));
}

#[test]
fn set_power_in_dbm_selects_the_level() {
    let mut rig = Rig::new(params(), VtxKind::Smartaudio);
    let reply = rig.ask(0.0, &request_frame(CMD_SET_POWER, &[0x80 | 28]));
    assert_eq!(reply, response_frame(CMD_SET_POWER, &[0x80 | 28]));
    assert_eq!(rig.signal(names::VTX_POWER_MW), 600.0);
    assert_eq!(rig.signal(names::VTX_PIT), 0.0);
}

#[test]
fn zero_dbm_is_pit_mode_and_clear_pit_leaves_it() {
    let mut rig = Rig::new(params(), VtxKind::Smartaudio);
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
    let mut rig = Rig::new(params(), VtxKind::Smartaudio);
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
    let mut rig = Rig::new(params(), VtxKind::Smartaudio);
    let reply = rig.ask(0.0, &request_frame(CMD_SET_FREQUENCY, &[0x0F, 0xA0])); // 4000 MHz
    assert!(!reply.is_empty());
    assert_eq!(rig.signal(names::VTX_FREQ_MHZ), 5658.0);
}

#[test]
fn locking_clears_the_unlocked_bit() {
    let mut rig = Rig::new(params(), VtxKind::Smartaudio);
    rig.ask(0.0, &request_frame(CMD_SET_MODE, &[0]));
    let settings = rig.ask(0.1, &request_frame(CMD_GET_SETTINGS, &[]));
    assert_eq!(settings[6] & MODE_UNLOCKED, 0, "{settings:02x?}");
}

#[test]
fn a_corrupted_request_gets_no_answer() {
    let mut rig = Rig::new(params(), VtxKind::Smartaudio);
    let mut frame = request_frame(CMD_SET_CHANNEL, &[3]);
    *frame.last_mut().unwrap() ^= 0xFF;
    assert!(rig.ask(0.0, &frame).is_empty());
    assert_eq!(rig.model.bad_frames(), 1);
    assert_eq!(rig.signal(names::VTX_CHANNEL), 1.0, "unchanged");
}

fn tramp_frame(cmd: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = vec![0x0F, cmd];
    f.extend_from_slice(payload);
    f.resize(14, 0);
    f.push(tramp::crc(&f)); // crc itself excludes the 0x0F
    f.push(0x00);
    f
}

#[test]
fn the_tramp_crc_is_the_sum_of_bytes_one_to_thirteen() {
    // Pinned against the pinned Betaflight's vtx_tramp.c trampChecksum: 'F' 5800 MHz.
    let f = tramp_frame(b'F', &5800u16.to_le_bytes());
    assert_eq!(f[14], (0x46 + 0xA8 + 0x16) as u8, "0x0F itself is excluded");
    assert_eq!(f[15], 0x00);
}

#[test]
fn tramp_requests_decode_across_chunk_boundaries() {
    let mut p = Parser::default();
    let mut frames = Vec::new();
    frames.extend(tramp_frame(b'F', &5800u16.to_le_bytes()));
    frames.extend(tramp_frame(b'P', &200u16.to_le_bytes()));
    frames.extend(tramp_frame(b'I', &1u16.to_le_bytes()));
    frames.extend(tramp_frame(b'v', &[]));
    let mut got = Vec::new();
    for chunk in frames.chunks(5) {
        got.extend(p.push(chunk));
    }
    assert_eq!(
        got,
        vec![
            TrampRequest::SetFrequency(5800),
            TrampRequest::SetPowerMw(200),
            TrampRequest::SetActive(true),
            TrampRequest::GetSettings,
        ]
    );
    assert_eq!(p.bad_frames(), 0);
}

#[test]
fn tramp_garbage_resyncs_and_counts() {
    let mut p = Parser::default();
    let mut bad = tramp_frame(b'F', &5800u16.to_le_bytes());
    bad[14] ^= 0xFF; // a bad crc inside a frame's shape
    let mut garbage = vec![0x01, 0x02, 0x0F, 0x46, 0x00, 0x00, 0xFF, 0xFF]; // junk ahead of it
    garbage.extend_from_slice(&bad);
    p.push(&garbage);
    assert_eq!(p.bad_frames(), 1, "the bad frame counted");
    assert_eq!(p.push(&tramp_frame(b'v', &[])), vec![TrampRequest::GetSettings], "the parser resynced");
}

#[test]
fn tramp_responses_are_sixteen_bytes_with_the_same_crc() {
    // 'v' for R1 5658 MHz, 200 mW, not in pit: pinned by hand (ruling 1).
    let resp = tramp::response_frame(b'v', &[
        0x1A, 0x16, // freq 5658
        0xC8, 0x00, // configured power 200
        0x00,       // control mode
        0x00,       // pit off
        0xC8, 0x00, // actual power 200
        0, 0, 0, 0, // rest of the 12-byte payload
    ]);
    assert_eq!(resp.len(), 16);
    assert_eq!(resp[1], b'v');
    assert_eq!(resp[14], 0x36, "the checksum the FC recomputes");
    assert_eq!(resp[15], 0x00);
}

/// The file's `params()` with the Tramp table (the FC's built-in power list, ruling 3) and one step up.
fn tramp_params() -> VtxParams {
    VtxParams {
        power_levels_mw: vec![25, 100, 200, 400, 600],
        power_levels_dbm: vec![14, 20, 23, 26, 28],
        default_band: band_index("R").unwrap(),
        default_channel: 1,
        default_power_index: 2,
        reply_latency_s: 0.005,
    }
}

fn tramp_rig() -> Rig {
    Rig::new(tramp_params(), VtxKind::Tramp)
}

#[test]
fn the_tramp_vtx_answers_r_v_s_and_applies_f_p_i() {
    let mut rig = tramp_rig();

    // Init handshake: 'r' then 'v'.
    let reply = rig.ask(0.0, &tramp_frame(b'r', &[]));
    assert_eq!(reply.len(), 16, "the 'r' limits reply");
    assert_eq!(reply[1], b'r');
    assert_eq!(u16::from_le_bytes([reply[2], reply[3]]), 5000, "the FC discards a zero min_freq");

    let reply = rig.ask(0.5, &tramp_frame(b'v', &[]));
    assert_eq!(reply[1], b'v');
    assert_eq!(u16::from_le_bytes([reply[2], reply[3]]), 5658, "R1, the power-up default");
    assert_eq!(u16::from_le_bytes([reply[4], reply[5]]), 200, "default_power_index 2 of the tramp table");
    assert_eq!(reply[7], 0, "not in pit");

    // Frequency and power apply silently; pit mode flips with 'I' (0 = enter pit).
    rig.requests.write(&tramp_frame(b'F', &5800u16.to_le_bytes()));
    rig.requests.write(&tramp_frame(b'P', &600u16.to_le_bytes()));
    rig.requests.write(&tramp_frame(b'I', &0u16.to_le_bytes()));
    rig.step(1.0);
    rig.step(1.006);
    assert_eq!(rig.replies.take(1024).len(), 0, "F/P/I get no reply (ruling 2)");
    assert_eq!(rig.signal("vtx.freq_mhz"), 5800.0);
    assert_eq!(rig.signal("vtx.power_mw"), 600.0);
    assert_eq!(rig.signal("vtx.pit_mode"), 1.0);

    // The FC's confirmation query reflects everything, including the pit flag.
    let reply = rig.ask(1.5, &tramp_frame(b'v', &[]));
    assert_eq!(u16::from_le_bytes([reply[2], reply[3]]), 5800);
    assert_eq!(reply[7], 1, "pit on");

    let temperature = rig.bus.signal::<f64>(names::VTX_TEMP_C);
    rig.bus.set(temperature, 47.4);
    let reply = rig.ask(2.0, &tramp_frame(b's', &[]));
    assert_eq!(reply[1], b's');
    assert_eq!(i16::from_le_bytes([reply[6], reply[7]]), 47, "the VTX's temperature, rounded");
}

#[test]
fn tramp_status_encodes_a_negative_temperature() {
    let mut rig = Rig::new(tramp_params(), VtxKind::Tramp);
    let temperature = rig.bus.signal::<f64>(names::VTX_TEMP_C);
    rig.bus.set(temperature, -5.6);
    let reply = rig.ask(0.0, &tramp_frame(b's', &[]));
    assert_eq!(reply[1], b's');
    assert_eq!(i16::from_le_bytes([reply[6], reply[7]]), -6);
}

proptest! {
    #[test]
    fn tramp_never_panics_on_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
        let mut p = Parser::default();
        let _ = p.push(&bytes);
    }
}
