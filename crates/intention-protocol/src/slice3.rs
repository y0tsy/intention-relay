//! Slice 3 daemon-facade projection surface.
//!
//! Owner: ADR 0044. Slice 3 adds no negotiated capability, no envelope, and no
//! new wire family: the selections ride the single live
//! `run-execution-meaning-v4` record and the daemon facade's existing typed
//! commands and queries. This module is the public protocol path through which
//! the typed Slice 3 records owned by `intention-domain` are reachable for
//! protocol and client consumers: harness rules and triggers, the closed
//! programmatic-caller admission decisions and confirmations, Goal
//! identity/readiness/gates, and the delegated verification authority and
//! verdicts.
//!
//! Every item here is a re-export. `intention-domain` remains the sole owner of
//! the canonical records, validation codes, bounds, digests, and numeric ledger
//! tags; this module adds no numeric mirror, no parallel DTO shape, and no
//! second validation rule.

pub use intention_domain::goal_domain::{
    GOAL_FAILURE_CODES, GoalDto, GoalEvidenceKindV1, GoalEvidenceReferenceV1, GoalGateDefinitionV1,
    GoalLifecycleStateDto, GoalReadinessStateDto, GoalScopeDto, GoalUserDecisionStateDto,
    VerificationGateDto, validate_goal_readiness_claim,
};
pub use intention_domain::harness::{
    HARNESS_CLOSED_SAFE_FAILURES, HarnessCalendarScheduleV1, HarnessCompletionLinkV1,
    HarnessIntervalScheduleV1, HarnessPresentationModeV1, HarnessRuleLifecycleStateV1,
    HarnessRuleOperationV1, HarnessRuleRevisionV1, HarnessRuleScopeV1, HarnessRuleSourceV1,
    HarnessRuleV1, HarnessTaskModeV1, HarnessTerminalOutcomeV1, HarnessTriggerRecordV1,
    apply_harness_rule_operation, validate_harness_rule_operation, validate_harness_time_zone,
};
pub use intention_domain::programmatic_policy::{
    PROGRAMMATIC_POLICY_FAILURE_CODES, ProgrammaticAdmissionRuleV1,
    ProgrammaticAuthorizationCorridorV1, ProgrammaticCalendarPeriodKindV1,
    ProgrammaticCallerPolicyLifecycleStateV1, ProgrammaticCallerPolicyScopeV1,
    ProgrammaticCallerPolicyV1, ProgrammaticConfirmationBindingV1, ProgrammaticExactConfirmationV1,
    ProgrammaticMcpMethodReferenceV1, ProgrammaticRunLimitsV1,
};
pub use intention_domain::run_execution_meaning::{
    DisabledOr, FixedRunLimits, ProgrammaticCallerPolicySelectionV1, RunExecutionMeaningV4Record,
};
pub use intention_domain::slice3_selections::{
    ContinualHarnessSelectionV1, GoalCardReferenceV1, GoalGateRevisionReferenceV1,
    GoalRevisionReferenceV1, GoalRunKindV1, GoalRunSelectionBoundsV1, GoalRunSelectionV1,
    GoalScopeLinkProvenanceV1, HarnessClassResolutionV1, HarnessExecutionClassV1,
    HarnessSelectionBoundsV1, HarnessSourceKindV1, HarnessTriggerReasonV1, MAX_SLICE3_TEXT_CHARS,
    McpMethodCatalogSelectionV1, ProgrammaticCallerRootOriginV1,
};
pub use intention_domain::verification::{
    UserLifecycleOperationV1, VerificationAuditEvidenceDto, VerificationAuditVerdictDto,
    VerificationMandateAuthorityDto, VerificationVerdictDto, VerifierAuthorityReferenceV1,
    VerifierContractReferenceV1, VerifierEvidenceKindV1, VerifierGoalReferenceV1,
    VerifierOperationV1, VerifierReconciliationOutcomeV1, VerifierTargetReferenceV1,
    VerifierTargetSetReferenceV1,
};
