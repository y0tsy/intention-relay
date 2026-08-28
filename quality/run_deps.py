#!/usr/bin/env python3
"""Run independent dependency gates with a bounded worker pool."""

from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor, as_completed
import sys
import tomllib
from pathlib import Path

try:
    from timing import run_command
except ImportError:  # pragma: no cover - package/module invocation compatibility
    from quality.timing import run_command

ROOT = Path(__file__).resolve().parents[1]
MAX_WORKERS = 2


def run_check(label: str, command: list[str]) -> tuple[str, int, str]:
    completed = run_command(
        command,
        cwd=ROOT,
        phase="deps",
        gate=label,
        stage="dependency-check",
        capture_output=True,
    )
    output = (completed.stdout + completed.stderr).strip()
    return label, completed.returncode, output


def main() -> int:
    python = sys.executable
    # Metadata and notices are prerequisites; the independent checks follow.
    prerequisites = [
        ("metadata", ["cargo", "metadata", "--locked", "--format-version", "1"]),
        ("deny-policy", [python, "quality/check_deny_policy.py"]),
    ]
    for label, command in prerequisites:
        result = run_check(label, command)
        if result[1] != 0:
            print(f"deps: {label} failed\n{result[2]}", file=sys.stderr)
            return result[1]

    ignored = ",".join(tomllib.loads((ROOT / "quality/outdated.toml").read_text(encoding="utf-8"))["outdated_ignores"]["crates"])
    checks = [
        ("deny", ["cargo", "deny", "check"]),
        ("audit", ["cargo", "audit", "--ignore", "RUSTSEC-2024-0384", "--ignore", "RUSTSEC-2025-0012"]),
        ("machete", ["cargo", "machete", "--with-metadata", "--skip-target-dir"]),
        ("outdated", ["cargo", "outdated", "--workspace", "--root-deps-only", "--ignore", ignored, "--exit-code", "1"]),
    ]
    failures: list[tuple[str, int, str]] = []
    with ThreadPoolExecutor(max_workers=MAX_WORKERS) as executor:
        futures = [executor.submit(run_check, label, command) for label, command in checks]
        for future in as_completed(futures):
            result = future.result()
            if result[1] != 0:
                failures.append(result)
    udeps = run_check("udeps", [python, "quality/run_profiles.py", "udeps"])
    if udeps[1] != 0:
        failures.append(udeps)
    if failures:
        for label, code, output in sorted(failures):
            print(f"deps: {label} failed (exit {code})\n{output}", file=sys.stderr)
        return failures[0][1]
    print("deps: all dependency checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
