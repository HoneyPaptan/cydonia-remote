use super::*;
use remote::proto::File;
use std::sync::Mutex as Lock;

static ASKED: Lock<Vec<Query>> = Lock::new(Vec::new());

fn record(query: Query) {
    ASKED.lock().unwrap().push(query);
}

fn read(path: &str) -> Query {
    Query::ReadFile { path: path.into() }
}

fn file(text: &str) -> Answer {
    Answer::File {
        file: File(text.as_bytes().to_vec()),
    }
}

fn asked(query: &Query) -> usize {
    ASKED.lock().unwrap().iter().filter(|seen| *seen == query).count()
}

#[test]
fn a_question_is_sent_once_and_answered_from_the_reply() {
    install(record, "laptop");
    let query = read("/p/one");

    assert_eq!(ask(query.clone()), Err(Pending));
    assert_eq!(ask(query.clone()), Err(Pending));
    assert_eq!(asked(&query), 1, "a question in flight is not sent again");

    answered(query.clone(), file("one"));
    assert_eq!(ask(query.clone()), Ok(file("one")));
    assert_eq!(asked(&query), 1, "a fresh answer is not asked for again");
}

#[test]
fn a_reply_asked_before_a_local_write_does_not_undo_it() {
    install(record, "laptop");
    let query = read("/p/two");

    assert_eq!(ask(query.clone()), Err(Pending));
    assume(query.clone(), file("written"));
    answered(query.clone(), file("before the write"));

    assert_eq!(ask(query), Ok(file("written")));
}

#[test]
fn a_lost_reply_lets_the_question_be_sent_again() {
    install(record, "laptop");
    let query = read("/p/three");

    assert_eq!(ask(query.clone()), Err(Pending));
    unanswered(&query);
    assert_eq!(ask(query.clone()), Err(Pending));
    assert_eq!(asked(&query), 2);
}

#[test]
fn writes_are_not_kept_as_answers() {
    install(record, "laptop");
    let write = Query::WriteFile {
        path: "/p/four".into(),
        text: "x".into(),
    };
    answered(write.clone(), Answer::Written);

    assert!(!cache().held.contains_key(&write));
}

#[test]
fn a_made_folder_is_listed_without_asking_again() {
    install(record, "laptop");
    let made = Answer::Folders(remote::proto::Folders {
        path: "/p/new".into(),
        parent: Some("/p".into()),
        folders: Vec::new(),
    });
    let make = Query::MakeFolder {
        path: "/p/new".into(),
    };
    answered(make.clone(), made.clone());

    let listing = Query::Folders {
        path: "/p/new".into(),
    };
    assert_eq!(ask(listing.clone()), Ok(made.clone()));
    assert_eq!(asked(&listing), 0);
    assert_eq!(take(&make), Some(made));
    assert_eq!(take(&make), None, "an answer is taken once");
}
