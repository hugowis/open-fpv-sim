//! The video transmitter, controlled by Betaflight over SmartAudio v2.1 or Tramp.
use std::collections::VecDeque;

use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx, Wire};

/// The configured protocol kind, shared with the quad files' `[vtx] kind`.
pub use ofs_config::VtxKind;

use crate::smartaudio::{
    response_frame, settings_payload as smartaudio_settings_payload, Request, RequestParser, CMD_SET_CHANNEL,
    CMD_SET_FREQUENCY, CMD_SET_MODE, CMD_SET_POWER, FREQ_GET_PIT, FREQ_MASK, FREQ_SET_PIT, MODE_FREQUENCY, MODE_PIT,
    MODE_UNLOCKED, POWER_IS_DBM, RESP_SETTINGS_V21, SET_MODE_CLEAR_PIT, SET_MODE_PIT_IN_RANGE,
    SET_MODE_PIT_OUT_OF_RANGE, SET_MODE_UNLOCK,
};
use crate::tramp;

pub const BAND_LETTERS: [char; 6] = ['A', 'B', 'E', 'F', 'R', 'L'];

/// Channel frequencies in MHz per band, as Betaflight's factory vtxtable lists them.
pub const FREQUENCIES_MHZ: [[u16; 8]; 6] = [
    [5865, 5845, 5825, 5805, 5785, 5765, 5745, 5725], // A (Boscam A)
    [5733, 5752, 5771, 5790, 5809, 5828, 5847, 5866], // B (Boscam B)
    [5705, 5685, 5665, 5645, 5885, 5905, 5925, 5945], // E
    [5740, 5760, 5780, 5800, 5820, 5840, 5860, 5880], // F (Fatshark)
    [5658, 5695, 5732, 5769, 5806, 5843, 5880, 5917], // R (Raceband)
    [5362, 5399, 5436, 5473, 5510, 5547, 5584, 5621], // L (Lowband)
];

/// Index into [`BAND_LETTERS`] of a band letter ("A".."L").
pub fn band_index(letter: &str) -> Option<usize> {
    let mut chars = letter.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    BAND_LETTERS.iter().position(|b| *b == c)
}

#[derive(Debug, Clone)]
pub struct VtxParams {
    pub power_levels_mw: Vec<u32>,
    /// One dBm value per level, strictly increasing (SmartAudio v2.1 reports them).
    pub power_levels_dbm: Vec<u8>,
    /// Index into [`BAND_LETTERS`].
    pub default_band: usize,
    /// 1..=8.
    pub default_channel: u8,
    pub default_power_index: usize,
    pub reply_latency_s: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VtxState {
    /// `band * 8 + channel - 1` (0..48), the SmartAudio channel value; used while `user_freq_mhz` is None.
    pub index: u8,
    /// Set after SET_FREQUENCY: the VTX is in user-frequency mode.
    pub user_freq_mhz: Option<u16>,
    pub power_level: usize,
    pub pit: bool,
    pub unlocked: bool,
}

impl VtxState {
    pub fn freq_mhz(&self) -> u16 {
        self.user_freq_mhz
            .unwrap_or_else(|| FREQUENCIES_MHZ[usize::from(self.index / 8)][usize::from(self.index % 8)])
    }

    /// 1..=6 (A, B, E, F, R, L), or 0 in user-frequency mode.
    pub fn band(&self) -> u8 {
        if self.user_freq_mhz.is_some() { 0 } else { self.index / 8 + 1 }
    }

