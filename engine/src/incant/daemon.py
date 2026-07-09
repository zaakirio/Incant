"""The incant daemon: receives text, produces speech, tracks sessions.

One FastAPI app on localhost, with three responsibilities:

1. Audio - synthesize each narration with mlx-audio and play it, strictly
   one at a time (serialized), each agent in its own voice.
2. Sessions - track every active agent session (source + id + cwd + pid),
   its narration behavior (auto / notify / off), and its unread state.
3. Events - a Server-Sent Events stream so a UI can render live session
   state without polling.

Speech settings and narration behavior are re-read from the config file
per turn, so edits apply live. This module is the contract the menu bar
app and bubble overlay build on; keep the /sessions and /events shapes
stable.
"""

from __future__ import annotations

import asyncio
import contextlib
import json
import logging
import math
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from collections import deque
from dataclasses import dataclass, field
from pathlib import Path

import httpx
import uvicorn
from fastapi import FastAPI, Request
from fastapi.responses import StreamingResponse
from pydantic import BaseModel

from .config import ACTIVE_WINDOW, NARRATION_BEHAVIORS, STATE_DIR, Config, load_config
from .textwork import prepare_narration

log = logging.getLogger("incant")

MAX_QUEUE = 10
HISTORY = 20
FIRST_SYNTH_TIMEOUT = 600.0  # first call may download model weights
SYNTH_TIMEOUT = 120.0


# -- request models ----------------------------------------------------


class NarrateBody(BaseModel):
    text: str
    source: str = "unknown"
    session_id: str | None = None
    cwd: str | None = None
    pid: int | None = None
    voice: str | None = None


class MuteBody(BaseModel):
    seconds: float | None = None


class BehaviorBody(BaseModel):
    behavior: str | None = None  # auto | notify | off; null clears the override


class ConfigPatch(BaseModel):
    mode: str | None = None
    behavior: str | None = None
    voice: str | None = None
    speed: float | None = None
    max_chars: int | None = None
    voices: dict[str, str] | None = None


# -- events ------------------------------------------------------------


class EventHub:
    """Fan-out of state-change events to any number of SSE subscribers."""

    def __init__(self) -> None:
        self._subscribers: set[asyncio.Queue] = set()

    def subscribe(self) -> asyncio.Queue:
        q: asyncio.Queue = asyncio.Queue(maxsize=100)
        self._subscribers.add(q)
        return q

    def unsubscribe(self, q: asyncio.Queue) -> None:
        self._subscribers.discard(q)

    def publish(self, event: dict) -> None:
        for q in list(self._subscribers):
            try:
                q.put_nowait(event)
            except asyncio.QueueFull:
                pass


# -- sessions ----------------------------------------------------------


@dataclass
class Session:
    source: str
    session_id: str
    cwd: str = ""
    pid: int | None = None
    behavior_override: str | None = None
    unread: bool = False
    pending_text: str | None = None  # unspoken narration (notify mode)
    last_seen: float = 0.0
    speaking: bool = False
    _dedup_text: str = ""
    _dedup_at: float = 0.0
    history: deque = field(default_factory=lambda: deque(maxlen=HISTORY))

    @property
    def key(self) -> str:
        return f"{self.source}:{self.session_id}"

    @property
    def project(self) -> str:
        return os.path.basename(self.cwd.rstrip("/")) or self.cwd or "?"

    @property
    def can_kill(self) -> bool:
        return self.pid is not None

    def to_dict(self, effective_behavior: str) -> dict:
        return {
            "key": self.key,
            "source": self.source,
            "session_id": self.session_id,
            "cwd": self.cwd,
            "project": self.project,
            "pid": self.pid,
            "can_kill": self.can_kill,
            "behavior": effective_behavior,
            "behavior_override": self.behavior_override,
            "unread": self.unread,
            "speaking": self.speaking,
            "last_seen": self.last_seen,
            "last_text": self.history[-1]["text"] if self.history else None,
            "history": list(self.history),
        }


# -- audio -------------------------------------------------------------


@dataclass
class Narration:
    text: str
    source: str
    verbatim: bool = False
    session_key: str | None = None
    voice: str | None = None  # override; used for voice auditions


