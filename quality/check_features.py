#!/usr/bin/env python3
"""Validate the declared Cargo feature profile policy."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_POLICY = ROOT / "quality" / "features.toml"
REQUIRED_PROFILES = {
    "default": [],
    "no_default": ["--no-default-features"],
    "all": ["--all-features"],
}
ISOLATED_TARGETS = {"lib", "bins"}


def fail(message: str) -> None:
    print(f"features-check: {message}", file=sys.stderr)
    raise SystemExit(1)


def workspace_package_names() -> set[str]:
    completed = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        fail("cannot enumerate workspace packages from locked Cargo metadata")
    try:
        metadata = json.loads(completed.stdout)
        packages = metadata["packages"]
        names = {package["name"] for package in packages}
    except (KeyError, TypeError, json.JSONDecodeError):
        fail("Cargo metadata has an invalid package shape")
    if not all(isinstance(name, str) for name in names):
        fail("Cargo metadata has an invalid package name")
    return names


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    parser.add_argument("--print", action="store_true")
    arguments = parser.parse_args()

    with arguments.policy.open("rb") as policy_file:
        policy = tomllib.load(policy_file)

    profiles = policy.get("profiles")
    if not isinstance(profiles, dict):
        fail("missing [profiles] table")
    for name, expected in REQUIRED_PROFILES.items():
        if profiles.get(name) != expected:
            fail(f"profile {name!r} must equal {expected!r}")

    isolated_packages = policy.get("isolated_production_packages")
    if not isinstance(isolated_packages, dict) or not isolated_packages:
        fail("missing [isolated_production_packages] table")
    workspace_packages = workspace_package_names()
    for package, configuration in isolated_packages.items():
        if not isinstance(package, str) or not package:
            fail("isolated production package names must be non-empty strings")
        if package not in workspace_packages:
            fail(f"isolated production package {package!r} is not a workspace package")
        if not isinstance(configuration, dict):
            fail(f"isolated production package {package!r} requires a configuration table")
        profile_names = configuration.get("profiles")
        targets = configuration.get("targets")
        if not isinstance(profile_names, list) or not all(
            isinstance(profile, str) and profile in REQUIRED_PROFILES for profile in profile_names
        ):
            fail(f"isolated production package {package!r} requires known profile names")
        if len(profile_names) != len(set(profile_names)):
            fail(f"isolated production package {package!r} has duplicate profiles")
        if not profile_names:
            fail(f"isolated production package {package!r} requires at least one profile")
        if not isinstance(targets, list) or not all(
            isinstance(target, str) and target in ISOLATED_TARGETS for target in targets
        ):
            fail(f"isolated production package {package!r} requires known release targets")
        if len(targets) != len(set(targets)):
            fail(f"isolated production package {package!r} has duplicate release targets")
        if not targets:
            fail(f"isolated production package {package!r} requires at least one release target")

    combinations = policy.get("critical_combinations", [])
    names: set[str] = set()
    for combination in combinations:
        name = combination.get("name")
        enabled = combination.get("enabled")
        features = combination.get("features")
        if not isinstance(name, str) or not name:
            fail("critical combinations require a non-empty name")
        if name in names:
            fail(f"duplicate critical combination {name!r}")
        names.add(name)
        if not isinstance(enabled, bool):
            fail(f"critical combination {name!r} requires boolean enabled")
        if not isinstance(features, list) or not all(isinstance(item, str) and item for item in features):
            fail(f"critical combination {name!r} requires a string feature list")
        if enabled and not features:
            fail(f"enabled critical combination {name!r} cannot be empty")

    if arguments.print:
        for name, flags in REQUIRED_PROFILES.items():
            print(f"{name}: {' '.join(flags) or '(default)'}")
        for package, configuration in isolated_packages.items():
            print(
                f"isolated:{package}: profiles={', '.join(configuration['profiles'])}; "
                f"targets={', '.join(configuration['targets'])}"
            )
        for combination in combinations:
            if combination["enabled"]:
                print(f"critical:{combination['name']}: --features {','.join(combination['features'])}")

    print("features-check: required feature profiles are valid")


if __name__ == "__main__":
    main()
