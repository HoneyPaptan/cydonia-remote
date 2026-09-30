use crate::{
    apply, fonts, keeper,
    net::{self, Endpoint, Inbound},
    seed,
};
use bezel::gpui::{App, Application, ApplicationHandle, AsyncApp, Entity, WindowHandle};
use futures::{
    FutureExt as _, StreamExt as _,
    channel::mpsc::{self, UnboundedReceiver, UnboundedSender},
    select,
};
use gui::{
    boot,
    model::{
        sink::{SessionChange, Sink, Write},
        workspace::{NoBackdropBlur, Workspace},
    },
    view::root::{self, Cydonia},
};
use remote::{
    mirror::Mirror,
    proto::{
        Action, Change, Command, Event, File, Frame, Outcome, SessionKey, SessionView, Upload,
        VERSION,
    },
};
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    path::Path,
    rc::Rc,
    sync::Arc,
};
use wasm_bindgen::prelude::wasm_bindgen;

const FONTS: [&[u8]; 5] = [
    include_bytes!("../../demo/assets/fonts/IBMPlexSans-Regular.ttf"),
    include_bytes!("../../demo/assets/fonts/IBMPlexSans-SemiBold.ttf"),
    include_bytes!("../../demo/assets/fonts/IBMPlexSans-Italic.ttf"),
    include_bytes!("../../demo/assets/fonts/Lilex-Regular.ttf"),
    include_bytes!("../../demo/assets/fonts/Lilex-Bold.ttf"),
];

const FIRST_WAIT: i32 = 500;
const LAST_WAIT: i32 = 10_000;
const HEARTBEAT_CHECK: i32 = 5_000;
const SILENCE_LIMIT: f64 = 25_000.0;
const REMEMBER_EVERY: f64 = 30_000.0;

thread_local! {
    static APPLICATION: RefCell<Option<ApplicationHandle>> = const { RefCell::new(None) };
    static WINDOW: RefCell<Option<WindowHandle<Cydonia>>> = const { RefCell::new(None) };
}

fn element(id: &str) -> Option<web_sys::Element> {
    web_sys::window()?.document()?.get_element_by_id(id)
}

fn show(text: &str) {
    if let Some(failure) = element("failure") {
        failure.set_text_content(Some(&format!("Cydonia did not start: {text}")));
        let _ = failure.remove_attribute("hidden");
    }
    if let Some(boot) = element("boot") {
        boot.remove();
    }
}

struct Commands {
    outbox: UnboundedSender<Command>,
}

struct Opening {
    wanted: RefCell<Option<SessionKey>>,
    workspace: Entity<Workspace>,
}

impl Opening {
    fn want(&self, key: SessionKey) {
        *self.wanted.borrow_mut() = Some(key);
    }

    fn settle(&self, cx: &mut AsyncApp) {
        let Some(key) = self.wanted.borrow().clone() else {
            return;
        };
        let opened = self.workspace.update(cx, |workspace, cx| {
            let Some(id) = apply::chat_mut(workspace, &key).map(|chat| chat.id) else {
                return false;
            };
            workspace.select_session(id, cx);
            true
        });
        if opened {
            self.wanted.borrow_mut().take();
        }
    }
}

fn key(project: &Path, record: &str) -> SessionKey {
    SessionKey {
        project: project.to_string_lossy().into_owned(),
        record: record.to_owned(),
    }
}

impl Commands {
    fn deliver(&self, action: Action) {
        let _ = self.outbox.unbounded_send(Command {
            id: net::fresh_id(),
            action,
        });
    }
}

async fn settle(endpoint: &Endpoint, command: &Command, opening: &Opening, cx: &mut AsyncApp) {
    let mut wait = FIRST_WAIT;
    loop {
        match endpoint.command(command).await {
            Ok(ack) => {
                if let Outcome::Created { key } = ack.outcome {
                    opening.want(key);
                    opening.settle(cx);
                }
                return;
            }
            Err(failure) if failure.retryable() => {
                net::sleep(wait).await;
                wait = (wait * 2).min(LAST_WAIT);
            }
            Err(_) => return,
        }
    }
}

async fn send_in_order(
    endpoint: Rc<Endpoint>,
    opening: Rc<Opening>,
    mut cx: AsyncApp,
    mut outbox: UnboundedReceiver<Command>,
) {
    while let Some(command) = outbox.next().await {
        settle(&endpoint, &command, &opening, &mut cx).await;
    }
}

impl Sink for Commands {
    fn send_prompt(&self, project: &Path, record: &str, text: String) {
        self.deliver(Action::SendPrompt {
            key: key(project, record),
            text,
        });
    }

