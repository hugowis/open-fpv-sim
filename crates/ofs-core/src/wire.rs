//! Byte ports between models: a serial line from one model's TX to another's RX (for example a radio
//! receiver's CRSF output into the flight controller's UART). Bounded like a UART RX buffer: when full,
//! the oldest bytes are dropped and counted. The scheduler's fixed model order keeps traffic deterministic.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug)]
struct Fifo {
    bytes: VecDeque<u8>,
    capacity: usize,
    dropped: u64,
}

/// A shared, bounded byte FIFO. Clones are handles to the same FIFO.
#[derive(Debug, Clone)]
pub struct Wire(Arc<Mutex<Fifo>>);

// Models (and the wires between them) move to the simulation thread.
const _: () = {
    const fn shared<T: Send + Sync>() {}
    shared::<Wire>();
};

impl Wire {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "wire capacity must be > 0");
        Self(Arc::new(Mutex::new(Fifo { bytes: VecDeque::new(), capacity, dropped: 0 })))
    }

    fn fifo(&self) -> MutexGuard<'_, Fifo> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Appends `bytes`; if the FIFO overflows, the oldest bytes are discarded.
    pub fn write(&self, bytes: &[u8]) {
        let mut f = self.fifo();
        f.bytes.extend(bytes);
        let excess = f.bytes.len().saturating_sub(f.capacity);
        if excess > 0 {
            f.bytes.drain(..excess);
            f.dropped += excess as u64;
        }
    }

    /// Removes and returns up to `max` bytes, oldest first.
    pub fn take(&self, max: usize) -> Vec<u8> {
        let mut f = self.fifo();
        let n = f.bytes.len().min(max);
        f.bytes.drain(..n).collect()
    }

    pub fn len(&self) -> usize {
        self.fifo().bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes discarded because the FIFO was full.
    pub fn dropped(&self) -> u64 {
        self.fifo().dropped
    }
}
