use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::protocol::{Command, Response, Status};
use crate::state::AppState;

pub struct IpcServer {
    listener: UnixListener,
    socket_path: PathBuf,
}

impl IpcServer {
    pub async fn bind(socket_path: PathBuf) -> Result<Self> {
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)
                .with_context(|| format!("removing stale socket {:?}", socket_path))?;
        }
        std::fs::create_dir_all(socket_path.parent().unwrap_or(std::path::Path::new("/tmp")))
            .context("creating socket directory")?;
        let listener = UnixListener::bind(&socket_path)
            .with_context(|| format!("binding Unix socket at {:?}", socket_path))?;
        info!("IPC server listening on {:?}", socket_path);
        Ok(Self { listener, socket_path })
    }

    pub async fn accept(&self) -> Result<(UnixStream, tokio::net::unix::SocketAddr)> {
        self.listener.accept().await.context("accepting Unix socket connection")
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        if self.socket_path.exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }
    }
}

pub async fn handle_client(
    mut stream: UnixStream,
    state: AppState,
    config: Config,
    _state_tx: broadcast::Sender<crate::protocol::DaemonState>,
) -> Result<()> {
    let (read_half, mut write_half) = stream.split();
    let reader = BufReader::new(read_half);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Command>(line) {
            Ok(cmd) => process_command(cmd, &state, &config).await,
            Err(e) => Response::err(format!("invalid command: {}", e)).with_state(state.current()),
        };

        let json = serde_json::to_string(&response)?;
        if let Err(e) = write_half.write_all(json.as_bytes()).await {
            warn!("Failed to write IPC response: {}", e);
            break;
        }
        if let Err(e) = write_half.write_all(b"\n").await {
            warn!("Failed to write newline: {}", e);
            break;
        }
    }

    Ok(())
}

async fn process_command(cmd: Command, state: &AppState, config: &Config) -> Response {
    match cmd {
        Command::Press => {
            // If already recording in locked mode, tap again to stop.
            if state.is_recording() && state.is_locked() {
                state.stop_recording();
                info!("Recording stopped (tap-to-stop in locked mode)");
                return Response::ok("recording stopped (locked)").with_state(state.current());
            }

            if state.is_recording() {
                return Response::err("already recording").with_state(state.current());
            }

            // Check for double-tap.
            let is_double_tap = if config.double_tap_lock_enabled {
                state.last_press_elapsed()
                    .map(|d| d.as_millis() < config.double_tap_window_ms as u128)
                    .unwrap_or(false)
            } else {
                false
            };

            state.record_press();
            state.start_recording();
            state.set_status(Status::Recording);

            if is_double_tap {
                state.set_locked(true);
                info!("Recording started (double-tap lock)");
                Response::ok("recording started (locked)").with_state(state.current())
            } else {
                info!("Recording started (press)");
                Response::ok("recording started").with_state(state.current())
            }
        }
        Command::Release => {
            if !state.is_recording() {
                return Response::err("not recording").with_state(state.current());
            }
            // In locked mode, release does nothing.
            if state.is_locked() {
                return Response::ok("release ignored (locked mode)").with_state(state.current());
            }
            // Just set the flag; state machine will handle stop logic.
            state.recording.store(false, std::sync::atomic::Ordering::SeqCst);
            info!("Recording stopped (release)");
            Response::ok("recording stopped").with_state(state.current())
        }
        Command::Cancel => {
            let was_recording = state.is_recording();
            state.recording.store(false, std::sync::atomic::Ordering::SeqCst);
            state.clear_audio();
            state.reset_meter();
            state.set_status(Status::Hidden);
            if was_recording {
                info!("Recording cancelled");
            }
            Response::ok("cancelled").with_state(state.current())
        }
        Command::Status => Response::ok("status").with_state(state.current()),
        Command::Ping => Response::ok("pong").with_state(state.current()),
    }
}