    fn send_attached(&self, project: &Path, record: &str, text: String, files: Vec<(String, Vec<u8>)>) {
        self.deliver(Action::SendAttached {
            key: key(project, record),
            text,
            files: files
                .into_iter()
                .map(|(name, bytes)| Upload { name, file: File(bytes) })
                .collect(),
        });
    }

    fn cancel(&self, project: &Path, record: &str) {
        self.deliver(Action::Cancel {
            key: key(project, record),
        });
    }

    fn respond_permission(&self, project: &Path, record: &str, request: u64, option: String) {
        self.deliver(Action::RespondPermission {
            key: key(project, record),
            request,
            option,
        });
    }

    fn set_mode(&self, project: &Path, record: &str, mode: String) {
        self.deliver(Action::SetMode {
            key: key(project, record),
            mode,
        });
    }

    fn set_config(&self, project: &Path, record: &str, config: String, value: String) {
        self.deliver(Action::SetConfig {
            key: key(project, record),
            config,
            value,
        });
    }

    fn write(&self, project: &Path, write: Write) {
        if let Some(action) = crate::write::action(project.to_string_lossy().into_owned(), write) {
            self.deliver(action);
        }
    }

    fn agent(&self, id: &str, install: bool) {
        let id = id.to_owned();
        self.deliver(match install {
            true => Action::InstallAgent { id },
            false => Action::RemoveAgent { id },
        });
    }

    fn project(&self, path: &Path, open: bool) {
        let path = path.to_string_lossy().into_owned();
        self.deliver(match open {
            true => Action::OpenProject { path },
            false => Action::CloseProject { path },
        });
    }

    fn switch(&self, key: &str, on: bool) {
        self.deliver(Action::SetSwitch {
            key: key.to_owned(),
            on,
        });
    }

    fn session(&self, project: &Path, record: &str, change: SessionChange) {
        let key = key(project, record);
        self.deliver(match change {
            SessionChange::Rename(name) => Action::RenameSession { key, name },
            SessionChange::Archive(archived) => Action::ArchiveSession { key, archived },
            SessionChange::Remove => Action::RemoveSession { key },
        });
    }

    fn new_session(&self, project: &Path, agent: &str, text: Option<String>) {
        self.deliver(Action::NewSession {
            project: project.to_string_lossy().into_owned(),
            agent: Some(agent.to_owned()),
            text,
        });
    }
}

struct Focus {
    keys: RefCell<Vec<SessionKey>>,
    wake: UnboundedSender<()>,
}

impl Focus {
    fn follow(&self, keys: Vec<SessionKey>) {
        if *self.keys.borrow() == keys {
            return;
        }
        *self.keys.borrow_mut() = keys;
        let _ = self.wake.unbounded_send(());
    }

    fn current(&self) -> Vec<SessionKey> {
        self.keys.borrow().clone()
    }
}

fn watched(workspace: &Workspace) -> Vec<SessionKey> {
    workspace
        .active_session()
        .and_then(|chat| Some(key(&chat.cwd, chat.record.as_deref()?)))
        .into_iter()
        .collect()
}

enum Order {
    Old,
    Next,
    Gap,
}

enum Woke {
    Heard(Option<Inbound>),
    Returned,
    Checked,
    Refocus,
}

struct Client {
    endpoint: Rc<Endpoint>,
    opening: Rc<Opening>,
    workspace: Entity<Workspace>,
    mirror: RefCell<Mirror>,
    epoch: Cell<u64>,
    seq: Cell<u64>,
    focus: Rc<Focus>,
    refocus: futures::lock::Mutex<UnboundedReceiver<()>>,
    remembered: Cell<(f64, u64)>,
}

impl Client {
    fn remember(&self) {
        let (epoch, seq) = (self.epoch.get(), self.seq.get());
        keeper::remember(&self.endpoint.base, &self.mirror.borrow().shell(epoch, seq));
        self.remembered.set((net::now(), seq));
    }

    fn remember_if_stale(&self) {
        let (at, seq) = self.remembered.get();
        if self.seq.get() != seq && net::now() - at > REMEMBER_EVERY {
            self.remember();
        }
    }

    fn order(&self, seq: u64) -> Order {
        let last = self.seq.get();
        match seq {
            seq if seq <= last => Order::Old,
            seq if seq == last + 1 => Order::Next,
            _ => Order::Gap,
        }
    }

    fn show(&self, change: &Change, cx: &mut AsyncApp) {
        self.mirror.borrow_mut().apply(change);
        let mirror = self.mirror.borrow();
        self.workspace
            .update(cx, |workspace, cx| apply::change(workspace, &mirror, change, cx));
        self.opening.settle(cx);
    }

