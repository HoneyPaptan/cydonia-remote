use crate::{
    hub::Hub,
    log::Replay,
    proto::{
        Ack, Action, Answer, Command, Frame, Outcome, Query as Asked, Reason, Screen,
        SessionKey, ShellInput, Upstream,
    },
    receipts::Receipts,
    scope::Scope,
};
use axum::{
    Json, Router,
    body::Body,
    extract::DefaultBodyLimit,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures::channel::{mpsc, oneshot};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Write as _},
    net::SocketAddr,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};
use tokio::{net::TcpListener, sync::broadcast::error::RecvError};

const RECEIPTS: usize = 1024;
const COMMAND_BODY_LIMIT: usize = 64 * 1024 * 1024;
const BEAT: Duration = Duration::from_secs(10);
const HANDOVER_GRACE: Duration = Duration::from_millis(250);
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const REVALIDATE: &str = "no-cache";

pub type StepDown = Arc<dyn Fn() + Send + Sync>;

pub type Dispatch = mpsc::UnboundedSender<(Action, oneshot::Sender<Outcome>)>;

#[derive(Default)]
pub struct Config {
    pub token: String,
    pub ui: Option<PathBuf>,
    pub local: Option<Arc<dyn Local>>,
    pub receipts: Option<PathBuf>,
    pub step_down: Option<StepDown>,
}

pub trait Local: Send + Sync + 'static {
    fn answer(&self, query: Asked) -> Answer;
    fn shell(&self, cwd: &str, cols: u16, rows: u16) -> Option<ShellLink>;
}

pub struct ShellLink {
    pub input: Box<dyn Fn(ShellInput) + Send + Sync>,
    pub screens: mpsc::UnboundedReceiver<Screen>,
}

impl Drop for ShellLink {
    fn drop(&mut self) {
        (self.input)(ShellInput::Close);
    }
}

#[derive(Clone)]
struct Shared {
    hub: Arc<Hub>,
    token: Arc<str>,
    ui: Option<Arc<Path>>,
    dispatch: Dispatch,
    receipts: Arc<tokio::sync::Mutex<Receipts>>,
    local: Option<Arc<dyn Local>>,
    step_down: Option<StepDown>,
}

const PROTOCOL: &str = "cydonia";
const NOTICE_WAIT_LIMIT: u64 = 30;

#[derive(Deserialize)]
struct Open {
    cwd: String,
    cols: Option<u16>,
    rows: Option<u16>,
}

#[derive(Deserialize)]
struct Scoped {
    scope: Option<String>,
}

#[derive(Deserialize)]
struct Waiting {
    after: Option<u64>,
    wait: Option<u64>,
}

#[derive(Deserialize)]
struct Subscribe {
    epoch: Option<u64>,
    seq: Option<u64>,
}

pub fn router(config: Config, hub: Arc<Hub>, dispatch: Dispatch) -> Router {
    let receipts = match config.receipts {
        Some(path) => Receipts::journaled(path, RECEIPTS),
        None => Receipts::new(RECEIPTS),
    };
    let shared = Shared {
        hub,
        token: config.token.into(),
        ui: config.ui.map(Into::into),
        dispatch,
        receipts: Arc::new(tokio::sync::Mutex::new(receipts)),
        local: config.local,
        step_down: config.step_down,
    };
    Router::new()
        .route("/v1/snapshot", get(snapshot))
        .route(
            "/v1/commands",
            post(command).layer(DefaultBodyLimit::max(COMMAND_BODY_LIMIT)),
        )
        .route("/v1/events", get(events))
        .route("/v1/notices", get(notices))
        .route("/v1/query", post(query))
        .route("/v1/shell", get(shell))
        .route("/v1/handover", post(handover))
        .fallback(get(ui))
        .with_state(shared)
}

pub async fn bind(addresses: &[SocketAddr]) -> io::Result<Vec<TcpListener>> {
    let mut listeners = Vec::with_capacity(addresses.len());
    for address in addresses {
        listeners.push(TcpListener::bind(address).await?);
    }
    Ok(listeners)
}

pub async fn serve(listeners: Vec<TcpListener>, router: Router) -> io::Result<()> {
    let serving = listeners
        .into_iter()
        .map(|listener| axum::serve(listener, router.clone()).into_future());
    for result in futures::future::join_all(serving).await {
        result?;
    }
    Ok(())
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |diff, (x, y)| diff | (x ^ y))
            == 0
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

fn offered_protocols(headers: &HeaderMap) -> impl Iterator<Item = &str> {
    headers
        .get_all(header::SEC_WEBSOCKET_PROTOCOL)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
}

