#!/usr/bin/env python3
"""Single source for the pinned toolchain selectors the quality runners use."""

from __future__ import annotations

from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def tools_policy(root: Path = ROOT) -> dict[str, object]:
    """Load the pinned toolchain, component, and tool policy."""
    with (root / "quality" / "tools.toml").open("rb") as policy_file:
        return tomllib.load(policy_file)


def nightly_pin(root: Path = ROOT) -> str:
    """Return the pinned dated nightly toolchain name from the tool policy."""
    toolchains = tools_policy(root).get("toolchains")
    nightly = toolchains.get("nightly") if isinstance(toolchains, dict) else None
    if not isinstance(nightly, str) or not nightly:
        raise ValueError("quality/tools.toml requires a non-blank [toolchains].nightly pin")
    return nightly


def nightly_selector(root: Path = ROOT) -> str:
    """Return the `cargo +<pin>` selector for the pinned nightly toolchain."""
    return f"+{nightly_pin(root)}"