    fn take(&self, event: Event, cx: &mut AsyncApp) -> bool {
        match self.order(event.seq) {
            Order::Old => true,
            Order::Gap => false,
            Order::Next => {
                self.seq.set(event.seq);
                self.show(&event.change, cx);
                true
            }
        }
    }

    fn skip(&self, seq: u64) -> bool {
        match self.order(seq) {
            Order::Old => true,
            Order::Gap => false,
            Order::Next => {
                self.seq.set(seq);
                true
            }
        }
    }

    fn adopt_session(&self, key: SessionKey, session: SessionView, cx: &mut AsyncApp) {
        self.show(&Change::SessionPut { key, session }, cx);
    }

    fn carry_focused_items(&self, next: &mut Mirror) {
        let held = self.mirror.borrow();
        for key in self.focus.current() {
            let Some(items) = held.session(&key).map(|session| session.items.clone()) else {
                continue;
            };
            if let Some(session) = next.session_mut(&key) {
                session.items = items;
            }
        }
    }

    async fn resync(&self, cx: &mut AsyncApp) -> bool {
        let Ok(snapshot) = self.endpoint.snapshot().await else {
            return false;
        };
        self.epoch.set(snapshot.epoch);
        self.seq.set(snapshot.seq);
        let mut next = Mirror::from_snapshot(snapshot);
        self.carry_focused_items(&mut next);
        let changes = self.mirror.borrow().diff(&next);
        for change in &changes {
            self.show(change, cx);
        }
        self.remember();
        true
    }

    async fn resync_ending(&self, cx: &mut AsyncApp) -> Ending {
        if self.resync(cx).await {
            Ending::Current
        } else {
            Ending::Lost
        }
    }

    fn handle(&self, frame: Frame, cx: &mut AsyncApp) -> bool {
        match frame {
            Frame::Event { event } => self.take(*event, cx),
            Frame::Quiet { seq } => self.skip(seq),
            Frame::Session { key, session, .. } => {
                self.adopt_session(key, *session, cx);
                true
            }
            Frame::Ping => true,
            Frame::Resync => false,
        }
    }

    async fn listen(
        &self,
        returns: &mut UnboundedReceiver<()>,
        wait: &mut i32,
        cx: &mut AsyncApp,
    ) -> Ending {
        let (sender, mut inbound) = mpsc::unbounded();
        let Ok(socket) = self
            .endpoint
            .subscribe(self.epoch.get(), self.seq.get(), sender)
        else {
            return Ending::Lost;
        };
        let (_ticker, mut checks) = net::ticks(HEARTBEAT_CHECK);
        let mut refocus = self.refocus.lock().await;
        let mut heard = net::now();
        loop {
            let woke = select! {
                message = inbound.next() => Woke::Heard(message),
                _ = returns.next() => Woke::Returned,
                _ = checks.next() => Woke::Checked,
                _ = refocus.next() => Woke::Refocus,
            };
            match woke {
                Woke::Returned => return Ending::Returned,
                Woke::Checked if net::now() - heard > SILENCE_LIMIT => return Ending::Lost,
                Woke::Checked => self.remember_if_stale(),
                Woke::Refocus => socket.focus(&self.focus.current()),
                Woke::Heard(None) | Woke::Heard(Some(Inbound::Closed)) => return Ending::Lost,
                Woke::Heard(Some(Inbound::Opened)) => {
                    heard = net::now();
                    crate::hosts::linked(true);
                    socket.focus(&self.focus.current());
                }
                Woke::Heard(Some(Inbound::Frame(frame))) => {
                    heard = net::now();
                    *wait = FIRST_WAIT;
                    if !self.handle(frame, cx) {
                        return self.resync_ending(cx).await;
                    }
                }
            }
        }
    }

    async fn follow(self: Rc<Self>, cx: &mut AsyncApp) {
        let mut returns = net::returns();
        let mut wait = FIRST_WAIT;
        loop {
            let returned = match self.listen(&mut returns, &mut wait, cx).await {
                Ending::Current => continue,
                Ending::Returned => true,
                Ending::Lost => {
                    crate::hosts::linked(false);
                    select! {
                        _ = net::sleep(wait).fuse() => false,
                        _ = returns.next() => true,
                    }
                }
            };
            drain(&mut returns);
            wait = if returned {
                FIRST_WAIT
            } else {
                (wait * 2).min(LAST_WAIT)
            };
        }
    }
}

enum Ending {
    Current,
    Returned,
    Lost,
}

fn drain(returns: &mut UnboundedReceiver<()>) {
    while returns.try_recv().is_ok() {}
}

