#!/usr/bin/env python3
"""Install exact quality tools without rebuilding matching restored binaries."""

from __future__ import annotations

from pathlib import Path
import re
import shutil
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str]) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, check=True, cwd=ROOT)


def tool_matches_version(tool: dict[str, object]) -> bool:
    name = tool["name"]
    version = tool["version"]
    assert isinstance(name, str) and isinstance(version, str)
    if shutil.which(name) is None:
        return False
    command = tool["command"]
    assert isinstance(command, list) and all(isinstance(argument, str) for argument in command)
    completed = subprocess.run(command, capture_output=True, text=True, cwd=ROOT)
    output = f"{completed.stdout}\n{completed.stderr}"
    return completed.returncode == 0 and re.search(rf"(?<![0-9.]){re.escape(version)}(?![0-9.])", output) is not None


def main() -> None:
    with (ROOT / "quality" / "tools.toml").open("rb") as policy_file:
        policy = tomllib.load(policy_file)
    stable = policy["toolchains"]["stable"]
    nightly = policy["toolchains"]["nightly"]

    run(["rustup", "toolchain", "install", stable, "--profile", "minimal"])
    run(["rustup", "toolchain", "install", nightly, "--profile", "minimal"])
    for component in policy["components"]["stable"]:
        run(["rustup", "component", "add", component, "--toolchain", stable])
    for component in policy["components"]["nightly"]:
        run(["rustup", "component", "add", component, "--toolchain", nightly])

    for tool in policy["tools"]:
        name = tool["name"]
        version = tool["version"]
        assert isinstance(name, str) and isinstance(version, str)
        if tool_matches_version(tool):
            print(f"+ {name} {version} already installed", flush=True)
            continue
        command = ["cargo", "install", "--locked", "--force", "--version", version, name]
        features = tool.get("install_features", [])
        if not isinstance(features, list) or not all(isinstance(feature, str) and feature for feature in features):
            raise RuntimeError(f"tool {name} has invalid install_features")
        if features:
            command.extend(["--features", ",".join(features)])
        if name == "cargo-udeps":
            command.insert(1, f"+{nightly}")
        run(command)


if __name__ == "__main__":
    main()
