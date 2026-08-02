#!/usr/bin/env python3
"""Generate or verify the committed third-party notices from the locked Cargo graph."""

from __future__ import annotations

import argparse
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = ROOT / "THIRD_PARTY_NOTICES.md"
DEFAULT_CONFIG = ROOT / "quality" / "about.toml"
DEFAULT_TEMPLATE = ROOT / "quality" / "third_party_notices.hbs"


def fail(message: str) -> None:
    print(f"notices-check: {message}", file=sys.stderr)
    raise SystemExit(1)


def render(output: Path, config: Path, template: Path) -> None:
    for path in (ROOT / "Cargo.lock", config, template):
        if not path.is_file():
            fail(f"required input is missing: {path.relative_to(ROOT)}")
    if shutil.which("cargo-about") is None:
        fail("cargo-about is unavailable; run make bootstrap-tools")
    command = [
        "cargo",
        "about",
        "generate",
        "--locked",
        "--workspace",
        "--fail",
        "--config",
        str(config),
        "--output-file",
        str(output),
        str(template),
    ]
    print("+", " ".join(command), flush=True)
    completed = subprocess.run(command, cwd=ROOT)
    if completed.returncode != 0:
        fail("cargo-about could not generate complete third-party notices")
    text = output.read_text(encoding="utf-8")
    output.write_text(text.rstrip() + "\n", encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    parser.add_argument("--template", type=Path, default=DEFAULT_TEMPLATE)
    arguments = parser.parse_args()

    output = arguments.output.resolve()
    config = arguments.config.resolve()
    template = arguments.template.resolve()
    if arguments.check:
        if not output.is_file():
            fail(f"generated notice file is missing: {output.relative_to(ROOT)}")
        with tempfile.TemporaryDirectory() as temporary:
            regenerated = Path(temporary) / output.name
            render(regenerated, config, template)
            if regenerated.read_bytes() != output.read_bytes():
                fail(
                    f"{output.relative_to(ROOT)} is stale; run make notices and commit the result"
                )
        print("notices-check: committed third-party notices match the locked dependency graph")
        return

    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=output.parent) as temporary:
        regenerated = Path(temporary) / output.name
        render(regenerated, config, template)
        if not output.exists() or regenerated.read_bytes() != output.read_bytes():
            regenerated.replace(output)
            print(f"notices: wrote {output.relative_to(ROOT)}")
        else:
            print(f"notices: {output.relative_to(ROOT)} is already current")


if __name__ == "__main__":
    main()
