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

# Scoped CI checks: every CI job validates only the toolchains, components,
# and tools its phase actually uses, while local runs (no scope) still
# validate the complete pinned set. The union of all scopes equals the
# unscoped policy, so no pinned tool or version escapes validation.
SCOPES = ("all", "lint-arch", "test", "coverage", "selftest", "deps")


def scope_toolchains(scope: str, policy: dict[str, object]) -> list[str]:
    toolchains = policy["toolchains"]
    stable = toolchains["stable"]
    nightly = toolchains["nightly"]
    return {
        "all": [stable, nightly],
        "lint-arch": [stable],
        "test": [stable],
        "coverage": [nightly],
        "selftest": [stable],
        "deps": [stable, nightly],
    }[scope]


def scope_components(scope: str, policy: dict[str, object]) -> dict[str, list[str]]:
    toolchains = policy["toolchains"]
    stable = toolchains["stable"]
    nightly = toolchains["nightly"]
    return {
        "all": {
            stable: list(policy["components"]["stable"]),
            nightly: list(policy["components"]["nightly"]),
        },
        "lint-arch": {stable: ["rustfmt", "clippy"]},
        "test": {},
        "coverage": {nightly: ["llvm-tools-preview"]},
        "selftest": {stable: ["rustfmt", "clippy"]},
        "deps": {},
    }[scope]


def scope_tools(scope: str, policy: dict[str, object]) -> list[dict[str, object]]:
    tools = list(policy["tools"])
    if scope == "all":
        return tools
    names = {
        "lint-arch": set(),
        "test": {"cargo-nextest"},
        "coverage": {"cargo-nextest", "cargo-llvm-cov"},
        "selftest": {"cargo-machete", "cargo-outdated"},
        "deps": {
            "cargo-deny",
            "cargo-audit",
            "cargo-udeps",
            "cargo-machete",
            "cargo-about",
            "cargo-outdated",
        },
    }[scope]
    return [tool for tool in tools if tool.get("name") in names]


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
    parser.add_argument(
        "--scope",
        choices=SCOPES,
        default="all",
        help="validate only the toolchains, components, and tools for one CI "
        "phase scope; defaults to the complete pinned set for local runs",
    )
    arguments = parser.parse_args()

    with arguments.policy.open("rb") as policy_file:
        policy = tomllib.load(policy_file)

    stable = policy["toolchains"]["stable"]
    nightly = policy["toolchains"]["nightly"]
    selected_toolchains = scope_toolchains(arguments.scope, policy)
    for toolchain in selected_toolchains:
        require_toolchain(toolchain)
    if stable in selected_toolchains:
        require_version("Rust stable", ["rustup", "run", stable, "rustc", "--version"], stable)
    if nightly in selected_toolchains:
        if not nightly.startswith("nightly-"):
            fail("nightly toolchain must use a dated nightly-YYYY-MM-DD pin")
        nightly_rustc_version = policy["toolchains"].get("nightly_rustc_version")
        if not isinstance(nightly_rustc_version, str) or not nightly_rustc_version:
            fail("nightly_rustc_version must be an explicit Rust version pin")
        require_version("Rust nightly", ["rustup", "run", nightly, "rustc", "--version"], nightly_rustc_version)
        run(["rustup", "run", nightly, "rustdoc", "--version"])

    for toolchain, components in scope_components(arguments.scope, policy).items():
        for component in components:
            require_component(toolchain, component)

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

    for tool in scope_tools(arguments.scope, policy):
        executable = shutil.which(tool["name"])
        if executable is None:
            fail(f"required tool is unavailable: {tool['name']}")
        if canonical_bin is not None and Path(executable).resolve().parent != canonical_bin:
            fail(f"{tool['name']} must resolve from canonical Cargo bin: {canonical_bin}")
        require_version(tool["name"], tool["command"], tool["version"])

    print(f"tools-check: pinned toolchains, components, and tools are available (scope {arguments.scope})")


if __name__ == "__main__":
    main()
