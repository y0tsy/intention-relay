#!/usr/bin/env python3
"""Validate required cargo-deny policy controls before online graph checks."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_POLICY = ROOT / "deny.toml"


def fail(message: str) -> None:
    print(f"deny-policy-check: {message}", file=sys.stderr)
    raise SystemExit(1)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    arguments = parser.parse_args()
    with arguments.policy.open("rb") as policy_file:
        policy = tomllib.load(policy_file)

    graph = policy.get("graph")
    advisories = policy.get("advisories")
    licenses = policy.get("licenses")
    bans = policy.get("bans")
    sources = policy.get("sources")
    if not all(isinstance(section, dict) for section in (graph, advisories, licenses, bans, sources)):
        fail("graph, advisories, licenses, bans, and sources sections are required")
    if graph.get("all-features") is not True:
        fail("graph.all-features must be true")
    if advisories.get("version") != 2 or not isinstance(advisories.get("ignore"), list):
        fail("advisories must use version 2 with an explicit ignore list")
    allowed = licenses.get("allow")
    if not isinstance(allowed, list) or not allowed:
        fail("licenses.allow must be an explicit non-empty list")
    if bans.get("multiple-versions") != "deny" or bans.get("wildcards") != "deny":
        fail("bans must deny multiple versions and wildcards")
    if sources.get("unknown-registry") != "deny" or sources.get("unknown-git") != "deny":
        fail("sources must deny unknown registries and git sources")
    registries = sources.get("allow-registry")
    if registries != ["https://github.com/rust-lang/crates.io-index"]:
        fail("sources.allow-registry must allow only crates.io")
    for section_name, section in (("advisories", advisories), ("licenses", licenses), ("bans", bans), ("sources", sources)):
        for exception_name in ("ignore", "exceptions", "deny", "skip", "skip-tree", "allow-git"):
            entries = section.get(exception_name)
            if entries is not None and not isinstance(entries, list):
                fail(f"{section_name}.{exception_name} must be a list")
    print("deny-policy-check: required supply-chain controls are valid")


if __name__ == "__main__":
    main()
