use crate::{
    mirror::Mirror,
    proto::{SessionHeader, SessionKey, Status},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Approval,
    Done,
    Lost,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub id: u64,
    pub kind: Kind,
    pub project: String,
    pub record: String,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notices {
    pub latest: u64,
    pub notices: Vec<Notice>,
}

pub struct Raised {
    pub kind: Kind,
    pub key: SessionKey,
    pub title: String,
    pub body: String,
}

fn name_of(header: &SessionHeader) -> String {
    header
        .name
        .clone()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| header.title.clone())
}

fn asks_again(before: &SessionHeader, after: &SessionHeader) -> Option<String> {
    let asked = after.permission.as_ref()?;
    let same = before
        .permission
        .as_ref()
        .is_some_and(|held| held.request == asked.request);
    (!same).then(|| asked.title.clone())
}

fn finished(before: &SessionHeader, after: &SessionHeader) -> bool {
    let busy = matches!(before.status, Status::Working | Status::WaitingForPermission);
    busy && after.status == Status::Idle
}

fn lost(before: &SessionHeader, after: &SessionHeader) -> bool {
    before.status != Status::Lost && after.status == Status::Lost
}

fn raised_by(key: SessionKey, before: &SessionHeader, after: &SessionHeader) -> Vec<Raised> {
    let title = name_of(after);
    let make = |kind, body: String| Raised {
        kind,
        key: key.clone(),
        title: title.clone(),
        body,
    };
    let mut found = Vec::new();
    if let Some(command) = asks_again(before, after) {
        found.push(make(Kind::Approval, command));
    }
    if finished(before, after) {
        found.push(make(Kind::Done, format!("{} finished", after.agent)));
    }
    if lost(before, after) {
        found.push(make(Kind::Lost, format!("{} lost its connection", after.agent)));
    }
    found
}

pub fn between(before: &Mirror, after: &Mirror) -> Vec<Raised> {
    let mut found = Vec::new();
    for project in &after.projects {
        for (record, session) in &project.sessions {
            let key = SessionKey {
                project: project.path.clone(),
                record: record.clone(),
            };
            let Some(held) = before.session(&key) else {
                continue;
            };
            found.extend(raised_by(key, &held.header, &session.header));
        }
    }
    found
}
