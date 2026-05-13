# 🎙️ Incant

Press-and-hold a hotkey to transcribe your voice and paste the result wherever you're typing.

Built for **Omarchy** (Arch Linux + Hyprland + Wayland + PipeWire). Fully local inference. No cloud. No accounts.

## Features

| | |
|---|---|
| **Press-and-hold** | Hold `SUPER+Shift+D` → speak → release → text appears |
| **Red recording glow** | Centered capsule overlay with animated audio meter |
| **Sound FX** | Start beep, stop beep, paste chime, cancel tone |
| **Parakeet-TDT** | NVIDIA's state-of-the-art ASR (6.34% WER, 25 languages) |
| **Fast** | ~200ms transcription latency after first load |
| **Private** | Zero data leaves your machine |

## Install

```bash
# Dependencies (Arch)
sudo pacman -S wtype gtk4-layer-shell
yay -S dotool

# Build
git clone https://github.com/zaakirio/incant.git
cd incant
cargo build --release

# Install
sudo cp target/release/incant-daemon /usr/local/bin/
sudo cp target/release/incant /usr/local/bin/
sudo cp target/release/incant-overlay /usr/local/bin/

# Model (auto-downloaded on first run, ~630MB)
incant-daemon download-model

# Hyprland bindings
cp hyprland/incant.conf ~/.config/hypr/
# Add to ~/.config/hypr/hyprland.conf:
#   source = ~/.config/hypr/incant.conf

# systemd service
cp systemd/incant-daemon.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now incant-daemon
```

## Usage

1. **Press and hold** `SUPER+Shift+D`
2. **Speak**
3. **Release** to transcribe
4. Text appears where your cursor is

### Alternative: Double-tap lock

Double-tap `SUPER+Shift+D` to enter locked recording mode. Tap once more to stop and transcribe.

### Cancel

Press `Escape` to cancel a recording or transcription in progress.

### CLI

```bash
incant press     # Start recording (hold)
incant release   # Stop and transcribe (release)
incant cancel    # Cancel
incant status    # Show daemon state
incant ping      # Check daemon health
```

## Configuration

`~/.config/incant/config.toml` (auto-created on first run):

```toml
# Model: Parakeet-TDT-0.6B-v2-int8 (default) or Moonshine Tiny
model_path = "/home/YOU/.cache/incant/models/parakeet-tdt-0.6b-v2-int8"

# Minimum hold time before transcription (ms)
minimum_key_time_ms = 150

# Double-tap lock
double_tap_lock_enabled = true
double_tap_window_ms = 300

# Sound
# (no config needed — generated procedurally)

# Debug: save last_recording.wav
debug = false
```

## Architecture

```
Hyprland (bind/bindr)
    │
    ▼
incant press/release
    │
    ▼
Unix Socket IPC
    │
    ▼
incant-daemon
├── Audio capture (cpal + PipeWire)
├── Audio metering (RMS → overlay)
├── State machine (press-and-hold)
├── Sound FX (rodio, procedural WAV)
├── STT (Sherpa-ONNX + Parakeet)
└── Text injection (wtype → dotool → wl-copy)
    │
    ▼
incant-overlay (GTK4 layer shell)
```

## Dependencies

| Package | Purpose |
|---------|---------|
| `pipewire` | Audio capture |
| `gtk4` | Overlay UI |
| `gtk4-layer-shell` | Wayland layer shell |
| `wtype` | Text injection (primary) |
| `dotool` | Text injection (fallback) |
| `wl-clipboard` | Clipboard fallback |

## License

MIT
