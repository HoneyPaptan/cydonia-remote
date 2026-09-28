//! The backend a project's work is kept in.
//!
//! Every read and write of boards, sessions, articles and numbers goes through
//! the [`Store`] that [`open`] answers, and `open` is the one place that picks
//! the backend: the memory project [`seed`] put at a path, else the files
//! there.

use crate::model::sink::{self, Write};
use anyhow::Result;
use artifact::{
    article::{Article, properties::Properties},
    board::Board,
    project::{Project, Watching, fs, memory},
    session::record::Record,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

/// Every backend this build can hold a project in.
#[derive(Clone)]
pub enum Store {
    Fs(fs::Project),
    Memory(Arc<memory::Project>),
    Relayed(Arc<memory::Project>, Arc<Path>),
}

#[derive(Clone)]
struct Held {
    project: Arc<memory::Project>,
    relayed: bool,
}

/// Projects held in memory, by the path they stand at, for the life of the
/// process.
fn seeded() -> &'static Mutex<HashMap<PathBuf, Held>> {
    static SEEDED: OnceLock<Mutex<HashMap<PathBuf, Held>>> = OnceLock::new();
    SEEDED.get_or_init(Default::default)
}

/// Stand `project` at `path`: every [`open`] of that path answers it from here
/// on, and nothing is read from or written to the disk there.
pub fn seed(path: impl Into<PathBuf>, project: memory::Project) {
    hold(path.into(), project, false);
}

pub fn relay(path: impl Into<PathBuf>, project: memory::Project) {
    hold(path.into(), project, true);
}

fn hold(path: PathBuf, project: memory::Project, relayed: bool) {
    seeded()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(
            path,
            Held {
                project: Arc::new(project),
                relayed,
            },
        );
}

pub fn open(path: &Path) -> Store {
    let held = seeded()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(path)
        .cloned();
    match held {
        Some(Held {
            project,
            relayed: true,
        }) => Store::Relayed(project, path.into()),
        Some(Held { project, .. }) => Store::Memory(project),
        None => Store::Fs(fs::Project::new(path)),
    }
}

/// The same call on whichever backend is held.
macro_rules! each {
    ($self:ident, $store:ident => $call:expr) => {
        match $self {
            Store::Fs($store) => $call,
            Store::Memory($store) => $call,
            Store::Relayed($store, _) => $call,
        }
    };
}

impl Store {
    fn relay(&self, write: impl FnOnce() -> Write) {
        if let (Store::Relayed(_, path), Some(sink)) = (self, sink::get()) {
            sink.write(path, write());
        }
    }
}

impl Project for Store {
    fn boards(&self) -> Vec<Board> {
        each!(self, store => store.boards())
    }

    fn board(&self, id: &str) -> Option<Board> {
        each!(self, store => store.board(id))
    }

    fn create_board(&self, name: &str, key: &str) -> Result<Board> {
        let done = each!(self, store => store.create_board(name, key))?;
        self.relay(|| Write::CreateBoard {
            name: name.to_owned(),
            key: key.to_owned(),
        });
        Ok(done)
    }

    fn save_board(&self, board: &mut Board) -> Result<()> {
        let done = each!(self, store => store.save_board(board))?;
        self.relay(|| Write::SaveBoard(board.clone()));
        Ok(done)
    }

    fn remove_board(&self, id: &str) -> Result<()> {
        let done = each!(self, store => store.remove_board(id))?;
        self.relay(|| Write::RemoveBoard(id.to_owned()));
        Ok(done)
    }

    fn sessions(&self) -> Vec<Record> {
        each!(self, store => store.sessions())
    }

    fn session(&self, id: &str) -> Option<Record> {
        each!(self, store => store.session(id))
    }

    fn create_session(&self) -> Result<String> {
        each!(self, store => store.create_session())
    }

    fn save_session(&self, record: &Record) -> Result<()> {
        each!(self, store => store.save_session(record))
    }

    fn remove_session(&self, id: &str) -> Result<()> {
        each!(self, store => store.remove_session(id))
    }

    fn articles(&self) -> Vec<Article> {
        each!(self, store => store.articles())
    }

    fn article(&self, id: &str) -> Option<Article> {
        each!(self, store => store.article(id))
    }

    fn create_article(&self, markdown: &str) -> Result<Article> {
        let done = each!(self, store => store.create_article(markdown))?;
        self.relay(|| Write::CreateArticle(markdown.to_owned()));
        Ok(done)
    }

    fn read_article(&self, id: &str) -> Result<String> {
        each!(self, store => store.read_article(id))
    }

    fn write_article(&self, id: &str, markdown: &str) -> Result<()> {
        let done = each!(self, store => store.write_article(id, markdown))?;
        self.relay(|| Write::WriteArticle {
            id: id.to_owned(),
            markdown: markdown.to_owned(),
        });
        Ok(done)
    }

    fn properties(&self, id: &str) -> Properties {
        each!(self, store => store.properties(id))
    }

    fn save_properties(&self, id: &str, properties: &Properties) -> Result<()> {
        let done = each!(self, store => store.save_properties(id, properties))?;
        self.relay(|| Write::SaveProperties {
            id: id.to_owned(),
            properties: properties.clone(),
        });
        Ok(done)
    }

    fn remove_article(&self, id: &str) -> Result<()> {
        let done = each!(self, store => store.remove_article(id))?;
        self.relay(|| Write::RemoveArticle(id.to_owned()));
        Ok(done)
    }

    fn asset(&self, id: &str, name: &str) -> Result<Vec<u8>> {
        each!(self, store => store.asset(id, name))
    }

    fn put_asset(&self, id: &str, name: &str, bytes: &[u8]) -> Result<()> {
        each!(self, store => store.put_asset(id, name, bytes))
    }

    fn watch(&self, knock: impl Fn() + Send + Sync + 'static) -> Option<Watching> {
        each!(self, store => store.watch(knock))
    }

    fn number(&self, kind: &str, id: &str) -> Result<u64> {
        each!(self, store => store.number(kind, id))
    }

    fn resolve(&self, kind: &str, number: u64) -> Result<Option<String>> {
        each!(self, store => store.resolve(kind, number))
    }

    fn retire(&self, kind: &str, id: &str) -> Result<()> {
        each!(self, store => store.retire(kind, id))
    }
}
