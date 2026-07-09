"""Interactive install, uninstall, and doctor.

The flow follows heretic's interactivity idiom: questionary prompts with
a reverse-highlight style, rich for status output, and every prompt
skippable via flags (--yes) so the whole thing stays scriptable.
Success is defined as "you heard it speak", not "exit 0".
"""

from __future__ import annotations

import shutil
import sys
import time

import questionary
from questionary import Choice, Style
from rich.console import Console
from rich.panel import Panel

from .config import CONFIG_PATH, load_config, write_default_config
from .install import (
    CLAUDE_SETTINGS,
    CODEX_CONFIG,
    INSTALLERS,
    detect_tools,
    opencode_plugin_path,
)

console = Console()
QSTYLE = Style([("highlighted", "reverse")])

TOOL_TARGETS = {
    "claude": ("Claude Code", str(CLAUDE_SETTINGS)),
    "codex": ("Codex", str(CODEX_CONFIG)),
    "opencode": ("OpenCode", "~/.config/opencode/plugin/incant.js"),
}

TLDR_SNIPPET = """\
## Spoken updates
End every reply with a final line:
TLDR: <what you did or found, max 20 words, plain speech, no code or paths>\
"""


def _hf_model_cached(model: str) -> bool:
    from pathlib import Path

    slug = "models--" + model.replace("/", "--")
    return (Path("~/.cache/huggingface/hub").expanduser() / slug / "snapshots").is_dir()


def _speak_and_wait(text: str, timeout: float = 600.0) -> bool:
    """Queue a verbatim narration and wait until the queue drains."""
    import httpx

    from .hooks import ensure_daemon

    cfg = load_config()
    if not ensure_daemon():
        return False
    try:
        httpx.post(cfg.daemon_url + "/say", json={"text": text, "source": "cli"}, timeout=5.0)
    except Exception:
        return False
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            health = httpx.get(cfg.daemon_url + "/health", timeout=2.0).json()
            if health.get("queue", 1) == 0:
                # Queue empty means synthesis finished; playback may lag a
                # moment, which is fine for a success check.
                time.sleep(1.0)
                return True
        except Exception:
            pass
        time.sleep(1.0)
    return False


def run_interactive_install(tools: list[str], yes: bool = False) -> int:
    interactive = not yes and sys.stdin.isatty()
    write_default_config()
    detected = detect_tools()
    available = [name for name, present in detected.items() if present]

    console.print(Panel.fit("[bold]incant[/] - voice for your coding agents", border_style="cyan"))

    if tools:
        targets = [t for t in tools if t in INSTALLERS]
    elif interactive:
        if not available:
            console.print("[red]No supported tools detected (claude, codex, opencode).[/]")
            return 1
        targets = questionary.checkbox(
            "Which agents should incant narrate?",
            choices=[
                Choice(title=f"{TOOL_TARGETS[name][0]}  ({TOOL_TARGETS[name][1]})", value=name, checked=True)
                for name in available
            ],
            style=QSTYLE,
        ).ask()
        if not targets:
            console.print("Nothing selected; nothing installed.")
            return 0
    else:
        targets = available

    if not targets:
        console.print("[red]No supported tools detected (claude, codex, opencode).[/]")
        return 1

    console.print("\n[bold]This will edit:[/]")
    for name in targets:
        console.print(f"  • {TOOL_TARGETS[name][0]:<12} {TOOL_TARGETS[name][1]}")
    console.print(f"  • config       {CONFIG_PATH}\n")

    if interactive:
        proceed = questionary.confirm("Install the hooks?", default=True, style=QSTYLE).ask()
        if not proceed:
            return 0

    for name in targets:
        message = INSTALLERS[name][0]()
        color = "yellow" if "SKIPPED" in message else "green"
        console.print(f"[{color}]✓[/] {message}")

    from .install import ensure_spacy_model

    with console.status("[cyan]Preparing the voice frontend…[/]"):
        ok, detail = ensure_spacy_model()
    console.print(f"[{'green' if ok else 'red'}]{'✓' if ok else '✗'}[/] voice frontend: {detail}")

    cfg = load_config()
    cached = _hf_model_cached(cfg.tts_model)
    if not cached:
        console.print(f"\nFirst run downloads the TTS model ([bold]{cfg.tts_model}[/], ~300 MB).")
    with console.status("[cyan]Starting the narrator and warming up the voice…[/]"):
        heard = _speak_and_wait("Incant is online. Your agents can speak now.")

    if heard:
        console.print("[green]✓[/] Spoke the test narration. If you heard it, you're done.")
    else:
        console.print(
            "[red]✗[/] Could not produce speech. Run [bold]incant doctor[/] and check ~/.local/state/incant/*.log"
        )

    console.print()
    notes = []
    if "opencode" in targets:
        notes.append("Restart OpenCode to load its plugin.")
    notes.append("Narration modes: incant mode full | tldr | summary   (full is the default)")
    notes.append("In a meeting: incant mute 30m   Talking too much: incant skip")
    notes.append("Pair with Hex (brew install --cask kitlangton-hex) for push-to-talk dictation.")
    for note in notes:
        console.print(f"  [dim]›[/] {note}")

    if interactive:
        console.print()
        show = questionary.confirm(
            "Want the optional agent snippet for crisp TLDR narrations?", default=False, style=QSTYLE
        ).ask()
        if show:
            console.print(
                Panel(
                    TLDR_SNIPPET,
                    title="paste into CLAUDE.md / AGENTS.md, then: incant mode tldr",
                    border_style="dim",
                )
            )
    return 0 if heard else 1


def run_interactive_uninstall(tools: list[str], yes: bool = False) -> int:
    from .install import run_uninstall

    interactive = not yes and sys.stdin.isatty()
    targets = tools or list(INSTALLERS)
    if interactive:
        proceed = questionary.confirm(
            f"Remove incant hooks from: {', '.join(targets)}?", default=True, style=QSTYLE
        ).ask()
        if not proceed:
            return 0
    for message in run_uninstall(targets):
        console.print(f"[green]✓[/] {message}")
    console.print("Daemon left running; stop it with: pkill -f 'incant.cli serve'")
    return 0


def run_doctor() -> int:
    from .checks import doctor_checks

    checks = doctor_checks()
    failures = 0
    for check in checks:
        mark = "[green]✓[/]" if check["ok"] else "[red]✗[/]"
        if not check["ok"]:
            failures += 1
        console.print(f" {mark} {check['title']:<18} [dim]{check['detail']}[/]")
    if failures:
        console.print(f"\n[red]{failures} check(s) failed.[/] Logs: ~/.local/state/incant/")
    else:
        console.print("\n[green]All checks passed.[/]")
    return 1 if failures else 0
