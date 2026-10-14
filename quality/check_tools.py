#!/usr/bin/env python3
"""Validate the pinned Rust toolchain and the quality tools one gate runs."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_POLICY = ROOT / "quality" / "tools.toml"

def fail(message: str) -> None:
    print(f"tools-check: {message}", file=sys.stderr)
    raise SystemExit(1)

def run(command: list[str]) -> str:
    try:
        completed = subprocess.run(command, check=True, capture_output=True, text=True)
    except FileNotFoundError:
        fail(f"required command is unavailable: {command[0]}")
    except subprocess.CalledProcessError as error:
        output = error.stderr.strip() or error.stdout.strip()
        fail(f"command failed ({' '.join(command)}): {output}")
    return f"{completed.stdout}\n{completed.stderr}"

def require_version(label: str, command: list[str], expected: str) -> None:
    output = run(command)
    if not re.search(rf"(?<![0-9.]){re.escape(expected)}(?![0-9.])", output):
        fail(f"{label} version mismatch: expected {expected}, got {output.strip()!r}")

def require_toolchain(name: str) -> None:
    output = run(["rustup", "toolchain", "list"])
    installed = {line.split()[0] for line in output.splitlines() if line.split()}
    if not any(item == name or item.startswith(f"{name}-") for item in installed):
        fail(f"required toolchain is unavailable: {name}")

def require_component(toolchain: str, component: str) -> None:
    output = run(["rustup", "component", "list", "--installed", "--toolchain", toolchain])
    prefix = component.replace("-preview", "")
    if not any(line.startswith(prefix) for line in output.splitlines()):
        fail(f"{toolchain} is missing required component {component}")

def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    parser.add_argument(
        "--component",
        action="append",
        default=[],
        metavar="NAME",
        help="validate one rustup component of the pinned toolchain",
    )
    parser.add_argument(
        "--tool",
        action="append",
        default=[],
        metavar="NAME",
        help="validate one pinned quality tool and its exact version",
    )
    arguments = parser.parse_args()

    with arguments.policy.open("rb") as policy_file:
        policy = tomllib.load(policy_file)
    stable = policy["toolchains"]["stable"]
    pinned_components = set(policy["components"]["stable"])
    pinned_tools = {tool["name"]: tool for tool in policy["tools"]}

    require_toolchain(stable)
    require_version("Rust stable", ["rustup", "run", stable, "rustc", "--version"], stable)

    for component in arguments.component:
        if component not in pinned_components:
            fail(f"{component} is not a pinned component in quality/tools.toml")
        require_component(stable, component)

    for name in arguments.tool:
        tool = pinned_tools.get(name)
        if tool is None:
            fail(f"{name} is not a pinned tool in quality/tools.toml")
        if shutil.which(name) is None:
            fail(f"required tool is unavailable: {name}")
        require_version(name, tool["command"], tool["version"])

    print("tools-check: the pinned toolchain, its components, and the gate's tools are available")

if __name__ == "__main__":
    main()
