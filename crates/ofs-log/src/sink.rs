//! The deterministic MCAP writer: same channels and records in, same bytes out (docs/research/mcap-determinism.md).
use std::collections::BTreeMap;
use std::io::{Seek, Write};

use glam::{DQuat, DVec3};
use mcap::records::MessageHeader;
use mcap::{Compression, WriteOptions};
use ofs_core::SignalKind;

use crate::spec::Channel;

/// The header's library string, pinned: the crate default carries its version, which would change the bytes on
/// every mcap bump.
pub const LIBRARY: &str = "ofs-log";

/// JSON schemas of the three record shapes, so viewers (Foxglove) can address the fields.
const SCALAR_SCHEMA: &str = r#"{"type":"object","properties":{"v":{"type":"number"}}}"#;
const VEC3_SCHEMA: &str = r#"{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"},"z":{"type":"number"}}}"#;
const QUAT_SCHEMA: &str =
    r#"{"type":"object","properties":{"w":{"type":"number"},"x":{"type":"number"},"y":{"type":"number"},"z":{"type":"number"}}}"#;

/// `{"v":x}` for a scalar. Numbers print with serde_json (shortest round-trip; a non-finite value is `null`).
pub fn scalar_json(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(b"{\"v\":");
    number(out, v);
    out.push(b'}');
}

/// `{"x":..,"y":..,"z":..}` for a vec3.
pub fn vec3_json(out: &mut Vec<u8>, v: DVec3) {
    fields(out, &[("x", v.x), ("y", v.y), ("z", v.z)]);
}

/// `{"w":..,"x":..,"y":..,"z":..}` for a quaternion.
pub fn quat_json(out: &mut Vec<u8>, q: DQuat) {
    fields(out, &[("w", q.w), ("x", q.x), ("y", q.y), ("z", q.z)]);
}

fn fields(out: &mut Vec<u8>, fields: &[(&str, f64)]) {
    out.push(b'{');
    for (i, (k, v)) in fields.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.push(b'"');
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(b"\":");
        number(out, *v);
    }
    out.push(b'}');
}

fn number(out: &mut Vec<u8>, v: f64) {
    serde_json::to_writer(&mut *out, &v).expect("writing to a Vec cannot fail");
}

/// The error and its causes: mcap's own message ("I/O error from writing...") hides the cause ("disk full").
fn describe(e: mcap::McapError) -> String {
    let mut message = e.to_string();
    let mut source = std::error::Error::source(&e);
    while let Some(cause) = source {
        message = format!("{message}: {cause}");
        source = cause.source();
    }
    message
}

/// Counts of what a finished file holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SinkSummary {
    pub messages: u64,
    /// Log time of the first and last message, ns (both 0 when there are none).
    pub first_ns: u64,
    pub last_ns: u64,
}

/// One MCAP file: a channel per recorded signal (topic = bus name, JSON records with a JSON schema), zstd chunks,
/// every option that could vary between machines pinned.
pub struct McapSink<W: Write + Seek> {
    writer: mcap::Writer<W>,
    ids: Vec<u16>,
    sequence: Vec<u32>,
    summary: SinkSummary,
}

impl<W: Write + Seek> McapSink<W> {
    pub fn new(w: W, channels: &[Channel]) -> Result<Self, String> {
        let opts = WriteOptions::new()
            .library(LIBRARY)
            .compression(Some(Compression::Zstd))
            // Single-threaded: identical bytes whatever the machine's core count, and no worker threads.
            // Message index records stay on (the default): Foxglove's reader finds messages only through them and
            // refuses a chunk without them, though they are most of the file's size.
            .compression_threads(0);
        let mut writer = opts.create(w).map_err(describe)?;
        let mut schema = |name: &str, data: &str| writer.add_schema(name, "jsonschema", data.as_bytes());
        let schemas = [
            schema("ofs.Scalar", SCALAR_SCHEMA).map_err(describe)?,
            schema("ofs.Vec3", VEC3_SCHEMA).map_err(describe)?,
            schema("ofs.Quat", QUAT_SCHEMA).map_err(describe)?,
        ];
        let mut ids = Vec::with_capacity(channels.len());
        for c in channels {
            let schema_id = match c.kind {
                SignalKind::Scalar => schemas[0],
                SignalKind::Vec3 => schemas[1],
                SignalKind::Quat => schemas[2],
            };
            let metadata = BTreeMap::from([("rate_hz".to_string(), format!("{}", c.rate_hz))]);
            ids.push(writer.add_channel(schema_id, &c.name, "json", &metadata).map_err(describe)?);
        }
        let sequence = vec![0; ids.len()];
        Ok(Self { writer, ids, sequence, summary: SinkSummary { messages: 0, first_ns: 0, last_ns: 0 } })
    }

    /// Appends one record to channel `index` (the order `new` was given) at `time_ns`.
    pub fn write(&mut self, index: usize, time_ns: u64, data: &[u8]) -> Result<(), String> {
        let header = MessageHeader { channel_id: self.ids[index], sequence: self.sequence[index], log_time: time_ns, publish_time: time_ns };
        self.writer.write_to_known_channel(&header, data).map_err(describe)?;
        self.sequence[index] = self.sequence[index].wrapping_add(1);
        if self.summary.messages == 0 {
            self.summary.first_ns = time_ns;
        }
        self.summary.messages += 1;
        self.summary.last_ns = time_ns;
        Ok(())
    }

    /// Writes the summary section and footer. The file is readable (and indexed) only after this.
    pub fn finish(mut self) -> Result<SinkSummary, String> {
        self.writer.finish().map_err(describe)?;
        Ok(self.summary)
    }
}
