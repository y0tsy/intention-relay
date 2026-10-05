# Intention Relay Architecture Plan

**Planning reference, approved direction.** This directory defines the target architecture and delivery constraints for
Intention Relay. It is an implementation plan, not application code and not a migration plan for Antibusy.

## Purpose and precedence

This directory defines the implementation architecture for Intention Relay. The legacy-derived baseline the rewrite
started from is retired and remains in git history.

When the documents differ:

1. explicit future product decisions override legacy implementation details;
2. research material under [`../../reference/`](../../reference/README.md) is preserved reference only;
3. this architecture must not inherit a legacy design merely because it existed.

## Architectural summary

Intention Relay is a local-first, single-user system. A standalone daemon owns the application runtime and state.
Desktop Tauri and TUI/REPL are presentation adapters over one typed local protocol and one shared Rust client.

```mermaid
flowchart TD
  UI[Tauri UI] --> BR[Native bridge]
  TUI[TUI or REPL] --> CL[Typed client]
  BR --> CL
  CL --> TR[Local transport]
  TR --> DM[Intention daemon]
  DM --> AP[Application]
  AP --> RT[Run runtime]
  AP --> ST[SQLite storage]
  RT --> TL[Tools and hooks]
  RT --> MD[Model drivers]
```

<!-- UI: Svelte presentation; BR: Tauri Rust bridge; DM: single-user daemon. -->

## Core terms

| Term | Meaning |
| --- | --- |
| **DTO** | A strictly typed data-transfer object used at every crate and process boundary. It has an explicit schema and does not expose implementation resources. |
| **Daemon** | The single local process that owns runtime actors, persistence connections, active runs, providers, tools, hooks, and subscriptions. |
| **Project** | A logical user project associated with one or more sessions. |
| **Session** | A durable project-backed conversation and work record. It has one `WorkspaceRoot` and at most one active run. |
| **Turn** | A causally identified unit of conversation, such as a user request or assistant response. |
| **Run** | One agent execution lifecycle started from an accepted user turn. |
| **WorkspaceRoot** | The required addressing anchor and process CWD for every session tool: relative paths join the root, `execute` starts there, and pathless `glob`/`grep` search from there (ADR 0047). |
| **Artifact** | A durable work product associated with a session or run. Plans are artifacts. |
| **Hook** | A typed, ordered extension point around tool execution and result processing. |
| **Plan** | A physical, revisioned artifact produced by a planning-focused mode; ordinary project writes remain denied, while `execute` is available as trusted-local, advisory-guided execution. |
| **Build Autopilot** | The single user-authorized Build policy that executes the configured active tool surface without per-action confirmation. |
| **Projection** | A normalized current-state representation derived and stored for efficient queries. |
| **TTD** | Test-driven delivery. Each milestone starts from executable contracts and observable outcomes, not only structural design. |
| **Quality gate** | A reproducible, blocking Makefile pipeline that verifies formatting, linting, feature profiles, tests, documentation, architecture, coverage, and supply-chain policy. |

## Reading paths

### Start here

1. [Principles and scope](00-principles-and-scope.md)
2. [Workspace and crate map](01-workspace-and-crate-map.md)
3. [DTO and contract policy](02-dto-and-contract-policy.md)
4. [Quality gates and Makefile](12-quality-gates-and-makefile.md)
5. [Test-driven delivery and verification](10-test-driven-delivery-and-verification.md)

### Runtime and persistence

