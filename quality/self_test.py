#!/usr/bin/env python3
"""Prove that quality gates reject deliberately invalid isolated inputs."""

from __future__ import annotations

import argparse
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str], *, cwd: Path, expect_success: bool) -> None:
    completed = subprocess.run(command, cwd=cwd, capture_output=True, text=True)
    succeeded = completed.returncode == 0
    if succeeded != expect_success:
        output = (completed.stdout + "\n" + completed.stderr).strip()
        expectation = "succeed" if expect_success else "fail"
        raise RuntimeError(f"expected {' '.join(command)} to {expectation}, got {completed.returncode}: {output}")


@contextmanager
def copied_repository() -> Iterator[Path]:
    with tempfile.TemporaryDirectory() as temporary:
        destination = Path(temporary) / "repo"
        ignored = shutil.ignore_patterns(".git", "target", "__pycache__", "reports")
        shutil.copytree(ROOT, destination, ignore=ignored)
        yield destination


@contextmanager
def modified(path: Path) -> Iterator[None]:
    existed = path.exists()
    original = path.read_bytes() if existed else b""
    try:
        yield
    finally:
        if existed:
            path.write_bytes(original)
        elif path.exists():
            path.unlink()


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise RuntimeError(f"fixture replacement source not found in {path}: {old!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def test_formatting_drift(root: Path) -> None:
    harness = root / "quality/harness/src/lib.rs"
    with modified(harness):
        harness.write_text("pub const fn broken()->u8{1}\n", encoding="utf-8")
        run(["cargo", "fmt", "--all", "--check"], cwd=root, expect_success=False)


def test_lint_warning(root: Path) -> None:
    harness = root / "quality/harness/src/lib.rs"
    with modified(harness):
        harness.write_text("pub fn warning() { let unused = 1; }\n", encoding="utf-8")
        run(
            ["cargo", "clippy", "--workspace", "--all-targets", "--locked", "--", "-Dwarnings"],
            cwd=root,
            expect_success=False,
        )


def test_unreasoned_suppression(root: Path) -> None:
    harness = root / "quality/harness/src/lib.rs"
    with modified(harness):
        harness.write_text("#[allow(clippy::unwrap_used)]\npub fn invalid() {}\n", encoding="utf-8")
        run([sys.executable, "quality/check_architecture.py"], cwd=root, expect_success=False)


def test_tool_version_mismatch(root: Path) -> None:
    tools = root / "quality/tools.toml"
    with modified(tools):
        replace_once(tools, 'version = "0.9.140"', 'version = "0.0.0"')
        run(
            [sys.executable, "quality/check_tools.py", "--policy", str(tools)],
            cwd=root,
            expect_success=False,
        )


def test_missing_crate_metadata(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(policy, 'test_target = "dto and validation tests"', 'test_target = ""')
        run(
            [sys.executable, "quality/check_architecture.py", "--policy", str(policy)],
            cwd=root,
            expect_success=False,
        )


def test_m1_dependency_boundary(root: Path) -> None:
    manifest = root / "crates/intention-domain/Cargo.toml"
    with modified(manifest):
        manifest.write_text(
            manifest.read_text(encoding="utf-8")
            + '\nintention-protocol = { path = "../intention-protocol" }\n',
            encoding="utf-8",
        )
        run([sys.executable, "quality/check_architecture.py"], cwd=root, expect_success=False)


def test_m1_skeleton_api_boundary(root: Path) -> None:
    source = root / "crates/intention-application/src/lib.rs"
    with modified(source):
        source.write_text(
            source.read_text(encoding="utf-8") + "\npub const INVALID_SKELETON_API: u8 = 1;\n",
            encoding="utf-8",
        )
        run([sys.executable, "quality/check_architecture.py"], cwd=root, expect_success=False)


def test_m1_public_resource_leak(root: Path) -> None:
    source = root / "crates/intention-types/src/lib.rs"
    with modified(source):
        source.write_text(
            source.read_text(encoding="utf-8") + "\npub struct LeakedResource { file: std::fs::File }\n",
            encoding="utf-8",
        )
        run([sys.executable, "quality/check_architecture.py"], cwd=root, expect_success=False)


def test_m1_secret_projection(root: Path) -> None:
    source = root / "crates/intention-config/src/lib.rs"
    with modified(source):
        source.write_text(
            source.read_text(encoding="utf-8")
            + "\npub struct LeakedCredential { credential: String }\n",
            encoding="utf-8",
        )
        run([sys.executable, "quality/check_architecture.py"], cwd=root, expect_success=False)


def test_forbidden_source_boundary(root: Path) -> None:
    source = root / "quality/harness/src/forbidden.rs"
    with modified(source):
        source.write_text("use rusqlite::Connection;\n", encoding="utf-8")
        run([sys.executable, "quality/check_architecture.py"], cwd=root, expect_success=False)


def test_adapter_isolation_boundary(root: Path) -> None:
    source = root / "crates/intention-tauri/src/lib.rs"
    with modified(source):
        source.write_text("use intention_storage::Repository;\n", encoding="utf-8")
        run([sys.executable, "quality/check_architecture.py"], cwd=root, expect_success=False)


def test_protocol_isolation_boundary(root: Path) -> None:
    source = root / "crates/intention-protocol/src/lib.rs"
    with modified(source):
        source.write_text(
            source.read_text(encoding="utf-8") + "\npub struct LeakedSdk { client: async_openai::Client }\n",
            encoding="utf-8",
        )
        run([sys.executable, "quality/check_architecture.py"], cwd=root, expect_success=False)


def test_composition_ownership_boundary(root: Path) -> None:
    source = root / "crates/intention-daemon/src/lib.rs"
    with modified(source):
        source.write_text("use intention_storage_sqlite::SqliteRepository;\n", encoding="utf-8")
        run([sys.executable, "quality/check_architecture.py"], cwd=root, expect_success=False)


def test_provider_sdk_public_contract_boundary(root: Path) -> None:
    source = root / "crates/intention-types/src/lib.rs"
    with modified(source):
        source.write_text(
            source.read_text(encoding="utf-8")
            + "\npub struct LeakedSdk { client: async_openai::Client }\n",
            encoding="utf-8",
        )
        run([sys.executable, "quality/check_architecture.py"], cwd=root, expect_success=False)


def test_error_detail_and_correlation_validation(root: Path) -> None:
    fixture = root / "crates/intention-types/tests/fixtures/error-v1-missing-workspace-path.json"
    with modified(fixture):
        replace_once(fixture, '"src/missing.rs"', '"/etc/passwd"')
        run(
            ["cargo", "test", "-p", "intention-types", "--test", "error_contracts", "--locked"],
            cwd=root,
            expect_success=False,
        )


def test_coverage_failures(root: Path) -> None:
    policy = root / "quality/coverage.toml"
    report = root / "quality/fixtures/coverage-low.json"
    with modified(policy), modified(report):
        policy.write_text(
            policy.read_text(encoding="utf-8").replace(
                '"intention-types" = "A"', '"intention-types" = "B"', 1
            ),
            encoding="utf-8",
        )
        run([sys.executable, "quality/check_architecture.py"], cwd=root, expect_success=False)
        report.write_text(
            '{"data": [{"files": [{"filename": "'
            + str(root / "crates/intention-types/src/lib.rs")
            + '", "summary": {"lines": {"count": 100, "covered": 1}, "branches": {"percent": 0.0}}}], "totals": {"branches": {"percent": 0.0}}}]}\n',
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_coverage.py", "--policy", str(policy), "--report", str(report)],
            cwd=root,
            expect_success=False,
        )
        replace_once(policy, "enabled = false", "enabled = true")
        run(
            [sys.executable, "quality/check_coverage.py", "--policy", str(policy), "--report", str(report)],
            cwd=root,
            expect_success=False,
        )


def test_missing_feature_profile(root: Path) -> None:
    policy = root / "quality/features.toml"
    with modified(policy):
        replace_once(policy, 'no_default = ["--no-default-features"]', "no_default = []")
        run(
            [sys.executable, "quality/check_features.py", "--policy", str(policy)],
            cwd=root,
            expect_success=False,
        )


def test_supply_chain_policy_failures(root: Path) -> None:
    invalid_replacements = [
        ('unknown-git = "deny"', 'unknown-git = "allow"'),
        ('allow = ["Apache-2.0", "MIT", "Unicode-3.0"]', 'allow = ["Apache-2.0"]'),
        ('multiple-versions = "deny"', 'multiple-versions = "allow"'),
        ("version = 2", "version = 1"),
    ]
    policy = root / "deny.toml"
    for old, new in invalid_replacements:
        with modified(policy):
            replace_once(policy, old, new)
            run(
                [sys.executable, "quality/check_deny_policy.py", "--policy", str(policy)],
                cwd=root,
                expect_success=False,
            )


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


def test_secret_fixture(root: Path) -> None:
    checked = root / "checked_secret.py"
    with modified(checked):
        assignment = "api" + "_key" + ' = "not-a-real-secret-12345"\n'
        checked.write_text(assignment, encoding="utf-8")
        run([sys.executable, "quality/check_docs.py"], cwd=root, expect_success=False)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--list", action="store_true")
    arguments = parser.parse_args()
    repository_tests = [
        test_formatting_drift,
        test_lint_warning,
        test_unreasoned_suppression,
        test_tool_version_mismatch,
        test_missing_crate_metadata,
        test_m1_dependency_boundary,
        test_m1_skeleton_api_boundary,
        test_m1_public_resource_leak,
        test_m1_secret_projection,
        test_forbidden_source_boundary,
        test_adapter_isolation_boundary,
        test_protocol_isolation_boundary,
        test_composition_ownership_boundary,
        test_provider_sdk_public_contract_boundary,
        test_error_detail_and_correlation_validation,
        test_coverage_failures,
        test_missing_feature_profile,
        test_supply_chain_policy_failures,
        test_secret_fixture,
    ]
    standalone_tests = [test_unused_dependency, test_outdated_dependency]
    if arguments.list:
        for test in [*repository_tests, *standalone_tests]:
            print(test.__name__)
        return
    with copied_repository() as root:
        for test in repository_tests:
            print(f"self-test: {test.__name__}", flush=True)
            test(root)
    for test in standalone_tests:
        print(f"self-test: {test.__name__}", flush=True)
        test()
    print("quality-self-test: all intentional invalid fixtures failed as expected")


if __name__ == "__main__":
    main()
