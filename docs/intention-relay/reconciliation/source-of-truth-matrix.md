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
| TLS-002..015 | direct descriptor admission, registry, WorkspaceRoot | Adopt for future Mandate execution; ordinary behavior preserved | `15-tool-registry-and-mandate-tool-loop.md` |
| MTL-001..018 | model-tool loop | Adopt for future Mandate execution | `15-tool-registry-and-mandate-tool-loop.md` |
| SCH-001..012 | durable scheduler and readiness-driven admission | Adopt for future Mandate execution; calendar/interval semantics deferred | `16-mandate-scheduler-and-readiness-driven-admission.md` |
| BRG-001..014, KER-001..014 | bridge and IPython | Defer | gateway/programmable-runtime packages |
| CHD-001..018, VER-001..011, ACT-001..010 | child graph, verifier, activity | Defer | child/verifier/activity packages |
| GOL-001..012, SKL-001..015, MEM-001..006, CMP-001..006 | Goals, Skills, memory, compaction | Defer | context packages |
| MCP-001..016 | Mandate MCP lifecycle | Defer | MCP package |
| PRV-001..020, RSN-001..015 | responses, profiles, reasoning | Defer | provider evolution package |
| FRK-001..018 | session forks | Defer | branching package |
| EXC-001..020 | live reload, pricing, discovery, plugin packages, sandboxing, remote continuation, dynamic ToolId, rich MIME and worker supervision | Exclude or Defer as recorded in concept | explicit later decision only |

## Tool registry and Mandate-loop topics

| Topic ID | Normative proposition | Applicability | Disposition | Primary owner | Compatibility/failure rule | Delivery bucket | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| TLS-002 | The registry has fourteen canonical slots with immutable intended owners. | future Mandate | Adopt | `15-tool-registry-and-mandate-tool-loop.md` | Missing, reordered, reassigned, or duplicate slots block before effect. | Tool-loop | canonical goldens planned |
| TLS-003 | Only composition assembles active descriptors into the one daemon-owned capability path. | future Mandate | Adopt | `15-tool-registry-and-mandate-tool-loop.md` | A private registry or direct primitive bypass fails before effect. | Tool-loop | architecture fixture planned |
| TLS-004 | Reserved slots are absent from model visibility and execution. | future Mandate | Adopt | `15-tool-registry-and-mandate-tool-loop.md` | Reserved invocation has known pre-effect `ExecutionUnavailable`. | Tool-loop | slot-state fixture planned |
| TLS-005 | Descriptor and registry revisions are immutable canonical meaning. | future Mandate | Adopt | `15-tool-registry-and-mandate-tool-loop.md` | Current registry/readiness cannot rebuild stored meaning. | Tool-loop | decode goldens planned |
| TLS-006 | Direct tool selection freezes only active descriptors supplied to the model. | future Mandate | Adopt | `15-tool-registry-and-mandate-tool-loop.md` | Unknown/corrupt selection blocks dependent work before effect. | Tool-loop | no-fallback fixture planned |
| TLS-007 | Effect profiles are descriptive, non-authorizing, and not sandbox claims. | future Mandate | Adopt | `15-tool-registry-and-mandate-tool-loop.md` | Profiles cannot create confirmation or OS-authority claims. | Tool-loop | policy fixture planned |
| TLS-008 | Mandate WorkspaceRoot is default base/CWD with safe observation; ordinary containment remains unchanged. | future Mandate | Adopt | `15-tool-registry-and-mandate-tool-loop.md` | Explicit Mandate paths are not denied solely by location. | Tool-loop | cross-kind path fixture planned |
| TLS-009 | Compatible frozen active Mandate descriptors admit without confirmation, corridor, quota, or root-origin gate. | future Mandate | Adopt | `15-tool-registry-and-mandate-tool-loop.md` | Only typed incompatibility or actual unavailability can refuse pre-effect work. | Tool-loop | admission matrix planned |
| MTL-001 | Tool-calling model steps atomically record one ordered group before effects. | future Mandate | Adopt | `15-tool-registry-and-mandate-tool-loop.md` | Invalid groups fail before local work. | Tool-loop | transaction fixture planned |
| MTL-002 | Calls admit independently, may complete concurrently, and next model context preserves call order. | future Mandate | Adopt | `15-tool-registry-and-mandate-tool-loop.md` | Completion order cannot change semantic exchange order. | Tool-loop | concurrency fixture planned |
| MTL-003 | Started unknown tool effects pause only the owning Mandate; recovery never repeats work. | future Mandate | Adopt | `15-tool-registry-and-mandate-tool-loop.md` | No next step, retry, attach, or resume follows uncertainty. | Tool-loop | crash matrix planned |
| MTL-004 | Tool-loop delivery is separately negotiated and fails closed for unsupported peers. | future Mandate | Adopt | `15-tool-registry-and-mandate-tool-loop.md` | No partial replay/history/live projection. | Tool-loop | negotiation fixture planned |

