<p align="center">
  <img src="assets/banner.png" alt="Incant" width="720">
</p>

<p align="center">
  <strong>Local-first voice dictation for Linux.</strong><br>
  Double-tap a hotkey, speak, tap again — your words appear wherever your cursor is.
</p>

<p align="center">
  <a href="#installation"><img src="https://img.shields.io/badge/platform-Linux-1f1f1f?style=flat-square" alt="Platform: Linux"></a>
  <a href="#installation"><img src="https://img.shields.io/badge/compositor-Hyprland-5e4b8b?style=flat-square" alt="Compositor: Hyprland"></a>
  <a href="#installation"><img src="https://img.shields.io/badge/audio-PipeWire-5e4b8b?style=flat-square" alt="Audio: PipeWire"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/built%20with-Rust-dea584?style=flat-square&logo=rust&logoColor=white" alt="Built with Rust"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="License: MIT"></a>
</p>

---

## Overview

Incant is a voice dictation daemon for **Arch Linux + Hyprland + Wayland + PipeWire**. It runs entirely on your machine — no cloud services, no accounts, no telemetry. Speech is transcribed locally with NVIDIA's Parakeet‑TDT model via [sherpa‑onnx](https://github.com/k2-fsa/sherpa-onnx); typical end‑to‑end latency after the first inference is a few hundred milliseconds on a modern CPU.

## Features

- **Double‑tap dictation** — tap the hotkey twice quickly to start recording; tap once more to stop and transcribe.
- **Safe with Alt combos** — by default Incant only reacts to an explicit double‑tap, so bindings like `Alt` alone, `Alt+Tab`, or `Alt+F4` never trigger a recording.
- **Live recording overlay** — a GTK4 layer‑shell capsule with an animated audio meter shows recording state.
- **Procedural sound effects** — start / stop / paste / cancel cues generated at runtime (no sample files on disk).
- **Two on‑device models** — **Parakeet‑TDT 0.6B v2 (default, ~622 MB)** for accuracy, or **Moonshine Tiny (~120 MB, English)** for size.
- **Pinned, verified model downloads** — every file is fetched from a pinned HuggingFace commit and SHA‑256 verified.
- **Resilient text injection** — `wtype` (Wayland virtual keyboard), with `dotool` and `wl-copy` fallbacks.
- **Fully local** — model weights live in `~/.cache/incant/models/`; no audio or text ever leaves your machine.

## Requirements

| Component  | Notes                                                  |
|------------|--------------------------------------------------------|
| OS         | Arch Linux (Omarchy recommended)                       |
| Compositor | Hyprland (Wayland)                                     |
| Audio      | PipeWire                                               |
| Rust       | 1.80 or newer (only required if building from source)  |
| Disk       | ~660 MB for the default Parakeet model                 |

Other Linux distributions are likely to work for the daemon itself but are not regularly tested; the installer is Arch/`pacman`‑specific. PRs welcome.

## Installation

### Quick install (recommended)

```bash
git clone https://github.com/zaakirio/Incant.git
cd Incant
./install.sh
```

The installer detects your system, installs system dependencies, builds in release mode, downloads and verifies the speech model, and runs `incant doctor`.

To remove everything later:

```bash
./uninstall.sh
```

### Arch Linux (PKGBUILD)

A `PKGBUILD` is provided. **Note:** the current PKGBUILD assumes a populated `~/.cache/sherpa-rs/` from a prior `cargo build`; until that limitation is removed (see [`PKGBUILD`](PKGBUILD) header), prefer `./install.sh`.

```bash
makepkg -si
```

### From source

**1. Install dependencies**

```bash
sudo pacman -S pipewire pipewire-alsa gtk4 gtk4-layer-shell wtype wl-clipboard
yay -S dotool   # AUR
```

**2. Build the workspace**

```bash
cargo build --release
sudo install -Dm755 target/release/incant-daemon  /usr/local/bin/incant-daemon
sudo install -Dm755 target/release/incant         /usr/local/bin/incant
sudo install -Dm755 target/release/incant-overlay /usr/local/bin/incant-overlay
```

**3. Download the speech model** (~660 MB, pinned + SHA‑256 verified)

```bash
incant-daemon download-model
```

The model is cached under `~/.cache/incant/models/`. Files are downloaded with resume support, so an interrupted download can be restarted.

**4. Wire up Hyprland**

```bash
cp hyprland/incant.conf ~/.config/hypr/
```

Add to your `~/.config/hypr/hyprland.conf`:

```conf
source = ~/.config/hypr/incant.conf
```

**5. Enable the user service**

```bash
cp systemd/incant-daemon.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now incant-daemon
incant doctor
```

## Usage

By default, Incant is in **double‑tap‑only mode** — a single tap of the hotkey does nothing, which makes it safe to bind to a bare modifier like `Alt` without breaking `Alt+Tab` and friends.

