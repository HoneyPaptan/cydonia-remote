//! Browser tabs in the right panel.
//!
//! A tab holds a tab id, not the page. Pages live in [`Pages`], app-wide, so a
//! panel or window dropping does not drop them; only closing the tab does.

use crate::model::{relay, servers};
use bezel::{
    gpui::{
        self, AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, Global,
        KeyBinding, Subscription, Task, Window, actions, div, prelude::*, px,
    },
    motion::{Fade, Painter},
    theme::{TextStyle, Theme, Typeset},
    ui::{
        icons,
        input::TextField,
        tooltip::Tooltip,
        widgets::{ButtonStyle, Buttons as _},
    },
};
#[cfg(not(target_family = "wasm"))]
use browser::WebView;
use browser::WebViewEvent;
#[cfg(target_family = "wasm")]
use phone::Page as WebView;
use remote::proto::{Answer, LocalServer, Query};
use std::{collections::HashMap, path::PathBuf, time::Duration};
use web_time::Instant;

actions!(cydonia_browser, [Go]);

/// Claimed on the address field, so `enter` loads what it holds.
const ADDRESS_CONTEXT: &str = "CydoniaAddress";

const REFRESH: Duration = Duration::from_secs(1);
const STOPPING: Duration = Duration::from_secs(8);
const SHARE_WAIT: Duration = Duration::from_millis(100);
const SHARE_TRIES: usize = 40;
const LISTING_TITLE: &str = "Servers";

pub fn bindings() -> Vec<KeyBinding> {
    vec![KeyBinding::new("enter", Go, Some(ADDRESS_CONTEXT))]
}

/// Whether pages can be shown here. bezel-browser builds pages under X11
/// only, so a gpui window on Wayland shows none.
pub fn supported(cx: &App) -> bool {
    !(cfg!(target_os = "linux") && cx.compositor_name() == "Wayland")
}

/// Every live page, by tab id.
#[derive(Default)]
struct Pages(HashMap<u64, Entity<WebView>>);

impl Global for Pages {}

/// A tab id no other tab holds: tab ids are saved across restarts.
pub fn new_id() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos() as u64)
}

/// Drop the tab's page. Called on closing the tab.
pub fn forget(id: u64, cx: &mut App) {
    if let Some(pages) = cx.try_global::<Pages>()
        && pages.0.contains_key(&id)
    {
        cx.global_mut::<Pages>().0.remove(&id);
    }
}

/// The title or location changed.
pub struct Changed;

pub struct Browser {
    pub id: u64,
    /// Built on first render: a page needs a window.
    page: Option<Entity<WebView>>,
    /// The page's last location, or the saved one until it is built.
    url: String,
    pub(super) title: String,
    address: Entity<TextField>,
    focus: FocusHandle,
    _page: Option<Subscription>,
    root: PathBuf,
    servers: Vec<LocalServer>,
    loaded: bool,
    stopping: HashMap<u16, Instant>,
    opening: Option<u16>,
    note: Option<String>,
    _refresh: Task<()>,
}

impl EventEmitter<Changed> for Browser {}

