#!/usr/bin/env python3
"""Validate documentation links, Mermaid fences, and obvious secret assignments."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
SUPPORTED_MERMAID = {"flowchart", "graph", "stateDiagram", "sequenceDiagram", "classDiagram", "erDiagram", "xychart-beta"}
SECRET_ASSIGNMENT = re.compile(
    r"(?i)\b(?:api[_-]?key|secret|token|password)\s*[:=]\s*['\"][A-Za-z0-9_\-]{8,}['\"]"
)
# The root documents are checked for links exactly like `docs/`.
ROOT_DOCUMENTS = ("README.md", "AGENTS.md", "THIRD_PARTY_NOTICES.md")
CURRENT_POLICY_MARKER = "**Current policy.**"
BOUND_TOKEN = re.compile(r"\b(?:MAX|MIN)_[A-Z0-9_]+\b")


def fail(message: str) -> None:
    print(f"docs-check: {message}", file=sys.stderr)
    raise SystemExit(1)


def markdown_files(root: Path) -> list[Path]:
    return sorted(root.rglob("*.md"))


def documentation_files(root: Path) -> list[Path]:
    """Return every checked Markdown document: `docs/` plus the root documents."""
    documents = markdown_files(root / "docs") if (root / "docs").exists() else []
    for name in ROOT_DOCUMENTS:
        path = root / name
        if path.exists():
            documents.append(path)
    return documents


def source_files(root: Path) -> list[Path]:
    ignored = {".git", "target", "fixtures", "__pycache__"}
    suffixes = {".rs", ".py", ".toml", ".yml", ".yaml", ".mk"}
    return sorted(
        path
        for path in root.rglob("*")
        if path.is_file()
        and (path.suffix in suffixes or path.name == "Makefile")
        and not any(part in ignored for part in path.relative_to(root).parts)
    )


def check_markdown(root: Path) -> list[str]:
    failures: list[str] = []
    link_pattern = re.compile(r"(?<!!)\[[^\]]+\]\(([^)]+)\)")
    for path in documentation_files(root):
        text = path.read_text(encoding="utf-8")
        fences = re.findall(r"^```([^\n]*)$", text, flags=re.MULTILINE)
        if len(fences) % 2:
            failures.append(f"{path}: unbalanced code fences")
        lines = text.splitlines()
        for index, line in enumerate(lines):
            if line == "```mermaid":
                diagram = next((candidate.strip() for candidate in lines[index + 1 :] if candidate.strip()), "")
                if not any(diagram.startswith(kind) for kind in SUPPORTED_MERMAID):
                    failures.append(f"{path}:{index + 1}: unsupported Mermaid diagram")
        for match in link_pattern.finditer(text):
            target = match.group(1).split("#", maxsplit=1)[0]
            if not target or "://" in target or target.startswith("mailto:"):
                continue
            if not (path.parent / target).resolve().exists():
                failures.append(f"{path}: unresolved Markdown link {target!r}")
    return failures


def check_documented_bounds(root: Path) -> list[str]:
    """Return failures for a documented `MAX_`/`MIN_` bound no crate defines.

    A current-policy document is the recorded limits-by-precedent rationale, so
    a bound it names must exist in `crates/`; a phantom constant would
    invalidate the policy record. Future-design documents carry no current
    policy marker and stay outside this check.
    """
    crates = root / "crates"
    if not crates.exists():
        return []
    crate_text = "\n".join(
        path.read_text(encoding="utf-8") for path in sorted(crates.rglob("*.rs"))
    )
    failures: list[str] = []
    for path in documentation_files(root):
        text = path.read_text(encoding="utf-8")
        if CURRENT_POLICY_MARKER not in text:
            continue
        for name in sorted(set(BOUND_TOKEN.findall(text))):
            if name not in crate_text:
                failures.append(f"{path}: documented bound {name} does not exist in crates/")
    return failures


def check_secrets(root: Path) -> list[str]:
    failures: list[str] = []
    for path in source_files(root):
        text = path.read_text(encoding="utf-8")
        if SECRET_ASSIGNMENT.search(text):
            failures.append(f"{path}: secret-like assignment is forbidden")
    return failures


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    arguments = parser.parse_args()
    root = arguments.root.resolve()
    failures = check_markdown(root)
    failures.extend(check_documented_bounds(root))
    failures.extend(check_secrets(root))
    if failures:
        fail("\n".join(failures))
    print(
        "docs-check: Markdown links, code fences, Mermaid headers, documented bounds, "
        "and secret assignments are valid"
    )


if __name__ == "__main__":
    main()