class Speaker:
    """Serialized synth-then-play pipeline."""

    def __init__(self, cfg: Config, state: State):
        self.cfg = cfg  # startup config: ports and tts mode only
        self.state = state
        self.queue: deque[Narration] = deque()
        self.wakeup = asyncio.Event()
        self.player_proc: subprocess.Popen | None = None
        self.synthesized_once = False
        self.mlx_proc: subprocess.Popen | None = None

    def _now(self) -> float:
        return asyncio.get_event_loop().time()

    def enqueue(self, item: Narration) -> None:
        while len(self.queue) >= MAX_QUEUE:
            self.queue.popleft()
        self.queue.append(item)
        self.wakeup.set()

    def skip(self) -> int:
        dropped = len(self.queue)
        self.queue.clear()
        if self.player_proc and self.player_proc.poll() is None:
            self.player_proc.terminate()
            dropped += 1
        return dropped

    # -- managed mlx-audio server ------------------------------------

    def start_managed_tts(self) -> None:
        if self.cfg.tts_mode != "managed":
            return
        STATE_DIR.mkdir(parents=True, exist_ok=True)
        logfile = (STATE_DIR / "mlx-audio.log").open("ab")
        self.mlx_proc = subprocess.Popen(
            [
                sys.executable,
                "-m",
                "mlx_audio.server",
                "--host",
                "127.0.0.1",
                "--port",
                str(self.cfg.tts_port),
                "--log-dir",
                str(STATE_DIR / "mlx-logs"),
            ],
            cwd=STATE_DIR,
            stdout=logfile,
            stderr=logfile,
        )
        log.info("managed mlx-audio server starting on port %d (pid %d)", self.cfg.tts_port, self.mlx_proc.pid)

    def stop_managed_tts(self) -> None:
        if self.mlx_proc and self.mlx_proc.poll() is None:
            self.mlx_proc.terminate()
            with contextlib.suppress(subprocess.TimeoutExpired):
                self.mlx_proc.wait(timeout=5)
            if self.mlx_proc.poll() is None:
                self.mlx_proc.kill()

    async def wait_for_tts(self, timeout: float = 60.0) -> bool:
        deadline = self._now() + timeout
        async with httpx.AsyncClient() as client:
            while self._now() < deadline:
                try:
                    resp = await client.get(self.cfg.tts_base_url + "/", timeout=2.0)
                    if resp.status_code < 500:
                        return True
                except Exception:
                    pass
                await asyncio.sleep(0.5)
        return False

    # -- synthesis + playback ----------------------------------------

    async def synthesize(self, text: str, voice: str, cfg: Config) -> Path | None:
        headers = {}
        if cfg.tts_api_key:
            headers["Authorization"] = f"Bearer {cfg.tts_api_key}"
        timeout = SYNTH_TIMEOUT if self.synthesized_once else FIRST_SYNTH_TIMEOUT
        payload = {
            "model": cfg.tts_model,
            "input": text,
            "voice": voice,
            "speed": cfg.tts_speed,
            "response_format": "wav",
            "stream": False,
        }
        audio = b""
        # Kokoro occasionally crashes mid-stream on specific text/speed
        # combinations; a retry almost always succeeds.
        for attempt in (1, 2):
            try:
                async with httpx.AsyncClient() as client:
                    resp = await client.post(
                        self.cfg.tts_base_url + "/v1/audio/speech",
                        json=payload,
                        headers=headers,
                        timeout=timeout,
                    )
                    resp.raise_for_status()
                    audio = resp.content
                break
            except Exception as exc:
                log.error("TTS synthesis failed (attempt %d): %s", attempt, exc)
        if not audio:
            return None
        self.synthesized_once = True
        fd, name = tempfile.mkstemp(prefix="incant-", suffix=".wav")
        Path(name).write_bytes(audio)
        os.close(fd)
        return Path(name)

    def _player_cmd(self, path: Path) -> list[str] | None:
        if sys.platform == "darwin" and shutil.which("afplay"):
            return ["afplay", str(path)]
        if shutil.which("ffplay"):
            return ["ffplay", "-nodisp", "-autoexit", "-loglevel", "quiet", str(path)]
        if shutil.which("aplay"):
            return ["aplay", "-q", str(path)]
        return None

    async def play(self, path: Path) -> None:
        cmd = self._player_cmd(path)
        if not cmd:
            log.error("no audio player found (afplay/ffplay/aplay)")
            return
        self.player_proc = subprocess.Popen(cmd)
        try:
            while self.player_proc.poll() is None:
                await asyncio.sleep(0.1)
        finally:
            path.unlink(missing_ok=True)
            self.player_proc = None

    async def worker(self) -> None:
        while True:
            if not self.queue:
                self.wakeup.clear()
                await self.wakeup.wait()
                continue
            item = self.queue.popleft()
            cfg = await asyncio.to_thread(load_config)
            if item.verbatim:
                text = item.text.strip()
            else:
                text = await asyncio.to_thread(
                    prepare_narration,
                    item.text,
                    cfg.max_chars,
                    cfg.speech_mode,
                    cfg.summarizer_url,
                    cfg.summarizer_model,
                )
            if not text:
                continue
            voice = item.voice or cfg.voice_for(item.source)
            log.info("narrating (%s, %s, %d chars): %.80s", item.source, voice, len(text), text)
            self.state.set_speaking(item.session_key, True)
            if not self.synthesized_once and not await self.wait_for_tts():
                log.error("TTS server never became ready; dropping narration")
                self.state.set_speaking(item.session_key, False)
                continue
            audio_path = await self.synthesize(text, voice, cfg)
            if audio_path:
                await self.play(audio_path)
            self.state.set_speaking(item.session_key, False)


