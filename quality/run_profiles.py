#!/usr/bin/env python3
"""Run a Cargo quality command for each required feature profile."""

from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys
import tomllib

if __package__:
    from .timing import run_command
else:
    from timing import run_command

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_POLICY = ROOT / "quality" / "features.toml"


def run(command: list[str]) -> None:
    completed = run_command(command, cwd=ROOT, phase="profiles", gate=command[1])
    if completed.returncode != 0:
        raise subprocess.CalledProcessError(completed.returncode, command)

def filter_profiles(
    selected: list[tuple[str, list[str]]], requested: list[str]
) -> list[tuple[str, list[str]]]:
    """Restrict profile runs to the requested names (CI split).

    Unknown requested names raise so a typo in a Makefile target or workflow
    cannot silently drop a required profile.
    """
    wanted = set(requested)
    result = [entry for entry in selected if entry[0] in wanted]
    unknown = wanted - {entry[0] for entry in result}
    if unknown:
        raise RuntimeError(f"unknown quality profiles: {', '.join(sorted(unknown))}")
    return result


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
    parser.add_argument(
        "command",
        choices=["check", "lint", "test", "doctest", "doc", "udeps"],
    )
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    parser.add_argument("--profile", dest="profile", default=None)
    parser.add_argument(
        "--profiles",
        default=None,
        help="comma-separated profile names to run (CI split)",
    )
    arguments = parser.parse_args()
    if arguments.profile is not None and arguments.profiles is not None:
        raise RuntimeError("--profile and --profiles are mutually exclusive")

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

    selected_profiles = profile_arguments(policy)
    if arguments.profile is not None:
        selected_profiles = [entry for entry in selected_profiles if entry[0] == arguments.profile]
        if not selected_profiles:
            raise RuntimeError(f"unknown quality profile: {arguments.profile}")
    if arguments.profiles is not None:
        requested = [name.strip() for name in arguments.profiles.split(",") if name.strip()]
        selected_profiles = filter_profiles(selected_profiles, requested)
        if not selected_profiles:
            raise RuntimeError(f"no quality profiles matched: {arguments.profiles}")

    for profile_name, flags in selected_profiles:
        print(f"profile: {profile_name}", flush=True)
        command = list(commands[arguments.command])
        if arguments.command == "lint":
            warning_flags = command[-2:]
            command = command[:-2] + flags + warning_flags
        else:
            command.extend(flags)
        environment = None
        if arguments.command == "doc":
            environment = {"RUSTDOCFLAGS": "-D warnings"}
        if environment is None:
            run(command)
        else:
            completed = run_command(
                command,
                cwd=ROOT,
                phase="profiles",
                gate=arguments.command,
                profile=profile_name,
                env_overrides=environment,
            )
            if completed.returncode != 0:
                raise subprocess.CalledProcessError(completed.returncode, command)


if __name__ == "__main__":
    main()
