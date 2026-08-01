# Intention Relay Agent Instructions

## Authority and context

- [`docs/intention-relay/`](docs/intention-relay/README.md) is the authoritative source for product, architecture, quality, and roadmap context.
- Before work that affects milestones, phases, architecture, crate boundaries, quality policy, or workflow behavior, read the relevant documents in [`docs/intention-relay/architecture/`](docs/intention-relay/architecture/README.md) completely enough to understand the applicable context.
- In particular, consult the [quality-gate policy](docs/intention-relay/architecture/12-quality-gates-and-makefile.md), [TDD/TTD policy](docs/intention-relay/architecture/10-test-driven-delivery-and-verification.md), and [implementation roadmap](docs/intention-relay/architecture/11-implementation-roadmap.md) when they apply.

## Parallel tool use

- Treat parallel tool calls as the target for broad or multi-surface work.
- For most such tasks, target at least five genuinely independent useful calls. Target 10–15 only when that many independent checks materially improve speed or coverage.
- Do not create no-op calls, duplicate calls, or parallelize actions that depend on earlier results.

## Sub-agent discipline

- Do not start sub-agents when work is sequential, narrow, or can be completed directly without material benefit.
- Use sub-agents only when parallel exploration, specialized expertise, or isolated long-running work provides a concrete benefit.

## Test-first and architectural boundaries

- Follow TDD/TTD: establish applicable contract, architecture, and outcome tests before implementation.
- Every new production crate must be declared in the machine-readable policy with its responsibility, test target, and coverage tier before production code is accepted.
- Never bypass `WorkspaceRoot`, DTO-first, or adapter-isolation rules.

## Quality and documentation consistency

- Use the root Makefile as the quality orchestration interface. Run `make quick` while iterating and `make verify` before handing off implementation work.
- Changes to Makefile, linting, coverage, feature profiles, or supply-chain policy must update the relevant machine-readable policy and architecture documentation in the same change.

## Git discipline

- Use Conventional Commit messages.
- Prefer atomic commits. Combine changes only when splitting them would harm coherence or make history misleading. State the technical reason in the commit body when relevant.
