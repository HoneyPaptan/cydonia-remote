use crate::{
    model::{git, servers},
    view::component::terminal::Shell,
};
use futures::{StreamExt as _, channel::mpsc};
use portable_pty::PtySize;
use remote::{
    hub::Hub,
    proto::{
        Answer, BOLD, Color, DIM, DirEntry, File, Folders, ITALIC, Output, Query, Run, Screen,
        ShellInput, UNDERLINE,
    },
    server::{Local, ShellLink},
};
use std::{
    ffi::OsString,
    io::Read as _,
    net::IpAddr,
    path::{Component, Path, PathBuf},
    sync::{Arc, mpsc as channel},
    time::Duration,
};
use terminal::emulator::{CellColor, CellSnapshot, Emulator};

const FILE_LIMIT: u64 = 256 * 1024;
const STATUS: [&str; 6] = [
    "--porcelain=v1",
    "--renames",
    "-z",
    "--untracked-files=all",
    "--ignore-submodules=none",
    "status",
];
const DIFF: [&str; 7] = [
    "diff",
    "--no-ext-diff",
    "--no-textconv",
    "--no-color",
    "--submodule=short",
    "--cached",
    "--no-index",
];
const NULL: &str = "/dev/null";
const SETTLE: Duration = Duration::from_millis(12);

pub struct Laptop {
    hub: Arc<Hub>,
    hosts: Vec<IpAddr>,
}

impl Laptop {
    pub fn new(hub: Arc<Hub>, hosts: Vec<IpAddr>) -> Arc<Self> {
        Arc::new(Self { hub, hosts })
    }

    fn roots(&self) -> Vec<PathBuf> {
        self.hub
            .mirror()
            .projects
            .iter()
            .filter_map(|project| std::fs::canonicalize(&project.path).ok())
            .collect()
    }

    fn inside(&self, path: &Path) -> Option<PathBuf> {
        let real = std::fs::canonicalize(path).ok().or_else(|| {
            let parent = std::fs::canonicalize(path.parent()?).ok()?;
            Some(parent.join(path.file_name()?))
        })?;
        self.roots()
            .iter()
            .any(|root| real.starts_with(root))
            .then_some(real)
    }
}

fn failed(message: impl ToString) -> Answer {
    Answer::Failed {
        message: message.to_string(),
    }
}

fn read_dir(path: &Path) -> Answer {
    let Ok(listing) = std::fs::read_dir(path) else {
        return failed("Could not list this folder");
    };
    let entries = listing
        .filter_map(Result::ok)
        .map(|entry| DirEntry {
            directory: entry.file_type().is_ok_and(|kind| kind.is_dir()),
            path: path.join(entry.file_name()).to_string_lossy().into_owned(),
        })
        .collect();
    Answer::Dir { entries }
}

fn absolute(path: &str) -> Option<PathBuf> {
    let path = match path {
        "" => dirs::home_dir()?,
        typed => PathBuf::from(typed),
    };
    let plain = path
        .components()
        .all(|part| !matches!(part, Component::ParentDir | Component::CurDir));
    (path.is_absolute() && plain).then_some(path)
}

fn shown(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn folders(path: &Path) -> Answer {
    let Ok(real) = std::fs::canonicalize(path) else {
        return failed("That folder is not there any more");
    };
    let Ok(listing) = std::fs::read_dir(&real) else {
        return failed("Could not list this folder");
    };
    let mut names: Vec<String> = listing
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    Answer::Folders(Folders {
        path: shown(&real),
        parent: real.parent().map(shown),
        folders: names,
    })
}

fn make_folder(path: &Path) -> Answer {
    if path.file_name().is_none() {
        return failed("Name the new folder");
    }
    match std::fs::create_dir(path) {
        Ok(()) => folders(path),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            failed("Something with that name is already here")
        }
        Err(error) => failed(error),
    }
}

fn browse(query: &Query) -> Option<Answer> {
    let (path, make) = match query {
        Query::Folders { path } => (path, false),
        Query::MakeFolder { path } => (path, true),
        _ => return None,
    };
    let Some(path) = absolute(path) else {
        return Some(failed("Not a full path"));
    };
    Some(match make {
        true => make_folder(&path),
        false => folders(&path),
    })
}

