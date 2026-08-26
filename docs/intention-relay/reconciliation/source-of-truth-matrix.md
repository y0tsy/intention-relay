# Autopilot transition authority note

For Plan/Build transition behavior, consult ADR 0017 and ADR 0018 together with
architecture 07. The current authoritative direction is: Plan `execute` is
available and advisory-guided, ordinary Plan project `write`/`edit` is denied,
Build Autopilot is unrestricted by per-action confirmation, plan approval starts
a fresh Build run in the same Session by default, and optional handoff creates a
separate frozen-context Session. Existing M3/M4 and recovery semantics remain
unchanged until implementation evidence is recorded.

# Source-of-Truth Matrix

## Status vocabulary

`Adopt`, `Adapt`, `Supersede`, `PreserveHistorical`, `Defer`, `Exclude`, and
`Conflict` are the only dispositions. `Conflict` is forbidden for Foundation
rules at package exit; deferred non-Foundation conflicts remain explicit in the
contradiction register.

## Foundation topics

| Topic ID | Normative proposition | Applicability | Disposition | Primary owner | Compatibility/failure rule | Delivery bucket | Evidence status/anchor |
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

Family rows are navigation summaries only. Atomic topic rows below are the
claim-level inventory; each future row must eventually carry an exact immutable
concept anchor and an evidence status from the controlled vocabulary.

| Family | Concept anchor family | Disposition | Owner/delivery bucket |
| --- | --- | --- | --- |
| TLS-002..015 | direct descriptor admission, registry, WorkspaceRoot | Adopt for future Mandate execution; ordinary behavior preserved | `15-tool-registry-and-mandate-tool-loop.md` |
| MTL-001..018 | model-tool loop | Adopt for future Mandate execution | `15-tool-registry-and-mandate-tool-loop.md` |
| SCH-001..012 | durable scheduler and readiness-driven admission | Adopt for future Mandate execution; calendar/interval semantics deferred | `16-mandate-scheduler-and-readiness-driven-admission.md` |
| BRG-001..014 | Gateway/RLM bridge | Adopt for future Mandate execution; historical bridge/RLM preserved | `19-mandate-gateway-rlm-bridge.md` |
| KER-001..018 | Run-scoped IPython kernel lifecycle | Adopt for future Mandate execution; retained IPython/RLM preserved | `20-ipython-kernel-lifecycle.md` |
| CHD-001..018, VER-001..011 | child graph and verifier authority | Adopt for future Mandate execution; retained RLM and ordinary history preserved | `17-mandate-child-graph-and-delegated-verifier-authority.md` |
| ACT-001..010 | general activity and notifications | Adopt for future projections; historical M3/M4 compatibility only | `24-activity-ui-and-adapters.md` |
| GOL-001..012, SKL-001..015, MEM-001..006, CMP-001..006 | Goals, Skills, memory, compaction | Adopt for future Mandate execution; historical context preserved | `21-goals-skills-context-memory-and-compaction.md` |
| MCP-001..016 | Mandate MCP capability lifecycle | Adopt for future Mandate execution; retained bounded-MCP history preserved | `18-mandate-mcp-capability-lifecycle.md` |
| PRV-001..020, RSN-001..015 | responses, profiles, reasoning | Adopt for future Mandate execution; M3/M4 provider behavior preserved | `22-provider-evolution-profiles-and-reasoning.md` |
| FRK-001..018 | session forks | Adopt for future ordinary Session branching; M3/M4 behavior preserved | `23-non-destructive-session-branching-and-regeneration.md` |
| EXC-001..020 | live reload, pricing, discovery, plugin packages, sandboxing, remote continuation, dynamic ToolId, rich MIME and worker supervision | Exclude or Defer as recorded in concept | explicit later decision only |

## Tool registry and Mandate-loop topics

| Topic ID | Normative proposition | Applicability | Disposition | Primary owner | Compatibility/failure rule | Delivery bucket | Evidence status/anchor |
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

## Gateway/RLM bridge topics

