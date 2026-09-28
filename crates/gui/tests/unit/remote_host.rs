use super::*;
use crate::model::{
    session::{ChatSession, Choice, PermissionPrompt},
    settings::Agent,
};
use artifact::session::{chat::ChatItem, record::Record};
use cacp::schema::PermissionOptionKind;
use remote::proto::{SessionKey, Status};
use std::path::{Path, PathBuf};

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
    PermissionPrompt::new(
        request,
        "cargo test --workspace".into(),
        vec![Choice {
            id: "allow".into(),
            name: "Allow".into(),
            kind: PermissionOptionKind::AllowOnce,
        }],
        false,
    )
}

#[test]
fn the_mirror_carries_sessions_and_board_files() {
    let projects = vec![project_with_session("mirror")];
    let mut publisher = Publisher::new(Hub::new(1));
    let mirror = publisher.mirror(&projects, &[agent()]);
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
    let mut publisher = Publisher::new(hub.clone());
    let next = publisher.mirror(&projects, &[agent()]);
    publisher.hub.publish(next);
    let seq = hub.snapshot().seq;
    let next = publisher.mirror(&projects, &[agent()]);
    publisher.hub.publish(next);
    assert_eq!(hub.snapshot().seq, seq);
}

#[test]
fn a_new_transcript_item_becomes_one_event() {
    let mut projects = vec![project_with_session("append")];
    let hub = Hub::new(1);
    let mut publisher = Publisher::new(hub.clone());
    let next = publisher.mirror(&projects, &[agent()]);
    publisher.hub.publish(next);
    let before = hub.snapshot().seq;
    projects[0].sessions[0]
        .items
        .push(ChatItem::Agent("more".into()));
    let next = publisher.mirror(&projects, &[agent()]);
    publisher.hub.publish(next);
    assert_eq!(hub.snapshot().seq, before + 1);
}

#[test]
fn a_standing_prompt_is_waiting_for_permission_with_its_request_id() {
    let mut projects = vec![project_with_session("waiting")];
    projects[0].sessions[0].permission = Some(prompt(41));
    let mut publisher = Publisher::new(Hub::new(1));
    let mirror = publisher.mirror(&projects, &[agent()]);
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

#[test]
fn files_are_exported_once_until_a_reload_asks_again() {
    let projects = vec![project_with_session("reread")];
    let mut publisher = Publisher::new(Hub::new(1));
    publisher.mirror(&projects, &[agent()]);
    assert!(publisher.dirty.is_empty());
    std::fs::remove_file(projects[0].path.join(".cydonia/boards/road.toml")).unwrap();
    let stale = publisher.mirror(&projects, &[agent()]);
    assert!(stale.projects[0].files.contains_key("boards/road.toml"));
    publisher.reread();
    let fresh = publisher.mirror(&projects, &[agent()]);
    assert!(!fresh.projects[0].files.contains_key("boards/road.toml"));
}

fn fs_project(name: &str) -> (PathBuf, crate::model::store::Store) {
    let dir = std::env::temp_dir().join(format!(
        "cydonia-remote-write-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".cydonia/boards")).unwrap();
    std::fs::write(dir.join(".cydonia/boards/road.toml"), BOARD).unwrap();
    let store = crate::model::store::open(&dir);
    (dir, store)
}

fn project_of(dir: &Path) -> String {
    dir.to_string_lossy().into_owned()
}

#[test]
fn a_saved_board_from_the_phone_lands_on_disk() {
    let (dir, store) = fs_project("save-board");
    let mut board = store.board("road").unwrap();
    board.name = "Roadmap, from the phone".into();
    let action = remote::proto::Action::SaveBoard {
        project: project_of(&dir),
        board: toml::to_string(&board).unwrap(),
    };
    super::route::perform(&store, &action).unwrap();
    assert_eq!(store.board("road").unwrap().name, "Roadmap, from the phone");
}

#[test]
fn articles_created_and_written_from_the_phone_land_on_disk() {
    let (dir, store) = fs_project("articles");
    super::route::perform(
        &store,
        &remote::proto::Action::CreateArticle {
            project: project_of(&dir),
            markdown: "# Plan".into(),
        },
    )
    .unwrap();
    let id = store.articles()[0].id.clone();
    super::route::perform(
        &store,
        &remote::proto::Action::WriteArticle {
            project: project_of(&dir),
            id: id.clone(),
            markdown: "# Plan\n\nShip it.".into(),
        },
    )
    .unwrap();
    assert_eq!(store.read_article(&id).unwrap(), "# Plan\n\nShip it.");
}

#[test]
fn a_malformed_board_is_refused_and_nothing_changes() {
    let (dir, store) = fs_project("malformed");
    let action = remote::proto::Action::SaveBoard {
        project: project_of(&dir),
        board: "name = [".into(),
    };
    assert!(super::route::perform(&store, &action).is_err());
    assert_eq!(store.board("road").unwrap().name, "Roadmap");
}

#[test]
fn ids_that_climb_out_of_the_project_are_refused_before_touching_disk() {
    let (dir, store) = fs_project("traversal");
    let outside = dir
        .parent()
        .unwrap()
        .join(format!("escaped-{}.toml", std::process::id()));
    let _ = std::fs::remove_file(&outside);
    let escape = format!(
        "../../../{}",
        outside.file_stem().unwrap().to_string_lossy()
    );
    let board = format!("id = {escape:?}\nname = \"Owned\"\nkey = \"OWN\"\n");
    let project = project_of(&dir);
    let attempts = [
        remote::proto::Action::SaveBoard {
            project: project.clone(),
            board,
        },
        remote::proto::Action::RemoveBoard {
            project: project.clone(),
            id: escape.clone(),
        },
        remote::proto::Action::WriteArticle {
            project: project.clone(),
            id: escape.clone(),
            markdown: "owned".into(),
        },
        remote::proto::Action::RemoveArticle {
            project: project.clone(),
            id: "..".into(),
        },
        remote::proto::Action::SaveProperties {
            project,
            id: "/etc".into(),
            properties: String::new(),
        },
    ];
    for attempt in &attempts {
        assert!(
            super::route::perform(&store, attempt).is_err(),
            "{attempt:?}"
        );
    }
    assert!(!outside.exists());
    assert!(
        std::fs::read_dir(dir.parent().unwrap())
            .unwrap()
            .flatten()
            .all(|entry| { !entry.file_name().to_string_lossy().starts_with("escaped-") })
    );
}

#[test]
fn every_written_id_must_be_a_plain_name_even_where_the_disk_would_refuse_it() {
    let (dir, store) = fs_project("plain");
    let project = project_of(&dir);
    for id in ["..", ".hidden", "a/b", "a\\b", "", "name with space"] {
        let attempt = remote::proto::Action::SaveProperties {
            project: project.clone(),
            id: id.into(),
            properties: String::new(),
        };
        let error = super::route::perform(&store, &attempt)
            .unwrap_err()
            .to_string();
        assert!(error.contains("is not a plain name"), "{id:?}: {error}");
    }
}
