//! Read-only Git queries with literal, non-UTF-8 path support.

use anyhow::{Context as _, Result, bail};
#[cfg(feature = "desktop")]
use std::{
    io::Read as _,
    process::{Command, Stdio},
};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub mod preview;

const PATCH_LIMIT: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    Staged,
    Unstaged,
    Untracked,
}

impl Area {
    pub fn label(self) -> &'static str {
        match self {
            Self::Staged => "Staged",
            Self::Unstaged => "Unstaged",
            Self::Untracked => "Untracked",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub path: PathBuf,
    pub original: Option<PathBuf>,
    pub area: Area,
    pub status: char,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repository {
    pub root: PathBuf,
    pub files: Vec<Change>,
}

#[cfg(feature = "desktop")]
fn command(root: &Path) -> Command {
    let mut git = Command::new("git");
    git.arg("--literal-pathspecs")
        .args(["-c", "core.fsmonitor=false"])
        .arg("-C")
        .arg(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    git
}

fn os_path(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt as _;
        OsString::from_vec(bytes.to_vec()).into()
    }
    #[cfg(not(unix))]
    {
        OsString::from(String::from_utf8_lossy(bytes).into_owned()).into()
    }
}

pub fn status(cwd: &Path) -> Result<Option<Repository>> {
    let found = run(cwd, &words(["rev-parse", "--show-toplevel"])).context("Could not run Git")?;
    if !found.success {
        if found.stderr.contains("not a git repository") {
            return Ok(None);
        }
        bail!("{}", found.stderr.trim());
    }
    // Remove Git's terminator only: whitespace can be part of the root path.
    let root = os_path(found.stdout.strip_suffix(b"\n").unwrap_or(&found.stdout));
    let listed = run(
        &root,
        &words([
            "status",
            "--porcelain=v1",
            "--renames",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ]),
    )?;
    if !listed.success {
        bail!("{}", listed.stderr.trim());
    }
    Ok(Some(Repository {
        root,
        files: parse_status(&listed.stdout)?,
    }))
}

fn parse_status(bytes: &[u8]) -> Result<Vec<Change>> {
    let mut entries = bytes
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty());
    let mut files = Vec::new();
    while let Some(entry) = entries.next() {
        if entry.len() < 4 || entry[2] != b' ' {
            bail!("Invalid Git status record");
        }
        let path = os_path(&entry[3..]);
        let original = if entry[..2].iter().any(|code| matches!(code, b'R' | b'C')) {
            Some(os_path(
                entries.next().context("Missing Git rename source")?,
            ))
        } else {
            None
        };
        if &entry[..2] == b"??" {
            files.push(Change {
                path,
                original,
                area: Area::Untracked,
                status: '?',
            });
            continue;
        }
        // Show conflicts once, in the working-tree section.
        if entry[..2].contains(&b'U') || matches!(&entry[..2], b"AA" | b"DD") {
            files.push(Change {
                path,
                original,
                area: Area::Unstaged,
                status: 'U',
            });
            continue;
        }
        for (code, area) in [(entry[0], Area::Staged), (entry[1], Area::Unstaged)] {
            if code != b' ' && code != b'!' {
                files.push(Change {
                    path: path.clone(),
                    original: original.clone(),
                    area,
                    status: code as char,
                });
            }
        }
    }
    files.sort_by_key(|file| {
        (
            match file.area {
                Area::Staged => 0,
                Area::Unstaged => 1,
                Area::Untracked => 2,
            },
            file.path.clone(),
        )
    });
    Ok(files)
}

pub fn diff(root: &Path, change: &Change) -> Result<String> {
    let mut args = words([
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "--submodule=short",
    ]);
    if change.area == Area::Untracked {
        // Let Git handle untracked file metadata; exit 1 means a diff.
        args.extend(words([
            "--no-index",
            "--",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        ]));
        args.push(change.path.clone().into_os_string());
    } else {
        if change.area == Area::Staged {
            args.push("--cached".into());
        }
        args.push("--".into());
        args.push(change.path.clone().into_os_string());
        if let Some(original) = &change.original {
            args.push(original.clone().into_os_string());
        }
    }
    // Bound output before collecting large generated patches.
    let ran = run(root, &args)?;
    let (truncated, mut bytes) = (ran.truncated, ran.stdout);
    let differs = change.area == Area::Untracked && ran.code == Some(1);
    if !truncated && !ran.success && !differs {
        bail!("Git could not read this diff. Refresh to try again.");
    }
    bytes.truncate(PATCH_LIMIT as usize);
    let mut patch = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        patch.push_str("\nPreview truncated at 1 MiB.\n");
    }
    if patch.is_empty() {
        patch.push_str("No textual diff (the file may have changed since refresh).");
    }
    Ok(patch)
}

/// Bound patch and source reads, reporting truncation to callers.
pub struct Ran {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: String,
    pub truncated: bool,
}

fn words<const N: usize>(words: [&str; N]) -> Vec<OsString> {
    words.into_iter().map(OsString::from).collect()
}

#[cfg(feature = "desktop")]
pub fn run(cwd: &Path, args: &[OsString]) -> Result<Ran> {
    let mut git = command(cwd);
    git.args(args);
    let mut child = git.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let mut stdout = Vec::new();
    let read = child
        .stdout
        .take()
        .context("Missing Git output")?
        .take(PATCH_LIMIT + 1)
        .read_to_end(&mut stdout);
    let truncated = stdout.len() as u64 > PATCH_LIMIT;
    if truncated || read.is_err() {
        let _ = child.kill();
    }
    let mut stderr = String::new();
    if let Some(mut errors) = child.stderr.take() {
        let _ = errors.read_to_string(&mut stderr);
    }
    let exit = child.wait()?;
    read?;
    Ok(Ran {
        success: exit.success(),
        code: exit.code(),
        stdout,
        stderr,
        truncated,
    })
}

#[cfg(not(feature = "desktop"))]
pub fn run(cwd: &Path, args: &[OsString]) -> Result<Ran> {
    use remote::proto::{Answer, Query};
    let query = Query::Git {
        cwd: cwd.to_string_lossy().into_owned(),
        args: args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
    };
    match super::relay::ask(query)? {
        Answer::Output(output) => Ok(Ran {
            success: output.success,
            code: output.code,
            stdout: output.stdout.0,
            stderr: output.stderr,
            truncated: output.truncated,
        }),
        Answer::Failed { message } => bail!("{message}"),
        _ => bail!("The laptop answered something else"),
    }
}

#[cfg(all(test, unix))]
#[path = "../../tests/unit/git_status.rs"]
mod tests;
