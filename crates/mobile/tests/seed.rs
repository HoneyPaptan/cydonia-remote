use artifact::{
    project::Project as _,
    session::chat::{ChatItem, PlanStatus},
};
use cydonia_mobile::seed;
use remote::proto::{File, ProjectView, SessionHeader, SessionView, Snapshot, Status, VERSION};

const BOARD: &str = "id = \"road\"\nname = \"Roadmap\"\nkey = \"ROAD\"\n";

fn view(agent: &str) -> SessionView {
    SessionView {
        header: SessionHeader {
            agent: agent.into(),
            number: Some(3),
            title: "Remote protocol".into(),
            name: Some("Protocol".into()),
            updated: 1_790_000_000,
            status: Status::Working,
            closed: false,
            fork: None,
            sent_at: [(0, 1_790_000_000)].into(),
            plan: vec![("Write the hub".into(), PlanStatus::Active)],
            permission: None,
            queued: 0,
            usage: None,
            config: Vec::new(),
            modes: None,
        },
        items: vec![
            ChatItem::User("go".into()),
            ChatItem::Agent("working".into()),
        ],
    }
}

fn project() -> ProjectView {
    let mut project = ProjectView::new("/work/cydonia-remote");
    project
        .files
        .insert("boards/road.toml".into(), File(BOARD.as_bytes().to_vec()));
    project
        .sessions
        .insert("1790000000300".into(), view("Claude Code"));
    project
}

#[test]
fn a_seeded_project_answers_its_boards_and_sessions() {
    let held = seed::memory_project(&project());
    assert_eq!(held.boards()[0].name, "Roadmap");
    let record = held.session("1790000000300").unwrap();
    assert_eq!(record.title, "Remote protocol");
    assert_eq!(record.items, view("Claude Code").items);
    assert_eq!(record.sent_at, view("Claude Code").header.sent_at);
}

#[test]
fn settings_name_every_agent_the_laptop_knows_or_used() {
    let snapshot = Snapshot {
        version: VERSION,
        epoch: 1,
        seq: 0,
        agents: vec!["Codex".into()],
        projects: vec![project()],
        setup: Default::default(),
    };
    let names: Vec<String> = seed::settings(&snapshot)
        .agents
        .into_iter()
        .map(|agent| agent.name)
        .collect();
    assert_eq!(names, vec!["Codex".to_owned(), "Claude Code".to_owned()]);
    assert_eq!(
        seed::state(&snapshot).projects,
        vec![std::path::PathBuf::from("/work/cydonia-remote")]
    );
}
