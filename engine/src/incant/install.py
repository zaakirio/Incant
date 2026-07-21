"""Wire incant into Claude Code, Codex, OpenCode, and Kimi CLI.

Each integration is an idempotent edit to that tool's own config:
  Claude Code -> lifecycle hooks in ~/.claude/settings.json
  Codex       -> the notify program in ~/.codex/config.toml
                 plus lifecycle hooks in ~/.codex/hooks.json
  OpenCode    -> a plugin file in ~/.config/opencode/plugin/
  Kimi CLI    -> [[hooks]] blocks in ~/.kimi-code/config.toml
                 (or ~/.kimi/config.toml for the older kimi-cli)

Beyond the finished-turn hook that feeds narration, every integration
also wires lifecycle hooks (turn started, waiting for approval, needs
input, subagent started/stopped, session ended) that feed the daemon's
/activity endpoint so the UI can show live status.
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
CODEX_HOOKS = Path("~/.codex/hooks.json").expanduser()
OPENCODE_DIR = Path("~/.config/opencode").expanduser()
# Kimi Code CLI (new) and kimi-cli (old) read different config dirs.
KIMI_DIRS = (Path("~/.kimi-code").expanduser(), Path("~/.kimi").expanduser())

OPENCODE_PLUGIN_TEMPLATE = """\
// incant: narrates finished turns and mirrors live session status
// (working / awaiting approval / subagents) into the incant daemon.
// Installed by `incant install`; remove with `incant uninstall`.
const INCANT_URL = "http://127.0.0.1:{port}"

const latestBySession = new Map()
const textByMessage = new Map()
const spoken = new Set()
const parentOf = new Map()          // subagent sessionID -> parent sessionID
const finishedChildren = new Set()
const lastWorkingPing = new Map()