fn read_file(path: &Path) -> Answer {
    if !std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
        return failed("Choose a regular file");
    }
    let mut bytes = Vec::new();
    match std::fs::File::open(path).and_then(|file| file.take(FILE_LIMIT + 1).read_to_end(&mut bytes)) {
        Ok(_) => Answer::File { file: File(bytes) },
        Err(error) => failed(error),
    }
}

fn write_file(path: &Path, text: &str) -> Answer {
    let temporary = path.with_file_name(format!(".cydonia-remote-save-{}", std::process::id()));
    let written = std::fs::write(&temporary, text).and_then(|()| {
        if let Ok(metadata) = std::fs::metadata(path) {
            std::fs::set_permissions(&temporary, metadata.permissions())?;
        }
        std::fs::rename(&temporary, path)
    });
    match written {
        Ok(()) => Answer::Written,
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            failed(error)
        }
    }
}

fn inside_path(arg: &str) -> bool {
    let path = Path::new(arg);
    !arg.is_empty()
        && !arg.starts_with('-')
        && path.is_relative()
        && path
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_) | std::path::Component::CurDir))
}

fn diff_allowed(args: &[String]) -> bool {
    let Some(split) = args.iter().position(|arg| arg == "--") else {
        return false;
    };
    let (flags, paths) = (&args[1..split], &args[split + 1..]);
    let no_index = flags.iter().any(|flag| flag == "--no-index");
    let paths_allowed = match no_index {
        true => paths.len() == 2 && paths[0] == NULL && inside_path(&paths[1]),
        false => (1..=2).contains(&paths.len()) && paths.iter().all(|path| inside_path(path)),
    };
    flags.iter().all(|flag| DIFF[1..].contains(&flag.as_str())) && paths_allowed
}

fn allowed(args: &[String]) -> bool {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["rev-parse", "--show-toplevel"] => true,
        ["status", flags @ ..] => flags.iter().all(|flag| STATUS[..5].contains(flag)),
        ["diff", ..] => diff_allowed(args),
        ["cat-file", "blob", spec] => !spec.starts_with('-') && !spec.contains(".."),
        _ => false,
    }
}

fn run_git(cwd: &Path, args: &[String]) -> Answer {
    if !allowed(args) {
        return failed("That Git command is not relayed");
    }
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    match git::run(cwd, &args) {
        Ok(ran) => Answer::Output(Output {
            success: ran.success,
            code: ran.code,
            stdout: File(ran.stdout),
            stderr: ran.stderr,
            truncated: ran.truncated,
        }),
        Err(error) => failed(error),
    }
}

impl Local for Laptop {
    fn answer(&self, query: Query) -> Answer {
        if let Some(answer) = browse(&query) {
            return answer;
        }
        let path = match &query {
            Query::Agents => {
                return Answer::Agents {
                    agents: crate::agent::catalogue(),
                };
            }
            Query::Servers => {
                return Answer::Servers {
                    servers: servers::fresh(),
                };
            }
            Query::Expose { port } => {
                return match servers::expose(*port, &self.hosts) {
                    Ok(()) => Answer::Exposed,
                    Err(message) => failed(message),
                };
            }
            Query::Stop { pid, port } => {
                return match servers::stop_now(*pid, *port) {
                    Ok(()) => Answer::Stopped,
                    Err(message) => failed(message),
                };
            }
            Query::Mentions { project } => {
                return match self.inside(Path::new(project)) {
                    Some(project) => Answer::Mentions(crate::model::mentions::of(&project)),
                    None => failed("Outside every open project"),
                };
            }
            Query::ReadDir { path } | Query::ReadFile { path } | Query::WriteFile { path, .. } => {
                path
            }
            Query::Git { cwd, .. } => cwd,
            Query::Folders { .. } | Query::MakeFolder { .. } => return failed("Not a project path"),
        };
        let Some(path) = self.inside(Path::new(path)) else {
            return failed("Outside every open project");
        };
        match &query {
            Query::ReadDir { .. } => read_dir(&path),
            Query::ReadFile { .. } => read_file(&path),
            Query::WriteFile { text, .. } => write_file(&path, text),
            Query::Git { args, .. } => run_git(&path, args),
            Query::Agents
            | Query::Mentions { .. }
            | Query::Folders { .. }
            | Query::MakeFolder { .. }
            | Query::Servers
            | Query::Expose { .. }
            | Query::Stop { .. } => failed("Not a path"),
        }
    }

