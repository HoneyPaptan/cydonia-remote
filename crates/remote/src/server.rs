use crate::{
    hub::Hub,
    log::Replay,
    proto::{Ack, Action, Command, Event, Frame, Outcome, Reason, Snapshot},
    receipts::Receipts,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures::channel::{mpsc, oneshot};
use serde::Deserialize;
use std::{
    io,
    net::SocketAddr,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::UNIX_EPOCH,
};
use tokio::{net::TcpListener, sync::broadcast::error::RecvError};

const RECEIPTS: usize = 1024;

pub type Dispatch = mpsc::UnboundedSender<(Action, oneshot::Sender<Outcome>)>;

pub struct Config {
    pub token: String,
    pub ui: Option<PathBuf>,
}

#[derive(Clone)]
struct Shared {
    hub: Arc<Hub>,
    token: Arc<str>,
    ui: Option<Arc<Path>>,
    dispatch: Dispatch,
    receipts: Arc<tokio::sync::Mutex<Receipts>>,
}

const PROTOCOL: &str = "cydonia";

#[derive(Deserialize)]
struct Subscribe {
    epoch: Option<u64>,
    seq: Option<u64>,
}

pub fn router(config: Config, hub: Arc<Hub>, dispatch: Dispatch) -> Router {
    let shared = Shared {
        hub,
        token: config.token.into(),
        ui: config.ui.map(Into::into),
        dispatch,
        receipts: Arc::new(tokio::sync::Mutex::new(Receipts::new(RECEIPTS))),
    };
    Router::new()
        .route("/v1/snapshot", get(snapshot))
        .route("/v1/commands", post(command))
        .route("/v1/events", get(events))
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

async fn snapshot(
    State(shared): State<Shared>,
    headers: HeaderMap,
) -> Result<Json<Snapshot>, StatusCode> {
    if !authorized(&shared, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(shared.hub.snapshot()))
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

async fn send(socket: &mut WebSocket, frame: &Frame) -> bool {
    let Ok(text) = serde_json::to_string(frame) else {
        return false;
    };
    socket.send(Message::Text(text.into())).await.is_ok()
}

fn event_frame(event: Event) -> Frame {
    Frame::Event {
        event: Box::new(event),
    }
}

async fn stream(mut socket: WebSocket, hub: Arc<Hub>, epoch: u64, seq: u64) {
    let mut subscription = hub.subscribe(epoch, seq);
    let mut last = match subscription.replay {
        Replay::Resync => {
            send(&mut socket, &Frame::Resync).await;
            return;
        }
        Replay::Events(events) => {
            let mut last = seq;
            for event in events {
                last = event.seq;
                if !send(&mut socket, &event_frame(event)).await {
                    return;
                }
            }
            last
        }
    };
    loop {
        tokio::select! {
            received = subscription.live.recv() => match received {
                Ok(event) if event.seq <= last => {}
                Ok(event) => {
                    last = event.seq;
                    if !send(&mut socket, &event_frame(event)).await {
                        return;
                    }
                }
                Err(RecvError::Lagged(_)) => {
                    send(&mut socket, &Frame::Resync).await;
                    return;
                }
                Err(RecvError::Closed) => return,
            },
            incoming = socket.recv() => match incoming {
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
    answer.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    answer.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    if let Ok(tag) = HeaderValue::from_str(&tag) {
        answer.insert(header::ETAG, tag);
    }
    if gzip {
        answer.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    }
    response
}
