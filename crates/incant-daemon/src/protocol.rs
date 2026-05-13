use serde::{Deserialize, Serialize};

/// Broadcast state from daemon to overlay/clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonState {
    pub status: Status,
    pub meter: Meter,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Hidden,
    Preparing,
    Recording,
    Transcribing,
    Prewarming,
    Error,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Meter {
    pub average_power: f32,
    pub peak_power: f32,
}

/// Commands sent from CLI/overlay to daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    Press,
    Release,
    Cancel,
    Status,
    Ping,
}

/// Responses from daemon to CLI/overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    pub message: String,
    pub state: Option<DaemonState>,
}

impl Response {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            state: None,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            state: None,
        }
    }

    pub fn with_state(mut self, state: DaemonState) -> Self {
        self.state = Some(state);
        self
    }
}
