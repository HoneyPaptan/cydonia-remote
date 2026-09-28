use remote::proto::{Action, Command, Outcome, SessionKey, Snapshot, Status};
use std::{
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command as Process, Stdio},
    time::{Duration, Instant},
};

const AGENT: &str = r#"
import json, sys

def send(message):
    print(json.dumps(message), flush=True)

def reply(id, result):
    send({'jsonrpc': '2.0', 'id': id, 'result': result})

def say(session, text):
    send({'jsonrpc': '2.0', 'method': 'session/update', 'params': {
        'sessionId': session, 'update': {'sessionUpdate': 'agent_message_chunk',
        'content': {'type': 'text', 'text': text}}}})

def wait_for(id):
    for line in sys.stdin:
        message = json.loads(line)
        if message.get('id') == id and 'method' not in message:
            return message.get('result', {})

for line in sys.stdin:
    request = json.loads(line)
    method = request.get('method')
    if 'id' not in request:
        continue
    if method == 'initialize':
        reply(request['id'], {'protocolVersion': 1, 'agentCapabilities': {'loadSession': True}, 'authMethods': []})
    elif method in ('session/new', 'session/load'):
        reply(request['id'], {'sessionId': request.get('params', {}).get('sessionId', 'remote-e2e')})
    elif method == 'session/prompt':
        session = request['params']['sessionId']
        text = ''.join(block.get('text', '') for block in request['params']['prompt'])
        if 'permission' in text:
            send({'jsonrpc': '2.0', 'id': 'ask-1', 'method': 'session/request_permission', 'params': {
                'sessionId': session,
                'toolCall': {'toolCallId': 'tool-1', 'title': 'cargo test --workspace'},
                'options': [{'optionId': 'allow', 'name': 'Allow', 'kind': 'allow_once'},
                            {'optionId': 'deny', 'name': 'Deny', 'kind': 'reject_once'}]}})
            outcome = wait_for('ask-1').get('outcome', {})
            say(session, 'permission ' + outcome.get('optionId', outcome.get('outcome', 'none')))
        else:
            say(session, 'echo: ' + text)
        reply(request['id'], {'stopReason': 'end_turn'})
    else:
        send({'jsonrpc': '2.0', 'id': request['id'], 'error': {'code': -32601, 'message': 'unsupported'}})
"#;

struct Home {
    root: PathBuf,
}

impl Home {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("cydonia-e2e-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let config = root.join("config/cydonia");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(root.join("project/.cydonia/boards")).unwrap();
        std::fs::write(
            root.join("project/.cydonia/boards/launch.toml"),
            "name = \"Launch\"\nkey = \"LAUNCH\"\n",
        )
        .unwrap();
        std::fs::write(root.join("agent.py"), AGENT).unwrap();
        std::fs::write(
            config.join("settings.toml"),
            format!(
                "[[agents]]\nname = \"fake\"\ncommand = \"/usr/bin/python3\"\nargs = [\"-u\", {:?}]\n",
                root.join("agent.py").to_string_lossy()
            ),
        )
        .unwrap();
        std::fs::write(
            config.join("state.toml"),
            format!(
                "projects = [{:?}]\n",
                root.join("project").to_string_lossy()
            ),
        )
        .unwrap();
        Self { root }
    }

    fn project(&self) -> String {
        self.root.join("project").to_string_lossy().into_owned()
    }

    fn token(&self) -> String {
        std::fs::read_to_string(self.root.join("config/cydonia/remote-token")).unwrap()
    }
}

