#!/usr/bin/env python3
"""Prove that quality gates reject deliberately invalid isolated inputs."""

from __future__ import annotations

import argparse
from collections.abc import Iterator
from contextlib import contextmanager
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SHARED_TARGET_DIRECTORY = ROOT / "target"


def command_environment(cwd: Path) -> dict[str, str] | None:
    """Share Cargo artifacts only with an isolated copy of this repository."""
    if cwd != ROOT and (cwd / "quality" / "self_test.py").is_file():
        return {**os.environ, "CARGO_TARGET_DIR": str(SHARED_TARGET_DIRECTORY)}
    return None


def run(
    command: list[str],
    *,
    cwd: Path,
    expect_success: bool,
    expected_output: str | None = None,
    expected_outputs: tuple[str, ...] = (),
) -> None:
    completed = subprocess.run(
        command,
        cwd=cwd,
        capture_output=True,
        text=True,
        env=command_environment(cwd),
    )
    output = (completed.stdout + "\n" + completed.stderr).strip()
    succeeded = completed.returncode == 0
    if succeeded != expect_success:
        expectation = "succeed" if expect_success else "fail"
        raise RuntimeError(f"expected {' '.join(command)} to {expectation}, got {completed.returncode}: {output}")
    if expected_output is not None and expected_output not in output:
        raise RuntimeError(f"expected output to contain {expected_output!r}, got: {output}")
    for expected in expected_outputs:
        if expected not in output:
            raise RuntimeError(f"expected output to contain {expected!r}, got: {output}")


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


def test_copied_repository_commands_share_the_controller_target(_root: Path) -> None:
    with copied_repository() as copied_root:
        environment = command_environment(copied_root)
        if environment is None:
            raise RuntimeError("copied repository command environment is missing")
        if environment.get("CARGO_TARGET_DIR") != str(SHARED_TARGET_DIRECTORY):
            raise RuntimeError("copied repository commands must share the controller target directory")
    with tempfile.TemporaryDirectory() as temporary:
        if command_environment(Path(temporary)) is not None:
            raise RuntimeError("standalone temporary-project commands must not share the controller target directory")


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
    with modified(tools):
        replace_once(tools, 'version = "0.9.1"', 'version = "0.0.0"')
        run(
            [sys.executable, "quality/check_tools.py", "--policy", str(tools)],
            cwd=root,
            expect_success=False,
        )


def test_third_party_notices_drift(root: Path) -> None:
    notices = root / "THIRD_PARTY_NOTICES.md"
    with modified(notices):
        notices.write_text("stale notices\n", encoding="utf-8")
        run(
            [sys.executable, "quality/generate_third_party_notices.py", "--check"],
            cwd=root,
            expect_success=False,
            expected_output="THIRD_PARTY_NOTICES.md is stale",
        )
    with modified(notices):
        notices.unlink()
        run(
            [sys.executable, "quality/generate_third_party_notices.py", "--check"],
            cwd=root,
            expect_success=False,
            expected_output="generated notice file is missing",
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


def test_m2_dependency_boundary(root: Path) -> None:
    manifest = root / "crates/intention-client/Cargo.toml"
    with modified(manifest):
        manifest.write_text(
            manifest.read_text(encoding="utf-8")
            + '\nintention-config = { path = "../intention-config", version = "=0.0.0" }\n',
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="intention-client: workspace dependencies must equal",
        )


def test_workspace_dependency_cycle(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    manifest = root / "crates/intention-types/Cargo.toml"
    with modified(policy), modified(manifest):
        replace_once(policy, '"intention-types" = []', '"intention-types" = ["intention-protocol"]')
        manifest.write_text(
            manifest.read_text(encoding="utf-8").replace(
                "\n[dev-dependencies]\n",
                '\nintention-protocol = { path = "../intention-protocol", version = "=0.0.0" }\n\n[dev-dependencies]\n',
                1,
            ),
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="workspace dependency cycle:",
        )


def test_executable_test_target_policy(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(policy, 'test_targets = ["contracts", "error_contracts", "m3_contracts", "m4_model_values"]', 'test_targets = ["does-not-exist"]')
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="intention-types: declared integration test targets",
        )
    with modified(policy):
        replace_once(policy, 'test_targets = ["contracts", "error_contracts", "m3_contracts", "m4_model_values"]', 'test_targets = ["contracts", "contracts"]')
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="future crate intention-types has duplicate test targets",
        )
    with modified(policy):
        replace_once(policy, 'name = "intention-application"\nresponsibility = "Commands, queries, use cases, and transaction orchestration."\ntest_target = "use-case and architecture tests"\ntest_targets = ["m3_application", "m4_application_scheduling"]', 'name = "intention-application"\nresponsibility = "Commands, queries, use cases, and transaction orchestration."\ntest_target = "use-case and architecture tests"\ntest_targets = ["contracts"]')
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="intention-application: declared integration test targets must equal Cargo targets",
        )


