#!/usr/bin/env python3
"""Prove that quality gates reject deliberately invalid isolated inputs."""

from __future__ import annotations

import argparse
from collections.abc import Iterator
from contextlib import contextmanager
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SHARED_TARGET_DIRECTORY = ROOT / "target"
FIXTURE_TRACKED_SUFFIXES = {".json", ".lock", ".py", ".toml"}


class FixtureScopeError(RuntimeError):
    """Raised when a fixture would mutate untracked or binary repository files."""


def load_quality_module(name: str) -> object:
    """Import a quality script from the repository as a plain module."""
    spec = importlib.util.spec_from_file_location(
        name, ROOT / "quality" / f"{name}.py"
    )
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load quality/{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_sccache_collector_parses_pinned_v017_schema() -> None:
    """The sccache collector parses the pinned sccache v0.17.0 stats schema."""
    collector = load_quality_module("collect_sccache_metrics")
    cache_location = f"Local disk: {tempfile.gettempdir()}"
    # Mirror of sccache v0.17.0 `--show-stats --stats-format json` output
    # (src/server.rs ServerInfo/ServerStats serialization): scalar counters,
    # per-language counter objects, and an integer cache_size.
    json_output = json.dumps(
        {
            "stats": {
                "compile_requests": 9,
                "requests_unsupported_compiler": 0,
                "requests_not_compile": 1,
                "requests_not_cacheable": 2,
                "requests_executed": 9,
                "cache_errors": {"counts": {"c/c++": 1}, "adv_counts": {}},
                "cache_hits": {"counts": {"c/c++": 5, "Rust": 2}, "adv_counts": {}},
                "cache_misses": {"counts": {"Rust": 3}, "adv_counts": {}},
                "cache_timeouts": 0,
                "cache_read_errors": 0,
                "non_cacheable_compilations": 2,
                "forced_recaches": 0,
                "cache_write_errors": 0,
                "cache_writes": 7,
                "cache_write_duration": {"secs": 0, "nanos": 100},
                "cache_read_hit_duration": {"secs": 0, "nanos": 200},
                "compilations": 5,
                "compiler_write_duration": {"secs": 0, "nanos": 300},
                "compile_fails": 0,
                "not_cached": {},
                "dist_compiles": {},
                "dist_errors": 0,
                "multi_level": None,
            },
            "cache_location": cache_location,
            "cache_size": 1048576,
            "max_cache_size": 5368709120,
            "use_preprocessor_cache_mode": False,
            "version": "0.17.0",
            "basedirs": [],
        }
    )
    stats, size, location = collector._json_stats(json_output)
    if stats.get("cache_hits") != 7 or stats.get("cache_misses") != 3:
        raise RuntimeError(f"per-language counters not aggregated: {stats!r}")
    if stats.get("cache_errors") != 1:
        raise RuntimeError(f"per-language cache_errors not aggregated: {stats!r}")
    if stats.get("compile_requests") != 9 or stats.get("compile_fails") != 0:
        raise RuntimeError(f"scalar counters not parsed: {stats!r}")
    if size != 1048576:
        raise RuntimeError(f"cache_size not parsed as integer: {size!r}")
    if location != cache_location:
        raise RuntimeError(f"cache_location not parsed: {location!r}")
    if collector._hit_rate(stats) != 0.7:
        raise RuntimeError(f"unexpected hit rate: {collector._hit_rate(stats)!r}")

    # Mirror of v0.17.0 non-advanced human `--show-stats` output.
    text_output = (
        "Compile requests                     10\n"
        "Compile requests executed             9\n"
        "Cache hits                            7\n"
        "Cache hits (C/C++)                    5\n"
        "Cache hits (Rust)                     2\n"
        "Cache misses                          3\n"
        "Cache misses (Rust)                   3\n"
        "Cache hits rate                   70.00 %\n"
        "Cache hits rate (C/C++)             100.00 %\n"
        "Cache hits rate (Rust)              40.00 %\n"
        "Cache timeouts                        0\n"
        "Cache read errors                     0\n"
        "Forced recaches                       0\n"
        "Cache write errors                    0\n"
        "Cache errors                          1\n"
        "Cache errors (C/C++)                  1\n"
        "Compilations                          5\n"
        "Compilation failures                  0\n"
        "Non-cacheable compilations            2\n"
        "Non-cacheable calls                   2\n"
        "Non-compilation calls                 1\n"
        "Unsupported compiler calls            0\n"
        "Average cache write                0.000 s\n"
        "Average compiler                   0.000 s\n"
        "Average cache read hit              0.000 s\n"
        "Failed distributed compilations       0\n"
        "Cache location                        " + cache_location + "\n"
        "Base directories                      (none)\n"
        "Use direct/preprocessor mode?         yes\n"
        "Version (client)                      0.17.0\n"
        "Cache size                            1.0 MiB\n"
        "Max cache size                        5.0 GiB\n"
    )
    stats, size, location = collector._text_stats(text_output)
    if stats.get("cache_hits") != 7 or stats.get("cache_misses") != 3:
        raise RuntimeError(f"text per-language aggregate not parsed: {stats!r}")
    if stats.get("cache_errors") != 1:
        raise RuntimeError(f"text cache_errors not parsed: {stats!r}")
    if stats.get("requests_executed") != 9 or stats.get("compile_fails") != 0:
        raise RuntimeError(f"text name overrides not applied: {stats!r}")
    if stats.get("requests_not_cacheable") != 2 or stats.get("requests_not_compile") != 1:
        raise RuntimeError(f"text name overrides not applied: {stats!r}")
    if size != 1048576:
        raise RuntimeError(f"text cache size not parsed: {size!r}")


def test_sccache_collector_rejects_malformed_counters() -> None:
    """Bools and negative counters are rejected without crashing diagnostics."""
    collector = load_quality_module("collect_sccache_metrics")
    malformed = json.dumps(
        {
            "stats": {
                "compile_requests": -1,
                "cache_timeouts": True,
                "cache_hits": {"counts": {"c/c++": True, "Rust": 2}, "adv_counts": {}},
                "cache_misses": {"counts": {"Rust": -3}, "adv_counts": {}},
                "cache_errors": {"counts": {"c/c++": 1}, "adv_counts": {}},
            },
            "cache_location": None,
            "cache_size": -1048576,
        }
    )
    stats, size, location = collector._json_stats(malformed)
    if "compile_requests" in stats or "cache_timeouts" in stats:
        raise RuntimeError(f"bool and negative scalar counters must be rejected: {stats!r}")
    if stats.get("cache_hits") != 2:
        raise RuntimeError(f"malformed per-language entries must be ignored, valid ones summed: {stats!r}")
    if "cache_misses" in stats or stats.get("cache_errors") != 1:
        raise RuntimeError(f"per-language malformed handling is wrong: {stats!r}")
    if size is not None or location is not None:
        raise RuntimeError("malformed cache_size and cache_location must become unavailable")
    text_stats, text_size, _text_location = collector._text_stats(
        "Compile requests                     10\n"
        "Cache hits                           -1\n"
        "Cache errors                          1\n"
        "Cache size                            1.0 MiB\n"
    )
    if "cache_hits" in text_stats or text_stats.get("cache_errors") != 1:
        raise RuntimeError(f"negative text counters must be rejected: {text_stats!r}")
    if text_size != 1048576:
        raise RuntimeError(f"valid text cache size must still parse: {text_size!r}")


def test_benchmark_jobs_parsing() -> None:
    """The --jobs benchmark parses values and aggregates durations."""
    benchmark = load_quality_module("benchmark_cargo_jobs")
    if benchmark.parse_jobs("2,4,8") != ["2", "4", "8"]:
        raise RuntimeError("benchmark --jobs parsing failed")
    if benchmark.parse_jobs("") != [] or benchmark.parse_jobs(" , ") != []:
        raise RuntimeError("empty --jobs must yield the baseline-only configuration")
    if benchmark.parse_jobs("2, ,4") != ["2", "4"]:
        raise RuntimeError("blank entries within --jobs must be skipped")
    for invalid in ("x", "0", "-1", "2.5"):
        try:
            benchmark.parse_jobs(invalid)
        except ValueError:
            continue
        raise RuntimeError(f"invalid --jobs value {invalid!r} must be rejected")
    if benchmark.aggregate([1.0, 2.0, 3.0]) != {
        "p50": 2.0,
        "p95": 3.0,
        "mean": 2.0,
        "min": 1.0,
        "max": 3.0,
    }:
        raise RuntimeError("benchmark aggregation failed")
    if benchmark.build_command("cargo", benchmark.BASELINE) != ["cargo", "build", "--workspace", "--locked"]:
        raise RuntimeError("baseline command must omit --jobs")
    if benchmark.build_command("cargo", "4") != ["cargo", "build", "--workspace", "--locked", "--jobs=4"]:
        raise RuntimeError("configured command must include --jobs=N")


def test_cleanup_gha_caches_decides_deletion() -> None:
    """The cache cleaner deletes non-kept refs and stale kept-ref caches only."""
    import datetime as _datetime

    cleanup = load_quality_module("cleanup_gha_caches")
    now = _datetime.datetime(2026, 8, 31, 12, 0, tzinfo=_datetime.timezone.utc)
    recent = "2026-08-31T10:00:00Z"
    old = "2026-08-01T10:00:00Z"
    # Non-kept refs (pull requests and feature branches) are always deleted.
    if not cleanup.should_delete("refs/pull/21/merge", recent, "refs/heads/main", 7, now):
        raise RuntimeError("pull-request caches must always be deleted")
    if not cleanup.should_delete("refs/heads/feature-x", recent, "refs/heads/main", 7, now):
        raise RuntimeError("feature-branch caches must always be deleted")
    # Kept-ref caches are kept while fresh and deleted once stale.
    if cleanup.should_delete("refs/heads/main", recent, "refs/heads/main", 7, now):
        raise RuntimeError("fresh kept-ref cache must be kept")
    if not cleanup.should_delete("refs/heads/main", old, "refs/heads/main", 7, now):
        raise RuntimeError("stale kept-ref cache must be deleted")
    # Age is measured against the explicit clock; boundary at exactly the
    # limit is kept (strictly older than the limit is deleted).
    boundary = (now - _datetime.timedelta(days=7)).isoformat().replace("+00:00", "Z")
    if cleanup.should_delete("refs/heads/main", boundary, "refs/heads/main", 7, now):
        raise RuntimeError("cache exactly at the age limit must be kept")


def test_profile_arguments_include_critical_combinations(_root: Path) -> None:
    namespace = {"__file__": str(ROOT / "quality/run_profiles.py"), "__name__": "quality.run_profiles"}
    exec((ROOT / "quality/run_profiles.py").read_text(encoding="utf-8"), namespace)
    profiles = namespace["profile_arguments"]({
        "profiles": {"default": []},
        "critical_combinations": [{"enabled": True, "name": "extra", "features": ["a", "b"]}],
    })
    if profiles != [("default", []), ("critical:extra", ["--features", "a,b"])]:
        raise RuntimeError(f"unexpected profile selection: {profiles!r}")


def command_environment(cwd: Path) -> dict[str, str] | None:
    """Share Cargo artifacts only with an isolated copy of this repository."""
    if cwd != ROOT and (cwd / "quality" / "self_test.py").is_file():
        return {**os.environ, "CARGO_TARGET_DIR": str(SHARED_TARGET_DIRECTORY)}
    return None


def architecture_check_command(policy: Path | None = None) -> list[str]:
    command = [sys.executable, "quality/check_architecture.py"]
    if policy is not None:
        command.extend(["--policy", str(policy)])
    return command


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
def fixture_scope(working_root: Path) -> Iterator[None]:
    """Scope one repository fixture to tracked source files.

    In CI (in-place) mode the fixtures run against the working tree itself so
    that Cargo artifacts for the same source paths are reused across fixtures.
    The working tree must be clean before the fixture runs and every mutation
    is restored afterwards with `git restore`. In the isolated-copy mode the
    copy has no `.git` directory: the temporary directory and the `modified`
    context manager already provide isolation, so the scope is a no-op there.
    """
    git_dir = subprocess.run(
        ["git", "rev-parse", "--git-dir"],
        cwd=working_root,
        capture_output=True,
        text=True,
        check=False,
    )
    if git_dir.returncode != 0:
        yield
        return
    status = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=working_root,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    if status:
        raise FixtureScopeError(
            f"fixture scope requires a clean working tree, found: {status!r}"
        )
    try:
        yield
    finally:
        restored = subprocess.run(
            ["git", "restore", "--worktree", "--source=HEAD", "--", "."],
            cwd=working_root,
            capture_output=True,
            text=True,
            check=False,
        )
        if restored.returncode != 0:
            raise FixtureScopeError(
                f"fixture restore failed: {restored.stderr.strip() or restored.stdout.strip()}"
            )


def tracked_modified_paths(working_root: Path) -> set[str]:
    """Return files changed by a fixture for diagnostics, tracked files only."""
    status = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=working_root,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    return {
        line[3:]
        for line in status.splitlines()
        if line.startswith((" M ", "M "))
    }


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
    with fixture_scope(root), modified(harness):
        harness.write_text("pub const fn broken()->u8{1}\n", encoding="utf-8")
        run(["cargo", "fmt", "--all", "--check"], cwd=root, expect_success=False)


def test_lint_warning(root: Path) -> None:
    harness = root / "quality/harness/src/lib.rs"
    # quality-harness is a dependency-free workspace member, so scoping the
    # fixture to that crate proves the lint gate rejects a warning without
    # recompiling the whole workspace a second time.
    with fixture_scope(root), modified(harness):
        harness.write_text("pub fn warning() { let unused = 1; }\n", encoding="utf-8")
        run(
            ["cargo", "clippy", "-p", "quality-harness", "--all-targets", "--locked", "--", "-Dwarnings"],
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
            architecture_check_command(policy),
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
    lockfile = root / "Cargo.lock"
    with modified(policy), modified(manifest), modified(lockfile):
        replace_once(policy, '"intention-types" = []', '"intention-types" = ["intention-protocol"]')
        manifest.write_text(
            manifest.read_text(encoding="utf-8").replace(
                "\n[dev-dependencies]\n",
                '\nintention-protocol = { path = "../intention-protocol", version = "=0.0.0" }\n\n[dev-dependencies]\n',
                1,
            ),
            encoding="utf-8",
        )
        # The fixture is intentionally cyclic, so Cargo's locked metadata
        # query must not be allowed to fail before the checker reports it.
        lockfile.unlink()
        run(
            [sys.executable, "quality/check_architecture.py", "--policy", str(policy)],
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
        replace_once(policy, 'active_milestone = "m5"', 'active_milestone = "m2"')
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="policy phase and active_milestone must be matching supported milestones",
        )


def test_m4_phase_activation_policy(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(policy, 'active_milestone = "m5"', 'active_milestone = "m3"')
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="policy phase and active_milestone must be matching supported milestones",
        )
    with modified(policy):
        replace_once(
            policy,
            '  "intention-provider-generic-chat",\n  "intention-tools",\n  "intention-workspace",\n  "intention-hooks",\n]',
            ']',
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="M5 active production crates must equal the roadmap crate set",
        )


def test_m4_active_test_target_policy(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(
            policy,
            'name = "intention-model"\nresponsibility = "Provider-neutral model DTOs and driver contract."\ntest_target = "model contract and stream tests"\ntest_targets = ["model_contracts", "m4_execution_contracts", "m4_reexports"]',
            'name = "intention-model"\nresponsibility = "Provider-neutral model DTOs and driver contract."\ntest_target = "model contract and stream tests"\ntest_targets = []',
        )


def test_m5_activation_policy(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(policy, '"intention-tools",\n', '')
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="M5 active production crates must equal the roadmap crate set",
        )
    with modified(policy):
        replace_once(
            policy,
            'test_targets = ["tool_contracts", "tool_coverage_contracts", "tool_coverage_extra", "tool_coverage_final", "tool_coverage_invocation", "tool_coverage_last", "tool_coverage_remaining", "tool_coverage_search"]',
            'test_targets = []',
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="intention-tools: declared integration test targets",
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="intention-tools: declared integration test targets",
        )


def test_m5_skeleton_drift_policy(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(policy, '"intention-vfr",', '"intention-tools",')
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="an M5 crate cannot be both active and a skeleton",
        )
    with modified(policy):
        replace_once(policy, '"intention-vfr",', '"missing-skeleton",')
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="M5 active, skeleton, adapter, and test-support crates must cover",
        )


def test_m5_exact_active_crate_policy(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(policy, '  "intention-hooks",\n]', '  "intention-hooks",\n  "intention-test-support",\n]')
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="M5 active production crates must equal the roadmap crate set",
        )


def test_configured_quality_harness_partition_policy(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(policy, 'quality_harness = "quality-harness"', 'quality_harness = "intention-test-support"')
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="M5 active, skeleton, adapter, and test-support crates must cover",
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
            '"intention-daemon" = ["intention", "intention-application", "intention-client", "intention-config", "intention-domain", "intention-model", "intention-protocol", "intention-runtime", "intention-tools", "intention-transport", "intention-types"]',
            '"intention-daemon" = ["intention", "intention-application", "intention-client", "intention-domain", "intention-model", "intention-protocol", "intention-runtime", "intention-tools", "intention-transport", "intention-types"]',
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


def test_test_only_source_patterns_are_cfg_scoped(root: Path) -> None:
    source = root / "crates/intention-workspace/src/lib.rs"
    with modified(source):
        original = source.read_text(encoding="utf-8")
        source.write_text(
            original + "\n#[cfg(test)]\nmod regression {\n    fn fixture() { let _ = std::env::current_dir(); }\n}\n",
            encoding="utf-8",
        )
        run([sys.executable, "quality/check_architecture.py"], cwd=root, expect_success=True)
    with modified(source):
        source.write_text(
            source.read_text(encoding="utf-8") + "\npub fn production() { let _ = std::env::current_dir(); }\n",
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="std::env::current_dir",
        )


def test_adapter_isolation_boundary(root: Path) -> None:
    source = root / "crates/intention-tauri/src/lib.rs"
    with modified(source):
        source.write_text("use intention_storage::Repository;\n", encoding="utf-8")
        run([sys.executable, "quality/check_architecture.py"], cwd=root, expect_success=False)


def test_adapter_dto_boundary(root: Path) -> None:
    source = root / "crates/intention-tui/src/lib.rs"
    with modified(source):
        source.write_text("pub struct Leaked { value: serde_json::Value }\n", encoding="utf-8")
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="serde_json::Value",
        )


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


def test_adapter_test_dependency_boundary(root: Path) -> None:
    manifest = root / "crates/intention-tui/Cargo.toml"
    with modified(manifest):
        original = manifest.read_text(encoding="utf-8")
        marker = "[dev-dependencies]\n"
        if marker not in original:
            raise RuntimeError("TUI fixture requires dev-dependencies table")
        manifest.write_text(
            original.replace(marker, marker + "intention-storage = { path = \"../intention-storage\", version = \"=0.0.0\" }\n", 1),
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="intention-tui: adapter test workspace dependencies",
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
        source.write_text("use intention_storage_sqlite::SqliteStorageRepository;\n", encoding="utf-8")
        run([sys.executable, "quality/check_architecture.py"], cwd=root, expect_success=False)


def test_composition_selection_ownership_policy(root: Path) -> None:
    policy = root / "quality/architecture.toml"
    with modified(policy):
        replace_once(policy, '  "SqliteStorageRepository",', '  "MissingSelection",')
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="composition root must own concrete selection",
        )


def test_tools_workspace_dependency_boundary(root: Path) -> None:
    manifest = root / "crates/intention-tools/Cargo.toml"
    with modified(manifest), modified(root / "Cargo.lock"):
        replace_once(
            manifest,
            'intention-workspace = { path = "../intention-workspace", version = "=0.0.0" }',
            'intention-domain = { path = "../intention-domain", version = "=0.0.0" }',
        )
        run(
            [sys.executable, "quality/check_architecture.py"],
            cwd=root,
            expect_success=False,
            expected_output="intention-tools: workspace dependencies must equal",
        )


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
    with fixture_scope(root), modified(fixture):
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


def metadata_snapshot(root: Path, crates: list[str]) -> str:
    return json.dumps({
        "packages": [
            {"name": crate, "manifest_path": str(root / "crates" / crate / "Cargo.toml")}
            for crate in crates
        ],
    })


def test_coverage_failures(root: Path) -> None:
    policy = root / "quality/coverage.toml"
    report = root / "quality/fixtures/coverage-low.json"
    with modified(policy), modified(report):
        report.write_text(
            coverage_report(
                root,
                [
                    ("crates/intention-types/src/lib.rs", 100, 1),
                    ("crates/intention-daemon/src/main.rs", 1, 1),
                ],
            ),
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_coverage.py", "--policy", str(policy), "--report", str(report)],
            cwd=root,
            expect_success=False,
        )
def test_coverage_tier_policy_requires_exact_numeric_tiers(root: Path) -> None:
    policy = root / "quality/coverage.toml"
    report = root / "quality/fixtures/coverage-low.json"
    with modified(policy), modified(report):
        report.write_text(
            coverage_report(root, [("crates/intention-types/src/lib.rs", 100, 100)]),
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_coverage.py", "--policy", str(policy), "--report", str(report), "--crate", "intention-types"],
            cwd=root,
            expect_success=True,
            expected_output="intention-types line coverage 100.00% satisfies tier A",
        )
        invalid_tiers = [
            ("missing-tier", "A = 95.0\nB = 90.0\nC = 85.0\n", "A = 95.0\nB = 90.0\n"),
            ("extra-tier", "A = 95.0\nB = 90.0\nC = 85.0\n", "A = 95.0\nB = 90.0\nC = 85.0\nD = 80.0\n"),
            ("non-numeric", "A = 95.0\nB = 90.0\nC = 85.0\n", "A = 95.0\nB = 90.0\nC = \"high\"\n"),
        ]
        for _name, old, new in invalid_tiers:
            policy.write_text(
                policy.read_text(encoding="utf-8").replace(old, new, 1),
                encoding="utf-8",
            )
            run(
                [sys.executable, "quality/check_coverage.py", "--policy", str(policy), "--report", str(report)],
                cwd=root,
                expect_success=False,
                expected_output="tiers must define numeric A, B, and C thresholds",
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
                    ("crates/intention-tools/src/lib.rs", 100, 100),
                    ("crates/intention-workspace/src/lib.rs", 100, 100),
                    ("crates/intention-hooks/src/lib.rs", 100, 100),
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
                    ("crates/intention-tools/src/lib.rs", 100, 100),
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


def test_coverage_runner_policy_and_profile_names(root: Path) -> None:
    runner = root / "quality/run_coverage.py"
    coverage_policy = root / "quality/coverage.toml"
    source = runner.read_text(encoding="utf-8")
    if 'crate in {"intention-daemon", "intention-workspace"}' not in source:
        raise RuntimeError("coverage runner must use cargo test for boundary crates")
    if '"--all-targets"' not in source:
        raise RuntimeError("coverage must run all targets")
    aggregate_start = source.index("    for name, flags in combinations:\n", source.index("REPORTS.mkdir"))
    crate_loop_end = source.index("    for name, flags in combinations:\n", aggregate_start + 1)
    aggregate_block = source[crate_loop_end:source.index("\n\n\nif __name__", crate_loop_end)]
    if "--workspace-aggregate" not in aggregate_block:
        raise RuntimeError("coverage runner must check each workspace aggregate")
    if source.index("    for name, flags in combinations:\n", source.index("REPORTS.mkdir")) != aggregate_start:
        raise RuntimeError("coverage runner profile loop structure changed unexpectedly")
    if aggregate_block.count("--workspace-aggregate") != 1:
        raise RuntimeError("coverage runner must run one aggregate check per profile")
    with modified(coverage_policy):
        replace_once(coverage_policy, '"intention-tools",', '"intention-types",')
        if "ISOLATED_CRATES" in source:
            raise RuntimeError("coverage runner must not hard-code isolated crates")
        run(
            [sys.executable, "-m", "py_compile", "quality/run_coverage.py"],
            cwd=root,
            expect_success=True,
        )
    features = root / "quality/features.toml"
    with modified(features):
        replace_once(features, 'no_default = ["--no-default-features"]', 'custom = []')
        run(
            [sys.executable, "quality/run_coverage.py"],
            cwd=root,
            expect_success=False,
            expected_output="unsupported coverage profile name: custom",
        )


def test_quick_uses_one_explicit_default_test_profile(_root: Path) -> None:
    source = (ROOT / "Makefile").read_text(encoding="utf-8")
    if "quick: tools-check fmt-check lint" not in source:
        raise RuntimeError("quick target contract changed")
    if "quality/run_profiles.py test --profile default" not in source:
        raise RuntimeError("quick must run one explicit default profile")
    if "quick:\n\t$(CARGO) nextest" in source:
        raise RuntimeError("quick must not invoke an unprofiled nextest command")


def test_coverage_target_narrowing_requires_exact_inventory(root: Path) -> None:
    runner = root / "quality/run_coverage.py"
    coverage_policy = root / "quality/coverage.toml"
    source = runner.read_text(encoding="utf-8")
    if '"--all-targets"' not in source:
        raise RuntimeError("coverage runner must keep all targets by default")
    if "Target narrowing is intentionally disabled" not in source:
        raise RuntimeError("coverage runner must document why target narrowing is disabled")


def test_workspace_aggregate_coverage_check(root: Path) -> None:
    report = root / "quality/fixtures/coverage-low.json"
    with modified(report):
        report.write_text(
            coverage_report(
                root,
                [
                    ("crates/intention-types/src/lib.rs", 1, 0),
                    ("crates/intention-daemon/src/main.rs", 1, 0),
                ],
            ),
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_coverage.py", "--report", str(report), "--workspace-aggregate"],
            cwd=root,
            expect_success=True,
            expected_outputs=(
                "excluding crates/intention-daemon/src/main.rs from intention-daemon denominator",
                "workspace aggregate line coverage 0.000% (0/1)",
            ),
        )
        report.write_text(
            coverage_report(
                root,
                [
                    ("crates/intention-types/src/lib.rs", 100, 1),
                    ("crates/intention-daemon/src/main.rs", 100, 100),
                ],
            ),
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_coverage.py", "--report", str(report), "--workspace-aggregate"],
            cwd=root,
            expect_success=True,
            expected_output="workspace aggregate line coverage 1.000% (1/100)",
        )


def test_coverage_metadata_snapshot(root: Path) -> None:
    report = root / "quality/fixtures/coverage-low.json"
    metadata = root / "quality/fixtures/coverage-metadata.json"
    with modified(report), modified(metadata):
        report.write_text(
            coverage_report(root, [("crates/intention-types/src/lib.rs", 100, 100)]),
            encoding="utf-8",
        )
        metadata.write_text(
            metadata_snapshot(root, ["intention-types", "intention-daemon"]),
            encoding="utf-8",
        )
        run(
            [
                sys.executable,
                "quality/check_coverage.py",
                "--report",
                str(report),
                "--crate",
                "intention-types",
                "--metadata",
                str(metadata),
            ],
            cwd=root,
            expect_success=True,
            expected_output="intention-types line coverage 100.00% satisfies tier A",
        )
        report.write_text(
            coverage_report(
                root,
                [
                    ("crates/intention-types/src/lib.rs", 100, 100),
                    ("crates/intention-daemon/src/main.rs", 1, 0),
                ],
            ),
            encoding="utf-8",
        )
        run(
            [
                sys.executable,
                "quality/check_coverage.py",
                "--report",
                str(report),
                "--workspace-aggregate",
                "--metadata",
                str(metadata),
            ],
            cwd=root,
            expect_success=True,
            expected_outputs=(
                "excluding crates/intention-daemon/src/main.rs from intention-daemon denominator",
                "workspace aggregate line coverage 100.000% (100/100)",
            ),
        )


def test_coverage_metadata_standalone_fallback(root: Path) -> None:
    report = root / "quality/fixtures/coverage-low.json"
    with modified(report):
        report.write_text(
            coverage_report(root, [("crates/intention-types/src/lib.rs", 100, 100)]),
            encoding="utf-8",
        )
        run(
            [
                sys.executable,
                "quality/check_coverage.py",
                "--report",
                str(report),
                "--crate",
                "intention-types",
            ],
            cwd=root,
            expect_success=True,
            expected_output="intention-types line coverage 100.00% satisfies tier A",
        )


def test_coverage_metadata_invalid_snapshot(root: Path) -> None:
    report = root / "quality/fixtures/coverage-low.json"
    with tempfile.TemporaryDirectory() as temporary:
        missing = Path(temporary) / "missing.json"
        malformed = Path(temporary) / "malformed.json"
        malformed.write_text("{not json", encoding="utf-8")
        structural = Path(temporary) / "structural.json"
        structural.write_text(json.dumps({"packages": [{"name": "intention-types"}]}), encoding="utf-8")
        with modified(report):
            report.write_text(
                coverage_report(root, [("crates/intention-types/src/lib.rs", 100, 100)]),
                encoding="utf-8",
            )
            run(
                [sys.executable, "quality/check_coverage.py", "--report", str(report), "--crate", "intention-types", "--metadata", str(missing)],
                cwd=root,
                expect_success=False,
                expected_output="coverage metadata snapshot is missing",
            )
            run(
                [sys.executable, "quality/check_coverage.py", "--report", str(report), "--crate", "intention-types", "--metadata", str(malformed)],
                cwd=root,
                expect_success=False,
                expected_output="coverage metadata snapshot is not valid JSON",
            )
            run(
                [sys.executable, "quality/check_coverage.py", "--report", str(report), "--crate", "intention-types", "--metadata", str(structural)],
                cwd=root,
                expect_success=False,
                expected_output="must define name and manifest_path",
            )


def test_coverage_metadata_escape_rejected(root: Path) -> None:
    report = root / "quality/fixtures/coverage-low.json"
    metadata = root / "quality/fixtures/coverage-metadata.json"
    with modified(report), modified(metadata):
        report.write_text(
            coverage_report(root, [("crates/intention-types/src/lib.rs", 100, 100)]),
            encoding="utf-8",
        )
        invalid_snapshots = [
            (
                "traversal",
                {"packages": [{"name": "intention-types", "manifest_path": str(root / "crates/intention-types/../../../outside/Cargo.toml")}]},
            ),
            (
                "outside-root",
                {"packages": [{"name": "intention-types", "manifest_path": str(root.parent / "outside/Cargo.toml")}]},
            ),
        ]
        for _name, payload in invalid_snapshots:
            metadata.write_text(json.dumps(payload), encoding="utf-8")
            run(
                [sys.executable, "quality/check_coverage.py", "--report", str(report), "--crate", "intention-types", "--metadata", str(metadata)],
                cwd=root,
                expect_success=False,
                expected_output="manifest_path must not escape workspace root",
            )
        # Ordinary absolute and in-root traversal manifest paths stay accepted.
        metadata.write_text(
            json.dumps(
                {"packages": [{"name": "intention-types", "manifest_path": str(root / "crates/intention-types/../intention-types/Cargo.toml")}]}
            ),
            encoding="utf-8",
        )
        run(
            [sys.executable, "quality/check_coverage.py", "--report", str(report), "--crate", "intention-types", "--metadata", str(metadata)],
            cwd=root,
            expect_success=True,
            expected_output="intention-types line coverage 100.00% satisfies tier A",
        )


def test_coverage_metadata_collected_once_and_forwarded(_root: Path) -> None:
    namespace = {
        "__file__": str(ROOT / "quality/run_coverage.py"),
        "__name__": "quality.run_coverage",
    }
    exec((ROOT / "quality/run_coverage.py").read_text(encoding="utf-8"), namespace)
    captured: list[list[str]] = []
    payload = json.dumps({"packages": [{"name": "fixture", "manifest_path": "/tmp/fixture/Cargo.toml"}]})

    class FakeCompleted:
        def __init__(self, returncode: int, stdout: str = "") -> None:
            self.returncode = returncode
            self.stdout = stdout

    def fake_run_command(command: list[str], **_kwargs: object) -> FakeCompleted:
        captured.append(command)
        if command[:2] == ["cargo", "metadata"]:
            return FakeCompleted(0, payload)
        return FakeCompleted(0)

    namespace["run_command"] = fake_run_command
    previous_argv = sys.argv
    sys.argv = ["quality/run_coverage.py", "--profile", "default"]
    try:
        namespace["main"]()
    finally:
        sys.argv = previous_argv
    metadata_calls = [command for command in captured if command[:2] == ["cargo", "metadata"]]
    if metadata_calls != [["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"]]:
        raise RuntimeError(f"coverage metadata must be collected exactly once per run: {metadata_calls!r}")
    snapshot = namespace["metadata_snapshot_path"](namespace["ROOT"])
    if not snapshot.is_file() or snapshot.read_text(encoding="utf-8") != payload:
        raise RuntimeError("coverage runner must save the metadata snapshot under root/quality/reports")
    checker_calls = [
        command
        for command in captured
        if len(command) > 1 and command[0] == sys.executable and "check_coverage.py" in command[1]
    ]
    if not checker_calls:
        raise RuntimeError("coverage runner must invoke the coverage checker")
    for command in checker_calls:
        if command.count("--metadata") != 1 or command[command.index("--metadata") + 1] != str(snapshot):
            raise RuntimeError(f"every checker invocation must receive the collected metadata snapshot: {command!r}")
    if not any("--workspace-aggregate" in command for command in checker_calls):
        raise RuntimeError("coverage runner must check a workspace aggregate with the metadata snapshot")


def test_metrics_manifest_start_clears_stale_events(root: Path) -> None:
    reports = root / "quality" / "reports"
    reports.mkdir(parents=True, exist_ok=True)
    events = reports / "quality-run.events.jsonl"
    events.write_text('{"stale": true}\n', encoding="utf-8")
    run(
        [sys.executable, "quality/metrics.py", "start"],
        cwd=root,
        expect_success=True,
    )
    if events.exists() and events.read_text(encoding="utf-8").strip():
        raise RuntimeError("metrics start must clear stale event records")
    run(
        [sys.executable, "quality/metrics.py", "finish"],
        cwd=root,
        expect_success=True,
    )
    manifest = reports / "quality-run.json"
    if not manifest.exists():
        raise RuntimeError("metrics finish must write the manifest")


def test_supply_chain_policy_failures(root: Path) -> None:
    invalid_replacements = [
        ('unknown-git = "deny"', 'unknown-git = "allow"'),
        ('allow = ["Apache-2.0", "MIT", "Unicode-3.0", "0BSD", "Zlib", "BSD-3-Clause", "ISC", "CDLA-Permissive-2.0"]', 'allow = ["Apache-2.0"]'),
        ('multiple-versions = "deny"', 'multiple-versions = "allow"'),
        ('ignore = []', 'ignore = ["RUSTSEC-2024-0000"]'),
        ("version = 2", "version = 1"),
    ]
    policy = root / "deny.toml"
    outdated_policy = root / "quality/outdated.toml"
    with modified(policy), modified(outdated_policy):
        outdated_policy.write_text(
            outdated_policy.read_text(encoding="utf-8").replace(
                'crates = []', 'crates = ["async-openai"]', 1
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
    parser.add_argument(
        "--in-place",
        action="store_true",
        help="run repository fixtures against the working tree with git-restore "
        "scoping instead of an isolated copy (CI mode)",
    )
    arguments = parser.parse_args()
    repository_tests = [
        test_profile_arguments_include_critical_combinations,
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
        test_m5_activation_policy,
        test_m5_skeleton_drift_policy,
        test_m5_exact_active_crate_policy,
        test_configured_quality_harness_partition_policy,
        test_m4_sdk_ownership_and_public_contract_boundaries,
        test_m3_daemon_test_dependency_policy,
        test_m3_non_production_test_target_policy,
        test_m3_non_production_dependency_policy,
        test_m3_non_production_external_dependency_policy,
        test_m2_secret_projection,
        test_forbidden_source_boundary,
        test_test_only_source_patterns_are_cfg_scoped,
        test_adapter_isolation_boundary,
        test_adapter_dto_boundary,
        test_adapter_production_dependency_boundary,
        test_adapter_test_dependency_boundary,
        test_protocol_isolation_boundary,
        test_composition_ownership_boundary,
        test_composition_selection_ownership_policy,
        test_tools_workspace_dependency_boundary,
        test_provider_sdk_public_contract_boundary,
        test_error_detail_and_correlation_validation,
        test_coverage_failures,
        test_coverage_tier_policy_requires_exact_numeric_tiers,
        test_coverage_exclusion_semantics,
        test_coverage_runner_policy_and_profile_names,
        test_coverage_target_narrowing_requires_exact_inventory,
        test_metrics_manifest_start_clears_stale_events,
        test_quick_uses_one_explicit_default_test_profile,
        test_workspace_aggregate_coverage_check,
        test_coverage_metadata_snapshot,
        test_coverage_metadata_standalone_fallback,
        test_coverage_metadata_invalid_snapshot,
        test_coverage_metadata_escape_rejected,
        test_coverage_metadata_collected_once_and_forwarded,
        test_missing_feature_profile,
        test_supply_chain_policy_failures,
        test_secret_fixture,
    ]
    standalone_tests = [
        test_unused_dependency,
        test_outdated_dependency,
        test_sccache_collector_parses_pinned_v017_schema,
        test_sccache_collector_rejects_malformed_counters,
        test_benchmark_jobs_parsing,
        test_cleanup_gha_caches_decides_deletion,
    ]
    if arguments.list:
        for test in [*repository_tests, *standalone_tests]:
            print(test.__name__)
        return
    if arguments.in_place:
        print("quality-self-test: running repository fixtures in place", flush=True)
        for test in repository_tests:
            print(f"self-test: {test.__name__}", flush=True)
            test(ROOT)
    else:
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
