#!/usr/bin/env python3
"""Benchmark clean-target cargo build wall time across --jobs settings.

Manual CI tool for the `quality-benchmark` workflow (workflow_dispatch
only). It measures `cargo build --workspace --locked` wall-clock time for
the cargo default (no explicit --jobs) plus each requested --jobs value.
Every build runs inside a benchmark-specific temporary CARGO_TARGET_DIR
created with the platform-native temp directory API, so a local run never
touches the workspace's shared target directory and the benchmark removes
only its own temporary target when finished. Each measured run starts from
`cargo clean` scoped to that temporary target, so each run is a clean-target
build and job-count parallelism effects remain observable. An empty --jobs
value benchmarks only the cargo-default baseline. Each configuration runs
one warmup build followed by `--repetitions` measured builds. Results are
printed as a human table and written as a structured JSON report with
nearest-rank p50/p95 and mean/min/max aggregation and the exact command
(including `--jobs=N`) per configuration. The benchmark never runs
in the blocking quality workflow and never affects a gate verdict.
"""

from __future__ import annotations

import argparse
import datetime
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BASELINE = "baseline"
WARMUP_BUILDS = 1
MIN_REPETITIONS = 3
TARGET_DIR_PREFIX = "benchmark-cargo-jobs-"


def percentile(sorted_values: list[float], rank: float) -> float:
    """Nearest-rank percentile of an ascending list."""
    if not sorted_values:
        return float("nan")
    index = max(1, math.ceil(rank / 100.0 * len(sorted_values))) - 1
    return sorted_values[index]


def aggregate(durations: list[float]) -> dict[str, float]:
    ordered = sorted(durations)
    return {
        "p50": percentile(ordered, 50.0),
        "p95": percentile(ordered, 95.0),
        "mean": sum(ordered) / len(ordered),
        "min": ordered[0],
        "max": ordered[-1],
    }


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


def toolchain_versions() -> dict[str, str]:
    versions: dict[str, str] = {}
    for command in (["cargo", "--version"], ["rustc", "--version"]):
        try:
            completed = subprocess.run(
                command, capture_output=True, text=True, timeout=30, check=False
            )
            if completed.returncode == 0 and completed.stdout.strip():
                versions[command[0]] = completed.stdout.strip()
        except OSError:
            pass
    return versions


def build_environment(target_dir: Path) -> dict[str, str]:
    environment = os.environ.copy()
    environment["CARGO_INCREMENTAL"] = "0"
    # Isolate every build into the benchmark's own temporary target so a
    # local run never touches the workspace's shared target directory.
    environment["CARGO_TARGET_DIR"] = str(target_dir)
    return environment


def cargo_clean(cargo: str, workdir: Path, target_dir: Path) -> None:
    completed = subprocess.run(
        [cargo, "clean"], cwd=workdir, env=build_environment(target_dir), check=False
    )
    if completed.returncode != 0:
        raise RuntimeError(f"{cargo} clean failed with exit code {completed.returncode}")


def build_command(cargo: str, config: str) -> list[str]:
    """Exact cargo command for one configuration, --jobs applied explicitly."""
    command = [cargo, "build", "--workspace", "--locked"]
    if config != BASELINE:
        command.append(f"--jobs={config}")
    return command


def measure(cargo: str, workdir: Path, config: str, target_dir: Path) -> float:
    command = build_command(cargo, config)
    started = time.monotonic()
    completed = subprocess.run(
        command, cwd=workdir, env=build_environment(target_dir), check=False
    )
    elapsed = time.monotonic() - started
    if completed.returncode != 0:
        raise RuntimeError(f"{' '.join(command)} failed with exit code {completed.returncode}")
    return elapsed