1. [Daemon, transport, and adapters](03-daemon-transport-and-adapters.md)
2. [Sessions, runs, events, and storage](04-sessions-runs-events-and-storage.md)
3. [Mandate domain and durable lifecycle](13-mandate-domain-and-durable-lifecycle.md)
4. [Run execution meaning and historical compatibility](14-run-execution-meaning-and-historical-compatibility.md)
5. [Tool registry and direct Mandate tool loop](15-tool-registry-and-mandate-tool-loop.md)
6. [Mandate scheduler and readiness-driven admission](16-mandate-scheduler-and-readiness-driven-admission.md)
7. [Mandate child graph and delegated verifier authority](17-mandate-child-graph-and-delegated-verifier-authority.md)
8. [Mandate MCP capability lifecycle](18-mandate-mcp-capability-lifecycle.md)
9. [Mandate Gateway/RLM bridge](19-mandate-gateway-rlm-bridge.md)
10. [Run-scoped IPython kernel lifecycle](20-ipython-kernel-lifecycle.md)
11. [Goals, Skills, context, memory, and compaction](21-goals-skills-context-memory-and-compaction.md)
12. [Provider evolution, profiles, and reasoning](22-provider-evolution-profiles-and-reasoning.md)
13. [Non-destructive session branching and regeneration](23-non-destructive-session-branching-and-regeneration.md)
14. [Activity, UI, and adapters](24-activity-ui-and-adapters.md)
15. [Tools, workspace, and hooks](05-tools-workspace-and-hooks.md)
16. [VFR and Headroom](06-vfr-and-headroom.md)
17. [Configuration and provider control plane](25-configuration-provider-control-plane.md)
18. [Programmatic-caller policy and admission](27-programmatic-caller-policy-and-admission.md)
19. [Goal domain and verification](28-goal-domain-and-verification.md)
20. [Provider session selection and profiles protocol](29-provider-session-and-profiles-protocol.md)

### Agent operation

1. [Plan and Build modes](07-plan-and-build-modes.md)
2. [Model protocol and providers](08-model-protocol-and-providers.md)
3. [Configuration, security, and observability](09-configuration-security-and-observability.md)
4. [Instruction sources and system context](30-instruction-sources-and-system-context.md)

### Delivery

1. [Quality gates and Makefile](12-quality-gates-and-makefile.md)
2. [Test-driven delivery and verification](10-test-driven-delivery-and-verification.md)
3. [Implementation roadmap](11-implementation-roadmap.md)
5. [Architecture decision records](../decisions/README.md)
7. [M0/M1 closure evidence](../closeout/m0-m1-closure-evidence.md)
8. [M1+ quality hardening evidence](../closeout/m1-plus-quality-hardening-evidence.md)
9. [M2 closure evidence](../closeout/m2-closure-evidence.md)
10. [M3 closure evidence](../closeout/m3-closure-evidence.md)
11. [M4 closure evidence](../closeout/m4-closure-evidence.md)
12. [M5 closure evidence](../closeout/m5-closure-evidence.md)

## Document map

```mermaid
flowchart TD
  P[00 Principles] --> C[01 Crates]
  P --> D[02 DTO policy]
  C --> T[03 Daemon transport]
  D --> T
  D --> S[04 Sessions storage]
  T --> S
  D --> W[05 Tools workspace hooks]
  W --> V[06 VFR Headroom]
  W --> B[07 Plan Build]
  D --> M[08 Model providers]
  D --> G[09 Config security]
  P --> K[12 Quality Makefile]
  K --> Q[10 TTD verification]
  K --> R[11 Roadmap]
  S --> Q
  V --> Q
  B --> Q
  M --> Q
  G --> Q
  G --> I30[30 Instructions]
  I30 --> M
  Q --> R[11 Roadmap]
```

## Shared authoring rules

Every plan in this directory must:

- state its scope, ownership, invariants, dependencies, non-goals, failure behavior, and verification requirements;
- distinguish **decided**, **implementation-required**, and **deferred** items;
- use DTO terminology at every boundary;
- avoid Tauri, TUI, or REPL business logic;
- define observable outcomes in addition to architecture;
- identify required tests and their blocking quality gates before implementation work begins;
- link quality requirements to [Quality gates and Makefile](12-quality-gates-and-makefile.md);
- use Mermaid only for architecture, state, relationship, or lifecycle clarity;
- keep Mermaid labels short enough for terminal rendering;
- wrap prose at 120 columns; tables, code fences, and Mermaid blocks keep their own layout.

