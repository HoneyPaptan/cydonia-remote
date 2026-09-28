use crate::{
    apply,
    net::{self, Endpoint, Inbound},
    seed,
};
use bezel::gpui::{App, Application, ApplicationHandle, AsyncApp, Entity};
use futures::{
    FutureExt as _, StreamExt as _,
    channel::mpsc::{self, UnboundedReceiver},
    select,
};
use gui::{
    boot,
    model::{
        sink::{Sink, Write},
        workspace::{NoBackdropBlur, Workspace},
    },
    view::root,
};
use remote::{
    mirror::Mirror,
    proto::{Action, Command, Event, Frame, Outcome, SessionKey, VERSION},
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
const ATTEMPTS: usize = 6;

thread_local! {
    static APPLICATION: RefCell<Option<ApplicationHandle>> = const { RefCell::new(None) };
}

fn show(text: &str) {
    if let Some(boot) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("boot"))
    {
        boot.set_text_content(Some(&format!("Cydonia did not start: {text}")));
    }
}

fn hide_boot() {
    if let Some(boot) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("boot"))
    {
        boot.remove();
    }
}

struct Commands {
    endpoint: Rc<Endpoint>,
    opening: Rc<Opening>,
    app: AsyncApp,
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
        let endpoint = self.endpoint.clone();
        let opening = self.opening.clone();
        let mut cx = self.app.clone();
        let command = Command {
            id: net::fresh_id(),
            action,
        };
        wasm_bindgen_futures::spawn_local(async move {
            let mut wait = FIRST_WAIT;
            for _ in 0..ATTEMPTS {
                if let Ok(ack) = endpoint.command(&command).await {
                    if let Outcome::Created { key } = ack.outcome {
                        opening.want(key);
                        opening.settle(&mut cx);
                    }
                    return;
                }
                net::sleep(wait).await;
                wait = (wait * 2).min(LAST_WAIT);
            }
        });
    }
}

impl Sink for Commands {
    fn send_prompt(&self, project: &Path, record: &str, text: String) {
        self.deliver(Action::SendPrompt {
            key: key(project, record),
            text,
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

    fn write(&self, project: &Path, write: Write) {
        if let Some(action) = crate::write::action(project.to_string_lossy().into_owned(), write) {
            self.deliver(action);
        }
    }

    fn new_session(&self, project: &Path, agent: &str, text: Option<String>) {
        self.deliver(Action::NewSession {
            project: project.to_string_lossy().into_owned(),
            agent: Some(agent.to_owned()),
            text,
        });
    }
}

struct Client {
    endpoint: Rc<Endpoint>,
    opening: Rc<Opening>,
    workspace: Entity<Workspace>,
    mirror: RefCell<Mirror>,
    epoch: Cell<u64>,
    seq: Cell<u64>,
}

impl Client {
    fn take(&self, event: Event, cx: &mut AsyncApp) -> bool {
        let last = self.seq.get();
        if event.seq <= last {
            return true;
        }
        if event.seq != last + 1 {
            return false;
        }
        self.mirror.borrow_mut().apply(&event.change);
        self.seq.set(event.seq);
        let mirror = self.mirror.borrow();
        self.workspace.update(cx, |workspace, cx| {
            apply::change(workspace, &mirror, &event.change, cx)
        });
        self.opening.settle(cx);
        true
    }

    async fn resync(&self, cx: &mut AsyncApp) -> bool {
        let Ok(snapshot) = self.endpoint.snapshot().await else {
            return false;
        };
        self.epoch.set(snapshot.epoch);
        self.seq.set(snapshot.seq);
        *self.mirror.borrow_mut() = Mirror::from_snapshot(snapshot);
        let mirror = self.mirror.borrow();
        self.workspace.update(cx, |workspace, cx| {
            apply::everything(workspace, &mirror, cx)
        });
        self.opening.settle(cx);
        true
    }

    async fn resync_ending(&self, cx: &mut AsyncApp) -> Ending {
        if self.resync(cx).await {
            Ending::Current
        } else {
            Ending::Lost
        }
    }

    async fn listen(
        &self,
        returns: &mut UnboundedReceiver<()>,
        wait: &mut i32,
        cx: &mut AsyncApp,
    ) -> Ending {
        let (sender, mut inbound) = mpsc::unbounded();
        let Ok(_socket) = self
            .endpoint
            .subscribe(self.epoch.get(), self.seq.get(), sender)
        else {
            return Ending::Lost;
        };
        loop {
            let message = select! {
                message = inbound.next() => message,
                _ = returns.next() => return Ending::Returned,
            };
            match message {
                Some(Inbound::Frame(Frame::Event { event })) => {
                    *wait = FIRST_WAIT;
                    if !self.take(*event, cx) {
                        return self.resync_ending(cx).await;
                    }
                }
                Some(Inbound::Frame(Frame::Resync)) => return self.resync_ending(cx).await,
                Some(Inbound::Closed) | None => return Ending::Lost,
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
                Ending::Lost => select! {
                    _ = net::sleep(wait).fuse() => false,
                    _ = returns.next() => true,
                },
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

async fn boot() -> Result<(), String> {
    let endpoint = Rc::new(Endpoint::from_location()?);
    let snapshot = endpoint.snapshot().await?;
    if snapshot.version != VERSION {
        return Err(format!(
            "the laptop speaks protocol {} and this app speaks {VERSION}",
            snapshot.version
        ));
    }
    for project in &snapshot.projects {
        seed::project(project);
    }
    let settings = seed::settings(&snapshot);
    let state = seed::state(&snapshot);
    let epoch = snapshot.epoch;
    let seq = snapshot.seq;
    let mirror = Mirror::from_snapshot(snapshot);

    let platform = Rc::new(gpui_web::WebPlatform::new(false));
    let http_client = Arc::new(platform.fetch_http_client());
    let handle = Application::with_platform(platform)
        .with_http_client(http_client)
        .run_embedded(move |cx: &mut App| {
            if let Err(error) = cx
                .text_system()
                .add_fonts(FONTS.map(Cow::Borrowed).to_vec())
            {
                show(&format!("font registration failed: {error:?}"));
            }
            cx.set_global(NoBackdropBlur);
            cx.set_global(bezel::ui::tooltip::Hidden);
            boot::init(&settings, cx);
            let window = root::open(settings, state, cx).expect("failed to open the window");
            let workspace = window
                .read_with(cx, |root, _| root.workspace())
                .expect("the window holds a workspace");
            let opening = Rc::new(Opening {
                wanted: RefCell::new(None),
                workspace: workspace.clone(),
            });
            crate::laptop::install(endpoint.clone());
            gui::model::sink::install(Rc::new(Commands {
                endpoint: endpoint.clone(),
                opening: opening.clone(),
                app: cx.to_async(),
            }));
            let client = Rc::new(Client {
                endpoint,
                opening,
                workspace,
                mirror: RefCell::new(mirror),
                epoch: Cell::new(epoch),
                seq: Cell::new(seq),
            });
            let initial = client.mirror.borrow().clone();
            client.workspace.update(cx, |workspace, cx| {
                apply::everything(workspace, &initial, cx)
            });
            cx.spawn(async move |cx| client.follow(cx).await).detach();
            hide_boot();
        });
    APPLICATION.with(|application| *application.borrow_mut() = Some(handle));
    Ok(())
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