impl Browser {
    pub fn new(
        id: u64,
        url: String,
        title: String,
        root: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        let address = cx.new(|cx| {
            TextField::new(cx)
                .with_key_context(ADDRESS_CONTEXT)
                .with_placeholder("Search or enter address")
        });
        address.update(cx, |field, cx| field.set_content(url.clone(), cx));
        Self {
            id,
            page: None,
            url,
            title,
            address,
            focus: cx.focus_handle(),
            _page: None,
            root,
            servers: Vec::new(),
            loaded: false,
            stopping: HashMap::new(),
            opening: None,
            note: None,
            _refresh: cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(REFRESH).await;
                    let alive = this.update(cx, |this, cx| {
                        if this.listing() {
                            cx.notify();
                        }
                    });
                    if alive.is_err() {
                        break;
                    }
                }
            }),
        }
    }

    fn listing(&self) -> bool {
        self.url.is_empty()
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// The page's title, or its location while it has none.
    pub fn title(&self) -> &str {
        if self.listing() {
            LISTING_TITLE
        } else if self.title.is_empty() {
            &self.url
        } else {
            &self.title
        }
    }

    fn page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<WebView> {
        if let Some(page) = &self.page {
            return page.clone();
        }
        let existing = cx
            .try_global::<Pages>()
            .and_then(|pages| pages.0.get(&self.id).cloned());
        let page = existing.unwrap_or_else(|| {
            let url = self.url.clone();
            let page = cx.new(|cx| WebView::new(url, window, cx));
            cx.default_global::<Pages>().0.insert(self.id, page.clone());
            page
        });
        let current = page.read(cx);
        if let Some(location) = current.location() {
            self.url = location.to_owned();
        }
        if !current.title().is_empty() {
            self.title = current.title().to_owned();
        }
        self._page = Some(cx.subscribe_in(&page, window, Self::report));
        self.page = Some(page.clone());
        page
    }

    fn report(
        &mut self,
        _: &Entity<WebView>,
        event: &WebViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            WebViewEvent::Location(url) => {
                self.url = url.clone();
                if !self.address.focus_handle(cx).is_focused(window) {
                    self.address
                        .update(cx, |field, cx| field.set_content(url.clone(), cx));
                }
            }
            WebViewEvent::Title(title) => self.title = title.clone(),
            WebViewEvent::Load(_) => {}
        }
        cx.emit(Changed);
        cx.notify();
    }

    fn show(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        forget(self.id, cx);
        self.page = None;
        self._page = None;
        self.title.clear();
        self.address
            .update(cx, |field, cx| field.set_content(url.clone(), cx));
        self.url = url;
        self.note = None;
        window.focus(&self.focus, cx);
        cx.emit(Changed);
        cx.notify();
    }

    fn show_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show(String::new(), window, cx);
    }

    fn refresh_servers(&mut self) {
        let Ok(listed) = servers::list(&self.root) else {
            return;
        };
        self.loaded = true;
        self.stopping.retain(|port, at| {
            at.elapsed() < STOPPING && listed.iter().any(|server| server.port == *port)
        });
        self.servers = listed
            .into_iter()
            .filter(|server| !self.stopping.contains_key(&server.port))
            .collect();
    }

    fn stop(&mut self, server: &LocalServer, cx: &mut Context<Self>) {
        self.stopping.insert(server.port, Instant::now());
        servers::stop(server);
        cx.notify();
    }

    fn open(&mut self, server: &LocalServer, window: &mut Window, cx: &mut Context<Self>) {
        let url = format!("http://localhost:{}/", server.port);
        if !cfg!(target_family = "wasm") || server.shared {
            self.show(url, window, cx);
            return;
        }
        let query = Query::Expose { port: server.port };
        relay::take(&query);
        relay::tell(query.clone());
        self.opening = Some(server.port);
        self.note = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let mut answer = None;
            for _ in 0..SHARE_TRIES {
                cx.background_executor().timer(SHARE_WAIT).await;
                answer = relay::take(&query);
                if answer.is_some() {
                    break;
                }
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.opening = None;
                match answer {
                    Some(Answer::Exposed) => this.show(url, window, cx),
                    Some(Answer::Failed { message }) => this.note = Some(message),
                    _ => this.note = Some("The laptop did not answer".to_owned()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn go(&mut self, _: &Go, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.address.read(cx).content().trim().to_owned();
        if typed.is_empty() {
            return;
        }
        let url = address(&typed);
        let page = self.page(window, cx);
        if cfg!(target_family = "wasm") {
            self.url = url.clone();
            self.title.clear();
            cx.emit(Changed);
        }
        page.update(cx, |page, cx| {
            page.load(url);
            cx.notify();
        });
        window.focus(&page.focus_handle(cx), cx);
    }
}

/// What the address field's text loads: a URL as typed, a bare port on this
/// laptop, a bare host over https.
fn address(typed: &str) -> String {
    if typed.contains("://") || typed.starts_with("about:") {
        typed.to_owned()
    } else if typed.chars().all(|c| c.is_ascii_digit()) {
        format!("http://localhost:{typed}")
    } else if typed.starts_with("localhost") || typed.starts_with("127.") {
        format!("http://{typed}")
    } else {
        format!("https://{typed}")
    }
}

impl Focusable for Browser {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match &self.page {
            Some(page) => page.focus_handle(cx),
            None => self.focus.clone(),
        }
    }
}

impl Browser {
    fn listing_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.refresh_servers();
        let theme = Theme::of(cx).clone();
        let caption = |text: String| {
            div()
                .flex_none()
                .px(px(12.))
                .py(px(8.))
                .text_style(TextStyle::Caption)
                .text_color(theme.text_muted)
                .child(text)
        };
        let rows: Vec<AnyElement> = self
            .servers
            .clone()
            .into_iter()
            .map(|server| self.server_row(server, &theme, cx))
            .collect();
        let status = match (self.loaded, rows.is_empty()) {
            (false, _) => Some("Looking for running servers"),
            (true, true) => Some("Nothing is running on this laptop yet. Start a dev server and it shows up here."),
            _ => None,
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .child(caption("Running on this laptop".to_owned()))
            .children(self.note.clone().map(caption))
            .children(status.map(|text| caption(text.to_owned())))
            .child(
                div()
                    .id("running-servers")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(6.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(rows),
            )
            .into_any_element()
    }

    fn server_row(
        &self,
        server: LocalServer,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let port = server.port;
        let primary = server
            .title
            .clone()
            .or_else(|| server.folder.clone())
            .unwrap_or_else(|| server.process.clone());
        let mut details = vec![format!(":{port}"), server.process.clone()];
        details.extend(server.folder.clone().filter(|folder| *folder != primary));
        let secondary = if self.opening == Some(port) {
            "Opening".to_owned()
        } else {
            details.join(" · ")
        };
        let (open, stop) = (server.clone(), server);
        div()
            .id(("server", usize::from(port)))
            .flex_none()
            .h(px(52.))
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(10.))
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|row| row.bg(theme.element_hover))
            .child(
                icons::icon(icons::development::Server)
                    .size(px(16.))
                    .flex_none()
                    .text_color(theme.text_muted),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().text_style(TextStyle::Body).child(primary))
                    .child(
                        div()
                            .truncate()
                            .text_style(TextStyle::Caption)
                            .text_color(theme.text_muted)
                            .child(secondary),
                    ),
            )
            .child(
                theme
                    .icon_button(icons::notifications::X, ButtonStyle::Ghost, None)
                    .id(("server-stop", usize::from(port)))
                    .flex_none()
                    .tooltip(|window, cx| Tooltip::text("Stop this server", window, cx))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.stop(&stop, cx);
                    })),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.open(&open, window, cx)))
            .into_any_element()
    }
}

