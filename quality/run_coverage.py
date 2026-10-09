#!/usr/bin/env python3
"""Collect line-only coverage per crate and for the workspace."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import tomllib

try:
    from .timing import run_command
    from .toolchains import nightly_selector
except ImportError:
    from timing import run_command
    from toolchains import nightly_selector

ROOT = Path(__file__).resolve().parents[1]
COVERAGE_POLICY = ROOT / "quality" / "coverage.toml"
REPORTS = ROOT / "quality" / "reports"

# The workspace declares exactly one feature (`test-support`, non-production on
# `intention-daemon`), so one configuration collects every executed path.
FEATURE_FLAGS = ["--all-features"]


def run(command: list[str], *, crate: str = "", stage: str = "command") -> None:
    completed = run_command(
        command,
        cwd=ROOT,
        phase="coverage",
        gate="coverage",
        crate=crate,
        stage=stage,
    )
    if completed.returncode != 0:
        raise subprocess.CalledProcessError(completed.returncode, command)


def collected_crates(coverage_policy: dict[str, object]) -> list[str]:
    """Return every crate whose coverage tier is above zero."""
    tiers = coverage_policy.get("tiers")
    assignments = coverage_policy.get("crate_tiers")
    if not isinstance(tiers, dict) or not isinstance(assignments, dict):
        raise ValueError("coverage policy requires [tiers] and [crate_tiers] tables")
    collected: list[str] = []
    for crate, tier in assignments.items():
        percent = tiers.get(tier) if isinstance(tier, str) else None
        if not isinstance(crate, str) or not isinstance(percent, (int, float)) or isinstance(percent, bool):
            raise ValueError("coverage [crate_tiers] entries must name a numeric tier")
        if float(percent) > 0.0:
            collected.append(crate)
    return sorted(collected)


def unexecutable_crates(coverage_policy: dict[str, object]) -> list[str]:
    """Return the collected crates the policy declares as having no test target."""
    state = coverage_policy.get("policy")
    declared = state.get("unexecutable_crates") if isinstance(state, dict) else None
    if not isinstance(declared, list) or not all(isinstance(name, str) for name in declared):
        raise ValueError("coverage [policy].unexecutable_crates must be a string list")
    return declared


def metadata_packages(snapshot: Path) -> dict[str, dict[str, object]]:
    """Read the runner's locked Cargo metadata snapshot into a package map."""
    try:
        with snapshot.open(encoding="utf-8") as metadata_file:
            metadata = json.load(metadata_file)
    except FileNotFoundError:
        raise SystemExit(f"coverage-runner: metadata snapshot is missing: {snapshot}") from None
    except json.JSONDecodeError as error:
        raise SystemExit(f"coverage-runner: metadata snapshot is not valid JSON: {snapshot}: {error}") from None
    packages = metadata.get("packages") if isinstance(metadata, dict) else None
    if not isinstance(packages, list):
        raise SystemExit("coverage-runner: metadata snapshot does not expose a packages list")
    result: dict[str, dict[str, object]] = {}
    for package in packages:
        if not isinstance(package, dict) or not isinstance(package.get("name"), str):
            raise SystemExit("coverage-runner: metadata snapshot packages must name a package")
        result[package["name"]] = package
    return result


def package_has_test_target(package: dict[str, object]) -> bool:
    """Whether Cargo declares any target that executes tests for this package.

    Test executability is decided from the Cargo metadata snapshot rather than a
    source-text marker, so a `#[tokio::test]` integration target counts exactly
    like a `#[test]` one and a crate whose directory name diverges from its
    package name cannot hide a test target.
    """
    name = package.get("name")
    targets = package.get("targets")
    if not isinstance(targets, list):
        raise SystemExit(f"coverage-runner: package {name} has no target list")
    for target in targets:
        if not isinstance(target, dict):
            raise SystemExit(f"coverage-runner: package {name} has an invalid target")
        kinds = target.get("kind")
        if not isinstance(kinds, list) or not all(isinstance(kind, str) for kind in kinds):
            raise SystemExit(f"coverage-runner: package {name} has an invalid target shape")
        if target.get("test") is True and set(kinds) & {"test", "lib", "bin"}:
            return True
    return False


METADATA_COMMAND = ["cargo", nightly_selector(), "metadata", "--no-deps", "--format-version", "1", "--locked"]


