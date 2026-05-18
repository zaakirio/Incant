#!/usr/bin/env bash
set -euo pipefail

# ── Colors ──
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
RESET='\033[0m'

info()  { echo -e "${BLUE}ℹ${RESET}  $*"; }
ok()    { echo -e "${GREEN}✓${RESET}  $*"; }
warn()  { echo -e "${YELLOW}⚠${RESET}  $*"; }
err()   { echo -e "${RED}✗${RESET}  $*"; }
die()   { err "$*"; exit 1; }

# ── Header ──
echo -e "\n${BOLD}🔮 Incant Installer${RESET}\n"
echo "Voice dictation for Hyprland / Wayland"
echo ""

# ── Preflight checks ──
info "Checking system..."

if [[ ! -f /etc/arch-release ]]; then
    warn "This installer is designed for Arch Linux / Omarchy."
    read -rp "Continue anyway? [y/N] " reply
    [[ "$reply" =~ ^[Yy]$ ]] || exit 1
fi

for cmd in cargo cmake git; do
    if ! command -v "$cmd" &>/dev/null; then
        die "Missing required tool: $cmd\n    Install: sudo pacman -S base-devel rust cmake git"
    fi
done
ok "Build tools present"

# ── Install system dependencies ──
info "Installing system dependencies..."

PACMAN_DEPS=(pipewire pipewire-alsa gtk4 gtk4-layer-shell wtype wl-clipboard)
MISSING_DEPS=()

for dep in "${PACMAN_DEPS[@]}"; do
    if ! pacman -Q "$dep" &>/dev/null; then
        MISSING_DEPS+=("$dep")
    fi
done

if [[ ${#MISSING_DEPS[@]} -gt 0 ]]; then
    echo "    Will install: ${MISSING_DEPS[*]}"
    sudo pacman -S --needed --noconfirm "${MISSING_DEPS[@]}"
fi

# dotool is AUR-only
if ! command -v dotool &>/dev/null; then
    if command -v yay &>/dev/null; then
        info "Installing dotool via yay..."
        yay -S --needed --noconfirm dotool
    elif command -v paru &>/dev/null; then
        info "Installing dotool via paru..."
        paru -S --needed --noconfirm dotool
    else
        warn "No AUR helper found (yay/paru). Skipping dotool."
        warn "    You can install it later: git clone https://aur.archlinux.org/dotool.git && cd dotool && makepkg -si"
    fi
else
    ok "dotool already installed"
fi

ok "System dependencies ready"

# ── Build ──
info "Building Incant (release mode)..."
cargo build --release
ok "Build complete"

# ── Install binaries ──
info "Installing binaries to /usr/local/bin..."
sudo install -Dm755 target/release/incant-daemon /usr/local/bin/incant-daemon
sudo install -Dm755 target/release/incant        /usr/local/bin/incant
sudo install -Dm755 target/release/incant-overlay /usr/local/bin/incant-overlay
ok "Binaries installed"

# ── Sherpa shared libraries ──
info "Installing Sherpa-ONNX shared libraries..."
SHERPA_LIB=$(find "$HOME/.cache/sherpa-rs" -name "libsherpa-onnx-c-api.so" -path "*/lib/*" | head -1 | xargs dirname 2>/dev/null)
if [ -n "$SHERPA_LIB" ] && [ -d "$SHERPA_LIB" ]; then
    sudo install -dm755 /usr/lib/incant
    sudo install -Dm644 "$SHERPA_LIB"/libsherpa-onnx-c-api.so   /usr/lib/incant/
    sudo install -Dm644 "$SHERPA_LIB"/libsherpa-onnx-cxx-api.so /usr/lib/incant/
    sudo install -Dm644 "$SHERPA_LIB"/libonnxruntime.so          /usr/lib/incant/
    echo "/usr/lib/incant" | sudo tee /etc/ld.so.conf.d/incant.conf >/dev/null
    sudo ldconfig
    ok "Sherpa libraries installed to /usr/lib/incant"
else
    warn "Could not find Sherpa-ONNX libraries in ~/.cache/sherpa-rs"
    warn "    The daemon may fail to start. Try rebuilding: cargo build --release"
fi

# ── Download model ──
info "Downloading STT model (~630 MB)..."
if ! incant-daemon download-model; then
    warn "Model download failed."
    warn "    You can retry later with: incant-daemon download-model"
fi
ok "Model ready"

# ── Hyprland config ──
HYPRLAND_CONFIG_DIR="${HOME}/.config/hypr"
HYPRLAND_CONFIG="${HYPRLAND_CONFIG_DIR}/incant.conf"

if [[ -f "$HYPRLAND_CONFIG" ]]; then
    warn "Existing Hyprland config found at ${HYPRLAND_CONFIG}"
    read -rp "    Overwrite? [y/N] " reply
    if [[ "$reply" =~ ^[Yy]$ ]]; then
        cp hyprland/incant.conf "$HYPRLAND_CONFIG"
        ok "Hyprland config updated"
    else
        info "Skipped. Make sure your Hyprland config includes the incant binds."
    fi
else
    mkdir -p "$HYPRLAND_CONFIG_DIR"
    cp hyprland/incant.conf "$HYPRLAND_CONFIG"
    ok "Installed Hyprland config to ${HYPRLAND_CONFIG}"
    info "    Add this line to ~/.config/hypr/hyprland.conf:"
    echo "        source = ${HYPRLAND_CONFIG}"
fi

# ── systemd service ──
SYSTEMD_USER_DIR="${HOME}/.config/systemd/user"
mkdir -p "$SYSTEMD_USER_DIR"

# Update ExecStart in the service file to match our install path
sed 's|^ExecStart=.*|ExecStart=/usr/local/bin/incant-daemon|' \
    systemd/incant-daemon.service \
    > "${SYSTEMD_USER_DIR}/incant-daemon.service"

systemctl --user daemon-reload
systemctl --user enable incant-daemon.service

read -rp "Start incant-daemon now? [Y/n] " reply
if [[ ! "$reply" =~ ^[Nn]$ ]]; then
    systemctl --user restart incant-daemon.service
    sleep 1
    ok "Daemon started"
else
    info "You can start it later with: systemctl --user start incant-daemon"
fi

# ── Doctor ──
echo ""
info "Running diagnostics..."
if command -v incant &>/dev/null; then
    incant doctor || true
else
    warn "incant not in PATH yet. Open a new terminal and run: incant doctor"
fi

# ── Done ──
echo -e "\n${GREEN}${BOLD}Installation complete!${RESET}\n"
echo "Quick start:"
echo "  Hold SUPER+Shift+D  → speak → release → text appears"
echo ""
echo "Useful commands:"
echo "  incant doctor       — check health"
echo "  incant status       — show daemon state"
echo "  incant ping         — check daemon responsiveness"
echo ""
