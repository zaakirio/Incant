mod audio;
mod config;
mod ipc;
mod output;
mod protocol;
mod sound;
mod state;
mod stt;

#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use protocol::{Command, Response, Status};
use sound::{Effect, SoundEffects};
use state::AppState;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

#[derive(Parser, Debug)]
#[command(name = "incant-daemon")]
#[command(about = "Voice dictation daemon for Omarchy")]
struct Cli {
    #[arg(short, long)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Run,
    DownloadModel,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = config::Config::load().context("loading config")?;

    if config.debug {
        tracing_subscriber::fmt()
            .with_env_filter("incant_daemon=debug")
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter("incant_daemon=info")
            .init();
    }

    match cli.command {
        Some(Commands::DownloadModel) => {
            info!("Downloading model...");
            stt::download_model(&config.cache_dir).await?;
            return Ok(());
        }
        _ => {}
    }

    info!("incant-daemon starting");
    info!("Config: {:?}", config);

    let app_state = AppState::default();

    // Sound effects.
    let sounds = SoundEffects::new(config.sound_volume);
    if sounds.is_none() {
        warn!("No audio output available; sound effects disabled");
    }

    // Ensure model is available.
    if !config.model_path.exists() {
        warn!("Model not found at {:?}, downloading...", config.model_path);
        stt::download_model(&config.cache_dir).await?;
    }

    // Initialize STT engine.
    let stt_engine = Arc::new(
        tokio::sync::Mutex::new(
            stt::SttEngine::new(&config).context("initializing STT engine")?,
        )
    );

    // State broadcast channel for overlay/clients.
    let (state_tx, _state_rx) = broadcast::channel::<protocol::DaemonState>(16);

    // Start IPC server.
    let ipc_server = ipc::IpcServer::bind(config.socket_path.clone())
        .await
        .context("binding IPC server")?;

    // Spawn overlay process.
    if config.show_overlay {
        let overlay_path = std::env::current_exe()?
            .parent()
            .unwrap()
            .join("incant-overlay");
        match tokio::process::Command::new(&overlay_path).spawn() {
            Ok(mut child) => {
                tokio::spawn(async move {
                    let _ = child.wait().await;
                });
                info!("Overlay spawned: {:?}", overlay_path);
            }
            Err(e) => {
                warn!("Failed to spawn overlay at {:?}: {}", overlay_path, e);
            }
        }
    }

    // Spawn IPC accept loop.
    let ipc_state = app_state.clone();
    let ipc_tx = state_tx.clone();
    let ipc_config = config.clone();
    tokio::spawn(async move {
        loop {
            match ipc_server.accept().await {
                Ok((stream, _addr)) => {
                    let state = ipc_state.clone();
                    let tx = ipc_tx.clone();
                    let cfg = ipc_config.clone();
                    tokio::spawn(async move {
                        if let Err(e) = ipc::handle_client(stream, state, cfg, tx).await {
                            warn!("IPC client handler error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("IPC accept error: {}", e);
                    break;
                }
            }
        }
    });

    // Spawn state broadcast loop (sends state to overlay ~30fps).
    let broadcast_state = app_state.clone();
    let broadcast_tx = state_tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(33));
        loop {
            interval.tick().await;
            let _ = broadcast_tx.send(broadcast_state.current());
        }
    });

    // Start audio capture stream.
    let _audio_stream = audio::start_capture(
        app_state.recording.clone(),
        app_state.audio_buffer.clone(),
        config.sample_rate,
    )
    .context("starting audio capture")?;
    info!("Audio capture started");

    // Spawn audio meter calculation loop.
    let meter_state = app_state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(33));
        loop {
            interval.tick().await;
            if meter_state.is_recording() {
                let buf = meter_state.audio_buffer.lock().unwrap().clone();
                let len = buf.len();
                if len > 0 {
                    let avg_power = buf.iter().map(|s| s * s).sum::<f32>() / len as f32;
                    let peak_power = buf.iter().map(|s| s * s).fold(0.0f32, f32::max);
                    meter_state.set_meter(avg_power.sqrt(), peak_power.sqrt());
                }
            }
        }
    });

    // Main state machine loop.
    let sm_state = app_state.clone();
    let sm_config = config.clone();
    let sm_engine = stt_engine.clone();
    tokio::spawn(async move {
        state_machine_loop(sm_state, sm_config, sm_engine, sounds).await;
    });

    info!("Daemon ready. Waiting for commands.");

    let mut sigterm = signal(SignalKind::terminate()).context("SIGTERM handler")?;
    let mut sigint = signal(SignalKind::interrupt()).context("SIGINT handler")?;

    tokio::select! {
        _ = sigterm.recv() => info!("Received SIGTERM, shutting down..."),
        _ = sigint.recv() => info!("Received SIGINT, shutting down..."),
    }

    if config.socket_path.exists() {
        let _ = std::fs::remove_file(&config.socket_path);
    }

    info!("Daemon stopped.");
    Ok(())
}

