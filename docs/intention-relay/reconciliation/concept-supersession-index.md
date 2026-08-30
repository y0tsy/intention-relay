# Concept Supersession Index

| Concept heading family | Matrix topics | Current disposition | Future authority/delivery bucket |
| --- | --- | --- | --- |
| Status, closed M4 baseline, recommendation | GOV-001..005 | Research provenance and historical baseline retained. | Architecture index, roadmap, compatibility register. |
| Selected semi-autonomous Mandate overlay | MAN-001..018, RUN-001..006 | Lifecycle/admission and execution-meaning compatibility authority adopted; nested owner payloads remain later. | Mandate lifecycle and execution-meaning packages. |
| Direct descriptor admission and unified registry | TLS-001..015 | Adopted for future Mandate execution; ordinary behavior preserved. | Tool registry and Mandate-loop package. |
| Model-tool loop | MTL-001..018 | Ordinary daemon model-tool continuation adopted by ADR 0019; Mandate-specific execution meaning remains future-scoped. | Tool registry and Mandate-loop package. |
| Gateway/RLM bridge | BRG-001..014 | Adopted for future Mandate execution; retained bridge/RLM identity remains historical. | Architecture 19, Gateway/RLM bridge package. |
| Kernel and programmable runtime | KER-001..018 | Adopted for future Mandate execution; retained IPython/RLM identity remains historical. | Architecture 20, run-scoped IPython kernel package. |
| Durable Mandate scheduler and readiness admission | SCH-001..012 | Readiness-driven admission adopted; calendar/interval/time-zone and continual-harness ownership remain deferred or historical-only. | Mandate scheduler package. |
| Child work, verification, activity | CHD-001..018, VER-001..011, ACT-001..010 | Child/verifier authority and activity/UI projections adopted; retained RLM and M3/M4 history remain historical/compatibility-only. | Architectures 17 and 24. |
| Goals, Skills, memory, compaction | GOL-001..012, SKL-001..015, MEM-001..006, CMP-001..006 | Adopted for future Mandate execution; retained context material remains historical. | Architecture 21, Goals/Skills/context package. |
| Dynamic MCP | MCP-001..016 | Adopted for future Mandate execution; retained bounded connection/catalogue policy remains historical. | Architecture 18, Mandate MCP capability lifecycle. |
| Provider, reasoning, execution meaning | PRV-001..020, RSN-001..015, RUN-001..012 | Envelope/canonical compatibility remains architecture 14; provider and reasoning payload semantics adopted for future Mandates. | Architectures 14 and 22, provider evolution package. |
| Forks and lineage | FRK-001..018 | Adopted for future ordinary Session branching; M3/M4 and Mandate authority remain separate. | Architecture 23, session branching package. |
| Verification portfolio, checklist, deferred work | DUR-001..003, QLT-001, RDM-001 (remaining topics require atomic inventory) | Evidence and delivery rules mapped. | Quality and later roadmap reconciliation. |
| Provider profiles, configuration reload, control plane | CFG-001..008 | Adopted for post-M5 fresh runs; M3/M4 startup-only configuration and recorded snapshots remain authoritative. | Architecture 25, configuration and provider control plane. |
| Continual-harness model | CHR-001..008 | Adopted for post-M5 fresh runs; M3/M4 queue tickets, sessions, runs, events, and recovery remain authoritative. | Architecture 26, continual harness. |
| Programmatic-caller policy and admission | PCP-001..008 | Adopted for post-M5 fresh runs; historical-only for new Mandate work where conflicting; M3/M4 and retained RLM history remain authoritative. | Architecture 27, programmatic-caller policy and admission. |
| Goal aggregate domain and verification | GOL-004..012, VGT-001..006 | Adopted for post-M5 fresh runs; Goals remain acceptance/evidence records for new Mandate work. | Architecture 28, Goal domain and verification. |
| Provider session selection and profiles protocol | PSS-001..008 | Adopted for post-M5 fresh runs; M3/M4 startup-only configuration and recorded selections remain authoritative. | Architecture 29, provider session selection and profiles protocol. |
| Agent communication, observation, and notifications | ACT-001..018 | Adopted for post-M5 fresh runs; activity/notification detail (DTOs, record kinds, bounds, safe failures) plus child-operations/model-exchange detail (`RlmMessageExchangeDto`, `RlmChildMessageOperation`) is owned by architecture 24; M3/M4 compatibility-only projections remain. | Architecture 24, activity, UI, and adapters. |
| Mandate identity, revisions, and limit classification | MAN-007..012 | Adopted for post-M5 fresh runs; the Mandate DTO family, stop-conditions/disposition semantics, limit-class and capacity-outcome DTOs, and the shared attempt-evidence family are owned by architecture 13. | Architecture 13, Mandate domain and durable lifecycle. |
| Autonomous continuation | MAN-011 | Adopted for post-M5 fresh runs; Continue autonomously creates or activates a Build-mode Mandate by default, additive to ADR 0017/0018. | Architecture 13, Mandate domain and durable lifecycle. |
| Continual-harness closed safe failures and selection record | CHR-009..010 | Adopted for post-M5 fresh runs; the 15 `harness_*` failures and `ContinualHarnessSelectionV1` content are owned by architectures 26/28. | Architectures 26 and 28. |
| Normalized reasoning detail | RSN-007..015 | Adopted for post-M5 fresh runs; stream correspondence, `provider_reasoning_stream_invalid`, 4-MiB output bound, M4-as-Primary, capability-slice effort/mode sets, resolved policy, `ReasoningEffortDto`, and automatic-summary detail are owned by architecture 22. | Architecture 22, provider evolution. |
| Child graph detail and recovery clauses | CHD-013..018 | Adopted for post-M5 fresh runs; child creation/delegation fields, admission validation, class non-bypass, message-delivery, terminalization, and same-process continuation clauses are owned by architecture 17. | Architecture 17, child graph and verifier authority. |
| Bridge and kernel cancellation boundaries | BRG-015, KER-019 | Adopted for post-M5 fresh runs; no per-`ToolCallId` cancellation command and `StopRunCommandDto`-only kernel cancellation are owned by architectures 19/20. | Architectures 19 and 20. |
| Durable-fact and historical-version rules | DUR-004..006, RUN-003..005 | Adopted for post-M5 fresh runs; no-new-sequence, hook-before-transaction, publication-gate, crate version-ownership, Skill-decoder, and `fork-model-context-v1` trilemma rules are owned by architectures 04/14/23. | Architectures 04, 14, and 23. |
| Accepted deferred directions | ACT-015..016, RSN-015 | Tree-level metadata, semantic content inspection, and per-call cancellation are adopted for post-M5 fresh runs under ADR 0032. | Architectures 24, 22, and 19. |
| Tool-loop detail: schema availability, group shapes, step limit, typed references, combined gate | TLS-010..011, MTL-005..007 | Adopted for post-M5 fresh runs; `model_schema_availability`, the closed `provider_tool_group_invalid` shape matrix, no numeric step limit, typed-reference alternatives, and the combined reasoning+tool publication gate are owned by architecture 15 (and 19 for the gate). | Architecture 15. |
| Bridge slow-peer non-delay | BRG-016 | Adopted for post-M5 fresh runs; the bounded slow-peer path never delays durable execution or healthy subscribers. | Architecture 19. |
| Kernel disposal, diagnostics, background capture | KER-020..022 | Adopted for post-M5 fresh runs; idle disposal, no formatted-footer reconstruction, diagnostics content, and later-cell background capture are owned by architecture 20. | Architecture 20. |
| Child usage no-token-ceiling | CHD-019 | Adopted for post-M5 fresh runs; no token ceiling from providers that cannot report usage. | Architecture 17. |
| Taxonomy version and MCP name supersession | RSN-016, MCP | The taxonomy value `model-capability-taxonomy-v1` and the `reasoning_input_contract` field name are owned by architecture 22; the concept2 `MandateMcpCapabilitySourceDto`/`DiscoveryDto`/`CapabilityRevisionDto` names are superseded by the `V1` records in architecture 18. | Architectures 22 and 18. |
| Accepted execution directions | CFG-009..010, RSN-018..020, FRK-019..022, CHR-011..013, GOL-013, MCP-017, ACT-017 | Raw-TOML/configuration editing, model discovery, arbitrary headers, provider-native preservation, server-side parser, fork tool-result/child-agent execution, export, cross-workspace clone/rebind, autonomous harness goal mode, post-disconnect work, and RLM packaging are adopted for post-M5 execution under ADR 0033. | Architectures 25, 22, 23, 26, 28, 18, 24, and 29. |

Every selected heading is either represented by a topic family above or is
context-only explanatory material. This index does not alter the concept.


## Coverage status

This index maps immutable concept headings to derivative owner documents. A
mapping is provenance, not implementation approval. Each entry must be marked
`Mapped`, `Context-only`, or `Unmapped`; broad family ranges do not establish
claim-level completeness. Conflicting retained concept prose is classified as
historical/ordinary where the owner architecture explicitly supersedes it for
future Mandate work. See the [evidence register](evidence-register.md) and
[deferred/excluded register](deferred-excluded-register.md) for evidence and
non-scope claims.
