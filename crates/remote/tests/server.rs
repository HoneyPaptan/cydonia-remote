mod common;

use common::{agent, key, project, session, user};
use cydonia_remote::{
    hub::Hub,
    mirror::Mirror,
    proto::{Action, Command, Frame, Outcome, Reason, Snapshot},
    server::{self, Config},
};
use futures::{StreamExt as _, channel::mpsc};
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_tungstenite::tungstenite::{self, Message, client::IntoClientRequest as _};

const TOKEN: &str = "s3cret-token";
const STANDING_REQUEST: u64 = 7;

struct Running {
    address: SocketAddr,
    hub: Arc<Hub>,
    prompts: Arc<AtomicUsize>,
}

async fn start() -> Running {
    start_serving(None).await
}

async fn start_serving(ui: Option<std::path::PathBuf>) -> Running {
    let hub = Hub::new(99);
    let (dispatch, mut actions) = mpsc::unbounded();
    let prompts = Arc::new(AtomicUsize::new(0));
    let counted = prompts.clone();
    tokio::spawn(async move {
        while let Some((action, reply)) = actions.next().await {
            let outcome = match action {
                Action::SendPrompt { .. } => {
                    counted.fetch_add(1, Ordering::SeqCst);
                    Outcome::Accepted
                }
                Action::RespondPermission { request, .. } if request == STANDING_REQUEST => {
                    Outcome::Accepted
                }
                Action::RespondPermission { .. } => Outcome::Rejected {
                    reason: Reason::StalePermission,
                },
                _ => Outcome::Accepted,
            };
            let _ = futures::channel::oneshot::Sender::send(reply, outcome);
        }
    });
    let router = server::router(
        Config {
            token: TOKEN.into(),
            ui,
        },
        hub.clone(),
        dispatch,
    );
    let listeners = server::bind(&["127.0.0.1:0".parse().unwrap()])
        .await
        .unwrap();
    let address = listeners[0].local_addr().unwrap();
    tokio::spawn(server::serve(listeners, router));
    Running {
        address,
        hub,
        prompts,
    }
}

async fn get(address: SocketAddr, path: &str, token: Option<&str>) -> Result<String, u16> {
    let url = format!("http://{address}{path}");
    let token = token.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        let mut request = ureq::get(&url);
        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        match request.call() {
            Ok(mut response) => Ok(response.body_mut().read_to_string().unwrap()),
            Err(ureq::Error::StatusCode(code)) => Err(code),
            Err(other) => panic!("{other}"),
        }
    })
    .await
    .unwrap()
}

async fn post(address: SocketAddr, body: String, token: &str) -> Result<String, u16> {
    let url = format!("http://{address}/v1/commands");
    let token = token.to_owned();
    tokio::task::spawn_blocking(move || {
        match ureq::post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .send(body)
        {
            Ok(mut response) => Ok(response.body_mut().read_to_string().unwrap()),
            Err(ureq::Error::StatusCode(code)) => Err(code),
            Err(other) => panic!("{other}"),
        }
    })
    .await
    .unwrap()
}

