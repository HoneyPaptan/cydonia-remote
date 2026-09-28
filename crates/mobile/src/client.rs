use crate::{
    apply,
    net::{self, Endpoint, Inbound},
    seed,
};
use bezel::gpui::{App, Application, ApplicationHandle, AsyncApp, Entity};
use futures::{StreamExt as _, channel::mpsc};
use gui::{
    boot,
    model::{
        sink::{Sink, Write},
        workspace::Workspace,
    },
    view::root,
};
use remote::{
    mirror::Mirror,
    proto::{Action, Command, Event, Frame, SessionKey, VERSION},
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
        let command = Command {
            id: net::fresh_id(),
            action,
        };
        wasm_bindgen_futures::spawn_local(async move {
            let mut wait = FIRST_WAIT;
            for _ in 0..ATTEMPTS {
                if endpoint.command(&command).await.is_ok() {
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
        true
    }

    async fn resync(&self, cx: &mut AsyncApp) {
        let Ok(snapshot) = self.endpoint.snapshot().await else {
            return;
        };
        self.epoch.set(snapshot.epoch);
        self.seq.set(snapshot.seq);
        *self.mirror.borrow_mut() = Mirror::from_snapshot(snapshot);
        let mirror = self.mirror.borrow();
        self.workspace.update(cx, |workspace, cx| {
            apply::everything(workspace, &mirror, cx)
        });
    }

    async fn follow(self: Rc<Self>, cx: &mut AsyncApp) {
        let mut wait = FIRST_WAIT;
        loop {
            let (sender, mut inbound) = mpsc::unbounded();
            if let Ok(_socket) = self
                .endpoint
                .subscribe(self.epoch.get(), self.seq.get(), sender)
            {
                while let Some(message) = inbound.next().await {
                    match message {
                        Inbound::Frame(Frame::Event { event }) => {
                            wait = FIRST_WAIT;
                            if !self.take(*event, cx) {
                                self.resync(cx).await;
                                break;
                            }
                        }
                        Inbound::Frame(Frame::Resync) => {
                            self.resync(cx).await;
                            break;
                        }
                        Inbound::Closed => break,
                    }
                }
            }
            net::sleep(wait).await;
            wait = (wait * 2).min(LAST_WAIT);
        }
    }
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
            boot::init(&settings, cx);
            let window = root::open(settings, state, cx).expect("failed to open the window");
            let workspace = window
                .read_with(cx, |root, _| root.workspace())
                .expect("the window holds a workspace");
            gui::model::sink::install(Rc::new(Commands {
                endpoint: endpoint.clone(),
            }));
            let client = Rc::new(Client {
                endpoint,
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