    /// 1..=8, or 0 in user-frequency mode.
    pub fn channel(&self) -> u8 {
        if self.user_freq_mhz.is_some() { 0 } else { self.index % 8 + 1 }
    }
}

/// The wire codec a [`VtxModel`] speaks, chosen by the configured [`VtxKind`].
enum Codec {
    Smartaudio(RequestParser),
    Tramp(tramp::Parser),
}

/// A video transmitter speaking SmartAudio v2.1 or Tramp. Reads Betaflight's requests from `requests` (the tap on
/// its UART TX), answers on `replies` (the wire into its UART RX) after the configured latency, and publishes its
/// state on the bus for the link model.
pub struct VtxModel {
    kind: VtxKind,
    params: VtxParams,
    state: VtxState,
    codec: Codec,
    requests: Wire,
    replies: Wire,
    pending: VecDeque<(f64, Vec<u8>)>,
    pit_freq_mhz: u16,
    divisor: u32,
    present: Signal<f64>,
    band: Signal<f64>,
    channel: Signal<f64>,
    freq: Signal<f64>,
    power: Signal<f64>,
    pit: Signal<f64>,
    /// The thermal model's VTX temperature, for Tramp's status reply.
    temperature: Signal<f64>,
}

impl VtxModel {
    pub fn new(kind: VtxKind, params: VtxParams, requests: Wire, replies: Wire, divisor: u32, bus: &mut Bus) -> Self {
        assert_eq!(params.power_levels_mw.len(), params.power_levels_dbm.len(), "one dBm value per power level");
        assert!(params.default_power_index < params.power_levels_mw.len(), "default power level out of range");
        assert!(params.default_band < BAND_LETTERS.len() && (1..=8).contains(&params.default_channel), "default band/channel");
        let state = VtxState {
            index: (params.default_band * 8) as u8 + params.default_channel - 1,
            user_freq_mhz: None,
            power_level: params.default_power_index,
            pit: false,
            unlocked: true,
        };
        let codec = match kind {
            VtxKind::Smartaudio => Codec::Smartaudio(RequestParser::default()),
            VtxKind::Tramp => Codec::Tramp(tramp::Parser::default()),
        };
        Self {
            kind,
            params,
            state,
            codec,
            requests,
            replies,
            pending: VecDeque::new(),
            pit_freq_mhz: 0,
            divisor,
            present: bus.signal(names::VTX_PRESENT),
            band: bus.signal(names::VTX_BAND),
            channel: bus.signal(names::VTX_CHANNEL),
            freq: bus.signal(names::VTX_FREQ_MHZ),
            power: bus.signal(names::VTX_POWER_MW),
            pit: bus.signal(names::VTX_PIT),
            temperature: bus.signal(names::VTX_TEMP_C),
        }
    }

    pub fn state(&self) -> VtxState {
        self.state
    }

    /// The protocol kind this VTX was built for.
    pub fn kind(&self) -> VtxKind {
        self.kind
    }

    /// Frames ignored so far: a bad CRC or an unknown command (the SmartAudio codec also counts a wrong payload length).
    pub fn bad_frames(&self) -> u32 {
        match &self.codec {
            Codec::Smartaudio(parser) => parser.bad_frames(),
            Codec::Tramp(parser) => parser.bad_frames(),
        }
    }

    fn current_dbm(&self) -> u8 {
        self.params.power_levels_dbm[self.state.power_level]
    }

    fn mode_byte(&self) -> u8 {
        let mut mode = 0;
        if self.state.user_freq_mhz.is_some() {
            mode |= MODE_FREQUENCY;
        }
        if self.state.pit {
            mode |= MODE_PIT;
        }
        if self.state.unlocked {
            mode |= MODE_UNLOCKED;
        }
        mode
    }

    fn handle(&mut self, request: Request) -> Vec<u8> {
        match request {
            Request::GetSettings => {
                let payload = smartaudio_settings_payload(
                    self.state.index,
                    self.mode_byte(),
                    self.state.freq_mhz(),
                    self.current_dbm(),
                    &self.params.power_levels_dbm,
                );
                response_frame(RESP_SETTINGS_V21, &payload)
            }
            Request::SetChannel(index) => {
                if index < 48 {
                    self.state.index = index;
                    self.state.user_freq_mhz = None;
                }
                response_frame(CMD_SET_CHANNEL, &[self.state.index])
            }
            Request::SetPower(value) => {
                if value & POWER_IS_DBM != 0 {
                    let dbm = value & !POWER_IS_DBM;
                    if dbm == 0 {
                        self.state.pit = true; // 0 dBm is how v2.1 turns pit mode on; the reported level stays
                    } else if let Some(level) = self.params.power_levels_dbm.iter().position(|d| *d == dbm) {
                        self.state.power_level = level;
                        self.state.pit = false;
                    }
                }
                response_frame(CMD_SET_POWER, &[value])
            }
            Request::SetFrequency(raw) => {
                let freq = raw & FREQ_MASK;
                if raw & FREQ_GET_PIT != 0 {
                    // The pit-mode frequency query: answered with the stored value, flagged as such.
                    let reply = self.pit_freq_mhz | FREQ_GET_PIT;
                    return response_frame(CMD_SET_FREQUENCY, &reply.to_be_bytes());
                }
                if raw & FREQ_SET_PIT != 0 {
                    self.pit_freq_mhz = freq;
                } else if (5000..=5999).contains(&freq) {
                    self.state.user_freq_mhz = Some(freq);
                }
                response_frame(CMD_SET_FREQUENCY, &raw.to_be_bytes())
            }
            Request::SetMode(mode) => {
                self.state.unlocked = mode & SET_MODE_UNLOCK != 0;
                if mode & SET_MODE_CLEAR_PIT != 0 {
                    self.state.pit = false;
                } else if mode & (SET_MODE_PIT_IN_RANGE | SET_MODE_PIT_OUT_OF_RANGE) != 0 {
                    self.state.pit = true;
                }
                response_frame(CMD_SET_MODE, &[mode])
            }
        }
    }

