use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckResult {
    Pass,
    Warn,
    Fail,
}

struct Check {
    name: String,
    result: CheckResult,
    message: String,
    fix: Option<String>,
}

pub async fn run() -> Result<()> {
    println!("\n  🔮 Incant Diagnostic\n");

    let mut checks: Vec<Check> = Vec::new();

    // ── Environment ──
    checks.push(check_wayland());
    checks.push(check_hyprland());

    // ── Audio subsystem ──
    checks.push(check_pipewire());
    checks.push(check_microphone());

    // ── Dependencies ──
    checks.push(check_binary("wtype", true));
    checks.push(check_binary("dotool", false));
    checks.push(check_binary("wl-copy", false));
    checks.push(check_binary("incant-daemon", true));
    checks.push(check_binary("incant-overlay", true));
    checks.push(check_sherpa_libs());

    // ── Model ──
    checks.push(check_model());

    // ── Runtime paths ──
    checks.push(check_socket_path());

    // ── Daemon health ──
    checks.push(check_daemon_reachable().await);

    let _pass = checks
        .iter()
        .filter(|c| c.result == CheckResult::Pass)
        .count();
    let warn = checks
        .iter()
        .filter(|c| c.result == CheckResult::Warn)
        .count();
    let fail = checks
        .iter()
        .filter(|c| c.result == CheckResult::Fail)
        .count();

    for check in &checks {
        print_check(check);
    }

    println!();
    match (fail, warn) {
        (0, 0) => println!("  ✅ All checks passed. Incant is ready!\n"),
        (0, w) => {
            println!("  ⚠️  {w} warning(s). Incant will work, but some features may be limited.\n")
        }
        (f, _) => {
            println!("  ❌ {f} check(s) failed. Please fix the issues above and run again.\n")
        }
    }

    if fail > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn check_wayland() -> Check {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        Check {
            name: "Wayland display".into(),
            result: CheckResult::Pass,
            message: std::env::var("WAYLAND_DISPLAY").unwrap_or_default(),
            fix: None,
        }
    } else {
        Check {
            name: "Wayland display".into(),
            result: CheckResult::Fail,
            message: "WAYLAND_DISPLAY not set".into(),
            fix: Some("Incant requires a Wayland session.".into()),
        }
    }
}

fn check_hyprland() -> Check {
    if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
        Check {
            name: "Window manager".into(),
            result: CheckResult::Pass,
            message: "Hyprland detected".into(),
            fix: None,
        }
    } else {
        Check {
            name: "Window manager".into(),
            result: CheckResult::Warn,
            message: "Hyprland not detected".into(),
            fix: Some(
                "Incant is designed for Hyprland. Other compositors may work partially.".into(),
            ),
        }
    }
}

fn check_pipewire() -> Check {
    let ok = std::process::Command::new("pactl")
        .arg("info")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if ok {
        Check {
            name: "PipeWire / PulseAudio".into(),
            result: CheckResult::Pass,
            message: "Audio server reachable".into(),
            fix: None,
        }
    } else {
        Check {
            name: "PipeWire / PulseAudio".into(),
            result: CheckResult::Fail,
            message: "pactl info failed — is PipeWire running?".into(),
            fix: Some("Start PipeWire: systemctl --user start pipewire pipewire-pulse".into()),
        }
    }
}

fn check_microphone() -> Check {
    let output = std::process::Command::new("pactl")
        .args(["list", "sources", "short"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let sources: Vec<_> = stdout.lines().filter(|l| !l.is_empty()).collect();
            if sources.iter().any(|l| l.contains("input")) {
                Check {
                    name: "Microphone input".into(),
                    result: CheckResult::Pass,
                    message: format!("{} source(s) found", sources.len()),
                    fix: None,
                }
            } else {
                Check {
                    name: "Microphone input".into(),
                    result: CheckResult::Warn,
                    message: "No input sources detected".into(),
                    fix: Some("Check that a microphone is connected and not muted.".into()),
                }
            }
        }
        _ => Check {
            name: "Microphone input".into(),
            result: CheckResult::Warn,
            message: "Could not enumerate audio sources".into(),
            fix: Some("Ensure PipeWire is running.".into()),
        },
    }
}

fn check_binary(name: &str, required: bool) -> Check {
    let found = which::which(name).is_ok();
    if found {
        Check {
            name: name.to_string(),
            result: CheckResult::Pass,
            message: "found in PATH".into(),
            fix: None,
        }
    } else if required {
        Check {
            name: name.to_string(),
            result: CheckResult::Fail,
            message: "not found in PATH".into(),
            fix: Some(format!("Install {name} (see README for package name).")),
        }
    } else {
        Check {
            name: name.to_string(),
            result: CheckResult::Warn,
            message: "not found in PATH (optional)".into(),
            fix: Some(format!("Install {name} if you want this fallback.")),
        }
    }
}

fn check_model() -> Check {
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("~/.cache"))
        .join("incant/models");

    let candidates = [
        "parakeet-tdt-0.6b-v3-int8",
        "parakeet-tdt-0.6b-v2-int8",
        "moonshine-tiny-en-int8",
    ];

    for &cand in &candidates {
        let path = cache_dir.join(cand);
        if path.is_dir()
            && path
                .read_dir()
                .map(|mut d| d.next().is_some())
                .unwrap_or(false)
        {
            return Check {
                name: "STT model".into(),
                result: CheckResult::Pass,
                message: format!("found at ~/.cache/incant/models/{cand}"),
                fix: None,
            };
        }
    }

    Check {
        name: "STT model".into(),
        result: CheckResult::Fail,
        message: "No model found in ~/.cache/incant/models".into(),
        fix: Some("Run: incant-daemon download-model".into()),
    }
}