| Topic ID | Normative proposition | Applicability | Disposition | Primary owner | Compatibility/failure rule | Delivery bucket | Evidence status/anchor |
| --- | --- | --- | --- | --- | --- | --- | --- |
| BRG-001 | One daemon-owned Gateway/RLM bridge is the only future facade/direct-model ingress. | future Mandate | Adopt | architecture 19 | A second registry, gateway, listener, daemon, or primitive bypass fails before effect. | Gateway/RLM bridge | architecture fixture planned |
| BRG-002 | Attachment uses negotiated capability and daemon-issued ephemeral grant. | future Mandate | Adopt | architecture 19 | Unsupported or stale attachment fails closed before partial future state. | Gateway/RLM bridge | negotiation fixture planned |
| BRG-003 | A grant is scoped transport evidence, not lifecycle, policy, execution meaning, or durable authority. | future Mandate | Adopt | architecture 19 | Expired/detached/restarted grants fail before admission. | Gateway/RLM bridge | scope fixture planned |
| BRG-004 | Bridge operations bind immutable identity, frozen context, typed input digest, and ToolCallId before effect. | future Mandate | Adopt | architecture 19 | Raw input/resources remain private; changed reuse fails before effect. | Gateway/RLM bridge | canonical fixture planned |
| BRG-005 | Equal operation replay returns existing durable evidence and never creates another effect. | future Mandate | Adopt | architecture 19 | Duplicate ingress converges; changed reuse conflicts. | Gateway/RLM bridge | idempotency fixture planned |
| BRG-006 | Bridge ingress uses frozen architecture-15 descriptor admission and tool-loop facts only. | future Mandate | Adopt | architecture 19 | No confirmation, corridor, quota, reservation, or root-origin gate appears. | Gateway/RLM bridge | admission fixture planned |
| BRG-007 | Bridge publication follows commit and scoped reread and creates no sequence or result channel. | future Mandate | Adopt | architecture 19 | Publisher failure cannot roll back or redispatch. | Gateway/RLM bridge | publication fixture planned |
| BRG-008 | Bridge replay is negotiated, history-before-live, and read-only. | future Mandate | Adopt | architecture 19 | Replay/resync/reconnect causes no effect or partial ordinary projection. | Gateway/RLM bridge | protocol fixture planned |
| BRG-009 | Channel loss and grant expiry do not cancel a run; bridge only propagates cancellation. | future Mandate | Adopt | architecture 19 | Late facts cannot mutate terminal state. | Gateway/RLM bridge | race fixture planned |
| BRG-010 | A started unproven bridge effect pauses only its owning Mandate. | future Mandate | Adopt | architecture 19 | No retry, next step, reattach, or replay follows uncertainty. | Gateway/RLM bridge | crash matrix planned |
| BRG-011 | Recovery invalidates grants and never reissues, re-admits, reattaches, retries, resumes, or reruns old bridge work. | future Mandate | Adopt | architecture 19 | Later work requires fresh run/grant/operation identities. | Gateway/RLM bridge | recovery fixture planned |
| BRG-012 | Bridge-carried `sub_agent` uses architecture-17 child creation and assigns no RLM child authority. | future Mandate | Adopt | architecture 19 | No retained RLM identity becomes a Mandate edge. | Gateway/RLM bridge | child outcome planned |
| BRG-013 | Bridge-held grants, evidence, child references, and MCP facts are non-authorizing. | future Mandate | Adopt | architecture 19 | No verifier, child, MCP, scheduler, or lifecycle authority amplification. | Gateway/RLM bridge | authority fixture planned |
| BRG-014 | Historical M3/M4 and retained RLM records gain no bridge state or reconstructed meaning. | historical-only | Adopt | architecture 19 | Preserve bytes/meaning; readable history remains isolated. | Gateway/RLM bridge | compatibility fixture planned |

## Run-scoped IPython kernel topics