## v1 boundary

v1 includes Tauri as the primary adapter, TUI/REPL as proof of adapter isolation, a local single-user daemon,
SQLite-first persistence, OpenRouter and generic Chat Completion drivers, typed built-in tools, WorkspaceRoot
addressing, Plan/Build modes, Build Autopilot, VFR, and Headroom/CCR.

v1 excludes Web, Telegram, remote transport, multi-user access, parallel runs in one session, sandbox/container
execution, automatic run resumption, and a direct MCP administration interface. Build Autopilot is trusted-local and
does not claim shell isolation; Plan `execute` is advisory-guided rather than technically read-only.

## Post-M4 authority foundation

The closed M4 baseline remains authoritative for implemented behavior. The post-M4 direction is recorded in the
[accepted decision records](../decisions/README.md); those records establish the authority, compatibility, ownership,
and dependency boundary that later authoritative packages must satisfy, and the [implementation
roadmap](11-implementation-roadmap.md) owns activation order and status.

### Owner documents

-  [13 Mandate domain and durable lifecycle](13-mandate-domain-and-durable-lifecycle.md): future Mandate lifecycle and
  admission; it does not amend M4 or ordinary v1 behavior.
-  [14 Run execution meaning and historical compatibility](14-run-execution-meaning-and-historical-compatibility.md):
  the historical compatibility semantics that remain after the binary canonical codec and digest layer were deleted by
  [ADR 0046](../decisions/0046-typed-serde-json-contracts.md).
-  [15 Tool registry and direct Mandate tool loop](15-tool-registry-and-mandate-tool-loop.md): future fixed registry
  identity, frozen tool selection, Mandate direct admission and WorkspaceRoot semantics, tool-loop facts, and
  tool-effect recovery.
-  [16 Mandate scheduler and readiness-driven admission](16-mandate-scheduler-and-readiness-driven-admission.md): future
  durable candidate reevaluation, readiness/capacity evidence, scheduler handoff, and recovery admission gating;
  [architecture 13](13-mandate-domain-and-durable-lifecycle.md) keeps Mandate lifecycle, reason validity/order, and
  atomic fresh admission.
-  [17 Mandate child graph and delegated verifier
  authority](17-mandate-child-graph-and-delegated-verifier-authority.md): future immutable child edges, direct-parent
  controls, graph terminalization, and target-scoped verifier authority.
-  [18 Mandate MCP capability lifecycle](18-mandate-mcp-capability-lifecycle.md): future typed MCP source acquisition,
  discovery normalization, run-local capability selections, invocation, disposal, and MCP recovery.
-  [19 Mandate Gateway/RLM bridge](19-mandate-gateway-rlm-bridge.md): future bridge attachment, ephemeral grants,
  operation correlation, safe replay, cancellation propagation, and bridge recovery; typed ingress to the one capability
  path.
-  [20 Run-scoped IPython kernel lifecycle](20-ipython-kernel-lifecycle.md): future private kernel epochs, cells,
  namespace checkpoints, safe projections, and kernel recovery, plus the kernel-side import surface and script-import
  evidence of the project script library (`.ir/scripts`, [decision
  0042](../decisions/0042-project-script-library-for-kernel-cells.md)); the library's path convention and tools stay
  with architectures 05 and 15.
-  [21 Goals, Skills, context, memory, and compaction](21-goals-skills-context-memory-and-compaction.md): future
  non-authorizing Goal scope/evidence, Skill disclosure, context manifests/projections, typed memory, and immutable
  compaction; project Goals bind to sessions only through explicit applicability links.
-  [22 Provider evolution, profiles, and reasoning](22-provider-evolution-profiles-and-reasoning.md): future provider
  kinds, profiles/catalogs, immutable provider and capability selections, driver compatibility, and normalized
  reasoning.
