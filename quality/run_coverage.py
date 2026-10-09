#!/usr/bin/env python3
"""Collect line-only coverage per crate and for the workspace."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys
import tomllib

try:
    from .timing import run_command
except ImportError:
    from timing import run_command

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


TEST_MARKERS = ("#[test]", "#[cfg(test)]")


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


def crate_has_test_code(root: Path, crate: str) -> bool:
    """Whether a collected crate has any test harness to execute."""
    return any(
        marker in path.read_text(encoding="utf-8")
        for path in (root / "crates" / crate).rglob("*.rs")
        for marker in TEST_MARKERS
    )


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
    for crate in coverage_crates:
        if not crate_has_test_code(ROOT, crate):
            # A compile-only package (for example a newly scaffolded crate
            # with no test code yet) has no test harness, so llvm-cov cannot
            # emit a report for it. Its declared floor applies as soon as the
            # crate gains executable test code.
            print(f"coverage-runner: skipping {crate}: no test code to execute", flush=True)
            continue
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
            "+nightly-2026-07-31",
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
        "cargo", "+nightly-2026-07-31", "llvm-cov", "--json",
        "--summary-only", "--output-path", str(report), "nextest",
        "--all-targets", "--workspace", "--locked", *FEATURE_FLAGS,
    ], stage="collect")
    run([
        sys.executable, "quality/check_coverage.py", "--report", str(report),
        "--workspace-aggregate", "--metadata", str(metadata),
    ], stage="check")


if __name__ == "__main__":
    main()
