mod common;

use common::{agent, file, header, key, permission, project, session, user};
use cydonia_remote::{
    mirror::Mirror,
    proto::{Change, Status},
};

fn assert_round_trip(before: &Mirror, after: &Mirror) -> Vec<Change> {
    let changes = before.diff(after);
    let mut rebuilt = before.clone();
    rebuilt.apply_all(&changes);
    assert_eq!(&rebuilt, after);
    changes
}

fn mirror(projects: Vec<cydonia_remote::proto::ProjectView>) -> Mirror {
    Mirror {
        agents: vec!["Claude Code".into()],
        projects,
        ..Mirror::default()
    }
}

#[test]
fn identical_mirrors_produce_no_changes() {
    let held = mirror(vec![project("/a", &[("s1", session(vec![user("hi")]))])]);
    assert!(held.diff(&held.clone()).is_empty());
}

#[test]
fn streaming_frame_replaces_only_the_last_item() {
    let before = mirror(vec![project(
        "/a",
        &[("s1", session(vec![user("go"), agent("Wor")]))],
    )]);
    let after = mirror(vec![project(
        "/a",
        &[("s1", session(vec![user("go"), agent("Working")]))],
    )]);
    let changes = assert_round_trip(&before, &after);
    assert_eq!(
        changes,
        vec![Change::ItemText {
            key: key("/a", "s1"),
            index: 1,
            text: "king".into(),
        }]
    );
}

#[test]
fn a_rewritten_item_is_replaced_whole() {
    let before = mirror(vec![project("/a", &[("s1", session(vec![agent("Wor")]))])]);
    let after = mirror(vec![project("/a", &[("s1", session(vec![agent("Done")]))])]);
    let changes = assert_round_trip(&before, &after);
    assert!(matches!(changes.as_slice(), [Change::ItemReplace { .. }]));
}

#[test]
fn new_items_are_appended_in_one_change() {
    let before = mirror(vec![project("/a", &[("s1", session(vec![user("go")]))])]);
    let after = mirror(vec![project(
        "/a",
        &[("s1", session(vec![user("go"), agent("a"), agent("b")]))],
    )]);
    let changes = assert_round_trip(&before, &after);
    assert_eq!(
        changes,
        vec![Change::ItemsAppend {
            key: key("/a", "s1"),
            items: vec![agent("a"), agent("b")],
        }]
    );
}

#[test]
fn shrinking_transcript_truncates() {
    let before = mirror(vec![project(
        "/a",
        &[("s1", session(vec![user("a"), agent("b"), agent("c")]))],
    )]);
    let after = mirror(vec![project("/a", &[("s1", session(vec![user("a")]))])]);
    assert_round_trip(&before, &after);
}

#[test]
fn header_change_is_one_change() {
    let before = mirror(vec![project("/a", &[("s1", session(vec![]))])]);
    let mut waiting = session(vec![]);
    waiting.header = header(Status::WaitingForPermission);
    waiting.header.permission = Some(permission(7));
    let after = mirror(vec![project("/a", &[("s1", waiting)])]);
    let changes = assert_round_trip(&before, &after);
    assert!(matches!(changes.as_slice(), [Change::SessionHeader { .. }]));
}

#[test]
fn sessions_and_projects_come_and_go() {
    let before = mirror(vec![
        project("/a", &[("s1", session(vec![user("x")]))]),
        project("/b", &[]),
    ]);
    let after = mirror(vec![
        project("/c", &[]),
        project("/a", &[("s2", session(vec![agent("y")]))]),
    ]);
    assert_round_trip(&before, &after);
}

#[test]
fn reordered_projects_are_reordered() {
    let before = mirror(vec![project("/a", &[]), project("/b", &[])]);
    let after = mirror(vec![project("/b", &[]), project("/a", &[])]);
    let changes = assert_round_trip(&before, &after);
    assert!(matches!(changes.as_slice(), [Change::ProjectOrder { .. }]));
}

#[test]
fn files_are_put_and_removed() {
    let mut before = project("/a", &[]);
    std::sync::Arc::make_mut(&mut before.files).insert("boards/one.toml".into(), file("old"));
    std::sync::Arc::make_mut(&mut before.files).insert("boards/two.toml".into(), file("gone"));
    let mut after = project("/a", &[]);
    std::sync::Arc::make_mut(&mut after.files).insert("boards/one.toml".into(), file("new"));
    std::sync::Arc::make_mut(&mut after.files).insert("articles/x/content.md".into(), file("# x"));
    assert_round_trip(&mirror(vec![before]), &mirror(vec![after]));
}

struct Rng(u64);

impl Rng {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % bound.max(1)
    }
}

fn random_mirror(rng: &mut Rng) -> Mirror {
    let paths = ["/a", "/b", "/c"];
    let mut projects = Vec::new();
    for path in paths {
        if rng.next(4) == 0 {
            continue;
        }
        let mut held = project(path, &[]);
        for record in ["s1", "s2", "s3"] {
            if rng.next(3) == 0 {
                continue;
            }
            let items = (0..rng.next(6))
                .map(|i| {
                    if rng.next(2) == 0 {
                        user(&format!("u{}", rng.next(3) + i))
                    } else {
                        agent(&format!("a{}", rng.next(3)))
                    }
                })
                .collect();
            let mut view = session(items);
            if rng.next(2) == 0 {
                view.header = header(Status::Working);
            }
            held.sessions.insert(record.into(), view);
        }
        for name in ["one", "two"] {
            if rng.next(2) == 0 {
                std::sync::Arc::make_mut(&mut held.files).insert(
                    format!("boards/{name}.toml"),
                    file(&format!("v{}", rng.next(3))),
                );
            }
        }
        projects.push(held);
    }
    let len = projects.len();
    for i in 0..len {
        projects.swap(i, rng.next(len));
    }
    let mut held = mirror(projects);
    if rng.next(3) == 0 {
        held.agents.push("Codex".into());
    }
    if rng.next(2) == 0 {
        held.setup.switches.insert("mcp.serve".into(), rng.next(2) == 0);
    }
    held
}

#[test]
fn any_diff_applied_rebuilds_the_target() {
    let mut rng = Rng(42);
    for _ in 0..2000 {
        let before = random_mirror(&mut rng);
        let after = random_mirror(&mut rng);
        assert_round_trip(&before, &after);
    }
}

#[test]
fn a_new_agent_list_is_one_change() {
    let before = mirror(vec![]);
    let mut after = mirror(vec![]);
    after.agents.push("Codex".into());
    let changes = assert_round_trip(&before, &after);
    assert!(matches!(changes.as_slice(), [Change::Agents { .. }]));
}

#[test]
fn a_shell_snapshot_carries_headers_and_no_transcript() {
    let held = mirror(vec![project("/a", &[("s1", session(vec![user("hi"), agent("yo")]))])]);
    let shell = held.shell(1, 2);
    let view = &shell.projects[0].sessions["s1"];
    assert!(view.items.is_empty());
    assert_eq!(view.header, held.projects[0].sessions["s1"].header);
}
