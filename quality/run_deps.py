#!/usr/bin/env python3
"""Run independent dependency gates with a bounded worker pool."""

from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor, as_completed
import sys
import tempfile
import tomllib
from pathlib import Path

try:
    from timing import run_command
except ImportError:  # pragma: no cover - package/module invocation compatibility
    from quality.timing import run_command

ROOT = Path(__file__).resolve().parents[1]
MAX_WORKERS = 2


def run_check(label: str, command: list[str], cwd: Path = ROOT) -> tuple[str, int, str]:
    completed = run_command(
        command,
        cwd=cwd,
        phase="deps",
        gate=label,
        stage="dependency-check",
        capture_output=True,
    )
    output = (completed.stdout + completed.stderr).strip()
    return label, completed.returncode, output


def without_patch_tables(manifest: str) -> str:
    """Return one manifest text without its `[patch.*]` tables."""
    kept: list[str] = []
    skipping = False
    for line in manifest.splitlines(keepends=True):
        if line.lstrip().startswith("["):
            skipping = line.lstrip().startswith("[patch")
        if not skipping:
            kept.append(line)
    return "".join(kept)


def outdated_workspace(destination: Path) -> Path:
    """Return a workspace copy for `cargo outdated` that carries no path patch.

    `cargo outdated` copies every manifest it learns about into a temporary
    project of its own and keeps the root `[patch.crates-io]` entry verbatim, so
    a path-patched vendored dependency cannot be resolved there. The copy keeps
    the real member directories as symlinks and drops only the patch tables, so
    the tool still resolves the graph the pinned registry versions describe.
    """
    manifest = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    document = tomllib.loads(manifest)
    (destination / "Cargo.toml").write_text(without_patch_tables(manifest), encoding="utf-8")
    (destination / "Cargo.lock").write_text(
        (ROOT / "Cargo.lock").read_text(encoding="utf-8"), encoding="utf-8"
    )
    toolchain = ROOT / "rust-toolchain.toml"
    if toolchain.exists():
        (destination / "rust-toolchain.toml").write_text(
            toolchain.read_text(encoding="utf-8"), encoding="utf-8"
        )
    for member in document["workspace"]["members"]:
        link = destination / member
        link.parent.mkdir(parents=True, exist_ok=True)
        link.symlink_to(ROOT / member, target_is_directory=True)
    return destination


def main() -> int:
    python = sys.executable
    # Metadata and notices are prerequisites; the independent checks follow.
    prerequisites = [
        ("metadata", ["cargo", "metadata", "--locked", "--format-version", "1"]),
        ("deny-policy", [python, "quality/check_deny_policy.py"]),
    ]
    for label, command in prerequisites:
        result = run_check(label, command)
        if result[1] != 0:
            print(f"deps: {label} failed\n{result[2]}", file=sys.stderr)
            return result[1]

    ignored = ",".join(tomllib.loads((ROOT / "quality/outdated.toml").read_text(encoding="utf-8"))["outdated_ignores"]["crates"])
    outdated = ["cargo", "outdated", "--workspace", "--root-deps-only", "--exit-code", "1"]
    if ignored:
        outdated += ["--ignore", ignored]
    checks = [
        ("deny", ["cargo", "deny", "check"], ROOT),
        ("audit", ["cargo", "audit"], ROOT),
        ("machete", ["cargo", "machete", "--with-metadata", "--skip-target-dir"], ROOT),
    ]
    failures: list[tuple[str, int, str]] = []
    # The outdated check runs against a patch-free copy of the manifests,
    # because a path patch cannot be resolved in the temporary project
    # `cargo outdated` builds for itself.
    with tempfile.TemporaryDirectory(prefix="intention-relay-outdated-") as temporary:
        checks.append(("outdated", outdated, outdated_workspace(Path(temporary))))
        with ThreadPoolExecutor(max_workers=MAX_WORKERS) as executor:
            futures = [
                executor.submit(run_check, label, command, cwd)
                for label, command, cwd in checks
            ]
            for future in as_completed(futures):
                result = future.result()
                if result[1] != 0:
                    failures.append(result)
    udeps = run_check("udeps", [python, "quality/run_profiles.py", "udeps"])
    if udeps[1] != 0:
        failures.append(udeps)
    if failures:
        for label, code, output in sorted(failures):
            print(f"deps: {label} failed (exit {code})\n{output}", file=sys.stderr)
        return failures[0][1]
    print("deps: all dependency checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
