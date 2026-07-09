# Incant (menu bar app)

A native macOS menu bar companion for [incant](https://github.com/zaakirio/incant), the local TTS narration engine for coding agents.
It shows every active Claude Code / Codex / OpenCode session, lets you set narration behavior per session or globally, pick voices, and mute - all from the menu bar, editing live.

This is a thin client. The Python `incant` daemon is the engine and single source of truth; the app renders its live state (over Server-Sent Events) and sends control commands over HTTP. It never touches the TTS server or config file directly, so the CLI and the app never fight over ownership.

## Status

Phase 3: the menu bar popover.

- Live session list via the daemon's SSE stream (project, agent, unread, speaking).
- Per-session narration behavior (auto / notify / off) and a global default.
- Voice picker with instant audition, speed slider, digest-mode picker.
- Mute / skip; "start engine" when the daemon is offline.
- End-a-session (SIGTERM) with confirmation.

Phase 4 (next) adds the floating Messenger-style session bubbles.

## Build

Requires Xcode command-line tools and [XcodeGen](https://github.com/yonaskolb/XcodeGen) (`brew install xcodegen`).
The full Xcode IDE does not need to be opened.

```sh
./build.sh              # build
./build.sh --run        # build and launch
./build.sh --dist       # build and package dist/Incant.zip
```

The build is ad-hoc signed for local use. Distribution (Developer ID signing, notarization, Sparkle auto-update, Homebrew cask) comes with Phase 3's release step.

## Requires the engine

Install and set up the daemon first:

```sh
uv tool install incant
incant install
```

The app connects to it on `127.0.0.1:5111`.
