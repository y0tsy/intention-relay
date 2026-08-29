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


def package_source_roots_from_metadata(metadata: object, root: Path) -> dict[str, Path]:
    """Extract package source roots from a Cargo metadata snapshot."""
    if not isinstance(metadata, dict):
        fail("Cargo metadata snapshot must be a JSON object")
    packages = metadata.get("packages")
    if not isinstance(packages, list):
        fail("Cargo metadata snapshot does not expose a packages list")
    roots: dict[str, Path] = {}
    for package in packages:
        if not isinstance(package, dict):
            fail("Cargo metadata snapshot packages must be JSON objects")
        name = package.get("name")
        manifest_path = package.get("manifest_path")
        if not isinstance(name, str) or not isinstance(manifest_path, str):
            fail("Cargo metadata snapshot packages must define name and manifest_path strings")
        manifest = Path(manifest_path)
        if not manifest.is_absolute():
            manifest = root / manifest
        # Resolve before containment so symlinked and traversed manifest
        # paths are checked at their actual location, and derive the source
        # root from that resolved location.
        resolved_manifest = manifest.resolve()
        if not is_under(resolved_manifest, root):
            fail(
                f"coverage metadata package {name!r} manifest_path must not escape workspace root: {manifest_path}"
            )
        roots[name] = (resolved_manifest.parent / "src").resolve()
    return roots


def metadata_source_roots(metadata_path: Path, root: Path) -> dict[str, Path]:
    """Read and validate the runner's Cargo metadata snapshot."""
    try:
        with metadata_path.open(encoding="utf-8") as metadata_file:
            metadata = json.load(metadata_file)
    except FileNotFoundError:
        fail(f"coverage metadata snapshot is missing: {metadata_path}")
    except (json.JSONDecodeError, OSError) as error:
        fail(f"coverage metadata snapshot is not valid JSON: {metadata_path}: {error}")
    return package_source_roots_from_metadata(metadata, root)


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


def production_files(
    root: Path,
    files: list[dict[str, object]],
    production_crates: set[str],
    source_roots: dict[str, Path],
) -> list[dict[str, object]]:
    return [
        item
        for item in files
        if isinstance(item.get("filename"), str)
        and any(
            is_under(coverage_path(root, item["filename"]), source_roots.get(crate, root / "__missing__"))
            for crate in production_crates
        )
    ]


def is_under(path: Path, parent: Path) -> bool:
    try:
        path.resolve().relative_to(parent.resolve())
    except ValueError:
        return False
    return True


def coverage_path(root: Path, filename: str) -> Path:
    normalized = filename.replace("\\", "/")
    marker = "/crates/"
    if marker in normalized:
        return (root / "crates" / normalized.split(marker, 1)[1]).resolve()
    path = Path(filename)
    return path.resolve() if path.is_absolute() else (root / path).resolve()


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
            path = coverage_path(root, filename)
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
        print(f"coverage-check: excluding {relative.as_posix()} from {owner} denominator")
    return excluded


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--crate")
    parser.add_argument("--workspace-aggregate", action="store_true")
    parser.add_argument("--metadata", type=Path, help="Cargo metadata snapshot captured by the coverage runner")
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
    if set(tiers) != {"A", "B", "C"} or not all(isinstance(threshold, (int, float)) for threshold in tiers.values()):
        fail("tiers must define numeric A, B, and C thresholds")

    production_crates = policy_state.get("production_crates")
    if not isinstance(production_crates, list) or not all(isinstance(crate, str) for crate in production_crates):
        fail("production_crates must be a string list")
    production_set = set(production_crates)
    # Thresholds come directly from each crate's declared tier.  The approved
    # policy intentionally has no coverage override mechanism.
    if "coverage_overrides" in policy:
        fail("coverage policy must not define coverage_overrides")
    if arguments.crate is not None:
        if arguments.crate not in production_set:
            fail(f"coverage crate {arguments.crate!r} is not an active production crate")
        production_crates = [arguments.crate]
    if arguments.metadata is not None:
        source_roots = metadata_source_roots(arguments.metadata, root)
    else:
        source_roots = package_source_roots(root)
    files = report_files(report)
    require_branch_metrics(files)
    if arguments.workspace_aggregate:
        # Aggregate reports intentionally contain production source files from
        # every crate. Validate their aggregate line metric and exclusion
        # declarations, while leaving per-crate thresholds to crate reports.
        aggregate_files = production_files(root, files, production_set, source_roots)
        if not aggregate_files:
            fail("workspace aggregate report contains no production source files")
        excluded_paths = enabled_exclusion_paths(root, exclusions, production_set, source_roots, files)
        covered, count = line_totals(
            [item for item in aggregate_files if coverage_path(root, item["filename"]) not in excluded_paths]
        )
        if count == 0:
            fail("workspace aggregate has no reportable non-excluded source lines")
        observed = 100.0 * covered / count
        print(f"coverage-check: workspace aggregate line coverage {observed:.3f}% ({covered}/{count})")
        return
    # An isolated package report legitimately cannot contain exclusions owned by
    # another package. Validate only exclusions relevant to the report's crate.
    report_exclusions = (
        [item for item in exclusions if isinstance(item, dict) and item.get("owner") == arguments.crate]
        if arguments.crate is not None
        else exclusions
    )
    excluded_paths = enabled_exclusion_paths(root, report_exclusions, production_set, source_roots, files)

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
            and is_under(coverage_path(root, item["filename"]), source_root)
            and coverage_path(root, item["filename"]) not in excluded_paths
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
