use gtk4::prelude::*;
use gtk4_layer_shell::{Layer, LayerShell};
use serde::Deserialize;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
struct Meter {
    average_power: f32,
    peak_power: f32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Status {
    Hidden,
    Preparing,
    Recording,
    Transcribing,
    Prewarming,
    Error,
}

#[derive(Debug, Clone, Deserialize)]
struct DaemonState {
    status: Status,
    meter: Meter,
    message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Response {
    // `ok` and `message` are deserialized for completeness but the overlay
    // only consumes the `state` payload; keep them present so the wire
    // format stays in sync with `incant-daemon::protocol::Response`.
    #[allow(dead_code)]
    ok: bool,
    #[allow(dead_code)]
    message: String,
    state: Option<DaemonState>,
}

fn main() {
    let app = gtk4::Application::new(Some("org.omarchy.incant.overlay"), Default::default());

    app.connect_activate(|app| {
        let window = gtk4::ApplicationWindow::new(app);
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        // Anchor to the top edge so the capsule sits just below the status bar
        // rather than dead-center on the screen. We span the full width and
        // center the capsule horizontally inside the layer-shell surface.
        window.set_anchor(gtk4_layer_shell::Edge::Top, true);
        window.set_anchor(gtk4_layer_shell::Edge::Left, true);
        window.set_anchor(gtk4_layer_shell::Edge::Right, true);
        // Small top margin to clear a typical Waybar (~32 px) with a little breathing room.
        window.set_margin(gtk4_layer_shell::Edge::Top, 44);
        window.set_default_size(400, 40);
        window.set_decorated(false);

        // Capsule container
        let capsule = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        capsule.set_halign(gtk4::Align::Center);
        capsule.set_valign(gtk4::Align::Start);
        capsule.set_widget_name("incant-capsule");

        // Inner glow / meter bar
        let meter_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        meter_bar.set_widget_name("incant-meter");
        meter_bar.set_hexpand(false);

        capsule.append(&meter_bar);
        window.set_child(Some(&capsule));

        // CSS provider — make window background transparent so we never paint a grey screen.
        // `load_from_data` (not `load_from_string`) keeps us compatible with the GTK 4.6
        // available on Ubuntu 22.04 LTS, which is what our release pipeline builds against.
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(CSS);
        gtk4::style_context_add_provider_for_display(
            &gtk4::gdk::Display::default().expect("no display"),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        window.present();

        // Shared state
        let state: Arc<Mutex<DaemonState>> = Arc::new(Mutex::new(DaemonState {
            status: Status::Hidden,
            meter: Meter {
                average_power: 0.0,
                peak_power: 0.0,
            },
            message: None,
        }));

        // IPC listener — exits if daemon is gone for too long.
        let state_clone = state.clone();
        std::thread::spawn(move || {
            ipc_listener(state_clone);
        });

        // Animation loop ~30fps
        let state_clone = state.clone();
        glib::source::timeout_add_local(Duration::from_millis(33), move || {
            let daemon_state = state_clone.lock().unwrap().clone();
            update_ui(&capsule, &meter_bar, &daemon_state);
            glib::ControlFlow::Continue
        });
    });

    app.run();
}

fn update_ui(capsule: &gtk4::Box, meter_bar: &gtk4::Box, state: &DaemonState) {
    // Remove all status classes (GTK4: `add_css_class`/`remove_css_class`
    // directly on the widget; `style_context()` was deprecated in 4.10).
    for class in &[
        "hidden",
        "preparing",
        "recording",
        "transcribing",
        "prewarming",
        "error",
    ] {
        capsule.remove_css_class(class);
    }

    // Clean up any previous error label to avoid leaking widgets
    let mut child = capsule.first_child();
    while let Some(c) = child {
        let next = c.next_sibling();
        if c.widget_name() == "incant-error-label" {
            capsule.remove(&c);
        }
        child = next;
    }

    match state.status {
        Status::Hidden => {
            capsule.add_css_class("hidden");
        }
        Status::Preparing => {
            capsule.add_css_class("preparing");
        }
        Status::Recording => {
            capsule.add_css_class("recording");
            // Speech RMS / peak typically sits in 0.02–0.3, so scale the
            // signal aggressively (and gamma-curve it) for a lively meter.
            let avg = (state.meter.average_power * 4.0).clamp(0.0, 1.0).powf(0.6);
            let peak = (state.meter.peak_power * 2.5).clamp(0.0, 1.0).powf(0.6);
            let width = 16.0 + (peak * 80.0); // 16px to 96px
            meter_bar.set_size_request(width as i32, -1);
            // Keep the bar visible at all times, just modulate its opacity.
            meter_bar.set_opacity((0.35 + 0.65 * avg as f64).clamp(0.35, 1.0));
        }
        Status::Transcribing => {
            capsule.add_css_class("transcribing");
        }
        Status::Prewarming => {
            capsule.add_css_class("prewarming");
        }
        Status::Error => {
            capsule.add_css_class("error");
            if let Some(ref msg) = state.message {
                let label = gtk4::Label::new(Some(msg));
                label.set_widget_name("incant-error-label");
                label.set_halign(gtk4::Align::Center);
                label.set_valign(gtk4::Align::Center);
                capsule.append(&label);
            }
        }
    }
}

fn ipc_listener(state: Arc<Mutex<DaemonState>>) {
    let socket_path = dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("incant/daemon.sock");

    let mut consecutive_failures = 0;
    let status_cmd = b"{\"cmd\":\"status\"}\n";

    loop {
        if let Ok(mut stream) = UnixStream::connect(&socket_path) {
            consecutive_failures = 0;
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();

            loop {
                // Poll the daemon for current state
                if stream.write_all(status_cmd).is_err() {
                    break; // Connection broken
                }
                if stream.flush().is_err() {
                    break;
                }

                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if let Ok(response) = serde_json::from_str::<Response>(&line) {
                            if let Some(daemon_state) = response.state {
                                *state.lock().unwrap() = daemon_state;
                            }
                        }
                    }
                    Err(_) => break,
                }

                std::thread::sleep(Duration::from_millis(50));
            }
            // Connection dropped — daemon may have restarted.
        }

        consecutive_failures += 1;
        if consecutive_failures > 10 {
            eprintln!("incant-overlay: daemon unreachable for 5s, exiting.");
            std::process::exit(0);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

const CSS: &str = r#"
window {
    background: transparent;
}

#incant-capsule {
    min-width: 16px;
    min-height: 16px;
    border-radius: 8px;
    transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
    opacity: 0;
    transform: scale(0);
    box-shadow: 0 0 0 1px rgba(255,255,255,0.1);
}

#incant-capsule.preparing {
    /* Invisible during prepare phase so quick modifier taps (e.g. Alt-Tab) don't flash the HUD. */
    opacity: 0;
    transform: scale(0);
}

#incant-capsule.recording {
    background: rgba(40, 0, 0, 0.9);
    opacity: 1;
    transform: scale(1);
    min-width: 56px;
    box-shadow:
        inset 0 0 8px rgba(255, 0, 0, 0.5),
        0 0 16px rgba(255, 0, 0, 0.3),
        0 0 32px rgba(255, 0, 0, 0.1);
    animation: recording-pulse 1s ease-in-out infinite alternate;
}

#incant-capsule.transcribing {
    background: rgba(0, 20, 60, 0.9);
    opacity: 1;
    transform: scale(1);
    min-width: 16px;
    box-shadow:
        inset 0 0 8px rgba(0, 120, 255, 0.5),
        0 0 16px rgba(0, 120, 255, 0.3);
    animation: shine-sweep 0.6s ease-in-out infinite;
}

