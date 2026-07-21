"""Config loading for incant.

Config lives at ~/.config/incant/config.toml. Every field has a working
default so a fresh install needs no config file at all. The daemon
re-reads the file before each narration, so edits apply live.
"""

from __future__ import annotations

import os
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

CONFIG_DIR = Path(os.environ.get("INCANT_CONFIG_DIR", "~/.config/incant")).expanduser()
CONFIG_PATH = CONFIG_DIR / "config.toml"
STATE_DIR = Path(os.environ.get("INCANT_STATE_DIR", "~/.local/state/incant")).expanduser()

NARRATION_MODES = ("full", "tldr", "summary")
NARRATION_BEHAVIORS = ("auto", "notify", "off")
ACTIVE_WINDOW = 1800.0  # seconds a session stays "active" after its last turn

# Distinct default voices so consecutive narrations are audibly
# attributable to their agent (playback is always one at a time).
DEFAULT_VOICES = {
    "claude": "af_heart",
    "codex": "am_michael",
    "opencode": "bf_emma",
    "kimi": "bm_george",
}

DEFAULT_CONFIG_TOML = """\
# incant configuration. Every key is optional; these are the defaults.

[daemon]
port = 5111

[tts]
# "managed": incant runs mlx-audio's server for you (Apple Silicon).
# "remote": point at any OpenAI-compatible /v1/audio/speech endpoint.
mode = "managed"
port = 5112                      # port for the managed mlx-audio server
# url = "https://example.com"    # base URL when mode = "remote"
# api_key = ""                   # bearer token for remote endpoints
model = "mlx-community/Kokoro-82M-bf16"
voice = "af_heart"               # default voice for sources not in [voices]
speed = 1.1

[voices]
# Per-agent voices. Playback is serialized (one narration at a time);
# distinct voices tell you which agent is speaking without looking.
claude = "af_heart"
codex = "am_michael"
opencode = "bf_emma"
kimi = "bm_george"

[speech]
# How much of a reply becomes speech:
#   "full"    - speak the cleaned reply (truncated at max_chars)
#   "tldr"    - if the reply ends with a "TLDR: ..." line, speak only that;
#               otherwise fall back to "full". Pair with an agent snippet
#               (see README) that asks for a final TLDR line.
#   "summary" - compress long replies with the [summarizer] LLM
mode = "full"
max_chars = 700

[narration]
# Whether/when a finished turn speaks (the global default; override per
# agent below, or per session at runtime from the menu bar):
#   "auto"   - speak immediately
#   "notify" - stay silent, mark the session unread; speak on demand
#   "off"    - never speak
behavior = "auto"

[narration.providers]
# Per-agent overrides, e.g.:
# codex = "notify"

[summarizer]
# An OpenAI-compatible chat endpoint (e.g. a local llama-server) used by
# "summary" mode. Without it, "summary" behaves like "full".
# url = "http://127.0.0.1:8080"
# model = "default"
"""


@dataclass
class Config:
    daemon_port: int = 5111
    tts_mode: str = "managed"
    tts_port: int = 5112
    tts_url: str = ""
    tts_api_key: str = ""
    tts_model: str = "mlx-community/Kokoro-82M-bf16"
    tts_voice: str = "af_heart"
    tts_speed: float = 1.1
    voices: dict = field(default_factory=lambda: dict(DEFAULT_VOICES))
    speech_mode: str = "full"
    max_chars: int = 700
    behavior: str = "auto"
    provider_behaviors: dict = field(default_factory=dict)
    summarizer_url: str = ""
    summarizer_model: str = "default"
    raw: dict = field(default_factory=dict)

    def behavior_for(self, source: str) -> str:
        return self.provider_behaviors.get(source, self.behavior)

    @property
    def tts_base_url(self) -> str:
        if self.tts_mode == "remote" and self.tts_url:
            return self.tts_url.rstrip("/")
        return f"http://127.0.0.1:{self.tts_port}"

    @property
    def daemon_url(self) -> str:
        return f"http://127.0.0.1:{self.daemon_port}"

    def voice_for(self, source: str) -> str:
        return self.voices.get(source, self.tts_voice)


def load_config() -> Config:
    cfg = Config()
    if CONFIG_PATH.exists():
        data = tomllib.loads(CONFIG_PATH.read_text())
        cfg.raw = data
        daemon = data.get("daemon", {})
        tts = data.get("tts", {})
        speech = data.get("speech", {})
        summ = data.get("summarizer", {})
        cfg.daemon_port = int(daemon.get("port", cfg.daemon_port))
        cfg.tts_mode = tts.get("mode", cfg.tts_mode)
        cfg.tts_port = int(tts.get("port", cfg.tts_port))
        cfg.tts_url = tts.get("url", cfg.tts_url)
        cfg.tts_api_key = tts.get("api_key", cfg.tts_api_key)
        cfg.tts_model = tts.get("model", cfg.tts_model)
        cfg.tts_voice = tts.get("voice", cfg.tts_voice)
        cfg.tts_speed = float(tts.get("speed", cfg.tts_speed))
        cfg.voices.update(data.get("voices", {}))
        mode = speech.get("mode", cfg.speech_mode)
        cfg.speech_mode = mode if mode in NARRATION_MODES else "full"
        cfg.max_chars = int(speech.get("max_chars", cfg.max_chars))
        narr = data.get("narration", {})
        behavior = narr.get("behavior", cfg.behavior)
        cfg.behavior = behavior if behavior in NARRATION_BEHAVIORS else "auto"
        cfg.provider_behaviors = {
            source: value
            for source, value in narr.get("providers", {}).items()
            if value in NARRATION_BEHAVIORS
        }
        cfg.summarizer_url = summ.get("url", cfg.summarizer_url)
        cfg.summarizer_model = summ.get("model", cfg.summarizer_model)
    if os.environ.get("INCANT_PORT"):
        cfg.daemon_port = int(os.environ["INCANT_PORT"])
    return cfg


def write_default_config() -> Path:
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    if not CONFIG_PATH.exists():
        CONFIG_PATH.write_text(DEFAULT_CONFIG_TOML)
    return CONFIG_PATH


def set_config_key(section: str, key: str, value) -> None:
    """Set one key in config.toml, preserving everything else.

    Line-based edit so user comments survive; the file is small and the
    grammar we touch is limited to scalar keys inside [section].
    """
    import re

    write_default_config()
    text = CONFIG_PATH.read_text()
    rendered = f'"{value}"' if isinstance(value, str) else str(value)
    section_re = re.compile(rf"^\[{re.escape(section)}\]\s*$", re.MULTILINE)
    match = section_re.search(text)
    if not match:
        text = text.rstrip("\n") + f"\n\n[{section}]\n{key} = {rendered}\n"
        CONFIG_PATH.write_text(text)
        return
    start = match.end()
    next_section = re.search(r"^\[", text[start:], re.MULTILINE)
    end = start + next_section.start() if next_section else len(text)
    body = text[start:end]
    key_re = re.compile(rf"^(\s*{re.escape(key)}\s*=\s*).*$", re.MULTILINE)
    if key_re.search(body):
        body = key_re.sub(rf"\g<1>{rendered}", body, count=1)
    else:
        body = "\n" + f"{key} = {rendered}" + body
    CONFIG_PATH.write_text(text[:start] + body + text[end:])


def is_apple_silicon() -> bool:
    import platform

    return sys.platform == "darwin" and platform.machine() == "arm64"