    /// Tramp requests. `None` means the FC expects no reply: it confirms 'F', 'P' and 'I' with its next 'v' at
    /// least 200 ms later (the plan's ruling 2), so the model applies them silently.
    fn handle_tramp(&mut self, request: tramp::Request, temperature_c: f64) -> Option<Vec<u8>> {
        match request {
            // Limits: the model accepts the SmartAudio frequency window (5000..5999) and its own power table.
            tramp::Request::Reset => {
                let payload = limits_payload(&self.params.power_levels_mw);
                Some(tramp::response_frame(tramp::CMD_RESET, &payload))
            }
            tramp::Request::GetSettings => {
                let freq = self.state.freq_mhz();
                let power_mw = self.params.power_levels_mw[self.state.power_level];
                let payload = settings_payload(freq, power_mw, self.state.pit);
                Some(tramp::response_frame(tramp::CMD_GET_SETTINGS, &payload))
            }
            tramp::Request::GetStatus => {
                // 's': the FC reads the temperature as an i16 at frame byte 6 (the plan's ruling 2) and discards a
                // zero one. It is the thermal model's VTX temperature (the ambient, 25 °C by default, without one).
                let mut payload = vec![0, 0, 0, 0];
                payload.extend_from_slice(&(temperature_c.round() as i16).to_le_bytes());
                Some(tramp::response_frame(tramp::RESP_STATUS, &payload))
            }
            tramp::Request::SetFrequency(freq) => {
                if (5000..=5999).contains(&freq) {
                    self.state.user_freq_mhz = Some(freq);
                }
                None // confirmed by the FC's next 'v' (ruling 2)
            }
            tramp::Request::SetPowerMw(mw) => {
                if let Some(level) = self.params.power_levels_mw.iter().position(|p| *p == u32::from(mw)) {
                    self.state.power_level = level;
                }
                None
            }
            tramp::Request::SetActive(active) => {
                self.state.pit = !active;
                None
            }
        }
    }

    fn publish(&self, bus: &mut Bus) {
        bus.set(self.present, 1.0);
        bus.set(self.band, f64::from(self.state.band()));
        bus.set(self.channel, f64::from(self.state.channel()));
        bus.set(self.freq, f64::from(self.state.freq_mhz()));
        bus.set(self.power, f64::from(self.params.power_levels_mw[self.state.power_level]));
        bus.set(self.pit, if self.state.pit { 1.0 } else { 0.0 });
    }
}

impl Model for VtxModel {
    fn name(&self) -> &str {
        "video.vtx"
    }

    fn rate_divisor(&self) -> u32 {
        self.divisor
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let bytes = self.requests.take(usize::MAX);
        if !bytes.is_empty() {
            match &mut self.codec {
                Codec::Smartaudio(parser) => {
                    for request in parser.push(&bytes) {
                        let reply = self.handle(request);
                        self.pending.push_back((ctx.time_s + self.params.reply_latency_s, reply));
                    }
                }
                Codec::Tramp(parser) => {
                    for request in parser.push(&bytes) {
                        if let Some(reply) = self.handle_tramp(request, bus.get(self.temperature)) {
                            self.pending.push_back((ctx.time_s + self.params.reply_latency_s, reply));
                        }
                    }
                }
            }
        }
        while self.pending.front().is_some_and(|(due, _)| *due <= ctx.time_s) {
            if let Some((_, reply)) = self.pending.pop_front() {
                self.replies.write(&reply);
            }
        }
        self.publish(bus);
        Ok(())
    }
}

/// 'r': min frequency 5000, max 5999 (the model's accepted window), max power from the model's table —
/// little endian; the FC ignores the reply unless min_freq is not zero.
fn limits_payload(power_levels_mw: &[u32]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&5000u16.to_le_bytes());
    p.extend_from_slice(&5999u16.to_le_bytes());
    p.extend_from_slice(&((*power_levels_mw.iter().max().unwrap_or(&0)).min(u32::from(u16::MAX)) as u16).to_le_bytes());
    p
}

/// 'v': frequency, configured power, control mode 0, pit flag, actual power — little endian; the FC ignores
/// the reply unless the frequency is not zero, which it never is while the VTX exists.
fn settings_payload(freq_mhz: u16, power_mw: u32, pit: bool) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&freq_mhz.to_le_bytes());
    p.extend_from_slice(&(power_mw.min(u32::from(u16::MAX)) as u16).to_le_bytes());
    p.push(0); // control mode: no race lock
    p.push(u8::from(pit));
    p.extend_from_slice(&(power_mw.min(u32::from(u16::MAX)) as u16).to_le_bytes());
    p
}
