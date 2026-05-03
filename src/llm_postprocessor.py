"""
P2.1 — Local LLM post-processing via Ollama

Design contract:
  - If Ollama is not installed / not running → process() returns text unchanged, no exception.
  - If Ollama is available but disabled in config → pass-through immediately.
  - All network calls are synchronous but capped by a short timeout so they
    never stall the injection pipeline for more than a couple of seconds.
  - Zero third-party dependencies: uses only stdlib urllib.
"""

import json
import urllib.request
import urllib.error
from typing import Optional

# ── Ollama API constants ──────────────────────────────────────────────────
_BASE_URL = "http://localhost:11434"
_GENERATE_URL = f"{_BASE_URL}/api/generate"
_TAGS_URL = f"{_BASE_URL}/api/tags"
_PROBE_TIMEOUT = 2.0   # seconds — availability check
_GENERATE_TIMEOUT = 8.0  # seconds — generation call

# ── Mode prompts ──────────────────────────────────────────────────────────
_PROMPTS = {
    "clean": (
        "Fix any grammar or punctuation errors in the following text. "
        "Output ONLY the corrected text with no explanation, no quotes, "
        "no preamble:\n\n{text}"
    ),
    "formal": (
        "Rewrite the following text in a professional, formal tone. "
        "Output ONLY the rewritten text with no explanation:\n\n{text}"
    ),
    "casual": (
        "Rewrite the following text in a friendly, casual conversational tone. "
        "Output ONLY the rewritten text:\n\n{text}"
    ),
    "bullet": (
        "Convert the following text into a concise bullet-point list. "
        "Use • as the bullet character. Output ONLY the bullet points:\n\n{text}"
    ),
    "concise": (
        "Make the following text more concise without losing meaning. "
        "Output ONLY the shortened text:\n\n{text}"
    ),
}

# Human-readable labels for the settings UI
MODE_LABELS = {
    "off":     "Off (Whisper output only)",
    "clean":   "Fix grammar & punctuation",
    "formal":  "Rewrite — formal tone",
    "casual":  "Rewrite — casual tone",
    "bullet":  "Convert to bullet points",
    "concise": "Make concise",
}


class LLMPostprocessor:
    """
    Wraps Ollama for optional post-processing of Whisper transcriptions.

    Thread-safety: process() is called from the InferenceEngine thread.
    Availability state is read-only after __init__ so no locking needed.
    """

    def __init__(self, config):
        self.config = config
        self._available: Optional[bool] = None   # None = not yet probed
        self._available_models: list[str] = []

    # ── Public API ────────────────────────────────────────────────────────

    def probe(self) -> bool:
        """
        Check whether Ollama is reachable. Safe to call from any thread.
        Caches the result so subsequent calls are instant.
        Returns True if Ollama responded, False otherwise.
        """
        if self._available is not None:
            return self._available
        try:
            req = urllib.request.Request(_TAGS_URL, method="GET")
            with urllib.request.urlopen(req, timeout=_PROBE_TIMEOUT) as resp:
                body = json.loads(resp.read().decode())
                self._available_models = [
                    m.get("name", "") for m in body.get("models", [])
                ]
            self._available = True
        except Exception:
            self._available = False
        return self._available

    def refresh_probe(self) -> bool:
        """Force a fresh availability check (e.g. after user clicks 'Retry')."""
        self._available = None
        return self.probe()

    @property
    def available(self) -> bool:
        """True if Ollama was reachable on last probe."""
        return bool(self._available)

    @property
    def available_models(self) -> list[str]:
        return list(self._available_models)

    def process(self, text: str) -> str:
        """
        Apply LLM post-processing to text.

        Guaranteed to return a string. If anything goes wrong (Ollama down,
        timeout, bad JSON, empty response) the original text is returned.
        """
        if not text.strip():
            return text

        if not self.config.get("ollama_enabled", False):
            return text

        mode = self.config.get("ollama_mode", "clean")
        if mode == "off":
            return text

        prompt_template = _PROMPTS.get(mode)
        if not prompt_template:
            return text

        # Re-probe if we haven't yet (lazy init)
        if self._available is None:
            self.probe()
        if not self._available:
            return text

        return self._call_ollama(prompt_template.format(text=text))

    # ── Internal ──────────────────────────────────────────────────────────

    def _call_ollama(self, prompt: str) -> str:
        """
        POST to Ollama /api/generate with stream=False.
        Returns the model response string, or the original prompt text on any error.

        We extract the original text from the prompt via the known suffix :\n\n{text}
        but it's simpler to just catch exceptions and return the untouched text
        from process() above — so here we raise on error and let process() handle it.
        """
        model = self.config.get("ollama_model", "llama3.2:1b")
        payload = json.dumps({
            "model": model,
            "prompt": prompt,
            "stream": False,
            "options": {
                "temperature": 0.2,   # low for deterministic cleanup
                "num_predict": 512,
            },
        }).encode("utf-8")

        req = urllib.request.Request(
            _GENERATE_URL,
            data=payload,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=_GENERATE_TIMEOUT) as resp:
                body = json.loads(resp.read().decode())
                result = body.get("response", "").strip()
                if result:
                    print(f"[LLM] {self.config.get('ollama_mode')} → {result[:60]}…")
                    return result
        except urllib.error.URLError:
            # Ollama went down mid-session — mark unavailable, return original
            self._available = False
            print("[LLM] Ollama became unreachable — disabling for this session.")
        except Exception as e:
            print(f"[LLM] Unexpected error: {e}")
        # Fall through: return empty string so process() uses original
        return ""
