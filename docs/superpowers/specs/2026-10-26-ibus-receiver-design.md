# iBUS (FlySky) receiver (issue #8)

Date 2026-10-26. A third `Receiver` part, beside CRSF, that turns the link's delivered channels into FlySky iBUS frames on
a flight-controller UART, with the matching Betaflight config. The brainstorming questions were skipped (the brief asks
for few or none): the calls below are mine.

## Intent

The radio is three swappable parts (`docs/writing-a-radio-model.md`); the receiver is the only one with a protocol on the
flight-controller side. Today the only receiver is CRSF. iBUS is the simplest serial protocol after it and the one
FlySky receivers (FS-iA6B and friends) speak, so it also proves the receiver seam for SBUS (#7), FrSky (#9) and Spektrum
(#10): a pilot who owns a FlySky radio sees how the quad behaves behind that receiver.

Goals: an `ibus` receiver kind (`[radio] receiver = "ibus"`), a frame codec with a hand-computed golden frame, an example
quad with its Betaflight diff, and a live check against Betaflight SITL.

Non-goals: the FlySky air protocol (AFHDS 2A) and its link behaviour (the example quad uses the fixed link as a stand-in);
iBUS sensor telemetry (the single-wire telemetry polling; the receiver has no downlink); the 18-channel extension; SBUS,
FrSky and Spektrum (their own issues). Anti-goal: any change to an existing quad, the CRSF path or a golden.

## Facts and their sources

Provenance labels follow the library's: `datasheet` (a document states it), `measured` (we captured it) and `estimated`.

| Figure | Value | Label and source |
|---|---|---|
| Frame | 32 bytes: `0x20` (the length), `0x40` (the command: channel data), 14 channels, 2-byte checksum | Betaflight `src/main/rx/ibus.c` (2026.6.2, the pinned tag): the first byte must be 4 or 32 and sets the frame size; `IBUS_MAX_SLOTS 14`, `IBUS_SERIAL_RX_PACKET_LENGTH 32`. The `0x40` is not checked by Betaflight; it is documented by the IBusBM Arduino library and the FlySky reverse-engineering write-ups (`datasheet`-grade community documentation, no vendor document exists) |
| Channel word | 16-bit little-endian, microseconds, 1000 to 2000 | Betaflight reads `lo + ((hi & 0x0F) << 8)`, so 12 bits are the value and the high nibble belongs to channels 15 to 18 (`updateChannelData`). Range 1000 to 2000: community documentation; the vendor range is `estimated` |
| Checksum | `0xFFFF` minus the sum of the first 30 bytes, little-endian in bytes 30 and 31 | Betaflight `telemetry/ibus_shared.c` (`isChecksumOkIa6b`, low byte first) and the same rule in the community documentation |
| Serial format | 115200 baud, 8N1, not inverted, receive only | Betaflight `ibus.c` (`IBUS_BAUDRATE 115200`, `SERIAL_NOT_INVERTED`, `MODE_RX`). One community post says 8N2; the IBusBM library and Betaflight open 8N1, which is what SITL's TCP UART needs anyway |
| Inter-byte gap | more than 500 µs restarts the frame | Betaflight `ibus.c` (`IBUS_FRAME_GAP 500`) |
| Frame period | about 7 ms (143 Hz) | `estimated`: the issue says "about 7 ms"; the only number found in documentation (Betaflight's iBUS telemetry page, "queries every 7 ms") is for the sensor query cycle, not the channel frame. No capture of a real receiver was made |

## Design

- **Codec** (`crates/ofs-radio/src/ibus.rs`, like `crsf.rs`): `frame`, `decode` (header, length and checksum checks, an
  error for each), `checksum`, and the channel mapping.
- **Channels.** The currency between parts is 16 CRSF channels as 11-bit ticks (1000 µs = 192, 1500 µs = 992, 2000 µs =
  1792; `crsf::stick_ticks`). The receiver maps ticks to microseconds linearly (`us = 1000 + (ticks - 192) * 0.625`,
  rounded) and clamps to 1000 to 2000; the reverse (`ticks = 192 + (us - 1000) * 1.6`) is for decoding and the tests.
  microseconds to ticks and back is exact over 1000 to 2000; ticks to microseconds and back is within one tick (the
  codec has 1001 levels, the ticks 1601). Channels 15 and 16 are not carried (iBUS has 14): the joystick handset leaves them
  centred, and the high nibbles stay zero.
- **Pacing.** A real iBUS receiver sends one frame about every 7 ms whatever the radio link does; the simulated link
  delivers at its packet rate (50 to 500 Hz). `IbusReceiver` holds the last delivered channels and writes a frame when
  a delivery is *fresh* (a packet arrived since the last frame) and 7 ms of packet time has gone by (a deterministic
  accumulator; no random draws, as the receiver contract requires). It must not write a frame per packet: Betaflight
  restarts a frame on a gap above 500 µs and a faster stream would run frames into each other.
- **Link loss.** While no packet is delivered, no frame is written (my call, `estimated`): real receivers differ (some
  repeat the last values, some send failsafe values, some go silent), and silence is the case Betaflight can detect on
  its own (the frame timeout starts its failsafe). A radio-link-loss fault therefore makes Betaflight's RXLOSS and
  failsafe fire as with CRSF.
- **Telemetry.** None: bytes the flight controller writes to the UART are drained and dropped, and no downlink frame is
  ever queued.
- **Config.** `ReceiverKind::Ibus` (`receiver = "ibus"`); nothing else changes, no schema bump (an older build refuses
  the new name as an unknown variant, which is the existing behaviour for a bad name). Any link can feed any receiver (the
  parts are swappable); `link_stats_interval_packets` is ignored by this receiver.
- **Example quad.** `quads/opendrone-5f-ibus.toml` and its diff: the freestyle quad on the fixed link at 250 Hz (a stand-in
  for the FlySky link) with `serialrx_provider = IBUS` on the same UART2.

## What SITL can take (scoping)

The simulator already feeds a UART by appending `[uart][len]bytes` blocks to the state datagram
(`docs/research/sitl-interface.md`), whatever the protocol. But the patched `target.h` still `#undef`s
`USE_SERIALRX_IBUS`, so the shipped SITL has no iBUS driver (it was only re-enabled for CRSF). I add one line to
`third_party/betaflight/ofs-sitl.patch` (`#define USE_SERIALRX_IBUS`), rebuild in a separate directory, and check live
that a frame on UART2 reads back over `MSP_RC`. This changes the SITL binary for every quad (a driver added, none run
unless `serialrx_provider = IBUS`), so CI rebuilds it. If the live check failed I would have shipped the
receiver without the quad and said so; the result is in `docs/research/ibus-receiver.md`.

## Tests (first)

`crates/ofs-radio/tests/ibus.rs`: a hand-computed golden frame, encode/decode round trip, checksum (good, bad, the
`0xFFFF` complement), header and length errors, the high nibble ignored on decode, the tick/microsecond mapping at the
ends, centre and out of range, and the receiver: pacing at 500, 250 and 150 Hz, no frame on loss, latest channels,
channels 15 and 16 absent. `ofs-config`: the kind parses, the shipped iBUS quad loads. `ofs-fc` (ignored, live): iBUS
sticks over UART2 read back over `MSP_RC`.
