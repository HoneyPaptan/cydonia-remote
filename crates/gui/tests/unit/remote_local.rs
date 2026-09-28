use super::*;
use remote::mirror::Mirror;
use remote::proto::ProjectView;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("cydonia-local-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("project/src")).unwrap();
        std::fs::create_dir_all(root.join("elsewhere")).unwrap();
        std::fs::write(root.join("project/src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("elsewhere/secret"), "no\n").unwrap();
        Self(root)
    }

    fn laptop(&self) -> Arc<Laptop> {
        let hub = Hub::new(1);
        hub.publish(Mirror {
            agents: Vec::new(),
            projects: vec![ProjectView::new(self.0.join("project").to_string_lossy())],
        });
        Laptop::new(hub)
    }

    fn path(&self, rest: &str) -> String {
        self.0.join(rest).to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn files_inside_a_project_are_listed_read_and_written() {
    let scratch = Scratch::new("inside");
    let laptop = scratch.laptop();

    let Answer::Dir { entries } = laptop.answer(Query::ReadDir {
        path: scratch.path("project"),
    }) else {
        panic!("a listing");
    };
    assert_eq!(entries.len(), 1);
    assert!(entries[0].directory);

    let written = laptop.answer(Query::WriteFile {
        path: scratch.path("project/src/main.rs"),
        text: "fn main() { run() }\n".into(),
    });
    assert_eq!(written, Answer::Written);
    assert_eq!(
        laptop.answer(Query::ReadFile {
            path: scratch.path("project/src/main.rs"),
        }),
        Answer::File {
            file: File(b"fn main() { run() }\n".to_vec()),
        }
    );
}

#[test]
fn nothing_outside_an_open_project_is_reached() {
    let scratch = Scratch::new("outside");
    let laptop = scratch.laptop();

    for path in [
        scratch.path("elsewhere/secret"),
        scratch.path("project/../elsewhere/secret"),
    ] {
        assert!(matches!(
            laptop.answer(Query::ReadFile { path }),
            Answer::Failed { .. }
        ));
    }
    assert!(laptop.shell(&scratch.path("elsewhere"), 80, 24).is_none());
}

fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

#[test]
fn the_git_calls_cydonia_makes_are_relayed() {
    for args in [
        &["rev-parse", "--show-toplevel"][..],
        &["status", "--porcelain=v1", "--renames", "-z", "--untracked-files=all", "--ignore-submodules=none"],
        &["diff", "--no-ext-diff", "--no-textconv", "--no-color", "--submodule=short", "--", "src/main.rs"],
        &["diff", "--no-ext-diff", "--cached", "--", "new.rs", "old.rs"],
        &["diff", "--no-ext-diff", "--no-index", "--", "/dev/null", "src/new.rs"],
        &["cat-file", "blob", "HEAD:src/main.rs"],
    ] {
        assert!(allowed(&words(args)), "{args:?}");
    }
}

#[test]
fn git_cannot_reach_outside_the_project_or_write() {
    for args in [
        &["diff", "--no-index", "--", "/dev/null", "/etc/passwd"][..],
        &["diff", "--no-index", "--", "/dev/null", "../../etc/passwd"],
        &["diff", "--", "/etc/passwd"],
        &["diff", "--output=/tmp/x", "--", "a"],
        &["diff", "--ext-diff", "--", "a"],
        &["diff", "--no-index", "/dev/null", "/etc/passwd"],
        &["status", "--porcelain=v1", "--exec-path=/tmp"],
        &["cat-file", "blob", "--batch"],
        &["rev-parse", "--git-dir"],
        &["push"],
        &[],
    ] {
        assert!(!allowed(&words(args)), "{args:?}");
    }
}

#[test]
fn a_screen_keeps_colours_and_drops_trailing_blanks() {
    let mut emulator = Emulator::new(20, 2);
    emulator.feed(b"\x1b[31mred\x1b[0m plain");
    let screen = screen(&emulator);
    assert_eq!(screen.rows[0][0].text, "red");
    assert_eq!(screen.rows[0][0].fg, Color::Indexed(1));
    assert_eq!(screen.rows[0][1].text, " plain");
    assert!(screen.rows[1].is_empty());
    assert_eq!(screen.cursor, Some((0, 9)));
}
