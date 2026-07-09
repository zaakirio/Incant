"""Structured pipeline health checks.

Shared by `incant doctor` (CLI) and the daemon's GET /doctor endpoint,
which the menu bar app's onboarding renders. Deliberately dependency-
light so importing it in the daemon is cheap.
"""

from __future__ import annotations

import shutil

from .config import CONFIG_PATH, load_config
from .install import CLAUDE_SETTINGS, CODEX_CONFIG, opencode_plugin_path


def _hf_model_cached(model: str) -> bool:
    from pathlib import Path

    slug = "models--" + model.replace("/", "--")
    return (Path("~/.cache/huggingface/hub").expanduser() / slug / "snapshots").is_dir()


def doctor_checks() -> list[dict]:
    """Each check: {id, title, ok, detail}."""
    import httpx

    from .hooks import daemon_alive

    cfg = load_config()
    checks: list[dict] = []

    alive = daemon_alive()
    checks.append({"id": "daemon", "title": "Narration daemon", "ok": alive, "detail": f"port {cfg.daemon_port}"})

    tts_ok = False
    if alive or cfg.tts_mode == "remote":
        try:
            tts_ok = httpx.get(cfg.tts_base_url + "/", timeout=3.0).status_code < 500
        except Exception:
            tts_ok = False
    checks.append({"id": "tts", "title": "Speech server", "ok": tts_ok, "detail": f"{cfg.tts_mode} → {cfg.tts_base_url}"})

    checks.append({"id": "model", "title": "Voice model", "ok": _hf_model_cached(cfg.tts_model),
                   "detail": cfg.tts_model})

    player = shutil.which("afplay") or shutil.which("ffplay") or shutil.which("aplay")
    checks.append({"id": "player", "title": "Audio player", "ok": bool(player),
                   "detail": player or "afplay/ffplay/aplay"})

    claude_ok = CLAUDE_SETTINGS.exists() and "incant hook claude" in CLAUDE_SETTINGS.read_text()
    codex_text = CODEX_CONFIG.read_text() if CODEX_CONFIG.exists() else ""
    codex_ok = "notify" in codex_text and "incant" in codex_text
    checks.append({"id": "claude", "title": "Claude Code hook", "ok": claude_ok, "detail": str(CLAUDE_SETTINGS)})
    checks.append({"id": "codex", "title": "Codex hook", "ok": codex_ok, "detail": str(CODEX_CONFIG)})
    checks.append({"id": "opencode", "title": "OpenCode plugin", "ok": opencode_plugin_path().exists(),
                   "detail": str(opencode_plugin_path())})

    checks.append({"id": "config", "title": "Config file", "ok": CONFIG_PATH.exists(), "detail": str(CONFIG_PATH)})
    return checks
