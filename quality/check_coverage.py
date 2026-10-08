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


def collected_files(
    root: Path,
    files: list[dict[str, object]],
    collected_crates: set[str],
    source_roots: dict[str, Path],
) -> list[dict[str, object]]:
    crate_names = set(source_roots)
    return [
        item
        for item in files
        if isinstance(item.get("filename"), str)
        and any(
            is_under(
                coverage_path(root, item["filename"], crate_names),
                source_roots.get(crate, root / "__missing__"),
            )
            for crate in collected_crates
        )
    ]


def is_under(path: Path, parent: Path) -> bool:
    try:
        path.resolve().relative_to(parent.resolve())
    except ValueError:
        return False
    return True


def coverage_path(root: Path, filename: str, crate_names: set[str]) -> Path:
    """Map a coverage report filename to its path under the workspace root.

    llvm-cov emits workspace-relative paths and absolute paths whose tail
    mirrors the workspace layout. Re-root only when a ``crates`` path
    component is immediately followed by a known workspace crate directory,
    so an unrelated path that merely contains ``crates/`` is not mistaken for
    a workspace source.
    """
    normalized = filename.replace("\\", "/")
    parts = normalized.split("/")
    for index in range(len(parts) - 1):
        if parts[index] == "crates" and parts[index + 1] in crate_names:
            return (root / "crates" / Path(*parts[index + 1 :])).resolve()
    path = Path(filename)
    return path.resolve() if path.is_absolute() else (root / path).resolve()


def report_path_index(
    root: Path, files: list[dict[str, object]], crate_names: set[str]
) -> dict[Path, list[dict[str, object]]]:
    """Group coverage report entries by their resolved workspace path."""
    index: dict[Path, list[dict[str, object]]] = {}
    for item in files:
        filename = item.get("filename")
        if isinstance(filename, str):
            path = coverage_path(root, filename, crate_names)
            index.setdefault(path, []).append(item)
    return index


def enabled_exclusion_paths(
    root: Path,
    exclusions: object,
    collected_crates: set[str],
    source_roots: dict[str, Path],
    files: list[dict[str, object]],
) -> set[Path]:
    if not isinstance(exclusions, list):
        fail("exclusions must be a list")
    report_paths = report_path_index(root, files, set(source_roots))

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
        if owner not in collected_crates:
            fail(f"enabled exclusion owner must be a collected crate: {owner}")
        source_root = source_roots.get(owner)
        if source_root is None or not is_under(path, source_root):
            fail(f"enabled exclusion path must be under {owner} source root: {relative}")
        occurrences = len(report_paths.get(path, []))
        if occurrences != 1:
            fail(f"enabled exclusion path must appear exactly once in coverage report: {relative}, got {occurrences}")
        excluded.add(path)
        print(f"coverage-check: excluding {relative.as_posix()} from {owner} denominator")
    return excluded


def tier_policy(policy: dict[str, object]) -> tuple[dict[str, float], dict[str, str]]:
    """Resolve the tier ladder and each crate's assigned tier name."""
    tiers = policy.get("tiers")
    if not isinstance(tiers, dict) or not tiers:
        fail("coverage policy is missing the [tiers] table")
    ladder: dict[str, float] = {}
    for name, percent in tiers.items():
        if not isinstance(name, str) or not name:
            fail("coverage tier names must be non-empty strings")
        if not isinstance(percent, (int, float)) or isinstance(percent, bool):
            fail(f"coverage tier {name!r} must be numeric")
        if not 0.0 <= float(percent) <= 100.0:
            fail(f"coverage tier {name!r} must be between 0 and 100")
        ladder[name] = float(percent)
    assignments = policy.get("crate_tiers")
    if not isinstance(assignments, dict) or not assignments:
        fail("coverage policy is missing the [crate_tiers] table")
    placed: dict[str, str] = {}
    for crate, tier in assignments.items():
        if not isinstance(crate, str) or not crate:
            fail("coverage crate names must be non-empty strings")
        if not isinstance(tier, str) or tier not in ladder:
            fail(f"coverage crate {crate!r} must name a tier declared in [tiers]")
        placed[crate] = tier
    return ladder, placed