# -- central state -----------------------------------------------------


class State:
    """Owns the session registry, the event hub, and mute state; the
    Speaker owns audio and calls back here for lifecycle events."""

    def __init__(self, cfg: Config):
        self.cfg = cfg
        self.events = EventHub()
        self.speaker = Speaker(cfg, self)
        self.sessions: dict[str, Session] = {}
        self.muted_until: float | None = None

    # mute ------------------------------------------------------------

    def _mono(self) -> float:
        return asyncio.get_event_loop().time()

    @property
    def muted(self) -> bool:
        if self.muted_until is None:
            return False
        if self.muted_until <= self._mono():
            self.muted_until = None
            return False
        return True

    def mute(self, seconds: float | None) -> None:
        self.muted_until = math.inf if seconds is None else self._mono() + seconds
        self.events.publish({"type": "mute.changed", "muted": True})

    def unmute(self) -> None:
        self.muted_until = None
        self.events.publish({"type": "mute.changed", "muted": False})

    # sessions --------------------------------------------------------

    def effective_behavior(self, session: Session, cfg: Config | None = None) -> str:
        if session.behavior_override in NARRATION_BEHAVIORS:
            return session.behavior_override
        cfg = cfg or load_config()
        return cfg.behavior_for(session.source)

    def sweep(self) -> None:
        now = time.time()
        for key, session in list(self.sessions.items()):
            if now - session.last_seen > ACTIVE_WINDOW:
                del self.sessions[key]
                self.events.publish({"type": "session.removed", "key": key})

    def _emit_session(self, session: Session, cfg: Config | None = None) -> None:
        self.events.publish(
            {"type": "session.updated", "session": session.to_dict(self.effective_behavior(session, cfg))}
        )

    def set_speaking(self, key: str | None, value: bool) -> None:
        if not key:
            return
        session = self.sessions.get(key)
        if not session:
            return
        session.speaking = value
        self.events.publish(
            {"type": "narration.started" if value else "narration.finished", "key": key}
        )
        self._emit_session(session)

    def handle_narration(self, body: NarrateBody) -> dict:
        cfg = load_config()

        # Sourceless / CLI narrations have no session; keep the old
        # behavior: speak unless muted.
        if not body.session_id:
            if self.muted:
                log.info("muted, dropping narration (%s)", body.source)
                return {"queued": False}
            self.speaker.enqueue(Narration(text=body.text, source=body.source))
            return {"queued": True}

        session = self.sessions.get(f"{body.source}:{body.session_id}")
        if session is None:
            session = Session(source=body.source, session_id=body.session_id)
            self.sessions[session.key] = session
        if body.cwd:
            session.cwd = body.cwd
        if body.pid:
            session.pid = body.pid
        session.last_seen = time.time()

        # Per-session dedup: agents sometimes fire the hook twice per turn.
        now = self._mono()
        if body.text == session._dedup_text and now - session._dedup_at < 90.0:
            log.info("dropping duplicate narration (%s)", session.key)
            self._emit_session(session, cfg)
            return {"queued": False, "duplicate": True}
        session._dedup_text = body.text
        session._dedup_at = now

        behavior = self.effective_behavior(session, cfg)
        muted = self.muted

        if behavior == "off":
            self._emit_session(session, cfg)
            return {"queued": False, "behavior": "off"}

        if behavior == "notify" or muted:
            # Hold as unread; the UI (or `incant read`) speaks it on demand.
            session.pending_text = body.text
            session.unread = True
            self._emit_session(session, cfg)
            return {"queued": False, "held": True, "behavior": behavior, "muted": muted}

        # auto
        session.history.append({"text": body.text, "at": time.time()})
        session.pending_text = None
        session.unread = False
        self.speaker.enqueue(Narration(text=body.text, source=body.source, session_key=session.key))
        self._emit_session(session, cfg)
        return {"queued": True, "behavior": "auto"}

    def mark_read(self, key: str, speak: bool = True) -> dict:
        session = self.sessions.get(key)
        if not session:
            return {"ok": False, "error": "no such session"}
        session.unread = False
        spoke = False
        if speak and session.pending_text:
            session.history.append({"text": session.pending_text, "at": time.time()})
            self.speaker.enqueue(
                Narration(text=session.pending_text, source=session.source, session_key=key)
            )
            session.pending_text = None
            spoke = True
        self._emit_session(session)
        return {"ok": True, "spoke": spoke}

    def replay(self, key: str) -> dict:
        session = self.sessions.get(key)
        if not session or not session.history:
            return {"ok": False, "error": "nothing to replay"}
        text = session.history[-1]["text"]
        self.speaker.enqueue(Narration(text=text, source=session.source, session_key=key))
        return {"ok": True}

    def set_behavior(self, key: str, behavior: str | None) -> dict:
        session = self.sessions.get(key)
        if not session:
            return {"ok": False, "error": "no such session"}
        if behavior is not None and behavior not in NARRATION_BEHAVIORS:
            return {"ok": False, "error": f"behavior must be one of {NARRATION_BEHAVIORS}"}
        session.behavior_override = behavior
        self._emit_session(session)
        return {"ok": True, "behavior": self.effective_behavior(session)}

    def kill(self, key: str) -> dict:
        session = self.sessions.get(key)
        if not session:
            return {"ok": False, "error": "no such session"}
        if session.pid is None:
            return {"ok": False, "error": "session has no killable process"}
        try:
            os.killpg(os.getpgid(session.pid), signal.SIGTERM)
        except ProcessLookupError:
            pass  # already gone; fall through to cleanup
        except Exception as exc:
            return {"ok": False, "error": str(exc)}
        del self.sessions[key]
        self.events.publish({"type": "session.removed", "key": key})
        return {"ok": True, "killed_pid": session.pid}

    def session_list(self) -> list[dict]:
        self.sweep()
        cfg = load_config()
        return [s.to_dict(self.effective_behavior(s, cfg)) for s in self.sessions.values()]


