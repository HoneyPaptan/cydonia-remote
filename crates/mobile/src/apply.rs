use crate::seed;
use artifact::project::Project as _;
use bezel::gpui::Context;
use gui::model::{
    session::{ChatSession, Choice, Command, Connection, PermissionPrompt, Usage},
    store, switches,
    workspace::Workspace,
};
use remote::{
    mirror::{Mirror, apply_items},
    proto::{
        Change, PermissionView, ProjectView, SessionHeader, SessionKey, SessionView, Setup, Status,
    },
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use web_time::UNIX_EPOCH;

fn prompt(view: &PermissionView) -> PermissionPrompt {
    PermissionPrompt::new(
        view.request,
        view.title.clone(),
        view.options
            .iter()
            .filter_map(|option| {
                Some(Choice {
                    id: option.id.clone(),
                    name: option.name.clone(),
                    kind: serde_json::from_value(serde_json::Value::String(option.kind.clone()))
                        .ok()?,
                })
            })
            .collect(),
        view.always,
    )
}

pub fn header(chat: &mut ChatSession, header: &SessionHeader) {
    chat.title = header.title.clone();
    chat.name = header.name.clone();
    chat.number = header.number;
    chat.closed = header.closed;
    chat.fork = header.fork.clone();
    chat.sent_at = header.sent_at.clone();
    chat.plan = header.plan.clone();
    chat.updated = UNIX_EPOCH + Duration::from_secs(header.updated);
    chat.follow_turn(matches!(
        header.status,
        Status::Working | Status::WaitingForPermission
    ));
    chat.connection = match header.status {
        Status::Connecting => Connection::Connecting,
        Status::Lost => Connection::Lost,
        _ => Connection::Idle,
    };
    chat.permission = header.permission.as_ref().map(prompt);
    chat.config = header.config.clone();
    chat.modes = header.modes.clone();
    chat.commands = header
        .commands
        .iter()
        .map(|command| Command {
            name: command.name.clone(),
            description: command.description.clone(),
        })
        .collect();
    chat.usage = header.usage.map(|usage| Usage {
        used: usage.used,
        size: usage.size,
    });
}

fn keep(chat: &mut ChatSession, record: &str, view: &SessionView) {
    if chat.history_unloaded() {
        let _ = store::open(&chat.cwd).save_session(&seed::record(record, view));
    } else {
        chat.items = view.items.clone();
    }
    header(chat, &view.header);
}

pub(crate) fn chat_mut<'a>(workspace: &'a mut Workspace, key: &SessionKey) -> Option<&'a mut ChatSession> {
    workspace
        .projects
        .iter_mut()
        .find(|project| project.path == Path::new(&key.project))?
        .sessions
        .iter_mut()
        .find(|chat| chat.record.as_deref() == Some(key.record.as_str()))
}

fn sessions(workspace: &mut Workspace, view: &ProjectView) {
    let path = PathBuf::from(&view.path);
    let Some(project) = workspace
        .projects
        .iter_mut()
        .find(|project| project.path == path)
    else {
        return;
    };
    project.sessions.retain(|chat| {
        chat.record
            .as_deref()
            .is_none_or(|record| view.sessions.contains_key(record))
    });
    for (record, session) in &view.sessions {
        let key = SessionKey {
            project: view.path.clone(),
            record: record.clone(),
        };
        if chat_mut(workspace, &key).is_none() {
            workspace.adopt_session(&path, seed::record(record, session));
        }
        if let Some(chat) = chat_mut(workspace, &key) {
            keep(chat, record, session);
        }
    }
}

fn reload(workspace: &mut Workspace, view: &ProjectView, cx: &mut Context<Workspace>) {
    seed::project(view);
    let path = PathBuf::from(&view.path);
    if workspace
        .projects
        .iter()
        .any(|project| project.path == path)
    {
        workspace.reload_project(&path, cx);
    } else {
        workspace.open_project(path, cx);
    }
}

fn project(workspace: &mut Workspace, view: &ProjectView, cx: &mut Context<Workspace>) {
    reload(workspace, view, cx);
    sessions(workspace, view);
}

fn close(workspace: &mut Workspace, path: &str, cx: &mut Context<Workspace>) {
    if let Some(ix) = workspace
        .projects
        .iter()
        .position(|project| project.path == Path::new(path))
    {
        workspace.close_project(ix, cx);
    }
}

fn setup(workspace: &mut Workspace, setup: &Setup) {
    let settings = &mut workspace.settings;
    switches::adopt(setup, &mut settings.features, &mut settings.mcp);
}

pub fn everything(workspace: &mut Workspace, mirror: &Mirror, cx: &mut Context<Workspace>) {
    setup(workspace, &mirror.setup);
    let open: Vec<String> = workspace
        .projects
        .iter()
        .map(|project| project.path.to_string_lossy().into_owned())
        .collect();
    for path in open {
        if mirror.project(&path).is_none() {
            close(workspace, &path, cx);
        }
    }
    for view in &mirror.projects {
        project(workspace, view, cx);
    }
    cx.notify();
}

pub fn change(
    workspace: &mut Workspace,
    mirror: &Mirror,
    change: &Change,
    cx: &mut Context<Workspace>,
) {
    match change {
        Change::Setup { setup: held } => setup(workspace, held),
        Change::Agents { agents } => {
            workspace
                .settings
                .agents
                .retain(|agent| agents.contains(&agent.name));
            for name in agents {
                if !workspace
                    .settings
                    .agents
                    .iter()
                    .any(|agent| &agent.name == name)
                {
                    workspace.settings.agents.push(seed::agent(name));
                }
            }
        }
        Change::ProjectPut { project: view } => project(workspace, view, cx),
        Change::ProjectRemoved { path } => close(workspace, path, cx),
        Change::ProjectOrder { .. } => {}
        Change::FilePut { project: path, .. } | Change::FileRemoved { project: path, .. } => {
            if let Some(view) = mirror.project(path) {
                reload(workspace, view, cx);
            }
        }
        Change::SessionPut { key, .. } | Change::SessionRemoved { key } => {
            if let Some(view) = mirror.project(&key.project) {
                sessions(workspace, view);
            }
        }
        Change::SessionHeader { key, header: next } => {
            if let Some(chat) = chat_mut(workspace, key) {
                header(chat, next);
            }
        }
        Change::ItemsTruncate { key, .. }
        | Change::ItemReplace { key, .. }
        | Change::ItemsAppend { key, .. }
        | Change::ItemText { key, .. } => {
            let Some(chat) = chat_mut(workspace, key) else {
                return;
            };
            if chat.history_unloaded() {
                if let Some(view) = mirror.session(key) {
                    keep(chat, &key.record, view);
                }
            } else {
                apply_items(&mut chat.items, change);
            }
        }
    }
    cx.notify();
}
