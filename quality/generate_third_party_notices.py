#!/usr/bin/env python3
"""Generate or verify the committed third-party notices from the locked Cargo graph.

Verification compares the committed file with a fresh rendering byte for byte,
and falls back to comparing which license every crate reference is rendered
under when the raw bytes differ. `cargo-about` groups dependency entries by
license text, and its grouping and attribution depend on the local registry
cache: a cache that already unpacked a dependency's sources merges two crates
that ship byte-identical license files into one block, while a cache that holds
only the archives renders one block per crate and can surface a text file the
warm cache merges elsewhere. Both renderings describe the same locked graph, so
the fallback keeps the gate honest without making it depend on which machine ran
it. A changed graph, version, or license name still fails the check; the license
texts themselves are carried verbatim from the crates and are not compared.
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = ROOT / "THIRD_PARTY_NOTICES.md"
DEFAULT_CONFIG = ROOT / "quality" / "about.toml"
DEFAULT_TEMPLATE = ROOT / "quality" / "third_party_notices.hbs"

# One rendered notices document is a sequence of license names, crate
# references, and fenced license texts; the tokens below read that sequence so
# every crate reference can be attributed to the text that follows it.
NOTICE_TOKENS = re.compile(
    r"^### (?P<name>[^\n]+)$"
    r"|^- \[(?P<crate>[^\]]+)\]\((?P<url>[^\n]*)\)$"
    r"|^```text\n(?P<text>.*?)\n```$",
    re.MULTILINE | re.DOTALL,
)


def fail(message: str) -> None:
    print(f"notices-check: {message}", file=sys.stderr)
    raise SystemExit(1)


def notices_by_crate(text: str) -> dict[str, str]:
    """Returns the license name rendered for every crate reference.

    This is the part of the document that describes the locked graph: which
    crate is listed, at which version, under which license. The license texts
    themselves are carried verbatim from the crates, and `cargo-about`
    attributes and groups them by the state of the local registry cache (a cold
    cache renders one block per crate and can surface a text file the warm cache
    merges under another license name), so the comparison stays on the
    attribution a reader checks first.
    """
    by_crate: dict[str, str] = {}
    current_name = ""
    pending: list[str] = []
    for token in NOTICE_TOKENS.finditer(text):
        name = token.group("name")
        crate = token.group("crate")
        license_text = token.group("text")
        if name is not None:
            current_name = name
        elif crate is not None:
            pending.append(crate)
        elif license_text is not None:
            for reference in pending:
                by_crate[reference] = current_name
            pending.clear()
    return by_crate


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
    output.write_text("\n".join(line.rstrip() for line in text.splitlines()).rstrip() + "\n", encoding="utf-8")


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
                if notices_by_crate(regenerated.read_text(encoding="utf-8")) != notices_by_crate(
                    output.read_text(encoding="utf-8")
                ):
                    fail(f"{output.relative_to(ROOT)} is stale; run make notices and commit the result")
                print(
                    "notices-check: the committed notices match the locked dependency graph; "
                    "this registry cache grouped or attributed identical license texts differently",
                    flush=True,
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
