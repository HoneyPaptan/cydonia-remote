mod common;

use common::{file, key};
use cydonia_remote::proto::{Action, Change, Command, Frame};
use serde_json::json;

#[test]
fn commands_are_semantic_json() {
    let command: Command = serde_json::from_value(json!({
        "id": "cmd_123",
        "action": {
            "type": "send_prompt",
            "key": {"project": "/p", "record": "s"},
            "text": "Continue implementation"
        }
    }))
    .unwrap();
    assert_eq!(
        command.action,
        Action::SendPrompt {
            key: key("/p", "s"),
            text: "Continue implementation".into(),
        }
    );
}

#[test]
fn unknown_command_type_is_refused() {
    let parsed = serde_json::from_value::<Command>(json!({
        "id": "cmd_1",
        "action": {"type": "execute_shell", "command": "rm -rf /"}
    }));
    assert!(parsed.is_err());
}

#[test]
fn file_bytes_travel_as_base64() {
    let change = Change::FilePut {
        project: "/p".into(),
        path: "boards/a.toml".into(),
        file: file("name = \"A\""),
    };
    let text = serde_json::to_string(&change).unwrap();
    assert!(text.contains("bmFtZSA9ICJBIg=="));
    assert_eq!(serde_json::from_str::<Change>(&text).unwrap(), change);
}

#[test]
fn resync_frame_is_tagged() {
    assert_eq!(
        serde_json::to_value(Frame::Resync).unwrap(),
        json!({"type": "resync"})
    );
}
