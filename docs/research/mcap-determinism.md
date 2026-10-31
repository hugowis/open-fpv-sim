# MCAP byte-identity

The M4 sandbox spec ("Logging: `ofs-log` and MCAP (M4c)") promises that two lockstep runs with the same seed
and inputs write **byte-identical MCAP files**, and lists the `mcap` crate embedding nondeterministic bytes
as M4c's riskiest point. This note records the spike run before M4c started (2026-10-12) and the dependency
decision it led to: `ofs-log` depends on upstream `mcap` at a pinned git revision, not on the crates.io
release.

```toml
mcap = { git = "https://github.com/foxglove/mcap.git", rev = "00e47a1b" }  # 0.25.1, unreleased
```

## The finding

`mcap` 0.25.0 — the newest release on crates.io (2026-06-11) — is not deterministic. At `finish()` the
writer repeats every channel and schema record in the summary section, and it walks `Summary::channels` and
`Summary::schemas` to do so: both are `HashMap`s, so the order is the per-process random `RandomState`
order. Eight runs of identical input gave eight different file hashes with 24 channels and two with 2
channels, under every compression mode (zstd, lz4, none). The first differing byte sits in the summary,
a few kilobytes before the end of a 6.6 MB file; the data section was identical.

Everything else the spec worried about is stable:

| Suspect | Result |
|---|---|
| Chunk offsets, message indexes, chunk indexes | identical across runs |
| Chunk, data-section and summary CRCs | identical (they are functions of the bytes) |
| Multithreaded zstd | identical output for 0, 1, 4 and 16 threads and the per-core default |
| Wall clock | none anywhere in the writer |
| The `library` header string | defaults to `mcap-rust/<version>`; `ofs-log` pins it with `WriteOptions::library` |

## Why not switch the summary records off

`repeat_channels(false)` + `repeat_schemas(false)` removes the unordered walk and makes 0.25.0
byte-identical — but the Python `mcap` reader (1.5.0) then cannot read the file by index: `iter_messages()`
resolves each message's channel from the summary and raises `KeyError`. That reader is what
`ofs.reader.to_dataframe` is built on, and Foxglove relies on the summary the same way. Losing indexed reads
to keep a determinism test is the wrong trade.

## The fix, upstream

Upstream fixed it in `01c9d495` ("rust: serialize summaries from ordered writer IDs", foxglove/mcap#1838,
2026-09-08): the summary records are written in the order of the writer's `BTreeMap` id maps. No crates.io
release carries it yet. The pin is `00e47a1b` (2026-09-24), the commit that labels the crate 0.25.1: it
contains the fix and the compression-context reuse (#1841), and stops before the reader refactors and the
zstd 0.14 bump that followed. When 0.25.1 or later is published, the git dependency becomes a version
requirement and nothing else changes.

Verified at the pin, unpatched, with 24 channels at the spec's default rates (imu 1 kHz, motor 240 Hz,
radio 50 Hz, battery 10 Hz), JSON records, 20 simulated seconds:

| Compression | Runs | Distinct SHA-256 |
|---|---|---|
| zstd (default) | 8 Windows + 3 Linux (WSL) | 1 |
| lz4 | 8 Windows + 3 Linux | 1 |
| none | 8 Windows + 3 Linux | 1 |

The bytes also match a hand-patched 0.25.0 that sorts the summary by id, so #1841 does not change the
output. The Python reader reads every variant fully: indexed, filtered by topic, by time window and as a
stream.

## File size: the per-message index records stay

Measured in M4c with the shipped quad, open loop, 60 simulated seconds at the default rates (105 signals, 15 456
records per simulated second): **24.9 MB** with the crate's defaults, **9.9 MB** with `emit_message_indexes(false)`.
A message index record is 16 uncompressed bytes per message, written after each chunk, so it outweighs the
zstd-compressed records themselves. The Python reader and the Rust `MessageStream` read files without them, and M4c
first shipped without them, but **Foxglove cannot**: its reader (`@mcap/core`'s `McapIndexedReader`) finds messages
only through each chunk's message index records and refuses a chunk that has none ("Encountered a chunk index without
message indexes and non-zero start and end times"; checked with `@mcap/core` against a sandbox recording, and seen in
app.foxglove.dev). Since the spec requires the files to open in Foxglove unchanged, `ofs-log` keeps the crate's
default, and `crates/ofs-log/tests/determinism.rs` pins that every chunk indexes its channels. The sandbox flight
(Betaflight, 106 signals) records about 4.8 MB per 10 simulated seconds. Reading back with `ofs.reader` (pure Python)
used to cost about 3.7 s for three patterns of a one-minute recording and 11.7 s for every signal, because `mcap`'s
iterator parses and merges every record of the file. Message indexes make that avoidable: `ofs.reader` cuts only the
selected signals' records out of each decompressed chunk (open-loop minute, 148 signals: 0.34 s for three patterns and
2.2 s for every signal, against 3.3 s and 9.8 s; `scripts/bench_reader.py`). Files without message indexes, or with
lz4 chunks, are still read through the iterator.

## Reading a recording while it is being written