#incant-capsule.prewarming {
    background: rgba(0, 20, 60, 0.9);
    opacity: 1;
    transform: scale(1);
    min-width: 16px;
    box-shadow: inset 0 0 8px rgba(0, 120, 255, 0.5);
}

#incant-capsule.error {
    background: rgba(60, 0, 0, 0.9);
    opacity: 1;
    transform: scale(1);
    min-width: 120px;
    padding: 8px 12px;
}

#incant-error-label {
    color: #ff6b6b;
    font-size: 12px;
    font-weight: 500;
    padding: 4px 8px;
}

#incant-meter {
    background: rgba(255, 0, 0, 0.6);
    border-radius: 6px;
    margin: 4px;
    box-shadow: inset 0 0 4px rgba(255, 100, 100, 0.8);
    transition: width 0.05s linear;
}

@keyframes recording-pulse {
    from {
        box-shadow:
            inset 0 0 8px rgba(255, 0, 0, 0.4),
            0 0 12px rgba(255, 0, 0, 0.2),
            0 0 24px rgba(255, 0, 0, 0.05);
    }
    to {
        box-shadow:
            inset 0 0 12px rgba(255, 0, 0, 0.7),
            0 0 20px rgba(255, 0, 0, 0.4),
            0 0 40px rgba(255, 0, 0, 0.15);
    }
}

@keyframes shine-sweep {
    0% {
        background: linear-gradient(90deg, rgba(0,120,255,0.9) 0%, rgba(0,120,255,0.9) 35%, rgba(200,230,255,0.6) 50%, rgba(0,120,255,0.9) 65%, rgba(0,120,255,0.9) 100%);
        background-size: 250% 100%;
        background-position: 100% 0;
    }
    100% {
        background: linear-gradient(90deg, rgba(0,120,255,0.9) 0%, rgba(0,120,255,0.9) 35%, rgba(200,230,255,0.6) 50%, rgba(0,120,255,0.9) 65%, rgba(0,120,255,0.9) 100%);
        background-size: 250% 100%;
        background-position: -100% 0;
    }
}
"#;
