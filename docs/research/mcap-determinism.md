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

## Test data must avoid platform libm

The Windows and Linux files first differed at byte 40 — the first chunk's length. The spike's test signal
used `f64::sin`, which Rust takes from the platform libm, and MSVC's and glibc's `sin` disagree in the last
bits; different floats print as different-length JSON. With a signal built only from IEEE `+ − × ÷` and
`floor`, the two platforms produce the same bytes. The simulator's own runs are deterministic per platform
(the physics uses libm too); M4c's byte-identity test compares two runs on one machine, which is what the
spec asks for, and needs no cross-platform claim.
