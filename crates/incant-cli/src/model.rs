//! `incant model list` and `incant model use <name>` — the plug-and-play
//! surface for switching STT models. Reads the registry from `incant-daemon`
//! so there's a single source of truth for known models.

use anyhow::{Context, Result};
use incant_daemon::stt::{self, MODELS};
use std::path::PathBuf;

fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("~/.cache"))
        .join("incant")
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("incant/config.toml")
}

/// Print every known model with a "downloaded" and "active" marker.
pub fn list() -> Result<()> {
    let cache = cache_dir();
    let active_name = active_model_name().ok();

    println!();
    println!("  {:<10}  {:<10}  {:<10}  {}", "NAME", "STATUS", "ACTIVE", "DESCRIPTION");
    println!("  {:<10}  {:<10}  {:<10}  {}", "----", "------", "------", "-----------");

    for def in MODELS {
        let status = if stt::is_downloaded(&cache, def) {
            "✓ ready"
        } else {
            "—"
        };
        let active = match active_name.as_deref() {
            Some(name) if name == def.name => "● yes",
            _ => "",
        };
        println!(
            "  {:<10}  {:<10}  {:<10}  {}",
            def.name, status, active, def.description
        );
    }
    println!();
    println!("  Switch with: incant model use <name>");
    println!();
    Ok(())
}

/// Switch the configured model. Downloads the model if needed, then rewrites
/// `~/.config/incant/config.toml` to set `model = "<name>"`.
pub async fn use_model(name: &str) -> Result<()> {
    let def = stt::find_by_name(name).ok_or_else(|| {
        let known: Vec<&str> = MODELS.iter().map(|m| m.name).collect();
        anyhow::anyhow!("unknown model '{}'. Known: {}", name, known.join(", "))
    })?;

    let cache = cache_dir();
    if stt::is_downloaded(&cache, def) {
        println!("✓ {} already downloaded", def.name);
    } else {
        println!("Downloading {}...", def.name);
        stt::download_by_name(&cache, name).await?;
    }

    write_model_to_config(name)?;
    println!("✓ config updated → model = \"{}\"", name);
    println!();
    println!("  Restart the daemon to apply:");
    println!("    systemctl --user restart incant-daemon");
    println!();
    Ok(())
}

/// Resolve which model the daemon would load right now. Prefers the named
/// `model` field; falls back to matching `model_path`'s basename against the
/// registry.
fn active_model_name() -> Result<String> {
    let path = config_path();
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    let doc: toml_edit::DocumentMut = contents.parse().context("parsing config.toml")?;

    if let Some(s) = doc.get("model").and_then(|i| i.as_str()) {
        return Ok(s.to_string());
    }

    if let Some(s) = doc.get("model_path").and_then(|i| i.as_str()) {
        if let Some(basename) = std::path::Path::new(s).file_name().and_then(|n| n.to_str()) {
            if let Some(def) = MODELS.iter().find(|m| m.dir_name == basename) {
                return Ok(def.name.to_string());
            }
        }
    }

    anyhow::bail!("could not determine active model from config")
}

/// Edit `~/.config/incant/config.toml` in-place via `toml_edit` so comments
/// and field ordering are preserved. Sets `model = "<name>"` and removes any
/// stale `model_path` to avoid the two contradicting each other.
fn write_model_to_config(name: &str) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating config dir {}", parent.display()))?;
    }

    let contents = if path.exists() {
        std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?
    } else {
        String::new()
    };

    let mut doc: toml_edit::DocumentMut = contents.parse().context("parsing config.toml")?;
    doc["model"] = toml_edit::value(name);
    // model_path would override `model` confusion-wise — drop it so the named
    // field is the only source of truth going forward.
    doc.remove("model_path");

    std::fs::write(&path, doc.to_string())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
