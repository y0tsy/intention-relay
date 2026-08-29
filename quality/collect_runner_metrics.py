#!/usr/bin/env python3
"""Record observational runner resource metrics for CI coverage jobs.

Observational only: the script always exits 0 and writes nullable fields
when a metric is unavailable on the current platform, so it never changes a
quality gate's verdict. It records CPU count and load averages, memory
totals when the platform exposes them, and disk usage for the workspace and
home directories as one JSON document written atomically. It never dumps
the environment or process lists.
"""

from __future__ import annotations

import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


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


def collect_cpu() -> dict[str, object]:
    metrics: dict[str, object] = {"count": os.cpu_count()}
    try:
        one, five, fifteen = os.getloadavg()
        metrics["load_avg_1m"] = round(one, 2)
        metrics["load_avg_5m"] = round(five, 2)
        metrics["load_avg_15m"] = round(fifteen, 2)
    except (AttributeError, OSError):
        pass
    return metrics


def collect_memory() -> dict[str, object] | None:
    if sys.platform.startswith("linux"):
        try:
            with open("/proc/meminfo", encoding="utf-8") as stream:
                values: dict[str, int] = {}
                for line in stream:
                    key, separator, rest = line.partition(":")
                    if not separator:
                        continue
                    parts = rest.split()
                    if key in ("MemTotal", "MemAvailable") and parts and parts[0].isdigit():
                        values[key] = int(parts[0]) * 1024
            if values:
                return {
                    "total_bytes": values.get("MemTotal"),
                    "available_bytes": values.get("MemAvailable"),
                }
        except OSError:
            return None
    if sys.platform == "darwin":
        try:
            completed = subprocess.run(
                ["sysctl", "-n", "hw.memsize"],
                capture_output=True,
                text=True,
                check=False,
                timeout=10,
            )
            if completed.returncode == 0 and completed.stdout.strip().isdigit():
                return {
                    "total_bytes": int(completed.stdout.strip()),
                    "available_bytes": None,
                }
        except (OSError, subprocess.TimeoutExpired):
            return None
    return None


def collect_disk() -> list[dict[str, object]]:
    entries: list[dict[str, object]] = []
    for path in (Path.cwd(), Path.home()):
        try:
            usage = shutil.disk_usage(path)
            entries.append(
                {
                    "path": str(path),
                    "total_bytes": usage.total,
                    "used_bytes": usage.used,
                    "free_bytes": usage.free,
                }
            )
        except OSError:
            entries.append(
                {"path": str(path), "total_bytes": None, "used_bytes": None, "free_bytes": None}
            )
    return entries


def _human_bytes(value: int | None) -> str:
    if value is None:
        return "n/a"
    amount = float(value)
    for unit in ("B", "KiB", "MiB", "GiB", "TiB"):
        if amount < 1024 or unit == "TiB":
            return f"{amount:.1f} {unit}"
        amount /= 1024
    return f"{amount:.1f} TiB"


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Collect observational runner resource metrics as JSON."
    )
    parser.add_argument(
        "--label", required=True, choices=["before", "after"], help="metrics window"
    )
    parser.add_argument("--phase", default="", help="quality phase label")
    parser.add_argument("--out", type=Path, required=True, help="output JSON path")
    arguments = parser.parse_args()
    try:
        memory = collect_memory()
        disk = collect_disk()
        report = {
            "schema_version": 1,
            "run_id": run_id(),
            "workflow": os.environ.get("GITHUB_WORKFLOW"),
            "job": os.environ.get("GITHUB_JOB"),
            "phase": arguments.phase,
            "label": arguments.label,
            "collected_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "git_sha": git_sha(),
            "os": platform.system().lower(),
            "arch": platform.machine(),
            "cpu": collect_cpu(),
            "memory": memory,
            "disk": disk,
        }
        target = arguments.out.resolve()
        target.parent.mkdir(parents=True, exist_ok=True)
        temporary = target.with_suffix(".tmp")
        temporary.write_text(json.dumps(report, indent=2, sort_keys=True), encoding="utf-8")
        temporary.replace(target)
        available = memory.get("available_bytes") if memory else None
        free = disk[0].get("free_bytes") if disk else None
        print(
            f"runner-metrics: {arguments.label} {arguments.phase or '(unnamed phase)'}: "
            f"cpu {report['cpu'].get('count')}, "
            f"memory available {_human_bytes(available)}, "
            f"workspace disk free {_human_bytes(free)}",
            flush=True,
        )
    except Exception as error:  # observational: never fail the job
        print(f"runner-metrics: collection failed: {error}", file=sys.stderr, flush=True)


if __name__ == "__main__":
    main()
