use gui::model::state::{self, Keeper};
use remote::proto::Snapshot;
use std::rc::Rc;

const STATE: &str = "cydonia.state";
const SNAPSHOT: &str = "cydonia.snapshot";

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

fn key(what: &str, base: &str) -> String {
    format!("{what}@{base}")
}

fn read(key: &str) -> Option<String> {
    storage()?.get_item(key).ok()?
}

fn write(key: &str, body: &str) {
    if let Some(storage) = storage() {
        let _ = storage.set_item(key, body);
    }
}

struct Shelf {
    key: String,
}

impl Keeper for Shelf {
    fn read(&self) -> Option<String> {
        read(&self.key)
    }

    fn write(&self, body: &str) {
        write(&self.key, body);
    }
}

pub fn install(base: &str) {
    state::keep_with(Rc::new(Shelf {
        key: key(STATE, base),
    }));
}

pub fn cached(base: &str) -> Option<Snapshot> {
    serde_json::from_str(&read(&key(SNAPSHOT, base))?).ok()
}

pub fn remember(base: &str, snapshot: &Snapshot) {
    if let Ok(body) = serde_json::to_string(snapshot) {
        write(&key(SNAPSHOT, base), &body);
    }
}
