use futures::channel::oneshot;
use std::{cell::RefCell, rc::Rc};

pub struct Picked {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Photos,
    Files,
}

pub trait Picker {
    fn pick(&self, kind: Kind) -> oneshot::Receiver<Vec<Picked>>;
}

type Pasted = Rc<dyn Fn(Vec<Picked>)>;

thread_local! {
    static PICKER: RefCell<Option<Rc<dyn Picker>>> = const { RefCell::new(None) };
    static PASTED: RefCell<Option<Pasted>> = const { RefCell::new(None) };
}

pub fn on_paste(receiver: Pasted) {
    PASTED.with(|held| *held.borrow_mut() = Some(receiver));
}

pub fn pasted(files: Vec<Picked>) {
    if let Some(receiver) = PASTED.with(|held| held.borrow().clone()) {
        receiver(files);
    }
}

pub fn install(picker: Rc<dyn Picker>) {
    PICKER.with(|held| *held.borrow_mut() = Some(picker));
}

pub fn get() -> Option<Rc<dyn Picker>> {
    PICKER.with(|held| held.borrow().clone())
}
