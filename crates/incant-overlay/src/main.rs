use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use serde::Deserialize;
use std::io::{BufRead, BufReader};
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
    ok: bool,
    message: String,
    state: Option<DaemonState>,
}

fn main() {
    let app = gtk4::Application::new(Some("org.omarchy.incant.overlay"), Default::default());

    app.connect_activate(|app| {
        let window = gtk4::ApplicationWindow::new(app);
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Left, true);
        window.set_anchor(Edge::Right, true);
        window.set_anchor(Edge::Bottom, true);
        window.set_default_size(1, 1);

        // Capsule container
        let capsule = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        capsule.set_halign(gtk4::Align::Center);
        capsule.set_valign(gtk4::Align::Center);
        capsule.set_widget_name("incant-capsule");

        // Inner glow / meter bar
        let meter_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        meter_bar.set_widget_name("incant-meter");
        meter_bar.set_hexpand(false);

        capsule.append(&meter_bar);
        window.set_child(Some(&capsule));

        // CSS provider
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(CSS);
        gtk4::style_context_add_provider_for_display(
            &gtk4::gdk::Display::default().expect("no display"),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        window.present();

        // Shared state
        let state: Arc<Mutex<DaemonState>> = Arc::new(Mutex::new(DaemonState {
            status: Status::Hidden,
            meter: Meter { average_power: 0.0, peak_power: 0.0 },
            message: None,
        }));

        // Spawn IPC listener
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
    let ctx = capsule.style_context();

    // Remove all status classes
    for class in &["hidden", "preparing", "recording", "transcribing", "prewarming", "error"] {
        ctx.remove_class(class);
    }

    match state.status {
        Status::Hidden => {
            ctx.add_class("hidden");
        }
        Status::Preparing => {
            ctx.add_class("preparing");
        }
        Status::Recording => {
            ctx.add_class("recording");
            let avg = state.meter.average_power.min(1.0).max(0.0);
            let peak = state.meter.peak_power.min(1.0).max(0.0);
            let width = 16.0 + (peak * 40.0); // 16px to 56px
            meter_bar.set_size_request(width as i32, -1);
            meter_bar.set_opacity(avg as f64);
        }
        Status::Transcribing => {
            ctx.add_class("transcribing");
        }
        Status::Prewarming => {
            ctx.add_class("prewarming");
        }
        Status::Error => {
            ctx.add_class("error");
            if let Some(ref msg) = state.message {
                // Create a tooltip label
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

    loop {
        if let Ok(stream) = UnixStream::connect(&socket_path) {
            let reader = BufReader::new(stream);
            for line in reader.lines() {
                if let Ok(line) = line {
                    if let Ok(response) = serde_json::from_str::<Response>(&line) {
                        if let Some(daemon_state) = response.state {
                            *state.lock().unwrap() = daemon_state;
                        }
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

const CSS: &str = r#"
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
    background: rgba(0, 0, 0, 0.8);
    opacity: 1;
    transform: scale(1);
    min-width: 16px;
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
