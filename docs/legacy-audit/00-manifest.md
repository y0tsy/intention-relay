# Audit Manifest

## Purpose

This external audit records only capabilities that are statically reachable from the active Antibusy frontend to a backend effect and a user-visible result. It is a product baseline for a clean rewrite and later naming work, not an architectural migration plan.

## Inclusion rule

A capability is included only when this route is evidenced:

```text
Visible frontend entry -> active handler -> service/store -> gateway/adapter -> Tauri IPC/event/channel -> Rust effect -> visible result
```

Internal code, configuration fields, tools, or backend services without a proven active frontend route are excluded from the capability catalogue. They may appear only in `05-excluded-or-unproven.md`.

## Evidence standard

Static source-path evidence only. This audit does not claim GUI runtime verification.

## Audited source

- Repository: `/home/data/antibusy`
- Revision: `8604fde0566d4dfadf8124e0724c5a82df3b89de`
- Branch: `refactor/phase-blockers-acceptance`
- Source areas: `crates/tauri-app/frontend`, `crates/tauri-app`, `crates/core`, `crates/tools`, `crates/config`, `crates/compression`, `crates/vfr`, `crates/prompt`, `crates/mcp`, `crates/skills`, `crates/hooks`

## Repository state at audit start

The source repository had pre-existing user changes. This audit writes only to `/home/data/antibusy-audit/` and must not modify the repository.

## Artifact index

- `01-frontend-surface.csv`: all identified visible frontend entries and classification.
- `02-capability-catalog.md`: proved user capabilities.
- `03-evidence-registry.json`: canonical source-path evidence.
- `04-agent-behavior.md`: user-visible behavior shaped by internal mechanisms.
- `05-excluded-or-unproven.md`: non-included and incomplete paths.
- `06-user-flows.md`: end-to-end flow diagrams.
- `07-naming-brief.md`: verified product definition for naming.
- `08-audit-gaps.md`: limitations and remaining validation work.
- `evidence/`: scoped evidence reports.
