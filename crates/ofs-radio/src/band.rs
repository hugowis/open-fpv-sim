//! The ExpressLRS bands the link can run on: carrier frequency, receiver noise floor, and the packet rates each
//! band's LoRa modes offer with their sensitivities (https://expresslrs.org/info/signal-health/).
use crate::elrs::{ELRS_FREQ_MHZ, NOISE_FLOOR_DBM};

/// The middle of the 868/915 MHz band, 915 MHz (the FCC side); frequency hopping is not modelled (estimated).
pub const ELRS_900_FREQ_MHZ: f64 = 915.0;
/// LoRa bandwidth 500 kHz, noise figure 6 dB (estimated): `-174 + 10*log10(500_000) + 6`.
pub const NOISE_FLOOR_900_DBM: f64 = -111.010_299_956_639_81;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Band {
    /// 2.4 GHz LoRa modes: 50, 150, 250 and 500 Hz (the model since M3c).
    #[default]
    G2_4,
    /// 868/915 MHz LoRa modes: 25, 50, 100, 200 and 250 Hz.
    Mhz900,
}

/// (packet rate in Hz, sensitivity in dBm), ascending by rate. ExpressLRS's table lists the 2.4 GHz 50 Hz mode at
/// -115 dBm; this model has used -117 since M3c and changing it would change every seeded 50 Hz run (a roadmap row).
const G2_4_MODES: [(u32, f64); 4] = [(50, -117.0), (150, -112.0), (250, -108.0), (500, -105.0)];
const MHZ_900_MODES: [(u32, f64); 5] = [(25, -123.0), (50, -120.0), (100, -117.0), (200, -112.0), (250, -111.0)];

impl Band {
    pub const ALL: [Band; 2] = [Band::G2_4, Band::Mhz900];

    /// The name a quad file uses: `"2g4"` or `"900"`.
    pub fn name(self) -> &'static str {
        match self {
            Band::G2_4 => "2g4",
            Band::Mhz900 => "900",
        }
    }

    pub fn from_name(name: &str) -> Option<Band> {
        Band::ALL.into_iter().find(|b| b.name() == name)
    }

    pub fn freq_mhz(self) -> f64 {
        match self {
            Band::G2_4 => ELRS_FREQ_MHZ,
            Band::Mhz900 => ELRS_900_FREQ_MHZ,
        }
    }

    pub fn noise_floor_dbm(self) -> f64 {
        match self {
            Band::G2_4 => NOISE_FLOOR_DBM,
            Band::Mhz900 => NOISE_FLOOR_900_DBM,
        }
    }

    fn modes(self) -> &'static [(u32, f64)] {
        match self {
            Band::G2_4 => &G2_4_MODES,
            Band::Mhz900 => &MHZ_900_MODES,
        }
    }

    /// The packet rates this band's LoRa modes offer, ascending.
    pub fn packet_rates(self) -> Vec<u32> {
        self.modes().iter().map(|m| m.0).collect()
    }

    /// The RSSI at which half the packets decode.
    pub fn sensitivity_dbm(self, packet_rate_hz: u32) -> Option<f64> {
        self.modes().iter().find(|m| m.0 == packet_rate_hz).map(|m| m.1)
    }

    /// The CRSF `rf_mode` a receiver reports: the rate's place in the band's ascending list plus one (0 stays the
    /// 4 Hz Crossfire mode). For 2.4 GHz that is 50 -> 1, 150 -> 2, 250 -> 3, 500 -> 4.
    pub fn rf_mode_index(self, packet_rate_hz: u32) -> Option<u8> {
        self.modes().iter().position(|m| m.0 == packet_rate_hz).map(|i| i as u8 + 1)
    }
}
