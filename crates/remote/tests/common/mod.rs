#![allow(dead_code)]

use artifact::session::chat::ChatItem;
use cydonia_remote::proto::{
    File, PermissionOption, PermissionView, ProjectView, SessionHeader, SessionKey, SessionView,
    Status,
};

pub fn header(status: Status) -> SessionHeader {
    SessionHeader {
        agent: "Claude Code".into(),
        number: Some(1),
        title: "Remote protocol".into(),
        name: None,
        updated: 1_790_000_000,
        status,
        closed: false,
        fork: None,
        sent_at: Default::default(),
        plan: Vec::new(),
        permission: None,
        queued: 0,
        usage: None,
        config: Vec::new(),
        modes: None,
    }
}

pub fn session(items: Vec<ChatItem>) -> SessionView {
    SessionView {
        header: header(Status::Idle),
        items,
    }
}

pub fn permission(request: u64) -> PermissionView {
    PermissionView {
        request,
        title: "cargo test --workspace".into(),
        options: vec![PermissionOption {
            id: "allow".into(),
            name: "Allow".into(),
            kind: "allow_once".into(),
        }],
        always: false,
    }
}

pub fn project(path: &str, sessions: &[(&str, SessionView)]) -> ProjectView {
    let mut project = ProjectView::new(path);
    for (record, session) in sessions {
        project.sessions.insert((*record).into(), session.clone());
    }
    project
}

pub fn file(text: &str) -> File {
    File(text.as_bytes().to_vec())
}

pub fn key(project: &str, record: &str) -> SessionKey {
    SessionKey {
        project: project.into(),
        record: record.into(),
    }
}

pub fn user(text: &str) -> ChatItem {
    ChatItem::User(text.into())
}

pub fn agent(text: &str) -> ChatItem {
    ChatItem::Agent(text.into())
}
