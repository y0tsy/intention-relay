#!/usr/bin/env python3
"""Run a Cargo quality command for each required feature profile."""

from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_POLICY = ROOT / "quality" / "features.toml"


def run(command: list[str]) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, check=True, cwd=ROOT)


def profile_arguments(policy: dict[str, object]) -> list[tuple[str, list[str]]]:
    profiles = policy["profiles"]
    assert isinstance(profiles, dict)
    result = [(name, list(flags)) for name, flags in profiles.items()]
    for combination in policy.get("critical_combinations", []):
        if combination["enabled"]:
            result.append((f"critical:{combination['name']}", ["--features", ",".join(combination["features"])]))
    return result


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["check", "lint", "test", "doctest", "doc", "udeps"])
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    arguments = parser.parse_args()

    with arguments.policy.open("rb") as policy_file:
        policy = tomllib.load(policy_file)

    commands: dict[str, list[str]] = {
        "check": ["cargo", "check", "--workspace", "--all-targets", "--locked"],
        "lint": ["cargo", "clippy", "--workspace", "--all-targets", "--locked", "--", "-Dwarnings"],
        "test": ["cargo", "nextest", "run", "--workspace", "--all-targets", "--locked"],
        "doctest": ["cargo", "test", "--workspace", "--doc", "--locked"],
        "doc": ["cargo", "doc", "--workspace", "--no-deps", "--locked"],
        "udeps": ["cargo", "+nightly-2026-07-31", "udeps", "--workspace", "--all-targets", "--locked"],
    }

    for profile_name, flags in profile_arguments(policy):
        print(f"profile: {profile_name}", flush=True)
        command = list(commands[arguments.command])
        if arguments.command == "lint":
            warning_flags = command[-2:]
            command = command[:-2] + flags + warning_flags
        else:
            command.extend(flags)
        environment = None
        if arguments.command == "doc":
            environment = {**dict(), "RUSTDOCFLAGS": "-D warnings"}
        if environment is None:
            run(command)
        else:
            print("+", " ".join(command), flush=True)
            subprocess.run(command, check=True, cwd=ROOT, env={**__import__("os").environ, **environment})


if __name__ == "__main__":
    main()