async fn fresh_snapshot(endpoint: &Endpoint) -> Result<remote::proto::Snapshot, String> {
    let snapshot = endpoint.snapshot().await.map_err(|failure| failure.to_string())?;
    if snapshot.version != VERSION {
        return Err(format!(
            "the laptop speaks protocol {} and this app speaks {VERSION}",
            snapshot.version
        ));
    }
    keeper::remember(&endpoint.base, &snapshot);
    Ok(snapshot)
}

async fn boot() -> Result<(), String> {
    let endpoint = Rc::new(Endpoint::from_location()?);
    keeper::install(&endpoint.base);
    let snapshot = match keeper::cached(&endpoint.base).filter(|held| held.version == VERSION) {
        Some(held) => held,
        None => fresh_snapshot(&endpoint).await?,
    };
    for project in &snapshot.projects {
        seed::project(project);
    }
    let mut settings = seed::settings(&snapshot);
    crate::wallpaper::install(&mut settings.appearance);
    let faces = fonts::fetch(&endpoint).await;
    faces.name_families(&mut settings.appearance);
    let state = seed::state(&snapshot);
    let epoch = snapshot.epoch;
    let seq = snapshot.seq;
    let mirror = Mirror::from_snapshot(snapshot);

    let platform = Rc::new(gpui_web::WebPlatform::new(false));
    let http_client = Arc::new(platform.fetch_http_client());
    let handle = Application::with_platform(platform)
        .with_http_client(http_client)
        .run_embedded(move |cx: &mut App| {
            let bundled = FONTS.map(Cow::Borrowed).into_iter();
            let fetched = faces.files.into_iter().map(Cow::Owned);
            if let Err(error) = cx.text_system().add_fonts(bundled.chain(fetched).collect())
            {
                show(&format!("font registration failed: {error:?}"));
            }
            cx.set_global(NoBackdropBlur);
            cx.set_global(bezel::ui::tooltip::Hidden);
            boot::init(&settings, cx);
            let window = root::open(settings, state, cx).expect("failed to open the window");
            WINDOW.with(|held| *held.borrow_mut() = Some(window));
            let workspace = window
                .read_with(cx, |root, _| root.workspace())
                .expect("the window holds a workspace");
            let opening = Rc::new(Opening {
                wanted: RefCell::new(None),
                workspace: workspace.clone(),
            });
            crate::laptop::install(endpoint.clone());
            crate::hosts::install();
            crate::pick::install();
            crate::paste::install(window, cx.to_async());
            crate::pictures::install();
            let (outbox, queued) = mpsc::unbounded();
            gui::model::sink::install(Rc::new(Commands { outbox }));
            let sender = cx.to_async();
            wasm_bindgen_futures::spawn_local(send_in_order(
                endpoint.clone(),
                opening.clone(),
                sender,
                queued,
            ));
            let (refocus, refocused) = mpsc::unbounded();
            let focus = Rc::new(Focus {
                keys: RefCell::new(Vec::new()),
                wake: refocus,
            });
            let watching = focus.clone();
            cx.observe(&workspace, move |workspace, cx| {
                watching.follow(watched(workspace.read(cx)))
            })
            .detach();
            let client = Rc::new(Client {
                endpoint,
                opening,
                workspace,
                mirror: RefCell::new(mirror),
                epoch: Cell::new(epoch),
                seq: Cell::new(seq),
                focus,
                refocus: futures::lock::Mutex::new(refocused),
                remembered: Cell::new((net::now(), seq)),
            });
            let initial = client.mirror.borrow().clone();
            client.workspace.update(cx, |workspace, cx| {
                apply::everything(workspace, &initial, cx)
            });
            cx.spawn(async move |cx| client.follow(cx).await).detach();
        });
    APPLICATION.with(|application| *application.borrow_mut() = Some(handle));
    Ok(())
}

#[wasm_bindgen(js_name = cydoniaBack)]
pub fn back() -> bool {
    let Some(window) = WINDOW.with(|held| *held.borrow()) else {
        return false;
    };
    APPLICATION.with(|application| {
        application.borrow().as_ref().is_some_and(|application| {
            application.update(|cx| {
                window
                    .update(cx, |root, window, cx| root.back(window, cx))
                    .unwrap_or(false)
            })
        })
    })
}

#[wasm_bindgen(start)]
pub fn start() {
    std::panic::set_hook(Box::new(|info| {
        console_error_panic_hook::hook(info);
        show(&info.to_string());
    }));
    gpui_web::init_logging();
    wasm_bindgen_futures::spawn_local(async {
        if let Err(error) = boot().await {
            show(&error);
        }
    });
}
