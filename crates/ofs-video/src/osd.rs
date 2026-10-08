//! Betaflight's OSD over MSP DisplayPort, decoded into a character grid.
//!
//! Betaflight redraws the screen as `clear`, many `write string`, then `draw`. Writes go to a back buffer; `draw`
//! publishes it, so a half-drawn screen is never shown. See `displayport_msp.c` of the pinned Betaflight.
use std::sync::{Arc, Mutex};

use ofs_core::rng::fnv1a64;
use ofs_core::{Bus, Model, SimError, StepCtx, Wire};
use ofs_fc::msp::MspParser;

/// MSP command Betaflight uses for DisplayPort traffic.
pub const MSP_DISPLAYPORT: u8 = 182;
const SUB_HEARTBEAT: u8 = 0;
const SUB_RELEASE: u8 = 1;
const SUB_CLEAR: u8 = 2;
const SUB_WRITE: u8 = 3;
const SUB_DRAW: u8 = 4;
const ATTR_VERSION: u8 = 0x80;
const ATTR_BLINK: u8 = 0x40;
const ATTR_FONT: u8 = 0x03;
/// The OSD counts as gone when no screen was drawn for this long (seconds of simulated time).
pub const PRESENT_TIMEOUT_S: f64 = 1.0;

/// One character cell: the font code, the font page and the blink flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: u8,
    pub page: u8,
    pub blink: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Cell { ch: b' ', page: 0, blink: false }
    }
}

impl Cell {
    /// `ch | page << 8 | blink << 10`, the wire form used in the gRPC `OsdFrame`.
    pub fn packed(self) -> u32 {
        u32::from(self.ch) | (u32::from(self.page) << 8) | (u32::from(self.blink) << 10)
    }

