# incant

Give your coding agents a voice.
incant speaks each finished turn from **Claude Code**, **Codex**, **OpenCode**, and **Kimi CLI** through fast local text-to-speech, and tracks what every session is doing in between: working, waiting for your approval, asking you a question, or fanning out subagents.
Everything runs on your machine; no cloud, no API keys.

## How it works

```
Claude Code (hooks)    ─┐
Codex (notify + hooks)  ├─> incant daemon ──> mlx-audio TTS ──> your speakers
OpenCode (plugin)       │   (clean, digest, queue,
Kimi CLI (hooks)       ─┘    live session status)
```

Each agent has a "turn finished" hook plus a set of lifecycle hooks.
`incant install` wires them to a small local daemon that strips code blocks and markdown, applies your narration mode, synthesizes speech with [mlx-audio](https://github.com/Blaizzy/mlx-audio) (Kokoro by default), and plays narrations strictly one at a time so parallel sessions never talk over each other.
Each agent speaks in its own voice, so you know who finished without looking.
The same hooks feed a live status per session (working / needs approval / needs input / subagent count) that the menu bar app renders and turns into macOS notifications.

## Requirements

- Apple Silicon Mac (M1 or later). The TTS engine is MLX-based.
  On other machines, point incant at any OpenAI-compatible `/v1/audio/speech` endpoint instead (see Remote mode).
- Python 3.10+

## Install

```bash
uv tool install incant   # or: pipx install incant / pip install incant
incant install
```

`incant install` walks you through it: pick which agents to hook, confirm the files it edits, then it starts the daemon, downloads the voice model (~300 MB, first run only), and finishes by speaking out loud.
If you heard it, you're done - the next agent turn narrates automatically.
Use `incant install --yes` for a promptless install, and `incant doctor` any time something seems off.

## Commands

```
incant install [tool...]   guided setup (claude / codex / opencode / kimi)
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

Each running agent is tracked as a session; `incant sessions` lists them with their project, behavior, unread state, live status, and subagent count. Per-session overrides (and everything else) are what the menu bar app drives over the daemon's HTTP + SSE API.

## Live status, approvals, and swarms

Narration covers finished turns; the lifecycle hooks cover everything in between.
Every wired agent reports these signals to the daemon's `/activity` endpoint:

1. **Working** - the turn started (and periodic keepalives while tools run), so the UI can show an in-progress indicator.
2. **Awaiting approval** - the agent is blocked on a permission prompt (Claude Code `PermissionRequest`/`Notification`, Codex `PermissionRequest` hook, OpenCode `permission` events, Kimi Code CLI `PermissionRequest`).
3. **Awaiting input** - the agent asked you a question or went idle waiting for you.
4. **Subagents** - swarm workers starting and stopping (`SubagentStart`/`SubagentStop` on Claude, Codex, and Kimi; child sessions on OpenCode). The parent session carries a live count rather than spawning a bubble per worker, so a 100-agent Kimi swarm stays readable.
5. **Ended** - the session closed, so its bubble disappears immediately.

The daemon exposes all of it on `/sessions` and the `/events` SSE stream (`session.status`, `turn.completed`), which the menu bar app turns into bubble states and macOS notifications.
A finished turn resets its session to idle and clears the subagent count.

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
kimi = "bm_george"
```

List what's available: `curl 'localhost:5112/v1/audio/voices?model=mlx-community/Kokoro-82M-bf16'`.
Any TTS model mlx-audio supports works via `[tts] model`, including voice-cloning models.

## Configuration

Everything lives in one file: **`~/.config/incant/config.toml`**.

- It's created for you on `incant install` with sensible defaults, so you only edit it to change something.
- **Speech settings apply live** - the daemon re-reads the file before every narration, so edits to voices, mode, behavior, and speed take effect on the next turn with no restart. (Only the daemon/TTS *ports* are read once at startup.)
- Every key is optional; anything you omit falls back to the default shown below.
- Prefer not to hand-edit? The menu bar app writes the same file, and the CLI does too: `incant mode tldr`, `incant behavior notify`, `incant mute 30m`.

Here is the full file, annotated:

```toml
[daemon]
port = 5111                      # HTTP/SSE port the app and hooks talk to

[tts]
mode = "managed"                 # "managed" = incant runs the local mlx-audio server;
                                 # "remote"  = use an external endpoint (see Remote mode)
port = 5112                      # port for the managed mlx-audio server
model = "mlx-community/Kokoro-82M-bf16"   # any TTS model mlx-audio supports
voice = "af_heart"               # default voice for agents without a [voices] entry
speed = 1.1                      # 0.7 slow ... 1.5 fast

[voices]
# One voice per agent, so you can tell who's speaking without looking.
claude   = "af_heart"            # American, female
codex    = "am_michael"          # American, male
opencode = "bf_emma"             # British, female
kimi     = "bm_george"           # British, male

[speech]
mode = "full"                    # how MUCH is spoken: full | tldr | summary
max_chars = 700                  # replies longer than this get shortened

[narration]
behavior = "auto"                # WHETHER/WHEN it speaks: auto | notify | off

[narration.providers]
# Per-agent overrides of the behavior above, e.g.:
# codex = "notify"               # Codex stays silent, just flags unread

[summarizer]
# Only used by [speech] mode = "summary". Any OpenAI-compatible chat endpoint.
# url   = "http://127.0.0.1:8080"
# model = "default"
```

### The two independent axes

Incant separates *how much* is spoken from *whether* it speaks - mixing them covers most needs:

- `[speech] mode` - **how much**: `full` (the cleaned reply), `tldr` (only a final `TLDR:` line), `summary` (LLM-compressed).
- `[narration] behavior` - **whether/when**: `auto` (speak now), `notify` (stay silent, mark the session unread, speak on demand), `off` (never).

### Set it up for your workflow

Pick the recipe that matches how you work and edit the file (or use the CLI/menu bar):

- **"I just want short spoken updates."** Add the `TLDR:` snippet above to your `CLAUDE.md`/`AGENTS.md`, then set `[speech] mode = "tldr"` (or run `incant mode tldr`).
- **"Codex should stay quiet, Claude should talk."** Under `[narration.providers]` add `codex = "off"` (or `"notify"`); leave `[narration] behavior = "auto"` for the rest.
- **"I run long autonomous tasks I don't want narrated step by step."** Set `[narration] behavior = "notify"` - sessions go silent and just show an unread dot; click the bubble (or `incant sessions` then the app) to hear the latest on demand. Keep short/interactive sessions on `auto` by flipping them per-session in the menu bar.
- **"I want a British male voice for everything."** Set `[tts] voice = "bm_george"` and clear the `[voices]` table (or set each agent). List voices: `curl 'localhost:5112/v1/audio/voices?model=mlx-community/Kokoro-82M-bf16'`.
- **"Narration talks too fast / too slow."** Adjust `[tts] speed` (0.7-1.5).
- **"I'm going into a meeting."** `incant mute 30m` (drops narrations for 30 minutes; sessions still collect unread), or `incant mute` until you `incant unmute`.
- **"I want a bigger/better voice than my Mac can run."** Use Remote mode below to point at a GPU box.

After any edit, the next agent turn uses it - or run `incant doctor` to confirm the pipeline is healthy.

### Remote mode

Point incant at any OpenAI-compatible speech endpoint instead of the managed local server, e.g. a GPU box running a bigger model:

```toml
[tts]
mode = "remote"
url = "https://your-gpu-box:8000"
api_key = "..."                  # sent as a bearer token
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

Hooks are removed from `~/.claude/settings.json`, `~/.codex/config.toml` + `~/.codex/hooks.json`, the OpenCode plugin directory, and `~/.kimi-code/config.toml` / `~/.kimi/config.toml`.
Model weights live in the Hugging Face cache (`~/.cache/huggingface`) if you want those gone too.

## License

MIT
