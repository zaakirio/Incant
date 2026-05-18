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
use protocol::Status;
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
#[command(version)]
#[command(about = "Voice dictation daemon for Hyprland / Wayland")]
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

    if let Some(Commands::DownloadModel) = cli.command {
        info!("Downloading model...");
        stt::download_model(&config.cache_dir, &config.model_path).await?;
        return Ok(());
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
        stt::download_model(&config.cache_dir, &config.model_path).await?;
    }

    // Initialize STT engine.
    let stt_engine = Arc::new(std::sync::Mutex::new(
        stt::SttEngine::new(&config).context("initializing STT engine")?,
    ));

    // State broadcast channel for overlay/clients.
    let (state_tx, _state_rx) = broadcast::channel::<protocol::DaemonState>(16);

    // Start IPC server.
    let ipc_server = ipc::IpcServer::bind(config.socket_path.clone())
        .await
        .context("binding IPC server")?;

    // Spawn overlay process.
    let mut overlay_child: Option<tokio::process::Child> = None;
    if config.show_overlay {
        match find_overlay_binary() {
            Some(overlay_path) => match tokio::process::Command::new(&overlay_path).spawn() {
                Ok(child) => {
                    info!("Overlay spawned: {:?}", overlay_path);
                    overlay_child = Some(child);
                }
                Err(e) => {
                    warn!("Failed to spawn overlay at {:?}: {}", overlay_path, e);
                }
            },
            None => {
                warn!("incant-overlay not found in PATH or next to incant-daemon");
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

    // Channel from audio callback (real-time thread) → async consumer.
    let (audio_tx, mut audio_rx) = tokio::sync::mpsc::channel::<Vec<f32>>(1024);

    // Start audio capture stream.
    let (_audio_stream, native_sample_rate) =
        audio::start_capture(app_state.recording.clone(), audio_tx, config.sample_rate)
            .context("starting audio capture")?;
    info!("Audio capture started (native {} Hz)", native_sample_rate);

    // Drain audio chunks from the real-time callback into AppState.
    let collect_state = app_state.clone();
    tokio::spawn(async move {
        while let Some(chunk) = audio_rx.recv().await {
            collect_state.append_audio(&chunk);
        }
    });

    // Spawn audio meter calculation loop.
    //
    // We compute RMS + peak over only the most recent ~100 ms of samples so the
    // meter stays lively for the entire recording. Averaging over the full
    // rolling buffer makes the values flatten out within a few seconds, which
    // produces a "static" looking bar.
    let meter_state = app_state.clone();
    let meter_window_samples = (native_sample_rate as usize / 10).max(1); // ~100 ms
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(33));
        loop {
            interval.tick().await;
            if meter_state.is_recording() {
                let buf = meter_state.audio_buffer.lock().unwrap();
                let len = buf.len();
                if len > 0 {
                    let start = len.saturating_sub(meter_window_samples);
                    let window = &buf[start..];
                    let wlen = window.len();
                    let sum_sq: f32 = window.iter().map(|s| s * s).sum();
                    let peak_sq = window.iter().map(|s| s * s).fold(0.0f32, f32::max);
                    let avg_power = (sum_sq / wlen as f32).sqrt();
                    let peak_power = peak_sq.sqrt();
                    drop(buf);
                    meter_state.set_meter(avg_power, peak_power);
                }
            }
        }
    });

    // Main state machine loop.
    let sm_state = app_state.clone();
    let sm_config = config.clone();
    let sm_engine = stt_engine.clone();
    tokio::spawn(async move {
        state_machine_loop(sm_state, sm_config, sm_engine, sounds, native_sample_rate).await;
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

    // Kill overlay child so it doesn't linger as an orphan.
    if let Some(mut child) = overlay_child {
        info!("Terminating overlay...");
        let _ = child.start_kill();
        let _ = child.wait().await;
    }

    info!("Daemon stopped.");
    Ok(())
}

/// Find the incant-overlay binary: first check next to the daemon,
/// then search PATH.
fn find_overlay_binary() -> Option<std::path::PathBuf> {
    // 1. Check next to current executable.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("incant-overlay");
            if sibling.exists() {
                return Some(sibling);
            }
        }
    }

    // 2. Search PATH.
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            let candidate = std::path::Path::new(dir).join("incant-overlay");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