    pub fn unpack(v: u32) -> Cell {
        Cell { ch: (v & 0xFF) as u8, page: ((v >> 8) & 0x3) as u8, blink: (v >> 10) & 1 == 1 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsdGrid {
    pub cols: usize,
    pub rows: usize,
    /// Row-major, `rows * cols` cells.
    pub cells: Vec<Cell>,
}

impl OsdGrid {
    pub fn blank(cols: usize, rows: usize) -> Self {
        OsdGrid { cols, rows, cells: vec![Cell::default(); cols * rows] }
    }

    pub fn get(&self, row: usize, col: usize) -> Cell {
        self.cells[row * self.cols + col]
    }

    fn set(&mut self, row: usize, col: usize, cell: Cell) {
        if row < self.rows && col < self.cols {
            self.cells[row * self.cols + col] = cell;
        }
    }

    /// The rows as text: printable ASCII as is, any other code as `?` (Betaflight's symbols live in the font, not
    /// in ASCII).
    pub fn text_rows(&self) -> Vec<String> {
        (0..self.rows)
            .map(|r| {
                (0..self.cols)
                    .map(|c| {
                        let ch = self.get(r, c).ch;
                        if (0x20..=0x7E).contains(&ch) {
                            ch as char
                        } else {
                            '?'
                        }
                    })
                    .collect()
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OsdFrame {
    /// Moves when the drawn cells change (not on an identical redraw).
    pub seq: u64,
    /// Simulated time of the sample.
    pub time_s: f64,
    /// False when the OSD is off, released, or silent for [`PRESENT_TIMEOUT_S`].
    pub present: bool,
    pub grid: OsdGrid,
}

impl OsdFrame {
    /// A stable hash of what the pilot would see (sequence, presence and cells; not the sample time).
    pub fn digest(&self) -> u64 {
        let mut bytes = Vec::with_capacity(16 + self.grid.cells.len() * 4);
        bytes.extend_from_slice(&self.seq.to_le_bytes());
        bytes.push(u8::from(self.present));
        bytes.extend_from_slice(&(self.grid.cols as u32).to_le_bytes());
        bytes.extend_from_slice(&(self.grid.rows as u32).to_le_bytes());
        for cell in &self.grid.cells {
            bytes.extend_from_slice(&cell.packed().to_le_bytes());
        }
        fnv1a64(&bytes)
    }
}

/// Turns the bytes Betaflight writes to its DisplayPort UART into OSD frames.
pub struct OsdDecoder {
    parser: MspParser,
    back: OsdGrid,
    front: OsdGrid,
    seq: u64,
    last_draw_s: Option<f64>,
    released: bool,
    unknown_frames: u32,
}

impl OsdDecoder {
    pub fn new(cols: usize, rows: usize) -> Self {
        OsdDecoder {
            parser: MspParser::default(),
            back: OsdGrid::blank(cols, rows),
            front: OsdGrid::blank(cols, rows),
            seq: 0,
            last_draw_s: None,
            released: false,
            unknown_frames: 0,
        }
    }

    /// Feeds bytes read at simulated time `now_s`. Anything that is not a valid DisplayPort frame is skipped.
    pub fn push(&mut self, bytes: &[u8], now_s: f64) {
        for reply in self.parser.push(bytes) {
            if reply.error || reply.cmd != MSP_DISPLAYPORT {
                self.unknown_frames += 1;
                continue;
            }
            let Some(&sub) = reply.payload.first() else {
                self.unknown_frames += 1;
                continue;
            };
            match sub {
                SUB_HEARTBEAT => {}
                SUB_RELEASE => self.released = true,
                SUB_CLEAR => self.back = OsdGrid::blank(self.back.cols, self.back.rows),
                SUB_WRITE => self.write(&reply.payload),
                SUB_DRAW => {
                    if self.front != self.back {
                        self.front = self.back.clone();
                        self.seq += 1;
                    }
                    self.last_draw_s = Some(now_s);
                    self.released = false;
                }
                _ => self.unknown_frames += 1,
            }
        }
    }

    fn write(&mut self, payload: &[u8]) {
        if payload.len() < 4 || payload[3] & ATTR_VERSION != 0 {
            self.unknown_frames += 1;
            return;
        }
        let (row, col, attr) = (usize::from(payload[1]), usize::from(payload[2]), payload[3]);
        for (i, ch) in payload[4..].iter().enumerate() {
            self.back.set(row, col + i, Cell { ch: *ch, page: attr & ATTR_FONT, blink: attr & ATTR_BLINK != 0 });
        }
    }

    pub fn frame(&self, now_s: f64) -> OsdFrame {
        let present = !self.released && self.last_draw_s.is_some_and(|t| now_s - t <= PRESENT_TIMEOUT_S);
        OsdFrame { seq: self.seq, time_s: now_s, present, grid: self.front.clone() }
    }

    /// Valid MSP frames that were not usable DisplayPort writes or draws.
    pub fn unknown_frames(&self) -> u32 {
        self.unknown_frames
    }
}

/// The latest frame, shared with the server's streams.
#[derive(Debug, Clone)]
pub struct OsdHandle(Arc<Mutex<OsdFrame>>);

impl OsdHandle {
    pub fn new(cols: usize, rows: usize) -> Self {
        OsdHandle(Arc::new(Mutex::new(OsdFrame { seq: 0, time_s: 0.0, present: false, grid: OsdGrid::blank(cols, rows) })))
    }

    pub fn latest(&self) -> OsdFrame {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn store(&self, frame: OsdFrame) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = frame;
    }
}

/// Reads the DisplayPort tap each exchange tick and keeps the [`OsdHandle`] current.
pub struct OsdModel {
    decoder: OsdDecoder,
    tap: Wire,
    handle: OsdHandle,
    divisor: u32,
}

impl OsdModel {
    pub fn new(cols: usize, rows: usize, tap: Wire, divisor: u32) -> Self {
        OsdModel { decoder: OsdDecoder::new(cols, rows), tap, handle: OsdHandle::new(cols, rows), divisor }
    }

    pub fn handle(&self) -> OsdHandle {
        self.handle.clone()
    }
}

impl Model for OsdModel {
    fn name(&self) -> &str {
        "video.osd"
    }

    fn rate_divisor(&self) -> u32 {
        self.divisor
    }

    fn step(&mut self, ctx: &StepCtx, _bus: &mut Bus) -> Result<(), SimError> {
        let bytes = self.tap.take(usize::MAX);
        if !bytes.is_empty() {
            self.decoder.push(&bytes, ctx.time_s);
        }
        let frame = self.decoder.frame(ctx.time_s);
        let current = self.handle.latest();
        if frame.seq != current.seq || frame.present != current.present {
            self.handle.store(frame);
        }
        Ok(())
    }
}
