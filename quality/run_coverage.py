#!/usr/bin/env python3
"""Collect branch-aware per-crate and workspace coverage."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys
import tomllib

try:
    from .timing import timed
except ImportError:
    from timing import timed

ROOT = Path(__file__).resolve().parents[1]
POLICY = ROOT / "quality" / "features.toml"
COVERAGE_POLICY = ROOT / "quality" / "coverage.toml"
REPORTS = ROOT / "quality" / "reports"


def run(command: list[str]) -> None:
    print("+", " ".join(command), flush=True)
    with timed("coverage: " + " ".join(command), phase="coverage", gate="coverage"):
        subprocess.run(command, check=True, cwd=ROOT)


def normalized_flags(flags: object) -> tuple[str, ...]:
    if not isinstance(flags, list) or not all(isinstance(flag, str) for flag in flags):
        raise ValueError("coverage profile flags must be a string list")
    return tuple(flags)


def main() -> None:
    with POLICY.open("rb") as policy_file:
        policy = tomllib.load(policy_file)
    with COVERAGE_POLICY.open("rb") as policy_file:
        coverage_policy = tomllib.load(policy_file)
    coverage_crates = coverage_policy["policy"]["coverage_crates"]
    if not isinstance(coverage_crates, list) or not all(isinstance(crate, str) for crate in coverage_crates):
        raise ValueError("coverage policy coverage_crates must be a string list")
    profiles = policy["profiles"]
    combinations = []
    for name, flags in profiles.items():
        if name not in {"default", "no_default", "all"}:
            raise ValueError(f"unsupported coverage profile name: {name}")
        normalized = normalized_flags(flags)
        combinations.append((name, list(normalized)))
    combinations.extend(
        (f"critical-{entry['name']}", ["--features", ",".join(entry["features"])])
        for entry in policy.get("critical_combinations", [])
        if entry["enabled"]
    )

    REPORTS.mkdir(parents=True, exist_ok=True)
    for crate in coverage_crates:
        seen_effective: set[tuple[str, ...]] = set()
        for name, flags in combinations:
            report = (REPORTS / f"coverage-{name}-{crate}.json").resolve()
            # `cargo llvm-cov nextest --package` instruments the package's
            # integration binaries, but nextest's workspace execution model
            # does not reliably merge the package library test harness.  The
            # latter is especially important for boundary crates whose
            # implementation lives in lib.rs. Cargo test executes both the
            # library harness and integration targets in one coverage run.
            coverage_command = "test" if crate == "intention-daemon" else "nextest"
            coverage_flags = [*flags]
            if crate == "intention-daemon" and "--all-features" not in coverage_flags:
                coverage_flags.append("--all-features")
            effective = tuple(coverage_flags)
            if crate == "intention-daemon" and effective in seen_effective:
                continue
            seen_effective.add(effective)
            # Release target declarations are not coverage target declarations.
            # Keep all targets until a dedicated coverage inventory proves an
            # extensionally equivalent explicit set.
            target_flags = ["--all-targets"]
            command = [
                "cargo",
                "+nightly-2026-07-31",
                "llvm-cov",
                "--branch",
                "--json",
                "--summary-only",
                "--output-path",
                str(report),
                coverage_command,
                *target_flags,
                "--locked",
                *coverage_flags,
                "--package",
                crate,
            ]
            run(
                command,
            )
            run(
                [
                    sys.executable,
                    "quality/check_coverage.py",
                    "--report",
                    str(report),
                    "--crate",
                    crate,
                ]
            )

    for name, flags in combinations:
        # The package reports enforce each crate's denominator.  This aggregate
        # report also exercises dependency code in the same instrumented test
        # process, preventing package isolation from hiding production paths.
        report = REPORTS / f"coverage-{name}-workspace.json"
        run([
            "cargo", "+nightly-2026-07-31", "llvm-cov", "--branch", "--json",
            "--summary-only", "--output-path", str(report), "nextest",
            "--all-targets", "--workspace", "--locked", *flags,
        ])
        run([
            sys.executable, "quality/check_coverage.py", "--report", str(report),
            "--workspace-aggregate",
        ])


if __name__ == "__main__":
    main()
