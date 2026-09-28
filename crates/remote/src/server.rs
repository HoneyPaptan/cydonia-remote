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
    http::{HeaderMap, StatusCode, Uri, header},
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

#[derive(Deserialize)]
struct Subscribe {
    epoch: Option<u64>,
    seq: Option<u64>,
    token: Option<String>,
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
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

fn authorized(shared: &Shared, headers: &HeaderMap, query: Option<&str>) -> bool {
    bearer(headers)
        .or(query)
        .is_some_and(|token| same(token, &shared.token))
}

async fn snapshot(
    State(shared): State<Shared>,
    Query(query): Query<TokenQuery>,
    headers: HeaderMap,
) -> Result<Json<Snapshot>, StatusCode> {
    if !authorized(&shared, &headers, query.token.as_deref()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(shared.hub.snapshot()))
}

async fn command(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Json(command): Json<Command>,
) -> Result<Json<Ack>, StatusCode> {
    if !authorized(&shared, &headers, None) {
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
    if !authorized(&shared, &headers, query.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let epoch = query.epoch.unwrap_or_default();
    let seq = query.seq.unwrap_or_default();
    upgrade.on_upgrade(move |socket| stream(socket, shared.hub, epoch, seq))
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
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let path = root.join(relative);
    Some(if path.is_dir() {
        path.join("index.html")
    } else {
        path
    })
}

async fn ui(State(shared): State<Shared>, uri: Uri) -> Response {
    let Some(root) = shared.ui.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(path) = contained(root, uri.path()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, content_type(&path)),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            Body::from(bytes),
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
