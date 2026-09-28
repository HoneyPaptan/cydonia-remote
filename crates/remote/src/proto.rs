use artifact::session::{
    chat::{ChatItem, PlanStatus},
    record::ForkOrigin,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub epoch: u64,
    pub seq: u64,
    pub agents: Vec<String>,
    pub projects: Vec<ProjectView>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectView {
    pub path: String,
    pub files: BTreeMap<String, File>,
    pub sessions: BTreeMap<String, SessionView>,
}

impl ProjectView {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct File(#[serde(with = "crate::bytes")] pub Vec<u8>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionView {
    pub header: SessionHeader,
    pub items: Vec<ChatItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionHeader {
    pub agent: String,
    pub number: Option<u64>,
    pub title: String,
    pub name: Option<String>,
    pub updated: u64,
    pub status: Status,
    pub closed: bool,
    pub fork: Option<ForkOrigin>,
    pub sent_at: BTreeMap<usize, u64>,
    pub plan: Vec<(String, PlanStatus)>,
    pub permission: Option<PermissionView>,
    pub queued: usize,
    pub usage: Option<Usage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Idle,
    Connecting,
    Working,
    WaitingForPermission,
    Lost,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PermissionView {
    pub request: u64,
    pub title: String,
    pub options: Vec<PermissionOption>,
    pub always: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PermissionOption {
    pub id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub used: u64,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionKey {
    pub project: String,
    pub record: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub change: Change,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Change {
    Agents {
        agents: Vec<String>,
    },
    ProjectPut {
        project: ProjectView,
    },
    ProjectRemoved {
        path: String,
    },
    ProjectOrder {
        paths: Vec<String>,
    },
    SessionPut {
        key: SessionKey,
        session: SessionView,
    },
    SessionRemoved {
        key: SessionKey,
    },
    SessionHeader {
        key: SessionKey,
        header: SessionHeader,
    },
    ItemsTruncate {
        key: SessionKey,
        len: usize,
    },
    ItemReplace {
        key: SessionKey,
        index: usize,
        item: ChatItem,
    },
    ItemsAppend {
        key: SessionKey,
        items: Vec<ChatItem>,
    },
    FilePut {
        project: String,
        path: String,
        file: File,
    },
    FileRemoved {
        project: String,
        path: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Frame {
    Event { event: Box<Event> },
    Resync,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Command {
    pub id: String,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    SendPrompt {
        key: SessionKey,
        text: String,
    },
    Cancel {
        key: SessionKey,
    },
    RespondPermission {
        key: SessionKey,
        request: u64,
        option: String,
    },
    NewSession {
        project: String,
        agent: Option<String>,
        text: Option<String>,
    },
    CreateBoard {
        project: String,
        name: String,
        key: String,
    },
    SaveBoard {
        project: String,
        board: String,
    },
    RemoveBoard {
        project: String,
        id: String,
    },
    CreateArticle {
        project: String,
        markdown: String,
    },
    WriteArticle {
        project: String,
        id: String,
        markdown: String,
    },
    SaveProperties {
        project: String,
        id: String,
        properties: String,
    },
    RemoveArticle {
        project: String,
        id: String,
    },
}

impl Action {
    pub fn written_project(&self) -> Option<&str> {
        match self {
            Action::CreateBoard { project, .. }
            | Action::SaveBoard { project, .. }
            | Action::RemoveBoard { project, .. }
            | Action::CreateArticle { project, .. }
            | Action::WriteArticle { project, .. }
            | Action::SaveProperties { project, .. }
            | Action::RemoveArticle { project, .. } => Some(project),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ack {
    pub id: String,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outcome {
    Accepted,
    Created { key: SessionKey },
    Rejected { reason: Reason },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    UnknownProject,
    UnknownSession,
    UnknownAgent,
    StalePermission,
    Conflict,
    Invalid,
    Closed,
    Unavailable,
}
