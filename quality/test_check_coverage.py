#!/usr/bin/env python3
"""Prove the coverage checker's per-crate tier decisions with synthetic inputs."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CHECKER = ROOT / "quality" / "check_coverage.py"

POLICY = """\
[policy]
phase = "m5"

[tiers]
core = 75.0
standard = 60.0
edge = 20.0
exempt = 0.0

[crate_tiers]
"fixture-core" = "core"
"fixture-standard" = "standard"
"fixture-edge" = "edge"
"fixture-exempt" = "exempt"
"""

CRATES = ("fixture-core", "fixture-standard", "fixture-edge", "fixture-exempt")


def write_policy(root: Path, name: str = "coverage.toml", text: str = POLICY) -> Path:
    policy = root / name
    policy.write_text(text, encoding="utf-8")
    return policy


def write_metadata(root: Path) -> Path:
    metadata = root / "metadata.json"
    metadata.write_text(
        json.dumps(
            {
                "packages": [
                    {
                        "name": crate,
                        "manifest_path": str(root / "crates" / crate / "Cargo.toml"),
                    }
                    for crate in CRATES
                ]
            }
        ),
        encoding="utf-8",
    )
    return metadata


def write_report(root: Path, lines: dict[str, tuple[int, int]]) -> Path:
    """Write a summary-only llvm-cov report with one file per named crate."""
    report = root / "coverage-report.json"
    report.write_text(
        json.dumps(
            {
                "data": [
                    {
                        "files": [
                            {
                                "filename": f"crates/{crate}/src/lib.rs",
                                "summary": {
                                    "lines": {"count": count, "covered": covered},
                                },
                            }
                            for crate, (count, covered) in lines.items()
                        ]
                    }
                ]
            }
        ),
        encoding="utf-8",
    )
    return report


def check(root: Path, policy: Path, report: Path, metadata: Path, *arguments: str) -> str:
    completed = subprocess.run(
        [
            sys.executable,
            str(CHECKER),
            "--root",
            str(root),
            "--policy",
            str(policy),
            "--report",
            str(report),
            "--metadata",
            str(metadata),
            *arguments,
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    output = f"{completed.stdout}\n{completed.stderr}".strip()
    if completed.returncode != 0:
        return f"FAILED ({completed.returncode})\n{output}"
    return output


def expect_pass(output: str, expected: str) -> None:
    if output.startswith("FAILED"):
        raise RuntimeError(f"expected success, got: {output}")
    if expected not in output:
        raise RuntimeError(f"expected output to contain {expected!r}, got: {output}")


def expect_fail(output: str, expected: str) -> None:
    if not output.startswith("FAILED"):
        raise RuntimeError(f"expected failure, got success: {output}")
    if expected not in output:
        raise RuntimeError(f"expected output to contain {expected!r}, got: {output}")


def test_tier_floors_are_per_crate(root: Path, policy: Path, metadata: Path) -> None:
    """Each crate is judged against its own tier floor, not a shared base."""
    report = write_report(
        root,
        {
            "fixture-core": (100, 74),
            "fixture-standard": (100, 60),
            "fixture-edge": (100, 20),
        },
    )
    expect_pass(
        check(root, policy, report, metadata, "--crate", "fixture-standard"),
        "fixture-standard line coverage 60.00% satisfies standard tier threshold 60.00%",
    )
    expect_pass(
        check(root, policy, report, metadata, "--crate", "fixture-edge"),
        "fixture-edge line coverage 20.00% satisfies edge tier threshold 20.00%",
    )
    expect_fail(
        check(root, policy, report, metadata, "--crate", "fixture-core"),
        "fixture-core line coverage 74.000% is below required core tier threshold 75.000%",
    )


def test_exempt_crate_is_outside_collection(root: Path, policy: Path, metadata: Path) -> None:
    """A 0% tier declares an exempt crate that the checker never collects."""
    report = write_report(root, {"fixture-exempt": (100, 100)})
    expect_fail(
        check(root, policy, report, metadata, "--crate", "fixture-exempt"),
        "coverage crate 'fixture-exempt' is exempt (0% tier) and is not collected",
    )
    expect_fail(
        check(root, policy, report, metadata, "--crate", "fixture-unknown"),
        "coverage crate 'fixture-unknown' is not declared in [crate_tiers]",
    )


def test_aggregate_has_no_threshold_and_excludes_exempt(
    root: Path, policy: Path, metadata: Path
) -> None:
    """The aggregate stays informational and never counts an exempt crate."""
    report = write_report(root, {"fixture-core": (100, 10), "fixture-exempt": (100, 0)})
    expect_pass(
        check(root, policy, report, metadata, "--workspace-aggregate"),
        "workspace aggregate line coverage 10.000% (10/100)",
    )


def test_policy_errors_are_typed(root: Path, _policy: Path, metadata: Path) -> None:
    """Invalid tier tables fail with the checker's typed policy errors."""
    report = write_report(root, {"fixture-core": (100, 100)})
    invalid = [
        (
            "unknown-tier",
            '"fixture-core" = "core"',
            '"fixture-core" = "gold"',
            "coverage crate 'fixture-core' must name a tier declared in [tiers]",
        ),
        (
            "non-numeric-tier",
            "core = 75.0",
            'core = "high"',
            "coverage tier 'core' must be numeric",
        ),
        (
            "out-of-range-tier",
            "exempt = 0.0",
            "exempt = -1.0",
            "coverage tier 'exempt' must be between 0 and 100",
        ),
    ]
    for name, old, new, expected in invalid:
        mutated = write_policy(root, f"{name}.toml", POLICY.replace(old, new, 1))
        expect_fail(check(root, mutated, report, metadata, "--crate", "fixture-core"), expected)


def test_collected_crate_without_lines_fails(root: Path, policy: Path, metadata: Path) -> None:
    """A collected crate with no reportable lines cannot satisfy its floor."""
    report = write_report(root, {"fixture-core": (0, 0)})
    expect_fail(
        check(root, policy, report, metadata, "--crate", "fixture-core"),
        "coverage crate 'fixture-core' has no reportable non-excluded source lines",
    )


def main() -> None:
    tests = [
        test_tier_floors_are_per_crate,
        test_exempt_crate_is_outside_collection,
        test_aggregate_has_no_threshold_and_excludes_exempt,
        test_policy_errors_are_typed,
        test_collected_crate_without_lines_fails,
    ]
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        policy = write_policy(root)
        metadata = write_metadata(root)
        for test in tests:
            print(f"coverage-check self-test: {test.__name__}", flush=True)
            test(root, policy, metadata)
    print(f"coverage-check self-test: {len(tests)} focused cases passed")


if __name__ == "__main__":
    main()
