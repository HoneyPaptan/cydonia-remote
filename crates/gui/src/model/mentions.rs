use remote::proto::Mentions;
use std::path::Path;

#[cfg(feature = "desktop")]
pub fn of(project: &Path) -> Mentions {
    scan::cached(project)
}

#[cfg(not(feature = "desktop"))]
pub fn of(project: &Path) -> Mentions {
    let query = remote::proto::Query::Mentions {
        project: project.to_string_lossy().into_owned(),
    };
    match super::relay::ask(query) {
        Ok(remote::proto::Answer::Mentions(mentions)) => mentions,
        _ => Mentions::default(),
    }
}

#[cfg(feature = "desktop")]
mod scan {
    use super::Mentions;
    use remote::proto::CommandView;
    use std::{
        collections::{HashMap, HashSet},
        path::{Path, PathBuf},
        process::Command,
        sync::{Mutex, OnceLock},
        time::{Duration, Instant},
    };

    const FRESH: Duration = Duration::from_secs(5);
    const FILE_LIMIT: usize = 5000;
    const WALK_DEPTH: usize = 6;
    const SKILL_FOLDERS: [&str; 6] = [
        ".claude/skills",
        ".agents/skills",
        ".codex/skills",
        ".gemini/skills",
        ".opencode/skills",
        ".config/opencode/skills",
    ];
    const SKIPPED: [&str; 5] = ["node_modules", "target", "dist", "build", "__pycache__"];
    const DESCRIPTION_LIMIT: usize = 140;

    static HELD: OnceLock<Mutex<HashMap<PathBuf, (Instant, Mentions)>>> = OnceLock::new();

    pub fn cached(project: &Path) -> Mentions {
        let held = HELD.get_or_init(Mutex::default);
        if let Some((at, mentions)) = held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(project)
            && at.elapsed() < FRESH
        {
            return mentions.clone();
        }
        let mentions = Mentions {
            files: project_files(project),
            skills: skills(project),
        };
        held.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(project.to_path_buf(), (Instant::now(), mentions.clone()));
        mentions
    }

    fn project_files(root: &Path) -> Vec<String> {
        tracked_files(root).unwrap_or_else(|| walked_files(root))
    }

    fn tracked_files(root: &Path) -> Option<Vec<String>> {
        let listed = Command::new("git")
            .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
            .current_dir(root)
            .output()
            .ok()
            .filter(|listed| listed.status.success())?;
        let mut files: Vec<String> = listed
            .stdout
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            .filter_map(|name| String::from_utf8(name.to_vec()).ok())
            .filter(|name| root.join(name).is_file())
            .collect();
        files.sort();
        files.dedup();
        files.truncate(FILE_LIMIT);
        Some(files)
    }

    fn walked_files(root: &Path) -> Vec<String> {
        let mut files = Vec::new();
        walk(root, root, 0, &mut files);
        files.sort();
        files
    }

    fn walk(root: &Path, folder: &Path, depth: usize, files: &mut Vec<String>) {
        if depth > WALK_DEPTH || files.len() >= FILE_LIMIT {
            return;
        }
        let Ok(entries) = std::fs::read_dir(folder) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || SKIPPED.contains(&name.as_str()) {
                continue;
            }
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => walk(root, &path, depth + 1, files),
                Ok(kind) if kind.is_file() => {
                    if let Ok(relative) = path.strip_prefix(root) {
                        files.push(relative.to_string_lossy().into_owned());
                    }
                }
                _ => {}
            }
        }
    }

    fn home() -> Option<PathBuf> {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
    }

    fn skill_roots(project: &Path) -> Vec<PathBuf> {
        [Some(project.to_path_buf()), home()]
            .into_iter()
            .flatten()
            .flat_map(|base| SKILL_FOLDERS.iter().map(move |folder| base.join(folder)))
            .collect()
    }

    fn skills(project: &Path) -> Vec<CommandView> {
        let mut seen = HashSet::new();
        let mut found: Vec<CommandView> = skill_roots(project)
            .iter()
            .filter_map(|root| std::fs::read_dir(root).ok())
            .flat_map(|entries| entries.flatten())
            .filter_map(|entry| skill(&entry.path()))
            .filter(|skill| seen.insert(skill.name.clone()))
            .collect();
        found.sort_by(|a, b| a.name.cmp(&b.name));
        found
    }

    fn skill(folder: &Path) -> Option<CommandView> {
        let text = std::fs::read_to_string(folder.join("SKILL.md")).ok()?;
        let front = text.strip_prefix("---")?.split("\n---").next()?;
        let name = value(front, "name")
            .or_else(|| folder.file_name().map(|name| name.to_string_lossy().into_owned()))?;
        let description = value(front, "description").unwrap_or_default();
        Some(CommandView {
            name,
            description: description.chars().take(DESCRIPTION_LIMIT).collect(),
        })
    }

    fn value(front: &str, key: &str) -> Option<String> {
        let mut lines = front.lines();
        let line = lines.find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))?;
        let inline = line.trim();
        let text = match inline {
            "" | ">" | "|" | ">-" | "|-" => lines
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or_default(),
            inline => inline,
        };
        let text = text.trim_matches(|c| c == '"' || c == '\'').trim();
        (!text.is_empty()).then(|| text.to_owned())
    }
}
