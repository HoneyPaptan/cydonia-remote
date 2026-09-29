mod common;

use common::{agent, key, session, user};
use cydonia_remote::{
    proto::{Change, Event, Frame},
    scope::Scope,
};

fn text(seq: u64, record: &str) -> Event {
    Event {
        seq,
        change: Change::ItemText {
            key: key("/a", record),
            index: 0,
            text: "more".into(),
        },
    }
}

#[test]
fn item_changes_of_an_unwatched_session_go_quiet() {
    let scope = Scope::default();
    assert_eq!(scope.frame(text(5, "s1")), Frame::Quiet { seq: 5 });
}

#[test]
fn a_watched_session_hears_only_what_came_after_its_catch_up() {
    let mut scope = Scope::default();
    scope.follow(key("/a", "s1"), 5);
    assert_eq!(scope.frame(text(5, "s1")), Frame::Quiet { seq: 5 });
    assert!(matches!(scope.frame(text(6, "s1")), Frame::Event { .. }));
    assert_eq!(scope.frame(text(6, "s2")), Frame::Quiet { seq: 6 });
}

#[test]
fn a_session_put_for_an_unwatched_session_drops_its_transcript() {
    let scope = Scope::default();
    let event = Event {
        seq: 1,
        change: Change::SessionPut {
            key: key("/a", "s1"),
            session: session(vec![user("hi"), agent("yo")]),
        },
    };
    let Frame::Event { event } = scope.frame(event) else {
        panic!("a put is always heard");
    };
    let Change::SessionPut { session, .. } = event.change else {
        panic!("still a put");
    };
    assert!(session.items.is_empty());
}
