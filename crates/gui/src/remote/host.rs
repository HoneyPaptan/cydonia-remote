use super::{route, view};
use crate::model::{
    project::Project,
    session::ChatSession,
    settings,
    workspace::{Reloaded, Workspace},
};
use anyhow::Result;
use artifact::{project::Project as _, session::chat::ChatItem};
use bezel::gpui::{App, Entity};
use futures::{StreamExt as _, channel::mpsc};
use remote::{
    hub::Hub,
    mirror::Mirror,
    proto::{File, ProjectView},
    server::{self, Config},
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    net::SocketAddr,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Options {
    pub listen: Vec<SocketAddr>,
    pub token: String,
    pub ui: Option<PathBuf>,
}

struct Publisher {
    hub: Arc<Hub>,
    files: HashMap<PathBuf, BTreeMap<String, File>>,
    dirty: HashSet<PathBuf>,
    unloaded: HashMap<String, Vec<ChatItem>>,
}

impl Publisher {
    fn new(hub: Arc<Hub>) -> Self {
        Self {
            hub,
            files: HashMap::new(),
            dirty: HashSet::new(),
            unloaded: HashMap::new(),
        }
    }

    fn follow(&mut self, projects: &[Project]) {
        let open: HashSet<&PathBuf> = projects.iter().map(|project| &project.path).collect();
        self.dirty.retain(|path| open.contains(path));
        self.files.retain(|path, _| open.contains(path));
        for project in projects {
            if !self.files.contains_key(&project.path) {
                self.dirty.insert(project.path.clone());
            }
        }
    }

    fn reread(&mut self) {
        self.dirty.extend(self.files.keys().cloned());
    }

    fn items(&mut self, project: &Project, chat: &ChatSession, record: &str) -> Vec<ChatItem> {
        if !chat.history_unloaded() {
            self.unloaded.remove(record);
            return chat.items.clone();
        }
        self.unloaded
            .entry(record.to_owned())
            .or_insert_with(|| {
                project
                    .store()
                    .session(record)
                    .map(|held| held.items)
                    .unwrap_or_default()
            })
            .clone()
    }

    fn mirror(&mut self, open: &[Project], agents: &[settings::Agent]) -> Mirror {
        self.follow(open);
        let mut projects = Vec::with_capacity(open.len());
        for project in open {
            if self.dirty.remove(&project.path) {
                self.files
                    .insert(project.path.clone(), view::export(&project.store()));
            }
            let mut held = ProjectView::new(project.path.to_string_lossy());
            held.files = self.files.get(&project.path).cloned().unwrap_or_default();
            for chat in &project.sessions {
                let Some(record) = chat.record.clone() else {
                    continue;
                };
                let items = self.items(project, chat, &record);
                held.sessions
                    .insert(record, view::session_view(chat, items));
            }
            projects.push(held);
        }
        Mirror {
            agents: agents.iter().map(|agent| agent.name.clone()).collect(),
            projects,
        }
    }

    fn publish(&mut self, workspace: &Workspace) {
        let next = self.mirror(&workspace.projects, &workspace.settings.agents);
        self.hub.publish(next);
    }
}

fn epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos() as u64)
}

pub fn start(workspace: Entity<Workspace>, options: Options, cx: &mut App) -> Result<()> {
    let hub = Hub::new(epoch());
    let (dispatch, mut actions) = mpsc::unbounded();
    let router = server::router(
        Config {
            token: options.token,
            ui: options.ui,
            local: Some(super::local::Laptop::new(hub.clone())),
        },
        hub.clone(),
        dispatch,
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let listeners = runtime.block_on(server::bind(&options.listen))?;
    std::thread::Builder::new()
        .name("cydonia-remote".into())
        .spawn(move || {
            if let Err(error) = runtime.block_on(server::serve(listeners, router)) {
                eprintln!("remote server stopped: {error}");
            }
        })?;

    let publisher = Rc::new(RefCell::new(Publisher::new(hub)));
    publisher.borrow_mut().publish(workspace.read(cx));

    let observed = publisher.clone();
    cx.observe(&workspace, move |workspace, cx| {
        observed.borrow_mut().publish(workspace.read(cx));
    })
    .detach();

    let reloaded = publisher.clone();
    cx.subscribe(&workspace, move |workspace, _: &Reloaded, cx| {
        let mut publisher = reloaded.borrow_mut();
        publisher.reread();
        publisher.publish(workspace.read(cx));
    })
    .detach();

    cx.spawn(async move |cx| {
        while let Some((action, reply)) = actions.next().await {
            let outcome = workspace.update(cx, |workspace, cx| route::route(workspace, action, cx));
            let _ = reply.send(outcome);
        }
    })
    .detach();
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/remote_host.rs"]
mod tests;
