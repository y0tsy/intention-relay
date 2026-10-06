# Intention Relay Agent Instructions

## Authority and context

- [`docs/intention-relay/`](docs/intention-relay/README.md) is the authoritative source for product, architecture, quality, and roadmap context.
- Before work that affects milestones, phases, architecture, crate boundaries, quality policy, or workflow behavior, read the relevant documents in [`docs/intention-relay/architecture/`](docs/intention-relay/architecture/README.md) completely enough to understand the applicable context.
- In particular, consult the [quality-gate policy](docs/intention-relay/architecture/12-quality-gates-and-makefile.md), [TDD/TTD policy](docs/intention-relay/architecture/10-test-driven-delivery-and-verification.md), and [implementation roadmap](docs/intention-relay/architecture/11-implementation-roadmap.md) when they apply.

## Engineering practice

### Think before coding

- Read the relevant code, tests, configuration, and call sites before changing anything.
- State important assumptions explicitly. If a requirement is genuinely unclear or has multiple materially different interpretations, ask before implementing.
- Surface relevant trade-offs and mention a simpler approach when one exists.
- Do not silently guess about requirements, compatibility, or intended behavior.
- Confirm that crates, modules, and subsystems referenced by prior context or documentation still exist in the current source tree before relying on them.

### Simplicity and surgical changes

- Implement the minimum change that satisfies the request.
- Do not add speculative features, abstractions, configuration, or error handling for impossible scenarios.
- Learn applicable Rust types, APIs, and patterns from their existing consumers and foundational definitions before introducing a new local mechanism.
- Touch only files and lines needed for the request.
- Match the surrounding style and established patterns, even when another approach seems preferable.
- Do not refactor unrelated code, comments, formatting, or pre-existing dead code.
- Remove imports, variables, functions, or tests only when the current change makes them unused or obsolete.
- Fix issues introduced by the current change, but do not use a task as an excuse for unrelated cleanup.
- Never add speculative limits, quotas, caps, or bureaucratic bounds without a real, demonstrated precedent. Liveness safeguards are the only exception.

### No backward compatibility

- Backward compatibility is neither required nor in demand. Nothing built or run from this project exists outside the development machine: no deployed users, no externally persisted data, and no third-party consumers.
- When an execution path becomes outdated, remove it. Do not add compatibility layers, fallback branches, or migration paths to keep old behavior readable, replayable, or upgradeable.
- Until every roadmap milestone is complete and fully closed, each versioned system keeps exactly one live version — version 1:
  - Database schemas: exactly one schema version, version 1. Tables, columns, and indexes are removed, updated, and added only within that version. There are no migrations, no versioned upgrade steps, and no opening of older schema versions.
  - The same single-version rule applies to protocol versions, wire formats, configuration formats, storage formats, and any other versioned system: evolve in place, never carry two versions at once.
- Do not write compatibility fixtures, golden files, or tests that assert behavior of any version other than the current one.
- When this policy conflicts with architecture documents or ADRs that require migration or preservation behavior, update those documents in the same change that removes the obsolete path.

### Goal-driven delivery

- Define observable success criteria before implementation.
- For bug fixes, add a focused regression test when practical.
- For new behavior, add or update focused tests where appropriate.
- For multi-step work, state a brief plan and verify each step.
- Continue until the requested behavior is implemented and the relevant checks pass.

## Parallel tool use

- Treat parallel tool calls as the target for broad or multi-surface work.
- For most such tasks, target at least five genuinely independent useful calls. Target 10–15 only when that many independent checks materially improve speed or coverage.
- Do not create no-op calls, duplicate calls, or parallelize actions that depend on earlier results.

## Sub-agent discipline

- Do not start sub-agents when work is sequential, narrow, or can be completed directly without material benefit.
- Use sub-agents only when parallel exploration, specialized expertise, or isolated long-running work provides a concrete benefit.

## Test-first and architectural boundaries

- Follow TDD/TTD: establish applicable contract, architecture, and outcome tests before implementation.
- Every new production crate must be declared in the machine-readable policy with its responsibility and test target before production code is accepted; the crate's coverage tier applies ([ADR 0016](docs/intention-relay/decisions/0016-per-crate-coverage-tiers.md)).
- Never bypass `WorkspaceRoot`, DTO-first, or adapter-isolation rules.
- Follow existing data, configuration, and dependency patterns. Do not hard-code values that are genuinely configurable, and do not add configuration unless configurability is a requirement.
- Check the workspace `Cargo.toml` and existing dependency graph before adding a dependency.
- Do not hard-code POSIX-specific path syntax (for example, `/tmp/...` or `/workspace/...`) in production code, tests, fixtures, or configuration that executes on supported platforms. Construct filesystem paths with platform-native APIs such as `std::env::temp_dir()` and `Path`/`PathBuf`; use explicit canonical Windows drive and UNC literals only in Windows-path validation tests.

## Quality and documentation consistency

- Use the root Makefile as the quality orchestration interface. Run `make quick` while iterating and `make verify` before handing off implementation work.
- Changes to Makefile, linting, coverage, feature profiles, or supply-chain policy must update the relevant machine-readable policy and architecture documentation in the same change.

## Security and language

- Use English for code, comments, documentation, variable names, commit messages, and pull-request descriptions. Non-English text is allowed only when it is intentional test data, fixture data, or user-facing content being tested.
- Never commit or expose secrets. API keys, tokens, passwords, and other credentials must not appear in code, configuration, logs, fixtures, or commit history.
- Never scan or filter user, model, or tool content for credential-shaped strings at runtime. Content scanning is banned; secrets are protected structurally.

## Git discipline

- Check `git status` before editing. Preserve existing user changes and never overwrite, move, delete, or commit work you did not create without explicit authorization.
- Use atomic commits for completed logical units. Do not bundle unrelated changes or commit unfinished work merely to keep the working tree clean.
- Use Conventional Commit messages in the form `type(scope): description`.
- Prefer a new branch for non-trivial changes, including new features, behavior changes, multi-file refactors, or work spanning multiple logical concerns. Small isolated fixes may use the current branch when appropriate.
- Prefer atomic commits. Combine changes only when splitting them would harm coherence or make history misleading. State the technical reason in the commit body when relevant.

## GitHub labels

- The repository keeps exactly this label set: ten `type:` labels matching the Conventional Commit types (`type: feat`, `type: fix`, `type: docs`, `type: refactor`, `type: test`, `type: chore`, `type: perf`, `type: ci`, `type: build`, `type: revert`), five `area:` labels for repository surfaces (`area: architecture`, `area: decisions`, `area: quality`, `area: crates`, `area: ci`), and the process labels `dependencies`, `security`, `breaking-change`, and `blocked`. Nothing else.
- Every pull request and issue carries exactly one `type:` label matching the commit type of the change (`type: feat` for `feat(...)`, and so on).
- An `area:` label marks each surface the change touches; a change spanning surfaces carries each one.
- Dependabot applies `dependencies` and `security` to its pull requests; treat both as dependency work.
- Add `breaking-change` when the change removes or alters existing behavior under the no-backward-compatibility policy; the accompanying ADR or owner document is updated in the same change.
- Add `blocked` while an external dependency or a pending decision stops progress, and remove it when unblocked.
- Do not invent ad-hoc labels. Extend this list first, then create the label with `gh label create`.
- Labels are not a status tracker beyond `blocked`; milestone and slice state stays in `docs/intention-relay`.
