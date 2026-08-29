#!/usr/bin/env python3
"""Validate the declared Cargo feature profile policy."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_POLICY = ROOT / "quality" / "features.toml"
REQUIRED_PROFILES = {
    "default": [],
    "no_default": ["--no-default-features"],
    "all": ["--all-features"],
}


def fail(message: str) -> None:
    print(f"features-check: {message}", file=sys.stderr)
    raise SystemExit(1)


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
        for combination in combinations:
            if combination["enabled"]:
                print(f"critical:{combination['name']}: --features {','.join(combination['features'])}")

    print("features-check: required feature profiles are valid")


if __name__ == "__main__":
    main()