| Topic ID | Normative proposition | Applicability | Disposition | Primary owner | Compatibility/failure rule | Delivery bucket | Evidence status/anchor |
| --- | --- | --- | --- | --- | --- | --- | --- |
| KER-001 | Kernel is a private daemon-managed sidecar, not a second daemon, registry, store, scheduler, or authority. | future Mandate | Adopt | architecture 20 | Second capability path or authority fails before effect. | Kernel lifecycle | architecture fixture planned |
| KER-002 | One live kernel epoch belongs to one admitted RunId. | future Mandate | Adopt | architecture 20 | No cross-run, child, verifier, session, project, or daemon sharing. | Kernel lifecycle | scope fixture planned |
| KER-003 | Kernel creation is lazy and occurs only after recovery, durable reread, compatible meaning, and live availability. | future Mandate | Adopt | architecture 20 | No process on replay, queued work, or invalid meaning. | Kernel lifecycle | lifecycle fixture planned |
| KER-004 | Kernel selection is immutable credential-free meaning; live resources are not. | future Mandate | Adopt | architecture 20 | Current kernel/process/configuration cannot repair missing meaning. | Kernel lifecycle | canonical fixture planned |
| KER-005 | Kernel host requests consume architecture 19 and architecture 15 only. | future Mandate | Adopt | architecture 20 | No listener, ToolId, registry, primitive bypass, sequence, or result channel. | Kernel lifecycle | gateway fixture planned |
| KER-006 | Foreground cells bind exact run, revision, step, epoch, selection, and attempt identity. | future Mandate | Adopt | architecture 20 | Python cannot choose application identities. | Kernel lifecycle | binding fixture planned |
| KER-007 | At most one foreground cell executes per epoch; direct effects retain exact attempt evidence. | future Mandate | Adopt | architecture 20 | Cell evidence does not replace ToolCallStarted for host calls. | Kernel lifecycle | concurrency fixture planned |
| KER-008 | Kernel output is closed safe text projection through existing durable facts. | future Mandate | Adopt | architecture 20 | Raw frames, MIME, binary, tracebacks, objects, and resources stay private. | Kernel lifecycle | projection fixture planned |
| KER-009 | Checkpoints are private, deterministic, verified, bounded convenience state. | future Mandate | Adopt | architecture 20 | They cannot prove effects or replace durable run/tool history. | Kernel lifecycle | checkpoint fixture planned |
| KER-010 | Checkpoints exclude grants, tasks, processes, handles, credentials, resources, and unfinished work. | future Mandate | Adopt | architecture 20 | Unsupported values are omitted; unsafe state never serializes. | Kernel lifecycle | redaction fixture planned |
| KER-011 | Required restore fails closed; optional restore may start empty only in a new run. | future Mandate | Adopt | architecture 20 | No silent checkpoint substitution or old-run continuation. | Kernel lifecycle | restore fixture planned |
| KER-012 | Kernel background work is private convenience and cannot schedule, create children, or retain authority. | future Mandate | Adopt | architecture 20 | Expired-grant requests fail and are never queued. | Kernel lifecycle | task fixture planned |
| KER-013 | Cancellation and late facts cannot assert rollback or repair terminal/unknown state. | future Mandate | Adopt | architecture 20 | Started unproven work pauses only its owner. | Kernel lifecycle | race fixture planned |
| KER-014 | Recovery rejects old sidecars and never resumes, retries, reattaches, reruns, polls, or redisplays old work. | future Mandate | Adopt | architecture 20 | Later work uses fresh RunId, epoch, grant, and operations. | Kernel lifecycle | recovery fixture planned |
| KER-015 | Child/verifier kernel state is isolated and non-authorizing. | future Mandate | Adopt | architecture 20 | Only selected checkpoint copy may seed a child; no verifier authority transfer. | Kernel lifecycle | isolation fixture planned |
| KER-016 | Kernel MCP work uses fixed mcp lifecycle and checkpoints contain no live MCP state. | future Mandate | Adopt | architecture 20 | Fresh work reacquires capabilities. | Kernel lifecycle | MCP fixture planned |
| KER-017 | Kernel replay is negotiated, history-before-live, read-only, and zero-effect. | future Mandate | Adopt | architecture 20 | Unnegotiated peers fail closed. | Kernel lifecycle | protocol fixture planned |
| KER-018 | Retained limits require intrinsic/capacity classification and historical records gain no kernel state. | future/historical | Adopt | architecture 20 | No hidden Mandate quota or synthetic history. | Kernel lifecycle | compatibility fixture planned |

## Scheduler and readiness topics

| Topic ID | Normative proposition | Applicability | Disposition | Primary owner | Compatibility/failure rule | Delivery bucket | Evidence status/anchor |
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

## Child graph and verifier topics

