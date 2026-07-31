#!/usr/bin/env python3
"""Validate M0 and future crate-boundary policy without product crates."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_POLICY = ROOT / "quality" / "architecture.toml"


def fail(message: str) -> None:
    print(f"architecture-check: {message}", file=sys.stderr)
    raise SystemExit(1)


def load_toml(path: Path) -> dict[str, object]:
    with path.open("rb") as policy_file:
        return tomllib.load(policy_file)


def workspace_members(root: Path) -> set[str]:
    cargo = shutil.which("cargo")
    if cargo is None:
        fail("cargo is unavailable; run make tools-check in the canonical Rust environment")
    completed = subprocess.run(
        [cargo, "metadata", "--no-deps", "--format-version", "1", "--locked"],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    metadata = json.loads(completed.stdout)
    return {package["name"] for package in metadata["packages"]}


def source_files(root: Path) -> list[Path]:
    ignored = {"target", ".git", "fixtures", "__pycache__"}
    return [
        path
        for path in root.rglob("*.rs")
        if not any(part in ignored for part in path.relative_to(root).parts)
    ]


def check_reasoned_lints(path: Path, text: str) -> list[str]:
    failures: list[str] = []
    for match in re.finditer(r"#!?\[(allow|expect)\((.*?)\)\]", text, flags=re.DOTALL):
        content = match.group(2)
        if "reason" not in content:
            failures.append(f"{path}: {match.group(1)} requires reason = \"...\"")
        if "clippy::all" in content or "clippy::nursery" in content:
            failures.append(f"{path}: broad lint suppression is forbidden")
    return failures


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    arguments = parser.parse_args()

    root = arguments.root.resolve()
    policy = load_toml(arguments.policy)
    policy_state = policy.get("policy")
    if not isinstance(policy_state, dict):
        fail("missing [policy] table")
    phase = policy_state.get("phase")
    if phase not in {"m0", "m1"}:
        fail("policy phase must be m0 or m1")

    crates = policy.get("future_crates")
    if not isinstance(crates, list) or not crates:
        fail("future crate declarations are required")
    names: set[str] = set()
    valid_tiers = {"A", "B", "C", "adapter"}
    for crate in crates:
        if not isinstance(crate, dict):
            fail("future crate declaration must be a table")
        name = crate.get("name")
        if not isinstance(name, str) or not name.startswith("intention"):
            fail("future crate names must begin with intention")
        if name in names:
            fail(f"duplicate future crate declaration {name}")
        names.add(name)
        for field in ("responsibility", "test_target"):
            if not isinstance(crate.get(field), str) or not crate[field]:
                fail(f"future crate {name} requires {field}")
        if crate.get("coverage_tier") not in valid_tiers:
            fail(f"future crate {name} has invalid coverage tier")

    actual_members = workspace_members(root)
    allowed_m0 = set(policy_state.get("allowed_m0_workspace_members", []))
    if phase == "m0":
        if actual_members != allowed_m0:
            fail(f"M0 workspace members must equal {sorted(allowed_m0)}, got {sorted(actual_members)}")
        if any(member.startswith("intention") for member in actual_members):
            fail("M0 may not contain an Intention Relay product crate")
    else:
        missing = names - actual_members
        if missing:
            fail(f"M1+ workspace is missing declared crates: {sorted(missing)}")

    forbidden = policy.get("forbidden")
    if not isinstance(forbidden, dict):
        fail("missing [forbidden] table")
    patterns = forbidden.get("source_patterns")
    if not isinstance(patterns, list):
        fail("forbidden source patterns must be a list")

    failures: list[str] = []
    for path in source_files(root):
        text = path.read_text(encoding="utf-8")
        for pattern in patterns:
            if pattern in text:
                failures.append(f"{path}: forbidden source pattern {pattern!r}")
        failures.extend(check_reasoned_lints(path, text))

    if failures:
        fail("\n".join(failures))
    print(f"architecture-check: {phase} policy and source boundaries are valid")


if __name__ == "__main__":
    main()
