use std::{cell::RefCell, path::Path, rc::Rc};

pub trait Sink {
    fn send_prompt(&self, project: &Path, record: &str, text: String);
    fn cancel(&self, project: &Path, record: &str);
    fn respond_permission(&self, project: &Path, record: &str, request: u64, option: String);
    fn new_session(&self, project: &Path, agent: &str, text: Option<String>);
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