| Topic ID | Normative proposition | Applicability | Disposition | Primary owner | Compatibility/failure rule | Delivery bucket | Evidence status/anchor |
| --- | --- | --- | --- | --- | --- | --- | --- |
| CHD-001 | `sub_agent` creates a durable child Mandate, not a child run or queue item. | future Mandate | Adopt | architecture 17 | No ordinary queue or retained RLM reinterpretation. | Child/verifier | creation outcome planned |
| CHD-002 | Child creation atomically binds Mandate, edge, delegation, result, projections, events, snapshots, and idempotency. | future Mandate | Adopt | architecture 17 | Commit all or nothing; publish after reread. | Child/verifier | fault injection planned |
| CHD-003 | Every child has one immutable direct parent and root graph identity. | future Mandate | Adopt | architecture 17 | Edges, not projections, are authority. | Child/verifier | topology fixture planned |
| CHD-004 | Self-link, cycle, reparent, detach, merge, and cross-graph relations reject. | future Mandate | Adopt | architecture 17 | Reject before mutation/effect. | Child/verifier | graph race planned |
| CHD-005 | Delegation is immutable, canonical, credential-free, and non-inheriting. | future Mandate | Adopt | architecture 17 | Current parent state cannot repair child meaning. | Child/verifier | canonical fixture planned |
| CHD-006 | Child fresh runs bind independent immutable execution meaning. | future Mandate | Adopt | architecture 17 | No live resource/permission/effect inheritance. | Child/verifier | no-fallback fixture planned |
| CHD-007 | Parenthood grants only closed direct-child controls. | future Mandate | Adopt | architecture 17 | No verifier/general lifecycle/indirect authority. | Child/verifier | authority matrix planned |
| CHD-008 | Direct-edge messages are typed, ordered, durable, and non-scheduling. | future Mandate | Adopt | architecture 17 | No RunId, trigger, or direct launch. | Child/verifier | message fixture planned |
| CHD-009 | Parent completion requires required descendant terminal evidence. | future Mandate | Adopt | architecture 17 | Child result never implicitly mutates parent. | Child/verifier | terminalization fixture planned |
| CHD-010 | Daemon safety cascade is durable and grants no indirect authority. | future Mandate | Adopt | architecture 17 | Revalidate graph epoch before completion. | Child/verifier | cascade race planned |
| CHD-011 | Child uncertainty remains child-local and recovery never resumes old work. | future Mandate | Adopt | architecture 17 | No fabricated parent uncertainty or replay. | Child/verifier | recovery matrix planned |
| CHD-012 | Historical M3/M4 and retained RLM records gain no child graph state. | historical-only | Adopt | architecture 17 | Preserve bytes/meaning and fail closed. | Child/verifier | historical fixture planned |
| VER-001 | Verifier mutation requires separately issued, revisioned, target-scoped authority. | future VerifierMandate | Adopt | architecture 17 | Prompt, relation, verdict, and evidence are non-authorizing. | Child/verifier | authority fixture planned |
| VER-002 | Target sets are explicit, immutable, non-self, and never relationship-expanded. | future VerifierMandate | Adopt | architecture 17 | Child work cannot relay or amplify authority. | Child/verifier | target-set fixture planned |
| VER-003 | Audit binds immutable authority, target baseline, contract, evidence, and verdict. | future VerifierMandate | Adopt | architecture 17 | Current state cannot repair a stale audit. | Child/verifier | baseline fixture planned |
| VER-004 | Missing, stale, revoked, expired, consumed, corrupt, or unsupported authority fails before mutation. | future VerifierMandate | Adopt | architecture 17 | Readable history remains isolated where supported. | Child/verifier | failure matrix planned |
| VER-005 | Only named operations may mutate a target under their lifecycle preconditions. | future VerifierMandate | Adopt | architecture 17 | No implicit pause/resume or live-run rewrite. | Child/verifier | operation matrix planned |
| VER-006 | Mutation atomically validates authority, baseline, evidence, target state, and idempotency. | future VerifierMandate | Adopt | architecture 17 | Changed reuse fails before consumption/mutation. | Child/verifier | fault/race fixture planned |
| VER-007 | User lifecycle/revision/reconciliation and authority commands win verifier conflicts. | future VerifierMandate | Adopt | architecture 17 | Loser rereads without merge or changed retry. | Child/verifier | precedence race planned |
| VER-008 | Exact delegated reconciliation names the uncertainty and yields only fresh Active or Stopped. | future VerifierMandate | Adopt | architecture 17 | No rollback, replay, or safe-repeat claim. | Child/verifier | reconciliation fixture planned |
| VER-009 | Verifier unknown effect pauses only the verifier and recovery replays nothing. | future VerifierMandate | Adopt | architecture 17 | Target remains untouched. | Child/verifier | recovery fixture planned |
| VER-010 | Child/verifier delivery is negotiated and fails closed. | future Mandate | Adopt | architecture 17 | Replay is read-only and no partial ordinary projection appears. | Child/verifier | protocol fixture planned |
| VER-011 | Historical records gain no verifier authority, audit, verdict, or mutation state. | historical-only | Adopt | architecture 17 | Preserve M3/M4 and retained RLM meaning. | Child/verifier | compatibility fixture planned |

## MCP capability topics

