use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;
use tracing::{info, warn};

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

        // Restrict the socket so only the owning user can connect.
        // On a single-user box this is moot; on a shared host it prevents
        // other local users from injecting keystrokes into our session.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            if let Err(e) = std::fs::set_permissions(&socket_path, perms) {
                warn!("Failed to chmod IPC socket {:?}: {}", socket_path, e);
            }
        }

        info!("IPC server listening on {:?}", socket_path);
        Ok(Self {
            listener,
            socket_path,
        })
    }

    pub async fn accept(&self) -> Result<(UnixStream, tokio::net::unix::SocketAddr)> {
        self.listener
            .accept()
            .await
            .context("accepting Unix socket connection")
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

pub(crate) async fn process_command(cmd: Command, state: &AppState, config: &Config) -> Response {
    match cmd {
        Command::Press => {
            // If already recording in locked mode, tap again to stop.
            if state.is_recording() && state.is_locked() {
                // Don't call stop_recording() here — it clears recording_start,
                // which breaks duration calculation in the state machine.
                state
                    .recording
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                state.set_locked(false);
                info!("Recording stopped (tap-to-stop in locked mode)");
                return Response::ok("recording stopped (locked)").with_state(state.current());
            }

            if state.is_recording() {
                return Response::err("already recording").with_state(state.current());
            }

            // Check for double-tap.
            let is_double_tap = if config.double_tap_lock_enabled || config.use_double_tap_only {
                state
                    .last_press_elapsed()
                    .map(|d| d.as_millis() < config.double_tap_window_ms as u128)
                    .unwrap_or(false)
            } else {
                false
            };

            // In double-tap-only mode, a lone press never starts recording.
            // We only record the timestamp so a second press within the window
            // can promote to locked recording. This prevents Alt+<anykey> combos
            // (Alt-Tab, Alt-F4, browser menu access, etc.) from triggering a
            // recording at all.
            if config.use_double_tap_only && !is_double_tap {
                state.record_press();
                info!("First tap registered (double-tap-only mode); waiting for second tap");
                return Response::ok("waiting for second tap").with_state(state.current());
            }

            state.record_press();
            state.start_recording();
            // Start in Preparing state. The state machine will promote to Recording
            // after minimum_key_time_ms, so quick taps (e.g. Alt-Tab) don't flash the HUD.
            state.set_status(Status::Preparing);

            // In double-tap-only mode, the second tap always enters locked mode
            // (since press-and-hold is disabled, there is no Release to stop us).
            if is_double_tap || config.use_double_tap_only {
                state.set_locked(true);
                info!("Recording started (double-tap lock)");
                Response::ok("recording started (locked)").with_state(state.current())
            } else {
                info!("Recording started (press)");
                Response::ok("recording started").with_state(state.current())
            }
        }
        Command::Release => {
            // In double-tap-only mode, releases are completely ignored —
            // recording is started and stopped exclusively by taps.
            if config.use_double_tap_only {
                return Response::ok("release ignored (double-tap-only mode)")
                    .with_state(state.current());
            }
            if !state.is_recording() {
                return Response::err("not recording").with_state(state.current());
            }
            // In locked mode, release does nothing.
            if state.is_locked() {
                return Response::ok("release ignored (locked mode)").with_state(state.current());
            }
            // Just set the flag; state machine will handle stop logic.
            state
                .recording
                .store(false, std::sync::atomic::Ordering::SeqCst);
            info!("Recording stopped (release)");
            Response::ok("recording stopped").with_state(state.current())
        }
        Command::Cancel => {
            let was_recording = state.is_recording();
            state
                .recording
                .store(false, std::sync::atomic::Ordering::SeqCst);
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