fn authorized(shared: &Shared, headers: &HeaderMap) -> bool {
    bearer(headers).is_some_and(|token| same(token, &shared.token))
}

fn authorized_socket(shared: &Shared, headers: &HeaderMap) -> bool {
    authorized(shared, headers)
        || offered_protocols(headers).any(|offered| same(offered, &shared.token))
}

fn gzip(body: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(body)?;
    encoder.finish()
}

fn json_response<T: Serialize>(headers: &HeaderMap, value: &T) -> Response {
    let Ok(plain) = serde_json::to_vec(value) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let (body, encoded) = match accepts_gzip(headers).then(|| gzip(&plain)) {
        Some(Ok(packed)) => (packed, true),
        _ => (plain, false),
    };
    let mut response = Body::from(body).into_response();
    let answer = response.headers_mut();
    answer.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    answer.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    if encoded {
        answer.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    }
    response
}

async fn snapshot(
    State(shared): State<Shared>,
    Query(scoped): Query<Scoped>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&shared, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match scoped.scope.as_deref() {
        Some("shell") => json_response(&headers, &shared.hub.shell_snapshot()),
        _ => json_response(&headers, &shared.hub.snapshot()),
    }
}

async fn notices(
    State(shared): State<Shared>,
    Query(waiting): Query<Waiting>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&shared, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(after) = waiting.after else {
        return json_response(&headers, &shared.hub.notices_after(u64::MAX));
    };
    let mut changes = shared.hub.watch_notices();
    let patience = Duration::from_secs(waiting.wait.unwrap_or_default().min(NOTICE_WAIT_LIMIT));
    let deadline = tokio::time::Instant::now() + patience;
    loop {
        let found = shared.hub.notices_after(after);
        if !found.notices.is_empty() {
            return json_response(&headers, &found);
        }
        match tokio::time::timeout_at(deadline, changes.changed()).await {
            Ok(Ok(())) => continue,
            _ => return json_response(&headers, &found),
        }
    }
}

async fn handover(State(shared): State<Shared>, headers: HeaderMap) -> StatusCode {
    if !authorized(&shared, &headers) {
        return StatusCode::UNAUTHORIZED;
    }
    let Some(step_down) = shared.step_down.clone() else {
        return StatusCode::NOT_FOUND;
    };
    tokio::spawn(async move {
        tokio::time::sleep(HANDOVER_GRACE).await;
        step_down();
    });
    StatusCode::ACCEPTED
}