/// Press-and-hold press-and-hold state machine.
/// Waits for Press/Release/Cancel commands and manages the full workflow.
async fn state_machine_loop(
    state: AppState,
    config: config::Config,
    stt_engine: Arc<std::sync::Mutex<stt::SttEngine>>,
    sounds: Option<SoundEffects>,
    native_sample_rate: u32,
) {
    // The IPC handler modifies AppState directly; this loop polls the state
    // and drives the audio capture / STT / injection pipeline accordingly.
    // (A future refactor could replace this with a tokio::sync::mpsc channel
    // carrying typed commands, but the polled-state approach is sufficient
    // given how rarely state transitions actually fire.)

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
        let current_status = *state.status.lock().unwrap();

        // Promote Preparing -> Recording after the minimum hold time,
        // but only if there's actual audio energy (or we've waited long enough).
        // This prevents modifier+key combos (Alt-Tab, etc.) from flashing the HUD
        // when no speech is present.
        if current_status == Status::Preparing && is_recording {
            let elapsed = state
                .recording_start
                .lock()
                .unwrap()
                .map(|s| s.elapsed())
                .unwrap_or(Duration::ZERO);
            let min_duration = Duration::from_millis(config.minimum_key_time_ms);
            let max_prepare = Duration::from_millis(config.max_preparing_duration_ms);

            if elapsed >= min_duration {
                let meter = state.meter.lock().unwrap();
                let has_audio = meter.peak_power >= config.promotion_peak_threshold;
                let maxed_out = elapsed >= max_prepare;

                if has_audio || maxed_out {
                    state.set_status(Status::Recording);
                    if has_audio {
                        info!(
                            "Promoted to Recording after {:.3}s hold (peak={:.3})",
                            elapsed.as_secs_f32(),
                            meter.peak_power
                        );
                    } else {
                        info!(
                            "Promoted to Recording after {:.3}s hold (max preparing duration reached)",
                            elapsed.as_secs_f32()
                        );
                    }
                    if let Some(ref s) = sounds {
                        s.play(Effect::Start, config.sound_volume);
                    }
                }
            }
        }

        // Detect release transition: recording -> stop.
        if was_recording && !is_recording {
            let duration = state
                .recording_start
                .lock()
                .unwrap()
                .and_then(|s| Some(s.elapsed()))
                .unwrap_or(Duration::ZERO);

            // Clear recording_start so we don't re-process.
            *state.recording_start.lock().unwrap() = None;

            let min_duration = Duration::from_millis(config.minimum_key_time_ms);

            if duration < min_duration {
                // Quick tap: silently discard without HUD flash or cancel sound.
                info!(
                    "Quick tap discarded ({:.3}s < {:.3}s)",
                    duration.as_secs_f32(),
                    min_duration.as_secs_f32()
                );
                state.set_status(Status::Hidden);
                state.clear_audio();
                was_recording = false;
                continue;
            }

            // Proceed to transcription.
            info!("Recording stopped, starting transcription...");
            state.set_status(Status::Transcribing);
            if let Some(ref s) = sounds {
                s.play(Effect::Stop, config.sound_volume);
            }

            let audio = state.take_audio();
            info!(
                "Captured {} samples native @ {} Hz (~{:.1}s)",
                audio.len(),
                native_sample_rate,
                audio.len() as f32 / native_sample_rate as f32
            );

            if audio.is_empty() {
                warn!("No audio captured");
                state.set_status(Status::Hidden);
                was_recording = false;
                continue;
            }

            // Resample if the capture device runs at a different rate than the model expects.
            let audio = if native_sample_rate != config.sample_rate {
                match audio::resample_once(&audio, native_sample_rate, config.sample_rate) {
                    Ok(resampled) => resampled,
                    Err(e) => {
                        error!("Resampling failed: {}", e);
                        state.set_error(Some(format!("Resampling failed: {}", e)));
                        state.set_status(Status::Error);
                        error_clear_time = Some(Instant::now());
                        was_recording = false;
                        continue;
                    }
                }
            } else {
                audio
            };

            if config.debug {
                let debug_path = config.cache_dir.join("last_recording.wav");
                if let Err(e) = audio::save_wav(&debug_path, &audio, config.sample_rate) {
                    warn!("Failed to save debug WAV: {}", e);
                } else {
                    info!("Saved debug WAV to {:?}", debug_path);
                }
            }

            let engine = stt_engine.clone();
            let sample_rate = config.sample_rate;
            let audio_clone = audio.clone();
            let result = tokio::task::spawn_blocking(move || {
                let mut engine = engine.lock().unwrap();
                engine.transcribe(&audio_clone, sample_rate)
            })
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("transcription task panicked: {}", e)));

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
                            if let Some(ref s) = sounds {
                                s.play(Effect::Paste, config.sound_volume);
                            }
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