def parse_jobs(raw: str) -> list[str]:
    """Parse a comma-separated --jobs list.

    Blank entries are skipped, so an empty (or whitespace-only) value
    yields no extra configurations: the benchmark then covers only the
    cargo-default baseline.
    """
    configs: list[str] = []
    for part in raw.split(","):
        value = part.strip()
        if not value:
            continue
        try:
            count = int(value)
        except ValueError:
            raise ValueError(f"invalid --jobs value {value!r}: expected a positive integer")
        if count < 1:
            raise ValueError(f"invalid --jobs value {value!r}: expected a positive integer")
        configs.append(str(count))
    return configs


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Benchmark clean-target cargo build wall time across --jobs settings."
    )
    parser.add_argument(
        "--jobs",
        default="2,4,8",
        help="comma-separated --jobs values; the cargo-default baseline is always included "
        "first, and an empty value benchmarks only that baseline",
    )
    parser.add_argument(
        "--repetitions",
        type=int,
        default=MIN_REPETITIONS,
        help=f"measured clean-target builds per configuration (minimum {MIN_REPETITIONS})",
    )
    parser.add_argument("--cargo", default=os.environ.get("CARGO", "cargo"))
    parser.add_argument(
        "--out",
        type=Path,
        default=ROOT / "quality" / "reports" / "benchmark-cargo-jobs.json",
        help="structured JSON report path",
    )
    arguments = parser.parse_args()
    if arguments.repetitions < MIN_REPETITIONS:
        parser.error(f"--repetitions must be at least {MIN_REPETITIONS}")
    configs = [BASELINE, *parse_jobs(arguments.jobs)]
    if len(set(configs)) != len(configs):
        parser.error("--jobs contains duplicates")
    workdir = ROOT
    target_dir = Path(tempfile.mkdtemp(prefix=TARGET_DIR_PREFIX))
    results: dict[str, dict[str, object]] = {}
    print(f"benchmark: isolated CARGO_TARGET_DIR {target_dir}", flush=True)
    print(f"benchmark: measuring {' '.join(['cargo', 'build', '--workspace', '--locked'])}", flush=True)
    print(
        f"benchmark: configurations {', '.join(repr(config) for config in configs)}, "
        f"warmup {WARMUP_BUILDS}, measured repetitions {arguments.repetitions}",
        flush=True,
    )
    try:
        for config in configs:
            command = build_command(arguments.cargo, config)
            cargo_clean(arguments.cargo, workdir, target_dir)
            warmup = measure(arguments.cargo, workdir, config, target_dir)
            durations: list[float] = []
            for _ in range(arguments.repetitions):
                cargo_clean(arguments.cargo, workdir, target_dir)
                durations.append(measure(arguments.cargo, workdir, config, target_dir))
            entry: dict[str, object] = {
                "command": " ".join(command),
                "warmup_seconds": round(warmup, 3),
                "durations_seconds": [round(duration, 3) for duration in durations],
            }
            entry.update({key: round(value, 3) for key, value in aggregate(durations).items()})
            results[config] = entry
            print(
                f"benchmark: {config}: warmup {warmup:.3f}s, durations "
                f"{[round(duration, 3) for duration in durations]}, "
                f"p50 {entry['p50']}s, p95 {entry['p95']}s",
                flush=True,
            )
        report = {
            "schema_version": 1,
            "run_id": run_id(),
            "workflow": os.environ.get("GITHUB_WORKFLOW"),
            "collected_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "git_sha": git_sha(),
            "cargo": arguments.cargo,
            "command": "cargo build --workspace --locked",
            "cpu_count": os.cpu_count(),
            "toolchain": toolchain_versions(),
            "warmup_builds": WARMUP_BUILDS,
            "repetitions": arguments.repetitions,
            "configs": results,
        }
        target = arguments.out.resolve()
        target.parent.mkdir(parents=True, exist_ok=True)
        temporary = target.with_suffix(".tmp")
        temporary.write_text(json.dumps(report, indent=2, sort_keys=True), encoding="utf-8")
        temporary.replace(target)
        print(f"benchmark: report written to {target}", flush=True)
    finally:
        # Remove only the benchmark's own temporary target; the workspace
        # target directory is never touched.
        shutil.rmtree(target_dir, ignore_errors=True)


if __name__ == "__main__":
    main()
