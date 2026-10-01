use artifact::session::{
    chat::{ChatItem, PlanStatus},
    record::ForkOrigin,
};
use cacp::schema::{SessionConfigOption, SessionModeState};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use std::{collections::BTreeMap, sync::Arc};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub epoch: u64,
    pub seq: u64,
    pub agents: Vec<String>,
    pub projects: Vec<ProjectView>,
    #[serde(default)]
    pub setup: Setup,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setup {
    pub switches: BTreeMap<String, bool>,
    pub mcp_url: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectView {
    pub path: String,
    pub files: Arc<BTreeMap<String, File>>,
    pub sessions: BTreeMap<String, SessionView>,
}

impl ProjectView {
    pub fn without_items(&self) -> Self {
        Self {
            path: self.path.clone(),
            files: self.files.clone(),
            sessions: self
                .sessions
                .iter()
                .map(|(record, session)| (record.clone(), session.without_items()))
                .collect(),
        }
    }

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

impl SessionView {
    pub fn without_items(&self) -> Self {
        Self {
            header: self.header.clone(),
            items: Vec::new(),
        }
    }
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
    #[serde(deserialize_with = "indexed")]
    pub sent_at: BTreeMap<usize, u64>,
    pub plan: Vec<(String, PlanStatus)>,
    pub permission: Option<PermissionView>,
    pub queued: usize,
    #[serde(default)]
    pub pending: Vec<String>,
    pub usage: Option<Usage>,
    #[serde(default)]
    pub config: Vec<SessionConfigOption>,
    #[serde(default)]
    pub modes: Option<SessionModeState>,
    #[serde(default)]
    pub commands: Vec<CommandView>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandView {
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mentions {
    pub files: Vec<String>,
    pub skills: Vec<CommandView>,
}

fn indexed<'de, D: Deserializer<'de>>(from: D) -> Result<BTreeMap<usize, u64>, D::Error> {
    BTreeMap::<String, u64>::deserialize(from)?
        .into_iter()
        .map(|(index, at)| index.parse().map(|index| (index, at)).map_err(D::Error::custom))
        .collect()
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
    Setup {
        setup: Setup,
    },
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
    ItemText {
        key: SessionKey,
        index: usize,
        text: String,
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
    Quiet { seq: u64 },
    Session { key: SessionKey, seq: u64, session: Box<SessionView> },
    Ping,
    Resync,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Upstream {
    Focus { keys: Vec<SessionKey> },
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
    SendAttached {
        key: SessionKey,
        text: String,
        files: Vec<Upload>,
    },
    Cancel {
        key: SessionKey,
    },
    Unqueue {
        key: SessionKey,
        index: usize,
        text: String,
    },
    RespondPermission {
        key: SessionKey,
        request: u64,
        option: String,
    },
    SetMode {
        key: SessionKey,
        mode: String,
    },
    SetConfig {
        key: SessionKey,
        config: String,
        value: String,
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
        #[serde(default)]
        id: Option<String>,
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
        #[serde(default)]
        id: Option<String>,
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
    SetCover {
        project: String,
        id: String,
        cover: Option<Cover>,
    },
    RenameSession {
        key: SessionKey,
        name: String,
    },
    ArchiveSession {
        key: SessionKey,
        archived: bool,
    },
    RemoveSession {
        key: SessionKey,
    },
    InstallAgent {
        id: String,
    },
    RemoveAgent {
        id: String,
    },
    OpenProject {
        path: String,
    },
    CloseProject {
        path: String,
    },
    SetSwitch {
        key: String,
        on: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Upload {
    pub name: String,
    pub file: File,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cover {
    pub name: String,
    pub file: File,
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
            | Action::RemoveArticle { project, .. }
            | Action::SetCover { project, .. } => Some(project),
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

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Query {
    ReadDir { path: String },
    ReadFile { path: String },
    WriteFile { path: String, text: String },
    Git { cwd: String, args: Vec<String> },
    Agents,
    Folders { path: String },
    MakeFolder { path: String },
    Mentions { project: String },
    Servers,
    Expose { port: u16 },
    Stop { pid: u32, port: u16 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Answer {
    Dir { entries: Vec<DirEntry> },
    File { file: File },
    Written,
    Output(Output),
    Agents { agents: Vec<AgentListing> },
    Folders(Folders),
    Mentions(Mentions),
    Servers { servers: Vec<LocalServer> },
    Exposed,
    Stopped,
    Failed { message: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalServer {
    pub port: u16,
    pub pid: u32,
    pub process: String,
    pub folder: Option<String>,
    pub title: Option<String>,
    pub shared: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Folders {
    pub path: String,
    pub parent: Option<String>,
    pub folders: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentListing {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub installed: Option<String>,
    pub installable: bool,
    pub busy: bool,
    pub icon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirEntry {
    pub directory: bool,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: File,
    pub stderr: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShellInput {
    Keys { text: String },
    Resize { cols: u16, rows: u16 },
    Scroll { lines: i32 },
    Close,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Screen {
    pub rows: Vec<Vec<Run>>,
    pub cursor: Option<(u16, u16)>,
    pub title: Option<String>,
    pub directory: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub text: String,
    pub fg: Color,
    pub bg: Color,
    #[serde(default, skip_serializing_if = "is_plain")]
    pub style: u8,
}

pub const BOLD: u8 = 1;
pub const DIM: u8 = 2;
pub const ITALIC: u8 = 4;
pub const UNDERLINE: u8 = 8;

fn is_plain(style: &u8) -> bool {
    *style == 0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}
