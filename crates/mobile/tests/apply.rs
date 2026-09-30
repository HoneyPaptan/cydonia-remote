use artifact::session::{chat::ChatItem, chat::PlanStatus, record::Record};
use cydonia_mobile::{apply, seed};
use gui::model::session::{ChatSession, Connection};
use remote::proto::{PermissionOption, PermissionView, SessionHeader, SessionView, Status, Usage};

fn header(status: Status) -> SessionHeader {
    SessionHeader {
        agent: "Claude Code".into(),
        number: Some(12),
        title: "Implement remote protocol".into(),
        name: None,
        updated: 1_790_000_100,
        status,
        closed: false,
        fork: None,
        sent_at: [(0, 1_790_000_000)].into(),
        plan: vec![("Stream events".into(), PlanStatus::Active)],
        permission: None,
        queued: 0,
        usage: Some(Usage {
            used: 10,
            size: 100,
        }),
        config: Vec::new(),
        modes: None,
        commands: Vec::new(),
    }
}

fn chat() -> ChatSession {
    let view = SessionView {
        header: header(Status::Idle),
        items: vec![ChatItem::User("go".into())],
    };
    let record: Record = seed::record("1790000000300", &view);
    ChatSession::restore(1, "/work/p".into(), seed::agent("Claude Code"), record)
}

#[test]
fn a_working_header_marks_the_session_streaming() {
    let mut chat = chat();
    apply::header(&mut chat, &header(Status::Working));
    assert!(chat.streaming);
    assert!(matches!(chat.connection, Connection::Idle));
    assert_eq!(chat.number, Some(12));
    assert_eq!(chat.plan.len(), 1);
    assert_eq!(chat.usage.map(|usage| usage.used), Some(10));
}

#[test]
fn a_lost_header_marks_the_connection_lost_and_idle() {
    let mut chat = chat();
    apply::header(&mut chat, &header(Status::Lost));
    assert!(!chat.streaming);
    assert!(matches!(chat.connection, Connection::Lost));
}

#[test]
fn a_standing_permission_arrives_with_its_request_id_and_options() {
    let mut chat = chat();
    let mut waiting = header(Status::WaitingForPermission);
    waiting.permission = Some(PermissionView {
        request: 41,
        title: "cargo test --workspace".into(),
        options: vec![
            PermissionOption {
                id: "allow".into(),
                name: "Allow".into(),
                kind: "allow_once".into(),
            },
            PermissionOption {
                id: "deny".into(),
                name: "Deny".into(),
                kind: "reject_once".into(),
            },
        ],
        always: false,
    });
    apply::header(&mut chat, &waiting);
    let prompt = chat.permission.as_ref().unwrap();
    assert_eq!(prompt.request, 41);
    assert_eq!(prompt.options.len(), 2);
    apply::header(&mut chat, &header(Status::Working));
    assert!(chat.permission.is_none());
}
