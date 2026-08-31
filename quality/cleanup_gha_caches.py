#!/usr/bin/env python3
"""Delete stale GitHub Actions caches to keep the repository under the cache limit.

GitHub Actions caches are scoped to branches and merge refs. Caches written
for pull requests (`refs/pull/<n>/merge`) are never removed automatically
after the PR is merged or closed, and every push to `main` writes a fresh
sccache/rust-cache set under a new key, so stale entries accumulate until the
repository cache limit (10 GB) is reached and new cache writes start failing.

This script deletes every cache that should be removed:

  * every cache whose ref is not the kept branch (default `refs/heads/main`),
    which removes all pull-request and feature-branch caches; and
  * caches of the kept branch older than `--max-age-days` (default 7).

Deletion is always by cache id (`DELETE /actions/caches/{cache_id}`): the
bulk by-ref endpoint requires a `key` query parameter that `gh api` cannot
pass for DELETE requests, so per-id deletion is the reliable path.

It is intended to run after a merge (a push to the default branch) and on a
weekly schedule so stale entries never accumulate. It is observational with
respect to the quality gate: it never runs inside the Quality gate and its
failures are reported but never block a merge.

Exit status is always 0 so a transient API failure cannot break the caller;
every error is printed to stderr and counted. `--dry-run` lists what would be
deleted without deleting anything. Authentication uses `GH_TOKEN` or
`GITHUB_TOKEN` when present, otherwise the ambient `gh` login.
"""

from __future__ import annotations

import argparse
import datetime
import json
import os
import subprocess
import sys

PAGE_SIZE = 100


def _gh(args: list[str]) -> subprocess.CompletedProcess:
    """Run `gh api` with the ambient gh auth (or GH_TOKEN when set)."""
    env = dict(os.environ)
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if token:
        env["GH_TOKEN"] = token
    return subprocess.run(
        ["gh", "api", *args],
        capture_output=True,
        text=True,
        env=env,
        check=False,
    )


def list_caches(repo: str) -> list[dict]:
    """Return every cache entry.

    `gh api --paginate` walks all pages itself and prints one JSON object per
    line (the jq expression emits one line per entry), so a single invocation
    with per_page=100 yields the complete list.
    """
    completed = _gh(
        [
            f"/repos/{repo}/actions/caches?per_page={PAGE_SIZE}",
            "--paginate",
            "--jq",
            ".actions_caches[] | {id, ref, key, created_at, last_used_at, size_in_bytes}",
        ]
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"listing caches failed: {completed.stderr.strip() or completed.stdout.strip()}"
        )
    entries = [
        json.loads(line)
        for line in completed.stdout.splitlines()
        if line.strip()
    ]
    if not entries:
        raise RuntimeError("cache listing returned no entries")
    return entries


def delete_by_id(repo: str, cache_id: int) -> None:
    completed = _gh([f"/repos/{repo}/actions/caches/{cache_id}", "-X", "DELETE"])
    if completed.returncode != 0:
        raise RuntimeError(
            f"deleting cache {cache_id} failed: "
            f"{completed.stderr.strip() or completed.stdout.strip()}"
        )


def _age_days(created_at: str, now: datetime.datetime) -> float:
    parsed = datetime.datetime.fromisoformat(created_at.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=datetime.timezone.utc)
    return max(0.0, (now - parsed).total_seconds() / 86400.0)


def should_delete(
    ref: str,
    created_at: str,
    keep_ref: str,
    max_age_days: int,
    now: datetime.datetime | None = None,
) -> bool:
    """Decide whether a cache entry should be deleted.

    Non-kept refs (pull requests and feature branches) are always deleted;
    kept-ref caches are deleted only when older than `max_age_days`.
    """
    if ref != keep_ref:
        return True
    return _age_days(created_at, now or datetime.datetime.now(datetime.timezone.utc)) > max_age_days


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Delete stale GitHub Actions caches."
    )
    parser.add_argument(
        "--repo",
        default=os.environ.get("GITHUB_REPOSITORY", "y0tsy/intention-relay"),
        help="owner/repository (default: GITHUB_REPOSITORY or y0tsy/intention-relay)",
    )
    parser.add_argument(
        "--keep-ref",
        default="refs/heads/main",
        help="branch whose caches are kept (default: refs/heads/main)",
    )
    parser.add_argument(
        "--max-age-days",
        type=int,
        default=7,
        help="delete kept-ref caches older than this many days (default: 7)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="list what would be deleted without deleting anything",
    )
    arguments = parser.parse_args()

    try:
        caches = list_caches(arguments.repo)
    except Exception as error:
        print(f"cleanup-gha-caches: listing failed: {error}", file=sys.stderr)
        return

    now = datetime.datetime.now(datetime.timezone.utc)
    to_delete = [
        cache
        for cache in caches
        if should_delete(
            cache["ref"],
            cache["created_at"],
            arguments.keep_ref,
            arguments.max_age_days,
            now,
        )
    ]
    kept = [cache for cache in caches if cache not in to_delete]
    deleted_bytes = sum(cache["size_in_bytes"] for cache in to_delete)
    kept_bytes = sum(cache["size_in_bytes"] for cache in kept)

    print(
        f"cleanup-gha-caches: {len(caches)} total, {len(to_delete)} to delete "
        f"({round(deleted_bytes / 1e6, 1)} MB), {len(kept)} kept "
        f"({round(kept_bytes / 1e6, 1)} MB)"
    )

    if arguments.dry_run:
        for cache in sorted(to_delete, key=lambda c: c["ref"]):
            print(
                f"  would delete {cache['id']} {cache['ref']} "
                f"{cache['created_at']} {cache['size_in_bytes']} bytes"
            )
        return

    failures = 0
    for cache in sorted(to_delete, key=lambda c: (c["ref"], c["created_at"])):
        try:
            delete_by_id(arguments.repo, cache["id"])
        except Exception as error:
            failures += 1
            print(f"cleanup-gha-caches: {error}", file=sys.stderr)

    if failures:
        print(f"cleanup-gha-caches: {failures} deletion failures", file=sys.stderr)
    else:
        print("cleanup-gha-caches: done")


if __name__ == "__main__":
    main()
