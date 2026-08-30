# ADR 0022: Post-M5 Programmatic-Caller Policy Directions

## Status

Accepted 2026-08-30. This decision records the programmatic-caller policy and
admission model as an accepted future direction. It does not activate
implementation: the model is bound to
[Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment)
and is implemented only through a separately accepted activating specification
at the start of that milestone.

## Decision

The programmatic-caller policy and admission model from
[`m4plus_concept2.md`](../m4plus_concept2.md) is adopted as the accepted future
direction and owned by the new [Programmatic Caller Policy and Admission](../architecture/27-programmatic-caller-policy-and-admission.md)
package:

- two closed root origins (`InteractiveUser`, `ContinualHarness`) with no third
  root, and immutable `ProgrammaticCallerProvenanceDto` audit records created
  before `ToolCallStarted`;
- durable policy identity/scope/narrowing (`Project`/`Goal`/`Session`) with
  intersection of effect selectors and exact tool/MCP selectors,
  most-restrictive-wins, child-narrowing-only, and fork shared calendar
  counters;
- closed admission decisions (`Prohibited`, `DirectLocalRead`,
  `ExactConfirmationRequired`, `BoundedConfirmationRequired`) with the
  `InteractiveLocalReadBaselineV1` (256/16), exact confirmation bound to one
  `ToolCallId`, and bounded `ProgrammaticAuthorizationCorridorDto`;
- policy lifecycle (`Active`/`Suspended`/`Revoked`/`Archived`) with live
  tightening, drafts, and no-reactivation-after-revoke;
- run and calendar limits with atomic reservations, idempotent equal-replay,
  release-on-known-pre-effect, permanent-on-start, and
  `InterruptedBeforeStart`/`ExternalEffectUnknown` recovery;
- `ProgrammaticCallerPolicySelectionV1` in `run-execution-meaning-v3`/v4 with
  `Disabled` only for historical M4, and 22 closed `ErrorDto` safe failures.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after
a later activating specification, and is bound to Milestone 5+ in the roadmap.

## Rationale

The authoritative package review of 2026-08-30 confirmed the programmatic-caller
policy is present in `m4plus_concept2.md` but absent from the authoritative
documentation: the term "programmatic" appears nowhere in `architecture/`,
`decisions/`, or `reconciliation/`, and the supersession index, deferred/excluded
register, and contradiction register have no row for it. Its "Historical-only
for new Mandate work" marking exists only inside the concept document. This
decision adopts the model as an accepted future direction for ordinary future
work so the authoritative documentation no longer leaves the feature unmapped,
while preserving the project rule that no feature is documented as implemented
without code evidence.

## Normative invariants

1. Every programmatic action has one daemon-assigned root origin; no third root
   exists in this first scope.
2. The policy is logical product control and audit evidence, not an OS security
   boundary; it does not create a durable autonomous actor, second daemon,
   second registry, remote identity, or authority surviving an active run.
3. Effective policy is the most-restrictive intersection of effect selectors
   and exact tool/MCP selectors; children and harness classes may only narrow.
4. `execute` never receives `DirectLocalRead` or bounded-corridor admission in
   this first scope; `fetch_url` and `mcp` never receive direct admission.
5. A corridor belongs to one active root tree, expires on terminalization or
   policy revocation, and is never a lasting policy revision.
6. Reservations are created atomically before `ToolCallStarted` or not at all;
   on start they become permanent consumption; recovery never recreates a
   reservation by rerunning an action.
7. Historical M4 and earlier selections retain `Disabled` policy selection and
   are never rewritten or given synthetic policy state.

## Failure semantics

- Limit, snapshot, revision, origin, corridor, counter, reservation, and draft
  failures are known typed pre-effect rejections.
- A started effect without durable terminal proof remains `ExternalEffectUnknown`
  and is never retried.
- Live suspension/revocation imposes stricter present-time denial but never
  rewrites historical semantics or resumes external work.

## Compatibility and supersession

This decision supersedes the absence of a disposition for the programmatic-caller
policy in the reconciliation registers. For new Mandate work, retained RLM
run-rooted activity identity, root-origin, direct-pair queue, and fixed
observation limits remain historical-only where they conflict, consistent with
architectures 17 and 24. The closed M4 baseline, M3/M4 bytes, and existing
behavior remain unchanged. Activation remains deferred: no code changes are
authorized by this decision.

## Security and residual risk

The policy remains trusted-local. Provenance, corridors, snapshots, and
failures are bounded and credential-free; redaction stays central and every
activating specification must pass the fake-secret regression suite.

## Affected documents

- [`architecture/11-implementation-roadmap.md`](../architecture/11-implementation-roadmap.md)
- [`architecture/27-programmatic-caller-policy-and-admission.md`](../architecture/27-programmatic-caller-policy-and-admission.md) (new)
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

This decision does not implement the policy; it does not change M3/M4 behavior;
it does not renumber M5--M9; it does not activate a crate, schema, migration,
protocol, or feature. A typed command-template direction, a new policy decoder,
and an OS security boundary remain outside this decision.