## Scheduler and readiness topics

| Topic ID | Normative proposition | Applicability | Disposition | Primary owner | Compatibility/failure rule | Delivery bucket | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| SCH-001 | One daemon-owned logical scheduler coordinates durable Mandate reasons; it is not a second runtime or authority. | future Mandate | Adopt | `16-mandate-scheduler-and-readiness-driven-admission.md` | Wakeups/task state cannot mutate lifecycle directly. | Trigger scheduler | architecture fixture planned |
| SCH-002 | Scheduler candidates derive only from durable eligible reasons and current Mandate state. | future Mandate | Adopt | `16-mandate-scheduler-and-readiness-driven-admission.md` | No synthetic queue item or current-state reconstruction. | Trigger scheduler | candidate fixture planned |
| SCH-003 | Selection uses the explicit-user-first total order owned by Mandate lifecycle. | future Mandate | Adopt | `16-mandate-scheduler-and-readiness-driven-admission.md` | Ties are deterministic by time, MandateId, and ReasonId. | Trigger scheduler | ordering fixture planned |
| SCH-004 | Readiness/capacity is typed operational evidence, not authority or immutable meaning. | future Mandate | Adopt | `16-mandate-scheduler-and-readiness-driven-admission.md` | Unknown/stale evidence fails closed; current resources cannot repair meaning. | Trigger scheduler | readiness matrix planned |
| SCH-005 | Unavailability retains the exact reason and creates no RunId, reservation, retry counter, or quota. | future Mandate | Adopt | `16-mandate-scheduler-and-readiness-driven-admission.md` | A later observation may reevaluate, never resume old work. | Trigger scheduler | capacity fixture planned |
| SCH-006 | Readiness restoration wakes reevaluation, not direct launch or fabricated trigger. | future Mandate | Adopt | `16-mandate-scheduler-and-readiness-driven-admission.md` | Duplicate/lost wakeups cannot lose or duplicate work. | Trigger scheduler | wakeup fixture planned |
| SCH-007 | Lifecycle-owned admission atomically revalidates sequence, revision, reason, meaning, and readiness. | future Mandate | Adopt | `16-mandate-scheduler-and-readiness-driven-admission.md` | Conflict loser rereads; no external effect occurs in transaction. | Trigger scheduler | fault/race fixture planned |
| SCH-008 | Recovery completes before fresh scheduler admission and never resumes old work. | future Mandate | Adopt | `16-mandate-scheduler-and-readiness-driven-admission.md` | Pre-crash live readiness is not trusted. | Trigger scheduler | recovery fixture planned |
| SCH-009 | Scheduler replay is Mandate-local, negotiated, and fail-closed. | future Mandate | Adopt | `16-mandate-scheduler-and-readiness-driven-admission.md` | Replay never repeats wakeups, admissions, or effects. | Trigger scheduler | protocol fixture planned |
| SCH-010 | Scheduler introduces no product ceiling, lease, claim, reservation, or fairness entitlement. | future Mandate | Adopt | `16-mandate-scheduler-and-readiness-driven-admission.md` | Retained numeric bounds require intrinsic/capacity classification. | Trigger scheduler | taxonomy fixture planned |
| SCH-011 | M3 queue promotion and M4 scheduling behavior remain unchanged. | historical-only | Adopt | `16-mandate-scheduler-and-readiness-driven-admission.md` | Queue tickets never become Mandate reasons. | Trigger scheduler | historical fixture planned |
| SCH-012 | Calendar/interval/time-zone/DST semantics and worker topology remain deferred. | future Mandate | Defer | Later scheduler package | No historical harness rule silently governs Mandates. | Later scheduler work | later decision |

## Coverage ledger

The selected concept headings listed in [Concept Supersession Index](concept-supersession-index.md) map to exactly one topic family. Detailed DTO field inventories, numeric bounds, and test-case lists remain in later owner documents rather than duplicating the research document here.
