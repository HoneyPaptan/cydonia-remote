use super::*;
use std::cell::RefCell;

#[derive(Default)]
struct Shelf(RefCell<Option<String>>);

impl Keeper for Shelf {
    fn read(&self) -> Option<String> {
        self.0.borrow().clone()
    }

    fn write(&self, body: &str) {
        *self.0.borrow_mut() = Some(body.to_owned());
    }
}

fn entry(id: &str) -> Entry {
    Entry {
        kind: Kind::Article,
        id: id.to_owned(),
    }
}

#[test]
fn a_saved_state_comes_back_from_the_keeper_and_not_from_the_disk() {
    let shelf = Rc::new(Shelf::default());
    keep_with(shelf.clone());
    let mut state = State::default();
    state.projects = vec!["/a".into(), "/b".into()];
    state.active = 1;
    state.last.insert("/b".into(), entry("notes.md"));

    save(&state);
    let back = kept().expect("the keeper holds the state");

    assert_eq!(back.active, 1);
    assert_eq!(back.last.get(&PathBuf::from("/b")).map(|e| e.id.as_str()), Some("notes.md"));
}

#[test]
fn the_active_project_is_found_by_path_when_the_open_list_changed() {
    let mut stored = State::default();
    stored.projects = vec!["/a".into(), "/b".into()];
    stored.active = 1;
    stored.last.insert("/b".into(), entry("board.json"));

    let now = reconcile(stored, vec!["/c".into(), "/b".into()]);

    assert_eq!(now.active, 1);
    assert_eq!(now.projects, vec![PathBuf::from("/c"), PathBuf::from("/b")]);
    assert!(now.last.contains_key(&PathBuf::from("/b")));
}

#[test]
fn a_project_that_is_gone_leaves_the_first_one_active() {
    let mut stored = State::default();
    stored.projects = vec!["/a".into()];

    let now = reconcile(stored, vec!["/c".into(), "/d".into()]);

    assert_eq!(now.active, 0);
}