impl Render for Browser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !supported(cx) {
            let theme = Theme::of(cx);
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .track_focus(&self.focus)
                .text_style(TextStyle::Caption)
                .text_color(theme.text_muted)
                .child("The browser needs X11. This session runs on Wayland.")
                .into_any_element();
        }
        if self.listing() {
            return self.listing_view(cx);
        }
        let page = self.page(window, cx);
        let theme = Theme::of(cx).clone();
        let loading = page.read(cx).is_loading();
        let nav = |icon: &'static [u8], id: &'static str, tip: &'static str| {
            theme
                .icon_button(
                    icon,
                    ButtonStyle::Ghost,
                    Some(Fade::new(Painter::of(cx), id)),
                )
                .id(id)
                .flex_none()
                .tooltip(move |window, cx| Tooltip::text(tip, window, cx))
        };
        let back = page.clone();
        let forward = page.clone();
        let reload = page.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(2.))
                    .px(px(6.))
                    .py(px(4.))
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        nav(
                            icons::development::Server,
                            "browser-servers",
                            "Running servers",
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.show_list(window, cx))),
                    )
                    .child(
                        nav(icons::arrows::ArrowLeft, "browser-back", "Back")
                            .on_click(move |_, _, cx| {
                                back.update(cx, |page, cx| {
                                    page.back();
                                    cx.notify();
                                })
                            }),
                    )
                    .child(
                        nav(icons::arrows::ArrowRight, "browser-forward", "Forward")
                            .on_click(move |_, _, cx| {
                                forward.update(cx, |page, cx| {
                                    page.forward();
                                    cx.notify();
                                })
                            }),
                    )
                    .child(
                        nav(
                            icons::arrows::RefreshCw,
                            "browser-reload",
                            if loading { "Loading…" } else { "Reload" },
                        )
                        .on_click(move |_, _, cx| {
                            reload.update(cx, |page, cx| {
                                page.reload();
                                cx.notify();
                            })
                        }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .on_action(cx.listener(Self::go))
                            .child(self.address.clone()),
                    ),
            )
            .child(div().flex_1().min_h_0().child(page))
            .into_any_element()
    }
}