    fn shell(&self, cwd: &str, cols: u16, rows: u16) -> Option<ShellLink> {
        let cwd = self.inside(Path::new(cwd))?;
        let (shell, output) = Shell::open(&cwd).ok()?;
        let (events, inbox) = channel::channel();
        let (screens, receiver) = mpsc::unbounded();
        let forward = events.clone();
        std::thread::spawn(move || {
            let mut output = output;
            while let Some(bytes) = futures::executor::block_on(output.next()) {
                if forward.send(Event::Output(bytes)).is_err() {
                    return;
                }
            }
            let _ = forward.send(Event::Ended);
        });
        std::thread::spawn(move || host(shell, inbox, screens, cols, rows));
        let events = std::sync::Mutex::new(events);
        Some(ShellLink {
            input: Box::new(move |input| {
                if let Ok(events) = events.lock() {
                    let _ = events.send(Event::Input(input));
                }
            }),
            screens: receiver,
        })
    }
}

enum Event {
    Input(ShellInput),
    Output(Vec<u8>),
    Ended,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn host(
    shell: Shell,
    inbox: channel::Receiver<Event>,
    screens: mpsc::UnboundedSender<Screen>,
    cols: u16,
    rows: u16,
) {
    let mut emulator = Emulator::new(cols, rows);
    let _ = shell.master.resize(size(cols, rows));
    while let Ok(first) = inbox.recv() {
        let mut next = Some(first);
        while let Some(event) = next.take() {
            match event {
                Event::Input(ShellInput::Keys { text }) => {
                    emulator.scroll_to_bottom();
                    let _ = shell.input.send(text.into_bytes());
                }
                Event::Input(ShellInput::Resize { cols, rows }) => {
                    let (cols, rows) = (cols.clamp(8, 500), rows.clamp(2, 300));
                    emulator.resize(cols, rows);
                    let _ = shell.master.resize(size(cols, rows));
                }
                Event::Input(ShellInput::Scroll { lines }) => emulator.scroll(lines),
                Event::Input(ShellInput::Close) => return,
                Event::Output(bytes) => {
                    let reply = emulator.feed(&bytes);
                    if !reply.is_empty() {
                        let _ = shell.input.send(reply);
                    }
                }
                Event::Ended => {
                    let _ = screens.unbounded_send(screen(&emulator));
                    return;
                }
            }
            next = inbox.recv_timeout(SETTLE).ok();
        }
        if screens.unbounded_send(screen(&emulator)).is_err() {
            return;
        }
    }
}

fn color(color: CellColor) -> Color {
    match color {
        CellColor::Foreground | CellColor::Background => Color::Default,
        CellColor::Indexed(index) if index < 16 => Color::Indexed(index),
        CellColor::Indexed(index) => {
            let (r, g, b) = terminal::view::indexed_rgb(bezel::theme::Appearance::Dark, index);
            Color::Rgb(r, g, b)
        }
        CellColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn style(cell: &CellSnapshot) -> u8 {
    [
        (cell.bold, BOLD),
        (cell.dim, DIM),
        (cell.italic, ITALIC),
        (cell.underline, UNDERLINE),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .fold(0, |style, (_, bit)| style | bit)
}

fn runs(line: &[CellSnapshot]) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for cell in line.iter().filter(|cell| !cell.wide_spacer) {
        let (fg, bg) = cell.display_colors();
        let (fg, bg, style) = (color(fg), color(bg), style(cell));
        let ch = if cell.hidden { ' ' } else { cell.ch };
        match runs.last_mut() {
            Some(run) if run.fg == fg && run.bg == bg && run.style == style => run.text.push(ch),
            _ => runs.push(Run {
                text: ch.to_string(),
                fg,
                bg,
                style,
            }),
        }
    }
    if let Some(run) = runs.last_mut()
        && run.bg == Color::Default
    {
        let kept = run.text.trim_end_matches(' ').len();
        run.text.truncate(kept);
    }
    runs.retain(|run| !run.text.is_empty());
    runs
}

fn screen(emulator: &Emulator) -> Screen {
    Screen {
        rows: emulator.lines().iter().map(|line| runs(line)).collect(),
        cursor: emulator
            .cursor()
            .map(|cursor| (cursor.row as u16, cursor.col as u16)),
        title: emulator.title().map(str::to_owned),
        directory: emulator
            .directory()
            .map(|directory| directory.to_string_lossy().into_owned()),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/remote_local.rs"]
mod tests;