| Topic ID | Normative proposition | Applicability | Disposition | Primary owner | Compatibility/failure rule | Delivery bucket | Evidence status/anchor |
| --- | --- | --- | --- | --- | --- | --- | --- |
| MCP-001 | `mcp` is the sole fixed MCP ToolId and one daemon-owned Rust capability path. | future Mandate | Adopt | architecture 18 | A second ToolId, registry, gateway, or bypass fails before effect. | MCP lifecycle | slot fixture planned |
| MCP-002 | A typed Mandate source proposal may initiate acquisition without a retained user catalog. | future Mandate | Adopt | architecture 18 | Raw URL, command, headers, credentials, and maps cannot cross the boundary. | MCP lifecycle | source fixture planned |
| MCP-003 | Source records are immutable, canonical, credential-free references to private material. | future Mandate | Adopt | architecture 18 | Missing/corrupt source blocks work without current-state fallback. | MCP lifecycle | canonical fixture planned |
| MCP-004 | Discovery is an independently identified external attempt outside transitions. | future Mandate | Adopt | architecture 18 | Before-start and started phases remain distinct. | MCP lifecycle | fault matrix planned |
| MCP-005 | Discovery normalizes only closed typed input/result schema families. | future Mandate | Adopt | architecture 18 | Unsupported, ambiguous, raw-map-only schemas fail before registration. | MCP lifecycle | normalization fixture planned |
| MCP-006 | Discovery creates immutable capability revisions and ordered selection revisions. | future Mandate | Adopt | architecture 18 | Schema drift creates new records and never rewrites old ones. | MCP lifecycle | revision fixture planned |
| MCP-007 | Every model step freezes the exact accumulated MCP selection it consumed. | future Mandate | Adopt | architecture 18 | Later discovery cannot alter an admitted/sent step. | MCP lifecycle | step-binding fixture planned |
| MCP-008 | Invocation binds exact selection, capability, input digest, operation, and ToolCallId before effect. | future Mandate | Adopt | architecture 18 | Current capability cannot substitute for frozen meaning. | MCP lifecycle | invocation fixture planned |
| MCP-009 | Acquisition and invocation have independent equal-replay/changed-reuse identities. | future Mandate | Adopt | architecture 18 | No duplicate discovery/invocation or changed semantic replay. | MCP lifecycle | idempotency fixture planned |
| MCP-010 | Endpoint and credential generations resolve only in private daemon material. | future Mandate | Adopt | architecture 18 | Secrets/resources never enter durable/public identity. | MCP lifecycle | redaction fixture planned |
| MCP-011 | Safe projection validates typed results and excludes raw resources and server bodies. | future Mandate | Adopt | architecture 18 | Invalid/unsafe output cannot cross the MCP boundary. | MCP lifecycle | projection fixture planned |
| MCP-012 | Source/server/capability/result grants no lifecycle, registry, scheduler, child, verifier, or user authority. | future Mandate | Adopt | architecture 18 | Authority amplification rejects before mutation/effect. | MCP lifecycle | authority fixture planned |
| MCP-013 | Known MCP outcomes remain known; unproven started work pauses only owning Mandate. | future Mandate | Adopt | architecture 18 | No retry, next step, or continuation follows uncertainty. | MCP lifecycle | effect matrix planned |
| MCP-014 | Cancellation/restart disposes private resources and never repeats old work. | future Mandate | Adopt | architecture 18 | Later work needs fresh run/acquisition identities. | MCP lifecycle | recovery fixture planned |
| MCP-015 | MCP readiness is non-authorizing and cannot discover or select capabilities. | future Mandate | Adopt | architecture 18 | Scheduler retains reason, cannot start MCP work. | MCP lifecycle | readiness fixture planned |
| MCP-016 | Dynamic acquisition supersedes retained bounded rules only for future Mandates. | future/historical | Adapt | architecture 18 | M3/M4 and retained bounded-MCP history stay unchanged. | MCP lifecycle | compatibility fixture planned |

## Session branching topics

