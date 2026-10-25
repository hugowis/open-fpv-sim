# Writing a radio model

The simulated radio is three swappable parts (`crates/ofs-radio/src/parts.rs`): a **handset** (sticks to channels),
a **link** (the air: loss, latency, link quality, the downlink) and a **receiver** (what arrives, as the serial
protocol the flight controller reads: CRSF or iBUS). A `RadioChain` (`crates/ofs-radio/src/chain.rs`) steps them in
order every packet slot. The quad file names the parts: `[radio] kind` picks the link (`"elrs"` or `"fixed"`), and
`handset` (`"joystick"`) and `receiver` (`"crsf"` or `"ibus"`) pick the other two; they default to `"joystick"` and
`"crsf"`. This page walks through writing a new **link**, the part you are most likely to want;
`crates/ofs-radio/src/fixed.rs` is the worked example (about 115 lines, comments included).

## The contract

```rust
pub trait Link: Send {
    fn name(&self) -> &str;
    fn packet_rate_hz(&self) -> u32;
    fn deliver(&mut self, ctx: &StepCtx, bus: &Bus, tx: &TxFrame) -> Delivery;
}
```

- `name` names the chain and the link's random stream (`FixedLink` returns `"radio.fixed"`).
- `deliver` runs once per packet slot (at `packet_rate_hz`, which must divide the base rate). `tx` is what the
  handset sent: 16 CRSF channels as 11-bit ticks and whether the transmitter is on.
- Return `Delivery { channels, downlink_slot, report }`: `channels` is `Some` when the receiver heard the packet (after
  whatever latency you model) and `None` when it was lost; `downlink_slot` is `true` when a telemetry frame flew back
  to the handset this slot; `report` (a `LinkReport`) carries the numbers the HUD and Betaflight's LINK_STATISTICS show.
- **Only a link draws random numbers.** Seed a stream with `ofs_core::rng::model_rng(seed, "radio.yours")`, and draw
  the same count of numbers every packet whatever the inputs, so a seeded run never shifts when a fault toggles.
  `FixedLink` keeps separate streams (`"fixed-faults"` for the uplink loss draw it hands to `FaultSignals::survives`
  and `"fixed-faults-downlink"` for the downlink draw it hands to `FaultSignals::downlink_survives`), so the loss
  draws never move when a fault is injected.
- Register the bus handles you read in your constructor. A link that depends on where the quad is (the ELRS link does)
  reads the pose, for example `bus.signal(names::BODY_POS_NED)`; `FixedLink` has no geometry and reads none. Every
  link should call `FaultSignals::register(bus)` and use `link_cut`, `survives` and `downlink_survives`, so the fault
  catalog's `radio_link_loss`, `radio_loss`, `downlink_loss` and `burst_loss` work on your link for free. `LqWindow`
  gives the last-100-packets
  link quality ExpressLRS reports.

Both shipped links take `[radio] telemetry_slot_divisor`: one downlink packet in that many carries a telemetry frame
when heard. It accepts 1, 2, 4, 8, 16, 32, 64 or 128, defaults to 4, and applies one value to every band and packet
rate — there is no off value and no per-mode ratio. It changes only the telemetry slot cadence, not packet loss,
uplink delivery or downlink LQ. A new link should take the same field (the vehicle passes it in) and use it for
`downlink_slot`. The default 4 keeps 0.2.0 recordings and the radio goldens byte-identical.

## Wiring it in

1. Add a module (`crates/ofs-radio/src/yours.rs`) and `pub mod yours;` in `crates/ofs-radio/src/lib.rs`.
2. Add a variant to `RadioKind` in `crates/ofs-config/src/lib.rs`, a table for its parameters (see `FixedSection`,
   read from `[radio.fixed]`), and its validation next to the `RadioKind::Fixed` arm in `validate` (unknown keys are
   errors by default; keys that belong to another link are rejected there).
3. Add the match arm in `radio_chain` in `crates/ofs-sim/src/vehicle.rs`: the compiler points at it, because the
   `match` over `RadioKind` is exhaustive. The handset and receiver are chosen by the same kind of exhaustive `match`
   (over `HandsetKind` and `ReceiverKind`), so a new handset or receiver variant is a compile error there until it is
   built.
4. Write tests like `crates/ofs-radio/tests/fixed.rs`: exact latency, seeded loss, faults honoured. A link defined
   inside a test file works too: see `crates/ofs-radio/tests/swap.rs`.
5. Ship an example quad (`quads/opendrone-5f-fixed-link.toml` is the one for the fixed link) so the model is one
   `load` away.

## Writing a receiver

A receiver implements `Receiver` (`parts.rs`): `emit` writes what a packet slot delivered to the flight controller's
UART `Wire`, `ingest_telemetry` and `next_telemetry` carry the flight controller's own telemetry back (an iBUS receiver
has none and drops it). `crates/ofs-radio/src/ibus.rs` is the example: the frame codec beside the receiver, tested
against a golden frame computed by hand (`crates/ofs-radio/tests/ibus.rs`). A receiver draws no random numbers, and it
owns its frame rate: the link delivers at its packet rate, a serial protocol has its own cadence (iBUS about 7 ms), so
hold the latest channels and write when a frame is due. Then:

1. Add a `ReceiverKind` variant (`ofs-config`) and its arm in `radio_chain` (`ofs-sim`).
2. Add `#define USE_SERIALRX_<PROTOCOL>` to the SITL patch (`third_party/betaflight/ofs-sitl.patch`): `target.h`
   undefines the serial receivers it does not use. Rebuild with `scripts/build-sitl.sh`; CI rebuilds on a patch change.
3. Ship an example quad whose diff sets `serialrx_provider` (`quads/opendrone-5f-ibus.betaflight.diff`), and a live test
   that reads the sticks back over `MSP_RC` (`ibus_bytes_in_the_state_datagram_reach_betaflight` in
   `crates/ofs-fc/tests/sitl_live.rs`).
4. Source every protocol figure and mark what is estimated (`docs/research/ibus-receiver.md`).

## Determinism

The radio goldens (`crates/ofs-radio/tests/golden.rs`, `crates/ofs-sim/tests/radio_golden.rs`) pin the ELRS link's
bytes. They are for the ELRS link: a new link gets its own tests, and nothing it does may change the ELRS digests.
