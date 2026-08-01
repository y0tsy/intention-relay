#!/usr/bin/env python3
"""Validate per-crate coverage tiers and exact exclusion semantics."""

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


def package_source_roots(root: Path) -> dict[str, Path]:
    completed = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
        check=True,
        capture_output=True,
        text=True,
        cwd=root,
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
    for item in files:
        summary = item.get("summary")
        branches = summary.get("branches") if isinstance(summary, dict) else None
        if isinstance(branches, dict) and isinstance(branches.get("percent"), (int, float)):
            return
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


def enabled_exclusion_paths(
    root: Path,
    exclusions: object,
    production_crates: set[str],
    source_roots: dict[str, Path],
    files: list[dict[str, object]],
) -> set[Path]:
    if not isinstance(exclusions, list):
        fail("exclusions must be a list")
    report_paths: dict[Path, int] = {}
    for item in files:
        filename = item.get("filename")
        if isinstance(filename, str):
            path = Path(filename).resolve()
            report_paths[path] = report_paths.get(path, 0) + 1

    excluded: set[Path] = set()
    for exclusion in exclusions:
        if not isinstance(exclusion, dict):
            fail("coverage exclusion must be a table")
        enabled = exclusion.get("enabled", False)
        if not isinstance(enabled, bool):
            fail("coverage exclusion enabled must be a boolean")
        if not enabled:
            continue
        for field in ("path", "rationale", "owner", "equivalent_test_evidence"):
            value = exclusion.get(field)
            if not isinstance(value, str) or not value:
                fail(f"enabled exclusion requires {field}")
        relative = Path(str(exclusion["path"]))
        if relative.is_absolute() or ".." in relative.parts:
            fail(f"enabled exclusion path must be workspace-relative without traversal: {relative}")
        path = (root / relative).resolve()
        if path in excluded:
            fail(f"duplicate enabled exclusion path: {relative}")
        if not path.is_file():
            fail(f"enabled exclusion path must be an existing regular file: {relative}")
        owner = str(exclusion["owner"])
        if owner not in production_crates:
            fail(f"enabled exclusion owner must be an active production crate: {owner}")
        source_root = source_roots.get(owner)
        if source_root is None or not is_under(path, source_root):
            fail(f"enabled exclusion path must be under {owner} source root: {relative}")
        occurrences = report_paths.get(path, 0)
        if occurrences != 1:
            fail(f"enabled exclusion path must appear exactly once in coverage report: {relative}, got {occurrences}")
        excluded.add(path)
        print(f"coverage-check: excluding {relative} from {owner} denominator")
    return excluded


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    parser.add_argument("--report", type=Path, required=True)
    arguments = parser.parse_args()

    root = arguments.root.resolve()
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

    production_crates = policy_state.get("production_crates")
    if not isinstance(production_crates, list) or not all(isinstance(crate, str) for crate in production_crates):
        fail("production_crates must be a string list")
    production_set = set(production_crates)
    source_roots = package_source_roots(root)
    files = report_files(report)
    require_branch_metrics(files)
    excluded_paths = enabled_exclusion_paths(root, exclusions, production_set, source_roots, files)

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
            if isinstance(item.get("filename"), str)
            and is_under(Path(item["filename"]), source_root)
            and Path(item["filename"]).resolve() not in excluded_paths
        ]
        covered, count = line_totals(crate_files)
        if count == 0:
            fail(f"production crate {crate!r} has no reportable non-excluded source lines")
        observed = 100.0 * covered / count
        required = float(tiers[tier])
        if observed < required:
            fail(f"{crate} line coverage {observed:.2f}% is below required {required:.2f}%")
        print(f"coverage-check: {crate} line coverage {observed:.2f}% satisfies tier {tier}")

    if not production_crates:
        print("coverage-check: M0 has no production crate coverage thresholds")


if __name__ == "__main__":
    main()
