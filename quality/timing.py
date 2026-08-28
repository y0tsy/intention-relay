"""Small, human-readable timing helpers for quality gates."""

from __future__ import annotations

from contextlib import contextmanager
from time import monotonic
from typing import Iterator
import json
from pathlib import Path
import shlex

REPORTS = Path(__file__).resolve().parent / "reports"


def _safe_command(command: str) -> str:
    """Render commands without leaking values that commonly contain secrets."""
    try:
        parts = shlex.split(command)
    except ValueError:
        return "[REDACTED COMMAND]"
    redacted = []
    for part in parts:
        if any(marker in part.lower() for marker in ("token", "password", "secret", "api_key", "apikey")):
            redacted.append("[REDACTED]")
        else:
            redacted.append(part)
    return shlex.join(redacted)


def record(
    label: str,
    seconds: float,
    *,
    phase: str = "quality",
    gate: str = "",
    profile: str = "",
    crate: str = "",
    stage: str = "",
    status: str = "passed",
    command: str | None = None,
) -> None:
    REPORTS.mkdir(parents=True, exist_ok=True)
    entry = {
        "phase": phase, "gate": gate, "profile": profile, "crate": crate,
        "stage": stage, "status": status, "duration": round(seconds, 3),
    }
    if command:
        entry["command"] = _safe_command(command)
    with (REPORTS / "timing.jsonl").open("a", encoding="utf-8") as stream:
        stream.write(json.dumps(entry, sort_keys=True) + "\n")


@contextmanager
def timed(label: str, **fields: str) -> Iterator[None]:
    """Print the elapsed wall-clock time for one quality phase."""
    started = monotonic()
    status = "passed"
    try:
        yield
    except BaseException:
        status = "failed"
        raise
    finally:
        elapsed = monotonic() - started
        print(f"timing: {label}: {elapsed:.2f}s", flush=True)
        record(label, elapsed, status=status, **fields)