#[cfg(target_family = "wasm")]
mod phone {
    use crate::model::relay::{self, PageOp};
    use bezel::{
        gpui::{
            App, AppContext as _, Bounds, Context, Element, ElementId, Entity, EventEmitter,
            FocusHandle, Focusable, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId,
            IntoElement, LayoutId, Pixels, Render, Style, Window, relative,
        },
        ui::cover::{self, Mark},
    };
    use browser::{Frame, WebViewEvent};
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::atomic::{AtomicU64, Ordering},
    };

    const LOOPBACK: [&str; 4] = ["localhost", "127.0.0.1", "0.0.0.0", "[::1]"];

    fn reach(address: &str) -> String {
        let (Some(laptop), Ok(mut url)) = (relay::laptop(), url::Url::parse(address)) else {
            return address.to_owned();
        };
        if url
            .host_str()
            .is_some_and(|host| LOOPBACK.contains(&host))
            && url.set_host(Some(laptop)).is_ok()
        {
            return url.to_string();
        }
        address.to_owned()
    }

    fn fresh() -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    pub struct Page {
        id: u64,
        url: Rc<RefCell<String>>,
        frame: Option<Entity<Frame>>,
        focus: FocusHandle,
    }

    impl EventEmitter<WebViewEvent> for Page {}

    impl Drop for Page {
        fn drop(&mut self) {
            relay::page(PageOp::Close { id: self.id });
        }
    }

    impl Page {
        pub fn new(url: String, _: &mut Window, cx: &mut Context<Self>) -> Self {
            let url = reach(&url);
            let frame = (!relay::pages_installed()).then(|| {
                let url = url.clone();
                cx.new(|_| Frame::new(url))
            });
            Self {
                id: fresh(),
                url: Rc::new(RefCell::new(url)),
                frame,
                focus: cx.focus_handle(),
            }
        }

        pub fn location(&self) -> Option<&str> {
            None
        }

        pub fn title(&self) -> &str {
            ""
        }

        pub fn is_loading(&self) -> bool {
            false
        }

        pub fn load(&mut self, url: String) {
            let url = reach(&url);
            *self.url.borrow_mut() = url.clone();
            relay::page(PageOp::Load { id: self.id, url });
        }

        pub fn back(&mut self) {
            relay::page(PageOp::Back { id: self.id });
        }

        pub fn forward(&mut self) {
            relay::page(PageOp::Forward { id: self.id });
        }

        pub fn reload(&mut self) {
            relay::page(PageOp::Reload { id: self.id });
        }
    }

    impl Focusable for Page {
        fn focus_handle(&self, _: &App) -> FocusHandle {
            self.focus.clone()
        }
    }

    impl Render for Page {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            if let Some(frame) = &self.frame {
                return frame.clone().into_any_element();
            }
            Native {
                id: self.id,
                url: self.url.clone(),
            }
            .into_any_element()
        }
    }

    struct Native {
        id: u64,
        url: Rc<RefCell<String>>,
    }

    struct Shown(u64);

    impl Drop for Shown {
        fn drop(&mut self) {
            relay::page(PageOp::Park { id: self.0 });
        }
    }

    impl IntoElement for Native {
        type Element = Self;

        fn into_element(self) -> Self::Element {
            self
        }
    }

    impl Element for Native {
        type RequestLayoutState = ();
        type PrepaintState = (Hitbox, Mark);

        fn id(&self) -> Option<ElementId> {
            Some("native-page".into())
        }

        fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
            None
        }

        fn request_layout(
            &mut self,
            _: Option<&GlobalElementId>,
            _: Option<&InspectorElementId>,
            window: &mut Window,
            cx: &mut App,
        ) -> (LayoutId, ()) {
            let mut style = Style::default();
            style.size.width = relative(1.).into();
            style.size.height = relative(1.).into();
            (window.request_layout(style, [], cx), ())
        }

        fn prepaint(
            &mut self,
            _: Option<&GlobalElementId>,
            _: Option<&InspectorElementId>,
            bounds: Bounds<Pixels>,
            _: &mut (),
            window: &mut Window,
            _: &mut App,
        ) -> (Hitbox, Mark) {
            (
                window.insert_hitbox(bounds, HitboxBehavior::BlockMouse),
                cover::mark(),
            )
        }

        fn paint(
            &mut self,
            id: Option<&GlobalElementId>,
            _: Option<&InspectorElementId>,
            bounds: Bounds<Pixels>,
            _: &mut (),
            (_, mark): &mut (Hitbox, Mark),
            window: &mut Window,
            cx: &mut App,
        ) {
            let Some(id) = id else { return };
            let covered = cover::covered(*mark, bounds, window, cx);
            let page = self.id;
            let op = match covered {
                true => PageOp::Park { id: page },
                false => PageOp::Place {
                    id: page,
                    url: self.url.borrow().clone(),
                    x: f32::from(bounds.origin.x),
                    y: f32::from(bounds.origin.y),
                    width: f32::from(bounds.size.width),
                    height: f32::from(bounds.size.height),
                },
            };
            window.with_element_state::<Shown, _>(id, |shown, _| {
                let shown = shown.unwrap_or(Shown(page));
                relay::page(op);
                ((), shown)
            });
        }
    }
}
