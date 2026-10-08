#!/usr/bin/env python3
"""Run one workspace Cargo quality command under the single feature configuration."""

from __future__ import annotations

import argparse
from pathlib import Path
import subprocess

if __package__:
    from .timing import run_command
else:
    from timing import run_command

ROOT = Path(__file__).resolve().parents[1]

# The workspace declares exactly one feature (`test-support`, non-production on
# `intention-daemon`), so every gate runs one configuration: `--all-features`
# is the superset the daemon's feature-gated tests require.
FEATURE_FLAGS = ["--all-features"]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "command",
        choices=["lint", "test", "doctest", "doc", "udeps"],
    )
    arguments = parser.parse_args()

    commands: dict[str, list[str]] = {
        "lint": ["cargo", "clippy", "--workspace", "--all-targets", "--locked", "--", "-Dwarnings"],
        "test": ["cargo", "nextest", "run", "--workspace", "--all-targets", "--locked"],
        "doctest": ["cargo", "test", "--workspace", "--doc", "--locked"],
        "doc": ["cargo", "doc", "--workspace", "--no-deps", "--locked"],
        "udeps": ["cargo", "+nightly-2026-07-31", "udeps", "--workspace", "--all-targets", "--locked"],
    }

    command = list(commands[arguments.command])
    if arguments.command == "lint":
        warning_flags = command[-2:]
        command = command[:-2] + FEATURE_FLAGS + warning_flags
    else:
        command.extend(FEATURE_FLAGS)
    environment = {"RUSTDOCFLAGS": "-D warnings"} if arguments.command == "doc" else None
    completed = run_command(
        command,
        cwd=ROOT,
        phase="gates",
        gate=arguments.command,
        env_overrides=environment,
    )
    if completed.returncode != 0:
        raise subprocess.CalledProcessError(completed.returncode, command)


if __name__ == "__main__":
    main()
