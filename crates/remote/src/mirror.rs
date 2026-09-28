use crate::proto::{Change, ProjectView, SessionKey, SessionView, Setup, Snapshot, VERSION};
use artifact::session::chat::ChatItem;

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

    fn session_mut(&mut self, key: &SessionKey) -> Option<&mut SessionView> {
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
            | Change::ItemsAppend { key, .. } => {
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
                    project.files.insert(path.clone(), file.clone());
                }
            }
            Change::FileRemoved { project, path } => {
                if let Some(project) = self.project_mut(project) {
                    project.files.remove(path);
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
        let order = |mirror: &Mirror| -> Vec<String> {
            mirror
                .projects
                .iter()
                .map(|project| project.path.clone())
                .collect()
        };
        let mut placed = self.clone();
        placed.apply_all(&changes);
        if order(&placed) != order(next) {
            changes.push(Change::ProjectOrder { paths: order(next) });
        }
        changes
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
        _ => {}
    }
}

fn diff_project(held: &ProjectView, next: &ProjectView, changes: &mut Vec<Change>) {
    for (path, file) in &next.files {
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
        if before[index] != after[index] {
            changes.push(Change::ItemReplace {
                key: key.clone(),
                index,
                item: after[index].clone(),
            });
        }
    }
    if after.len() > before.len() {
        changes.push(Change::ItemsAppend {
            key,
            items: after[before.len()..].to_vec(),
        });
    }
}
