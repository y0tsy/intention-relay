#!/usr/bin/env python3
"""Prove that M0 quality gates reject deliberately invalid isolated inputs."""

from __future__ import annotations

import argparse
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str], *, cwd: Path = ROOT, expect_success: bool) -> None:
    completed = subprocess.run(command, cwd=cwd, capture_output=True, text=True)
    succeeded = completed.returncode == 0
    if succeeded != expect_success:
        output = (completed.stdout + "\n" + completed.stderr).strip()
        expectation = "succeed" if expect_success else "fail"
        raise RuntimeError(f"expected {' '.join(command)} to {expectation}, got {completed.returncode}: {output}")


def copy_root() -> tempfile.TemporaryDirectory[str]:
    temporary = tempfile.TemporaryDirectory()
    destination = Path(temporary.name) / "repo"
    ignored = shutil.ignore_patterns(".git", "target", "__pycache__", "reports")
    shutil.copytree(ROOT, destination, ignore=ignored)
    return temporary


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise RuntimeError(f"fixture replacement source not found in {path}: {old!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def test_formatting_drift() -> None:
    with copy_root() as temporary:
        root = Path(temporary) / "repo"
        (root / "quality/harness/src/lib.rs").write_text("pub const fn broken()->u8{1}\n", encoding="utf-8")
        run(["cargo", "fmt", "--all", "--check"], cwd=root, expect_success=False)


def test_lint_warning() -> None:
    with copy_root() as temporary:
        root = Path(temporary) / "repo"
        (root / "quality/harness/src/lib.rs").write_text("pub fn warning() { let unused = 1; }\n", encoding="utf-8")
        run(["cargo", "clippy", "--workspace", "--all-targets", "--locked", "--", "-Dwarnings"], cwd=root, expect_success=False)


def test_unreasoned_suppression() -> None:
    with copy_root() as temporary:
        root = Path(temporary) / "repo"
        harness = root / "quality/harness/src/lib.rs"
        harness.write_text("#[allow(clippy::unwrap_used)]\npub fn invalid() {}\n", encoding="utf-8")
        run([sys.executable, "quality/check_architecture.py"], cwd=root, expect_success=False)


def test_tool_version_mismatch() -> None:
    with copy_root() as temporary:
        root = Path(temporary) / "repo"
        tools = root / "quality/tools.toml"
        replace_once(tools, 'version = "0.9.140"', 'version = "0.0.0"')
        run([sys.executable, "quality/check_tools.py", "--policy", str(tools)], cwd=root, expect_success=False)


def test_missing_crate_metadata() -> None:
    with copy_root() as temporary:
        root = Path(temporary) / "repo"
        policy = root / "quality/architecture.toml"
        replace_once(policy, 'test_target = "dto and validation tests"', 'test_target = ""')
        run([sys.executable, "quality/check_architecture.py", "--policy", str(policy)], cwd=root, expect_success=False)


def test_forbidden_source_boundary() -> None:
    with copy_root() as temporary:
        root = Path(temporary) / "repo"
        (root / "quality/harness/src/forbidden.rs").write_text("use rusqlite::Connection;\n", encoding="utf-8")
        run([sys.executable, "quality/check_architecture.py"], cwd=root, expect_success=False)


def test_coverage_failures() -> None:
    with copy_root() as temporary:
        root = Path(temporary) / "repo"
        policy = root / "quality/coverage.toml"
        report = root / "quality/fixtures/coverage-low.json"
        policy.write_text(
            policy.read_text(encoding="utf-8").replace(
                "production_crates = []", 'production_crates = ["quality-harness"]'
            ).replace("[crate_tiers]\n", '[crate_tiers]\nquality-harness = "A"\n'),
            encoding="utf-8",
        )
        report.parent.mkdir(parents=True, exist_ok=True)
        report.write_text(
            '{"data": [{"files": [{"filename": "' + str(root / "quality/harness/src/lib.rs") + '", "summary": {"lines": {"count": 100, "covered": 1}}}], "totals": {"branches": {"percent": 0.0}}}]}\n',
            encoding="utf-8",
        )
        run([sys.executable, "quality/check_coverage.py", "--policy", str(policy), "--report", str(report)], cwd=root, expect_success=False)
        report.write_text(
            '{"data": [{"files": [{"filename": "' + str(root / "quality/harness/src/lib.rs") + '", "summary": {"lines": {"count": 100, "covered": 100}}}], "totals": {"branches": {"percent": 100.0}}}]}\n',
            encoding="utf-8",
        )
        replace_once(policy, "enabled = false", "enabled = true")
        run([sys.executable, "quality/check_coverage.py", "--policy", str(policy), "--report", str(report)], cwd=root, expect_success=False)


def test_missing_feature_profile() -> None:
    with copy_root() as temporary:
        root = Path(temporary) / "repo"
        policy = root / "quality/features.toml"
        replace_once(policy, "no_default = [\"--no-default-features\"]", "no_default = []")
        run([sys.executable, "quality/check_features.py", "--policy", str(policy)], cwd=root, expect_success=False)


def test_supply_chain_policy_failures() -> None:
    invalid_replacements = [
        ('unknown-git = "deny"', 'unknown-git = "allow"'),
        ('allow = ["Apache-2.0"]', "allow = []"),
        ('multiple-versions = "deny"', 'multiple-versions = "allow"'),
        ("version = 2", "version = 1"),
    ]
    for old, new in invalid_replacements:
        with copy_root() as temporary:
            root = Path(temporary) / "repo"
            policy = root / "deny.toml"
            replace_once(policy, old, new)
            run([sys.executable, "quality/check_deny_policy.py", "--policy", str(policy)], cwd=root, expect_success=False)


def test_unused_dependency() -> None:
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        (root / "src").mkdir()
        (root / "Cargo.toml").write_text(
            "[package]\nname = \"unused-dependency-fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\nserde = \"1\"\n",
            encoding="utf-8",
        )
        (root / "src/lib.rs").write_text("pub const fn fixture() -> u8 { 1 }\n", encoding="utf-8")
        run(["cargo", "machete", "--with-metadata", "--skip-target-dir"], cwd=root, expect_success=False)


def test_outdated_dependency() -> None:
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        (root / "src").mkdir()
        (root / "Cargo.toml").write_text(
            "[package]\nname = \"stale-dependency-fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\nserde = \"=1.0.0\"\n",
            encoding="utf-8",
        )
        (root / "src/lib.rs").write_text("pub const fn fixture() -> u8 { 1 }\n", encoding="utf-8")
        run(["cargo", "outdated", "--root-deps-only", "--exit-code", "1"], cwd=root, expect_success=False)


def test_secret_fixture() -> None:
    with copy_root() as temporary:
        root = Path(temporary) / "repo"
        secret = "api" + "_key = \"not-a-real-secret-12345\"\n"
        (root / "quality/fixtures/secret_fixture.py").parent.mkdir(parents=True, exist_ok=True)
        (root / "quality/fixtures/secret_fixture.py").write_text(secret, encoding="utf-8")
        # Fixtures are normally excluded; move it into checked source for this assertion.
        checked = root / "checked_secret.py"
        checked.write_text(secret, encoding="utf-8")
        run([sys.executable, "quality/check_docs.py"], cwd=root, expect_success=False)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--list", action="store_true")
    arguments = parser.parse_args()
    tests = [
        test_formatting_drift,
        test_lint_warning,
        test_unreasoned_suppression,
        test_tool_version_mismatch,
        test_missing_crate_metadata,
        test_forbidden_source_boundary,
        test_coverage_failures,
        test_missing_feature_profile,
        test_supply_chain_policy_failures,
        test_unused_dependency,
        test_outdated_dependency,
        test_secret_fixture,
    ]
    if arguments.list:
        for test in tests:
            print(test.__name__)
        return
    for test in tests:
        print(f"self-test: {test.__name__}", flush=True)
        test()
    print("quality-self-test: all intentional invalid fixtures failed as expected")


if __name__ == "__main__":
    main()
