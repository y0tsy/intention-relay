#!/usr/bin/env python3
"""Collect branch-aware per-crate and workspace coverage."""

from __future__ import annotations

from pathlib import Path
import json
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


def run(command: list[str]) -> None:
    completed = run_command(command, cwd=ROOT, phase="coverage", gate="coverage")
    if completed.returncode != 0:
        raise subprocess.CalledProcessError(completed.returncode, command)


def normalized_flags(flags: object) -> tuple[str, ...]:
    if not isinstance(flags, list) or not all(isinstance(flag, str) for flag in flags):
        raise ValueError("coverage profile flags must be a string list")
    return tuple(flags)


def metadata_targets() -> dict[str, set[str]]:
    """Return each package's declared target kinds from locked Cargo metadata."""
    completed = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
        check=True, capture_output=True, text=True, cwd=ROOT,
    )
    metadata = json.loads(completed.stdout)
    result: dict[str, set[str]] = {}
    for package in metadata["packages"]:
        result[package["name"]] = {
            kind
            for target in package["targets"]
            for kind in target["kind"]
            if kind in {"lib", "bin", "test", "example", "bench"}
        }
    return result


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
    targets_by_crate = metadata_targets()
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
            # Target narrowing is enabled only when the coverage policy
            # declares an explicit target set for this crate and that set
            # exactly equals the metadata-derived inventory. Otherwise the
            # runner fails closed and keeps --all-targets.
            target_flags = ["--all-targets"]
            declared_targets = next(
                (
                    entry["targets"]
                    for entry in coverage_policy.get("coverage_targets", [])
                    if entry.get("crate") == crate and isinstance(entry.get("targets"), list)
                ),
                None,
            )
            if declared_targets is not None and set(declared_targets) == targets_by_crate.get(
                crate, set()
            ):
                target_flags = []
                for target in declared_targets:
                    if target == "lib":
                        target_flags.append("--lib")
                    elif target == "bin":
                        target_flags.append("--bins")
                    elif target == "test":
                        target_flags.append("--tests")
                    elif target == "example":
                        target_flags.append("--examples")
                    elif target == "bench":
                        target_flags.append("--benches")
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
