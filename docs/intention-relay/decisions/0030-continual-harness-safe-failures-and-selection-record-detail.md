# ADR 0030: Post-M5 Continual-Harness Closed Safe Failures and Selection-Record Detail

## Status

Accepted 2026-08-30. This decision records the continual-harness closed safe
failures and the `ContinualHarnessSelectionV1` nested-record content as accepted
future directions. It does not activate implementation: the detail is bound to
[Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment)
and is implemented only through a separately accepted activating specification
at the start of that milestone.

## Decision

The following detail from
[`m4plus_concept2.md`](../m4plus_concept2.md) is adopted and owned by the
respective authoritative packages:

- **Architecture 26 (continual harness)**: the 15 closed `harness_*` safe
  failures through `ErrorDto` (`harness_rule_limit_exceeded`,
  `harness_source_limit_exceeded`, `harness_concurrency_limit_exceeded`,
  `harness_interval_too_short`, `harness_schedule_invalid`,
  `harness_trigger_cycle`, `harness_dossier_too_large`,
  `harness_source_unavailable`, `harness_checkpoint_too_large`,
  `harness_checkpoint_unavailable`, `harness_result_too_large`,
  `harness_not_active`, `harness_archived`, `harness_revision_conflict`,
  `harness_cause_chain_limit_exceeded`) and their disclosure rule; and
- **Architectures 26 and 28 (harness model and run-execution meaning)**: the
  nested content of `ContinualHarnessSelectionV1` carried by
  `run-execution-meaning-v4` `harness_selection` (harness identity, active rule
  revision, durable trigger reason, class resolution, dossier digest,
  checkpoint reference, time-zone application, and immutable bounds).

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after
a later activating specification, and is bound to Milestone 5+ in the roadmap.

## Rationale

The authoritative package review of 2026-08-30 confirmed the 15 `harness_*`
closed safe failures and the `ContinualHarnessSelectionV1` nested-record content
are present in `m4plus_concept2.md` but absent from the authoritative
documentation: architecture 26 defines the harness model and its bounds but
carries no closed failure list, and the run-execution-meaning record is
referenced by name only. This decision adopts the detail so the authoritative
documentation fully covers the features, while preserving the project rule that
no feature is documented as implemented without code evidence.

## Normative invariants

1. Every `harness_*` failure is a known typed pre-effect rejection; no content
   is truncated or partly committed, and no external provider, tool, kernel,
   process, network, or scheduler action occurs in the transition transaction.
2. The failures disclose no credential, path, dossier content, Python value,
   grant, provider resource, process topology, or raw transcript.
3. `ContinualHarnessSelectionV1` is a separately versioned credential-free
   nested record of `run-execution-meaning-v4`; historical M4 and other
   non-harness runs acquire no synthetic harness record.
4. Harness bounds and failures are intrinsic/capacity/product-classified and
   never become Mandate admission quotas or child-graph limits.

## Failure semantics

- A limit failure (rules, sources, concurrency, chain depth, dossier,
  checkpoint, or result) is a known typed pre-effect rejection that retains the
  coalesced reason where applicable.
- `harness_not_active` and `harness_archived` reject operations against a
   non-active or archived rule; `harness_revision_conflict` rejects changed
   revision reuse; `harness_trigger_cycle` rejects a completion-cause cycle.
- A missing or oversized checkpoint is `harness_checkpoint_unavailable` or
   `harness_checkpoint_too_large` and never reruns the producing cell or run.

## Compatibility and supersession

This decision supersedes the absence of the closed safe failures and the
selection-record content in the principle-level text of architectures 26 and 28.
The closed M4 baseline, M3/M4 bytes, and existing behavior remain unchanged.
Activation remains deferred: no code changes are authorized by this decision.

## Security and residual risk

The detail remains trusted-local. Failures and selection records are bounded
and credential-free; redaction stays central and every activating specification
must pass the fake-secret regression suite.

## Affected documents

- [`architecture/11-implementation-roadmap.md`](../architecture/11-implementation-roadmap.md)
- [`architecture/26-continual-harness.md`](../architecture/26-continual-harness.md)
- [`architecture/28-goal-domain-and-verification.md`](../architecture/28-goal-domain-and-verification.md)
- [`reconciliation/README.md`](../reconciliation/README.md)
- [`reconciliation/source-of-truth-matrix.md`](../reconciliation/source-of-truth-matrix.md)
- [`reconciliation/contradiction-register.md`](../reconciliation/contradiction-register.md)
- [`reconciliation/concept-supersession-index.md`](../reconciliation/concept-supersession-index.md)
- [`reconciliation/deferred-excluded-register.md`](../reconciliation/deferred-excluded-register.md)
- [`reconciliation/evidence-register.md`](../reconciliation/evidence-register.md)
- [`reconciliation/compatibility-register.md`](../reconciliation/compatibility-register.md)
- [`decisions/README.md`](README.md)

## Required evidence

No implementation evidence is claimed. The activating specification must declare
exact crates, DTO/wire/storage versions, feature profiles, coverage tiers,
fixtures, and outcome evidence, and pass `make quick`, `make verify`, and
Linux/Windows CI before acceptance.

## Non-goals

This decision does not implement the detail; it does not change M3/M4 behavior;
it does not renumber M5--M9; it does not activate a crate, schema, migration,
protocol, or feature. Autonomous continuation, post-disconnect requeue,
multimodal payloads, plugin/MCP installation, process supervision, and
deletion/GC remain excluded pending separate future decisions.
