use crate::model::{
    session::{ChatSession, Connection},
    store::Store,
};
use artifact::{article::properties, project::Project as _, session::chat::ChatItem};
use remote::proto::{
    CommandView, File, PermissionOption, PermissionView, SessionHeader, SessionView, Status, Usage,
};
use std::{collections::BTreeMap, time::UNIX_EPOCH};

pub fn export(store: &Store) -> BTreeMap<String, File> {
    let mut files = BTreeMap::new();
    for board in store.boards() {
        if let Ok(text) = toml::to_string(&board) {
            files.insert(format!("boards/{}.toml", board.id), File(text.into_bytes()));
        }
    }
    for article in store.articles() {
        if let Ok(markdown) = store.read_article(&article.id) {
            files.insert(
                format!("articles/{}/content.md", article.id),
                File(markdown.into_bytes()),
            );
        }
        if let Some(cover) = article.cover.as_ref().and_then(crate::model::file_url::to_path)
            && let (Some(name), Ok(bytes)) = (
                cover.file_name().and_then(|name| name.to_str()),
                std::fs::read(&cover),
            )
        {
            files.insert(format!("articles/{}/{name}", article.id), File(bytes));
        }
        if let Some(text) = properties::apply("", &store.properties(&article.id)) {
            files.insert(
                format!("articles/{}/properties.toml", article.id),
                File(text.into_bytes()),
            );
        }
    }
    files
}

fn status(chat: &ChatSession) -> Status {
    if chat.permission.is_some() {
        return Status::WaitingForPermission;
    }
    if chat.streaming {
        return Status::Working;
    }
    match chat.connection {
        Connection::Connecting => Status::Connecting,
        Connection::Lost => Status::Lost,
        _ => Status::Idle,
    }
}

fn permission(chat: &ChatSession) -> Option<PermissionView> {
    let prompt = chat.permission.as_ref()?;
    Some(PermissionView {
        request: prompt.request,
        title: prompt.title.clone(),
        options: prompt
            .options
            .iter()
            .map(|choice| PermissionOption {
                id: choice.id.clone(),
                name: choice.name.clone(),
                kind: serde_json::to_value(&choice.kind)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default(),
            })
            .collect(),
        always: prompt.always,
    })
}

pub fn header(chat: &ChatSession) -> SessionHeader {
    SessionHeader {
        agent: chat.entry.name.clone(),
        number: chat.number,
        title: chat.title.clone(),
        name: chat.name.clone(),
        updated: chat
            .updated
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs()),
        status: status(chat),
        closed: chat.closed,
        fork: chat.fork.clone(),
        sent_at: chat.sent_at.clone(),
        plan: chat.plan.clone(),
        permission: permission(chat),
        queued: chat.queue.len(),
        usage: chat.usage.map(|usage| Usage {
            used: usage.used,
            size: usage.size,
        }),
        config: match chat.live() {
            true => chat.config.clone(),
            false => Vec::new(),
        },
        modes: chat.modes.clone().filter(|_| chat.live()),
        commands: chat
            .commands
            .iter()
            .map(|command| CommandView {
                name: command.name.clone(),
                description: command.description.clone(),
            })
            .collect(),
    }
}

pub fn session_view(chat: &ChatSession, items: Vec<ChatItem>) -> SessionView {
    SessionView {
        header: header(chat),
        items,
    }
}
