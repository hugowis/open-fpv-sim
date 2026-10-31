# The iBUS (FlySky) receiver (issue #8)

Date 2026-10-26. Design: `docs/superpowers/specs/2026-10-26-ibus-receiver-design.md`. Code: `crates/ofs-radio/src/ibus.rs`
(`IbusReceiver`), selected with `[radio] receiver = "ibus"`; the example is `quads/opendrone-5f-ibus.toml`.

## The protocol, with sources

Provenance labels follow the library's: `datasheet` (a document states it), `measured` (we captured it), `estimated`.

| Figure | Value | Label | Source |
|---|---|---|---|
| Frame | 32 bytes: `0x20`, `0x40`, 14 channels, 2-byte checksum | datasheet | Betaflight 2026.6.2 `src/main/rx/ibus.c`: the first byte is the frame length (4 or 32 bytes) and `IBUS_MAX_SLOTS` is 14. The `0x40` (command: channel data) is not checked by Betaflight; the IBusBM Arduino library (<https://github.com/bmellink/IBusBM>) and other community write-ups document it. FlySky published no specification |
| Channel word | 16-bit little-endian, microseconds | datasheet | Betaflight reads `lo + ((hi & 0x0F) << 8)`: 12 bits are the value, the high nibbles carry channels 15 to 18 in newer receivers (`updateChannelData`). The channels are used as microseconds directly |
| Value range | 1000 to 2000 | estimated | Community documentation; no vendor figure. The receiver clamps to it |
| Checksum | `0xFFFF` minus the sum of bytes 0 to 29, little-endian in bytes 30 and 31 | datasheet | Betaflight `telemetry/ibus_shared.c` (`isChecksumOkIa6b`); the same rule in community documentation |
| Line | 115200 baud, 8N1, not inverted | datasheet | Betaflight `ibus.c` (`IBUS_BAUDRATE`, `SERIAL_NOT_INVERTED`). A forum post says 8N2, which neither Betaflight nor the IBusBM library uses |
| Byte gap | more than 500 µs restarts a frame | datasheet | Betaflight `ibus.c` (`IBUS_FRAME_GAP`) |
| Frame period | about 7 ms (143 Hz) | estimated | The issue says "about 7 ms". The only documented 7 ms (Betaflight's iBUS telemetry page) is the sensor query cycle. No capture of a real receiver exists in this project |
| Behaviour on link loss | a real receiver's choice | estimated | The model sends no frames while no packet is delivered, which starts Betaflight's frame timeout and failsafe. Real receivers differ (repeat, send failsafe values, or stop) |

The sim's channels are CRSF ticks (1000 µs = 192, 1500 µs = 992, 2000 µs = 1792). The receiver maps them linearly to
microseconds and clamps; iBUS carries 14 channels, so the sim's channels 15 and 16 (centred by the joystick handset)
are not sent.

## What SITL can take

The simulator feeds any serial protocol the same way: `[uart][len]bytes` blocks after the state datagram
(`docs/research/sitl-interface.md`, "UART bytes in the state datagram"). The receiver model is therefore protocol
agnostic. What the shipped SITL lacked was the driver: its `target.h` `#undef`s `USE_SERIALRX_IBUS` (and SBUS, GHST,
JETIEXBUS), and only CRSF was switched back on. `third_party/betaflight/ofs-sitl.patch` now also defines
`USE_SERIALRX_IBUS` (the binary grows by about 600 bytes). No iBUS telemetry is built (`USE_TELEMETRY_IBUS` stays off):
the sensor polling protocol would need the receiver to answer on a half-duplex line.

## Live check

2026-10-25, WSL2 Ubuntu, Betaflight 2026.6.2 (`e0b7bb01`) built from the patched tree in a separate directory.
`ofs-fc`'s `ibus_bytes_in_the_state_datagram_reach_betaflight` (ignored, live) writes an iBUS frame every 7 ms on UART2
with `serialrx_provider = IBUS`, sticks roll 0.2, pitch -0.2, yaw 0, throttle 0, and reads `MSP_RC`: **1600, 1400, 1500,
1000** µs (roll, pitch, yaw, throttle), exactly as for CRSF. The same test against the previous SITL binary (no iBUS
driver) gets an empty `MSP_RC` channel list: the patch line is what makes it work.

Not checked: arming and flying through iBUS (the CRSF path has `e2e_betaflight.gd` for that), a long flight, and a
real FlySky radio or receiver.
