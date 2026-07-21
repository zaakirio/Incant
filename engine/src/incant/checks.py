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

    from .install import spacy_model_present

    checks.append({"id": "g2p", "title": "Voice frontend", "ok": spacy_model_present(),
                   "detail": "spaCy en_core_web_sm (misaki g2p)"})

    player = shutil.which("afplay") or shutil.which("ffplay") or shutil.which("aplay")
    checks.append({"id": "player", "title": "Audio player", "ok": bool(player),
                   "detail": player or "afplay/ffplay/aplay"})

    from .install import KIMI_MARKER, detect_tools, kimi_config_path

    claude_ok = CLAUDE_SETTINGS.exists() and "incant hook claude" in CLAUDE_SETTINGS.read_text()
    codex_text = CODEX_CONFIG.read_text() if CODEX_CONFIG.exists() else ""
    codex_ok = "notify" in codex_text and "incant" in codex_text
    kimi_config = kimi_config_path()
    kimi_ok = kimi_config.exists() and KIMI_MARKER in kimi_config.read_text()
    hook_checks = [
        ("claude", "Claude Code hook", claude_ok, str(CLAUDE_SETTINGS)),
        ("codex", "Codex hook", codex_ok, str(CODEX_CONFIG)),
        ("opencode", "OpenCode plugin", opencode_plugin_path().exists(), str(opencode_plugin_path())),
        ("kimi", "Kimi CLI hook", kimi_ok, str(kimi_config)),
    ]
    detected = detect_tools()
    for check_id, title, ok, detail in hook_checks:
        # A tool that isn't on this machine at all is not a failure.
        if detected.get(check_id) or ok:
            checks.append({"id": check_id, "title": title, "ok": ok, "detail": detail})

    checks.append({"id": "config", "title": "Config file", "ok": CONFIG_PATH.exists(), "detail": str(CONFIG_PATH)})
    return checks
