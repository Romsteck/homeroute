use std::collections::VecDeque;

use super::types::LogEntry;

const DEFAULT_CAPACITY: usize = 10_000;

pub struct RingBuffer {
    entries: VecDeque<LogEntry>,
    capacity: usize,
    last_flushed_id: u64,
}

impl RingBuffer {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
            last_flushed_id: 0,
        }
    }

    pub fn push(&mut self, entry: LogEntry) {
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// Drain entries that haven't been flushed yet (id > last_flushed_id).
    /// Updates last_flushed_id to the highest returned id.
    pub fn drain_since_flush(&mut self) -> Vec<LogEntry> {
        let result: Vec<LogEntry> = self
            .entries
            .iter()
            .filter(|e| e.id > self.last_flushed_id)
            .cloned()
            .collect();
        if let Some(last) = result.last() {
            self.last_flushed_id = last.id;
        }
        result
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}