| Topic ID | Normative proposition | Applicability | Disposition | Primary owner | Compatibility/failure rule | Delivery bucket | Evidence status/anchor |
| --- | --- | --- | --- | --- | --- | --- | --- |
| FRK-001 | A fork creates an independent child Session and never rewrites source history. | future ordinary Session | Adopt | architecture 23 | Rewind never truncates or rolls back effects. | Session branching | outcome fixture planned |
| FRK-002 | Tree/root/parent/boundary/operation identities are immutable typed lineage. | future ordinary Session | Adopt | architecture 23 | Child never derives or substitutes a root identity. | Session branching | canonical fixture planned |
| FRK-003 | Only committed user-turn and completed assistant-turn boundaries are valid. | future ordinary Session | Adopt | architecture 23 | Partial, queued, failed, waiting, and unfinished work rejects. | Session branching | boundary matrix planned |
| FRK-004 | Fork context is a closed materialized projection, not live ancestor replay. | future ordinary Session | Adopt | architecture 23 | Unsupported or oversized context rejects without fallback. | Session branching | context fixture planned |
| FRK-005 | Each child owns a flattened immutable credential-free base snapshot. | future ordinary Session | Adopt | architecture 23 | Corrupt or unsupported history blocks dependent work pre-effect. | Session branching | canonical fixture planned |
| FRK-006 | Reasoning and terminal references are typed provenance, not context or authority. | future ordinary Session | Adopt | architecture 23 | No text, resumed work, or authority transfer. | Session branching | isolation fixture planned |
| FRK-007 | Fork creation is atomic, idempotent, and external-effect-free. | future ordinary Session | Adopt | architecture 23 | Faults leave no partial child or lineage state. | Session branching | fault injection planned |
| FRK-008 | Lineage audit has its own sequence and never mutates source Session history. | future ordinary Session | Adopt | architecture 23 | No synthetic source fork event or replay widening. | Session branching | sequence fixture planned |
| FRK-009 | Roots migrate additively while preserving M3/M4 bytes and meaning. | historical/future | Adopt | architecture 23 | No synthetic parent, run, anchor, or message. | Session branching | migration fixture planned |
| FRK-010 | Regeneration is separate ordinary child execution and cannot bypass Mandate admission. | future ordinary Session | Adopt | architecture 23 | Failed start retains idle branch; no Mandate reason/RunId. | Session branching | workflow fixture planned |
| FRK-011 | Branch archive/title state is per Session and non-destructive. | future ordinary Session | Adopt | architecture 23 | Archive needs idle session; lineage/snapshot never rewrites. | Session branching | lifecycle fixture planned |
| FRK-012 | Workspace state is explicitly unverified and makes no rollback claim. | future ordinary Session | Adopt | architecture 23 | No file/process/remote-effect reconstruction. | Session branching | safety fixture planned |
| FRK-013 | Fork limits are ordinary-session policy and never Mandate quotas. | future ordinary Session | Adopt | architecture 23 | Representation bounds remain intrinsic/protocol only. | Session branching | taxonomy fixture planned |
| FRK-014 | Fork protocol is separately negotiated and tree reads are bounded/read-only. | future ordinary Session | Adopt | architecture 23 | Old peers fail closed; no tree-wide stream. | Session branching | protocol fixture planned |
| FRK-015 | Conversation lineage is distinct from Mandate child/verifier/RLM relations. | future/all | Adopt | architecture 23 | No authority or graph edge derives from a branch. | Session branching | authority fixture planned |
| FRK-016 | Forks copy no active work, grant, request, queue, or unfinished effect. | future/all | Adopt | architecture 23 | Recovery never resumes transferred work. | Session branching | recovery matrix planned |
| FRK-017 | Child runs bind their own provider meaning; default is non-authorizing. | future ordinary Session | Adopt | architecture 23 | No credential/client/request continuation or model-name routing. | Session branching | selection fixture planned |
| FRK-018 | Historical records gain no synthetic fork execution state. | historical-only | Adopt | architecture 23 | Preserve readable ordinary history. | Session branching | compatibility fixture planned |

## Activity, UI, and adapter topics

