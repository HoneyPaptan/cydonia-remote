use crate::{
    log::{Log, Replay},
    mirror::Mirror,
    proto::{Event, Snapshot},
};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::broadcast;

const LOG_CAPACITY: usize = 4096;
const WAKE_CAPACITY: usize = 1024;

pub struct Hub {
    state: Mutex<State>,
    wake: broadcast::Sender<Event>,
}

struct State {
    mirror: Mirror,
    log: Log,
}

pub struct Subscription {
    pub replay: Replay,
    pub live: broadcast::Receiver<Event>,
}

impl Hub {
    pub fn new(epoch: u64) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                mirror: Mirror::default(),
                log: Log::new(epoch, LOG_CAPACITY),
            }),
            wake: broadcast::channel(WAKE_CAPACITY).0,
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn publish(&self, next: Mirror) -> usize {
        let mut state = self.state();
        let changes = state.mirror.diff(&next);
        for change in &changes {
            let event = state.log.push(change.clone());
            let _ = self.wake.send(event);
        }
        state.mirror = next;
        changes.len()
    }

    pub fn watchers(&self) -> usize {
        self.wake.receiver_count()
    }

    pub fn mirror(&self) -> Mirror {
        self.state().mirror.clone()
    }

    pub fn snapshot(&self) -> Snapshot {
        let state = self.state();
        state.mirror.snapshot(state.log.epoch(), state.log.seq())
    }

    pub fn subscribe(&self, epoch: u64, seq: u64) -> Subscription {
        let state = self.state();
        Subscription {
            replay: state.log.since(epoch, seq),
            live: self.wake.subscribe(),
        }
    }
}
