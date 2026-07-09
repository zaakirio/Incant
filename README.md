<p align="center">
  <img src="assets/banner.png" alt="Incant" width="820">
</p>

<p align="center">
  <strong>Give your coding agents a voice.</strong><br>
  Local text-to-speech narration for Claude Code, Codex, and OpenCode — on your Mac, no cloud, no API keys.
</p>

<p align="center">
  <a href="#requirements"><img src="https://img.shields.io/badge/platform-macOS-1f1f1f?style=flat-square&logo=apple&logoColor=white" alt="Platform: macOS"></a>
  <a href="#requirements"><img src="https://img.shields.io/badge/chip-Apple%20Silicon-7c5cf0?style=flat-square" alt="Apple Silicon"></a>
  <a href="engine/"><img src="https://img.shields.io/badge/engine-Python%203.10%2B-7c5cf0?style=flat-square&logo=python&logoColor=white" alt="Engine: Python 3.10+"></a>
  <a href="app/"><img src="https://img.shields.io/badge/app-SwiftUI-7c5cf0?style=flat-square&logo=swift&logoColor=white" alt="App: SwiftUI"></a>
  <a href="https://github.com/Blaizzy/mlx-audio"><img src="https://img.shields.io/badge/speech-MLX%20%C2%B7%20Kokoro-7c5cf0?style=flat-square" alt="Speech: MLX / Kokoro"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-3b82f6?style=flat-square" alt="License: MIT"></a>
</p>

---

Incant speaks each finished turn from **Claude Code**, **Codex**, and **OpenCode** through fast local text-to-speech, with a native macOS menu bar app and floating per-session bubbles.
Everything runs on your machine.

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
- **app/** is a thin SwiftUI client of the engine's HTTP + SSE API. It renders live sessions as menu-bar controls and floating chat-head bubbles, with per-agent voices, hover-activated provider logos, and onboarding. See [app/README.md](app/README.md).

Incant is speech-out only and deliberately speech-to-text agnostic: pair it with [Hex](https://github.com/kitlangton/Hex) (free/OSS), superwhisper, Wispr Flow, or any dictation tool to talk back to your agents. The onboarding lists options.

## Requirements

Apple Silicon Mac (the TTS engine is MLX-based) and Python 3.10+.
On other machines, point the engine at any OpenAI-compatible `/v1/audio/speech` endpoint (see engine remote mode).

## License

MIT
