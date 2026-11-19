# The receiver UART: a contention ruling (#63)

Does the one wire between the radio receiver and the flight controller need a bandwidth model? The plan asked for
arithmetic and code reading rather than an instrumented capture: the radio model already emits the downlink slot, so a
capture would only reproduce the schedule this note computes. Every number below is derived from the frame lengths the
code writes and the baud the SITL diff sets; the derivation is at the end so it can be re-run.

**Short answer: no model.** One RC frame does not fit one 500 Hz packet slot (2.257 ms against 2 ms), but the wire only
has to carry the packets the uplink delivers and the FC's own telemetry, that load fits SITL's per-slot drain, and the
backlog it leaves in the FIFOs is one a run cannot fill at the defaults: no frame fails to arrive and failsafe never
sees a gap. Recorded in `docs/roadmap.md` under the FC/SITL limits.

## The code paths

| What | Where |
|---|---|
| The RC frame the receiver writes: `[0xC8][len][0x16][22-byte payload][CRC]` = **26 bytes** | `crates/ofs-radio/src/crsf.rs:76-89` (`CHANNEL_COUNT` 16, `RC_CHANNELS_PAYLOAD_SIZE` 22, `crates/ofs-radio/src/crsf.rs:13-14`), length asserted at `crates/ofs-radio/tests/crsf.rs:38-39` |
| One RC frame per **delivered** packet, **plus** one LINK_STATISTICS frame every `link_stats_interval_packets` | `crates/ofs-radio/src/receiver.rs:57-78`; default 50 (`crates/ofs-radio/src/receiver.rs:20-24`, `crates/ofs-config/src/lib.rs:510-512`) |
| The LINK_STATISTICS frame: `[0xC8][len][0x14][10-byte payload][CRC]` = **12 bytes** | `crates/ofs-radio/src/crsf.rs:138-140`, payload size at `crates/ofs-radio/src/crsf.rs:15` |
| The telemetry slot: every `telemetry_slot_divisor`th **downlink** packet, default **4** | `crates/ofs-radio/src/elrs.rs:40-42,313-318`; default at `crates/ofs-radio/src/elrs.rs:116`, `crates/ofs-config/src/lib.rs:502-504` |
| The packet slot: 2 ms at 500 Hz, where every shipped quad runs | `crates/ofs-radio/src/band.rs:21`; `crates/ofs-radio/tests/elrs.rs:375-385` steps at 1/500 s |
| A Betaflight CRSF telemetry frame: **~36 bytes**, the first term of the FC's own telemetry | Betaflight's `telemetry/crsf.c` encoder; the sim sees its decoded payloads (`crates/ofs-radio/src/telemetry.rs:45-66`) |
| The FC side of the wire: bounded FIFOs, then at most **512 bytes of UART blocks per SITL datagram** | `crates/ofs-core/src/wire.rs:35-43` (capacity 4096, `crates/ofs-sim/src/vehicle.rs:278`), `crates/ofs-fc/src/sitl/bridge.rs:301-319`, `crates/ofs-fc/src/sitl/codec.rs:124-129` |
| The baud: `serial 1 64 115200 57600 0 115200` (CRSF receiver on UART2) | `crates/ofs-fc/tests/data/crsf.betaflight.diff:4`, `docs/research/sitl-interface.md:151` |

115200 baud, 8N1 = 10 bits per byte: **11 520 bytes/s = 11.52 bytes/ms**; one byte takes 86.8 µs.

## One packet slot

| Traffic in one 2 ms slot | Bytes | Time on the wire |
|---|---|---|
| Receiver → FC: one RC frame | 26 | **2.257 ms** |
| Receiver → FC: on a stats packet (1 in 50), RC + LINK_STATISTICS | 38 | 3.299 ms (amortised 2.278 ms) |
| FC → receiver: on a telemetry slot, one CRSF telemetry frame | 36 | 3.125 ms |

The RC frame alone is **2.257 ms of a 2 ms slot: 113 %**. CRSF is half-duplex on one UART, so the two directions never
share a slot's time: a telemetry frame takes a whole slot of its own whenever it flies.

## The sustainable rate

The receiver writes only while the uplink delivers packets, and the FC only while a downlink slot flies, so over time
the wire carries `26·delivered + 12·delivered/50 + 36·rate/divisor` bytes per second. The model can't make the wire
carry more: a lost packet writes nothing, and the FC's telemetry is capped at one frame per slot. Against 11 520 B/s,
at every packet rate the bands offer (`crates/ofs-radio/src/band.rs:21-22`):

| Packet rate | RC frames only (no downlink) | Worst case (every packet down, divisor 1) |
|---|---|---|
| 500 Hz | 13 120 B/s — **114 %** | 31 120 B/s — **270 %** |
| 250 Hz | 6 560 B/s — 57 % | 15 560 B/s — 135 % |
| 150 Hz | 3 936 B/s — 34 % | 9 336 B/s — 81 % |
| 50 Hz | 1 312 B/s — 11 % | 3 112 B/s — 27 % |

