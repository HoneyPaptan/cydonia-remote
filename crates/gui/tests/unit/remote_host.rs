use super::*;
use crate::model::{
    session::{ChatSession, Choice, PermissionPrompt},
    settings::Agent,
};
use artifact::session::{chat::ChatItem, record::Record};
use cacp::schema::PermissionOptionKind;
use remote::proto::{SessionKey, Status};
use std::path::Path;

const BOARD: &str = r#"
id = "road"
name = "Roadmap"
key = "ROAD"
"#;

fn agent() -> Agent {
    Agent {
        name: "fake".into(),
        id: None,
        command: String::new(),
        args: Vec::new(),
        env: Default::default(),
    }
}

fn record(id: &str, items: Vec<ChatItem>) -> Record {
    Record {
        id: id.into(),
        number: None,
        agent: "fake".into(),
        agent_id: None,
        session: None,
        title: "Remote protocol".into(),
        name: None,
        updated: 1_790_000_000,
        closed: false,
        fork: None,
        draft: String::new(),
        items,
        sent_at: Default::default(),
    }
}

fn project_with_session(name: &str) -> Project {
    let dir =
        std::env::temp_dir().join(format!("cydonia-remote-host-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".cydonia/boards")).unwrap();
    std::fs::write(dir.join(".cydonia/boards/road.toml"), BOARD).unwrap();
    let mut project = Project::new(dir.clone());
    project.sessions.push(ChatSession::restore(
        900,
        dir,
        agent(),
        record(
            "s1",
            vec![ChatItem::User("go".into()), ChatItem::Agent("done".into())],
        ),
    ));
    project
}

fn key(project: &Project, record: &str) -> SessionKey {
    SessionKey {
        project: project.path.to_string_lossy().into_owned(),
        record: record.into(),
    }
}

fn prompt(request: u64) -> PermissionPrompt {
    PermissionPrompt::standing(
        request,
        "cargo test --workspace",
        vec![Choice {
            id: "allow".into(),
            name: "Allow".into(),
            kind: PermissionOptionKind::AllowOnce,
        }],
    )
}

#[test]
fn the_mirror_carries_sessions_and_board_files() {
    let projects = vec![project_with_session("mirror")];
    let mut publisher = Publisher::new(Hub::new(1), None);
    let mirror = publisher.mirror(&projects);
    let project = mirror.project(&projects[0].path.to_string_lossy()).unwrap();
    let session = &project.sessions["s1"];
    assert_eq!(session.items.len(), 2);
    assert_eq!(session.header.title, "Remote protocol");
    assert_eq!(session.header.status, Status::Idle);
    assert!(project.files.contains_key("boards/road.toml"));
}

#[test]
fn publishing_twice_without_change_emits_nothing() {
    let projects = vec![project_with_session("quiet")];
    let hub = Hub::new(1);
    let mut publisher = Publisher::new(hub.clone(), None);
    publisher.publish(&projects);
    let seq = hub.snapshot().seq;
    publisher.publish(&projects);
    assert_eq!(hub.snapshot().seq, seq);
}

#[test]
fn a_new_transcript_item_becomes_one_event() {
    let mut projects = vec![project_with_session("append")];
    let hub = Hub::new(1);
    let mut publisher = Publisher::new(hub.clone(), None);
    publisher.publish(&projects);
    let before = hub.snapshot().seq;
    projects[0].sessions[0]
        .items
        .push(ChatItem::Agent("more".into()));
    publisher.publish(&projects);
    assert_eq!(hub.snapshot().seq, before + 1);
}

#[test]
fn a_standing_prompt_is_waiting_for_permission_with_its_request_id() {
    let mut projects = vec![project_with_session("waiting")];
    projects[0].sessions[0].permission = Some(prompt(41));
    let mut publisher = Publisher::new(Hub::new(1), None);
    let mirror = publisher.mirror(&projects);
    let header = &mirror.session(&key(&projects[0], "s1")).unwrap().header;
    assert_eq!(header.status, Status::WaitingForPermission);
    let permission = header.permission.as_ref().unwrap();
    assert_eq!(permission.request, 41);
    assert_eq!(permission.options[0].kind, "allow_once");
}

#[test]
fn sessions_are_found_by_project_and_record_only() {
    let projects = vec![project_with_session("find")];
    assert_eq!(
        super::route::find(&projects, &key(&projects[0], "s1")),
        Some(900)
    );
    assert_eq!(
        super::route::find(&projects, &key(&projects[0], "s2")),
        None
    );
    let elsewhere = SessionKey {
        project: "/elsewhere".into(),
        record: "s1".into(),
    };
    assert_eq!(super::route::find(&projects, &elsewhere), None);
    assert!(Path::new(&key(&projects[0], "s1").project).is_absolute());
}

#[test]
fn only_the_standing_request_and_one_of_its_options_are_answerable() {
    let mut chat = project_with_session("standing").sessions.remove(0);
    assert!(!super::route::standing(&chat, 1, "allow"));
    chat.permission = Some(prompt(7));
    assert!(super::route::standing(&chat, 7, "allow"));
    assert!(!super::route::standing(&chat, 6, "allow"));
    assert!(!super::route::standing(&chat, 7, "reject"));
}
