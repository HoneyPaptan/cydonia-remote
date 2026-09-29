use crate::proto::{Change, Event, Frame, SessionKey};
use std::collections::HashMap;

#[derive(Default)]
pub struct Scope {
    focus: HashMap<SessionKey, u64>,
}

fn item_key(change: &Change) -> Option<&SessionKey> {
    match change {
        Change::ItemsTruncate { key, .. }
        | Change::ItemReplace { key, .. }
        | Change::ItemsAppend { key, .. }
        | Change::ItemText { key, .. } => Some(key),
        _ => None,
    }
}

impl Scope {
    pub fn focused(&self, key: &SessionKey) -> bool {
        self.focus.contains_key(key)
    }

    pub fn follow(&mut self, key: SessionKey, from: u64) {
        self.focus.insert(key, from);
    }

    pub fn retain(&mut self, keys: &[SessionKey]) {
        self.focus.retain(|held, _| keys.contains(held));
    }

    pub fn frame(&self, event: Event) -> Frame {
        let Event { seq, change } = event;
        match self.shaped(seq, change) {
            Some(change) => Frame::Event {
                event: Box::new(Event { seq, change }),
            },
            None => Frame::Quiet { seq },
        }
    }

    fn live(&self, key: &SessionKey, seq: u64) -> bool {
        self.focus.get(key).is_some_and(|from| seq > *from)
    }

    fn shaped(&self, seq: u64, change: Change) -> Option<Change> {
        if let Some(key) = item_key(&change) {
            return self.live(key, seq).then_some(change);
        }
        Some(match change {
            Change::ProjectPut { mut project } => {
                let path = project.path.clone();
                for (record, session) in project.sessions.iter_mut() {
                    let key = SessionKey {
                        project: path.clone(),
                        record: record.clone(),
                    };
                    if !self.focused(&key) {
                        session.items.clear();
                    }
                }
                Change::ProjectPut { project }
            }
            Change::SessionPut { key, mut session } => {
                if !self.focused(&key) {
                    session.items.clear();
                }
                Change::SessionPut { key, session }
            }
            other => other,
        })
    }
}
