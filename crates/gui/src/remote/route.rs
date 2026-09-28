use crate::model::{
    project::Project,
    session::ChatSession,
    store::{self, Store},
    workspace::Workspace,
};
use anyhow::Result;
use artifact::{
    article::properties,
    board::Board,
    project::{Project as _, Stale},
};
use bezel::gpui::Context;
use remote::proto::{Action, Outcome, Reason, SessionKey};
use std::path::{Path, PathBuf};

fn rejected(reason: Reason) -> Outcome {
    Outcome::Rejected { reason }
}

pub fn find(projects: &[Project], key: &SessionKey) -> Option<u64> {
    projects
        .iter()
        .find(|project| project.path == Path::new(&key.project))?
        .sessions
        .iter()
        .find(|chat| chat.record.as_deref() == Some(key.record.as_str()))
        .map(|chat| chat.id)
}

pub fn standing(chat: &ChatSession, request: u64, option: &str) -> bool {
    chat.permission.as_ref().is_some_and(|prompt| {
        prompt.request == request && prompt.options.iter().any(|choice| choice.id == option)
    })
}

pub fn perform(store: &Store, action: &Action) -> Result<()> {
    match action {
        Action::CreateBoard { name, key, .. } => store.create_board(name, key).map(drop),
        Action::SaveBoard { board, .. } => store.save_board(&mut toml::from_str::<Board>(board)?),
        Action::RemoveBoard { id, .. } => store.remove_board(id),
        Action::CreateArticle { markdown, .. } => store.create_article(markdown).map(drop),
        Action::WriteArticle { id, markdown, .. } => store.write_article(id, markdown),
        Action::SaveProperties {
            id,
            properties: text,
            ..
        } => store.save_properties(id, &properties::parse(text)),
        Action::RemoveArticle { id, .. } => store.remove_article(id),
        _ => Ok(()),
    }
}

fn written(workspace: &mut Workspace, action: &Action, cx: &mut Context<Workspace>) -> Outcome {
    let Some(path) = action.written_project().map(PathBuf::from) else {
        return rejected(Reason::Invalid);
    };
    if !workspace
        .projects
        .iter()
        .any(|project| project.path == path)
    {
        return rejected(Reason::UnknownProject);
    }
    match perform(&store::open(&path), action) {
        Ok(()) => {
            workspace.reload_project(&path, cx);
            Outcome::Accepted
        }
        Err(error) if error.is::<Stale>() => rejected(Reason::Conflict),
        Err(_) => rejected(Reason::Invalid),
    }
}

pub fn route(workspace: &mut Workspace, action: Action, cx: &mut Context<Workspace>) -> Outcome {
    if action.written_project().is_some() {
        return written(workspace, &action, cx);
    }
    match action {
        Action::SendPrompt { key, text } => {
            let Some(id) = find(&workspace.projects, &key) else {
                return rejected(Reason::UnknownSession);
            };
            workspace.send(id, text, cx);
            Outcome::Accepted
        }
        Action::Cancel { key } => {
            let Some(id) = find(&workspace.projects, &key) else {
                return rejected(Reason::UnknownSession);
            };
            workspace.with_session(id, cx, |chat| chat.cancel());
            Outcome::Accepted
        }
        Action::RespondPermission {
            key,
            request,
            option,
        } => {
            let Some(id) = find(&workspace.projects, &key) else {
                return rejected(Reason::UnknownSession);
            };
            if !workspace
                .session(id)
                .is_some_and(|chat| standing(chat, request, &option))
            {
                return rejected(Reason::StalePermission);
            }
            workspace.with_session(id, cx, |chat| chat.respond_permission(option));
            Outcome::Accepted
        }
        Action::NewSession {
            project,
            agent,
            text,
        } => {
            let path = Path::new(&project);
            if !workspace.projects.iter().any(|held| held.path == path) {
                return rejected(Reason::UnknownProject);
            }
            let entry = match agent {
                Some(name) => workspace
                    .settings
                    .agents
                    .iter()
                    .find(|entry| entry.name == name)
                    .cloned(),
                None => workspace.preferred_agent(),
            };
            let Some(entry) = entry else {
                return rejected(Reason::UnknownAgent);
            };
            let Some(id) = workspace.start_session_in(path, entry, text, cx) else {
                return rejected(Reason::Unavailable);
            };
            match workspace.mint_record(id) {
                Some(record) => Outcome::Created {
                    key: SessionKey { project, record },
                },
                None => rejected(Reason::Unavailable),
            }
        }
        _ => rejected(Reason::Invalid),
    }
}