def test_m3_active_test_target_policy(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(
            policy,
            'name = "intention-application"\nresponsibility = "Commands, queries, use cases, and transaction orchestration."\ntest_target = "use-case and architecture tests"\ntest_targets = ["m3_application", "m4_application_scheduling"]',
            'name = "intention-application"\nresponsibility = "Commands, queries, use cases, and transaction orchestration."\ntest_target = "use-case and architecture tests"\ntest_targets = ["contracts"]',
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="intention-application: declared integration test targets must equal Cargo targets",
        )


def test_m3_phase_partition_policy(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(policy, 'active_milestone = "m4"', 'active_milestone = "m2"')
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="policy phase and active_milestone must be matching supported milestones",
        )


def test_m4_phase_activation_policy(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(policy, 'active_milestone = "m4"', 'active_milestone = "m3"')
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="policy phase and active_milestone must be matching supported milestones",
        )
    with modified(policy):
        replace_once(
            policy,
            '  "intention-provider-generic-chat",\n]',
            ']',
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="M4 active production crates must equal the roadmap crate set",
        )


def test_m4_active_test_target_policy(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(
            policy,
            'name = "intention-model"\nresponsibility = "Provider-neutral model DTOs and driver contract."\ntest_target = "model contract and stream tests"\ntest_targets = ["model_contracts", "m4_execution_contracts", "m4_reexports"]',
            'name = "intention-model"\nresponsibility = "Provider-neutral model DTOs and driver contract."\ntest_target = "model contract and stream tests"\ntest_targets = []',
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="intention-model: declared integration test targets",
        )


def test_m4_sdk_ownership_and_public_contract_boundaries(root: Path) -> None:
    forbidden_owner = root / "crates/intention-model/src/lib.rs"
    with modified(forbidden_owner):
        forbidden_owner.write_text(
            forbidden_owner.read_text(encoding="utf-8")
            + "\nuse async_openai::Client as LeakedSdk;\n",
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="allowed only in intention-provider-generic-chat private implementation",
        )
    public_provider = root / "crates/intention-provider-generic-chat/src/lib.rs"
    with modified(public_provider):
        public_provider.write_text(
            public_provider.read_text(encoding="utf-8")
            + "\npub type LeakedGenericSdk = async_openai::Client<async_openai::config::OpenAIConfig>;\n",
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_public_api.py"],
            cwd=root,
            expect_success=False,
            expected_output="LeakedGenericSdk",
        )
    non_composition = root / "crates/intention-daemon/src/lib.rs"
    with modified(non_composition):
        non_composition.write_text(
            non_composition.read_text(encoding="utf-8")
            + "\nuse intention_provider_openrouter::OpenRouterDriver;\n",
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="composition ownership outside intention forbids",
        )


def test_m3_daemon_test_dependency_policy(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(
            policy,
            '"intention-daemon" = ["intention", "intention-application", "intention-client", "intention-config", "intention-domain", "intention-model", "intention-protocol", "intention-runtime", "intention-transport", "intention-types"]',
            '"intention-daemon" = ["intention", "intention-application", "intention-client", "intention-domain", "intention-model", "intention-protocol", "intention-runtime", "intention-transport", "intention-types"]',
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="intention-daemon: workspace dependencies must equal",
        )
    with modified(policy):
        replace_once(
            policy,
            '"intention-daemon" = ["futures-util", "serde_json", "tempfile", "tokio"]',
            '"intention-daemon" = []',
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="intention-daemon: external dependencies must equal",
        )


def test_m3_non_production_test_target_policy(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(
            policy,
            'test_targets = ["composition_contract", "fixture_host"]',
            'test_targets = ["composition_contract"]',
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="intention-test-support: declared integration test targets",
        )


def test_m3_non_production_dependency_policy(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(
            policy,
            '  "intention-types",\n]',
            '  "intention-types",\n  "intention-client",\n]',
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="test-support workspace dependencies must equal",
        )


def test_m3_non_production_external_dependency_policy(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(
            policy,
            '"intention-test-support" = ["serde_json", "tempfile"]',
            '"intention-test-support" = ["serde_json"]',
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="test-support external dependencies must equal",
        )


def test_signature_aware_public_api_leaks(root: Path) -> None:
    source = root / "crates/intention-types/src/lib.rs"
    with modified(source):
        source.write_text(
            source.read_text(encoding="utf-8")
            + "\nuse std::fs;\n"
            + "pub type LeakedAlias = fs::File;\n"
            + "pub struct LeakedWrapper(pub fs::File);\n"
            + "pub struct GenericWrapper<T>(pub T);\n"
            + "pub type LeakedGeneric = GenericWrapper<fs::File>;\n"
            + "pub fn leaked_signature(value: fs::File) -> fs::File { value }\n"
            + "pub use std::fs::File as LeakedReexport;\n",
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_public_api.py"],
            cwd=root,
            expect_success=False,
            expected_outputs=("LeakedAlias", "LeakedWrapper", "LeakedGeneric", "leaked_signature", "LeakedReexport"),
        )


def test_m2_public_resource_leak(root: Path) -> None:
    source = root / "crates/intention-client/src/lib.rs"
    with modified(source):
        source.write_text(
            source.read_text(encoding="utf-8") + "\npub type LeakedClientResource = std::fs::File;\n",
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_public_api.py"],
            cwd=root,
            expect_success=False,
            expected_output="LeakedClientResource",
        )


def test_m2_secret_projection(root: Path) -> None:
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


def test_adapter_production_dependency_boundary(root: Path) -> None:
    manifest = root / "crates/intention-tui/Cargo.toml"
    with modified(manifest):
        manifest.write_text(
            manifest.read_text(encoding="utf-8").replace(
                "[dependencies]\n",
                "[dependencies]\nintention-daemon = { path = \"../intention-daemon\", version = \"=0.0.0\" }\n",
                1,
            ),
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="intention-tui: adapter production workspace dependencies",
        )


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


def coverage_report(root: Path, files: list[tuple[str, int, int]]) -> str:
    return json.dumps({
        "data": [{
            "files": [
                {
                    "filename": str(root / path),
                    "summary": {
                        "lines": {"count": count, "covered": covered},
                        "branches": {"percent": 100.0},
                    },
                }
                for path, count, covered in files
            ],
        }],
    })


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
            coverage_report(root, [("crates/intention-types/src/lib.rs", 100, 1)]),
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_coverage.py", "--policy", str(policy), "--report", str(report)],
            cwd=root,
            expect_success=False,
        )


def test_coverage_exclusion_semantics(root: Path) -> None:
    policy = root / "quality/coverage.toml"
    report = root / "quality/fixtures/m1plus-coverage.json"
    target = root / "crates/intention-types/src/excluded_fixture.rs"
    unreported = root / "crates/intention-types/src/unreported_fixture.rs"
    outside_source = root / "crates/intention-types/outside_fixture.rs"
    with modified(target), modified(unreported), modified(outside_source), modified(policy), modified(report):
        target.write_text("pub const EXCLUDED_FIXTURE: u8 = 1;\n", encoding="utf-8")
        unreported.write_text("pub const UNREPORTED_FIXTURE: u8 = 1;\n", encoding="utf-8")
        outside_source.write_text("pub const OUTSIDE_FIXTURE: u8 = 1;\n", encoding="utf-8")
        report.write_text(
            coverage_report(
                root,
                [
                    ("crates/intention-types/src/lib.rs", 100, 100),
                    ("crates/intention-types/src/excluded_fixture.rs", 100, 0),
                    ("crates/intention-domain/src/lib.rs", 100, 100),
                    ("crates/intention-protocol/src/lib.rs", 100, 100),
                    ("crates/intention-config/src/lib.rs", 100, 100),
                    ("crates/intention-transport/src/lib.rs", 100, 100),
                    ("crates/intention-client/src/lib.rs", 100, 100),
                    ("crates/intention/src/lib.rs", 100, 100),
                    ("crates/intention-daemon/src/lib.rs", 100, 100),
                    ("crates/intention-daemon/src/main.rs", 100, 0),
                    ("crates/intention-application/src/lib.rs", 100, 100),
                    ("crates/intention-runtime/src/lib.rs", 100, 100),
                    ("crates/intention-storage/src/lib.rs", 100, 100),
                    ("crates/intention-storage-sqlite/src/lib.rs", 100, 100),
                    ("crates/intention-model/src/lib.rs", 100, 100),
                    ("crates/intention-provider-openrouter/src/lib.rs", 100, 100),
                    ("crates/intention-provider-generic-chat/src/lib.rs", 100, 100),
                ],
            ),
            encoding="utf-8",
        )
        valid = """
[[exclusions]]
path = "crates/intention-types/src/excluded_fixture.rs"
rationale = "Synthetic denominator fixture."
owner = "intention-types"
equivalent_test_evidence = "quality/self_test.py:test_coverage_exclusion_semantics"
enabled = true
"""
        policy.write_text(policy.read_text(encoding="utf-8") + valid, encoding="utf-8")
        run(
            [sys.executable, "quality/check_coverage.py", "--policy", str(policy), "--report", str(report)],
            cwd=root,
            expect_success=True,
            expected_output="excluding crates/intention-types/src/excluded_fixture.rs from intention-types denominator",
        )
        invalid_cases = [
            ("missing-metadata", valid.replace('rationale = "Synthetic denominator fixture."', 'rationale = ""'), "enabled exclusion requires rationale"),
            (
                "absolute-path",
                valid.replace(
                    'path = "crates/intention-types/src/excluded_fixture.rs"',
                    f"path = '''{target}'''",
                ),
                "workspace-relative without traversal",
            ),
            ("traversal", valid.replace('path = "crates/intention-types/src/excluded_fixture.rs"', 'path = "crates/intention-types/src/../src/excluded_fixture.rs"'), "workspace-relative without traversal"),
            ("other-owner", valid.replace('owner = "intention-types"', 'owner = "intention-domain"'), "must be under intention-domain source root"),
            ("unknown-owner", valid.replace('owner = "intention-types"', 'owner = "missing-owner"'), "owner must be an active production crate"),
            ("unreported", valid.replace('excluded_fixture.rs', 'unreported_fixture.rs'), "must appear exactly once in coverage report"),
            ("outside-source", valid.replace('crates/intention-types/src/excluded_fixture.rs', 'crates/intention-types/outside_fixture.rs'), "must be under intention-types source root"),
            ("duplicate", valid + valid, "duplicate enabled exclusion path"),
        ]
        for _name, invalid, expected_output in invalid_cases:
            policy.write_text(policy.read_text(encoding="utf-8").split("\n[[exclusions]]", 1)[0] + invalid, encoding="utf-8")
            run(
                [sys.executable, "quality/check_coverage.py", "--policy", str(policy), "--report", str(report)],
                cwd=root,
                expect_success=False,
                expected_output=expected_output,
            )
        policy.write_text(policy.read_text(encoding="utf-8").split("\n[[exclusions]]", 1)[0] + valid, encoding="utf-8")
        report.write_text(
            coverage_report(
                root,
                [
                    ("crates/intention-types/src/excluded_fixture.rs", 100, 0),
                    ("crates/intention-domain/src/lib.rs", 100, 100),
                    ("crates/intention-protocol/src/lib.rs", 100, 100),
                    ("crates/intention-config/src/lib.rs", 100, 100),
                    ("crates/intention-transport/src/lib.rs", 100, 100),
                    ("crates/intention-client/src/lib.rs", 100, 100),
                    ("crates/intention/src/lib.rs", 100, 100),
                    ("crates/intention-daemon/src/lib.rs", 100, 100),
                    ("crates/intention-daemon/src/main.rs", 100, 0),
                    ("crates/intention-application/src/lib.rs", 100, 100),
                    ("crates/intention-runtime/src/lib.rs", 100, 100),
                    ("crates/intention-storage/src/lib.rs", 100, 100),
                    ("crates/intention-storage-sqlite/src/lib.rs", 100, 100),
                    ("crates/intention-model/src/lib.rs", 100, 100),
                    ("crates/intention-provider-openrouter/src/lib.rs", 100, 100),
                    ("crates/intention-provider-generic-chat/src/lib.rs", 100, 100),
                ],
            ),
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_coverage.py", "--policy", str(policy), "--report", str(report)],
            cwd=root,
            expect_success=False,
            expected_output="intention-types' has no reportable non-excluded source lines",
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


def test_isolated_release_profile(root: Path) -> None:
    source = root / "crates/intention-daemon/src/lib.rs"
    with modified(source):
        source.write_text(
            source.read_text(encoding="utf-8") + "\nuse intention_types::SessionId;\n",
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/run_profiles.py", "isolated-release"],
            cwd=root,
            expect_success=False,
            expected_output="could not compile `intention-daemon`",
        )


def test_missing_isolated_release_profile(root: Path) -> None:
    policy = root / "quality/features.toml"
    with modified(policy):
        replace_once(
            policy,
            'profiles = ["default", "no_default"]',
            'profiles = ["missing"]',
        )
        run(
            [sys.executable, "quality/check_features.py", "--policy", str(policy)],
            cwd=root,
            expect_success=False,
            expected_output="requires known profile names",
        )


def test_invalid_isolated_release_package_and_target(root: Path) -> None:
    policy = root / "quality/features.toml"
    with modified(policy):
        replace_once(policy, '"intention-daemon"', '"missing-production-package"')
        run(
            [sys.executable, "quality/check_features.py", "--policy", str(policy)],
            cwd=root,
            expect_success=False,
            expected_output="is not a workspace package",
        )
    with modified(policy):
        replace_once(policy, 'targets = ["lib", "bins"]', 'targets = ["tests"]')
        run(
            [sys.executable, "quality/check_features.py", "--policy", str(policy)],
            cwd=root,
            expect_success=False,
            expected_output="requires known release targets",
        )


def test_supply_chain_policy_failures(root: Path) -> None:
    invalid_replacements = [
        ('unknown-git = "deny"', 'unknown-git = "allow"'),
        ('allow = ["Apache-2.0", "MIT", "Unicode-3.0", "0BSD", "Zlib", "BSD-3-Clause", "ISC", "CDLA-Permissive-2.0"]', 'allow = ["Apache-2.0"]'),
        ('multiple-versions = "deny"', 'multiple-versions = "allow"'),
        ('"RUSTSEC-2024-0384",', '"RUSTSEC-2024-0000",'),
        ('"RUSTSEC-2025-0012",', ''),
        ("version = 2", "version = 1"),
    ]
    policy = root / "deny.toml"
    outdated_policy = root / "quality/outdated.toml"
    with modified(policy), modified(outdated_policy):
        outdated_policy.write_text(
            outdated_policy.read_text(encoding="utf-8").replace(
                'crates = ["async-openai"]', 'crates = []', 1
            ),
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_deny_policy.py", "--policy", str(policy), "--outdated-policy", str(outdated_policy)],
            cwd=root,
            expect_success=False,
        )
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
        test_copied_repository_commands_share_the_controller_target,
        test_formatting_drift,
        test_lint_warning,
        test_unreasoned_suppression,
        test_tool_version_mismatch,
        test_third_party_notices_drift,
        test_missing_crate_metadata,
        test_m2_dependency_boundary,
        test_workspace_dependency_cycle,
        test_executable_test_target_policy,
        test_m3_active_test_target_policy,
        test_m3_phase_partition_policy,
        test_m4_phase_activation_policy,
        test_m4_active_test_target_policy,
        test_m4_sdk_ownership_and_public_contract_boundaries,
        test_m3_daemon_test_dependency_policy,
        test_m3_non_production_test_target_policy,
        test_m3_non_production_dependency_policy,
        test_m3_non_production_external_dependency_policy,
        test_signature_aware_public_api_leaks,
        test_m2_public_resource_leak,
        test_m2_secret_projection,
        test_forbidden_source_boundary,
        test_adapter_isolation_boundary,
        test_adapter_production_dependency_boundary,
        test_protocol_isolation_boundary,
        test_composition_ownership_boundary,
        test_provider_sdk_public_contract_boundary,
        test_error_detail_and_correlation_validation,
        test_coverage_failures,
        test_coverage_exclusion_semantics,
        test_missing_feature_profile,
        test_isolated_release_profile,
        test_missing_isolated_release_profile,
        test_invalid_isolated_release_package_and_target,
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
