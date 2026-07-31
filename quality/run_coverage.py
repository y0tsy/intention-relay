#!/usr/bin/env python3
"""Collect branch-aware coverage for every required Cargo feature profile."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
POLICY = ROOT / "quality" / "features.toml"
REPORTS = ROOT / "quality" / "reports"


def run(command: list[str]) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, check=True, cwd=ROOT)


def main() -> None:
    with POLICY.open("rb") as policy_file:
        policy = tomllib.load(policy_file)
    profiles = policy["profiles"]
    combinations = [(name, flags) for name, flags in profiles.items()]
    combinations.extend(
        (f"critical-{entry['name']}", ["--features", ",".join(entry["features"])])
        for entry in policy.get("critical_combinations", [])
        if entry["enabled"]
    )

    REPORTS.mkdir(parents=True, exist_ok=True)
    for name, flags in combinations:
        report = REPORTS / f"coverage-{name}.json"
        run(
            [
                "cargo",
                "+nightly-2026-07-31",
                "llvm-cov",
                "--branch",
                "--json",
                "--summary-only",
                "--output-path",
                str(report),
                "nextest",
                "--workspace",
                "--all-targets",
                "--locked",
                *flags,
            ]
        )
        run([sys.executable, "quality/check_coverage.py", "--report", str(report)])


if __name__ == "__main__":
    main()
