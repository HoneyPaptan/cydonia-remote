use artifact::{article::properties::Properties, board::Board};
use std::{cell::RefCell, path::Path, rc::Rc};

pub enum Write {
    CreateBoard { name: String, key: String },
    SaveBoard(Board),
    RemoveBoard(String),
    CreateArticle(String),
    WriteArticle { id: String, markdown: String },
    SaveProperties { id: String, properties: Properties },
    RemoveArticle(String),
    SetCover { id: String, cover: Option<(String, Vec<u8>)> },
}

pub enum SessionChange {
    Rename(String),
    Archive(bool),
    Remove,
}

pub trait Sink {
    fn send_prompt(&self, project: &Path, record: &str, text: String);
    fn send_attached(&self, project: &Path, record: &str, text: String, files: Vec<(String, Vec<u8>)>);
    fn cancel(&self, project: &Path, record: &str);
    fn respond_permission(&self, project: &Path, record: &str, request: u64, option: String);
    fn set_mode(&self, project: &Path, record: &str, mode: String);
    fn set_config(&self, project: &Path, record: &str, config: String, value: String);
    fn new_session(&self, project: &Path, agent: &str, text: Option<String>);
    fn write(&self, project: &Path, write: Write);
    fn session(&self, project: &Path, record: &str, change: SessionChange);
    fn agent(&self, id: &str, install: bool);
}

thread_local! {
    static SINK: RefCell<Option<Rc<dyn Sink>>> = const { RefCell::new(None) };
}

pub fn install(sink: Rc<dyn Sink>) {
    SINK.with(|held| *held.borrow_mut() = Some(sink));
}

pub fn get() -> Option<Rc<dyn Sink>> {
    SINK.with(|held| held.borrow().clone())
}
