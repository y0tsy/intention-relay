#!/usr/bin/env python3
"""Prove the coverage runner's metadata-driven executability decisions."""

from __future__ import annotations

import json
from pathlib import Path
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "quality"))

from run_coverage import (  # noqa: E402
    coverage_plan,
    metadata_packages,
    package_has_test_target,
    unexecutable_crates,
)

COVERAGE_POLICY = ROOT / "quality" / "coverage.toml"


def lib_target(name: str, *, tests: bool) -> dict[str, object]:
    return {"name": name, "kind": ["lib"], "test": tests}


def test_tokio_only_integration_target_is_executable() -> None:
    """A `#[tokio::test]` integration target counts exactly like a `#[test]` one."""
    package = {
        "name": "intention-tui",
        "targets": [lib_target("intention_tui", tests=True), {"name": "tui_contract", "kind": ["test"], "test": True}],
    }
    if not package_has_test_target(package):
        raise RuntimeError("a tokio-only integration target must count as executable")


def test_metadata_decides_executability_not_source_text() -> None:
    """Cargo metadata is the authority, so a directory rename cannot hide a target."""
    package = {"name": "renamed-directory", "targets": [lib_target("lib", tests=True)]}
    if not package_has_test_target(package):
        raise RuntimeError("the metadata snapshot must decide executability")


def test_target_without_tests_is_not_executable() -> None:
    """A compile-only target cannot satisfy a declared coverage floor."""
    package = {"name": "fixture", "targets": [lib_target("fixture", tests=False)]}
    if package_has_test_target(package):
        raise RuntimeError("a target that does not execute tests must not count as executable")


def test_undeclared_skip_fails() -> None:
    """A collected crate with no test target fails unless the policy declares it."""
    packages = {"fixture": {"name": "fixture", "targets": [lib_target("fixture", tests=False)]}}
    collect, skip, failures = coverage_plan(["fixture"], packages, set())
    if not failures or collect or skip:
        raise RuntimeError(f"an undeclared skip must fail: {collect=} {skip=} {failures=}")
    if "quality/coverage.toml" not in failures[0]:
        raise RuntimeError(f"the failure must name the remedy: {failures=}")


def test_declared_skip_is_legal() -> None:
    """A policy declaration makes the skip legal and keeps it out of collection."""
    packages = {"fixture": {"name": "fixture", "targets": [lib_target("fixture", tests=False)]}}
    collect, skip, failures = coverage_plan(["fixture"], packages, {"fixture"})
    if failures or collect or skip != ["fixture"]:
        raise RuntimeError(f"a declared skip must pass: {collect=} {skip=} {failures=}")


def test_stale_declaration_fails() -> None:
    """A declaration whose crate does have a test target is stale and fails."""
    packages = {"fixture": {"name": "fixture", "targets": [lib_target("fixture", tests=True)]}}
    collect, skip, failures = coverage_plan(["fixture"], packages, {"fixture"})
    if not failures or collect or skip:
        raise RuntimeError(f"a stale declaration must fail: {collect=} {skip=} {failures=}")


def test_declaration_outside_collection_fails() -> None:
    """A declaration for a crate outside collection can never take effect."""
    packages = {"fixture": {"name": "fixture", "targets": [lib_target("fixture", tests=True)]}}
    _, _, failures = coverage_plan(["fixture"], packages, {"fixture", "other"})
    if not any("not collected" in failure for failure in failures):
        raise RuntimeError(f"a declaration outside collection must fail: {failures=}")


def test_missing_snapshot_entry_fails() -> None:
    """A collected crate absent from the snapshot cannot be checked at all."""
    _, _, failures = coverage_plan(["fixture"], {}, set())
    if not any("absent from the locked Cargo metadata snapshot" in failure for failure in failures):
        raise RuntimeError(f"a missing snapshot entry must fail: {failures=}")


def test_snapshot_round_trip(root: Path) -> None:
    """The runner's own metadata snapshot drives the decision end to end."""
    snapshot = root / "coverage-metadata.json"
    snapshot.write_text(
        json.dumps(
            {
                "packages": [
                    {
                        "name": "intention-tui",
                        "targets": [{"name": "tui_contract", "kind": ["test"], "test": True}],
                    }
                ]
            }
        ),
        encoding="utf-8",
    )
    packages = metadata_packages(snapshot)
    collect, skip, failures = coverage_plan(["intention-tui"], packages, set())
    if failures or skip or collect != ["intention-tui"]:
        raise RuntimeError(f"the snapshot round trip must collect: {collect=} {skip=} {failures=}")


def test_live_policy_declares_no_unexecutable_crate() -> None:
    """The live policy skips nothing, so every collected crate keeps its floor."""
    with COVERAGE_POLICY.open("rb") as policy_file:
        policy = tomllib.load(policy_file)
    declared = unexecutable_crates(policy)
    if declared:
        raise RuntimeError(f"the live coverage policy must declare no unexecutable crate: {declared}")


def main() -> None:
    tests = [
        test_tokio_only_integration_target_is_executable,
        test_metadata_decides_executability_not_source_text,
        test_target_without_tests_is_not_executable,
        test_undeclared_skip_fails,
        test_declared_skip_is_legal,
        test_stale_declaration_fails,
        test_declaration_outside_collection_fails,
        test_missing_snapshot_entry_fails,
        test_live_policy_declares_no_unexecutable_crate,
    ]
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        for test in tests:
            print(f"coverage-runner self-test: {test.__name__}", flush=True)
            test()
        print("coverage-runner self-test: test_snapshot_round_trip", flush=True)
        test_snapshot_round_trip(root)
    print(f"coverage-runner self-test: {len(tests) + 1} focused cases passed")


if __name__ == "__main__":
    main()
