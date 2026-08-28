"""Small, human-readable timing helpers for quality gates."""

from __future__ import annotations

from contextlib import contextmanager
from time import monotonic
from typing import Iterator
import json
from pathlib import Path
import shlex
import os
import subprocess
import datetime

REPORTS = Path(__file__).resolve().parent / "reports"
EVENTS = REPORTS / "quality-run.events.jsonl"
MANIFEST = REPORTS / "quality-run.json"


def run_manifest() -> dict[str, object]:
    """Return one mutable manifest with run identity and current records."""
    try:
        with MANIFEST.open(encoding="utf-8") as stream:
            return json.load(stream)
    except (FileNotFoundError, json.JSONDecodeError):
        return {
            "schema_version": 1,
            "run_id": os.environ.get("QUALITY_METRICS_RUN_ID", "local"),
            "started_at": None,
            "finished_at": None,
            "status": "in_progress",
            "commands": [],
        }


def load_events() -> list[dict[str, object]]:
    """Read previously recorded events for the current run."""
    try:
        with EVENTS.open(encoding="utf-8") as stream:
            return [json.loads(line) for line in stream if line.strip()]
    except (FileNotFoundError, json.JSONDecodeError):
        return []


def save_manifest(status: str = "passed") -> None:
    """Write the final human-readable manifest atomically."""
    manifest = run_manifest()
    manifest["finished_at"] = datetime.datetime.now(
        datetime.timezone.utc
    ).isoformat()
    manifest["status"] = status
    manifest["commands"] = load_events()
    temporary = MANIFEST.with_suffix(".tmp")
    temporary.write_text(json.dumps(manifest, indent=2, sort_keys=True), encoding="utf-8")
    temporary.replace(MANIFEST)


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
    with EVENTS.open("a", encoding="utf-8") as stream:
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


def run_command(
    command: list[str],
    *,
    cwd: Path,
    phase: str,
    gate: str,
    profile: str = "",
    crate: str = "",
    stage: str = "command",
    capture_output: bool = False,
    env_overrides: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run one quality command with consistent safe timing output."""
    rendered = shlex.join(command)
    print("+", _safe_command(rendered), flush=True)
    environment = None
    if env_overrides:
        environment = os.environ.copy()
        environment.update(env_overrides)
    started = monotonic()
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            capture_output=capture_output,
            text=capture_output,
            env=environment,
            check=False,
        )
    except BaseException:
        elapsed = monotonic() - started
        record(
            rendered,
            elapsed,
            phase=phase,
            gate=gate,
            profile=profile,
            crate=crate,
            stage=stage,
            status="failed",
            command=rendered,
        )
        raise
    elapsed = monotonic() - started
    status = "passed" if completed.returncode == 0 else "failed"
    print(f"timing: {rendered}: {elapsed:.2f}s", flush=True)
    record(
        rendered,
        elapsed,
        phase=phase,
        gate=gate,
        profile=profile,
        crate=crate,
        stage=stage,
        status=status,
        command=rendered,
    )
    return completed