export const IncantPlugin = async ({{ $, directory }}) => {{
  // The plugin runs inside the opencode process, so our own PID is the
  // killable session process the daemon uses for "end this session".
  const pid = process.pid

  async function post(path, body) {{
    await fetch(INCANT_URL + path, {{
      method: "POST",
      headers: {{ "content-type": "application/json" }},
      body: JSON.stringify(body),
    }})
  }}

  async function withDaemon(fn) {{
    try {{
      await fn()
    }} catch {{
      // Daemon not up yet: start it, then retry once.
      await $`{incant_bin} _ensure`.nothrow().quiet()
      await fn()
    }}
  }}

  async function activity(sessionID, status, extra) {{
    await withDaemon(() =>
      post("/activity", {{ source: "opencode", session_id: sessionID, status, cwd: directory, pid, ...extra }})
    )
  }}

  return {{
    event: async ({{ event }}) => {{
      try {{
        const p = event.properties || {{}}
        if (event.type === "session.created") {{
          const info = p.info
          if (info && info.parentID) {{
            // Subagents get no bubble of their own; they tick the
            // parent's swarm counter instead.
            parentOf.set(info.id, info.parentID)
            await activity(info.parentID, "working", {{ subagent_delta: 1 }})
          }}
        }}
        if (event.type === "session.status") {{
          const id = p.sessionID
          const st = typeof p.status === "string" ? p.status : (p.status && p.status.type)
          if (id && !parentOf.has(id)) {{
            if (st === "busy" || st === "retry") {{
              if (Date.now() - (lastWorkingPing.get(id) || 0) > 15000) {{
                await activity(id, "working", st === "retry" ? {{ detail: "retrying" }} : undefined)
                lastWorkingPing.set(id, Date.now())
              }}
            }} else if (st === "idle") {{
              lastWorkingPing.delete(id)
              await activity(id, "idle")
            }}
          }}
        }}
        // The docs call these permission.asked/replied; the SDK emits
        // permission.updated/replied. Handle both spellings.
        if (event.type === "permission.updated" || event.type === "permission.asked") {{
          const sid = p.sessionID || (p.info && p.info.sessionID)
          if (sid) {{
            const title = p.title || (p.info && p.info.title)
            const target = parentOf.get(sid) || sid
            lastWorkingPing.delete(target)
            await activity(target, "awaiting_approval", title ? {{ detail: title }} : undefined)
          }}
        }}
        if (event.type === "permission.replied") {{
          const sid = p.sessionID
          if (sid) await activity(parentOf.get(sid) || sid, "working")
        }}
        if (event.type === "session.deleted") {{
          const info = p.info
          if (info) {{
            if (parentOf.has(info.id)) {{
              if (!finishedChildren.has(info.id)) {{
                finishedChildren.add(info.id)
                await activity(parentOf.get(info.id), "working", {{ subagent_delta: -1 }})
              }}
              parentOf.delete(info.id)
            }} else {{
              await activity(info.id, "ended")
            }}
          }}
        }}
        if (event.type === "message.updated") {{
          const info = p.info
          if (info && info.role === "assistant") latestBySession.set(info.sessionID, info.id)
        }}
        if (event.type === "message.part.updated") {{
          const part = p.part
          if (part && part.type === "text" && part.text) textByMessage.set(part.messageID, part.text)
        }}
        if (event.type !== "session.idle") return
        const sessionID = p.sessionID
        if (parentOf.has(sessionID)) {{
          // A subagent finished: tick the parent's counter, stay silent.
          if (!finishedChildren.has(sessionID)) {{
            finishedChildren.add(sessionID)
            await activity(parentOf.get(sessionID), "working", {{ subagent_delta: -1 }})
          }}
          return
        }}
        const messageID = latestBySession.get(sessionID)
        if (!messageID || spoken.has(messageID)) return
        const text = textByMessage.get(messageID)
        if (!text) return
        spoken.add(messageID)
        await withDaemon(() =>
          post("/narrate", {{ text, source: "opencode", session_id: sessionID, cwd: directory, pid }})
        )
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
        "kimi": any(d.is_dir() for d in KIMI_DIRS) or shutil.which("kimi") is not None,
    }


# -- Claude Code / Codex hooks.json (shared JSON hook-group shape) ------


def _strip_incant_groups(hooks: dict, marker: str) -> bool:
    """Remove every incant hook from a Claude-style hooks dict; True if any."""
    removed = False
    for event in list(hooks):
        groups = []
        for group in hooks[event]:
            kept = [
                h
                for h in group.get("hooks", [])
                if not ("incant" in h.get("command", "") and marker in h.get("command", ""))
            ]
            if len(kept) != len(group.get("hooks", [])):
                removed = True
            if kept:
                group["hooks"] = kept
                groups.append(group)
        if groups:
            hooks[event] = groups
        else:
            del hooks[event]
    return removed


# event -> `incant hook claude <kind>` argument ("" = the finished-turn hook)
CLAUDE_HOOK_KINDS = {
    "Stop": "",
    "UserPromptSubmit": "prompt",
    "PermissionRequest": "permission",
    "Notification": "notify",
    "PostToolUse": "tool",
    "SubagentStart": "subagent-start",
    "SubagentStop": "subagent-stop",
    "SessionEnd": "end",
}


def install_claude() -> str:
    settings: dict = {}
    if CLAUDE_SETTINGS.exists():
        settings = json.loads(CLAUDE_SETTINGS.read_text() or "{}")
    hooks = settings.setdefault("hooks", {})
    _strip_incant_groups(hooks, "hook claude")
    base = f"{incant_bin()} hook claude"
    for event, kind in CLAUDE_HOOK_KINDS.items():
        command = f"{base} {kind}" if kind else base
        hooks.setdefault(event, []).append({"hooks": [{"type": "command", "command": command}]})
    CLAUDE_SETTINGS.parent.mkdir(parents=True, exist_ok=True)
    CLAUDE_SETTINGS.write_text(json.dumps(settings, indent=2) + "\n")
    return "claude: hooks installed (turns, status, approvals, subagents) in ~/.claude/settings.json"


def uninstall_claude() -> str:
    if not CLAUDE_SETTINGS.exists():
        return "claude: nothing to remove"
    settings = json.loads(CLAUDE_SETTINGS.read_text() or "{}")
    hooks = settings.get("hooks", {})
    if not _strip_incant_groups(hooks, "hook claude"):
        return "claude: no incant hook found"
    if not hooks:
        settings.pop("hooks", None)
    CLAUDE_SETTINGS.write_text(json.dumps(settings, indent=2) + "\n")
    return "claude: hooks removed"


# -- Codex --------------------------------------------------------------

NOTIFY_RE = re.compile(r"^\s*notify\s*=", re.MULTILINE)


def _codex_notify_line() -> str:
    parts = incant_bin().split()
    args = json.dumps(parts + ["hook", "codex"])
    return f"notify = {args}"


# Codex hooks use session_id where notify uses thread-id; they are the
# same conversation UUID, so activity and narration land on one session.
CODEX_HOOK_KINDS = {
    "UserPromptSubmit": "prompt",
    "PermissionRequest": "permission",
    "PostToolUse": "tool",
    "SubagentStart": "subagent-start",
    "SubagentStop": "subagent-stop",
}


def _install_codex_hooks() -> str:
    data: dict = {}
    if CODEX_HOOKS.exists():
        try:
            data = json.loads(CODEX_HOOKS.read_text() or "{}")
        except ValueError:
            return "codex: SKIPPED hooks - ~/.codex/hooks.json is not valid JSON"
    hooks = data.setdefault("hooks", {})
    _strip_incant_groups(hooks, "hook codex")
    for event, kind in CODEX_HOOK_KINDS.items():
        command = f"{incant_bin()} hook codex {kind}"
        hooks.setdefault(event, []).append(
            {"hooks": [{"type": "command", "command": command, "statusMessage": "incant"}]}
        )
    CODEX_HOOKS.write_text(json.dumps(data, indent=2) + "\n")
    return "codex: lifecycle hooks written to ~/.codex/hooks.json"


def install_codex() -> str:
    CODEX_CONFIG.parent.mkdir(parents=True, exist_ok=True)
    text = CODEX_CONFIG.read_text() if CODEX_CONFIG.exists() else ""
    line = _codex_notify_line()
    match = NOTIFY_RE.search(text)
    already = False
    if match:
        start = match.start()
        end = text.find("\n", start)
        end = len(text) if end == -1 else end
        existing = text[start:end]
        if existing.strip() == line:
            already = True
        elif "incant" not in existing:
            return (
                "codex: SKIPPED - config.toml already has a notify program:\n"
                f"    {existing.strip()}\n"
                "  Chain it manually or replace it with: " + line
            )
        else:
            text = text[:start] + line + text[end:]
    else:
        # notify is a top-level key: it must come before any [table] section.
        table = re.search(r"^\s*\[", text, re.MULTILINE)
        insert_at = table.start() if table else len(text)
        prefix = text[:insert_at]
        if prefix and not prefix.endswith("\n"):
            prefix += "\n"
        text = prefix + line + "\n" + text[insert_at:]
    if not already:
        CODEX_CONFIG.write_text(text)
    hooks_msg = _install_codex_hooks()
    notify_msg = "notify already set" if already else "notify program set in ~/.codex/config.toml"
    return f"codex: {notify_msg}; {hooks_msg.split(': ', 1)[1]}"


def uninstall_codex() -> str:
    removed = False
    if CODEX_CONFIG.exists():
        lines = CODEX_CONFIG.read_text().splitlines(keepends=True)
        kept = [l for l in lines if not (NOTIFY_RE.match(l) and "incant" in l)]
        if len(kept) != len(lines):
            CODEX_CONFIG.write_text("".join(kept))
            removed = True
    if CODEX_HOOKS.exists():
        try:
            data = json.loads(CODEX_HOOKS.read_text() or "{}")
        except ValueError:
            data = None
        if data is not None and _strip_incant_groups(data.get("hooks", {}), "hook codex"):
            if not data.get("hooks"):
                CODEX_HOOKS.unlink()
            else:
                CODEX_HOOKS.write_text(json.dumps(data, indent=2) + "\n")
            removed = True
    return "codex: notify + hooks removed" if removed else "codex: nothing to remove"


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


# -- Kimi CLI -----------------------------------------------------------

# (event, matcher, `incant hook kimi <kind>` argument)
KIMI_HOOK_EVENTS = [
    ("Stop", None, ""),
    ("StopFailure", None, "fail"),
    ("UserPromptSubmit", None, "prompt"),
    ("PostToolUse", None, "tool"),
    ("PreToolUse", "AskUserQuestion", "question"),
    ("SubagentStart", None, "subagent-start"),
    ("SubagentStop", None, "subagent-stop"),
    ("SessionEnd", None, "end"),
]
# Only the newer Kimi Code CLI (~/.kimi-code) knows this event; the old
# kimi-cli may reject unknown event names at config parse.
KIMI_NEW_CLI_EVENTS = [("PermissionRequest", None, "permission")]

KIMI_MARKER = "incant hook kimi"


def kimi_config_path() -> Path:
    for directory in KIMI_DIRS:
        if directory.is_dir():
            return directory / "config.toml"
    return KIMI_DIRS[0] / "config.toml"


def _strip_kimi_blocks(text: str) -> str:
    """Drop every [[hooks]] block (and our marker comment) that runs incant."""
    lines = text.splitlines(keepends=True)
    out: list[str] = []
    i = 0
    while i < len(lines):
        if lines[i].strip() == "[[hooks]]":
            j = i + 1
            while j < len(lines) and not lines[j].lstrip().startswith("["):
                j += 1
            if KIMI_MARKER in "".join(lines[i:j]):
                while out and out[-1].strip().startswith("# incant"):
                    out.pop()
                i = j
                continue
        out.append(lines[i])
        i += 1
    return "".join(out)


def install_kimi() -> str:
    path = kimi_config_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    text = _strip_kimi_blocks(path.read_text() if path.exists() else "")
    events = list(KIMI_HOOK_EVENTS)
    if path.parent == KIMI_DIRS[0]:
        events += KIMI_NEW_CLI_EVENTS
    blocks = ["# incant hooks (added by `incant install`; remove with `incant uninstall`)"]
    base = f"{incant_bin()} hook kimi"
    for event, matcher, kind in events:
        command = f"{base} {kind}" if kind else base
        block = f'[[hooks]]\nevent = "{event}"\n'
        if matcher:
            block += f'matcher = "{matcher}"\n'
        block += f"command = {json.dumps(command)}\n"
        blocks.append(block)
    if text and not text.endswith("\n"):
        text += "\n"
    path.write_text(text + "\n" + "\n".join(blocks))
    return f"kimi: hooks installed (turns, status, approvals, swarm) in {path}"


def uninstall_kimi() -> str:
    removed = False
    for directory in KIMI_DIRS:
        path = directory / "config.toml"
        if not path.exists():
            continue
        text = path.read_text()
        stripped = _strip_kimi_blocks(text)
        if stripped != text:
            path.write_text(stripped)
            removed = True
    return "kimi: hooks removed" if removed else "kimi: no incant hooks found"


# -- entry --------------------------------------------------------------

INSTALLERS = {
    "claude": (install_claude, uninstall_claude),
    "codex": (install_codex, uninstall_codex),
    "opencode": (install_opencode, uninstall_opencode),
    "kimi": (install_kimi, uninstall_kimi),
}


def run_install(tools: list[str]) -> list[str]:
    write_default_config()
    detected = detect_tools()
    targets = tools or [name for name, present in detected.items() if present]
    messages = []
    if not targets:
        return ["No supported tools detected (claude, codex, opencode, kimi)."]
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
