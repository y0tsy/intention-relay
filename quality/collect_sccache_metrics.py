#!/usr/bin/env python3
"""Collect sccache statistics into raw and structured diagnostic artifacts.

Operational diagnostic only: this script never changes a quality gate's
verdict. It always exits 0, so a missing sccache binary, an unsupported
stats format, or an unexpected stats shape can never turn a passed gate red,
while the gate step's own failure still fails the job (the collector runs
with `if: always()` and never masks that verdict).

It writes one file set per phase under the reports directory; the phase is
sanitized into every filename so parallel or repeated jobs never overwrite
each other's artifacts:

  sccache-stats-raw-<phase>.json   raw `--show-stats --stats-format json`
                                   output, kept in the artifact only
  sccache-stats-raw-<phase>.txt    raw human `--show-stats` output, written
                                   only as a fallback; a stale file in the
                                   alternate format is removed before a new
                                   one is written
  sccache-diagnostic-<phase>.json  structured summary, written atomically

Only numeric aggregates and the cache location are recorded; no environment
or process data is dumped. Console output is limited to the sanitized
summary line so raw statistics never reach CI logs. Counter values must be
non-negative integers; bools (int subclasses) and negative integers are
rejected as malformed, leaving the affected stat unavailable or ignored, so
malformed output can never crash the diagnostics.
"""

from __future__ import annotations

import argparse
import datetime
import json
import os
from pathlib import Path
import re
import subprocess
import sys

# Scalar counters in the pinned sccache v0.17.0 `--show-stats
# --stats-format json` schema (src/server.rs `ServerStats`). Per-language
# counters (`cache_hits`, `cache_misses`, `cache_errors`) serialize as
# `{"counts": {...}, "adv_counts": {...}}` objects and are summed into
# single numeric aggregates below.
KNOWN_STAT_KEYS = (
    "compile_requests",
    "requests_unsupported_compiler",
    "requests_not_compile",
    "requests_not_cacheable",
    "requests_executed",
    "cache_timeouts",
    "cache_read_errors",
    "non_cacheable_compilations",
    "forced_recaches",
    "cache_write_errors",
    "cache_writes",
    "compilations",
    "compile_fails",
    "dist_errors",
)
KNOWN_STAT_KEYS_SET = frozenset(KNOWN_STAT_KEYS)
PER_LANGUAGE_STAT_KEYS = ("cache_hits", "cache_misses", "cache_errors")

# v0.17.0 human `--show-stats` names that do not normalize to their JSON key.
TEXT_STAT_OVERRIDES = {
    "Compile requests executed": "requests_executed",
    "Compilation failures": "compile_fails",
    "Non-cacheable calls": "requests_not_cacheable",
    "Non-compilation calls": "requests_not_compile",
    "Unsupported compiler calls": "requests_unsupported_compiler",
    "Failed distributed compilations": "dist_errors",
}

TEXT_STAT_LINE = re.compile(r"^([A-Za-z][A-Za-z /()+.-]*?)\s+(-?\d+)\s*$")
CACHE_SIZE_LINE = re.compile(r"^Cache size\s+([0-9.]+)\s*([A-Za-z]+)?\s*$")
CACHE_SIZE_UNITS = {
    "b": 1,
    "kb": 1000,
    "mb": 1000**2,
    "gb": 1000**3,
    "tb": 1000**4,
    "kib": 1024,
    "mib": 1024**2,
    "gib": 1024**3,
    "tib": 1024**4,
}


def run_id() -> str:
    return os.environ.get("QUALITY_METRICS_RUN_ID") or os.environ.get(
        "GITHUB_RUN_ID", f"local-{os.getpid()}"
    )


def phase_slug(phase: str) -> str:
    """Sanitize a phase label for use in filenames."""
    slug = re.sub(r"[^A-Za-z0-9._-]+", "-", (phase or "").strip())
    return slug or "default"


def _normalize_human_name(name: str) -> str:
    lowered = (
        name.lower()
        .replace("(", " ")
        .replace(")", " ")
        .replace("/", " ")
        .replace("-", " ")
        .replace("+", " ")
    )
    return "_".join(lowered.split())


def _valid_counter(value: object) -> bool:
    """A counter is a non-negative integer; bools are rejected explicitly."""
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def _per_language_total(value: object) -> int | None:
    """Sum the `counts` map of a v0.17.0 per-language counter object.

    Malformed entries (bools or negative integers) are ignored; a map with
    no valid entries leaves the aggregate unavailable.
    """
    if not isinstance(value, dict):
        return None
    counts = value.get("counts")
    if not isinstance(counts, dict):
        return None
    totals = [count for count in counts.values() if _valid_counter(count)]
    if not totals:
        return None
    return sum(totals)


def _json_stats(output: str) -> tuple[dict[str, object], int | None, str | None]:
    data = json.loads(output)
    if not isinstance(data, dict):
        return {}, None, None
    stats = data.get("stats")
    if not isinstance(stats, dict):
        stats = data
    selected: dict[str, object] = {
        key: stats[key] for key in KNOWN_STAT_KEYS if _valid_counter(stats.get(key))
    }
    for key in PER_LANGUAGE_STAT_KEYS:
        total = _per_language_total(stats.get(key))
        if total is not None:
            selected[key] = total
    location = data.get("cache_location")
    if not isinstance(location, str):
        location = None
    # v0.17.0 serializes cache_size as an integer (Option<u64>); malformed
    # values leave it unavailable.
    size = data.get("cache_size")
    if not _valid_counter(size):
        size = None
    return selected, size, location


