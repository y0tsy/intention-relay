#!/usr/bin/env python3
"""Enforce the per-crate line floors from one collected coverage report.

`cargo llvm-cov report` merges the whole profiling set on every invocation, so
asking it for one crate at a time pays that merge once per crate. The report the
coverage gate writes carries every file's line summary, and a crate's line
coverage is the covered-over-total ratio over its own files, so the floors are
checked here from that one artifact instead.

Floors arrive as `crate=percentage` entries, where the crate is the directory
name under `crates/`. A floor whose crate contributes no file to the report is a
failure rather than a skip, so a renamed or removed crate cannot drop out of the
gate silently.
"""

from __future__ import annotations

from pathlib import Path
import json
import sys

ROOT = Path(__file__).resolve().parents[1]


def fail(message: str) -> None:
    print(f"coverage-floors: {message}", file=sys.stderr)
    raise SystemExit(1)


def crate_lines(report: Path) -> dict[str, tuple[int, int]]:
    """Map each workspace member to its total and covered line counts."""
    try:
        document = json.loads(report.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"coverage report is unreadable: {report}: {error}")
    totals: dict[str, tuple[int, int]] = {}
    for entry in document["data"][0]["files"]:
        try:
            relative = Path(entry["filename"]).resolve().relative_to(ROOT).parts
        except ValueError:
            # A reported path outside the repository carries no floor.
            continue
        if len(relative) < 3 or relative[0] != "crates" or not relative[-1].endswith(".rs"):
            continue
        lines = entry["summary"]["lines"]
        total, covered = totals.get(relative[1], (0, 0))
        totals[relative[1]] = (total + lines["count"], covered + lines["covered"])
    return totals


def parse_floors(tokens: list[str]) -> list[tuple[str, float]]:
    floors: list[tuple[str, float]] = []
    for token in (token for argument in tokens for token in argument.split()):
        crate, _, floor = token.partition("=")
        if not crate or not floor:
            fail(f"coverage floor must be crate=percentage: {token}")
        floors.append((crate, float(floor)))
    if not floors:
        fail("no coverage floors were provided")
    return floors


def main() -> None:
    if len(sys.argv) < 3:
        fail("usage: check_coverage_floors.py <report.json> <crate=percentage>...")
    report = Path(sys.argv[1])
    floors = parse_floors(sys.argv[2:])
    totals = crate_lines(report)

    failures: list[str] = []
    print(f"{'crate':24}{'lines':>8}{'covered':>9}{'cover':>9}{'floor':>8}")
    for crate, floor in floors:
        if crate not in totals:
            failures.append(f"{crate}: no file in the coverage report")
            print(f"{crate:24}{'-':>8}{'-':>9}{'-':>9}{floor:8.2f}")
            continue
        total, covered = totals[crate]
        percent = 100 * covered / total if total else 0.0
        status = "ok"
        if percent < floor:
            status = "below floor"
            failures.append(f"{crate}: {percent:.2f}% is below the {floor:.2f}% floor")
        print(f"{crate:24}{total:8}{covered:9}{percent:8.2f}%{floor:8.2f} {status}")

    for failure in failures:
        print(f"coverage-floors: {failure}", file=sys.stderr)
    if failures:
        raise SystemExit(1)
    print(f"coverage-floors: {len(floors)} crate floors hold")


if __name__ == "__main__":
    main()
