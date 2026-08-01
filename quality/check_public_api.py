#!/usr/bin/env python3
"""Reject forbidden implementation types from public active-crate APIs."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_POLICY = ROOT / "quality" / "architecture.toml"
TOOLS_POLICY = ROOT / "quality" / "tools.toml"


def fail(message: str) -> None:
    print(f"public-api-check: {message}", file=sys.stderr)
    raise SystemExit(1)


def load_toml(path: Path) -> dict[str, object]:
    with path.open("rb") as policy_file:
        return tomllib.load(policy_file)


def string_list(table: dict[str, object], field: str) -> list[str]:
    value = table.get(field)
    if not isinstance(value, list) or not all(isinstance(item, str) and item for item in value):
        fail(f"policy field {field} must be a non-empty string list")
    return value


def rustdoc_json(root: Path, toolchain: str, package: str) -> dict[str, object]:
    command = [
        "cargo",
        f"+{toolchain}",
        "rustdoc",
        "--package",
        package,
        "--lib",
        "--locked",
        "-Z",
        "unstable-options",
        "--output-format",
        "json",
    ]
    try:
        subprocess.run(command, cwd=root, check=True)
    except subprocess.CalledProcessError as error:
        fail(f"failed to generate rustdoc JSON for {package}: {error}")
    artifact = root / "target" / "doc" / f"{package.replace('-', '_')}.json"
    if not artifact.is_file():
        fail(f"rustdoc JSON artifact is missing for {package}: {artifact}")
    with artifact.open(encoding="utf-8") as artifact_file:
        document = json.load(artifact_file)
    if not isinstance(document, dict):
        fail(f"rustdoc JSON for {package} must be an object")
    return document


def child_item_ids(item: dict[str, object]) -> list[object]:
    inner = item.get("inner")
    if not isinstance(inner, dict) or len(inner) != 1:
        return []
    kind, body = next(iter(inner.items()))
    if not isinstance(body, dict):
        return []
    if kind == "module":
        items = body.get("items", [])
        return items if isinstance(items, list) else []
    if kind in {"trait", "impl"}:
        items = body.get("items", [])
        return items if isinstance(items, list) else []
    if kind == "enum":
        variants = body.get("variants", [])
        return variants if isinstance(variants, list) else []
    if kind == "struct":
        structure = body.get("kind")
        if not isinstance(structure, dict) or len(structure) != 1:
            return []
        shape, fields = next(iter(structure.items()))
        if shape == "plain" and isinstance(fields, dict):
            values = fields.get("fields", [])
            return values if isinstance(values, list) else []
        if shape == "tuple" and isinstance(fields, list):
            return fields
    if kind == "variant":
        variant = body.get("kind")
        if not isinstance(variant, dict) or len(variant) != 1:
            return []
        shape, fields = next(iter(variant.items()))
        if shape == "plain" and isinstance(fields, dict):
            values = fields.get("fields", [])
            return values if isinstance(values, list) else []
        if shape == "tuple" and isinstance(fields, list):
            return fields
    return []


def resolved_paths(value: object) -> set[str]:
    paths: set[str] = set()

    def visit(current: object) -> None:
        if isinstance(current, dict):
            resolved = current.get("resolved_path")
            if isinstance(resolved, dict):
                path = resolved.get("path")
                if isinstance(path, str):
                    paths.add(path)
            use = current.get("use")
            if isinstance(use, dict):
                source = use.get("source")
                if isinstance(source, str):
                    paths.add(source)
            for nested in current.values():
                visit(nested)
        elif isinstance(current, list):
            for nested in current:
                visit(nested)

    visit(value)
    return paths


def public_items(document: dict[str, object]) -> list[tuple[dict[str, object], str]]:
    index = document.get("index")
    root = document.get("root")
    if not isinstance(index, dict) or root is None:
        fail("rustdoc JSON lacks index or root")
    root_item = index.get(str(root))
    if not isinstance(root_item, dict):
        fail("rustdoc JSON root item is missing")

    result: list[tuple[dict[str, object], str]] = []
    visited: set[str] = set()

    def visit(item_id: object, parent_public: bool, parent_label: str) -> None:
        key = str(item_id)
        if key in visited:
            return
        visited.add(key)
        item = index.get(key)
        if not isinstance(item, dict):
            return
        is_public = parent_public and item.get("visibility") == "public"
        if not is_public:
            return
        inner = item.get("inner")
        kind = next(iter(inner), "") if isinstance(inner, dict) else ""
        label = parent_label if kind == "struct_field" else item_label(item)
        result.append((item, label))
        for child_id in child_item_ids(item):
            visit(child_id, True, label)

    root_label = item_label(root_item)
    result.append((root_item, root_label))
    for child_id in child_item_ids(root_item):
        visit(child_id, True, root_label)
    return result


def item_label(item: dict[str, object]) -> str:
    name = item.get("name")
    if isinstance(name, str) and name:
        return name
    inner = item.get("inner")
    if isinstance(inner, dict) and "use" in inner:
        use = inner["use"]
        if isinstance(use, dict) and isinstance(use.get("name"), str):
            return f"re-export {use['name']}"
    return "anonymous public item"


def canonical_path(path: str) -> str:
    if path.startswith("fs::"):
        return f"std::{path}"
    if path.startswith("path::") or path.startswith("process::"):
        return f"std::{path}"
    return path


def check_package(package: str, document: dict[str, object], forbidden_prefixes: list[str]) -> list[str]:
    failures: list[str] = []
    for item, label in public_items(document):
        inner = item.get("inner")
        for raw_path in sorted(resolved_paths(inner)):
            path = canonical_path(raw_path)
            for prefix in forbidden_prefixes:
                if path == prefix or path.startswith(prefix):
                    failures.append(
                        f"{package}: public API item {label!r} exposes forbidden type {path!r}"
                    )
                    break
    return failures


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    parser.add_argument("--tools-policy", type=Path, default=TOOLS_POLICY)
    arguments = parser.parse_args()

    root = arguments.root.resolve()
    policy = load_toml(arguments.policy)
    tools = load_toml(arguments.tools_policy)
    state = policy.get("policy")
    public_contracts = policy.get("public_contracts")
    toolchains = tools.get("toolchains")
    if not isinstance(state, dict) or not isinstance(public_contracts, dict) or not isinstance(toolchains, dict):
        fail("architecture and tools policies require policy, public_contracts, and toolchains tables")
    active = string_list(state, "active_production_crates")
    forbidden_prefixes = string_list(public_contracts, "forbidden_type_prefixes")
    nightly = toolchains.get("nightly")
    if not isinstance(nightly, str) or not nightly:
        fail("tools policy requires a nightly toolchain")

    failures: list[str] = []
    for package in active:
        failures.extend(check_package(package, rustdoc_json(root, nightly, package), forbidden_prefixes))
    if failures:
        fail("\n".join(failures))
    print("public-api-check: active public contracts do not expose forbidden implementation types")


if __name__ == "__main__":
    main()
