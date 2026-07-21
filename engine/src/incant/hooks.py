"""Agent-side hook entrypoints.

These run inside the coding agent's hook mechanism, so they must exit
fast and never fail the agent's turn: extract the assistant's final
text plus session metadata (id, cwd, and the agent's PID), hand it to a
detached delivery child, exit 0. The child starts the daemon if needed
and POSTs the narration.

The agent PID must be captured HERE: the hook process is a descendant
of the agent, but the delivery child detaches into its own session, so
its ancestry is gone by the time it runs. We walk up from our parent to
find the agent process so the daemon can offer "end this session".

Deliberately stdlib-only on the hot path.
"""

from __future__ import annotations

import contextlib
import json
import os
import subprocess
import sys
import time
import urllib.request

# comm substrings that identify each agent's own process.
AGENT_PROCESS_HINTS = {
    "claude": ("claude",),
    "codex": ("codex",),
    "opencode": ("opencode",),
    "kimi": ("kimi",),
}


def _daemon_url() -> str:
    port = os.environ.get("INCANT_PORT", "5111")
    return f"http://127.0.0.1:{port}"


def find_agent_pid(source: str, max_hops: int = 15) -> int | None:
    """Walk up the process tree for the agent's own process.

    Returns None if no ancestor matches - safer than guessing, since the
    PID is used for session termination. Some agents (e.g. Claude Code
    running under node) may not match by name; those simply won't offer
    a kill action, which is the correct conservative failure.
    """
    hints = AGENT_PROCESS_HINTS.get(source, (source,))
    pid = os.getppid()
    for _ in range(max_hops):
        if pid <= 1:
            break
        try:
            out = subprocess.run(
                ["ps", "-o", "ppid=,comm=", "-p", str(pid)],
                capture_output=True,
                text=True,
                timeout=2,
            ).stdout.strip()
        except Exception:
            break
        if not out:
            break
        parts = out.split(None, 1)
        ppid = parts[0]
        comm = parts[1].lower() if len(parts) > 1 else ""
        if any(hint in comm for hint in hints):
            return pid
        pid = int(ppid) if ppid.isdigit() else 0
    return None


