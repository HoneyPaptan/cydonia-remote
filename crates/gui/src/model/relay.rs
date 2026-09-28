use futures::channel::mpsc::UnboundedReceiver;
use remote::proto::{Answer, Query, Screen, ShellInput};
use std::{
    cell::RefCell,
    rc::Rc,
    collections::HashMap,
    fmt,
    sync::{
        Mutex, MutexGuard, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use web_time::Instant;

const FRESH: Duration = Duration::from_millis(1500);
const GLANCE: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pending;

impl fmt::Display for Pending {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Waiting for the laptop")
    }
}

impl std::error::Error for Pending {}

pub fn is_pending(error: &anyhow::Error) -> bool {
    error.is::<Pending>()
        || error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
}

struct Held {
    answer: Answer,
    at: Instant,
}

#[derive(Default)]
struct Cache {
    held: HashMap<Query, Held>,
    asking: HashMap<Query, Instant>,
}

static FETCH: OnceLock<fn(Query)> = OnceLock::new();
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
static CHANGES: AtomicU64 = AtomicU64::new(0);
static LAPTOP: OnceLock<String> = OnceLock::new();

fn cache() -> MutexGuard<'static, Cache> {
    CACHE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn install(fetch: fn(Query), laptop: &str) {
    let _ = FETCH.set(fetch);
    let _ = LAPTOP.set(laptop.to_owned());
}

pub fn installed() -> bool {
    FETCH.get().is_some()
}

pub fn laptop() -> Option<&'static str> {
    LAPTOP.get().map(String::as_str)
}

pub fn ask(query: Query) -> Result<Answer, Pending> {
    let (held, stale) = {
        let cache = cache();
        match cache.held.get(&query) {
            Some(held) => (Some(held.answer.clone()), held.at.elapsed() > FRESH),
            None => (None, true),
        }
    };
    if stale {
        request(query);
    }
    held.ok_or(Pending)
}

pub fn tell(query: Query) {
    if let Some(fetch) = FETCH.get() {
        fetch(query);
    }
}

pub fn assume(query: Query, answer: Answer) {
    let mut cache = cache();
    cache.held.insert(
        query,
        Held {
            answer,
            at: Instant::now(),
        },
    );
    CHANGES.fetch_add(1, Ordering::Relaxed);
}

fn request(query: Query) {
    let Some(fetch) = FETCH.get() else {
        return;
    };
    let fresh = {
        let mut cache = cache();
        match cache.asking.contains_key(&query) {
            true => false,
            false => {
                cache.asking.insert(query.clone(), Instant::now());
                true
            }
        }
    };
    if fresh {
        fetch(query);
    }
}

fn keep(query: Query, answer: Answer) {
    let mut cache = cache();
    let asked = cache.asking.remove(&query);
    if let (Some(asked), Some(held)) = (asked, cache.held.get(&query))
        && held.at > asked
    {
        return;
    }
    let changed = cache
        .held
        .get(&query)
        .is_none_or(|held| held.answer != answer);
    cache.held.insert(
        query,
        Held {
            answer,
            at: Instant::now(),
        },
    );
    if changed {
        CHANGES.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn answered(query: Query, answer: Answer) {
    match (&query, &answer) {
        (Query::WriteFile { .. }, _) => return,
        (Query::MakeFolder { .. }, Answer::Folders(made)) => keep(
            Query::Folders {
                path: made.path.clone(),
            },
            answer.clone(),
        ),
        _ => {}
    }
    keep(query, answer);
}

pub fn take(query: &Query) -> Option<Answer> {
    cache().held.remove(query).map(|held| held.answer)
}

pub fn unanswered(query: &Query) {
    cache().asking.remove(query);
}

pub async fn pause(executor: &bezel::gpui::BackgroundExecutor, most: Duration) {
    if !installed() {
        executor.timer(most).await;
        return;
    }
    let seen = CHANGES.load(Ordering::Relaxed);
    let start = Instant::now();
    while start.elapsed() < most {
        executor.timer(GLANCE).await;
        if CHANGES.load(Ordering::Relaxed) != seen {
            return;
        }
    }
}

pub struct Opened {
    pub input: Rc<dyn Fn(ShellInput)>,
    pub screens: UnboundedReceiver<Screen>,
}

pub trait Shells {
    fn open(&self, cwd: &str, cols: u16, rows: u16) -> Option<Opened>;
}

thread_local! {
    static SHELLS: RefCell<Option<Rc<dyn Shells>>> = const { RefCell::new(None) };
}

pub fn install_shells(shells: Rc<dyn Shells>) {
    SHELLS.with(|held| *held.borrow_mut() = Some(shells));
}

pub fn shells() -> Option<Rc<dyn Shells>> {
    SHELLS.with(|held| held.borrow().clone())
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PageOp {
    Place {
        id: u64,
        url: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    Park {
        id: u64,
    },
    Load {
        id: u64,
        url: String,
    },
    Back {
        id: u64,
    },
    Forward {
        id: u64,
    },
    Reload {
        id: u64,
    },
    Close {
        id: u64,
    },
}

static PAGES: OnceLock<fn(PageOp)> = OnceLock::new();

pub fn install_pages(pages: fn(PageOp)) {
    let _ = PAGES.set(pages);
}

pub fn pages_installed() -> bool {
    PAGES.get().is_some()
}

pub fn page(op: PageOp) {
    if let Some(pages) = PAGES.get() {
        pages(op);
    }
}

#[cfg(test)]
#[path = "../../tests/unit/relay.rs"]
mod tests;