def _text_stats(output: str) -> tuple[dict[str, object], int | None, str | None]:
    selected: dict[str, object] = {}
    size: int | None = None
    for line in output.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        size_match = CACHE_SIZE_LINE.match(stripped)
        if size_match:
            try:
                amount = float(size_match.group(1))
                unit = (size_match.group(2) or "b").lower()
                if unit in CACHE_SIZE_UNITS:
                    size = int(amount * CACHE_SIZE_UNITS[unit])
            except ValueError:
                size = None
            continue
        match = TEXT_STAT_LINE.match(stripped)
        if not match:
            continue
        name = match.group(1)
        key = TEXT_STAT_OVERRIDES.get(name) or _normalize_human_name(name)
        if key in KNOWN_STAT_KEYS_SET or key in PER_LANGUAGE_STAT_KEYS:
            value = int(match.group(2))
            if value >= 0:
                selected[key] = value
    return selected, size, None


def _run(command: list[str]) -> str | None:
    try:
        completed = subprocess.run(
            command,
            capture_output=True,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if completed.returncode != 0 or not completed.stdout.strip():
        return None
    return completed.stdout.decode("utf-8", errors="replace")


def _hit_rate(stats: dict[str, object]) -> float | None:
    hits = stats.get("cache_hits")
    misses = stats.get("cache_misses")
    if not isinstance(hits, int) or not isinstance(misses, int):
        return None
    total = hits + misses
    if total <= 0:
        return None
    return round(hits / total, 4)


def _write_diagnostic(
    target: Path,
    phase: str,
    source: str,
    stats: dict[str, object] | None,
    size: int | None,
    location: str | None,
) -> dict[str, object]:
    hit_rate = _hit_rate(stats) if stats else None
    errors = stats.get("cache_errors") if stats else None
    summary = (
        f"source {source}"
        if stats is None
        else (
            f"{stats.get('cache_hits', 0)} hits / "
            f"{stats.get('cache_misses', 0)} misses / {errors or 0} errors"
            + (f" (hit rate {hit_rate})" if hit_rate is not None else "")
            + (f", cache {size} bytes" if size is not None else "")
        )
    )
    diagnostic = {
        "schema_version": 1,
        "run_id": run_id(),
        "workflow": os.environ.get("GITHUB_WORKFLOW"),
        "job": os.environ.get("GITHUB_JOB"),
        "phase": phase or os.environ.get("GITHUB_JOB") or "",
        "collected_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "source": source,
        "stats": stats,
        "cache_size_bytes": size,
        "cache_location": location,
        "hit_rate": hit_rate,
        "summary": summary,
    }
    temporary = target.with_suffix(".tmp")
    temporary.write_text(
        json.dumps(diagnostic, indent=2, sort_keys=True), encoding="utf-8"
    )
    temporary.replace(target)
    return diagnostic


def collect(sccache: str, reports: Path, phase: str) -> None:
    reports.mkdir(parents=True, exist_ok=True)
    slug = phase_slug(phase)
    raw_json_path = reports / f"sccache-stats-raw-{slug}.json"
    raw_text_path = reports / f"sccache-stats-raw-{slug}.txt"
    diagnostic_path = reports / f"sccache-diagnostic-{slug}.json"
    raw_json = _run([sccache, "--show-stats", "--stats-format", "json"])
    if raw_json is not None:
        raw_json_path.write_text(raw_json, encoding="utf-8")
        # A previous run may have left the alternate-format raw file behind;
        # remove it so the artifact never mixes formats.
        raw_text_path.unlink(missing_ok=True)
        try:
            stats, size, location = _json_stats(raw_json)
        except json.JSONDecodeError:
            stats, size, location = {}, None, None
        diagnostic = _write_diagnostic(diagnostic_path, phase, "json", stats, size, location)
        print(f"sccache-metrics: {diagnostic['summary']}", flush=True)
        return
    raw_text = _run([sccache, "--show-stats"])
    if raw_text is not None:
        raw_text_path.write_text(raw_text, encoding="utf-8")
        raw_json_path.unlink(missing_ok=True)
        stats, size, location = _text_stats(raw_text)
        diagnostic = _write_diagnostic(diagnostic_path, phase, "text", stats, size, location)
        print(f"sccache-metrics: {diagnostic['summary']}", flush=True)
        return
    diagnostic = _write_diagnostic(diagnostic_path, phase, "unavailable", None, None, None)
    print(f"sccache-metrics: {diagnostic['summary']}", flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Collect sccache statistics into raw and diagnostic artifacts."
    )
    parser.add_argument("--sccache", required=True, help="path to the sccache binary")
    parser.add_argument(
        "--reports", type=Path, required=True, help="directory for the artifacts"
    )
    parser.add_argument("--phase", default="", help="quality phase label")
    arguments = parser.parse_args()
    try:
        collect(arguments.sccache, arguments.reports, arguments.phase)
    except Exception as error:  # observational: never fail the job
        print(f"sccache-metrics: collection failed: {error}", file=sys.stderr, flush=True)
        try:
            arguments.reports.mkdir(parents=True, exist_ok=True)
            _write_diagnostic(
                arguments.reports / f"sccache-diagnostic-{phase_slug(arguments.phase)}.json",
                arguments.phase,
                "unavailable",
                None,
                None,
                None,
            )
        except OSError:
            pass


if __name__ == "__main__":
    main()