-  [23 Non-destructive session branching and regeneration](23-non-destructive-session-branching-and-regeneration.md):
  future ordinary Session lineage, frozen fork context, regeneration, and bounded lineage projections.
-  [24 Activity, UI, and adapters](24-activity-ui-and-adapters.md): future activity trees, safe projections, direct-pair
  messages, notifications, acknowledgement projections, and shared-client adapter behavior.
-  [25 Configuration and provider control plane](25-configuration-provider-control-plane.md): accepted post-M5
  directions — controlled live reload, credential rotation, provider health checks, discovery, pricing policy, and the
  profile UI/control plane ([ADR 0020](../decisions/0020-configuration-provider-control-plane-directions.md)); the Slice
  2 activation was reverted and re-introduction requires a new activating specification.
-  [27 Programmatic-caller policy and admission](27-programmatic-caller-policy-and-admission.md): the accepted post-M5
  programmatic-caller policy — the root origin, durable provenance, policy scope/narrowing, admission decisions,
  confirmation, lifecycle, and run-selection compatibility ([ADR
  0022](../decisions/0022-programmatic-caller-policy-directions.md)); activated under Milestone 5+.
-  [28 Goal domain and verification](28-goal-domain-and-verification.md): the accepted post-M5 Goal aggregate domain —
  Goal identity/scope/tree, lifecycle/readiness/user decision, leading-goal run selection, delegated Verification
  Mandates, verification gates, working memory/roles/templates, model proposals, and the conversation-compaction working
  form ([ADR 0023](../decisions/0023-goal-domain-and-verification-directions.md)); Goals remain acceptance/evidence
  records, not the work-authorization plane.
-  [29 Provider session selection and profiles protocol](29-provider-session-and-profiles-protocol.md): the accepted
  post-M5 provider session-selection layer — session defaults, per-turn/fork overrides, profile-keyed usage, the
  provider profiles protocol, and pending-removal/degraded recovery ([ADR
  0024](../decisions/0024-provider-session-and-profiles-protocol-directions.md)); the Slice 2 activation was reverted
  and re-introduction requires a new activating specification.
-  [30 Instruction sources and system context](30-instruction-sources-and-system-context.md): the instruction channel of
  a model request — closed instruction source kinds and scopes, the deployment profile, workspace `AGENTS.md` project
  instructions read through the `WorkspaceRoot` anchor, the `Mode` and `Vfr` contributions of architectures 07 and 06,
  deterministic assembly, the immutable effective instruction projection with its typed identity, closed failures, and
  safe observability ([ADR 0043](../decisions/0043-instruction-sources-and-system-context.md)); activated under
  Milestone 5+ as the fifth slice.

### Foundation terms

| Term | Meaning |
| --- | --- |
| **Ordinary execution** | Existing run semantics, including historical M3/M4 behavior. |
| **Mandate** | Future durable user-issued work authority. It is distinct from a Goal, prompt, Skill, tool permission, provider continuation, or daemon. |
| **VerifierMandate execution** | Future Mandate execution with explicit, target-scoped delegated verifier authority. |
| **Mandate child graph** | Future immutable direct-child Mandate edges and delegation snapshots, distinct from session or conversation lineage. |
| **Fresh run** | A new `RunId` admitted from durable future work state. It is never resumption of prior external work. |
| **ExternalEffectUnknown** | A future started external attempt whose terminal effect cannot be durably proven. It is distinct from a known failure. |
| **Intrinsic bound** | A representation, correctness, or security constraint. |
| **Capacity availability** | Observable temporary resource availability, not a product quota. |
| **Product ceiling** | A policy quota that requires a recorded precedent; it cannot silently govern future Mandate admission. |

A state name is always qualified by its owner, for example Mandate `Active` or Run `Running`. WorkspaceRoot, modes,
hooks, gateways, and audit are logical product controls in a trusted-local process, not OS sandbox or privilege
boundaries.