At the shipped default, divisor 4, the 500 Hz worst case is 500 × 26 + 500 × 12/50 + 125 × 36 = 13 000 + 120 + 4 500
= **17 620 B/s (153 %)**; 250 Hz is 8 810 B/s (76 %), 150 Hz 5 286 B/s (46 %), 50 Hz 1 762 B/s (15 %). Above
100 % the wire cannot carry the RC frames as fast as the uplink delivers them, and the excess waits; what a wait costs is
the next section.

## What a backlog does here

Three places could turn a backlog into a lost frame; none does:

1. **SITL's exchange drains a slot's traffic in one go.** The bridge sends up to 501 bytes of UART traffic in every
   state datagram (`crates/ofs-fc/src/sitl/bridge.rs:303-319`), one datagram per input tick. Open loop the base rate
   (8 kHz) is 16 ticks a slot, so a slot can drain 16 × 501 = 8 016 bytes — 258 times the 26 bytes an RC frame writes
   in one. With SITL in lockstep the exchange is one datagram per slot: 501 bytes against 2 ms × 26 B/ms = 52 bytes of
   RC traffic. Either way the router never withholds a slot's bytes past its own slot.
2. **The receiver's `Wire` is a FIFO.** `Wire::write` appends and drops the *oldest* bytes only when the 4096-byte
   FIFO overflows (`crates/ofs-core/src/wire.rs:35-43`). At 500 Hz/divisor 4 the wire is 6 100 B/s over capacity, so
   the FIFO would fill after 0.67 s *if nothing drained it* — but the bridge takes 8 016 bytes a slot against 26
   written, so the FIFO only grows when the exchange stalls, and then `fc.serial_dropped_bytes` reports it
   (`crates/ofs-fc/src/sitl/bridge.rs:238-248`).
3. **Betaflight parses a byte stream.** The decoder takes whole frames as they complete and resyncs after a bad CRC
   (`crates/ofs-radio/src/crsf.rs:149-202`): a late frame arrives whole, one packet older. SITL applies pending bytes
   whenever its main loop takes a tick (`third_party/betaflight/ofs-sitl.patch:759-769`); nothing ties a frame to the
   tick that wrote it.

The cost of the 113 % frame is therefore a constant one-slot lag on a 500 Hz link (the frame is out 0.257 ms into the
next slot), not a drop. (The lag is the model's, not a probe's: the sim never consumes the line at line rate, so its
`Wire` takes all 26 bytes at once; the 113 % is what a real line at that baud would do with them.)

## Why not model it

The threshold was "model it if the worst case leaves the RC frame unable to fit its slot **or** drops appear at the
default settings". Only the first half holds, and the right answer to it is the baud: 26 bytes fit 2 ms from 130 000
baud, the diff runs 115200 (the conservative case), and CRSF hardware runs 420 000, where a slot fits 84 bytes and even
a telemetry frame fits beside an RC frame. A per-frame contention model on the `Receiver` would put a constant one-slot
delay into every lockstep run — and into its digest — to reproduce lag the link's own `latency_packets` already stands
for, while changing no delivered value any run can observe. The one path that could drop (a FIFO overflow after the
exchange has stalled) is already counted and warned about.

The follow-up a real line would need, out of this wave: a baud, the 26/12/36-byte frames, `telemetry_slot_divisor`, and
a queue with delay and drop on the `Receiver` — worth its keep only once the sim consumes a serial line instead of
stretching it into datagrams, i.e. with the real-serial-radio route in the roadmap.

## Reproducing the arithmetic

```
bytes/s   = 115200 / 10 = 11520
ms(bytes) = bytes * 10 / 115200 * 1000        26 → 2.257, 38 → 3.299, 36 → 3.125
RC only   = 26 * rate + 12 * rate / 50        500 → 13000 + 120 = 13120 B/s (114 %)
worst     = RC only + 36 * rate / divisor     500, divisor 1 → 13120 + 18000 = 31120 B/s (270 %)
                                              500, divisor 4 → 13120 + 4500  = 17620 B/s (153 %)
                                              250, divisor 4 →  6560 + 2250  =  8810 B/s (76 %)
drain     = 8000/500 ticks per slot * (512 - 11) B = 16 * 501 = 8016 B/slot at the base rate
            (lockstep: 1 datagram per slot, 501 B against 2 ms * 26 B/ms = 52 B of traffic)
fit       = 26 bytes * 10 bits / 0.002 s = 130000 baud; 420000 baud → 84 bytes in a slot
```

The numbers are the model's schedule, not a measurement. If a routing changes (a faster link, a slower exchange, a
session long enough to fill the FIFO) and `fc.serial_dropped_bytes` moves at the defaults, this ruling flips: model the
line with the sizes above.
