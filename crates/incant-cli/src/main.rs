mod doctor;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(name = "incant")]
#[command(about = "Voice dictation client for Omarchy")]
struct Cli {
    #[arg(short, long)]
    socket: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start recording (press and hold).
    Press,
    /// Stop recording (release).
    Release,
    /// Cancel current recording/transcription.
    Cancel,
    /// Show daemon status.
    Status,
    /// Ping the daemon.
    Ping,
    /// Run diagnostic checks.
    Doctor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum IpcCommand {
    Press,
    Release,
    Cancel,
    Status,
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Meter {
    average_power: f32,
    peak_power: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DaemonState {
    status: String,
    meter: Meter,
    message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Response {
    ok: bool,
    message: String,
    state: Option<DaemonState>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    let socket_path = cli.socket.unwrap_or_else(|| {
        dirs::runtime_dir()
            .or_else(dirs::cache_dir)
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("incant/daemon.sock")
    });

    let cmd = match cli.command {
        Commands::Press => IpcCommand::Press,
        Commands::Release => IpcCommand::Release,
        Commands::Cancel => IpcCommand::Cancel,
        Commands::Status => IpcCommand::Status,
        Commands::Ping => IpcCommand::Ping,
        Commands::Doctor => return doctor::run().await,
    };

    let response = send_command(&socket_path, &cmd).await?;

    if response.ok {
        info!("OK: {}", response.message);
        if let Some(ref state) = response.state {
            println!("Status: {}", state.status);
            println!(
                "Meter: avg={:.3} peak={:.3}",
                state.meter.average_power, state.meter.peak_power
            );
            if let Some(ref msg) = state.message {
                println!("Message: {}", msg);
            }
        }
    } else {
        error!("Error: {}", response.message);
        std::process::exit(1);
    }

    Ok(())
}

async fn send_command(socket_path: &PathBuf, cmd: &IpcCommand) -> Result<Response> {
    let mut stream = UnixStream::connect(socket_path)
        .await
        .with_context(|| format!("connecting to daemon at {:?}", socket_path))?;

    let json = serde_json::to_string(cmd)?;
    stream.write_all(json.as_bytes()).await?;
    stream.write_all(b"\n").await?;

    let reader = BufReader::new(stream);
    let mut lines = reader.lines();

    let line = lines
        .next_line()
        .await?
        .context("daemon closed connection without response")?;

    let response: Response = serde_json::from_str(&line).context("parsing daemon response")?;
    Ok(response)
}
