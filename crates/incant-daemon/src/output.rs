use anyhow::{Context, Result};
use std::process::Command;
use tracing::{debug, error, info, warn};

/// Type text into the focused window using the best available method.
pub fn type_text(text: &str, methods: &[String]) -> Result<()> {
    for method in methods {
        match method.as_str() {
            "wtype" => match try_wtype(text) {
                Ok(()) => {
                    info!("Typed text via wtype");
                    return Ok(());
                }
                Err(e) => warn!("wtype failed: {}", e),
            },
            "dotool" => match try_dotool(text) {
                Ok(()) => {
                    info!("Typed text via dotool");
                    return Ok(());
                }
                Err(e) => warn!("dotool failed: {}", e),
            },
            "wl-copy" => match try_wl_copy(text) {
                Ok(()) => {
                    info!("Copied text to clipboard via wl-copy");
                    return Ok(());
                }
                Err(e) => warn!("wl-copy failed: {}", e),
            },
            other => warn!("Unknown output method: {}", other),
        }
    }

    anyhow::bail!("all output methods failed")
}

fn try_wtype(text: &str) -> Result<()> {
    // wtype does not handle newlines well; split and type line by line.
    let lines: Vec<&str> = text.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            // Insert Return key for newlines.
            Command::new("wtype")
                .arg("-k")
                .arg("Return")
                .status()
                .context("wtype Return key")?;
        }
        if !line.is_empty() {
            let status = Command::new("wtype")
                .arg(line)
                .status()
                .context("wtype command")?;
            if !status.success() {
                anyhow::bail!("wtype exited with code: {:?}", status.code());
            }
        }
    }
    Ok(())
}

fn try_dotool(text: &str) -> Result<()> {
    let mut child = Command::new("dotool")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("spawning dotool")?;

    let stdin = child
        .stdin
        .take()
        .context("getting dotool stdin")?;

    let escaped = text.replace('"', "\\\"");
    let cmd = format!("type \"{}\"\n", escaped);

    std::thread::spawn(move || {
        use std::io::Write;
        let mut stdin = stdin;
        let _ = stdin.write_all(cmd.as_bytes());
    });

    let status = child.wait().context("waiting for dotool")?;
    if !status.success() {
        anyhow::bail!("dotool exited with code: {:?}", status.code());
    }
    Ok(())
}

fn try_wl_copy(text: &str) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("spawning wl-copy")?;

    let stdin = child
        .stdin
        .take()
        .context("getting wl-copy stdin")?;

    let text = text.to_string();
    std::thread::spawn(move || {
        use std::io::Write;
        let mut stdin = stdin;
        let _ = stdin.write_all(text.as_bytes());
    });

    let status = child.wait().context("waiting for wl-copy")?;
    if !status.success() {
        anyhow::bail!("wl-copy exited with code: {:?}", status.code());
    }
    Ok(())
}