| Action       | Gesture                                                                                  |
|--------------|------------------------------------------------------------------------------------------|
| Dictate      | **Double‑tap** the hotkey within 300 ms → speak → **tap once more** to stop & transcribe |
| Cancel       | Press `Escape` while recording or transcribing                                           |

If you prefer the classic press‑and‑hold workflow, set `use_double_tap_only = false` in your config (see below).

### Command‑line interface

The `incant` CLI talks to the running daemon over a Unix socket (chmod 0600). It is invoked by Hyprland's `bind` handlers, but is also useful for scripting and debugging:

```bash
incant           # Show daemon status
incant press     # Tap (start recording on second tap in double-tap mode)
incant release   # Stop recording (press-and-hold mode only)
incant cancel    # Cancel an in-progress recording or transcription
incant status    # Print the current daemon state
incant ping      # Health check
incant stop      # Stop the daemon
incant doctor    # Run diagnostic checks
```

## Configuration

Incant looks for `~/.config/incant/config.toml`; the file is created with defaults on first run.

```toml
# Path to the ONNX model directory.
# Defaults to ~/.cache/incant/models/parakeet-tdt-0.6b-v3-int8
# To use Moonshine Tiny instead (~120 MB, English-only), set a path whose
# basename contains "moonshine":
# model_path = "/home/USER/.cache/incant/models/moonshine-tiny-en-int8"

# Sample rate the model expects. Do not change unless you know why.
sample_rate = 16000

# Unix socket for CLI <-> daemon IPC.
# socket_path = "/run/user/1000/incant/daemon.sock"

# Audio capture buffer size (samples).
buffer_size = 4096

# Text-injection backends, tried in order:
#   wtype   = Wayland virtual_keyboard protocol (preferred)
#   dotool  = uinput-based fallback
#   wl-copy = clipboard fallback (last resort)
output_methods = ["wtype", "dotool", "wl-copy"]

# Show the GTK4 overlay capsule while recording.
show_overlay = true

# ONNX runtime threads (0 = auto).
num_threads = 4

# Minimum recording duration in ms; shorter "presses" are discarded as
# accidental taps (prevents HUD flash on Alt+Tab etc.).
minimum_key_time_ms = 150

# Double-tap lock mode.
double_tap_lock_enabled = true
double_tap_window_ms    = 300

# Default: only react to an explicit double-tap. A single tap never starts
# a recording, so binding a bare modifier (e.g. Alt) is safe.
# Set false to enable press-and-hold dictation.
use_double_tap_only = true

# Sound effect volume (0.0 - 1.0).
sound_volume = 0.3

# Verbose logs; also writes last_recording.wav to ~/.cache/incant/.
debug = false
```

## Architecture

Incant is a small workspace of three Rust crates communicating over a Unix socket:

```
                 Hyprland (bind / bindr)
                          │
                          ▼
                    incant (CLI)
                          │  Unix socket IPC (chmod 0600)
                          ▼
                   incant-daemon
                  ┌─────────────────────────────────┐
                  │ Audio capture (cpal + PipeWire) │
                  │ RMS metering → overlay          │
                  │ Press-and-hold state machine    │
                  │ Sound FX (rodio, procedural)    │
                  │ STT (sherpa-onnx + Parakeet)    │
                  │ Text injection (wtype/dotool)   │
                  └─────────────────────────────────┘
                          │
                          ▼
                  incant-overlay
                  (GTK4 layer-shell HUD)
```

| Crate            | Role                                                                            |
|------------------|---------------------------------------------------------------------------------|
| `incant-cli`     | Thin client; forwards `press`, `release`, `cancel`, etc. to the daemon.         |
| `incant-daemon`  | Audio capture, state machine, model inference, and text injection.              |
| `incant-overlay` | GTK4 layer‑shell overlay rendering the recording capsule and meter.             |

## Runtime dependencies

| Package            | Purpose                                |
|--------------------|----------------------------------------|
| `pipewire`         | Audio capture                          |
| `gtk4`             | Overlay UI toolkit                     |
| `gtk4-layer-shell` | Wayland layer‑shell bindings           |
| `wtype`            | Text injection (primary)               |
| `dotool`           | Text injection (uinput fallback)       |
| `wl-clipboard`     | Clipboard fallback                     |

## Troubleshooting

- **Daemon won't start** — `journalctl --user -u incant-daemon -f`.
- **`libsherpa-onnx-c-api.so: cannot open shared object file`** — the sherpa libs were not installed where the dynamic linker can find them. `./install.sh` writes `/etc/ld.so.conf.d/incant.conf` and runs `ldconfig`; re-run it if you skipped that step.
- **No text appears after release** — make sure the focused window accepts virtual‑keyboard input; otherwise the `wl-copy` fallback will put the transcription on the clipboard.
- **No audio captured** — `systemctl --user status pipewire`; check the default source matches your microphone.
- **Model missing or corrupted** — re-run `incant-daemon download-model`. Existing files are SHA‑256 verified; any mismatch triggers a re-download.

## License

MIT — see [LICENSE](LICENSE).