| Topic ID | Normative proposition | Applicability | Disposition | Primary owner | Compatibility/failure rule | Delivery bucket | Evidence status/anchor |
| --- | --- | --- | --- | --- | --- | --- | --- |
| ACT-001 | Activity identity is daemon-assigned and distinct from Session/fork/Run identity. | future projections | Adopt | architecture 24 | No identity or sequence conversion/inference. | M6 activity/UI | canonical fixture planned |
| ACT-002 | Direct parent/child pairs use closed message kinds and directions. | future Mandate | Adopt | architecture 24 | Invalid relative/direction/order fails pre-publication. | M6 activity/UI | pair fixture planned |
| ACT-003 | Messages carry bounded safe text and typed provenance only. | future projections | Adopt | architecture 24 | No raw sensitive body or authority transfer. | M6 activity/UI | redaction fixture planned |
| ACT-004 | Activity journal is append-only, tree-local, and separately sequenced. | future projections | Adopt | architecture 24 | No Session/Run cursor reuse or global order. | M6 activity/UI | sequence fixture planned |
| ACT-005 | Activity transitions atomically commit safe projections and publish after reread. | future projections | Adopt | architecture 24 | Fault commits nothing; no external effect in transaction. | M6 activity/UI | fault fixture planned |
| ACT-006 | Activity archive/projections are read-only and non-authorizing. | future projections | Adopt | architecture 24 | Archive never deletes, restores authority, or starts work. | M6 activity/UI | lifecycle fixture planned |
| ACT-007 | Activity replay is negotiated, bounded, history-before-live, and resync-safe. | future projections | Adopt | architecture 24 | Old peers fail closed; reconnect has zero effect. | M6 activity/UI | protocol fixture planned |
| ACT-008 | Notifications are separate local-user safe journals with urgent priority. | future projections | Adopt | architecture 24 | No inbox/account/raw alert replay or work initiation. | M6 activity/UI | notification fixture planned |
| ACT-009 | Acknowledgement is separate durable presentation state, not cursor/fact mutation. | future projections | Adopt | architecture 24 | Cannot authorize, schedule, or rewrite journal facts. | M6 activity/UI | acknowledgement fixture planned |
| ACT-010 | Tauri/TUI/REPL use shared-client daemon projections; historical M3/M4 is compatibility-only. | future/historical | Adopt | architecture 24 | No adapter authority or synthetic historical state. | M6 activity/UI | parity fixture planned |

## Coverage ledger

The selected concept headings listed in [Concept Supersession Index](concept-supersession-index.md) are tracked as mapped, context-only, or unresolved. Topic ranges are summaries only and do not claim claim-level completeness until every topic has an exact source anchor, owner section, disposition, and evidence status. Detailed DTO field inventories and test-case lists remain in owner documents; planned evidence is not verification.

## Goals, Skills, context, memory, and compaction topics

| Topic ID | Normative proposition | Applicability | Disposition | Primary owner | Compatibility/failure rule | Delivery bucket | Evidence status/anchor |
| --- | --- | --- | --- | --- | --- | --- | --- |
| GOL-001 | Goals are immutable user-managed acceptance/evidence records, never Mandate authority. | future Mandate | Adopt | architecture 21 | Goal cannot create RunId, reason, lifecycle, scheduler, tool, child, verifier, MCP, bridge, kernel, provider, or reconciliation authority. | Context package | authority fixture planned |
| GOL-002 | Goals are project- or session-scoped; project applicability uses an explicit immutable session link. | future Mandate | Adopt | architecture 21 | No implicit inheritance or cross-session reconstruction. | Context package | scope fixture planned |
| GOL-003 | Selected Goal revisions and links are immutable canonical context evidence. | future Mandate | Adopt | architecture 21 | Current Goal state cannot alter admitted/model-step context. | Context package | canonical fixture planned |
| SKL-001 | Skills are immutable versioned untrusted instructional content. | future Mandate | Adopt | architecture 21 | A Skill cannot carry hidden executable or product authority. | Context package | isolation fixture planned |
| SKL-002 | Skill disclosure is explicit, bounded, and audience-safe. | future Mandate | Adopt | architecture 21 | Unsafe/unavailable material omits or refuses with typed evidence. | Context package | disclosure fixture planned |
| CTX-001 | Admission freezes an exact typed source manifest and model steps bind immutable safe projections. | future Mandate | Adopt | architecture 21 | No current file/catalog/index/memory/configuration/UI fallback. | Context package | binding fixture planned |
| CTX-002 | Safe context representation never broadens source content or audience. | future Mandate | Adopt | architecture 21 | Leakage blocks public/model projection. | Context package | redaction fixture planned |
| MEM-001 | Memory records, cards, disclosures, supersession, and rollback are immutable typed evidence. | future Mandate | Adopt | architecture 21 | No hidden prompt mutation, inferred conflict, or audience widening. | Context package | memory fixture planned |
| CMP-001 | Compaction is immutable safe summary over exact completed history with provenance and suffix. | future Mandate | Adopt | architecture 21 | Original facts remain authoritative; incomplete/unknown work cannot compact. | Context package | provenance fixture planned |
| CTX-003 | Context recovery/replay is read-only and never rediscloses, recompacts, resumes, or performs effects. | future/historical | Adopt | architecture 21 | Unsupported context blocks dependent work while unrelated history remains readable. | Context package | recovery fixture planned |
| CTX-004 | Historical M3/M4 and retained records gain no synthetic context state. | historical-only | Adopt | architecture 21 | Preserve bytes/meaning; no reconstruction from current state. | Context package | compatibility fixture planned |

## Provider evolution and reasoning topics

