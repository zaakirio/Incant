# Incant

Give your coding agents a voice.
Incant speaks each finished turn from **Claude Code**, **Codex**, and **OpenCode** through fast local text-to-speech, with a native macOS menu bar app and floating per-session bubbles.
Everything runs on your machine; no cloud, no API keys.

This is a monorepo:

```
Incant/
├── engine/   # the narration engine — Python, ships on PyPI as `incant`
│             #   the `incant` CLI, the daemon, agent hooks, TTS via mlx-audio
└── app/      # the macOS menu bar app — SwiftUI, ships as Incant.app
              #   session bubbles, live popover, onboarding; a client of the engine
```

## Quick start

```sh
# 1. the engine
uv tool install incant      # or: pipx install incant
incant install              # wires Claude Code / Codex / OpenCode, guided

# 2. the app (optional, adds the menu bar + bubbles)
brew install --cask incant  # once published; until then, build from app/
```

The next time any agent finishes a turn, you hear a spoken digest of what it did.

## The two halves

- **engine/** is the whole product on its own: a local daemon that cleans agent output, synthesizes speech with [mlx-audio](https://github.com/Blaizzy/mlx-audio) (Kokoro by default), and plays narrations one at a time. Fully usable from the CLI (`incant install`, `incant doctor`, `incant mode`, `incant mute`, `incant sessions`, …). See [engine/README.md](engine/README.md).
- **app/** is a thin SwiftUI client of the engine's HTTP + SSE API. It renders live sessions as menu-bar controls and floating chat-head bubbles, and carries the onboarding. See [app/README.md](app/README.md).

Incant is speech-out only and deliberately speech-to-text agnostic: pair it with [Hex](https://github.com/kitlangton/Hex) (free/OSS), superwhisper, Wispr Flow, or any dictation tool to talk back to your agents. The onboarding lists options.

## Requirements

Apple Silicon Mac (the TTS engine is MLX-based) and Python 3.10+. On other machines, point the engine at any OpenAI-compatible `/v1/audio/speech` endpoint (see engine remote mode).

## License

MIT
