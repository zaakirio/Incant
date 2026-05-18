<p align="center">
  <img src="assets/banner.png" alt="Incant" width="720">
</p>

<p align="center">
  <strong>Press-and-hold voice dictation for Linux.</strong><br>
  Hold a hotkey, speak, release — your words appear wherever your cursor is.
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

Incant is a local-first voice dictation daemon for **Omarchy** (Arch Linux + Hyprland + Wayland + PipeWire). It runs entirely on your machine — no cloud services, no accounts, no telemetry. State-of-the-art on-device speech recognition powered by NVIDIA's Parakeet-TDT model delivers transcription latency in the order of 200 ms after the first load.

## Features

- **Press-and-hold dictation** — hold `SUPER+SHIFT+D`, speak, release; the transcribed text is injected at the cursor.
- **Double-tap lock mode** — tap the hotkey twice in quick succession to enter hands-free recording; tap once more to finish.
- **Live recording overlay** — a centered GTK4 layer-shell capsule with an animated audio meter shows recording state.
- **Procedural sound effects** — distinct start, stop, paste, and cancel tones, generated at runtime (no sample files).
- **Moonshine Tiny** — fast local ASR (~120 MB, English); Parakeet-TDT 0.6B is also supported.
- **Resilient text injection** — `wtype` (Wayland virtual keyboard) with `dotool` and `wl-copy` fallbacks.
- **Fully local** — model weights live in `~/.cache/incant`; no audio or text ever leaves your machine.

## Requirements

| Component | Notes |
|---|---|
| OS | Arch Linux (Omarchy recommended) |
| Compositor | Hyprland (Wayland) |
| Audio | PipeWire |
| Rust | 1.80 or newer |
| Disk | ~630 MB for the default Parakeet model |

## Installation

### Arch Linux (PKGBUILD)

A `PKGBUILD` is provided for building and installing as a regular Arch package:

### Quick install (recommended)

```bash
git clone https://github.com/zaakirio/incant.git
cd incant
./install.sh
```

The installer will detect your system, install dependencies, build, download the model, and run diagnostics.

### Manual install

```bash
git clone https://github.com/zaakirio/incant.git
cd incant
makepkg -si
```

### From source

**1. Install dependencies**

```bash
sudo pacman -S pipewire gtk4 gtk4-layer-shell wtype wl-clipboard
yay -S dotool
```

**2. Build and install the binaries**

```bash
git clone https://github.com/zaakirio/incant.git
cd incant
cargo build --release

sudo install -Dm755 target/release/incant-daemon  /usr/local/bin/incant-daemon
sudo install -Dm755 target/release/incant         /usr/local/bin/incant
sudo install -Dm755 target/release/incant-overlay /usr/local/bin/incant-overlay
```

**3. Download the speech model** (~120 MB, auto-downloaded on first run)

```bash

**4. Wire up Hyprland**

```bash
cp hyprland/incant.conf ~/.config/hypr/
```

Add the following line to your `~/.config/hypr/hyprland.conf`:

```conf
source = ~/.config/hypr/incant.conf
```

**5. Enable the user service**

```bash
cp systemd/incant-daemon.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now incant-daemon

# Verify everything is working
incant doctor
```

## Usage

| Action | Gesture |
|---|---|
| Dictate | Press and hold `SUPER+SHIFT+D`, speak, then release |
| Lock recording | Double-tap `SUPER+SHIFT+D`; tap once more to stop |
| Cancel | Press `Escape` while recording or transcribing |

### Command-line interface

The `incant` CLI talks to the running daemon over a Unix socket. It is invoked by Hyprland's `bind`/`bindr` handlers, but is also useful for scripting and debugging:

```bash
incant press     # Begin recording
incant release   # Stop recording and transcribe
incant cancel    # Cancel an in-progress recording or transcription
incant status    # Print the current daemon state
incant ping      # Health check
incant doctor    # Run diagnostic checks
```

## Configuration

Incant looks for its configuration at `~/.config/incant/config.toml`, which is created on first run. All keys have sensible defaults; override only what you need.

```toml
# Path to the ONNX model directory.
# Defaults to ~/.cache/incant/models/moonshine-tiny-en-int8
# Moonshine Tiny (~120 MB, English) is the default.
# Parakeet-TDT-0.6B-v2 is also supported (~630 MB, 25 languages).
# model_path = "/home/you/.cache/incant/models/moonshine-tiny-en-int8"

# Sample rate expected by the model. Do not change unless you know why.
sample_rate = 16000

# Unix socket used for CLI <-> daemon IPC.
# socket_path = "/run/user/1000/incant/daemon.sock"

# Audio capture buffer size, in samples.
buffer_size = 4096

# Text-injection backends, tried in order.
#   wtype   - virtual keyboard (preferred)
#   dotool  - uinput-based fallback
#   wl-copy - clipboard paste (last resort)
output_methods = ["wtype", "dotool", "wl-copy"]

# Show the GTK4 overlay capsule while recording.
show_overlay = true

# ONNX runtime threads (0 = auto-detect).
num_threads = 4

# Minimum press duration before a release is treated as dictation (ms).
minimum_key_time_ms = 150

# Double-tap lock mode.
double_tap_lock_enabled = true
double_tap_window_ms = 300

# Verbose logs and save the last capture to last_recording.wav.
debug = false
```

## Architecture

Incant is a small workspace of three Rust crates that communicate over a Unix socket:

```
                 Hyprland (bind / bindr)
                          │
                          ▼
                    incant (CLI)
                          │  Unix socket IPC
                          ▼
                   incant-daemon
                  ┌─────────────────────────────────┐
                  │  Audio capture (cpal + PipeWire) │
                  │  RMS metering → overlay          │
                  │  Press-and-hold state machine    │
                  │  Sound FX (rodio, procedural)    │
                  │  STT (Sherpa-ONNX + Parakeet)    │
                  │  Text injection (wtype / dotool) │
                  └─────────────────────────────────┘
                          │
                          ▼
                  incant-overlay
                  (GTK4 layer shell HUD)
```

| Crate | Role |
|---|---|
| `incant-cli` | Thin client; forwards `press`, `release`, `cancel`, etc. to the daemon. |
| `incant-daemon` | Audio capture, state machine, model inference, and text injection. |
| `incant-overlay` | GTK4 layer-shell overlay rendering the recording capsule and meter. |

## Runtime dependencies

| Package | Purpose |
|---|---|
| `pipewire` | Audio capture |
| `gtk4` | Overlay UI toolkit |
| `gtk4-layer-shell` | Wayland layer-shell bindings |
| `wtype` | Text injection (primary) |
| `dotool` | Text injection (uinput fallback) |
| `wl-clipboard` | Clipboard fallback |

## Troubleshooting

- **Daemon won't start** — check the service logs with `journalctl --user -u incant-daemon -f`.
- **No text appears after release** — ensure the focused window accepts virtual-keyboard input; otherwise the `wl-copy` fallback will populate the clipboard.
- **No audio captured** — confirm PipeWire is running (`systemctl --user status pipewire`) and that the default source is the microphone you expect.
- **Model missing** — re-run `incant-daemon download-model`. The default Parakeet model is ~630 MB and is cached under `~/.cache/incant/models/`.

## License

Incant is released under the [MIT License](LICENSE).
