use crate::proto::{Change, Event};
use std::collections::VecDeque;

pub struct Log {
    epoch: u64,
    seq: u64,
    events: VecDeque<Event>,
    capacity: usize,
}

#[derive(Debug, PartialEq)]
pub enum Replay {
    Events(Vec<Event>),
    Resync,
}

impl Log {
    pub fn new(epoch: u64, capacity: usize) -> Self {
        Self {
            epoch,
            seq: 0,
            events: VecDeque::with_capacity(capacity),
            capacity: capacity.max(1),
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn push(&mut self, change: Change) -> Event {
        self.seq += 1;
        let event = Event {
            seq: self.seq,
            change,
        };
        if self.events.len() == self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(event.clone());
        event
    }

    pub fn since(&self, epoch: u64, seq: u64) -> Replay {
        if epoch != self.epoch || seq > self.seq {
            return Replay::Resync;
        }
        if seq == self.seq {
            return Replay::Events(Vec::new());
        }
        match self.events.front() {
            Some(oldest) if oldest.seq <= seq + 1 => Replay::Events(
                self.events
                    .iter()
                    .filter(|event| event.seq > seq)
                    .cloned()
                    .collect(),
            ),
            _ => Replay::Resync,
        }
    }
}
