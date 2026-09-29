use crate::{
    log::{Log, Replay},
    mirror::Mirror,
    notice::{self, Notice, Notices},
    proto::{Event, SessionKey, SessionView, Snapshot},
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::{broadcast, watch};

const LOG_CAPACITY: usize = 4096;
const WAKE_CAPACITY: usize = 1024;
const NOTICE_CAPACITY: usize = 64;

pub struct Hub {
    state: Mutex<State>,
    wake: broadcast::Sender<Event>,
    noticed: watch::Sender<u64>,
}

struct State {
    mirror: Mirror,
    log: Log,
    notices: VecDeque<Notice>,
}

fn clock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
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
                notices: VecDeque::new(),
            }),
            wake: broadcast::channel(WAKE_CAPACITY).0,
            noticed: watch::channel(clock()).0,
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
        let raised = notice::between(&state.mirror, &next);
        state.mirror = next;
        self.note(&mut state, raised);
        changes.len()
    }

    fn note(&self, state: &mut State, raised: Vec<notice::Raised>) {
        if raised.is_empty() {
            return;
        }
        let mut id = *self.noticed.borrow();
        for found in raised {
            id = clock().max(id + 1);
            state.notices.push_back(Notice {
                id,
                kind: found.kind,
                project: found.key.project,
                record: found.key.record,
                title: found.title,
                body: found.body,
            });
            if state.notices.len() > NOTICE_CAPACITY {
                state.notices.pop_front();
            }
        }
        self.noticed.send_replace(id);
    }

    pub fn notices_after(&self, after: u64) -> Notices {
        let state = self.state();
        Notices {
            latest: *self.noticed.borrow(),
            notices: state
                .notices
                .iter()
                .filter(|held| held.id > after)
                .cloned()
                .collect(),
        }
    }

    pub fn watch_notices(&self) -> watch::Receiver<u64> {
        self.noticed.subscribe()
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

    pub fn shell_snapshot(&self) -> Snapshot {
        let state = self.state();
        state.mirror.shell(state.log.epoch(), state.log.seq())
    }

    pub fn session_at(&self, key: &SessionKey) -> Option<(SessionView, u64)> {
        let state = self.state();
        state
            .mirror
            .session(key)
            .map(|session| (session.clone(), state.log.seq()))
    }

    pub fn subscribe(&self, epoch: u64, seq: u64) -> Subscription {
        let state = self.state();
        Subscription {
            replay: state.log.since(epoch, seq),
            live: self.wake.subscribe(),
        }
    }
}
