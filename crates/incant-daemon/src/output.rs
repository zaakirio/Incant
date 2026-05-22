use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;
use tracing::{info, warn};

/// Type text into the focused window using the best available method.
pub fn type_text(text: &str, methods: &[String]) -> Result<()> {
    for method in methods {
        match method.as_str() {
            // Hex-style clipboard paste: O(1) regardless of length. Works in
            // every Wayland app that supports Ctrl+V paste (the overwhelming
            // majority). For long transcriptions this is what makes the text
            // land in a single frame instead of being typed character by
            // character.
            "wl-clipboard-paste" => match try_wl_clipboard_paste(text) {
                Ok(()) => {
                    info!("Pasted text via wl-copy + Ctrl+V");
                    return Ok(());
                }
                Err(e) => warn!("wl-clipboard-paste failed: {}", e),
            },
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

/// Clipboard-paste injection: snapshot clipboard → wl-copy text → send Ctrl+V
/// via wtype → restore the previous clipboard after a short grace period.
///
/// This is the same trick Hex uses on macOS (PasteboardClient.pasteWithClipboard).
/// It collapses paste latency from O(text length) keystrokes to a single
/// Ctrl+V regardless of how long the transcription is.
fn try_wl_clipboard_paste(text: &str) -> Result<()> {
    // Snapshot the current clipboard so we can restore it. Best-effort: if
    // wl-paste fails (empty clipboard, MIME mismatch, etc.) we just skip
    // restoration rather than aborting the whole paste.
    let prior = snapshot_clipboard().ok();

    // 1. Put the transcription on the clipboard.
    write_clipboard(text).context("wl-copy of transcription")?;

    // 2. Give the compositor a beat to propagate the new clipboard offer to
    //    the focused client before we fire the paste shortcut. Without this,
    //    apps sometimes paste the *previous* clipboard contents (or nothing).
    //    Hex polls the macOS pasteboard's changeCount for up to 150 ms; on
    //    Wayland we can't poll the equivalent, so a small fixed delay is the
    //    pragmatic equivalent. 40 ms is well below human-perceptible.
    std::thread::sleep(Duration::from_millis(40));

    // 3. Send Ctrl+V as a chord. wtype's modifier-held semantics apply to
    //    *positional text*, not to `-k <keysym>` taps — using `-k v` releases
    //    the modifier too early on some wtype builds, which is why the user
    //    sees a bare 'v' (or nothing) rather than a paste. The canonical
    //    recipe from wtype's docs is: -M MOD <text> -m MOD.
    let status = Command::new("wtype")
        .args(["-M", "ctrl", "v", "-m", "ctrl"])
        .status()
        .context("spawning wtype for Ctrl+V")?;
    if !status.success() {
        anyhow::bail!("wtype Ctrl+V exited with code: {:?}", status.code());
    }

    // 3. Restore the prior clipboard contents after a short delay so the
    //    target app has time to actually read the paste data. Run this in
    //    a detached thread — we don't want the daemon's IPC loop blocked
    //    waiting on it.
    if let Some(prior_bytes) = prior {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            let _ = write_clipboard_bytes(&prior_bytes);
        });
    }

    Ok(())
}

fn snapshot_clipboard() -> Result<Vec<u8>> {
    let output = Command::new("wl-paste")
        .arg("--no-newline")
        .output()
        .context("spawning wl-paste")?;
    if !output.status.success() {
        anyhow::bail!("wl-paste exited with code: {:?}", output.status.code());
    }
    Ok(output.stdout)
}

fn write_clipboard(text: &str) -> Result<()> {
    write_clipboard_bytes(text.as_bytes())
}

fn write_clipboard_bytes(bytes: &[u8]) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .spawn()
        .context("spawning wl-copy")?;
    {
        let stdin = child.stdin.as_mut().context("getting wl-copy stdin")?;
        stdin.write_all(bytes).context("writing to wl-copy stdin")?;
    }
    let status = child.wait().context("waiting for wl-copy")?;
    if !status.success() {
        anyhow::bail!("wl-copy exited with code: {:?}", status.code());
    }
    Ok(())
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

    let stdin = child.stdin.take().context("getting dotool stdin")?;

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

    let stdin = child.stdin.take().context("getting wl-copy stdin")?;

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
