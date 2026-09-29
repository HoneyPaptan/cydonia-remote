use crate::proto::{Change, ProjectView, SessionKey, SessionView, Setup, Snapshot, VERSION};
use artifact::session::chat::ChatItem;
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mirror {
    pub agents: Vec<String>,
    pub projects: Vec<ProjectView>,
    pub setup: Setup,
}

impl Mirror {
    pub fn from_snapshot(snapshot: Snapshot) -> Self {
        Self {
            agents: snapshot.agents,
            projects: snapshot.projects,
            setup: snapshot.setup,
        }
    }

    pub fn snapshot(&self, epoch: u64, seq: u64) -> Snapshot {
        Snapshot {
            version: VERSION,
            epoch,
            seq,
            agents: self.agents.clone(),
            projects: self.projects.clone(),
            setup: self.setup.clone(),
        }
    }

    pub fn shell(&self, epoch: u64, seq: u64) -> Snapshot {
        Snapshot {
            version: VERSION,
            epoch,
            seq,
            agents: self.agents.clone(),
            projects: self.projects.iter().map(ProjectView::without_items).collect(),
            setup: self.setup.clone(),
        }
    }

    pub fn project(&self, path: &str) -> Option<&ProjectView> {
        self.projects.iter().find(|project| project.path == path)
    }

    fn project_mut(&mut self, path: &str) -> Option<&mut ProjectView> {
        self.projects
            .iter_mut()
            .find(|project| project.path == path)
    }

    pub fn session(&self, key: &SessionKey) -> Option<&SessionView> {
        self.project(&key.project)?.sessions.get(&key.record)
    }

    pub fn session_mut(&mut self, key: &SessionKey) -> Option<&mut SessionView> {
        self.project_mut(&key.project)?
            .sessions
            .get_mut(&key.record)
    }

    pub fn apply(&mut self, change: &Change) {
        match change {
            Change::Setup { setup } => self.setup = setup.clone(),
            Change::Agents { agents } => self.agents = agents.clone(),
            Change::ProjectPut { project } => match self.project_mut(&project.path) {
                Some(held) => *held = project.clone(),
                None => self.projects.push(project.clone()),
            },
            Change::ProjectRemoved { path } => {
                self.projects.retain(|project| &project.path != path)
            }
            Change::ProjectOrder { paths } => self.projects.sort_by_key(|project| {
                paths
                    .iter()
                    .position(|path| path == &project.path)
                    .unwrap_or(usize::MAX)
            }),
            Change::SessionPut { key, session } => {
                if let Some(project) = self.project_mut(&key.project) {
                    project.sessions.insert(key.record.clone(), session.clone());
                }
            }
            Change::SessionRemoved { key } => {
                if let Some(project) = self.project_mut(&key.project) {
                    project.sessions.remove(&key.record);
                }
            }
            Change::SessionHeader { key, header } => {
                if let Some(session) = self.session_mut(key) {
                    session.header = header.clone();
                }
            }
            Change::ItemsTruncate { key, .. }
            | Change::ItemReplace { key, .. }
            | Change::ItemsAppend { key, .. }
            | Change::ItemText { key, .. } => {
                if let Some(session) = self.session_mut(key) {
                    apply_items(&mut session.items, change);
                }
            }
            Change::FilePut {
                project,
                path,
                file,
            } => {
                if let Some(project) = self.project_mut(project) {
                    Arc::make_mut(&mut project.files).insert(path.clone(), file.clone());
                }
            }
            Change::FileRemoved { project, path } => {
                if let Some(project) = self.project_mut(project) {
                    Arc::make_mut(&mut project.files).remove(path);
                }
            }
        }
    }

    pub fn diff(&self, next: &Mirror) -> Vec<Change> {
        let mut changes = Vec::new();
        if self.setup != next.setup {
            changes.push(Change::Setup {
                setup: next.setup.clone(),
            });
        }
        if self.agents != next.agents {
            changes.push(Change::Agents {
                agents: next.agents.clone(),
            });
        }
        for project in &self.projects {
            if next.project(&project.path).is_none() {
                changes.push(Change::ProjectRemoved {
                    path: project.path.clone(),
                });
            }
        }
        for project in &next.projects {
            match self.project(&project.path) {
                None => changes.push(Change::ProjectPut {
                    project: project.clone(),
                }),
                Some(held) => diff_project(held, project, &mut changes),
            }
        }
        if self.order_after_puts(next) != next.paths() {
            changes.push(Change::ProjectOrder {
                paths: next.paths().into_iter().map(str::to_owned).collect(),
            });
        }
        changes
    }

    fn paths(&self) -> Vec<&str> {
        self.projects.iter().map(|project| project.path.as_str()).collect()
    }

    fn order_after_puts<'a>(&'a self, next: &'a Mirror) -> Vec<&'a str> {
        let kept = self
            .projects
            .iter()
            .filter(|project| next.project(&project.path).is_some());
        let added = next
            .projects
            .iter()
            .filter(|project| self.project(&project.path).is_none());
        kept.chain(added).map(|project| project.path.as_str()).collect()
    }