# -- app ---------------------------------------------------------------


def create_app(cfg: Config | None = None) -> FastAPI:
    cfg = cfg or load_config()
    state = State(cfg)
    app = FastAPI(title="incant")

    @app.on_event("startup")
    async def startup() -> None:
        state.speaker.start_managed_tts()
        asyncio.get_event_loop().create_task(state.speaker.worker())

    @app.on_event("shutdown")
    async def shutdown() -> None:
        state.speaker.stop_managed_tts()

    @app.get("/health")
    async def health() -> dict:
        live = load_config()
        state.sweep()
        return {
            "ok": True,
            "queue": len(state.speaker.queue),
            "muted": state.muted,
            "mode": live.speech_mode,
            "behavior": live.behavior,
            "sessions": len(state.sessions),
            "tts_mode": cfg.tts_mode,
            "tts_url": cfg.tts_base_url,
        }

    @app.get("/sessions")
    async def sessions() -> dict:
        return {"sessions": state.session_list()}

    @app.get("/events")
    async def events(request: Request) -> StreamingResponse:
        q = state.events.subscribe()

        async def stream():
            # Prime the client with a full snapshot, then stream deltas.
            snapshot = {"type": "snapshot", "sessions": state.session_list(), "muted": state.muted}
            yield f"data: {json.dumps(snapshot)}\n\n"
            try:
                while True:
                    if await request.is_disconnected():
                        break
                    try:
                        event = await asyncio.wait_for(q.get(), timeout=15.0)
                        yield f"data: {json.dumps(event)}\n\n"
                    except asyncio.TimeoutError:
                        yield ": keepalive\n\n"
            finally:
                state.events.unsubscribe(q)

        return StreamingResponse(stream(), media_type="text/event-stream")

    @app.post("/narrate")
    async def narrate(body: NarrateBody) -> dict:
        return state.handle_narration(body)

    @app.post("/say")
    async def say(body: NarrateBody) -> dict:
        # Auditions (voice override) bypass mute so the UI always previews.
        if state.muted and not body.voice:
            return {"queued": False}
        state.speaker.enqueue(
            Narration(text=body.text, source=body.source, verbatim=True, voice=body.voice)
        )
        return {"queued": True}

    @app.get("/config")
    async def get_config() -> dict:
        live = load_config()
        return {
            "mode": live.speech_mode,
            "behavior": live.behavior,
            "voice": live.tts_voice,
            "speed": live.tts_speed,
            "max_chars": live.max_chars,
            "voices": live.voices,
            "tts_model": live.tts_model,
            "provider_behaviors": live.provider_behaviors,
        }

    @app.post("/config")
    async def patch_config(body: ConfigPatch) -> dict:
        from .config import NARRATION_BEHAVIORS, NARRATION_MODES, set_config_key

        if body.mode in NARRATION_MODES:
            set_config_key("speech", "mode", body.mode)
        if body.max_chars is not None:
            set_config_key("speech", "max_chars", int(body.max_chars))
        if body.behavior in NARRATION_BEHAVIORS:
            set_config_key("narration", "behavior", body.behavior)
        if body.voice:
            set_config_key("tts", "voice", body.voice)
        if body.speed is not None:
            set_config_key("tts", "speed", float(body.speed))
        if body.voices:
            for source, voice in body.voices.items():
                set_config_key("voices", source, voice)
        return await get_config()

    @app.get("/doctor")
    async def doctor() -> dict:
        from .checks import doctor_checks

        checks = await asyncio.to_thread(doctor_checks)
        return {"checks": checks, "ok": all(c["ok"] for c in checks)}

    @app.get("/voices")
    async def voices_available() -> dict:
        live = load_config()
        try:
            async with httpx.AsyncClient() as client:
                resp = await client.get(
                    cfg.tts_base_url + "/v1/audio/voices",
                    params={"model": live.tts_model},
                    timeout=10.0,
                )
                resp.raise_for_status()
                data = resp.json()
            return {"voices": [v["id"] for v in data.get("data", [])]}
        except Exception as exc:
            return {"voices": [], "error": str(exc)}

    @app.post("/skip")
    async def skip() -> dict:
        return {"dropped": state.speaker.skip()}

    @app.post("/mute")
    async def mute(body: MuteBody) -> dict:
        state.mute(body.seconds)
        return {"muted": True, "seconds": body.seconds}

    @app.post("/unmute")
    async def unmute() -> dict:
        state.unmute()
        return {"muted": False}

    @app.post("/sessions/{key}/read")
    async def read(key: str) -> dict:
        return state.mark_read(key)

    @app.post("/sessions/{key}/replay")
    async def replay(key: str) -> dict:
        return state.replay(key)

    @app.post("/sessions/{key}/behavior")
    async def behavior(key: str, body: BehaviorBody) -> dict:
        return state.set_behavior(key, body.behavior)

    @app.post("/sessions/{key}/kill")
    async def kill(key: str) -> dict:
        return state.kill(key)

    return app


def run_daemon(foreground: bool = True) -> None:
    cfg = load_config()
    STATE_DIR.mkdir(parents=True, exist_ok=True)
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(name)s %(levelname)s %(message)s",
    )
    uvicorn.run(
        create_app(cfg),
        host="127.0.0.1",
        port=cfg.daemon_port,
        log_level="warning",
    )
