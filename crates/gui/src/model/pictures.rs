use bezel::gpui::{Bounds, Image, ObjectFit, Pixels};
use std::{cell::RefCell, rc::Rc, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Cover,
    Contain,
}

impl Fit {
    pub fn object(self) -> ObjectFit {
        match self {
            Self::Cover => ObjectFit::Cover,
            Self::Contain => ObjectFit::Contain,
        }
    }
}

pub struct Placed<'a> {
    pub key: u64,
    pub image: &'a Arc<Image>,
    pub bounds: Bounds<Pixels>,
    pub clip: Bounds<Pixels>,
    pub holes: Vec<Bounds<Pixels>>,
    pub radius: Pixels,
    pub fit: Fit,
}

pub trait Pictures {
    fn frame(&self);
    fn place(&self, placed: Placed);
}

thread_local! {
    static PICTURES: RefCell<Option<Rc<dyn Pictures>>> = const { RefCell::new(None) };
}

pub fn install(pictures: Rc<dyn Pictures>) {
    PICTURES.with(|held| *held.borrow_mut() = Some(pictures));
}

pub fn get() -> Option<Rc<dyn Pictures>> {
    PICTURES.with(|held| held.borrow().clone())
}

pub fn frame() {
    if let Some(pictures) = get() {
        pictures.frame();
    }
}