def coverage_plan(
    coverage_crates: list[str],
    packages: dict[str, dict[str, object]],
    unexecutable: set[str],
) -> tuple[list[str], list[str], list[str]]:
    """Return the crates to collect, the legal skips, and the plan failures.

    A collected crate must exist in the locked metadata snapshot and must have a
    test-executing target unless the policy declares it unexecutable. A
    declaration for a crate that does have one, or for a crate outside
    collection, is a failure: the declared floor can never silently retire.
    """
    collect: list[str] = []
    skip: list[str] = []
    failures: list[str] = []
    for crate in coverage_crates:
        package = packages.get(crate)
        if package is None:
            failures.append(f"{crate}: collected crate is absent from the locked Cargo metadata snapshot")
            continue
        executable = package_has_test_target(package)
        if crate in unexecutable:
            if executable:
                failures.append(
                    f"{crate}: [policy].unexecutable_crates lists it, but Cargo declares a "
                    "test-executing target; remove the stale declaration"
                )
            else:
                skip.append(crate)
            continue
        if not executable:
            failures.append(
                f"{crate}: collected but Cargo declares no test-executing target; add a test target "
                "or declare the crate in quality/coverage.toml [policy].unexecutable_crates"
            )
            continue
        collect.append(crate)
    uncollected = sorted(unexecutable - set(coverage_crates))
    if uncollected:
        failures.append(
            "[policy].unexecutable_crates names crates that are not collected: " + ", ".join(uncollected)
        )
    return collect, skip, failures


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
    with COVERAGE_POLICY.open("rb") as policy_file:
        coverage_policy = tomllib.load(policy_file)
    coverage_crates = collected_crates(coverage_policy)

    # `cargo llvm-cov` merges every profile data file left in
    # target/llvm-cov-target, so data from an interrupted run mixes
    # instrumentation from an older build into the merge. The report then
    # counts regions whose counts were never merged, and a crate can appear
    # tens of points below its real coverage. Run `make
    # coverage-artifacts-clean` after interrupting a coverage pass; the
    # Makefile does it after a passing pass.
    REPORTS.mkdir(parents=True, exist_ok=True)
    # Collect the locked workspace metadata snapshot once; every checker
    # invocation below receives the same snapshot so source-root resolution
    # does not re-run `cargo metadata` per report.
    metadata = collect_metadata(ROOT)
    packages = metadata_packages(metadata)
    collect, skip, plan_failures = coverage_plan(
        coverage_crates, packages, set(unexecutable_crates(coverage_policy))
    )
    if plan_failures:
        raise SystemExit("coverage-runner: " + "\ncoverage-runner: ".join(plan_failures))
    for crate in skip:
        print(
            f"coverage-runner: skipping {crate}: the policy declares no test-executing target",
            flush=True,
        )
    for crate in collect:
        report = (REPORTS / f"coverage-{crate}.json").resolve()
        # `cargo llvm-cov nextest --package` instruments the package's
        # integration binaries, but nextest's workspace execution model does
        # not reliably merge the package library test harness. The latter is
        # especially important for boundary crates whose implementation lives
        # in lib.rs. Cargo test executes both the library harness and
        # integration targets in one coverage run.
        coverage_command = (
            "test" if crate in {"intention-daemon", "intention-tools"} else "nextest"
        )
        # Target narrowing is intentionally disabled: explicit target sets do
        # not reliably reproduce the --all-targets coverage set (Windows
        # integration-target behavior differs, so the coverage gate runs on
        # Linux), and the runner always uses --all-targets to keep per-crate
        # thresholds comparable across coverage runs.
        command = [
            "cargo",
            nightly_selector(),
            "llvm-cov",
            "--json",
            "--summary-only",
            "--output-path",
            str(report),
            coverage_command,
            "--all-targets",
            "--locked",
            *FEATURE_FLAGS,
            "--package",
            crate,
        ]
        run(command, crate=crate, stage="collect")
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
            crate=crate,
            stage="check",
        )

    # The package reports enforce each crate's tier floor. This aggregate
    # report also exercises dependency code in the same instrumented test
    # process, preventing package isolation from hiding production paths; its
    # line metric stays informational over collected crates only.
    report = REPORTS / "coverage-workspace.json"
    run([
        "cargo", nightly_selector(), "llvm-cov", "--json",
        "--summary-only", "--output-path", str(report), "nextest",
        "--all-targets", "--workspace", "--locked", *FEATURE_FLAGS,
    ], stage="collect")
    run([
        sys.executable, "quality/check_coverage.py", "--report", str(report),
        "--workspace-aggregate", "--metadata", str(metadata),
    ], stage="check")


if __name__ == "__main__":
    main()
