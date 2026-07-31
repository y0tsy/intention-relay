#!/usr/bin/env python3
"""Validate per-crate coverage tiers and cargo-llvm-cov summary reports."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_POLICY = ROOT / "quality" / "coverage.toml"


def fail(message: str) -> None:
    print(f"coverage-check: {message}", file=sys.stderr)
    raise SystemExit(1)


def package_source_roots() -> dict[str, Path]:
    completed = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
        check=True,
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    metadata = json.loads(completed.stdout)
    return {
        package["name"]: Path(package["manifest_path"]).parent / "src"
        for package in metadata["packages"]
    }


def report_files(report: object) -> list[dict[str, object]]:
    if not isinstance(report, dict):
        fail("coverage report must be a JSON object")
    data = report.get("data")
    if not isinstance(data, list):
        fail("coverage report does not expose data entries")
    files: list[dict[str, object]] = []
    for entry in data:
        if not isinstance(entry, dict):
            continue
        entry_files = entry.get("files")
        if isinstance(entry_files, list):
            files.extend(item for item in entry_files if isinstance(item, dict))
    return files


def require_branch_metrics(files: list[dict[str, object]]) -> None:
    branch_metrics_found = False
    for item in files:
        summary = item.get("summary")
        branches = summary.get("branches") if isinstance(summary, dict) else None
        if isinstance(branches, dict) and isinstance(branches.get("percent"), (int, float)):
            branch_metrics_found = True
            break
    if not branch_metrics_found:
        fail("coverage report does not expose branch coverage")


def line_totals(files: list[dict[str, object]]) -> tuple[int, int]:
    covered = 0
    count = 0
    for item in files:
        summary = item.get("summary")
        lines = summary.get("lines") if isinstance(summary, dict) else None
        if not isinstance(lines, dict):
            continue
        file_count = lines.get("count")
        file_covered = lines.get("covered")
        if isinstance(file_count, int) and isinstance(file_covered, int):
            count += file_count
            covered += file_covered
    return covered, count


def is_under(path: Path, parent: Path) -> bool:
    try:
        path.resolve().relative_to(parent.resolve())
    except ValueError:
        return False
    return True


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    parser.add_argument("--report", type=Path, required=True)
    arguments = parser.parse_args()

    with arguments.policy.open("rb") as policy_file:
        policy = tomllib.load(policy_file)
    with arguments.report.open(encoding="utf-8") as report_file:
        report = json.load(report_file)

    tiers = policy.get("tiers")
    policy_state = policy.get("policy")
    classifications = policy.get("crate_tiers")
    exclusions = policy.get("exclusions", [])
    if not isinstance(tiers, dict) or not isinstance(policy_state, dict) or not isinstance(classifications, dict):
        fail("coverage policy is missing required tables")
    for tier, threshold in tiers.items():
        if tier not in {"A", "B", "C"} or not isinstance(threshold, (int, float)):
            fail("tiers must define numeric A, B, and C thresholds")
    for exclusion in exclusions:
        if exclusion.get("enabled"):
            for field in ("path", "rationale", "owner", "equivalent_test_evidence"):
                if not exclusion.get(field):
                    fail(f"enabled exclusion requires {field}")

    production_crates = policy_state.get("production_crates")
    if not isinstance(production_crates, list):
        fail("production_crates must be a list")
    source_roots = package_source_roots()
    files = report_files(report)
    require_branch_metrics(files)
    for crate in production_crates:
        tier = classifications.get(crate)
        if tier not in tiers:
            fail(f"production crate {crate!r} lacks a valid coverage tier")
        source_root = source_roots.get(crate)
        if source_root is None:
            fail(f"production crate {crate!r} is absent from Cargo metadata")
        crate_files = [
            item
            for item in files
            if isinstance(item.get("filename"), str) and is_under(Path(item["filename"]), source_root)
        ]
        covered, count = line_totals(crate_files)
        if count == 0:
            fail(f"production crate {crate!r} has no reportable source lines")
        observed = 100.0 * covered / count
        required = float(tiers[tier])
        if observed < required:
            fail(f"{crate} line coverage {observed:.2f}% is below required {required:.2f}%")
        print(f"coverage-check: {crate} line coverage {observed:.2f}% satisfies tier {tier}")

    if not production_crates:
        print("coverage-check: M0 has no production crate coverage thresholds")


if __name__ == "__main__":
    main()