/// Press-and-hold press-and-hold state machine.
/// Waits for Press/Release/Cancel commands and manages the full workflow.
async fn state_machine_loop(
    state: AppState,
    config: config::Config,
    stt_engine: Arc<tokio::sync::Mutex<stt::SttEngine>>,
    sounds: Option<SoundEffects>,
) {
    use tokio::sync::mpsc;

    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<Command>();

    // Store the command sender in a static-ish way for IPC handlers to use.
    // We'll use a simple approach: IPC handlers directly call into this loop
    // by sending commands through the channel.
    //
    // For now, the IPC handler modifies AppState directly and this loop polls.
    // This is a simplified version; a full implementation would use the channel.

    let mut was_recording = false;
    let mut error_clear_time: Option<Instant> = None;

    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Auto-clear error state after 3 seconds.
        if let Some(clear_time) = error_clear_time {
            if clear_time.elapsed() > Duration::from_secs(3) {
                state.set_error(None);
                state.set_status(Status::Hidden);
                error_clear_time = None;
            }
        }

        let is_recording = state.is_recording();

        // Detect press transition: start -> recording.
        if !was_recording && is_recording {
            if let Some(ref s) = sounds { s.play(Effect::Start, config.sound_volume); }
        }

        // Detect release transition: recording -> stop.
        if was_recording && !is_recording {
            let duration = state.recording_start.lock().unwrap()
                .and_then(|s| Some(s.elapsed()))
                .unwrap_or(Duration::ZERO);

            // Clear recording_start so we don't re-process.
            *state.recording_start.lock().unwrap() = None;

            let min_duration = Duration::from_millis(config.minimum_key_time_ms);

            if duration < min_duration {
                info!("Recording too short ({:.3}s < {:.3}s), discarding", duration.as_secs_f32(), min_duration.as_secs_f32());
                state.set_status(Status::Hidden);
                state.clear_audio();
                if let Some(ref s) = sounds { s.play(Effect::Cancel, config.sound_volume); }
                was_recording = false;
                continue;
            }

            // Proceed to transcription.
            info!("Recording stopped, starting transcription...");
            state.set_status(Status::Transcribing);
            if let Some(ref s) = sounds { s.play(Effect::Stop, config.sound_volume); }

            let audio = state.take_audio();
            info!("Captured {} samples (~{:.1}s)", audio.len(), audio.len() as f32 / config.sample_rate as f32);

            if audio.is_empty() {
                warn!("No audio captured");
                state.set_status(Status::Hidden);
                was_recording = false;
                continue;
            }

            if config.debug {
                let debug_path = config.cache_dir.join("last_recording.wav");
                if let Err(e) = audio::save_wav(&debug_path, &audio, config.sample_rate) {
                    warn!("Failed to save debug WAV: {}", e);
                } else {
                    info!("Saved debug WAV to {:?}", debug_path);
                }
            }

            let result = {
                let mut engine = stt_engine.lock().await;
                engine.transcribe(&audio, config.sample_rate)
            };

            match result {
                Ok(text) => {
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        info!("Transcription: {}", text);
                        state.set_result(text.clone());

                        if let Err(e) = output::type_text(&text, &config.output_methods) {
                            error!("Failed to inject text: {}", e);
                            state.set_error(Some(format!("Paste failed: {}", e)));
                            state.set_status(Status::Error);
                            error_clear_time = Some(Instant::now());
                        } else {
                            if let Some(ref s) = sounds { s.play(Effect::Paste, config.sound_volume); }
                            state.set_status(Status::Hidden);
                        }
                    } else {
                        warn!("Transcription returned empty text");
                        state.set_status(Status::Hidden);
                    }
                }
                Err(e) => {
                    error!("Transcription failed: {}", e);
                    state.set_error(Some(format!("Transcription failed: {}", e)));
                    state.set_status(Status::Error);
                    error_clear_time = Some(Instant::now());
                }
            }
        }

        was_recording = is_recording;
    }
}
