# Source-of-Truth Matrix

## Status vocabulary

`Adopt`, `Adapt`, `Supersede`, `PreserveHistorical`, `Defer`, `Exclude`, and
`Conflict` are the only dispositions. `Conflict` is forbidden for Foundation
rules at package exit; deferred non-Foundation conflicts remain explicit in the
contradiction register.

## Foundation topics

| Topic ID | Normative proposition | Applicability | Disposition | Primary owner | Compatibility/failure rule | Delivery bucket | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| GOV-001 | Research concepts do not reopen closed M4 or authorize implementation. | all | Adopt | architecture README | M4 meaning and evidence remain unchanged. | Foundation | documentation review |
| GOV-002 | Every adopted future rule has exactly one primary owner; mirrors state only local consequences. | all | Adopt | architecture README | Duplicate or missing ownership blocks later delivery. | Foundation | architecture review |
| RUN-001 | Execution kind is closed: Ordinary, Mandate, or VerifierMandate. | future | Adopt | `14-run-execution-meaning-and-historical-compatibility.md` | Kind/version/payload mismatch blocks dependent external work. | Execution meaning | canonical contract fixtures planned |
| RUN-002 | Historical M3/M4 and ordinary records gain no synthetic future state. | historical-only | Adopt | `14-run-execution-meaning-and-historical-compatibility.md` | Preserve readable history; do not reconstruct from current state. | Execution meaning | compatibility fixtures planned |
| MAN-001 | A Mandate is durable user-issued work authority, distinct from Goal, Skill, prompt, provider, and daemon. | mandate | Adopt | `13-mandate-domain-and-durable-lifecycle.md` | Non-authoritative references cannot schedule or mutate lifecycle. | Mandate lifecycle | authority fixture planned |
| MAN-002 | Mandate revision changes affect only future fresh-run admission. | mandate | Adopt | `13-mandate-domain-and-durable-lifecycle.md` | An admitted run retains immutable meaning. | Mandate lifecycle | lifecycle fixture planned |
| MAN-003 | A Mandate continuation admits a new RunId and never resumes old work. | mandate | Adopt | `13-mandate-domain-and-durable-lifecycle.md` | Restart never resumes provider, tool, process, kernel, child, MCP, or external work. | Mandate lifecycle | recovery outcome planned |
| DUR-001 | Durable transitions atomically commit projections, events, snapshots, and idempotency evidence. | future | Adapt | sessions/storage | Commit nothing on failure. | Foundation | fault injection planned |
| DUR-002 | External work occurs outside transition transactions; publication follows commit and scoped reread. | future | Adopt | sessions/storage | Publisher failure never rolls back commit. | Foundation | transaction outcome planned |
| DUR-003 | External attempts distinguish before-start, started, known terminal, and unknown terminal. | mandate | Adopt | principles/scope | Known failure is not unknown. | Foundation | phase fixture planned |
| MAN-004 | Unknown terminal effect pauses the owning Mandate until exact reconciliation. | mandate | Adopt | `13-mandate-domain-and-durable-lifecycle.md` | No retry, reattach, rediscovery, next step, or automatic continuation. | Mandate lifecycle | crash matrix planned |
| MAN-005 | User lifecycle/revision commands win optimistic conflicts with daemon or verifier mutations. | mandate | Adopt | `13-mandate-domain-and-durable-lifecycle.md` | Conflicting non-user mutation rejects safely. | Mandate lifecycle | race fixture planned |
| MAN-006 | Intrinsic bounds remain correctness constraints; capacity is typed observable unavailability; product ceilings cannot silently govern Mandate admission. | mandate | Adopt | principles/scope | Unclassified retained numeric limits are not Mandate quotas. | Foundation | policy review |
| TLS-001 | One daemon-owned, composition-assembled capability path is required; no second registry/gateway/authority exists. | future | Adapt | crate map | Bridge, child, kernel, MCP, and provider paths cannot bypass it. | Foundation | architecture fixture planned |
| SEC-001 | WorkspaceRoot, modes, hooks, gateway, and audit are product controls, not OS sandboxes. | all | Adopt | principles/scope | Trusted-local OS authority remains explicit. | Foundation | documentation review |
| QLT-001 | Later implementation requires declared owners, test targets, coverage tiers, feature treatment, architecture fixtures, and outcome evidence. | future | Adopt | TDD verification | No production activation from research alone. | Foundation | quality review |
| RDM-001 | M4+ Foundation is documentation-only; M5-M9 are not silently renumbered or replaced. | all | Adopt | roadmap | A later roadmap decision is required for delivery changes. | Foundation | roadmap review |

## Later topic families

| Family | Concept anchor family | Disposition | Owner/delivery bucket |
| --- | --- | --- | --- |
| TLS-002..015 | direct descriptor admission, registry, WorkspaceRoot | Defer, with CON-001/002 explicit | tools and tool-loop packages |
| MTL-001..018 | model-tool loop | Defer | model-tool-loop package |
| BRG-001..014, KER-001..014 | bridge and IPython | Defer | gateway/programmable-runtime packages |
| CHD-001..018, VER-001..011, ACT-001..010 | child graph, verifier, activity | Defer | child/verifier/activity packages |
| GOL-001..012, SKL-001..015, MEM-001..006, CMP-001..006 | Goals, Skills, memory, compaction | Defer | context packages |
| MCP-001..016 | Mandate MCP lifecycle | Defer | MCP package |
| PRV-001..020, RSN-001..015 | responses, profiles, reasoning | Defer | provider evolution package |
| FRK-001..018 | session forks | Defer | branching package |
| EXC-001..020 | live reload, pricing, discovery, plugin packages, sandboxing, remote continuation, dynamic ToolId, rich MIME and worker supervision | Exclude or Defer as recorded in concept | explicit later decision only |

## Coverage ledger

The selected concept headings listed in [Concept Supersession Index](concept-supersession-index.md) map to exactly one topic family. Detailed DTO field inventories, numeric bounds, and test-case lists remain in later owner documents rather than duplicating the research document here.
