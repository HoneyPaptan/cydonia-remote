use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use remote::proto::{Ack, Answer, Command, Frame, SessionKey, Snapshot, Query, Upstream};
use std::{cell::Cell, fmt};
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use web_sys::{CloseEvent, Headers, MessageEvent, Request, RequestInit, Response, WebSocket};

const TOKEN_KEY: &str = "token=";
const PROTOCOL: &str = "cydonia";
const RESUMED: &str = "cydonia-resume";
const PAUSED: &str = "cydonia-pause";

thread_local! {
    static AWAY: Cell<bool> = const { Cell::new(false) };
}

pub struct Endpoint {
    pub base: String,
    pub token: String,
}

pub enum Inbound {
    Opened,
    Frame(Frame),
    Closed,
}

#[derive(Debug)]
pub enum Failure {
    Status(u16),
    Network(String),
}

impl Failure {
    pub fn retryable(&self) -> bool {
        match self {
            Failure::Network(_) => true,
            Failure::Status(code) => *code >= 500 || *code == 429,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Status(code) => write!(out, "answered {code}"),
            Failure::Network(message) => out.write_str(message),
        }
    }
}

fn network(error: JsValue) -> Failure {
    Failure::Network(text(error))
}

pub struct Socket {
    socket: WebSocket,
    _open: Closure<dyn FnMut()>,
    _message: Closure<dyn FnMut(MessageEvent)>,
    _close: Closure<dyn FnMut(CloseEvent)>,
}

impl Socket {
    pub fn focus(&self, keys: &[SessionKey]) {
        let Ok(body) = serde_json::to_string(&Upstream::Focus { keys: keys.to_vec() }) else {
            return;
        };
        let _ = self.socket.send_with_str(&body);
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        self.socket.set_onopen(None);
        self.socket.set_onmessage(None);
        self.socket.set_onclose(None);
        let _ = self.socket.close();
    }
}

pub struct Ticker {
    handle: i32,
    _tick: Closure<dyn FnMut()>,
}

impl Drop for Ticker {
    fn drop(&mut self) {
        if let Some(window) = web_sys::window() {
            window.clear_interval_with_handle(self.handle);
        }
    }
}

pub fn ticks(milliseconds: i32) -> (Option<Ticker>, UnboundedReceiver<()>) {
    let (beat, beats) = mpsc::unbounded();
    let tick = Closure::<dyn FnMut()>::new(move || {
        let _ = beat.unbounded_send(());
    });
    let handle = web_sys::window().and_then(|window| {
        window
            .set_interval_with_callback_and_timeout_and_arguments_0(
                tick.as_ref().unchecked_ref(),
                milliseconds,
            )
            .ok()
    });
    (handle.map(|handle| Ticker { handle, _tick: tick }), beats)
}

pub fn now() -> f64 {
    js_sys::Date::now()
}

fn version() -> Option<String> {
    let window = web_sys::window()?;
    js_sys::Reflect::get(&window, &"cydoniaVersion".into())
        .ok()?
        .as_string()
        .filter(|version| !version.is_empty())
}

fn versioned(path: &str) -> String {
    match version() {
        Some(version) => format!("{path}?v={version}"),
        None => path.to_owned(),
    }
}

fn window() -> Result<web_sys::Window, String> {
    web_sys::window().ok_or_else(|| "no window".to_owned())
}

fn text(error: JsValue) -> String {
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}

impl Endpoint {
    pub fn from_location() -> Result<Self, String> {
        let location = window()?.location();
        let base = location.origin().map_err(text)?;
        let hash = location.hash().map_err(text)?;
        let token = hash
            .trim_start_matches('#')
            .split('&')
            .find_map(|part| part.strip_prefix(TOKEN_KEY))
            .filter(|token| !token.is_empty())
            .ok_or_else(|| "no token in the address".to_owned())?;
        Ok(Self {
            base,
            token: token.to_owned(),
        })
    }

    async fn call(&self, method: &str, path: &str, body: Option<String>) -> Result<String, Failure> {
        let headers = Headers::new().map_err(network)?;
        headers
            .set("Authorization", &format!("Bearer {}", self.token))
            .map_err(network)?;
        let init = RequestInit::new();
        init.set_method(method);
        if let Some(body) = body {
            headers.set("Content-Type", "application/json").map_err(network)?;
            init.set_body(&JsValue::from_str(&body));
        }
        init.set_headers(&headers);
        let request =
            Request::new_with_str_and_init(&format!("{}{path}", self.base), &init).map_err(network)?;
        let window = web_sys::window().ok_or_else(|| Failure::Network("no window".to_owned()))?;
        let response: Response = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(network)?
            .dyn_into()
            .map_err(network)?;
        if !response.ok() {
            return Err(Failure::Status(response.status()));
        }
        JsFuture::from(response.text().map_err(network)?)
            .await
            .map_err(network)?
            .as_string()
            .ok_or_else(|| Failure::Network("a body that is not text".to_owned()))
    }

