"""Wire incant into Claude Code, Codex, and OpenCode.

Each integration is an idempotent edit to that tool's own config:
  Claude Code -> a Stop hook in ~/.claude/settings.json
  Codex       -> the notify program in ~/.codex/config.toml
  OpenCode    -> a plugin file in ~/.config/opencode/plugin/
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

from .config import load_config, write_default_config

# misaki's English g2p needs this spaCy model. Pinned to the wheel that matches
# spaCy 3.8; `spacy download` needs pip (absent in a uv tool env), so we install
# the wheel with `uv pip install --python <this interpreter>`.
SPACY_MODEL = "en_core_web_sm"
SPACY_WHEEL = (
    "https://github.com/explosion/spacy-models/releases/download/"
    "en_core_web_sm-3.8.0/en_core_web_sm-3.8.0-py3-none-any.whl"
)


def spacy_model_present() -> bool:
    try:
        import spacy  # noqa: F401
        from spacy.util import is_package

        return bool(is_package(SPACY_MODEL))
    except Exception:
        return False


def ensure_spacy_model() -> tuple[bool, str]:
    """Make misaki's English voice frontend usable. Returns (ok, detail)."""
    if spacy_model_present():
        return True, "already installed"
    uv = shutil.which("uv")
    if uv:
        proc = subprocess.run(
            [uv, "pip", "install", "--python", sys.executable, SPACY_WHEEL],
            capture_output=True, text=True,
        )
        if proc.returncode == 0 and spacy_model_present():
            return True, "installed via uv pip"
    # Fallback for non-uv installs (pipx/pip venvs), where spacy's own downloader works.
    proc = subprocess.run(
        [sys.executable, "-m", "spacy", "download", SPACY_MODEL],
        capture_output=True, text=True,
    )
    if proc.returncode == 0 and spacy_model_present():
        return True, "installed via spacy download"
    return False, "could not install; run: uv pip install --python $(which incant-python) " + SPACY_WHEEL

CLAUDE_SETTINGS = Path("~/.claude/settings.json").expanduser()
CODEX_CONFIG = Path("~/.codex/config.toml").expanduser()
OPENCODE_DIR = Path("~/.config/opencode").expanduser()

OPENCODE_PLUGIN_TEMPLATE = """\
// incant: narrates finished turns through the incant daemon.
// Installed by `incant install`; remove with `incant uninstall`.
const INCANT_URL = "http://127.0.0.1:{port}"

const latestBySession = new Map()
const textByMessage = new Map()
const spoken = new Set()

export const IncantPlugin = async ({{ $, directory }}) => {{
  // The plugin runs inside the opencode process, so our own PID is the
  // killable session process the daemon uses for "end this session".
  const pid = process.pid

  async function post(text, sessionID) {{
    await fetch(INCANT_URL + "/narrate", {{
      method: "POST",
      headers: {{ "content-type": "application/json" }},
      body: JSON.stringify({{ text, source: "opencode", session_id: sessionID, cwd: directory, pid }}),
    }})
  }}

  return {{
    event: async ({{ event }}) => {{
      try {{
        if (event.type === "message.updated") {{
          const info = event.properties.info
          if (info && info.role === "assistant") latestBySession.set(info.sessionID, info.id)
        }}
        if (event.type === "message.part.updated") {{
          const part = event.properties.part
          if (part && part.type === "text" && part.text) textByMessage.set(part.messageID, part.text)
        }}
        if (event.type !== "session.idle") return
        const sessionID = event.properties.sessionID
        const messageID = latestBySession.get(sessionID)
        if (!messageID || spoken.has(messageID)) return
        const text = textByMessage.get(messageID)
        if (!text) return
        spoken.add(messageID)
        try {{
          await post(text, sessionID)
        }} catch {{
          // Daemon not up yet: start it, then retry once.
          await $`{incant_bin} _ensure`.nothrow().quiet()
          await post(text, sessionID)
        }}
      }} catch {{
        // Never let narration break the session.
      }}
    }},
  }}
}}

export default IncantPlugin
"""


def incant_bin() -> str:
    exe = shutil.which("incant")
    if exe:
        return exe
    candidate = Path(sys.argv[0]).resolve()
    if candidate.name == "incant":
        return str(candidate)
    return f"{sys.executable} -m incant.cli"


def detect_tools() -> dict[str, bool]:
    return {
        "claude": CLAUDE_SETTINGS.parent.exists(),
        "codex": CODEX_CONFIG.parent.exists(),
        "opencode": OPENCODE_DIR.exists() or shutil.which("opencode") is not None,
    }


# -- Claude Code --------------------------------------------------------


def install_claude() -> str:
    settings: dict = {}
    if CLAUDE_SETTINGS.exists():
        settings = json.loads(CLAUDE_SETTINGS.read_text() or "{}")
    hooks = settings.setdefault("hooks", {})
    stop = hooks.setdefault("Stop", [])
    command = f"{incant_bin()} hook claude"
    for group in stop:
        for hook in group.get("hooks", []):
            if "incant hook claude" in hook.get("command", ""):
                hook["command"] = command
                CLAUDE_SETTINGS.write_text(json.dumps(settings, indent=2) + "\n")
                return "claude: Stop hook already present (refreshed path)"
    stop.append({"hooks": [{"type": "command", "command": command}]})
    CLAUDE_SETTINGS.parent.mkdir(parents=True, exist_ok=True)
    CLAUDE_SETTINGS.write_text(json.dumps(settings, indent=2) + "\n")
    return "claude: Stop hook installed in ~/.claude/settings.json"


