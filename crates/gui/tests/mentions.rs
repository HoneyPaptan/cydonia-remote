use cydonia_gui::model::mentions;
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cydonia-mentions-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn a_project_skill_is_offered_with_its_description() {
    let project = scratch("skill");
    write(
        &project.join(".claude/skills/tidy/SKILL.md"),
        "---\nname: tidy\ndescription: Sweep the workshop\n---\nbody",
    );
    write(
        &project.join(".agents/skills/folded/SKILL.md"),
        "---\nname: folded\ndescription: >\n  Reads a folded line\n  and a second\n---\nbody",
    );
    let found = mentions::of(&project).skills;
    let tidy = found.iter().find(|skill| skill.name == "tidy").unwrap();
    assert_eq!(tidy.description, "Sweep the workshop");
    let folded = found.iter().find(|skill| skill.name == "folded").unwrap();
    assert_eq!(folded.description, "Reads a folded line");
}

#[test]
fn files_skip_hidden_and_generated_folders() {
    let project = scratch("files");
    write(&project.join("src/main.rs"), "");
    write(&project.join("README.md"), "");
    write(&project.join("node_modules/dep/index.js"), "");
    write(&project.join(".hidden/secret"), "");
    let files = mentions::of(&project).files;
    assert_eq!(files, vec!["README.md".to_owned(), "src/main.rs".to_owned()]);
}