    async fn call_json<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<String>,
    ) -> Result<T, Failure> {
        let answer = self.call(method, path, body).await?;
        serde_json::from_str(&answer).map_err(|error| Failure::Network(error.to_string()))
    }

    pub async fn asset(&self, path: &str) -> Option<Vec<u8>> {
        let address = format!("{}/{}", self.base, versioned(path));
        let response: Response = JsFuture::from(window().ok()?.fetch_with_str(&address))
            .await
            .ok()?
            .dyn_into()
            .ok()?;
        if !response.ok() {
            return None;
        }
        let buffer = JsFuture::from(response.array_buffer().ok()?).await.ok()?;
        Some(js_sys::Uint8Array::new(&buffer).to_vec())
    }

    pub async fn snapshot(&self) -> Result<Snapshot, Failure> {
        self.call_json("GET", "/v1/snapshot?scope=shell", None).await
    }

    pub async fn command(&self, command: &Command) -> Result<Ack, Failure> {
        let body = serde_json::to_string(command).map_err(|error| Failure::Network(error.to_string()))?;
        self.call_json("POST", "/v1/commands", Some(body)).await
    }

    pub async fn query(&self, query: &Query) -> Result<Answer, String> {
        let body = serde_json::to_string(query).map_err(|error| error.to_string())?;
        self.call_json("POST", "/v1/query", Some(body))
            .await
            .map_err(|failure| format!("/v1/query {failure}"))
    }

    fn socket_base(&self) -> String {
        self.base
            .replacen("https://", "wss://", 1)
            .replacen("http://", "ws://", 1)
    }

    fn protocols(&self) -> js_sys::Array {
        js_sys::Array::of2(&PROTOCOL.into(), &self.token.as_str().into())
    }

    pub fn shell(&self, cwd: &str, cols: u16, rows: u16) -> Result<WebSocket, String> {
        let cwd: String = url_encode(cwd);
        let address = format!(
            "{}/v1/shell?cwd={cwd}&cols={cols}&rows={rows}",
            self.socket_base()
        );
        WebSocket::new_with_str_sequence(&address, &self.protocols()).map_err(text)
    }

    pub fn socket_url(&self, epoch: u64, seq: u64) -> String {
        format!("{}/v1/events?epoch={epoch}&seq={seq}", self.socket_base())
    }

    pub fn subscribe(
        &self,
        epoch: u64,
        seq: u64,
        inbound: UnboundedSender<Inbound>,
    ) -> Result<Socket, String> {
        let socket = WebSocket::new_with_str_sequence(&self.socket_url(epoch, seq), &self.protocols())
            .map_err(text)?;
        let opened = inbound.clone();
        let frames = inbound.clone();
        let open = Closure::<dyn FnMut()>::new(move || {
            let _ = opened.unbounded_send(Inbound::Opened);
        });
        let message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let Some(body) = event.data().as_string() else {
                return;
            };
            let frame = serde_json::from_str::<Frame>(&body).unwrap_or(Frame::Resync);
            let _ = frames.unbounded_send(Inbound::Frame(frame));
        });
        let close = Closure::<dyn FnMut(CloseEvent)>::new(move |_: CloseEvent| {
            let _ = inbound.unbounded_send(Inbound::Closed);
        });
        socket.set_onopen(Some(open.as_ref().unchecked_ref()));
        socket.set_onmessage(Some(message.as_ref().unchecked_ref()));
        socket.set_onclose(Some(close.as_ref().unchecked_ref()));
        Ok(Socket {
            socket,
            _open: open,
            _message: message,
            _close: close,
        })
    }
}

fn url_encode(text: &str) -> String {
    js_sys::encode_uri_component(text).into()
}

fn listen(
    target: &web_sys::EventTarget,
    name: &str,
    wake: UnboundedSender<()>,
    when: fn() -> bool,
) {
    let callback = Closure::<dyn FnMut()>::new(move || {
        if when() {
            let _ = wake.unbounded_send(());
        }
    });
    let _ = target.add_event_listener_with_callback(name, callback.as_ref().unchecked_ref());
    callback.forget();
}

fn visible() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .is_some_and(|document| document.visibility_state() == web_sys::VisibilityState::Visible)
}

pub fn away() -> bool {
    AWAY.get()
}

fn present() -> bool {
    !away()
}

fn arrive() -> bool {
    AWAY.set(false);
    true
}

fn depart() -> bool {
    AWAY.set(true);
    true
}

fn shown() -> bool {
    visible() && arrive()
}

fn hidden() -> bool {
    !visible() && depart()
}

pub fn returns() -> UnboundedReceiver<()> {
    let (wake, returns) = mpsc::unbounded();
    if let Some(window) = web_sys::window() {
        listen(&window, "online", wake.clone(), present);
        listen(&window, RESUMED, wake.clone(), arrive);
        if let Some(document) = window.document() {
            listen(&document, "visibilitychange", wake, shown);
        }
    }
    returns
}

pub fn leaves() -> UnboundedReceiver<()> {
    let (wake, leaves) = mpsc::unbounded();
    if let Some(window) = web_sys::window() {
        listen(&window, PAUSED, wake.clone(), depart);
        if let Some(document) = window.document() {
            listen(&document, "visibilitychange", wake, hidden);
        }
    }
    leaves
}

pub async fn sleep(milliseconds: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(window) = web_sys::window() {
            let _ = window
                .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, milliseconds);
        }
    });
    let _ = JsFuture::from(promise).await;
}

pub fn fresh_id() -> String {
    let now = js_sys::Date::now() as u64;
    let noise = (js_sys::Math::random() * 9_007_199_254_740_991.0) as u64;
    format!("{now:x}-{noise:x}")
}
