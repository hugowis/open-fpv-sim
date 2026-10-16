//! The first handset: the stick and switch signals the joystick, the Pilot stream or `SetSticks` drive.
use ofs_core::{names, Bus, Signal};

use crate::crsf::{self, CHANNEL_COUNT};
use crate::parts::{Handset, RfFrontEnd, TxFrame};

pub struct JoystickHandset {
    front_end: RfFrontEnd,
    roll: Signal<f64>,
    pitch: Signal<f64>,
    yaw: Signal<f64>,
    throttle: Signal<f64>,
    aux: Vec<Signal<f64>>,
    tx_enabled: Signal<f64>,
}

impl JoystickHandset {
    pub fn new(front_end: RfFrontEnd, bus: &mut Bus) -> Self {
        Self {
            front_end,
            roll: bus.signal(names::RC_ROLL),
            pitch: bus.signal(names::RC_PITCH),
            yaw: bus.signal(names::RC_YAW),
            throttle: bus.signal(names::RC_THROTTLE),
            aux: (0..names::RC_AUX_COUNT).map(|i| bus.signal(&names::rc_aux(i))).collect(),
            tx_enabled: bus.signal(names::RADIO_TX_ENABLED),
        }
    }
}

impl Handset for JoystickHandset {
    fn rf_front_end(&self) -> &RfFrontEnd {
        &self.front_end
    }

    /// Sticks to channels in Betaflight's default AETR order, then AUX1..4, the rest centred.
    fn sample(&mut self, bus: &Bus) -> TxFrame {
        let mut ch = [crsf::stick_ticks(0.0); CHANNEL_COUNT];
        ch[0] = crsf::stick_ticks(bus.get(self.roll));
        ch[1] = crsf::stick_ticks(bus.get(self.pitch));
        ch[2] = crsf::throttle_ticks(bus.get(self.throttle));
        ch[3] = crsf::stick_ticks(bus.get(self.yaw));
        for (slot, s) in ch[4..].iter_mut().zip(&self.aux) {
            *slot = crsf::stick_ticks(bus.get(*s));
        }
        TxFrame { channels: ch, transmitting: bus.get(self.tx_enabled) > 0.5 }
    }
}