def _spawn_deliver(text: str, source: str, meta: dict) -> None:
    """Hand off to a detached child and return immediately."""
    if not text.strip():
        return
    proc = subprocess.Popen(
        [sys.executable, "-m", "incant.cli", "_deliver", "--source", source, "--meta", json.dumps(meta)],
        stdin=subprocess.PIPE,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    proc.stdin.write(text.encode())
    proc.stdin.close()


def _spawn_activity(source: str, status: str, meta: dict, detail: str | None = None,
                    subagent_delta: int | None = None) -> None:
    """Deliver a session status signal via a detached child.

    The child boots the daemon if needed — deliberately: an activity ping
    at turn start warms the daemon and TTS server, so the narration at
    turn end plays without a cold-start delay.
    """
    body = dict(meta)
    body.update({"source": source, "status": status})
    if detail:
        body["detail"] = detail
    if subagent_delta:
        body["subagent_delta"] = subagent_delta
    subprocess.Popen(
        [sys.executable, "-m", "incant.cli", "_activity", "--body", json.dumps(body)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )


def _throttled(source: str, session_id: str, seconds: float = 20.0) -> bool:
    """True if a keepalive ping for this session fired within `seconds`.

    High-frequency hooks (PostToolUse fires per tool call) only exist to
    keep 'working' fresh; a touch-file mtime check keeps the hot path at
    one os.stat instead of a daemon round-trip per call.
    """
    safe = "".join(c if c.isalnum() or c in "-_" else "_" for c in session_id)[:80]
    from .config import STATE_DIR

    path = STATE_DIR / f"activity-{source}-{safe}"
    now = time.time()
    try:
        if now - path.stat().st_mtime < seconds:
            return True
    except OSError:
        pass
    try:
        STATE_DIR.mkdir(parents=True, exist_ok=True)
        path.touch()
    except OSError:
        pass
    return False


# -- Claude Code (Stop hook) ------------------------------------------


def last_assistant_text_from_transcript(transcript_path: str) -> str:
    """Extract the final assistant message from a Claude Code transcript JSONL."""
    last = ""
    try:
        with open(transcript_path) as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    entry = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if entry.get("type") != "assistant":
                    continue
                content = entry.get("message", {}).get("content", [])
                if isinstance(content, str):
                    text = content
                else:
                    text = "\n".join(
                        block.get("text", "")
                        for block in content
                        if isinstance(block, dict) and block.get("type") == "text"
                    )
                if text.strip():
                    last = text
    except OSError:
        return ""
    return last


def hook_claude() -> int:
    try:
        payload = json.load(sys.stdin)
    except Exception:
        return 0
    # A Stop hook can itself trigger another Stop; never narrate twice.
    if payload.get("stop_hook_active"):
        return 0
    text = payload.get("last_assistant_message") or ""
    if not text:
        transcript = payload.get("transcript_path", "")
        text = last_assistant_text_from_transcript(transcript) if transcript else ""
    meta = {
        "session_id": payload.get("session_id"),
        "cwd": payload.get("cwd") or os.getcwd(),
        "pid": find_agent_pid("claude"),
    }
    _spawn_deliver(text, "claude", meta)
    return 0


# -- Codex (notify program) --------------------------------------------


def hook_codex(argv: list[str]) -> int:
    if not argv:
        return 0
    try:
        payload = json.loads(argv[0])
    except Exception:
        return 0
    if os.environ.get("INCANT_DEBUG_CODEX"):
        try:
            from .config import STATE_DIR

            STATE_DIR.mkdir(parents=True, exist_ok=True)
            (STATE_DIR / "codex-payload.json").write_text(json.dumps(payload, indent=2))
        except Exception:
            pass
    if payload.get("type") != "agent-turn-complete":
        return 0
    text = payload.get("last-assistant-message") or ""
    meta = {
        "session_id": payload.get("thread-id") or payload.get("thread_id"),
        "cwd": payload.get("cwd") or os.getcwd(),
        "pid": find_agent_pid("codex"),
    }
    _spawn_deliver(text, "codex", meta)
    return 0


# -- Kimi CLI (Stop hook, Claude-style payload) --------------------------


def hook_kimi() -> int:
    try:
        payload = json.load(sys.stdin)
    except Exception:
        return 0
    if payload.get("stop_hook_active"):
        return 0
    text = payload.get("last_assistant_message") or payload.get("response") or ""
    meta = {
        "session_id": payload.get("session_id"),
        "cwd": payload.get("cwd") or os.getcwd(),
        "pid": find_agent_pid("kimi"),
    }
    if text:
        _spawn_deliver(text, "kimi", meta)
    elif meta["session_id"]:
        # No text in the payload: still mark the turn finished.
        _spawn_activity("kimi", "idle", meta)
    return 0


# -- lifecycle events (shared: Claude, Kimi, and Codex hooks all use the
#    same Claude-style stdin JSON contract) ------------------------------

# Notification hook types that mean "the agent is waiting on the user".
_INPUT_NOTIFICATION_TYPES = ("agent_needs_input", "idle_prompt", "elicitation_dialog")


def _unthrottle(source: str, session_id: str) -> None:
    """Drop the keepalive throttle so the next 'working' ping goes
    through immediately — used when leaving an awaiting_* state, which
    must not linger for the throttle window after the user responds."""
    safe = "".join(c if c.isalnum() or c in "-_" else "_" for c in session_id)[:80]
    from .config import STATE_DIR

    with contextlib.suppress(OSError):
        (STATE_DIR / f"activity-{source}-{safe}").unlink()


def hook_agent_event(source: str, kind: str) -> int:
    """One entrypoint for every lifecycle hook that isn't a finished turn.

    kind is set at install time per hook event:
      prompt          UserPromptSubmit        -> working
      tool            PostToolUse             -> working (throttled keepalive)
      permission      PermissionRequest       -> awaiting_approval
      notify          Notification            -> awaiting_approval / awaiting_input
      question        PreToolUse[AskUserQuestion] -> awaiting_input
      subagent-start  SubagentStart           -> subagent count +1
      subagent-stop   SubagentStop            -> subagent count -1
      fail            StopFailure             -> idle (with the error as detail)
      end             SessionEnd              -> session removed
    """
    try:
        payload = json.load(sys.stdin)
    except Exception:
        payload = {}
    session_id = payload.get("session_id")
    if not session_id:
        return 0
    meta = {
        "session_id": session_id,
        "cwd": payload.get("cwd") or os.getcwd(),
        "pid": find_agent_pid(source),
    }
    if kind == "prompt":
        _spawn_activity(source, "working", meta)
    elif kind == "tool":
        if not _throttled(source, session_id):
            _spawn_activity(source, "working", meta)
    elif kind == "permission":
        _unthrottle(source, session_id)
        tool = payload.get("tool_name") or ""
        detail = f"wants to use {tool}" if tool else "waiting for permission"
        _spawn_activity(source, "awaiting_approval", meta, detail=detail)
    elif kind == "notify":
        ntype = payload.get("notification_type") or ""
        message = payload.get("message") or payload.get("body") or ""
        if ntype == "permission_prompt" or "permission" in message.lower():
            _unthrottle(source, session_id)
            _spawn_activity(source, "awaiting_approval", meta, detail=message or None)
        elif ntype in _INPUT_NOTIFICATION_TYPES or "waiting for your input" in message.lower():
            _unthrottle(source, session_id)
            _spawn_activity(source, "awaiting_input", meta, detail=message or None)
    elif kind == "question":
        _unthrottle(source, session_id)
        _spawn_activity(source, "awaiting_input", meta, detail="asking a question")
    elif kind == "subagent-start":
        name = payload.get("agent_name") or payload.get("agent_type") or ""
        detail = f"running {name}" if name else "running subagents"
        _spawn_activity(source, "working", meta, detail=detail, subagent_delta=1)
    elif kind == "subagent-stop":
        _spawn_activity(source, "working", meta, subagent_delta=-1)
    elif kind == "fail":
        _spawn_activity(source, "idle", meta, detail=payload.get("error_message") or "turn failed")
    elif kind == "end":
        _spawn_activity(source, "ended", meta)
    return 0


# -- delivery child -----------------------------------------------------


def daemon_alive(timeout: float = 1.0) -> bool:
    try:
        with urllib.request.urlopen(_daemon_url() + "/health", timeout=timeout) as resp:
            return resp.status == 200
    except Exception:
        return False


def ensure_daemon(wait: float = 30.0) -> bool:
    if daemon_alive():
        return True
    from .config import STATE_DIR

    STATE_DIR.mkdir(parents=True, exist_ok=True)
    logfile = (STATE_DIR / "daemon.log").open("ab")
    subprocess.Popen(
        [sys.executable, "-m", "incant.cli", "serve"],
        stdout=logfile,
        stderr=logfile,
        start_new_session=True,
    )
    deadline = time.monotonic() + wait
    while time.monotonic() < deadline:
        if daemon_alive():
            return True
        time.sleep(0.5)
    return False


def post_narration(text: str, source: str, endpoint: str = "/narrate", meta: dict | None = None) -> bool:
    body = {"text": text, "source": source}
    if meta:
        for field_name in ("session_id", "cwd", "pid"):
            if meta.get(field_name) is not None:
                body[field_name] = meta[field_name]
    req = urllib.request.Request(
        _daemon_url() + endpoint,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=10.0) as resp:
            if resp.status != 200:
                return False
            result = json.loads(resp.read())
            # "queued" false but "held"/"off"/"duplicate" is a successful
            # delivery - the daemon chose not to speak, which is correct.
            return bool(result.get("queued") or result.get("held") or "behavior" in result)
    except Exception:
        return False


def deliver(source: str, meta: dict | None = None) -> int:
    text = sys.stdin.read()
    if not text.strip():
        return 0
    if not ensure_daemon():
        return 1
    return 0 if post_narration(text, source, meta=meta) else 1


def deliver_activity(body: dict) -> int:
    if not body.get("session_id"):
        return 0
    if not ensure_daemon():
        return 1
    req = urllib.request.Request(
        _daemon_url() + "/activity",
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=10.0) as resp:
            return 0 if resp.status == 200 else 1
    except Exception:
        return 1