Can a file be opened while the session is still recording it? Probed on 2026-10-22 (Windows 11,
Python 3.11, `mcap` 1.5.0, `pandas` 3.0.6, a debug `ofs-sim` built from this repository at `60a8fe0`,
the 0.3.0 branch of PR #119) with a lockstep open-loop session recording every signal at the
default rates. **Foxglove is not installed on that
machine — desktop and web were not run, and nothing here claims what Foxglove does.** The `mcap` CLI
was not available either (no `mcap` on `PATH`; `pip install mcap-cli` found no distribution), so the
readers below are the Python `mcap` library and `ofs.reader` — the exact tools `to_dataframe` is
built on. The answer has two parts, both structural rather than tool-specific:

1. **Complete chunks reach the disk as the session runs.** The writer closes each chunk when it
   reaches the crate's default 1 MiB uncompressed, so the file grows steadily (probe run:
   163,420 B at 1 simulated second, 2,042,143 B at 5 s, 4,393,796 B just before `stop()`).
2. **The summary section, footer and tail magic are written only by `stop()`.** No reader that
   builds its view from the footer — every indexed reader, `ofs.reader` included — can read the file
   until then. Mid-recording the file ends inside an open chunk: the writer pre-writes each chunk's
   record header with a placeholder length of `u64::MAX` and patches it when the chunk closes
   (`write.rs`: `op_and_len(&mut sink, op::CHUNK, !0)`), so the last record on disk never terminates.

| Reader | Open file | Copied file | Closed file |
|---|---|---|---|
| `ofs.reader.read`/`to_dataframe` | refuses: `ValueError: …: unfinished recording (it is still being recorded, so its summary is not written yet: stop it first with sim.recording.stop(), or read sim.log, which stops it; otherwise …)` — neither the fast nor the iterator path engages; the refusal happens at the summary | same refusal (the copy has no summary either) | reads fully, message-index fast path |
| `mcap` 1.5.0 `get_summary()` (any indexed reader's first step) | `RecordLengthLimitExceeded: unknown (opcode 184) record has length 893077043047549237 that exceeds limit 4294967296` (the opcode and length vary — the reader seeks to EOF−footer and parses compressed bytes there) | same | OK: 130 channels, 10 chunk indexes, 173,952 messages |
| `mcap` 1.5.0 `StreamReader` (streaming, no index) | reads every flushed chunk — 0 messages at 1 s (the first chunk is still open), 71,763 of the eventual 173,952 at 5 s, 161,762 just before `stop()` — then `RecordLengthLimitExceeded: CHUNK record has length 18446744073709551615 that exceeds limit 4294967296` at the open chunk | identical bytes, identical result | all 173,952 messages, footer, no error |
| Foxglove desktop/web | **not tested** (not installed on the evidence machine) | **not tested** | not re-tested here; the message-index check above ran finished sandbox recordings through app.foxglove.dev |

Copying the file mid-recording does not make an indexed read possible — the copy is exactly as
unfinished — and it is unnecessary for access: reading the live path itself works on Windows (no
sharing violation; the server's `File` opens with share read/write). A copy only helps a reader you
do not want touching the live path, and only a streaming one can make anything of it, up to the last
flushed chunk.

**The workaround is to finish the file first:** `sim.recording.stop()` (or `sim.log`, which stops the
running recording and returns it) writes the summary and the footer; every reader then reads the
whole thing. There is nothing to fix in the writer: an MCAP file has one footer, at the end, so no
mid-recording flush can make an indexed reader accept a live file. One gap the probe did find: the
refusal's text names a write error or a killed server but not the third cause, a recording that is
still running — #117.

To re-run the evidence against a built server (`cargo build -p ofs-sim`), from the repository root:

```python
import pathlib, sys, tempfile
sys.path.insert(0, "python")
import ofs
from mcap.reader import make_reader
from mcap.records import Message
from mcap.stream_reader import StreamReader

sim = ofs.launch(binary=str(pathlib.Path("target/debug/ofs-sim.exe").resolve()),  # Windows wants it absolute
                 data_dir=tempfile.mkdtemp())
sim.load("quads/opendrone-5f-freestyle.toml", seed=1, open_loop_fc=True)
sim.set_sticks(throttle=0.6); sim.run(5.0)
live = pathlib.Path(sim.recording.path)
print(live.stat().st_size)                                # grows while the session runs
try: ofs.reader.read(live, ["battery.voltage_v"])
except Exception as e: print(type(e).__name__, e)         # the unfinished-recording refusal
with open(live, "rb") as f:                               # any indexed reader's first step
    try: print(make_reader(f).get_summary())
    except Exception as e: print(type(e).__name__, e)
n = 0
try:
    with open(live, "rb") as f:
        for r in StreamReader(f).records: n += isinstance(r, Message)  # MessageIndex has channel_id too
except Exception as e: print(n, "messages, then", type(e).__name__, e)
sim.recording.stop()                                      # then repeat the three probes
```

## Test data must avoid platform libm

The Windows and Linux files first differed at byte 40 — the first chunk's length. The spike's test signal
used `f64::sin`, which Rust takes from the platform libm, and MSVC's and glibc's `sin` disagree in the last
bits; different floats print as different-length JSON. With a signal built only from IEEE `+ − × ÷` and
`floor`, the two platforms produce the same bytes. The simulator's own runs are deterministic per platform
(the physics uses libm too); M4c's byte-identity test compares two runs on one machine, which is what the
spec asks for, and needs no cross-platform claim.
