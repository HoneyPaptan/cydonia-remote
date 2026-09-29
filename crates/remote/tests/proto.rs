mod common;

use common::{file, key};
use cydonia_remote::proto::{Action, Change, Command, Event, Frame, SessionHeader, Status};
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

#[test]
fn a_model_pick_travels_as_a_semantic_command() {
    let command: Command = serde_json::from_value(json!({
        "id": "cmd_7",
        "action": {
            "type": "set_config",
            "key": {"project": "/p", "record": "s"},
            "config": "model",
            "value": "opus"
        }
    }))
    .unwrap();
    assert_eq!(
        command.action,
        Action::SetConfig {
            key: key("/p", "s"),
            config: "model".into(),
            value: "opus".into(),
        }
    );
}

#[test]
fn a_header_without_switches_still_reads() {
    let mut value = serde_json::to_value(common::header(Status::Idle)).unwrap();
    let fields = value.as_object_mut().unwrap();
    fields.remove("config");
    fields.remove("modes");
    let header: SessionHeader = serde_json::from_value(value).unwrap();
    assert!(header.config.is_empty());
    assert_eq!(header.modes, None);
}

#[test]
fn a_header_change_with_send_times_reads_back_as_a_frame() {
    let mut header = common::header(Status::Idle);
    header.sent_at.insert(0, 1_790_639_505);
    header.sent_at.insert(4, 1_790_640_052);
    let frame = Frame::Event {
        event: Box::new(Event {
            seq: 27,
            change: Change::SessionHeader {
                key: key("/project", "record"),
                header,
            },
        }),
    };
    let wire = serde_json::to_string(&frame).unwrap();
    assert_eq!(serde_json::from_str::<Frame>(&wire).unwrap(), frame);
}
