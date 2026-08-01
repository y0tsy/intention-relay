#!/usr/bin/env python3
"""Validate pinned Rust toolchains, components, and Cargo quality tools."""

from __future__ import annotations

import argparse
import os
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
        completed = subprocess.run(
            command,
            check=True,
            capture_output=True,
            text=True,
            env=os.environ.copy(),
        )
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
    arguments = parser.parse_args()

    with arguments.policy.open("rb") as policy_file:
        policy = tomllib.load(policy_file)

    stable = policy["toolchains"]["stable"]
    nightly = policy["toolchains"]["nightly"]
    require_toolchain(stable)
    require_toolchain(nightly)
    require_version("Rust stable", ["rustup", "run", stable, "rustc", "--version"], stable)
    if not nightly.startswith("nightly-"):
        fail("nightly toolchain must use a dated nightly-YYYY-MM-DD pin")
    nightly_rustc_version = policy["toolchains"].get("nightly_rustc_version")
    if not isinstance(nightly_rustc_version, str) or not nightly_rustc_version:
        fail("nightly_rustc_version must be an explicit Rust version pin")
    require_version("Rust nightly", ["rustup", "run", nightly, "rustc", "--version"], nightly_rustc_version)
    run(["rustup", "run", nightly, "rustdoc", "--version"])

    for component in policy["components"]["stable"]:
        require_component(stable, component)
    for component in policy["components"]["nightly"]:
        require_component(nightly, component)

    cargo_home = os.environ.get("CARGO_HOME")
    canonical_bin: Path | None = None
    if cargo_home:
        canonical_bin = Path(cargo_home).expanduser().resolve() / "bin"
        path_entries = [Path(item).expanduser().resolve() for item in os.environ.get("PATH", "").split(os.pathsep) if item]
        if canonical_bin not in path_entries:
            fail(f"canonical Cargo bin is missing from PATH: {canonical_bin}")
        cargo = shutil.which("cargo")
        if cargo is None or Path(cargo).resolve().parent != canonical_bin:
            fail(f"cargo must resolve from canonical Cargo bin: {canonical_bin}")

    for tool in policy["tools"]:
        executable = shutil.which(tool["name"])
        if executable is None:
            fail(f"required tool is unavailable: {tool['name']}")
        if canonical_bin is not None and Path(executable).resolve().parent != canonical_bin:
            fail(f"{tool['name']} must resolve from canonical Cargo bin: {canonical_bin}")
        require_version(tool["name"], tool["command"], tool["version"])

    print("tools-check: pinned toolchains, components, and tools are available")


if __name__ == "__main__":
    main()