    pub fn apply_all(&mut self, changes: &[Change]) {
        for change in changes {
            self.apply(change);
        }
    }
}

pub fn apply_items(items: &mut Vec<ChatItem>, change: &Change) {
    match change {
        Change::ItemsTruncate { len, .. } => items.truncate(*len),
        Change::ItemReplace { index, item, .. } => {
            if let Some(slot) = items.get_mut(*index) {
                *slot = item.clone();
            }
        }
        Change::ItemsAppend { items: added, .. } => items.extend(added.iter().cloned()),
        Change::ItemText { index, text, .. } => {
            if let Some(item) = items.get_mut(*index) {
                grow(item, text);
            }
        }
        _ => {}
    }
}

fn appended<'a>(before: &str, after: &'a str) -> Option<&'a str> {
    after.strip_prefix(before).filter(|added| !added.is_empty())
}

pub fn growth(before: &ChatItem, after: &ChatItem) -> Option<String> {
    let added = match (before, after) {
        (ChatItem::Agent(was), ChatItem::Agent(now)) => appended(was, now),
        (
            ChatItem::Thinking { text: was, done: was_done },
            ChatItem::Thinking { text: now, done },
        ) if was_done == done => appended(was, now),
        (
            ChatItem::Tool { id, kind, label, status, output: was },
            ChatItem::Tool { id: next_id, kind: next_kind, label: next_label, status: next_status, output: now },
        ) if (id, kind, label, status) == (next_id, next_kind, next_label, next_status) => {
            appended(was, now)
        }
        (
            ChatItem::Process { command, output: was },
            ChatItem::Process { command: next, output: now },
        ) if command == next => appended(was, now),
        _ => None,
    };
    added.map(str::to_owned)
}

fn grow(item: &mut ChatItem, text: &str) {
    match item {
        ChatItem::Agent(held) => held.push_str(text),
        ChatItem::Thinking { text: held, .. } => held.push_str(text),
        ChatItem::Tool { output, .. } | ChatItem::Process { output, .. } => output.push_str(text),
        _ => {}
    }
}

fn diff_project(held: &ProjectView, next: &ProjectView, changes: &mut Vec<Change>) {
    diff_files(held, next, changes);
    diff_sessions(held, next, changes);
}

fn diff_files(held: &ProjectView, next: &ProjectView, changes: &mut Vec<Change>) {
    if Arc::ptr_eq(&held.files, &next.files) {
        return;
    }
    for (path, file) in next.files.iter() {
        if held.files.get(path) != Some(file) {
            changes.push(Change::FilePut {
                project: next.path.clone(),
                path: path.clone(),
                file: file.clone(),
            });
        }
    }
    for path in held.files.keys() {
        if !next.files.contains_key(path) {
            changes.push(Change::FileRemoved {
                project: next.path.clone(),
                path: path.clone(),
            });
        }
    }
}

fn diff_sessions(held: &ProjectView, next: &ProjectView, changes: &mut Vec<Change>) {
    for record in held.sessions.keys() {
        if !next.sessions.contains_key(record) {
            changes.push(Change::SessionRemoved {
                key: key(&next.path, record),
            });
        }
    }
    for (record, session) in &next.sessions {
        let key = key(&next.path, record);
        match held.sessions.get(record) {
            None => changes.push(Change::SessionPut {
                key,
                session: session.clone(),
            }),
            Some(before) => diff_session(key, before, session, changes),
        }
    }
}

fn key(project: &str, record: &str) -> SessionKey {
    SessionKey {
        project: project.to_owned(),
        record: record.to_owned(),
    }
}

fn diff_session(
    key: SessionKey,
    before: &SessionView,
    after: &SessionView,
    changes: &mut Vec<Change>,
) {
    if before.header != after.header {
        changes.push(Change::SessionHeader {
            key: key.clone(),
            header: after.header.clone(),
        });
    }
    diff_items(key, &before.items, &after.items, changes);
}

fn diff_items(key: SessionKey, before: &[ChatItem], after: &[ChatItem], changes: &mut Vec<Change>) {
    let shared = before.len().min(after.len());
    if after.len() < before.len() {
        changes.push(Change::ItemsTruncate {
            key: key.clone(),
            len: after.len(),
        });
    }
    for index in 0..shared {
        if before[index] == after[index] {
            continue;
        }
        changes.push(match growth(&before[index], &after[index]) {
            Some(text) => Change::ItemText {
                key: key.clone(),
                index,
                text,
            },
            None => Change::ItemReplace {
                key: key.clone(),
                index,
                item: after[index].clone(),
            },
        });
    }
    if after.len() > before.len() {
        changes.push(Change::ItemsAppend {
            key,
            items: after[before.len()..].to_vec(),
        });
    }
}
