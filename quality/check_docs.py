#!/usr/bin/env python3
"""Validate documentation links, Mermaid fences, documented identifiers, and obvious secret assignments."""

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
# The session live stream replaced the run-scoped subscription, so these wire
# names are dead spellings: a document or a crate that still carries one names a
# value no DTO declares. The list is curated and matched as a whole identifier,
# because a local name that merely contains a spelling is not a wire value.
REMOVED_WIRE_NAMES = ("run.frame", "RunFrameDto", "run_frame")
# A documented placeholder constant is an underscore-joined `SCREAMING_CASE`
# name; the trailing `*` form documents the prefix its constants share.
PLACEHOLDER_CONSTANT = re.compile(r"^[A-Z][A-Z0-9_]*_[A-Z0-9_]*\*?$")
BACKTICKED_TOKEN = re.compile(r"`([^`\n]+)`")
SENTENCE_BOUNDARY = re.compile(r"(?<=[.!?])\s+")
PLACEHOLDER_CONVENTION_MARKER = "@todo(core)"


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


def prose_sentences(text: str) -> list[str]:
    """Split a Markdown document into prose sentences, joining the lines a sentence wraps over.

    Table rows and fenced code are not prose, and each list item is its own
    paragraph: a cell, a code line, or a neighbouring bullet is not part of the
    sentence that documents a convention.
    """
    sentences: list[str] = []
    paragraph: list[str] = []
    fenced = False

    def flush() -> None:
        if paragraph:
            sentences.extend(SENTENCE_BOUNDARY.split(" ".join(paragraph)))
            paragraph.clear()

    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("```"):
            fenced = not fenced
            flush()
            continue
        if fenced or not stripped or stripped.startswith(("|", "- ", "* ")):
            flush()
            continue
        paragraph.append(stripped)
    flush()
    return sentences

def placeholder_defined(token: str, crate_text: str) -> bool:
    """Return whether `crates/` declares the placeholder constant a document names.

    A `NAME_*` token documents the prefix its constants share, so any identifier
    with that prefix satisfies it; a plain token must exist whole.
    """
    if token.endswith("*"):
        pattern = rf"(?<![A-Za-z0-9_]){re.escape(token[:-1])}[A-Za-z0-9_]+"
    else:
        pattern = rf"(?<![A-Za-z0-9_]){re.escape(token)}(?![A-Za-z0-9_])"
    return re.search(pattern, crate_text) is not None

def placeholder_convention_failures(documents: dict[str, str], crate_text: str) -> list[str]:
    """Return failures for a documented placeholder constant that no crate defines.

    A sentence that documents the `@todo(core)` convention is evidence about the
    code, so naming a constant that exists nowhere under `crates/` makes the
    documented convention a phantom and the sentence must resolve.
    """
    failures: list[str] = []
    for path, text in documents.items():
        for sentence in prose_sentences(text):
            if PLACEHOLDER_CONVENTION_MARKER not in sentence:
                continue
            for token in BACKTICKED_TOKEN.findall(sentence):
                if not PLACEHOLDER_CONSTANT.match(token):
                    continue
                if not placeholder_defined(token, crate_text):
                    failures.append(
                        f"{path}: documented placeholder {token!r} does not exist in crates/"
                    )
    return failures

def removed_wire_name_failures(texts: dict[str, str]) -> list[str]:
    """Return failures for a removed wire name that is still spelled in a checked tree."""
    patterns = [
        (name, re.compile(rf"(?<![A-Za-z0-9_.]){re.escape(name)}(?![A-Za-z0-9_])"))
        for name in REMOVED_WIRE_NAMES
    ]
    failures: list[str] = []
    for path, text in texts.items():
        for name, pattern in patterns:
            if pattern.search(text):
                failures.append(f"{path}: removed wire name {name!r} is still spelled")
    return failures

def crate_source_text(root: Path) -> str:
    """Return every Rust source under `crates/`: the record a documented identifier must exist in."""
    crates = root / "crates"
    if not crates.exists():
        return ""
    return "\n".join(path.read_text(encoding="utf-8") for path in sorted(crates.rglob("*.rs")))

def convention_documents(root: Path) -> dict[str, str]:
    """Return the Markdown documents a convention sentence may live in.

    The placeholder convention is documented in the crate READMEs, which the
    link and bound checks do not read.
    """
    paths = [*documentation_files(root), *(root / "crates").rglob("README.md")]
    return {
        str(path.relative_to(root)): path.read_text(encoding="utf-8")
        for path in sorted(paths)
    }

def checked_texts(root: Path) -> dict[str, str]:
    """Return the readable text of every file under `docs/` and `crates/`."""
    texts: dict[str, str] = {}
    for base in (root / "docs", root / "crates"):
        if not base.exists():
            continue
        for path in sorted(base.rglob("*")):
            if not path.is_file():
                continue
            try:
                texts[str(path.relative_to(root))] = path.read_text(encoding="utf-8")
            except UnicodeDecodeError:
                continue
    return texts

def self_test_removed_wire_names() -> None:
    """Proves the removed-wire-name check can fail before it is trusted."""
    live = {"crates/intention-proto/src/protocol.rs": "pub enum SessionFrameDto {"}
    if removed_wire_name_failures(live):
        fail("removed-wire-name self-test: the live session-frame spelling must pass")
    local = {"crates/intention-daemon/tests/real_api_e2e.rs": "async fn collect_run_frames() {}"}
    if removed_wire_name_failures(local):
        fail("removed-wire-name self-test: a local name that only contains a spelling must pass")
    stale = {"docs/example.md": "later committed values arrive as live `run.frame` notifications"}
    if not removed_wire_name_failures(stale):
        fail("removed-wire-name self-test: the removed run-scoped spelling must fail")

def self_test_placeholder_conventions() -> None:
    """Proves the placeholder-convention check can fail before it is trusted."""
    crate_text = 'const EXEC_REASON: &str = "@todo(core): the summary carries no execute fact";'
    documented = {
        "crates/intention-tui/README.md": (
            "A value the core does not carry yet is an explicit `@todo(core)` plate named by `EXEC_REASON`."
        )
    }
    if placeholder_convention_failures(documented, crate_text):
        fail("placeholder-convention self-test: a constant the crate defines must pass")
    phantom = {
        "crates/intention-tui/README.md": (
            "A value the core does not carry yet is a `MISSING_*` placeholder with a `// @todo(core)` comment."
        )
    }
    if not placeholder_convention_failures(phantom, crate_text):
        fail("placeholder-convention self-test: a constant no crate defines must fail")
    prose = {"docs/example.md": "The `MAX_ROWS` bound is documented here without the marker."}
    if placeholder_convention_failures(prose, crate_text):
        fail("placeholder-convention self-test: prose outside the convention sentence must pass")

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
    failures.extend(removed_wire_name_failures(checked_texts(root)))
    failures.extend(
        placeholder_convention_failures(convention_documents(root), crate_source_text(root))
    )
    failures.extend(check_secrets(root))
    # The two documentation rules are evidence only if they can fail, so each
    # runs its own passing and failing inputs before the report.
    self_test_removed_wire_names()
    self_test_placeholder_conventions()
    if failures:
        fail("\n".join(failures))
    print(
        "docs-check: Markdown links, code fences, Mermaid headers, documented bounds, "
        "removed wire names, placeholder conventions, and secret assignments are valid"
    )


if __name__ == "__main__":
    main()
