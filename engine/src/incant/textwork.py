"""Turning agent output into something worth listening to.

Agent replies are markdown full of code fences, paths, and tables.
Spoken verbatim they are unbearable, so: strip the unspeakable parts,
then either LLM-summarize (if a summarizer endpoint is configured) or
truncate to the first few sentences.
"""

from __future__ import annotations

import re

FENCE_RE = re.compile(r"```.*?```", re.DOTALL)
INLINE_CODE_RE = re.compile(r"`([^`]*)`")
LINK_RE = re.compile(r"\[([^\]]+)\]\([^)]+\)")
URL_RE = re.compile(r"https?://\S+")
TABLE_ROW_RE = re.compile(r"^\s*\|.*\|\s*$", re.MULTILINE)
HEADING_RE = re.compile(r"^#{1,6}\s+", re.MULTILINE)
BULLET_RE = re.compile(r"^\s*[-*+]\s+", re.MULTILINE)
EMPHASIS_RE = re.compile(r"(\*\*|__|\*|_)(?=\S)(.+?)(?<=\S)\1")
PATH_RE = re.compile(r"(?:~|\.{0,2}/)[\w./-]{8,}")
SENTENCE_RE = re.compile(r"(?<=[.!?])\s+")


def clean_for_speech(text: str) -> str:
    text = FENCE_RE.sub(" ", text)
    text = TABLE_ROW_RE.sub(" ", text)
    text = LINK_RE.sub(r"\1", text)
    text = URL_RE.sub("a link", text)
    text = INLINE_CODE_RE.sub(r"\1", text)
    text = HEADING_RE.sub("", text)
    text = BULLET_RE.sub("", text)
    text = EMPHASIS_RE.sub(r"\2", text)
    text = PATH_RE.sub("a file path", text)
    text = re.sub(r"\s+", " ", text).strip()
    return text


def truncate_sentences(text: str, max_chars: int) -> str:
    if len(text) <= max_chars:
        return text
    out: list[str] = []
    used = 0
    for sentence in SENTENCE_RE.split(text):
        if used + len(sentence) > max_chars and out:
            break
        out.append(sentence)
        used += len(sentence) + 1
    result = " ".join(out).strip()
    if len(result) > max_chars:
        result = result[: max_chars - 1].rsplit(" ", 1)[0] + "…"
    return result


SUMMARIZE_SYSTEM = (
    "You compress a coding agent's reply into a spoken status update. "
    "Reply with 1-2 plain sentences, no markdown, no code, present tense, "
    "as if briefly telling a colleague what the agent just did or found."
)


def summarize(text: str, url: str, model: str, timeout: float = 25.0) -> str | None:
    """Summarize via an OpenAI-compatible chat endpoint. None on any failure."""
    import httpx

    try:
        resp = httpx.post(
            url.rstrip("/") + "/v1/chat/completions",
            json={
                "model": model,
                "messages": [
                    {"role": "system", "content": SUMMARIZE_SYSTEM},
                    {"role": "user", "content": text[:6000]},
                ],
                "max_tokens": 120,
                "temperature": 0.3,
            },
            timeout=timeout,
        )
        resp.raise_for_status()
        content = resp.json()["choices"][0]["message"]["content"].strip()
        return content or None
    except Exception:
        return None


TLDR_RE = re.compile(r"^\s*(?:\*\*)?tl;?dr:?(?:\*\*)?\s*:?\s*(.+)$", re.IGNORECASE)


def extract_tldr(text: str) -> str | None:
    """The final non-empty line, if it is a TLDR line."""
    for line in reversed(text.strip().splitlines()):
        line = line.strip()
        if not line:
            continue
        match = TLDR_RE.match(line)
        return match.group(1).strip() if match else None
    return None


def prepare_narration(
    text: str,
    max_chars: int,
    mode: str = "full",
    summarizer_url: str = "",
    summarizer_model: str = "default",
) -> str:
    if mode == "tldr":
        tldr = extract_tldr(text)
        if tldr:
            return clean_for_speech(tldr) or "Turn complete."
    cleaned = clean_for_speech(text)
    if not cleaned:
        return "Turn complete."
    if len(cleaned) <= max_chars:
        return cleaned
    if mode == "summary" and summarizer_url:
        summary = summarize(text, summarizer_url, summarizer_model)
        if summary:
            return clean_for_speech(summary)
    return truncate_sentences(cleaned, max_chars)