async fn snapshot(address: SocketAddr) -> Snapshot {
    serde_json::from_str(&get(address, "/v1/snapshot", Some(TOKEN)).await.unwrap()).unwrap()
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(
    address: SocketAddr,
    query: &str,
    protocols: &str,
) -> Result<Socket, tungstenite::Error> {
    let mut request = format!("ws://{address}/v1/events{query}")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", protocols.parse().unwrap());
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
}

async fn subscribe(address: SocketAddr, epoch: u64, seq: u64) -> Socket {
    connect(
        address,
        &format!("?epoch={epoch}&seq={seq}"),
        &format!("cydonia, {TOKEN}"),
    )
    .await
    .unwrap()
}

async fn next_frame(socket: &mut Socket) -> Option<Frame> {
    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .expect("no frame within five seconds")?;
    match message.ok()? {
        Message::Text(text) => Some(serde_json::from_str(&text).unwrap()),
        _ => None,
    }
}

async fn next_seq(socket: &mut Socket) -> u64 {
    match next_frame(socket).await {
        Some(Frame::Event { event }) => event.seq,
        other => panic!("expected an event, got {other:?}"),
    }
}

fn mirror_with(items: Vec<artifact::session::chat::ChatItem>) -> Mirror {
    Mirror {
        agents: Vec::new(),
        projects: vec![project("/p", &[("s", session(items))])],
    }
}

fn prompt_command(id: &str) -> String {
    serde_json::to_string(&Command {
        id: id.into(),
        action: Action::SendPrompt {
            key: key("/p", "s"),
            text: "Continue".into(),
        },
    })
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_without_the_token_are_refused() {
    let running = start().await;
    assert_eq!(get(running.address, "/v1/snapshot", None).await, Err(401));
    assert_eq!(
        get(running.address, "/v1/snapshot", Some("wrong")).await,
        Err(401)
    );
    assert_eq!(
        post(running.address, prompt_command("c"), "wrong").await,
        Err(401)
    );
    let refused =
        tokio_tungstenite::connect_async(format!("ws://{}/v1/events?token=wrong", running.address))
            .await;
    assert!(matches!(refused, Err(tungstenite::Error::Http(_))));
    assert_eq!(running.prompts.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_then_subscribe_continues_at_the_next_sequence() {
    let running = start().await;
    running.hub.publish(mirror_with(vec![user("go")]));
    let taken = snapshot(running.address).await;
    assert_eq!((taken.epoch, taken.seq), (99, 1));
    assert_eq!(Mirror::from_snapshot(taken.clone()), running.hub.mirror());
    let mut socket = subscribe(running.address, taken.epoch, taken.seq).await;
    running
        .hub
        .publish(mirror_with(vec![user("go"), agent("ok")]));
    assert_eq!(next_seq(&mut socket).await, taken.seq + 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn reconnect_replays_missed_events_once() {
    let running = start().await;
    running.hub.publish(mirror_with(vec![user("go")]));
    running
        .hub
        .publish(mirror_with(vec![user("go"), agent("a")]));
    running
        .hub
        .publish(mirror_with(vec![user("go"), agent("ab")]));
    let mut socket = subscribe(running.address, 99, 1).await;
    assert_eq!(next_seq(&mut socket).await, 2);
    assert_eq!(next_seq(&mut socket).await, 3);
    running
        .hub
        .publish(mirror_with(vec![user("go"), agent("abc")]));
    assert_eq!(next_seq(&mut socket).await, 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn another_epoch_is_told_to_resync() {
    let running = start().await;
    running.hub.publish(mirror_with(vec![user("go")]));
    let mut socket = subscribe(running.address, 12, 1842).await;
    assert_eq!(next_frame(&mut socket).await, Some(Frame::Resync));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_retried_command_runs_once() {
    let running = start().await;
    let first = post(running.address, prompt_command("cmd_123"), TOKEN)
        .await
        .unwrap();
    let second = post(running.address, prompt_command("cmd_123"), TOKEN)
        .await
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(running.prompts.load(Ordering::SeqCst), 1);
    post(running.address, prompt_command("cmd_124"), TOKEN)
        .await
        .unwrap();
    assert_eq!(running.prompts.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_commands_are_refused_before_dispatch() {
    let running = start().await;
    let shell = r#"{"id":"x","action":{"type":"execute_shell","command":"ls"}}"#;
    assert!(
        post(running.address, shell.into(), TOKEN)
            .await
            .unwrap_err()
            >= 400
    );
    assert!(
        post(running.address, "not json".into(), TOKEN)
            .await
            .unwrap_err()
            >= 400
    );
    assert_eq!(running.prompts.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_permission_answer_is_rejected() {
    let running = start().await;
    let answer = |request| {
        serde_json::to_string(&Command {
            id: format!("perm_{request}"),
            action: Action::RespondPermission {
                key: key("/p", "s"),
                request,
                option: "allow".into(),
            },
        })
        .unwrap()
    };
    let stale: serde_json::Value =
        serde_json::from_str(&post(running.address, answer(3), TOKEN).await.unwrap()).unwrap();
    assert_eq!(stale["outcome"]["reason"], "stale_permission");
    let standing: serde_json::Value = serde_json::from_str(
        &post(running.address, answer(STANDING_REQUEST), TOKEN)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(standing["outcome"]["type"], "accepted");
}

#[tokio::test(flavor = "multi_thread")]
async fn live_events_arrive_in_order_while_streaming() {
    let running = start().await;
    let mut socket = subscribe(running.address, 99, 0).await;
    let mut text = String::new();
    for word in ["one ", "two ", "three"] {
        text.push_str(word);
        running
            .hub
            .publish(mirror_with(vec![user("go"), agent(&text)]));
    }
    let seqs = [
        next_seq(&mut socket).await,
        next_seq(&mut socket).await,
        next_seq(&mut socket).await,
    ];
    assert_eq!(seqs, [1, 2, 3]);
    socket.close(None).await.unwrap();
}

fn ui_dir(name: &str) -> std::path::PathBuf {
    let base = std::env::temp_dir().join(format!("cydonia-ui-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let ui = base.join("ui");
    std::fs::create_dir_all(&ui).unwrap();
    std::fs::write(ui.join("index.html"), "<main>cydonia</main>").unwrap();
    std::fs::write(base.join("secret.txt"), "outside").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(base.join("secret.txt"), ui.join("escape.txt")).unwrap();
    ui
}

#[tokio::test(flavor = "multi_thread")]
async fn the_ui_is_served_without_a_token() {
    let running = start_serving(Some(ui_dir("index"))).await;
    assert_eq!(
        get(running.address, "/", None).await.unwrap(),
        "<main>cydonia</main>"
    );
    assert_eq!(
        get(running.address, "/index.html", None).await.unwrap(),
        "<main>cydonia</main>"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_outside_the_ui_directory_is_served() {
    let running = start_serving(Some(ui_dir("escape"))).await;
    for path in [
        "/../secret.txt",
        "/%2e%2e/secret.txt",
        "/escape.txt",
        "/..%2fsecret.txt",
    ] {
        assert_eq!(get(running.address, path, None).await, Err(404), "{path}");
    }
}
