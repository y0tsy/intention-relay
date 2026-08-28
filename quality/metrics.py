#!/usr/bin/env python3
"""Start and finalize the human-readable quality metrics manifest."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import subprocess

ROOT = Path(__file__).resolve().parents[1]
REPORTS = ROOT / "quality" / "reports"
MANIFEST = REPORTS / "quality-run.json"
EVENTS = REPORTS / "quality-run.events.jsonl"


def run_id() -> str:
    return os.environ.get("QUALITY_METRICS_RUN_ID") or os.environ.get(
        "GITHUB_RUN_ID", f"local-{os.getpid()}"
    )


def git_sha() -> str | None:
    try:
        completed = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError:
        return None
    if completed.returncode != 0:
        return None
    return completed.stdout.strip() or None


def start() -> None:
    REPORTS.mkdir(parents=True, exist_ok=True)
    # Each run starts with a clean event stream so stale records from earlier
    # local or CI runs cannot leak into the current manifest.
    if EVENTS.exists():
        EVENTS.unlink()
    manifest = {
        "schema_version": 1,
        "run_id": run_id(),
        "repository": "intention-relay",
        "git_sha": git_sha(),
        "workflow": os.environ.get("GITHUB_WORKFLOW"),
        "job": os.environ.get("GITHUB_JOB"),
        "runner_os": os.environ.get("RUNNER_OS"),
        "os": platform.system().lower(),
        "arch": platform.machine(),
        "started_at": os.environ.get("QUALITY_METRICS_STARTED_AT"),
        "finished_at": None,
        "status": "in_progress",
        "commands": [],
    }
    temporary = MANIFEST.with_suffix(".tmp")
    temporary.write_text(json.dumps(manifest, indent=2, sort_keys=True), encoding="utf-8")
    temporary.replace(MANIFEST)
    print(f"metrics: manifest started for run {manifest['run_id']}", flush=True)


def finish() -> None:
    REPORTS.mkdir(parents=True, exist_ok=True)
    try:
        with MANIFEST.open(encoding="utf-8") as stream:
            manifest = json.load(stream)
    except (FileNotFoundError, json.JSONDecodeError):
        manifest = {
            "schema_version": 1,
            "run_id": run_id(),
            "repository": "intention-relay",
            "git_sha": git_sha(),
            "workflow": os.environ.get("GITHUB_WORKFLOW"),
            "job": os.environ.get("GITHUB_JOB"),
            "runner_os": os.environ.get("RUNNER_OS"),
            "os": platform.system().lower(),
            "arch": platform.machine(),
            "started_at": None,
            "finished_at": None,
            "status": "in_progress",
            "commands": [],
        }
    import datetime

    manifest["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    manifest["status"] = os.environ.get("QUALITY_METRICS_STATUS", "passed")
    try:
        with EVENTS.open(encoding="utf-8") as stream:
            manifest["commands"] = [json.loads(line) for line in stream if line.strip()]
    except (FileNotFoundError, json.JSONDecodeError):
        manifest["commands"] = []
    temporary = MANIFEST.with_suffix(".tmp")
    temporary.write_text(json.dumps(manifest, indent=2, sort_keys=True), encoding="utf-8")
    temporary.replace(MANIFEST)
    print(f"metrics: manifest finalized with status {manifest['status']}", flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description="Start or finalize quality metrics.")
    parser.add_argument("action", choices=["start", "finish"])
    arguments = parser.parse_args()
    if arguments.action == "start":
        start()
    else:
        finish()


if __name__ == "__main__":
    main()