def collected_crates(ladder: dict[str, float], placed: dict[str, str]) -> list[str]:
    """Return every crate whose tier is above zero; 0% crates are exempt."""
    return sorted(crate for crate, tier in placed.items() if ladder[tier] > 0.0)


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
    try:
        with arguments.policy.open("rb") as policy_file:
            policy = tomllib.load(policy_file)
    except OSError as error:
        fail(f"coverage policy is not readable: {arguments.policy}: {error}")
    except (tomllib.TOMLDecodeError, UnicodeDecodeError) as error:
        fail(f"coverage policy is not valid TOML: {arguments.policy}: {error}")
    try:
        with arguments.report.open(encoding="utf-8") as report_file:
            report = json.load(report_file)
    except OSError as error:
        fail(f"coverage report is not readable: {arguments.report}: {error}")
    except (json.JSONDecodeError, UnicodeDecodeError) as error:
        fail(f"coverage report is not valid JSON: {arguments.report}: {error}")

    policy_state = policy.get("policy")
    if not isinstance(policy_state, dict):
        fail("coverage policy is missing the [policy] table")
    # The approved policy intentionally has no coverage override mechanism.
    if "coverage_overrides" in policy:
        fail("coverage policy must not define coverage_overrides")
    ladder, placed = tier_policy(policy)
    collected = collected_crates(ladder, placed)
    if arguments.crate is not None:
        if arguments.crate not in placed:
            fail(f"coverage crate {arguments.crate!r} is not declared in [crate_tiers]")
        if ladder[placed[arguments.crate]] == 0.0:
            fail(f"coverage crate {arguments.crate!r} is exempt (0% tier) and is not collected")
        crate_scope = [arguments.crate]
    else:
        crate_scope = collected
    collected_set = set(collected)
    exclusions = policy.get("exclusions", [])
    if arguments.metadata is not None:
        source_roots = metadata_source_roots(arguments.metadata, root)
    else:
        source_roots = package_source_roots(root)
    crate_names = set(source_roots)
    files = report_files(report)
    if arguments.workspace_aggregate:
        # Aggregate reports intentionally contain source files from exempt and
        # uncollected crates too. They are informational: the aggregate metric
        # counts collected crates only and no aggregate threshold exists, while
        # every exclusion must still be valid.
        aggregate_files = collected_files(root, files, collected_set, source_roots)
        if not aggregate_files:
            fail("workspace aggregate report contains no collected crate source files")
        excluded_paths = enabled_exclusion_paths(root, exclusions, collected_set, source_roots, files)
        covered, count = line_totals(
            [
                item
                for item in aggregate_files
                if coverage_path(root, item["filename"], crate_names) not in excluded_paths
            ]
        )
        if count == 0:
            fail("workspace aggregate has no reportable non-excluded source lines")
        observed = 100.0 * covered / count
        print(
            f"coverage-check: workspace aggregate line coverage {observed:.3f}% ({covered}/{count}) "
            "over collected crates; per-crate tier thresholds are enforced by the crate reports"
        )
        return
    # An isolated package report legitimately cannot contain exclusions owned
    # by another crate. Validate only the entries relevant to the report's crate.
    report_exclusions = (
        [item for item in exclusions if isinstance(item, dict) and item.get("owner") == arguments.crate]
        if arguments.crate is not None
        else exclusions
    )
    # Exclusions subtract their paths from the owning crate's numerator and
    # denominator only; they never waive the tier floor itself.
    excluded_paths = enabled_exclusion_paths(root, report_exclusions, collected_set, source_roots, files)

    for crate in crate_scope:
        tier = placed[crate]
        required = ladder[tier]
        source_root = source_roots.get(crate)
        if source_root is None:
            fail(f"coverage crate {crate!r} is absent from Cargo metadata")
        crate_files = [
            item
            for item in files
            if isinstance(item.get("filename"), str)
            and is_under(coverage_path(root, item["filename"], crate_names), source_root)
            and coverage_path(root, item["filename"], crate_names) not in excluded_paths
        ]
        covered, count = line_totals(crate_files)
        if count == 0:
            fail(f"coverage crate {crate!r} has no reportable non-excluded source lines")
        observed = 100.0 * covered / count
        if observed < required:
            fail(
                f"{crate} line coverage {observed:.3f}% is below required "
                f"{tier} tier threshold {required:.3f}%"
            )
        print(
            f"coverage-check: {crate} line coverage {observed:.2f}% "
            f"satisfies {tier} tier threshold {required:.2f}%"
        )


if __name__ == "__main__":
    main()
