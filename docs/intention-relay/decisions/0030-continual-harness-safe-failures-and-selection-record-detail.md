# 0030: Post-M5 Continual-Harness Closed Safe Failures and Selection-Record Detail

## Status

Accepted 2026-08-30. Not implemented; activation requires an activating specification.

## Decision

The following detail from `m4plus_concept.md` is adopted and owned by the respective authoritative packages:

- **Architecture 26 (continual harness)**: the 15 closed `harness_*` safe
failures through `ErrorDto` (`harness_rule_limit_exceeded`, `harness_source_limit_exceeded`,
`harness_concurrency_limit_exceeded`, `harness_interval_too_short`, `harness_schedule_invalid`, `harness_trigger_cycle`,
`harness_dossier_too_large`, `harness_source_unavailable`, `harness_checkpoint_too_large`,
`harness_checkpoint_unavailable`, `harness_result_too_large`, `harness_not_active`, `harness_archived`,
`harness_revision_conflict`, `harness_cause_chain_limit_exceeded`) and their disclosure rule; and
- **Architectures 26 and 28 (harness model and run-execution meaning)**: the
nested content of `ContinualHarnessSelectionV1` formerly carried by the `run-execution-meaning-v4` `harness_selection`
field (harness identity, active rule revision, durable trigger reason, class resolution, dossier digest, checkpoint
reference, time-zone application, and immutable bounds). The former carrier is removed with the canonical codec by [ADR
0046](0046-typed-serde-json-contracts.md); the selection record survives as a typed serde JSON record owned by
architecture 26 (selection record and closed safe failures), carried by the run's immutable selection records owned by
architecture 28.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after its own activating specification, and
remains bound to Milestone 5+.

## Normative invariants

1. Every `harness_*` failure is a known typed pre-effect rejection; no content
is truncated or partly committed, and no external provider, tool, kernel, process, network, or scheduler action occurs
in the transition transaction.
2. The failures disclose no credential, path, dossier content, Python value,
grant, provider resource, process topology, or raw transcript.
3. `ContinualHarnessSelectionV1` is a separately versioned credential-free
nested record of the typed run-execution selection; historical M4 and other non-harness runs acquire no synthetic
harness record.
4. Harness bounds and failures are intrinsic/capacity/product-classified and
never become Mandate admission quotas or child-graph limits.

## Failure semantics

- A limit failure (rules, sources, concurrency, chain depth, dossier,
checkpoint, or result) is a known typed pre-effect rejection that retains the coalesced reason where applicable.
- `harness_not_active` and `harness_archived` reject operations against a
non-active or archived rule; `harness_revision_conflict` rejects changed revision reuse; `harness_trigger_cycle` rejects
a completion-cause cycle.
- A missing or oversized checkpoint is `harness_checkpoint_unavailable` or
`harness_checkpoint_too_large` and never reruns the producing cell or run.

## Rationale

The 15 `harness_*` closed safe failures and the `ContinualHarnessSelectionV1` nested-record content were present in
`m4plus_concept.md` but absent from the authoritative documentation: architecture 26 defined the harness model and its
bounds without a closed failure list, and the run-execution-meaning record was referenced by name only. Adopting the
detail makes the authoritative documentation cover the features without documenting any part of them as implemented.

## Compatibility and non-goals

This decision supersedes the absence of the closed safe failures and the selection-record content in the principle-level
text of architectures 26 and
28. The closed M4 baseline, M3/M4 bytes, and existing behavior remain
unchanged, and no code changes are authorized by this decision.

Autonomous continuation, post-disconnect requeue, multimodal payloads, plugin/MCP installation, process supervision, and
deletion/GC remain excluded pending separate future decisions. M5-M9 are not renumbered.

Owner: architectures 26 and 28. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
