use crate::protocol::{Command, DaemonState, Meter, Response, Status};

#[test]
fn test_command_serde() {
    let cmds = vec![
        (Command::Press, r#"{"cmd":"press"}"#),
        (Command::Release, r#"{"cmd":"release"}"#),
        (Command::Cancel, r#"{"cmd":"cancel"}"#),
        (Command::Status, r#"{"cmd":"status"}"#),
        (Command::Ping, r#"{"cmd":"ping"}"#),
    ];

    for (cmd, json) in cmds {
        let serialized = serde_json::to_string(&cmd).unwrap();
        assert_eq!(serialized, json);
        let deserialized: Command = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized, cmd);
    }
}

#[test]
fn test_response_serde() {
    let resp = Response::ok("test").with_state(DaemonState {
        status: Status::Recording,
        meter: Meter {
            average_power: 0.5,
            peak_power: 0.8,
        },
        message: None,
    });

    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"ok\":true"));
    assert!(json.contains("\"status\":\"recording\""));
    assert!(json.contains("\"average_power\":0.5"));
}

#[test]
fn test_status_variants() {
    let statuses = vec![
        Status::Hidden,
        Status::Preparing,
        Status::Recording,
        Status::Transcribing,
        Status::Prewarming,
        Status::Error,
    ];

    for status in statuses {
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: Status = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, status);
    }
}
