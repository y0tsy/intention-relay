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
        choices=["check", "isolated-release", "lint", "test", "doctest", "doc", "udeps"],
    )
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    parser.add_argument("--profile", dest="profile", default=None)
    arguments = parser.parse_args()

    with arguments.policy.open("rb") as policy_file:
        policy = tomllib.load(policy_file)

    if arguments.command == "isolated-release":
        profiles = policy.get("profiles")
        isolated_packages = policy.get("isolated_production_packages")
        target_flags = {"lib": ["--lib"], "bins": ["--bins"]}
        if not isinstance(profiles, dict) or not isinstance(isolated_packages, dict):
            raise RuntimeError("feature policy is missing isolated production package configuration")
        for package, configuration in isolated_packages.items():
            if not isinstance(package, str) or not isinstance(configuration, dict):
                raise RuntimeError("isolated production package configuration is invalid")
            profile_names = configuration.get("profiles")
            targets = configuration.get("targets")
            if not isinstance(profile_names, list) or not isinstance(targets, list):
                raise RuntimeError("isolated production package configuration is invalid")
            try:
                selected_target_flags = [flag for target in targets for flag in target_flags[target]]
            except (KeyError, TypeError) as error:
                raise RuntimeError("isolated production package target is invalid") from error
            for profile_name in profile_names:
                flags = profiles.get(profile_name)
                if not isinstance(profile_name, str) or not isinstance(flags, list):
                    raise RuntimeError("isolated production package profile is invalid")
                print(f"isolated release: {package} ({profile_name})", flush=True)
                run(["cargo", "check", "--package", package, *selected_target_flags, "--locked", *flags])
        return

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
