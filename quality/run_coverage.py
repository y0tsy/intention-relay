#!/usr/bin/env python3
"""Collect branch-aware per-crate and workspace coverage."""

from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys
import tomllib

try:
    from .timing import run_command
except ImportError:
    from timing import run_command

ROOT = Path(__file__).resolve().parents[1]
POLICY = ROOT / "quality" / "features.toml"
COVERAGE_POLICY = ROOT / "quality" / "coverage.toml"
REPORTS = ROOT / "quality" / "reports"


def run(
    command: list[str],
    *,
    profile: str = "",
    crate: str = "",
    stage: str = "command",
) -> None:
    completed = run_command(
        command,
        cwd=ROOT,
        phase="coverage",
        gate="coverage",
        profile=profile,
        crate=crate,
        stage=stage,
    )
    if completed.returncode != 0:
        raise subprocess.CalledProcessError(completed.returncode, command)


def normalized_flags(flags: object) -> tuple[str, ...]:
    if not isinstance(flags, list) or not all(isinstance(flag, str) for flag in flags):
        raise ValueError("coverage profile flags must be a string list")
    return tuple(flags)


METADATA_COMMAND = ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"]


def metadata_snapshot_path(root: Path) -> Path:
    """Root-relative locked Cargo metadata snapshot path."""
    return root / "quality" / "reports" / "coverage-metadata.json"


def collect_metadata(root: Path) -> Path:
    """Capture the locked Cargo metadata snapshot once per coverage run."""
    completed = run_command(
        METADATA_COMMAND,
        cwd=root,
        phase="coverage",
        gate="coverage",
        stage="metadata",
        capture_output=True,
    )
    if completed.returncode != 0:
        raise subprocess.CalledProcessError(completed.returncode, METADATA_COMMAND)
    snapshot = metadata_snapshot_path(root)
    snapshot.parent.mkdir(parents=True, exist_ok=True)
    temporary = snapshot.with_name(snapshot.name + ".tmp")
    temporary.write_text(completed.stdout, encoding="utf-8")
    temporary.replace(snapshot)
    return snapshot


def main() -> None:
    parser = argparse.ArgumentParser(description="Collect branch-aware coverage.")
    parser.add_argument(
        "--profile",
        choices=["default", "no_default", "all"],
        help="run only one coverage profile instead of all three",
    )
    parser.add_argument(
        "--group",
        choices=["a", "b"],
        help="run only one coverage group (CI parallel split)",
    )
    arguments = parser.parse_args()
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
    if arguments.profile is not None:
        combinations = [
            (name, flags) for name, flags in combinations if name == arguments.profile
        ]
        if not combinations:
            raise ValueError(f"coverage profile {arguments.profile!r} is not configured")

    groups = coverage_policy.get("groups")
    if arguments.group is not None:
        if not isinstance(groups, dict) or arguments.group not in groups:
            raise ValueError(f"coverage group {arguments.group!r} is not configured")
        group_crates = groups[arguments.group]
        if not isinstance(group_crates, list) or not all(
            isinstance(crate, str) for crate in group_crates
        ):
            raise ValueError("coverage groups must be string lists")
        unknown = [crate for crate in group_crates if crate not in coverage_crates]
        if unknown:
            raise ValueError(
                f"coverage group {arguments.group!r} references unknown crates: {', '.join(unknown)}"
            )
        coverage_crates = [crate for crate in coverage_crates if crate in group_crates]
        if not coverage_crates:
            raise ValueError(f"coverage group {arguments.group!r} selects no configured crates")

    REPORTS.mkdir(parents=True, exist_ok=True)
    # Collect the locked workspace metadata snapshot once; every checker
    # invocation below receives the same snapshot so source-root resolution
    # does not re-run `cargo metadata` per report.
    metadata = collect_metadata(ROOT)
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
            coverage_command = (
                "test" if crate in {"intention-daemon", "intention-workspace"} else "nextest"
            )
            coverage_flags = [*flags]
            if crate == "intention-daemon" and "--all-features" not in coverage_flags:
                coverage_flags.append("--all-features")
            effective = tuple(coverage_flags)
            if crate == "intention-daemon" and effective in seen_effective:
                continue
            seen_effective.add(effective)
            # Target narrowing is intentionally disabled: explicit target sets
            # do not reliably reproduce the --all-targets coverage set
            # (Windows integration-target behavior differs, so the coverage
            # gate runs on Linux), and the runner always uses --all-targets to
            # keep per-crate thresholds comparable across coverage runs.
            # Boundary crates use `cargo test` instead of nextest so the
            # library test harness is merged into the coverage report.
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
            run(command, profile=name, crate=crate, stage="collect")
            run(
                [
                    sys.executable,
                    "quality/check_coverage.py",
                    "--report",
                    str(report),
                    "--crate",
                    crate,
                    "--metadata",
                    str(metadata),
                ],
                profile=name,
                crate=crate,
                stage="check",
            )

    for name, flags in combinations:
        # The package reports enforce each crate's denominator.  This aggregate
        # report also exercises dependency code in the same instrumented test
        # process, preventing package isolation from hiding production paths.
        # In the CI group split the aggregate runs once per profile with group
        # a; group b covers per-crate collection only.
        if arguments.group is not None and arguments.group != "a":
            continue
        report = REPORTS / f"coverage-{name}-workspace.json"
        run([
            "cargo", "+nightly-2026-07-31", "llvm-cov", "--branch", "--json",
            "--summary-only", "--output-path", str(report), "nextest",
            "--all-targets", "--workspace", "--locked", *flags,
        ], profile=name, stage="collect")
        run([
            sys.executable, "quality/check_coverage.py", "--report", str(report),
            "--workspace-aggregate", "--metadata", str(metadata),
        ], profile=name, stage="check")


if __name__ == "__main__":
    main()
