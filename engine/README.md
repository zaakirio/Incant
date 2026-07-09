# incant

Give your coding agents a voice.
incant speaks each finished turn from **Claude Code**, **Codex**, and **OpenCode** through fast local text-to-speech.
Single purpose by design: agent turns become speech, nothing else.
Everything runs on your machine; no cloud, no API keys.

## How it works

```
Claude Code (Stop hook) ─┐
Codex (notify)           ├─> incant daemon ──> mlx-audio TTS ──> your speakers
OpenCode (plugin)        ─┘   (clean, digest, queue)
```

Each agent already has a "turn finished" hook.
`incant install` wires them to a small local daemon that strips code blocks and markdown, applies your narration mode, synthesizes speech with [mlx-audio](https://github.com/Blaizzy/mlx-audio) (Kokoro by default), and plays narrations strictly one at a time so parallel sessions never talk over each other.
Each agent speaks in its own voice, so you know who finished without looking.

## Requirements

- Apple Silicon Mac (M1 or later). The TTS engine is MLX-based.
  On other machines, point incant at any OpenAI-compatible `/v1/audio/speech` endpoint instead (see Remote mode).
- Python 3.10+

## Install

```bash
uv tool install incant   # or: pipx install incant / pip install incant
incant install
```

`incant install` walks you through it: pick which agents to hook, confirm the three files it edits, then it starts the daemon, downloads the voice model (~300 MB, first run only), and finishes by speaking out loud.
If you heard it, you're done - the next agent turn narrates automatically.
Use `incant install --yes` for a promptless install, and `incant doctor` any time something seems off.

## Commands

```
incant install [tool...]   guided setup (claude / codex / opencode)
incant uninstall [tool...] remove the hooks cleanly
incant mode [full|tldr|summary]   get or set the narration mode
incant mute [30m]          drop narrations, optionally for a duration
incant unmute              resume narrations
incant skip                stop talking right now and clear the queue
incant speak TEXT          say something verbatim
incant narrate TEXT        preview the narration pipeline on any text
incant status              daemon, mode, voices, and hook status
incant doctor              check every part of the pipeline
incant serve               run the daemon in the foreground (it self-starts otherwise)
```

## Narration behavior (auto / notify / off)

Separate from *how much* is spoken is *whether and when* a turn speaks - useful for long-horizon tasks you'd rather not hear every step of:

1. `auto` (default) - speak each finished turn immediately.
2. `notify` - stay silent, mark the session as having something unread; speak it on demand.
3. `off` - never speak.

Set the global default with `incant behavior notify`, or per agent in config:

```toml
[narration]
behavior = "auto"

[narration.providers]
codex = "notify"
```

Each running agent is tracked as a session; `incant sessions` lists them with their project, behavior, and unread state. Per-session overrides (and everything else) are what the forthcoming menu bar app drives over the daemon's HTTP + SSE API.

## Narration modes

Agent replies vary from one line to a wall of analysis. Pick how much you want to hear:

1. `full` (default) - speak the cleaned reply, truncated at sentence boundaries past `max_chars`.
2. `tldr` - if the reply ends with a `TLDR:` line, speak only that; otherwise same as `full`.
3. `summary` - compress long replies to two spoken sentences with a local LLM (needs `[summarizer]` configured).

`tldr` mode works best when your agents cooperate. Paste this into your `CLAUDE.md` / `AGENTS.md`:

```
## Spoken updates
End every reply with a final line:
TLDR: <what you did or found, max 20 words, plain speech, no code or paths>
```

Then `incant mode tldr`. Every turn now ends with one crisp spoken sentence.
Also worth having in your agent config regardless of mode: an instruction to be concise (e.g. "when reporting information, be extremely concise") - shorter replies make better listening.

## Voices

Each agent gets its own voice so consecutive narrations are audibly attributable (playback is always serialized - one speaker at a time):

```toml
[voices]
claude = "af_heart"
codex = "am_michael"
opencode = "bf_emma"
```

List what's available: `curl 'localhost:5112/v1/audio/voices?model=mlx-community/Kokoro-82M-bf16'`.
Any TTS model mlx-audio supports works via `[tts] model`, including voice-cloning models.

## Configuration

`~/.config/incant/config.toml`, created on install. Speech settings apply live - no restart:

```toml
[tts]
model = "mlx-community/Kokoro-82M-bf16"
voice = "af_heart"      # default for sources without a [voices] entry
speed = 1.1

[speech]
mode = "full"           # full | tldr | summary
max_chars = 700

[summarizer]
# used by "summary" mode; any OpenAI-compatible chat endpoint
# url = "http://127.0.0.1:8080"
# model = "default"
```

### Remote mode

Point incant at any OpenAI-compatible speech endpoint instead of the managed local server, e.g. a GPU box running a bigger model:

```toml
[tts]
mode = "remote"
url = "https://your-gpu-box:8000"
api_key = "..."
model = "your-model"
```

## Dictation (the other direction)

incant deliberately has no microphone or speech-to-text features.
For push-to-talk dictation into your agents, pair it with [Hex](https://github.com/kitlangton/Hex) - hold a hotkey, talk, and the transcript lands wherever your cursor is:

```bash
brew install --cask kitlangton-hex
```

Together they close the loop: you speak to the agent through Hex, the agent speaks back through incant.

## Uninstall

```bash
incant uninstall
uv tool uninstall incant
```

Hooks are removed from `~/.claude/settings.json`, `~/.codex/config.toml`, and the OpenCode plugin directory.
Model weights live in the Hugging Face cache (`~/.cache/huggingface`) if you want those gone too.

## License

MIT