| Topic ID | Normative proposition | Applicability | Disposition | Primary owner | Compatibility/failure rule | Delivery bucket | Evidence status/anchor |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PRV-001 | Future canonical Responses kind is `responses`; `openai` is parse-time-only normalization. | future Mandate | Adopt | architecture 22 | Alias never persists; unrepresentable input fails before provider work. | Provider evolution | alias fixture planned |
| PRV-002 | Model identifiers never select provider kind, endpoint, driver, capability, profile, or execution kind. | future/historical | Adopt | architecture 22 | No routing or fallback from `gpt-*`, `o*`, `codex*`, or any model ID. | Provider evolution | no-routing fixture planned |
| PRV-003 | User kinds are immutable closed declarative compositions and reserved first-party IDs cannot be replaced. | future Mandate | Adopt | architecture 22 | Plugins, templates, arbitrary parsers/maps, and executable config fail before driver creation. | Provider evolution | descriptor fixture planned |
| PRV-004 | Provider selections are immutable credential-free meaning; catalog/default/readiness are not. | future Mandate | Adopt | architecture 22 | Current state cannot repair, reroute, or replace selection. | Provider evolution | canonical fixture planned |
| PRV-005 | Capability is explicit descriptor envelope intersected with exact model subset and compatible driver contract. | future Mandate | Adopt | architecture 22 | Unknown/invalid capability blocks before outbound work. | Provider evolution | preflight fixture planned |
| PRV-006 | Driver compatibility is code-owned family/major/minor with explicit supported minors. | future Mandate | Adopt | architecture 22 | Major or unsupported-minor mismatch fails before provider work. | Provider evolution | compatibility fixture planned |
| PRV-007 | Credentials/private clients stay private; endpoint/transport metadata is safe selected evidence only. | future Mandate | Adopt | architecture 22 | No secret/raw endpoint/SDK resource crosses public or durable boundaries. | Provider evolution | redaction fixture planned |
| PRV-008 | Provider catalog/profile revisions are append-only; IDs tombstone and never reuse. | future Mandate | Adopt | architecture 22 | User-kind edits need new IDs; no historical selection rewrite. | Provider evolution | lifecycle fixture planned |
| PRV-009 | Catalog validation is all-or-nothing and local-only; acceptance and private activation are distinct. | future Mandate | Adopt | architecture 22 | No network/provider work before exact activated registry. | Provider evolution | fault fixture planned |
| PRV-010 | Provider readiness is non-authorizing live evidence. | future Mandate | Adopt | architecture 22 | Unavailability retains reason and creates no RunId; restoration only reevaluates. | Provider evolution | readiness fixture planned |
| PRV-011 | Provider recovery never admits/retries/attaches old work. | future Mandate | Adopt | architecture 22 | Later work requires fresh RunId after exact reconciliation. | Provider evolution | recovery fixture planned |
| PRV-012 | Historical M3/M4 selections remain ordinary without synthetic profile/catalog/Responses state. | historical-only | Adopt | architecture 22 | Preserve bytes/meaning; explicit bridge may reference only source bytes. | Provider evolution | preservation fixture planned |
| RSN-001 | Generic Chat remains narrow; divergent reasoning uses closed descriptor/kind semantics. | future Mandate | Adopt | architecture 22 | No model-name or raw vendor-flag reasoning inference. | Provider evolution | dialect fixture planned |
| RSN-002 | Normalized reasoning uses Primary/Detail fragments and distinct summaries on one run cursor. | future Mandate | Adopt | architecture 22 | No second stream, merge, deduplication, raw payload, or snapshot accumulation. | Provider evolution | ordering fixture planned |
| RSN-003 | Provider reasoning does not select context sources or disclose prior reasoning. | future Mandate | Adopt | architecture 22 | Architecture 21 owns source/audience/projection; no live history scan. | Provider evolution | authority fixture planned |
| RSN-004 | Responses is local-history-first with `store: false` and no remote continuation or built-in tools. | future Mandate | Adopt | architecture 22 | Unsupported calls/payloads fail before local tool action. | Provider evolution | Responses fixture planned |
| RSN-005 | Future reasoning delivery is negotiated, history-before-live, and read-only. | future Mandate | Adopt | architecture 22 | Unnegotiated peers fail closed; reconnect has zero effect. | Provider evolution | protocol fixture planned |
| RSN-006 | M4 reasoning remains its recorded untagged historical form. | historical-only | Adopt | architecture 22 | No synthetic category, summary, manifest, or replay widening. | Provider evolution | compatibility fixture planned |
