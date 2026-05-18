#!/usr/bin/env bash
# Incant uninstaller — undoes everything install.sh does, idempotently.
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

echo -e "\n${BOLD}🔮 Incant Uninstaller${RESET}\n"

# ── Stop and disable the user service ──
SYSTEMD_USER_FILE="${HOME}/.config/systemd/user/incant-daemon.service"
if [[ -f "$SYSTEMD_USER_FILE" ]]; then
    info "Stopping incant-daemon user service..."
    systemctl --user stop    incant-daemon.service 2>/dev/null || true
    systemctl --user disable incant-daemon.service 2>/dev/null || true
    rm -f "$SYSTEMD_USER_FILE"
    systemctl --user daemon-reload 2>/dev/null || true
    ok "User service removed"
else
    info "No user service unit found (already removed)"
fi

# ── Remove Hyprland source line + dropped-in config ──
HYPRLAND_CONFIG_DIR="${HOME}/.config/hypr"
HYPRLAND_CONFIG="${HYPRLAND_CONFIG_DIR}/incant.conf"

if [[ -f "$HYPRLAND_CONFIG" ]]; then
    rm -f "$HYPRLAND_CONFIG"
    ok "Removed ${HYPRLAND_CONFIG}"
fi

# Strip the auto-injected source block from any main hyprland.conf we find.
for candidate in \
    "${HYPRLAND_CONFIG_DIR}/hyprland.conf" \
    "${HYPRLAND_CONFIG_DIR}/hypr.conf" \
    "${HOME}/.hyprland.conf"; do
    if [[ -f "$candidate" ]] && grep -qF "incant.conf" "$candidate" 2>/dev/null; then
        info "Removing Incant source block from ${candidate}"
        # Delete the 3-line block: "# ── Incant voice dictation ──" + "# Remove this block..." + "source = .../incant.conf"
        # Also tolerate older single-line installs.
        tmp=$(mktemp)
        awk '
            /^# ── Incant voice dictation ──$/ { skip = 3; next }
            skip > 0 { skip--; next }
            /incant\.conf/ && /^source = / { next }
            { print }
        ' "$candidate" > "$tmp"
        # Trim trailing blank lines so we do not accumulate them across re-installs.
        sed -i -e :a -e '/^$/{$d;N;ba' -e '}' "$tmp"
        mv "$tmp" "$candidate"
        ok "Cleaned ${candidate}"
    fi
done

# Reload Hyprland if running.
if command -v hyprctl &>/dev/null && hyprctl instances &>/dev/null; then
    hyprctl reload &>/dev/null || true
    ok "Hyprland reloaded"
fi

# ── Remove binaries ──
for bin in incant incant-daemon incant-overlay; do
    for prefix in /usr/local/bin /usr/bin; do
        if [[ -f "${prefix}/${bin}" ]]; then
            sudo rm -f "${prefix}/${bin}"
            ok "Removed ${prefix}/${bin}"
        fi
    done
done

# ── Remove sherpa shared libs ──
for libdir in /usr/local/lib/incant /usr/lib/incant; do
    if [[ -d "$libdir" ]]; then
        sudo rm -rf "$libdir"
        ok "Removed ${libdir}"
    fi
done

# ── Remove legacy ld.so.conf.d drop-in ──
if [[ -f /etc/ld.so.conf.d/incant.conf ]]; then
    sudo rm -f /etc/ld.so.conf.d/incant.conf
    sudo ldconfig
    ok "Removed /etc/ld.so.conf.d/incant.conf"
fi

# ── Ask about user data ──
CACHE_DIR="${HOME}/.cache/incant"
CONFIG_DIR="${HOME}/.config/incant"
RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/incant"

if [[ -d "$CACHE_DIR" || -d "$CONFIG_DIR" ]]; then
    echo ""
    warn "User data still on disk:"
    [[ -d "$CACHE_DIR"  ]] && echo "    $CACHE_DIR  (models, debug recordings)"
    [[ -d "$CONFIG_DIR" ]] && echo "    $CONFIG_DIR (config.toml)"
    read -rp "Remove these too? [y/N] " reply
    if [[ "$reply" =~ ^[Yy]$ ]]; then
        rm -rf "$CACHE_DIR" "$CONFIG_DIR"
        ok "User data removed"
    else
        info "Keeping user data — re-installing later will reuse the model cache"
    fi
fi

# Always clean up the runtime socket dir; nothing valuable lives there.
[[ -d "$RUNTIME_DIR" ]] && rm -rf "$RUNTIME_DIR"

echo -e "\n${GREEN}${BOLD}Incant uninstalled.${RESET}\n"