def uninstall_claude() -> str:
    if not CLAUDE_SETTINGS.exists():
        return "claude: nothing to remove"
    settings = json.loads(CLAUDE_SETTINGS.read_text() or "{}")
    stop = settings.get("hooks", {}).get("Stop", [])
    kept = []
    for group in stop:
        group_hooks = [h for h in group.get("hooks", []) if "incant hook claude" not in h.get("command", "")]
        if group_hooks:
            group["hooks"] = group_hooks
            kept.append(group)
    if stop == kept:
        return "claude: no incant hook found"
    settings["hooks"]["Stop"] = kept
    if not kept:
        del settings["hooks"]["Stop"]
    CLAUDE_SETTINGS.write_text(json.dumps(settings, indent=2) + "\n")
    return "claude: Stop hook removed"


# -- Codex --------------------------------------------------------------

NOTIFY_RE = re.compile(r"^\s*notify\s*=", re.MULTILINE)


def _codex_notify_line() -> str:
    parts = incant_bin().split()
    args = json.dumps(parts + ["hook", "codex"])
    return f"notify = {args}"


def install_codex() -> str:
    CODEX_CONFIG.parent.mkdir(parents=True, exist_ok=True)
    text = CODEX_CONFIG.read_text() if CODEX_CONFIG.exists() else ""
    line = _codex_notify_line()
    match = NOTIFY_RE.search(text)
    if match:
        start = match.start()
        end = text.find("\n", start)
        end = len(text) if end == -1 else end
        existing = text[start:end]
        if existing.strip() == line:
            return "codex: notify already set"
        if "incant" not in existing:
            return (
                "codex: SKIPPED - config.toml already has a notify program:\n"
                f"    {existing.strip()}\n"
                "  Chain it manually or replace it with: " + line
            )
        text = text[:start] + line + text[end:]
    else:
        # notify is a top-level key: it must come before any [table] section.
        table = re.search(r"^\s*\[", text, re.MULTILINE)
        insert_at = table.start() if table else len(text)
        prefix = text[:insert_at]
        if prefix and not prefix.endswith("\n"):
            prefix += "\n"
        text = prefix + line + "\n" + text[insert_at:]
    CODEX_CONFIG.write_text(text)
    return "codex: notify program set in ~/.codex/config.toml"


def uninstall_codex() -> str:
    if not CODEX_CONFIG.exists():
        return "codex: nothing to remove"
    lines = CODEX_CONFIG.read_text().splitlines(keepends=True)
    kept = [l for l in lines if not (NOTIFY_RE.match(l) and "incant" in l)]
    if len(kept) == len(lines):
        return "codex: no incant notify found"
    CODEX_CONFIG.write_text("".join(kept))
    return "codex: notify program removed"


# -- OpenCode -----------------------------------------------------------


def _opencode_plugin_dir() -> Path:
    # Both spellings exist across opencode versions; prefer whichever
    # already exists so we sit next to the user's other plugins.
    for name in ("plugin", "plugins"):
        candidate = OPENCODE_DIR / name
        if candidate.is_dir():
            return candidate
    return OPENCODE_DIR / "plugin"


def opencode_plugin_path() -> Path:
    return _opencode_plugin_dir() / "incant.js"


def install_opencode() -> str:
    cfg = load_config()
    path = opencode_plugin_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(OPENCODE_PLUGIN_TEMPLATE.format(port=cfg.daemon_port, incant_bin=incant_bin()))
    return f"opencode: plugin written to {path}"


def uninstall_opencode() -> str:
    removed = False
    for name in ("plugin", "plugins"):
        path = OPENCODE_DIR / name / "incant.js"
        if path.exists():
            path.unlink()
            removed = True
    return "opencode: plugin removed" if removed else "opencode: no plugin found"


# -- entry --------------------------------------------------------------

INSTALLERS = {
    "claude": (install_claude, uninstall_claude),
    "codex": (install_codex, uninstall_codex),
    "opencode": (install_opencode, uninstall_opencode),
}


def run_install(tools: list[str]) -> list[str]:
    write_default_config()
    detected = detect_tools()
    targets = tools or [name for name, present in detected.items() if present]
    messages = []
    if not targets:
        return ["No supported tools detected (claude, codex, opencode)."]
    for name in targets:
        if name not in INSTALLERS:
            messages.append(f"{name}: unknown tool")
            continue
        if not tools and not detected.get(name):
            continue
        messages.append(INSTALLERS[name][0]())
    return messages


def run_uninstall(tools: list[str]) -> list[str]:
    targets = tools or list(INSTALLERS)
    return [INSTALLERS[name][1]() for name in targets if name in INSTALLERS]