### Cross-domain identity and sequencing invariants

The following table is normative at the role level. [Architecture
14](14-run-execution-meaning-and-historical-compatibility.md) remains the owner of historical compatibility semantics;
owner documents define only their domain-specific semantic payloads, and every new family is typed serde JSON ([ADR
0046](../decisions/0046-typed-serde-json-contracts.md)).

| Value | Owner | Scope | Representation/ordering | Reconstruction rule |
| --- | --- | --- | --- | --- |
| `SessionId`, `WorkspaceId`, `RunId`, `TurnId` | architecture 04 and existing domain owners | ordinary M3/M4 | historical domain newtypes and recorded sequences | never reconstruct or replace historical identity |
| `MandateId`, revision, `ReasonId` | architecture 13 | Mandate aggregate | daemon/user-issued domain values ordered by the Mandate container journal | never derive from current mutable state |
| future execution-kind contract | architecture 14 | admitted run | typed serde JSON contract; no canonical bytes or digest layer (ADR 0046) | no fallback after missing or mismatched typed data |
| child edge, delegation, verifier authority/baseline | architecture 17 | Mandate graph and target mutation | domain newtypes plus owner-defined typed references | stale or absent baselines fail before mutation |
| `ConversationTreeId`, `ForkOperationId` | architecture 23 | ordinary Session lineage | typed lineage values; tree root derivation is frozen by architecture 23 | never infer lineage from current ancestry |
| `AgentActivityTreeId` and activity records | architecture 24 | projections | daemon-assigned IDs and tree-local journal sequence | never convert from Session/Run/lineage identity |
| provider/tool/MCP/kernel selections | architectures 15, 18, 20, 22 | future admitted run | immutable credential-free semantic references | live registry/resources cannot repair selection |
| UUIDs, operation IDs, correlation IDs | architecture 02 plus owning architecture | all | distinct domain newtypes; UUID equality is not cross-domain identity | no conversion or authority inference |

Two durable ordering authorities exist, plus one observation position; together they are the closed set:

| Authority | Scope | Orders | Must not be reused for |
| --- | --- | --- | --- |
| Session event sequence (`SessionEventSequenceDto`) | one session | every record committed in the session: events, run lifecycle, tool lifecycle, model and tool facts | container records, observation positions |
| Container journal sequence (storage mechanism `container_journals`; the only kind today is `run`, whose position type is `RunEventCursorDto`) | one container: a run today; a Mandate aggregate, a delegation pair, a conversation tree, or an activity tree when activated | records that belong to the container and are not a record of one session | session records, observation positions, identity |
| Observation cursor (reserved name; no DTO until a producer exists) | one reader | nothing: it is a resume position | never an authority, never durable order, never a dedup key, never a conflict token |

1. No record family may introduce a further ordering sequence: a family orders by the session event sequence, or belongs
   to exactly one container and orders by that container's journal, or has no durable order; there is no fourth option.
2. A container journal is dense within its container and is its only gap-detection token; today's adjacency (`+1`)
   contracts stay in force.
3. The container-scoped optimistic append token stays container-local: a write to another container or to the session
   sequence must never invalidate it.
4. Cross-authority correlation uses typed identity only, with no arithmetic, offsets, or conversions.
5. Queue tickets are a queue-ordering mechanism, not an ordering authority: never merged, never reused, never
   renumbered.
6. Scheduler readiness observations order by the owner-local operational tuple `(source_instance_id, source_epoch,
   source_sequence)`; that is operational metadata, not an ordering authority.

Semantic/frozen metadata includes identities, revisions, selection references, and baselines. Operational/live metadata
includes readiness, capacity, processes, handles, endpoints, current catalogs, wakeups, grants, and publication state.
Operational data may defer or reject fresh work, but never repairs, reroutes, reinterprets, or replaces frozen semantic
data.
