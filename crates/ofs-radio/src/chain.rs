//! The radio as one model: a handset, a link and a receiver stepped in order every packet slot.
use ofs_core::{Bus, Model, SimError, StepCtx, Wire};

use crate::elrs::{ElrsLink, LinkParams};
use crate::joystick::JoystickHandset;
use crate::parts::{Handset, Link, LinkOutputs, Receiver, RfFrontEnd};
use crate::receiver::{CrsfReceiver, ReceiverParams};
use crate::telemetry::{self, FcTelemetryHandle};

pub struct RadioChain {
    handset: Box<dyn Handset>,
    link: Box<dyn Link>,
    receiver: Box<dyn Receiver>,
    outputs: LinkOutputs,
    fc_telemetry: FcTelemetryHandle,
    div: u32,
}

impl RadioChain {
    /// The chain runs at the link's packet rate, which must divide the base rate.
    pub fn new(handset: Box<dyn Handset>, link: Box<dyn Link>, receiver: Box<dyn Receiver>, base_hz: u32, bus: &mut Bus) -> Self {
        let rate = link.packet_rate_hz();
        assert!(rate > 0 && base_hz % rate == 0, "the link's packet rate ({rate} Hz) must divide the base rate ({base_hz} Hz)");
        Self { handset, link, receiver, outputs: LinkOutputs::register(bus), fc_telemetry: FcTelemetryHandle::new(), div: base_hz / rate }
    }

    /// The ELRS 2.4 GHz chain with the joystick handset and the CRSF receiver: what `kind = "elrs"` builds. The
    /// handset's radio end is read from `params`.
    pub fn elrs(params: LinkParams, receiver: ReceiverParams, base_hz: u32, seed: u64, uart: Wire, telemetry_in: Wire, bus: &mut Bus) -> Self {
        let front_end = RfFrontEnd {
            position: params.handset_position,
            antennas: params.handset_antennas.clone(),
            tx_power_mw: params.tx_power_mw,
        };
        let handset = JoystickHandset::new(front_end, bus);
        let link = ElrsLink::new(params, seed, bus);
        let receiver = CrsfReceiver::new(receiver, uart, telemetry_in);
        Self::new(Box::new(handset), Box::new(link), Box::new(receiver), base_hz, bus)
    }

    /// The handset's view of Betaflight's telemetry, updated as frames fly downlink slots.
    pub fn fc_telemetry_handle(&self) -> FcTelemetryHandle {
        self.fc_telemetry.clone()
    }
}

impl Model for RadioChain {
    fn name(&self) -> &str {
        self.link.name()
    }

    fn rate_divisor(&self) -> u32 {
        self.div
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let tx = self.handset.sample(bus);
        self.receiver.ingest_telemetry();
        let delivery = self.link.deliver(ctx, bus, &tx);
        if delivery.downlink_slot {
            if let Some((frame_type, payload)) = self.receiver.next_telemetry() {
                telemetry::apply_frame(&self.fc_telemetry, frame_type, &payload, ctx.time_s);
            }
        }
        self.receiver.emit(&delivery);
        self.outputs.publish(bus, &delivery.report);
        Ok(())
    }
}
