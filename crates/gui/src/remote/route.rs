use crate::model::{
    media::Attachment,
    project::Project,
    session::ChatSession,
    store::{self, Store},
    switches::Switch,
    workspace::Workspace,
};
use anyhow::Result;
use artifact::{
    article::properties,
    board::Board,
    project::{Project as _, Stale},
};
use bezel::gpui::Context;
use remote::proto::{Action, Cover, Outcome, Reason, SessionKey};
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

fn plain(id: &str) -> Result<&str> {
    anyhow::ensure!(
        !id.is_empty()
            && !id.starts_with('.')
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')),
        "{id:?} is not a plain name"
    );
    Ok(id)
}

pub fn perform(store: &Store, action: &Action) -> Result<()> {
    match action {
        Action::CreateBoard {
            name,
            key,
            id: Some(id),
            ..
        } => store.create_board_as(plain(id)?, name, key).map(drop),
        Action::CreateBoard { name, key, .. } => store.create_board(name, key).map(drop),
        Action::SaveBoard { board, .. } => {
            let mut board = toml::from_str::<Board>(board)?;
            plain(&board.id)?;
            store.save_board(&mut board)
        }
        Action::RemoveBoard { id, .. } => store.remove_board(plain(id)?),
        Action::CreateArticle {
            markdown,
            id: Some(id),
            ..
        } => store.create_article_as(plain(id)?, markdown).map(drop),
        Action::CreateArticle { markdown, .. } => store.create_article(markdown).map(drop),
        Action::WriteArticle { id, markdown, .. } => store.write_article(plain(id)?, markdown),
        Action::SaveProperties {
            id,
            properties: text,
            ..
        } => store.save_properties(plain(id)?, &properties::parse(text)),
        Action::RemoveArticle { id, .. } => store.remove_article(plain(id)?),
        _ => Ok(()),
    }
}

fn place_cover(project: &Path, id: &str, cover: Option<&Cover>) -> Result<()> {
    use artifact::article as layout;
    let content = layout::content(&layout::dir(project).join(plain(id)?));
    anyhow::ensure!(content.exists(), "no article {id}");
    while let Some(old) = layout::cover::of(&content) {
        std::fs::remove_file(old)?;
    }
    if let Some(cover) = cover {
        let name = plain(&cover.name)?;
        anyhow::ensure!(name.starts_with(layout::cover::MARK), "{name:?} is not a cover");
        std::fs::write(content.with_file_name(name), &cover.file.0)?;
    }
    Ok(())
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
    let done = match action {
        Action::SetCover { id, cover, .. } => place_cover(&path, id, cover.as_ref()),
        _ => perform(&store::open(&path), action),
    };
    match done {
        Ok(()) => {
            workspace.reload_project(&path, cx);
            Outcome::Accepted
        }
        Err(error) if error.is::<Stale>() => rejected(Reason::Conflict),
        Err(_) => rejected(Reason::Invalid),
    }
}

fn agent_job(
    id: String,
    cx: &mut Context<Workspace>,
    job: impl FnOnce(String) -> Result<()> + Send + 'static,
) {
    cx.spawn(async move |workspace, cx| {
        let done = cx.background_executor().spawn(async move { job(id) }).await;
        if let Err(error) = done {
            eprintln!("agent job failed: {error:#}");
        }
        let _ = workspace.update(cx, |workspace, cx| workspace.reload_settings(cx));
    })
    .detach();
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
        Action::SendAttached { key, text, files } => {
            let Some(id) = find(&workspace.projects, &key) else {
                return rejected(Reason::UnknownSession);
            };
            let attachments: Vec<_> = files
                .into_iter()
                .map(|upload| Attachment::Upload {
                    name: upload.name,
                    bytes: upload.file.0.into(),
                })
                .collect();
            workspace.send_attached(id, text, &attachments, cx);
            Outcome::Accepted
        }
        Action::Cancel { key } => {
            let Some(id) = find(&workspace.projects, &key) else {
                return rejected(Reason::UnknownSession);
            };
            workspace.with_session(id, cx, |chat| chat.cancel());
            Outcome::Accepted
        }
        Action::Unqueue { key, index, text } => {
            let Some(id) = find(&workspace.projects, &key) else {
                return rejected(Reason::UnknownSession);
            };
            workspace.with_session(id, cx, |chat| {
                chat.take_queued(index, &text);
            });
            Outcome::Accepted
        }
        Action::SetMode { key, mode } => {
            let Some(id) = find(&workspace.projects, &key) else {
                return rejected(Reason::UnknownSession);
            };
            workspace.set_session_mode(id, mode, cx);
            Outcome::Accepted
        }
        Action::SetConfig { key, config, value } => {
            let Some(id) = find(&workspace.projects, &key) else {
                return rejected(Reason::UnknownSession);
            };
            workspace.set_session_config(
                id,
                config,
                cacp::schema::SessionConfigOptionValue::ValueId {
                    value: value.into(),
                },
                cx,
            );
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
        Action::RenameSession { key, name } => {
            let Some(id) = find(&workspace.projects, &key) else {
                return rejected(Reason::UnknownSession);
            };
            workspace.rename_session(id, name, cx);
            Outcome::Accepted
        }
        Action::ArchiveSession { key, archived } => {
            let Some(id) = find(&workspace.projects, &key) else {
                return rejected(Reason::UnknownSession);
            };
            workspace.archive_session(id, archived, cx);
            Outcome::Accepted
        }
        Action::InstallAgent { id } => {
            agent_job(id, cx, |id| crate::agent::install_listed(&id));
            Outcome::Accepted
        }
        Action::RemoveAgent { id } => {
            agent_job(id, cx, |id| crate::agent::remove(&id));
            Outcome::Accepted
        }
        Action::OpenProject { path } => {
            let path = PathBuf::from(path);
            if !path.is_absolute() || !path.is_dir() {
                return rejected(Reason::Invalid);
            }
            workspace.open_project(path, cx);
            Outcome::Accepted
        }
        Action::CloseProject { path } => {
            let Some(ix) = workspace
                .projects
                .iter()
                .position(|project| project.path == Path::new(&path))
            else {
                return rejected(Reason::UnknownProject);
            };
            workspace.close_project(ix, cx);
            Outcome::Accepted
        }
        Action::SetSwitch { key, on } => {
            let Some(switch) = Switch::parse(&key) else {
                return rejected(Reason::Invalid);
            };
            workspace.set_switch(switch, on, cx);
            Outcome::Accepted
        }
        Action::RemoveSession { key } => {
            let Some(id) = find(&workspace.projects, &key) else {
                return rejected(Reason::UnknownSession);
            };
            workspace.close_session(id, cx);
            Outcome::Accepted
        }
        _ => rejected(Reason::Invalid),
    }
}
