use artifact::{
    project::{Project as _, memory},
    session::record::Record,
};
use cydonia_gui::model::{
    sink::{self, Sink, Write},
    store,
};
use std::{cell::RefCell, path::Path, rc::Rc};

#[derive(Default)]
struct Recorder(RefCell<Vec<String>>);

impl Sink for Recorder {
    fn send_prompt(&self, _: &Path, _: &str, _: String) {}
    fn send_attached(&self, _: &Path, _: &str, _: String, _: Vec<(String, Vec<u8>)>) {}
    fn cancel(&self, _: &Path, _: &str) {}
    fn respond_permission(&self, _: &Path, _: &str, _: u64, _: String) {}
    fn set_mode(&self, _: &Path, _: &str, _: String) {}
    fn set_config(&self, _: &Path, _: &str, _: String, _: String) {}
    fn new_session(&self, _: &Path, _: &str, _: Option<String>) {}
    fn session(&self, _: &Path, _: &str, _: sink::SessionChange) {}
    fn agent(&self, _: &str, _: bool) {}
    fn project(&self, _: &Path, _: bool) {}
    fn switch(&self, _: &str, _: bool) {}
    fn write(&self, project: &Path, write: Write) {
        let what = match write {
            Write::CreateBoard { name, .. } => format!("create board {name}"),
            Write::SaveBoard(board) => format!("save board {}", board.name),
            Write::RemoveBoard(id) => format!("remove board {id}"),
            Write::CreateArticle { .. } => "create article".to_owned(),
            Write::WriteArticle { markdown, .. } => format!("write article {markdown}"),
            Write::SaveProperties { .. } => "save properties".to_owned(),
            Write::RemoveArticle(_) => "remove article".to_owned(),
            Write::SetCover { .. } => "set cover".to_owned(),
        };
        self.0
            .borrow_mut()
            .push(format!("{} {what}", project.display()));
    }
}

#[test]
fn a_relayed_project_forwards_board_and_article_writes_but_not_sessions() {
    let recorder = Rc::new(Recorder::default());
    sink::install(recorder.clone());
    store::relay("/phone/project", memory::Project::new());
    let held = store::open(Path::new("/phone/project"));

    let mut board = held.create_board("Launch", "LAUNCH").unwrap();
    board.name = "Launch plan".into();
    held.save_board(&mut board).unwrap();
    let article = held.create_article("# Notes").unwrap();
    held.write_article(&article.id, "# Notes, again").unwrap();
    let session = held.create_session().unwrap();
    held.save_session(&Record {
        id: session,
        number: None,
        agent: "fake".into(),
        agent_id: None,
        session: None,
        title: String::new(),
        name: None,
        updated: 0,
        closed: false,
        fork: None,
        draft: "typed on the phone".into(),
        items: Vec::new(),
        sent_at: Default::default(),
    })
    .unwrap();

    assert_eq!(
        *recorder.0.borrow(),
        vec![
            "/phone/project create board Launch",
            "/phone/project save board Launch plan",
            "/phone/project create article",
            "/phone/project write article # Notes, again",
        ]
    );
    assert_eq!(held.boards()[0].name, "Launch plan");
}

#[test]
fn a_plainly_seeded_project_forwards_nothing() {
    let recorder = Rc::new(Recorder::default());
    sink::install(recorder.clone());
    store::seed("/demo/project", memory::Project::new());
    store::open(Path::new("/demo/project"))
        .create_board("Demo", "DEMO")
        .unwrap();
    assert!(recorder.0.borrow().is_empty());
}