struct Daemon {
    child: Child,
    base: String,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start(home: &Home) -> Daemon {
    let port = free_port();
    let child = Process::new(env!("CARGO_BIN_EXE_cydonia"))
        .args([
            "--headless",
            "--remote",
            "--listen",
            &format!("127.0.0.1:{port}"),
        ])
        .env("HOME", &home.root)
        .env("XDG_CONFIG_HOME", home.root.join("config"))
        .env("XDG_DATA_HOME", home.root.join("data"))
        .env("XDG_CACHE_HOME", home.root.join("cache"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let daemon = Daemon {
        child,
        base: format!("http://127.0.0.1:{port}"),
    };
    eventually(|| snapshot_of(&daemon, home).ok());
    daemon
}

fn eventually<T>(mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(found) = probe() {
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "condition not met within 30 seconds"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn http() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into()
}

fn snapshot_of(daemon: &Daemon, home: &Home) -> Result<Snapshot, ureq::Error> {
    let token = std::fs::read_to_string(home.root.join("config/cydonia/remote-token"))
        .map_err(ureq::Error::Io)?;
    let text = http()
        .get(format!("{}/v1/snapshot", daemon.base))
        .header("Authorization", format!("Bearer {token}"))
        .call()?
        .body_mut()
        .read_to_string()?;
    Ok(serde_json::from_str(&text).unwrap())
}

fn send(daemon: &Daemon, home: &Home, id: &str, action: Action) -> Outcome {
    let body = serde_json::to_string(&Command {
        id: id.into(),
        action,
    })
    .unwrap();
    let text = http()
        .post(format!("{}/v1/commands", daemon.base))
        .header("Authorization", format!("Bearer {}", home.token()))
        .header("Content-Type", "application/json")
        .send(body)
        .unwrap()
        .body_mut()
        .read_to_string()
        .unwrap();
    serde_json::from_str::<serde_json::Value>(&text)
        .map(|ack| serde_json::from_value(ack["outcome"].clone()).unwrap())
        .unwrap()
}

fn transcript(daemon: &Daemon, home: &Home, key: &SessionKey) -> Vec<String> {
    snapshot_of(daemon, home)
        .unwrap()
        .projects
        .iter()
        .find(|project| project.path == key.project)
        .and_then(|project| project.sessions.get(&key.record))
        .map(|session| {
            session
                .items
                .iter()
                .map(|item| format!("{item:?}"))
                .collect()
        })
        .unwrap_or_default()
}

fn session_count(daemon: &Daemon, home: &Home) -> usize {
    snapshot_of(daemon, home)
        .unwrap()
        .projects
        .iter()
        .map(|project| project.sessions.len())
        .sum()
}

fn created(outcome: Outcome) -> SessionKey {
    match outcome {
        Outcome::Created { key } => key,
        other => panic!("expected a created session, got {other:?}"),
    }
}

fn has(lines: &[String], needle: &str) -> bool {
    lines.iter().any(|line| line.contains(needle))
}

#[test]
fn a_phone_drives_a_laptop_session_end_to_end() {
    if !Path::new("/usr/bin/python3").exists() {
        return;
    }
    let home = Home::new("drive");
    let daemon = start(&home);
    let new_session = Action::NewSession {
        project: home.project(),
        agent: Some("fake".into()),
        text: Some("hello".into()),
    };
    let key = created(send(&daemon, &home, "new-1", new_session.clone()));
    eventually(|| has(&transcript(&daemon, &home, &key), "echo: hello").then_some(()));

    assert_eq!(
        created(send(&daemon, &home, "new-1", new_session)),
        key,
        "a retried command answers the first acknowledgement"
    );
    assert_eq!(session_count(&daemon, &home), 1);

    let prompt = Action::SendPrompt {
        key: key.clone(),
        text: "again".into(),
    };
    assert_eq!(
        send(&daemon, &home, "prompt-1", prompt.clone()),
        Outcome::Accepted
    );
    assert_eq!(send(&daemon, &home, "prompt-1", prompt), Outcome::Accepted);
    eventually(|| has(&transcript(&daemon, &home, &key), "echo: again").then_some(()));
    let agains = transcript(&daemon, &home, &key)
        .iter()
        .filter(|line| line.contains("User(\"again\")"))
        .count();
    assert_eq!(agains, 1, "the retried prompt reached the agent once");

    send(
        &daemon,
        &home,
        "prompt-2",
        Action::SendPrompt {
            key: key.clone(),
            text: "needs permission".into(),
        },
    );
    let request = eventually(|| {
        let snapshot = snapshot_of(&daemon, &home).ok()?;
        let header = &snapshot.projects[0].sessions.get(&key.record)?.header;
        (header.status == Status::WaitingForPermission)
            .then(|| {
                header
                    .permission
                    .as_ref()
                    .map(|permission| permission.request)
            })
            .flatten()
    });
    assert!(matches!(
        send(
            &daemon,
            &home,
            "answer-stale",
            Action::RespondPermission {
                key: key.clone(),
                request: request + 1,
                option: "allow".into(),
            },
        ),
        Outcome::Rejected { .. }
    ));
    assert_eq!(
        send(
            &daemon,
            &home,
            "answer-1",
            Action::RespondPermission {
                key: key.clone(),
                request,
                option: "allow".into(),
            },
        ),
        Outcome::Accepted
    );
    eventually(|| has(&transcript(&daemon, &home, &key), "permission allow").then_some(()));

    let board = "id = \"launch\"\nname = \"Launch, renamed on the phone\"\nkey = \"LAUNCH\"\n";
    assert_eq!(
        send(
            &daemon,
            &home,
            "board-1",
            Action::SaveBoard {
                project: home.project(),
                board: board.into(),
            },
        ),
        Outcome::Accepted
    );
    eventually(|| {
        let snapshot = snapshot_of(&daemon, &home).ok()?;
        let file = snapshot.projects[0].files.get("boards/launch.toml")?;
        String::from_utf8_lossy(&file.0)
            .contains("Launch, renamed on the phone")
            .then_some(())
    });

    let epoch = snapshot_of(&daemon, &home).unwrap().epoch;
    drop(daemon);
    let restarted = start(&home);
    let after = snapshot_of(&restarted, &home).unwrap();
    assert_ne!(after.epoch, epoch, "a restart is a new epoch");
    let lines = transcript(&restarted, &home, &key);
    assert!(
        has(&lines, "echo: hello") && has(&lines, "permission allow"),
        "{lines:?}"
    );
}
