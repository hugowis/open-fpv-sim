//! The video transmitter, controlled by Betaflight over SmartAudio v2.1.
use std::collections::VecDeque;

use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx, Wire};

use crate::smartaudio::{
    response_frame, settings_payload, Request, RequestParser, CMD_SET_CHANNEL, CMD_SET_FREQUENCY, CMD_SET_MODE,
    CMD_SET_POWER, FREQ_GET_PIT, FREQ_MASK, FREQ_SET_PIT, MODE_FREQUENCY, MODE_PIT, MODE_UNLOCKED, POWER_IS_DBM,
    RESP_SETTINGS_V21, SET_MODE_CLEAR_PIT, SET_MODE_PIT_IN_RANGE, SET_MODE_PIT_OUT_OF_RANGE, SET_MODE_UNLOCK,
};

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

/// A SmartAudio v2.1 VTX. Reads Betaflight's requests from `requests` (the tap on its UART TX), answers on `replies`
/// (the wire into its UART RX) after the configured latency, and publishes its state on the bus for the link model.
pub struct VtxModel {
    params: VtxParams,
    state: VtxState,
    parser: RequestParser,
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
}

impl VtxModel {
    pub fn new(params: VtxParams, requests: Wire, replies: Wire, divisor: u32, bus: &mut Bus) -> Self {
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
        Self {
            params,
            state,
            parser: RequestParser::default(),
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
        }
    }

    pub fn state(&self) -> VtxState {
        self.state
    }

    /// Frames ignored so far: bad CRC, unknown command or wrong length.
    pub fn bad_frames(&self) -> u32 {
        self.parser.bad_frames()
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
                let payload = settings_payload(
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
            for request in self.parser.push(&bytes) {
                let reply = self.handle(request);
                self.pending.push_back((ctx.time_s + self.params.reply_latency_s, reply));
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
