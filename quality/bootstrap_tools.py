#!/usr/bin/env python3
"""Install the exact quality toolchain declared by quality/tools.toml."""

from __future__ import annotations

from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str]) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, check=True, cwd=ROOT)


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
        command = ["cargo", "install", "--locked", "--force", "--version", version, name]
        if name == "cargo-udeps":
            command.insert(1, f"+{nightly}")
        run(command)


if __name__ == "__main__":
    main()
