"""incant command-line interface."""

from __future__ import annotations

import argparse
import re
import sys


def parse_duration(value: str) -> float:
    match = re.fullmatch(r"(\d+(?:\.\d+)?)\s*(s|m|h)?", value.strip())
    if not match:
        raise argparse.ArgumentTypeError(f"invalid duration: {value!r} (try 90s, 30m, 1h)")
    number = float(match.group(1))
    return number * {"s": 1, "m": 60, "h": 3600, None: 1}[match.group(2)]


def _post(path: str, body: dict | None = None) -> dict | None:
    import json
    import urllib.request

    from .hooks import _daemon_url

    req = urllib.request.Request(
        _daemon_url() + path,
        data=json.dumps(body or {}).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            return json.loads(resp.read())
    except Exception:
        return None


def main(argv: list[str] | None = None) -> int:
    argv = sys.argv[1:] if argv is None else argv

    # Hook paths must stay import-light and never fail the agent's turn.
    if argv[:2] == ["hook", "claude"]:
        if len(argv) > 2:
            from .hooks import hook_agent_event

            return hook_agent_event("claude", argv[2])
        from .hooks import hook_claude

        return hook_claude()
    if argv[:2] == ["hook", "codex"]:
        # Codex's notify program passes a JSON payload as the argument;
        # our lifecycle hooks pass a bare kind (prompt, permission, ...).
        if len(argv) > 2 and not argv[2].lstrip().startswith("{"):
            from .hooks import hook_agent_event

            return hook_agent_event("codex", argv[2])
        from .hooks import hook_codex

        return hook_codex(argv[2:])
    if argv[:2] == ["hook", "kimi"]:
        if len(argv) > 2:
            from .hooks import hook_agent_event

            return hook_agent_event("kimi", argv[2])
        from .hooks import hook_kimi

        return hook_kimi()
    if argv[:1] == ["_activity"]:
        import json as _json

        from .hooks import deliver_activity

        try:
            body = _json.loads(argv[argv.index("--body") + 1])
        except Exception:
            return 0
        return deliver_activity(body)
    if argv[:1] == ["_deliver"]:
        import json as _json

        from .hooks import deliver

        source = argv[argv.index("--source") + 1] if "--source" in argv else "unknown"
        meta = None
        if "--meta" in argv:
            try:
                meta = _json.loads(argv[argv.index("--meta") + 1])
            except Exception:
                meta = None
        return deliver(source, meta)
    if argv[:1] == ["_ensure"]:
        from .hooks import ensure_daemon

        return 0 if ensure_daemon() else 1

    parser = argparse.ArgumentParser(
        prog="incant",
        description="Voice for your coding agents: local TTS narration of finished turns.",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("serve", help="run the narration daemon in the foreground")

    p_install = sub.add_parser("install", help="wire incant into your coding tools (interactive)")
    p_install.add_argument("tools", nargs="*", help="claude, codex, opencode, kimi (default: choose from detected)")
    p_install.add_argument("--yes", "-y", action="store_true", help="no prompts; install for all detected tools")

    p_uninstall = sub.add_parser("uninstall", help="remove incant from your coding tools")
    p_uninstall.add_argument("tools", nargs="*", help="claude, codex, opencode, kimi (default: all)")
    p_uninstall.add_argument("--yes", "-y", action="store_true", help="no prompts")

    p_speak = sub.add_parser("speak", help="speak text verbatim (also warms up the model)")
    p_speak.add_argument("text", nargs="+")

    p_narrate = sub.add_parser("narrate", help="run text through the narration pipeline and speak it")
    p_narrate.add_argument("text", nargs="+")

    p_mode = sub.add_parser("mode", help="get or set the digest mode (how much is spoken)")
    p_mode.add_argument("value", nargs="?", choices=["full", "tldr", "summary"])

    p_behavior = sub.add_parser("behavior", help="get or set the default narration behavior")
    p_behavior.add_argument("value", nargs="?", choices=["auto", "notify", "off"])

    sub.add_parser("sessions", help="list active agent sessions")

    p_mute = sub.add_parser("mute", help="drop narrations (optionally for a duration, e.g. 30m)")
    p_mute.add_argument("duration", nargs="?", type=parse_duration, help="90s, 30m, 1h; omit for until-unmute")

    sub.add_parser("unmute", help="resume narrations")
    sub.add_parser("skip", help="stop current narration and clear the queue")
    sub.add_parser("status", help="show daemon and integration status")
    sub.add_parser("doctor", help="check every part of the pipeline")

    args = parser.parse_args(argv)

    if args.command == "serve":
        from .daemon import run_daemon

        run_daemon()
        return 0

    if args.command == "install":
        from .onboard import run_interactive_install

        return run_interactive_install(list(args.tools), yes=args.yes)

    if args.command == "uninstall":
        from .onboard import run_interactive_uninstall

        return run_interactive_uninstall(list(args.tools), yes=args.yes)

    if args.command in ("speak", "narrate"):
        from .hooks import ensure_daemon, post_narration

        if not ensure_daemon():
            print("Could not start the incant daemon; check ~/.local/state/incant/daemon.log", file=sys.stderr)
            return 1
        endpoint = "/say" if args.command == "speak" else "/narrate"
        ok = post_narration(" ".join(args.text), source="cli", endpoint=endpoint)
        if not ok:
            print("Not queued: the daemon is muted or unreachable (incant status).", file=sys.stderr)
            return 1
        print("Queued.")
        return 0

    if args.command == "mode":
        from .config import CONFIG_PATH, load_config, set_config_key

        if args.value:
            set_config_key("speech", "mode", args.value)
            print(f"mode = {args.value} (applies to the next narration)")
        else:
            print(f"mode = {load_config().speech_mode}  ({CONFIG_PATH})")
        return 0

    if args.command == "behavior":
        from .config import CONFIG_PATH, load_config, set_config_key

        if args.value:
            set_config_key("narration", "behavior", args.value)
            print(f"behavior = {args.value} (applies to the next turn)")
        else:
            print(f"behavior = {load_config().behavior}  ({CONFIG_PATH})")
        return 0

    if args.command == "sessions":
        import json
        import urllib.request

        from .hooks import _daemon_url, daemon_alive

        if not daemon_alive():
            print("Daemon not running.")
            return 0
        try:
            with urllib.request.urlopen(_daemon_url() + "/sessions", timeout=3) as resp:
                sessions = json.loads(resp.read()).get("sessions", [])
        except Exception:
            print("Daemon not reachable.", file=sys.stderr)
            return 1
        if not sessions:
            print("No active sessions.")
            return 0
        status_glyphs = {"working": "…", "awaiting_approval": "?", "awaiting_input": "?"}
        for s in sessions:
            dot = "●" if s["unread"] else ("♪" if s["speaking"] else status_glyphs.get(s.get("status", ""), " "))
            kill = "" if s["can_kill"] else "  (no kill)"
            extras = []
            status = s.get("status", "idle")
            if status != "idle":
                extras.append(status + (f": {s['status_detail']}" if s.get("status_detail") else ""))
            if s.get("subagents"):
                extras.append(f"{s['subagents']} subagents")
            extra = ("  " + "; ".join(extras)) if extras else ""
            print(f" {dot} {s['source']:<9} {s['project']:<18} {s['behavior']:<7} pid={s['pid']}{kill}{extra}")
        return 0

    if args.command == "mute":
        from .hooks import ensure_daemon

        if not ensure_daemon():
            print("Daemon not reachable.", file=sys.stderr)
            return 1
        result = _post("/mute", {"seconds": args.duration})
        if result is None:
            print("Daemon not reachable.", file=sys.stderr)
            return 1
        human = "until unmute" if args.duration is None else f"for {int(args.duration)}s"
        print(f"Muted {human}. Narrations are dropped, not queued.")
        return 0

    if args.command == "unmute":
        result = _post("/unmute")
        print("Unmuted." if result else "Daemon not running; nothing to unmute.")
        return 0

    if args.command == "skip":
        from .hooks import daemon_alive

        if not daemon_alive():
            print("Daemon not running.")
            return 0
        result = _post("/skip")
        print(result if result else "Daemon not reachable.")
        return 0

    if args.command == "status":
        import json
        import urllib.request

        from .config import CONFIG_PATH, load_config
        from .hooks import _daemon_url, daemon_alive
        from .install import (
            CLAUDE_SETTINGS,
            CODEX_CONFIG,
            KIMI_MARKER,
            detect_tools,
            kimi_config_path,
            opencode_plugin_path,
        )

        cfg = load_config()
        health = {}
        if daemon_alive():
            try:
                with urllib.request.urlopen(_daemon_url() + "/health", timeout=3) as resp:
                    health = json.loads(resp.read())
            except Exception:
                pass
        state = "running" if health else "stopped"
        if health.get("muted"):
            state += ", muted"
        print(f"daemon:   {state} (port {cfg.daemon_port})")
        if health:
            print(f"sessions: {health.get('sessions', 0)} active")
        print(f"behavior: {cfg.behavior} (default)")
        print(f"mode:     {cfg.speech_mode}")
        print(f"tts:      {cfg.tts_mode} -> {cfg.tts_base_url} ({cfg.tts_model})")
        voices = ", ".join(f"{k}={v}" for k, v in sorted(cfg.voices.items()))
        print(f"voices:   {voices} (default {cfg.tts_voice})")
        print(f"config:   {CONFIG_PATH}{'' if CONFIG_PATH.exists() else ' (defaults, not written yet)'}")
        detected = detect_tools()
        claude_installed = CLAUDE_SETTINGS.exists() and "incant hook claude" in CLAUDE_SETTINGS.read_text()
        codex_text = CODEX_CONFIG.read_text() if CODEX_CONFIG.exists() else ""
        codex_installed = "incant" in codex_text and "notify" in codex_text
        opencode_installed = opencode_plugin_path().exists()
        kimi_config = kimi_config_path()
        kimi_installed = kimi_config.exists() and KIMI_MARKER in kimi_config.read_text()
        for name, installed in (
            ("claude", claude_installed),
            ("codex", codex_installed),
            ("opencode", opencode_installed),
            ("kimi", kimi_installed),
        ):
            state = "hooked" if installed else ("detected, not hooked" if detected[name] else "not detected")
            print(f"{name + ':':<9} {state}")
        return 0

    if args.command == "doctor":
        from .onboard import run_doctor

        return run_doctor()

    return 1


if __name__ == "__main__":
    sys.exit(main())