fn check_sherpa_libs() -> Check {
    // 1. Check /usr/lib/incant
    let sys_path = std::path::Path::new("/usr/lib/incant/libsherpa-onnx-c-api.so");
    if sys_path.exists() {
        return Check {
            name: "Sherpa-ONNX libraries".into(),
            result: CheckResult::Pass,
            message: "found in /usr/lib/incant".into(),
            fix: None,
        };
    }

    // 2. Check if ldconfig knows about it
    let ldconfig_ok = std::process::Command::new("ldconfig")
        .arg("-p")
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.contains("sherpa-onnx"))
        })
        .unwrap_or(false);

    if ldconfig_ok {
        return Check {
            name: "Sherpa-ONNX libraries".into(),
            result: CheckResult::Pass,
            message: "found via ldconfig".into(),
            fix: None,
        };
    }

    // 3. Check ~/.cache/sherpa-rs (dev build)
    let cache_lib = dirs::cache_dir()
        .map(|d| d.join("sherpa-rs"))
        .and_then(|d| {
            std::fs::read_dir(d).ok().and_then(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .find(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                    .and_then(|e| {
                        let cand = e.path().join("lib/libsherpa-onnx-c-api.so");
                        cand.exists().then_some(cand)
                    })
            })
        });

    if let Some(lib) = cache_lib {
        return Check {
            name: "Sherpa-ONNX libraries".into(),
            result: CheckResult::Warn,
            message: format!(
                "found at {} — set LD_LIBRARY_PATH",
                lib.parent().unwrap().display()
            ),
            fix: Some("Run with: LD_LIBRARY_PATH=~/.cache/sherpa-rs/.../lib incant-daemon".into()),
        };
    }

    Check {
        name: "Sherpa-ONNX libraries".into(),
        result: CheckResult::Fail,
        message: "libsherpa-onnx-c-api.so not found".into(),
        fix: Some("Re-run ./install.sh or set LD_LIBRARY_PATH to the sherpa-rs lib dir.".into()),
    }
}

fn check_socket_path() -> Check {
    let socket_dir = dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("incant");

    if !socket_dir.exists() && std::fs::create_dir_all(&socket_dir).is_ok() {
        return Check {
            name: "Socket directory".into(),
            result: CheckResult::Pass,
            message: format!("writable at {}", socket_dir.display()),
            fix: None,
        };
    }

    let test = socket_dir.join(".write_test");
    match std::fs::File::create(&test) {
        Ok(_) => {
            let _ = std::fs::remove_file(&test);
            Check {
                name: "Socket directory".into(),
                result: CheckResult::Pass,
                message: format!("writable at {}", socket_dir.display()),
                fix: None,
            }
        }
        Err(e) => Check {
            name: "Socket directory".into(),
            result: CheckResult::Fail,
            message: format!("not writable: {e}"),
            fix: Some(format!("Fix permissions on {}", socket_dir.display())),
        },
    }
}

async fn check_daemon_reachable() -> Check {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let socket_path = dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("incant/daemon.sock");

    let mut stream = match UnixStream::connect(&socket_path).await {
        Ok(s) => s,
        Err(e) => {
            return Check {
                name: "Daemon health".into(),
                result: CheckResult::Fail,
                message: format!("Cannot connect to socket: {e}"),
                fix: Some("Start the daemon: systemctl --user start incant-daemon".into()),
            };
        }
    };

    let ping = r#"{"cmd":"ping"}"#;
    if stream.write_all(ping.as_bytes()).await.is_err() || stream.write_all(b"\n").await.is_err() {
        return Check {
            name: "Daemon health".into(),
            result: CheckResult::Fail,
            message: "Write to socket failed".into(),
            fix: Some("Restart the daemon.".into()),
        };
    }

    let reader = BufReader::new(stream);
    let mut lines = reader.lines();
    match lines.next_line().await {
        Ok(Some(line)) => {
            if line.contains("pong") || line.contains("\"ok\":true") {
                Check {
                    name: "Daemon health".into(),
                    result: CheckResult::Pass,
                    message: "Daemon responded to ping".into(),
                    fix: None,
                }
            } else {
                Check {
                    name: "Daemon health".into(),
                    result: CheckResult::Warn,
                    message: "Unexpected daemon response".into(),
                    fix: Some("Daemon may be starting up. Wait a few seconds.".into()),
                }
            }
        }
        _ => Check {
            name: "Daemon health".into(),
            result: CheckResult::Fail,
            message: "No response from daemon".into(),
            fix: Some("Check daemon logs: journalctl --user -u incant-daemon".into()),
        },
    }
}

fn print_check(check: &Check) {
    let (icon, color) = match check.result {
        CheckResult::Pass => ("✅", "\x1b[32m"),
        CheckResult::Warn => ("⚠️ ", "\x1b[33m"),
        CheckResult::Fail => ("❌", "\x1b[31m"),
    };
    let reset = "\x1b[0m";

    println!(
        "  {color}{icon}{reset}  {:<28} {}",
        check.name.as_str(),
        check.message
    );
    if let Some(fix) = &check.fix {
        println!("        {color}→{reset} {fix}");
    }
}