async fn command(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Json(command): Json<Command>,
) -> Result<Json<Ack>, StatusCode> {
    if !authorized(&shared, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let mut receipts = shared.receipts.lock().await;
    if let Some(ack) = receipts.get(&command.id) {
        return Ok(Json(ack.clone()));
    }
    let (reply, answer) = oneshot::channel();
    let outcome = match shared.dispatch.unbounded_send((command.action, reply)) {
        Ok(()) => answer.await.unwrap_or(Outcome::Rejected {
            reason: Reason::Unavailable,
        }),
        Err(_) => Outcome::Rejected {
            reason: Reason::Unavailable,
        },
    };
    let ack = Ack {
        id: command.id,
        outcome,
    };
    receipts.keep(ack.clone());
    Ok(Json(ack))
}

async fn events(
    State(shared): State<Shared>,
    Query(query): Query<Subscribe>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !authorized_socket(&shared, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let epoch = query.epoch.unwrap_or_default();
    let seq = query.seq.unwrap_or_default();
    upgrade
        .protocols([PROTOCOL])
        .on_upgrade(move |socket| stream(socket, shared.hub, epoch, seq))
}

async fn query(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Json(asked): Json<Asked>,
) -> Response {
    if !authorized(&shared, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(local) = shared.local.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match tokio::task::spawn_blocking(move || local.answer(asked)).await {
        Ok(answer) => json_response(&headers, &answer),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn shell(
    State(shared): State<Shared>,
    Query(open): Query<Open>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !authorized_socket(&shared, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(local) = shared.local.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let cols = open.cols.unwrap_or(80).clamp(8, 500);
    let rows = open.rows.unwrap_or(24).clamp(2, 300);
    let Some(link) = local.shell(&open.cwd, cols, rows) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    upgrade
        .protocols([PROTOCOL])
        .on_upgrade(move |socket| drive_shell(socket, link))
}

async fn drive_shell(mut socket: WebSocket, mut link: ShellLink) {
    use futures::StreamExt as _;
    loop {
        tokio::select! {
            screen = link.screens.next() => {
                let Some(screen) = screen else { return };
                let Ok(text) = serde_json::to_string(&screen) else { return };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    return;
                }
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(input) = serde_json::from_str::<ShellInput>(&text) {
                        (link.input)(input);
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                Some(Ok(_)) => {}
            },
        }
    }
}

async fn send(socket: &mut WebSocket, frame: &Frame) -> bool {
    let Ok(text) = serde_json::to_string(frame) else {
        return false;
    };
    socket.send(Message::Text(text.into())).await.is_ok()
}

async fn follow_focus(
    socket: &mut WebSocket,
    hub: &Hub,
    scope: &mut Scope,
    keys: Vec<SessionKey>,
) -> bool {
    scope.retain(&keys);
    for key in keys {
        if scope.focused(&key) {
            continue;
        }
        let Some((session, seq)) = hub.session_at(&key) else {
            continue;
        };
        scope.follow(key.clone(), seq);
        let frame = Frame::Session {
            key,
            seq,
            session: Box::new(session),
        };
        if !send(socket, &frame).await {
            return false;
        }
    }
    true
}

async fn stream(mut socket: WebSocket, hub: Arc<Hub>, epoch: u64, seq: u64) {
    let mut subscription = hub.subscribe(epoch, seq);
    let mut scope = Scope::default();
    let mut last = seq;
    match subscription.replay {
        Replay::Resync => {
            send(&mut socket, &Frame::Resync).await;
            return;
        }
        Replay::Events(events) => {
            for event in events {
                last = event.seq;
                if !send(&mut socket, &scope.frame(event)).await {
                    return;
                }
            }
        }
    }
    let mut beat = tokio::time::interval_at(tokio::time::Instant::now() + BEAT, BEAT);
    beat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            received = subscription.live.recv() => match received {
                Ok(event) if event.seq <= last => {}
                Ok(event) => {
                    last = event.seq;
                    if !send(&mut socket, &scope.frame(event)).await {
                        return;
                    }
                }
                Err(RecvError::Lagged(_)) => {
                    send(&mut socket, &Frame::Resync).await;
                    return;
                }
                Err(RecvError::Closed) => return,
            },
            _ = beat.tick() => {
                if !send(&mut socket, &Frame::Ping).await {
                    return;
                }
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(Upstream::Focus { keys }) = serde_json::from_str(&text)
                        && !follow_focus(&mut socket, &hub, &mut scope, keys).await
                    {
                        return;
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                Some(Ok(_)) => {}
            },
        }
    }
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn contained(root: &Path, requested: &str) -> Option<PathBuf> {
    let relative = Path::new(requested.trim_start_matches('/'));
    if requested.contains('\\')
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let joined = root.join(relative);
    let path = if joined.is_dir() {
        joined.join("index.html")
    } else {
        joined
    };
    let resolved = path.canonicalize().ok()?;
    resolved
        .starts_with(root.canonicalize().ok()?)
        .then_some(resolved)
}

fn is_versioned(query: &str) -> bool {
    query.split('&').any(|pair| pair.starts_with("v="))
}

fn accepts_gzip(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|coding| {
            coding
                .split(';')
                .next()
                .is_some_and(|name| name.trim() == "gzip")
        })
}

fn version_tag(metadata: &std::fs::Metadata, gzip: bool) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_nanos());
    let variant = if gzip { "-gz" } else { "" };
    format!("\"{:x}-{modified:x}{variant}\"", metadata.len())
}

fn unchanged(headers: &HeaderMap, tag: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == tag))
}

async fn ui(State(shared): State<Shared>, uri: Uri, headers: HeaderMap) -> Response {
    let Some(root) = shared.ui.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(path) = contained(root, uri.path()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let compressed = accepts_gzip(&headers)
        .then(|| contained(root, &format!("{}.gz", uri.path())))
        .flatten();
    let gzip = compressed.is_some();
    let file = compressed.unwrap_or_else(|| path.clone());
    let Ok(metadata) = tokio::fs::metadata(&file).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let tag = version_tag(&metadata, gzip);
    let mut response = if unchanged(&headers, &tag) {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        match tokio::fs::read(&file).await {
            Ok(bytes) => Body::from(bytes).into_response(),
            Err(_) => return StatusCode::NOT_FOUND.into_response(),
        }
    };
    let answer = response.headers_mut();
    answer.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(&path)),
    );
    let caching = if uri.query().is_some_and(is_versioned) { IMMUTABLE } else { REVALIDATE };
    answer.insert(header::CACHE_CONTROL, HeaderValue::from_static(caching));
    answer.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    if let Ok(tag) = HeaderValue::from_str(&tag) {
        answer.insert(header::ETAG, tag);
    }
    if gzip {
        answer.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    }
    response
}
