//! Slice 3 Goal runtime: goal admission, gate ordering, proposal flows.
//!
//! Owner: architecture 28 with ADR 0044. This module owns the application
//! transactions that admit goal-directed runs atomically, execute gate
//! ordering, drive memory/Skill/role/template mutations, proposal flows over
//! the existing `ask_user` path, and compaction working-form updates.
//!
//! # Boundary
//!
//! Every value that crosses this boundary is a daemon-assigned identity, an
//! exact revision, a bound, a canonical digest, a closed decision, or a
//! bounded credential-free and path-free scalar. Raw Goal bodies, gate command
//! lines, workspace paths, grants, credentials, provider resources, verifier
//! evidence payloads, and full card content never cross it: full records are
//! reached only through an explicit reveal of their exact typed retained-content
//! reference.
//!
//! # Frozen validation ownership
//!
//! The durable transaction is the only admission authority: each flow loads
//! the frozen repository records, validates them through the
//! `intention-domain` Goal and verification validators, and then commits once.
//! No current state is reconstructed from a selection: an unknown, stale,
//! unavailable, incompatible, or over-limit selected record fails closed
//! before the repository call. A leading-goal run is admitted only through
//! [`GoalRunAdmissionPort`], which commits the run, selection, and evidence
//! atomically or not at all.
//!
//! # Restart recovery
//!
//! Recovery reads the durable verification records and reports that no
//! verifier evidence work is replayed and no committed mutation is
//! re-applied; a later attempt obtains a new admission identity.

use intention_domain::canonical::{Digest256, contains_control_or_nul, contains_credential_shape};
use intention_domain::goal_domain::{
    GOAL_ACCEPTANCE_EXCEPTION_INVALID, GOAL_MAX_SESSION_LINKS_PER_PROJECT_GOAL,
    GoalAcceptanceDecisionV1, GoalAcceptanceRequestV1, GoalApplicabilityContextV1,
    GoalCompactionOriginV1, GoalCompactionWorkingFormV1, GoalEvidenceKindV1,
    GoalEvidenceReferenceV1, GoalGateExceptionKindV1, GoalGateExceptionV1, GoalGateInputFamilyV1,
    GoalGateOutcomeDispositionV1, GoalGateOutcomeV1, GoalGateTemplateCardV1,
    GoalInheritedExceptionV1, GoalMemoryCardV1, GoalMemoryReplacementLinkV1,
    GoalMemoryRollbackLinkV1, GoalMilestoneDto as GoalMilestoneV1, GoalParentLinkDto,
    GoalReadinessStateDto, GoalRecordScopeDto as GoalRecordScopeV1, GoalRevisionDto,
    GoalRoleCardV1, GoalRoleClassV1, GoalSessionLinkDto, GoalSkillCardV1, GoalTemplateProvenanceV1,
    GoalTemplateScopeDto as GoalTemplateScopeV1, GoalUserDecisionStateDto, RefinementDecisionV1,
    RefinementDraftDto, RefinementEditV1, VerificationGateDto, apply_required_gate_outcome,
    coalesce_refinement_draft, resolve_refinement_draft, validate_compaction_extension,
    validate_conversation_summary_correction, validate_fork_summary_inheritance,
    validate_gate_execution_preconditions, validate_gate_outcome, validate_gate_template_creation,
    validate_gate_template_lifecycle_transition, validate_gate_template_scope,
    validate_goal_acceptance, validate_goal_allocation, validate_goal_archive,
    validate_goal_child_link, validate_goal_direct_children, validate_goal_gate_definitions,
    validate_goal_lifecycle_transition, validate_goal_no_cycle, validate_goal_readiness_claim,
    validate_goal_restore, validate_goal_run_admission, validate_goal_tree_attachment,
    validate_memory_card_set, validate_memory_disclosure, validate_memory_replacement,
    validate_memory_rollback, validate_pending_draft_limit, validate_reference_gate_evidence,
    validate_role_narrowing, validate_skill_disclosure, validate_skill_role_card_set,
};
use intention_domain::slice3_selections::{
    GoalGateRevisionReferenceV1, GoalRunKindV1, GoalRunSelectionV1,
};
use intention_domain::verification::{
    VerificationAuditEvidenceDto, VerificationAuditVerdictDto, VerificationMandateAuthorityDto,
    VerificationVerdictDto, VerifierAuditBaselineV1, VerifierAuditEvidenceV1,
    VerifierAuditVerdictRecordV1, VerifierAuthorityConsumptionRuleV1,
    VerifierAuthorityConsumptionV1, VerifierAuthorityLifecycleV1, VerifierAuthorityReferenceV1,
    VerifierAuthorityV1, VerifierCommittedStateV1, VerifierContractReferenceV1,
    VerifierEvidenceKindV1, VerifierFrozenGoalGateEvidenceReferencesV1, VerifierGoalReferenceV1,
    VerifierOperationIdentityV1, VerifierOperationPreconditionContextV1, VerifierOperationV1,
    VerifierReconciliationOutcomeV1, VerifierTargetLifecycleV1, VerifierTargetMutationV1,
    VerifierTargetReferenceV1, VerifierTargetRevisionAndSequenceV1, VerifierTargetSetReferenceV1,
    validate_baseline_freshness, validate_verifier_authority_reference,
    validate_verifier_authority_use, validate_verifier_operation_preconditions,
};
use intention_storage::goal_repo::{
    AppendGoalRevisionInputDto, ApplyVerifierMutationOutcomeDto, AttachGoalChildInputDto,
    ConversationSummaryRecordDto, CreateGoalGateInputDto, CreateGoalInputDto,
    CreateGoalSessionLinkInputDto, GoalCardRepositoryDto, GoalCompactionRepositoryDto,
    GoalCompactionWorkingFormRecordDto, GoalEvidenceKindDto, GoalEvidenceReferenceDto,
    GoalGateDefinitionDto, GoalGateExceptionDto, GoalGateExceptionKindDto, GoalGateInputFamilyDto,
    GoalGateOutcomeDispositionDto, GoalGateOutcomeKindDto, GoalGateRecordDto,
    GoalGateRepositoryDto, GoalGateResultRecordDto, GoalGateRevisionReferenceDto,
    GoalGateTemplateRecordDto, GoalLifecycleStateDto as GoalLifecycleRecordDto,
    GoalMemoryCardRecordDto, GoalMemoryCardReplacementInputDto, GoalMemoryCardRollbackInputDto,
    GoalMilestoneDto as GoalMilestoneRecordDto, GoalParentLinkRecordDto, GoalProposalRepositoryDto,
    GoalReadinessStateDto as GoalReadinessRecordDto, GoalRecordDto, GoalRecordScopeDto,
    GoalRepositoryDto, GoalRevisionRecordDto, GoalRoleCardRecordDto, GoalRoleClassDto,
    GoalScopeDto as GoalScopeRecordDto, GoalSessionLinkRecordDto, GoalSkillCardRecordDto,
    GoalTemplateLifecycleStateDto, GoalTemplateProvenanceKindDto, GoalTemplateScopeDto,
    GoalUserDecisionStateDto as GoalUserDecisionRecordDto, GoalVerificationRepositoryDto,
    MemoryKindDto as MemoryKindRecordDto, RecordCompactionSuffixReferenceInputDto,
    RecordGoalUserDecisionInputDto, RefinementDraftRecordDto, RefinementDraftStateDto,
    RefinementEditKindDto, RefinementEditRecordDto, RevokeVerifierAuthorityInputDto,
    SetGoalReadinessInputDto, TransitionGoalGateTemplateInputDto, TransitionGoalLifecycleInputDto,
    VerifierAuditBaselineRecordDto, VerifierAuditEvidenceRecordDto, VerifierAuditVerdictRecordDto,
    VerifierAuthorityConsumptionRuleDto, VerifierAuthorityConsumptionStateDto,
    VerifierAuthorityRecordDto, VerifierAuthorityReferenceDto, VerifierContractReferenceDto,
    VerifierEvidenceKindDto, VerifierFrozenReferencesDto, VerifierOperationDto,
    VerifierOperationIdentityDto, VerifierTargetLifecycleDto, VerifierTargetMutationRecordDto,
    VerifierTargetReferenceDto,
};
use intention_types::{DtoResult, ErrorDto};

use crate::programmatic_policy::{
    derived_identity, digest_bytes, digest_text, identity_bytes, identity_text, push_framed,
};

/// The repository-wide safe credential rejection code.
const CREDENTIALS_FORBIDDEN: &str = "credentials_forbidden";

/// The maximum characters of one boundary reference scalar.
const MAX_BOUNDARY_TEXT_CHARS: usize = 256;

/// Builds one typed pre-effect Goal rejection.
fn goal_error(code: &'static str, message: &'static str) -> ErrorDto {
    ErrorDto::validation(code, message)
}

/// Parses one durable reference as a canonical daemon-assigned identity.
///
/// # Errors
///
/// Returns `code` for a value that is not canonical identity text.
fn goal_identity(value: &str, code: &'static str) -> DtoResult<[u8; 16]> {
    identity_bytes(value).map_err(|_| {
        goal_error(
            code,
            "a durable Goal reference is a canonical daemon-assigned identity",
        )
    })
}

/// Validates one bounded credential-free, path-free boundary reference.
///
/// # Errors
///
/// Returns `invalid_code` for a blank, control-bearing, or path-shaped value,
/// `credentials_forbidden` for a credential-shaped value, and
/// `too_large_code` for a value over the closed scalar bound.
fn validate_goal_scalar(
    value: &str,
    invalid_code: &'static str,
    too_large_code: &'static str,
) -> DtoResult<()> {
    if value.trim().is_empty() || contains_control_or_nul(value) {
        return Err(goal_error(
            invalid_code,
            "a Goal-domain reference must be non-blank and control-free",
        ));
    }
    if value.chars().count() > MAX_BOUNDARY_TEXT_CHARS {
        return Err(goal_error(
            too_large_code,
            "a Goal-domain reference stays inside its closed scalar bound",
        ));
    }
    if contains_credential_shape(value) {
        return Err(goal_error(
            CREDENTIALS_FORBIDDEN,
            "credentials are forbidden",
        ));
    }
    if names_filesystem_path(value) {
        return Err(goal_error(
            invalid_code,
            "a filesystem path never crosses the Goal-domain boundary",
        ));
    }
    Ok(())
}

/// Whether one boundary scalar names a filesystem path.
///
/// A gate template names a registered capability and a closed typed input
/// family, never an arbitrary path, URL, header map, or executable text, so
/// path-shaped input is refused at this boundary.
fn names_filesystem_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with('\\')
        || value.contains("..")
        || value.as_bytes().get(1).is_some_and(|byte| *byte == b':')
}

/// Computes the deterministic digest of one immutable Goal revision.
fn goal_revision_digest(record: &GoalRevisionRecordDto) -> Digest256 {
    let mut input = Vec::new();
    push_framed(&mut input, "goal-revision-v1");
    push_framed(&mut input, &record.goal_id);
    push_framed(&mut input, &record.revision.to_string());
    push_framed(&mut input, &record.title);
    push_framed(&mut input, &record.objective);
    for reference in &record.inherited_rule_references {
        push_framed(&mut input, reference);
    }
    for reference in &record.local_rule_references {
        push_framed(&mut input, reference);
    }
    for reference in &record.required_gate_references {
        push_framed(&mut input, &reference.gate_id);
        push_framed(&mut input, &reference.revision.to_string());
    }
    Digest256::sha256(&input)
}

/// Computes the deterministic digest of one obligatory parent-to-child link.
fn goal_link_digest(parent_goal_id: &str, child_goal_id: &str, child_revision: u64) -> Digest256 {
    let mut input = Vec::new();
    push_framed(&mut input, "goal-parent-link-v1");
    push_framed(&mut input, parent_goal_id);
    push_framed(&mut input, child_goal_id);
    push_framed(&mut input, &child_revision.to_string());
    Digest256::sha256(&input)
}

/// Computes the deterministic digest of one explicit session link.
fn goal_session_link_digest(
    link_id: &str,
    project_goal_id: &str,
    session_id: &str,
    effective_from_revision: u64,
) -> Digest256 {
    let mut input = Vec::new();
    push_framed(&mut input, "goal-session-link-v1");
    push_framed(&mut input, link_id);
    push_framed(&mut input, project_goal_id);
    push_framed(&mut input, session_id);
    push_framed(&mut input, &effective_from_revision.to_string());
    Digest256::sha256(&input)
}

/// Computes the deterministic digest of one gate definition revision.
fn goal_gate_digest(gate_id: &str, goal_id: &str, definition: &GoalGateDefinitionDto) -> Digest256 {
    let mut input = Vec::new();
    push_framed(&mut input, "goal-gate-v1");
    push_framed(&mut input, gate_id);
    push_framed(&mut input, goal_id);
    push_framed(&mut input, definition.kind_name());
    match definition {
        GoalGateDefinitionDto::Reference {
            evidence_contract_revision,
            accepted_reference_kinds,
        } => {
            push_framed(&mut input, &evidence_contract_revision.to_string());
            for kind in accepted_reference_kinds {
                push_framed(&mut input, kind.name());
            }
        }
        GoalGateDefinitionDto::Executable {
            template_id,
            template_revision,
        } => {
            push_framed(&mut input, template_id);
            push_framed(&mut input, &template_revision.to_string());
        }
    }
    Digest256::sha256(&input)
}

/// Computes the deterministic digest of one gate template card.
fn goal_template_digest(record: &GoalGateTemplateRecordDto) -> Digest256 {
    let mut input = Vec::new();
    push_framed(&mut input, "goal-gate-template-v1");
    push_framed(&mut input, &record.template_id);
    push_framed(&mut input, &record.revision.to_string());
    push_framed(&mut input, record.scope.kind_name());
    push_framed(&mut input, record.scope.owner_id());
    push_framed(&mut input, &record.capability_reference);
    push_framed(&mut input, record.input_family.name());
    push_framed(
        &mut input,
        if record.requires_confirmation {
            "1"
        } else {
            "0"
        },
    );
    push_framed(&mut input, record.provenance_kind.name());
    if let Some(draft_id) = &record.provenance_draft_id {
        push_framed(&mut input, draft_id);
    }
    Digest256::sha256(&input)
}

/// Computes the deterministic digest of one gate evaluation result.
fn goal_gate_result_digest(record: &GoalGateResultRecordDto) -> Digest256 {
    let mut input = Vec::new();
    push_framed(&mut input, "goal-gate-result-v1");
    push_framed(&mut input, &record.gate_id);
    push_framed(&mut input, &record.gate_revision.to_string());
    push_framed(&mut input, &record.producing_run_id);
    push_framed(&mut input, record.outcome_kind.name());
    if let Some(evidence) = &record.evidence {
        push_framed(&mut input, &evidence.evidence_id);
        push_framed(&mut input, &evidence.revision.to_string());
        push_framed(&mut input, evidence.kind.name());
    }
    Digest256::sha256(&input)
}

/// Computes the deterministic digest of one memory card revision.
fn goal_memory_card_digest(record: &GoalMemoryCardRecordDto) -> Digest256 {
    let mut input = Vec::new();
    push_framed(&mut input, "goal-memory-card-v1");
    push_framed(&mut input, &record.record_id);
    push_framed(&mut input, &record.revision.to_string());
    push_framed(&mut input, record.kind.name());
    push_framed(&mut input, record.scope.kind_name());
    push_framed(&mut input, record.scope.owner_id());
    push_framed(&mut input, &record.title);
    push_framed(&mut input, &record.safe_purpose);
    push_framed(&mut input, &record.retained_content_reference);
    Digest256::sha256(&input)
}

/// Computes the deterministic digest of one Skill card revision.
fn goal_skill_card_digest(record: &GoalSkillCardRecordDto) -> Digest256 {
    let mut input = Vec::new();
    push_framed(&mut input, "goal-skill-card-v1");
    push_framed(&mut input, &record.skill_id);
    push_framed(&mut input, &record.revision.to_string());
    push_framed(&mut input, &record.canonical_name);
    push_framed(&mut input, &record.description);
    push_framed(&mut input, record.owner_scope.kind_name());
    push_framed(&mut input, record.owner_scope.owner_id());
    push_framed(&mut input, &record.content_reference);
    Digest256::sha256(&input)
}

/// Computes the deterministic digest of one role card revision.
fn goal_role_card_digest(record: &GoalRoleCardRecordDto) -> Digest256 {
    let mut input = Vec::new();
    push_framed(&mut input, "goal-role-card-v1");
    push_framed(&mut input, &record.role_id);
    push_framed(&mut input, &record.revision.to_string());
    push_framed(&mut input, &record.canonical_name);
    push_framed(&mut input, &record.task);
    push_framed(&mut input, record.permitted_class.name());
    for tool in &record.tool_subset {
        push_framed(&mut input, tool);
    }
    push_framed(&mut input, &record.context_limit_bytes.to_string());
    push_framed(&mut input, &record.result_limit_bytes.to_string());
    Digest256::sha256(&input)
}

/// Computes the deterministic digest of one conversation summary revision.
fn goal_summary_digest(record: &ConversationSummaryRecordDto) -> Digest256 {
    let mut input = Vec::new();
    push_framed(&mut input, "goal-conversation-summary-v1");
    push_framed(&mut input, &record.summary_id);
    push_framed(&mut input, &record.revision.to_string());
    push_framed(&mut input, record.scope.kind_name());
    push_framed(&mut input, record.scope.owner_id());
    if let Some(previous) = &record.previous_summary_reference {
        push_framed(&mut input, previous);
    }
    push_framed(&mut input, &record.source_range_start);
    push_framed(&mut input, &record.source_range_end);
    push_framed(&mut input, &record.safe_content);
    Digest256::sha256(&input)
}

/// Computes the deterministic admission identity of one goal-directed run.
fn goal_admission_reference(
    leading_goal_id: &str,
    goal_revision: u64,
    run_kind: GoalRunKindV1,
    target_snapshot_digest: Digest256,
) -> [u8; 16] {
    let mut input = Vec::new();
    push_framed(&mut input, "goal-run-admission-v1");
    push_framed(&mut input, leading_goal_id);
    push_framed(&mut input, &goal_revision.to_string());
    push_framed(
        &mut input,
        match run_kind {
            GoalRunKindV1::GoalDirectedOrdinary => "goal_directed_ordinary",
            GoalRunKindV1::VerificationOnly => "verification_only",
        },
    );
    push_framed(&mut input, &target_snapshot_digest.to_string());
    derived_identity(Digest256::sha256(&input))
}

/// Converts one durable Goal scope into its domain value.
///
/// # Errors
///
/// Returns `goal_not_active` for a non-canonical scope identity.
fn goal_scope_from_storage(
    scope: &GoalScopeRecordDto,
) -> DtoResult<intention_domain::goal_domain::GoalScopeDto> {
    use intention_domain::goal_domain::GoalScopeDto;
    Ok(match scope {
        GoalScopeRecordDto::Project { project_id } => GoalScopeDto::Project {
            project_id: goal_identity(project_id, "goal_not_active")?,
        },
        GoalScopeRecordDto::Session {
            project_id,
            session_id,
        } => GoalScopeDto::Session {
            project_id: goal_identity(project_id, "goal_not_active")?,
            session_id: goal_identity(session_id, "goal_not_active")?,
        },
    })
}

/// Converts one durable memory-card owner scope into its domain value.
///
/// # Errors
///
/// Returns `memory_reference_unavailable` for a non-canonical scope identity.
fn record_scope_from_storage(scope: &GoalRecordScopeDto) -> DtoResult<GoalRecordScopeV1> {
    Ok(match scope {
        GoalRecordScopeDto::Project { project_id } => GoalRecordScopeV1::Project {
            project_id: goal_identity(project_id, "memory_reference_unavailable")?,
        },
        GoalRecordScopeDto::Goal { goal_id } => GoalRecordScopeV1::Goal {
            goal_id: goal_identity(goal_id, "memory_reference_unavailable")?,
        },
        GoalRecordScopeDto::Session { session_id } => GoalRecordScopeV1::Session {
            session_id: goal_identity(session_id, "memory_reference_unavailable")?,
        },
    })
}

/// Converts one durable Goal lifecycle state into its domain value.
const fn lifecycle_from_storage(
    state: GoalLifecycleRecordDto,
) -> intention_domain::goal_domain::GoalLifecycleStateDto {
    use intention_domain::goal_domain::GoalLifecycleStateDto::{
        Active, Archived, NeedsRework, Paused, Stopped,
    };
    match state {
        GoalLifecycleRecordDto::Active => Active,
        GoalLifecycleRecordDto::NeedsRework => NeedsRework,
        GoalLifecycleRecordDto::Paused => Paused,
        GoalLifecycleRecordDto::Stopped => Stopped,
        GoalLifecycleRecordDto::Archived => Archived,
    }
}

/// Converts one domain Goal lifecycle state into its storage value.
const fn lifecycle_to_storage(
    state: intention_domain::goal_domain::GoalLifecycleStateDto,
) -> GoalLifecycleRecordDto {
    use intention_domain::goal_domain::GoalLifecycleStateDto::{
        Active, Archived, NeedsRework, Paused, Stopped,
    };
    match state {
        Active => GoalLifecycleRecordDto::Active,
        NeedsRework => GoalLifecycleRecordDto::NeedsRework,
        Paused => GoalLifecycleRecordDto::Paused,
        Stopped => GoalLifecycleRecordDto::Stopped,
        Archived => GoalLifecycleRecordDto::Archived,
    }
}

/// Converts one durable gate evidence kind into its domain value.
const fn evidence_kind_from_storage(kind: GoalEvidenceKindDto) -> GoalEvidenceKindV1 {
    match kind {
        GoalEvidenceKindDto::TerminalChildResult => GoalEvidenceKindV1::TerminalChildResult,
        GoalEvidenceKindDto::AcceptedUserDeclaration => GoalEvidenceKindV1::AcceptedUserDeclaration,
        GoalEvidenceKindDto::TerminalRegisteredToolResult => {
            GoalEvidenceKindV1::TerminalRegisteredToolResult
        }
        GoalEvidenceKindDto::ExecutableGateResult => GoalEvidenceKindV1::ExecutableGateResult,
    }
}

/// Converts one domain gate evidence kind into its storage value.
const fn evidence_kind_to_storage(kind: GoalEvidenceKindV1) -> GoalEvidenceKindDto {
    match kind {
        GoalEvidenceKindV1::TerminalChildResult => GoalEvidenceKindDto::TerminalChildResult,
        GoalEvidenceKindV1::AcceptedUserDeclaration => GoalEvidenceKindDto::AcceptedUserDeclaration,
        GoalEvidenceKindV1::TerminalRegisteredToolResult => {
            GoalEvidenceKindDto::TerminalRegisteredToolResult
        }
        GoalEvidenceKindV1::ExecutableGateResult => GoalEvidenceKindDto::ExecutableGateResult,
    }
}

/// Converts one durable evidence reference into its domain value.
///
/// # Errors
///
/// Returns `code` for a non-canonical or inexact evidence reference.
fn evidence_from_storage(
    reference: &GoalEvidenceReferenceDto,
    code: &'static str,
) -> DtoResult<GoalEvidenceReferenceV1> {
    Ok(GoalEvidenceReferenceV1::new(
        goal_identity(&reference.evidence_id, code)?,
        reference.revision,
        evidence_kind_from_storage(reference.kind),
    ))
}

/// Converts one domain evidence reference into its storage value.
fn evidence_to_storage(reference: GoalEvidenceReferenceV1) -> GoalEvidenceReferenceDto {
    GoalEvidenceReferenceDto {
        evidence_id: identity_text(reference.evidence_id),
        revision: reference.revision,
        kind: evidence_kind_to_storage(reference.kind),
    }
}

/// Converts one durable readiness state into its domain value.
///
/// # Errors
///
/// Returns `code` for a non-canonical evidence identity and the nested domain
/// readiness validation failures.
fn readiness_from_storage(
    state: &GoalReadinessRecordDto,
    code: &'static str,
) -> DtoResult<GoalReadinessStateDto> {
    Ok(match state {
        GoalReadinessRecordDto::NotReady => GoalReadinessStateDto::NotReady,
        GoalReadinessRecordDto::Ready {
            verified_evidence_set,
        } => {
            let evidence = verified_evidence_set
                .iter()
                .map(|reference| evidence_from_storage(reference, code))
                .collect::<DtoResult<Vec<_>>>()?;
            GoalReadinessStateDto::ready(evidence)?
        }
    })
}

/// Converts one domain readiness state into its storage value.
fn readiness_to_storage(state: &GoalReadinessStateDto) -> GoalReadinessRecordDto {
    match state {
        GoalReadinessStateDto::NotReady => GoalReadinessRecordDto::NotReady,
        GoalReadinessStateDto::Ready {
            verified_evidence_set,
        } => GoalReadinessRecordDto::Ready {
            verified_evidence_set: verified_evidence_set
                .iter()
                .copied()
                .map(evidence_to_storage)
                .collect(),
        },
    }
}

/// Converts one durable gate exception into its domain value.
///
/// # Errors
///
/// Returns `goal_acceptance_exception_invalid` for a non-canonical reference.
fn exception_from_storage(exception: &GoalGateExceptionDto) -> DtoResult<GoalGateExceptionV1> {
    Ok(GoalGateExceptionV1 {
        gate_id: goal_identity(&exception.gate_id, GOAL_ACCEPTANCE_EXCEPTION_INVALID)?,
        gate_revision: exception.gate_revision,
        kind: exception_kind_from_storage(exception.kind),
        evidence: evidence_from_storage(&exception.evidence, GOAL_ACCEPTANCE_EXCEPTION_INVALID)?,
    })
}

/// Converts one domain gate exception into its storage value.
fn exception_to_storage(exception: GoalGateExceptionV1) -> GoalGateExceptionDto {
    GoalGateExceptionDto {
        gate_id: identity_text(exception.gate_id),
        gate_revision: exception.gate_revision,
        kind: exception_kind_to_storage(exception.kind),
        evidence: evidence_to_storage(exception.evidence),
    }
}

/// Converts one durable gate exception kind into its domain value.
const fn exception_kind_from_storage(kind: GoalGateExceptionKindDto) -> GoalGateExceptionKindV1 {
    match kind {
        GoalGateExceptionKindDto::Failed => GoalGateExceptionKindV1::Failed,
        GoalGateExceptionKindDto::Unavailable => GoalGateExceptionKindV1::Unavailable,
        GoalGateExceptionKindDto::Expired => GoalGateExceptionKindV1::Expired,
        GoalGateExceptionKindDto::ExternallyAmbiguous => {
            GoalGateExceptionKindV1::ExternallyAmbiguous
        }
    }
}

/// Converts one domain gate exception kind into its storage value.
const fn exception_kind_to_storage(kind: GoalGateExceptionKindV1) -> GoalGateExceptionKindDto {
    match kind {
        GoalGateExceptionKindV1::Failed => GoalGateExceptionKindDto::Failed,
        GoalGateExceptionKindV1::Unavailable => GoalGateExceptionKindDto::Unavailable,
        GoalGateExceptionKindV1::Expired => GoalGateExceptionKindDto::Expired,
        GoalGateExceptionKindV1::ExternallyAmbiguous => {
            GoalGateExceptionKindDto::ExternallyAmbiguous
        }
    }
}

/// Converts one durable user-decision state into its domain value.
///
/// # Errors
///
/// Returns the nested exception validation failures.
fn decision_from_storage(state: &GoalUserDecisionRecordDto) -> DtoResult<GoalUserDecisionStateDto> {
    Ok(match state {
        GoalUserDecisionRecordDto::Unaccepted => GoalUserDecisionStateDto::Unaccepted,
        GoalUserDecisionRecordDto::Accepted => GoalUserDecisionStateDto::Accepted,
        GoalUserDecisionRecordDto::AcceptedWithException {
            exception_evidence_set,
        } => GoalUserDecisionStateDto::accepted_with_exception(
            exception_evidence_set
                .iter()
                .map(exception_from_storage)
                .collect::<DtoResult<Vec<_>>>()?,
        )?,
    })
}

/// Converts one domain user-decision state into its storage value.
fn decision_to_storage(state: &GoalUserDecisionStateDto) -> GoalUserDecisionRecordDto {
    match state {
        GoalUserDecisionStateDto::Unaccepted => GoalUserDecisionRecordDto::Unaccepted,
        GoalUserDecisionStateDto::Accepted => GoalUserDecisionRecordDto::Accepted,
        GoalUserDecisionStateDto::AcceptedWithException {
            exception_evidence_set,
        } => GoalUserDecisionRecordDto::AcceptedWithException {
            exception_evidence_set: exception_evidence_set
                .iter()
                .copied()
                .map(exception_to_storage)
                .collect(),
        },
    }
}

/// Converts one durable Goal record into its domain value.
///
/// # Errors
///
/// Returns `goal_not_active` for a non-canonical scope, the nested readiness
/// or decision failures, and the domain Goal coherence failures.
fn goal_from_record(record: &GoalRecordDto) -> DtoResult<intention_domain::goal_domain::GoalDto> {
    let scope = goal_scope_from_storage(&record.scope)?;
    intention_domain::goal_domain::GoalDto::new(
        goal_identity(&record.goal_id, "goal_not_active")?,
        scope,
        record.active_revision,
        lifecycle_from_storage(record.lifecycle_state),
        readiness_from_storage(&record.readiness_state, "goal_not_ready")?,
        decision_from_storage(&record.user_decision_state)?,
    )
}

/// Converts one durable Goal revision into its domain value.
///
/// # Errors
///
/// Returns `goal_revision_conflict` for a non-canonical reference and the
/// nested domain revision validation failures.
fn revision_from_record(record: &GoalRevisionRecordDto) -> DtoResult<GoalRevisionDto> {
    let revision = GoalRevisionDto {
        goal_id: goal_identity(&record.goal_id, "goal_revision_conflict")?,
        revision: record.revision,
        title: record.title.clone(),
        objective: record.objective.clone(),
        inherited_rule_references: record
            .inherited_rule_references
            .iter()
            .map(|reference| goal_identity(reference, "goal_revision_conflict"))
            .collect::<DtoResult<Vec<_>>>()?,
        local_rule_references: record
            .local_rule_references
            .iter()
            .map(|reference| goal_identity(reference, "goal_revision_conflict"))
            .collect::<DtoResult<Vec<_>>>()?,
        required_gate_references: record
            .required_gate_references
            .iter()
            .map(|reference| {
                Ok(GoalGateRevisionReferenceV1 {
                    gate_reference: goal_identity(&reference.gate_id, "goal_revision_conflict")?,
                    revision: reference.revision,
                })
            })
            .collect::<DtoResult<Vec<_>>>()?,
        canonical_revision_digest: goal_revision_digest(record),
    };
    revision.validate()?;
    Ok(revision)
}

/// Converts one durable gate definition into its domain value.
///
/// # Errors
///
/// Returns `goal_gate_unavailable` for a non-canonical template identity.
fn gate_definition_from_storage(
    definition: &GoalGateDefinitionDto,
) -> DtoResult<VerificationGateDto> {
    Ok(match definition {
        GoalGateDefinitionDto::Reference {
            evidence_contract_revision,
            accepted_reference_kinds,
        } => VerificationGateDto::ReferenceGate {
            evidence_contract_revision: *evidence_contract_revision,
            accepted_reference_kinds: accepted_reference_kinds
                .iter()
                .copied()
                .map(evidence_kind_from_storage)
                .collect(),
        },
        GoalGateDefinitionDto::Executable {
            template_id,
            template_revision,
        } => VerificationGateDto::ExecutableGate {
            template_id: goal_identity(template_id, "goal_gate_unavailable")?,
            template_revision: *template_revision,
        },
    })
}

/// Converts one durable gate template record into its domain card.
///
/// # Errors
///
/// Returns `goal_gate_unavailable` for a non-canonical identity and the nested
/// domain card validation failures.
fn template_from_record(record: &GoalGateTemplateRecordDto) -> DtoResult<GoalGateTemplateCardV1> {
    let scope = match &record.scope {
        GoalTemplateScopeDto::Project { project_id } => GoalTemplateScopeV1::Project {
            project_id: goal_identity(project_id, "goal_gate_unavailable")?,
        },
        GoalTemplateScopeDto::Goal { goal_id } => GoalTemplateScopeV1::Goal {
            goal_id: goal_identity(goal_id, "goal_gate_unavailable")?,
        },
        GoalTemplateScopeDto::Session { session_id } => GoalTemplateScopeV1::Session {
            session_id: goal_identity(session_id, "goal_gate_unavailable")?,
        },
    };
    let card = GoalGateTemplateCardV1 {
        template_id: goal_identity(&record.template_id, "goal_gate_unavailable")?,
        revision: record.revision,
        scope,
        capability_reference: goal_identity(&record.capability_reference, "goal_gate_unavailable")?,
        input_family: match record.input_family {
            GoalGateInputFamilyDto::ClosedTextV1 => GoalGateInputFamilyV1::ClosedTextV1,
            GoalGateInputFamilyDto::ClosedPathSetV1 => GoalGateInputFamilyV1::ClosedPathSetV1,
        },
        requires_confirmation: record.requires_confirmation,
        canonical_digest: digest_bytes(&record.canonical_digest)?,
    };
    card.validate()?;
    Ok(card)
}

/// Converts one durable gate template scope into its domain value.
///
/// # Errors
///
/// Returns `goal_gate_unavailable` for a non-canonical scope identity.
fn template_scope_from_storage(scope: &GoalTemplateScopeDto) -> DtoResult<GoalTemplateScopeV1> {
    Ok(match scope {
        GoalTemplateScopeDto::Project { project_id } => GoalTemplateScopeV1::Project {
            project_id: goal_identity(project_id, "goal_gate_unavailable")?,
        },
        GoalTemplateScopeDto::Goal { goal_id } => GoalTemplateScopeV1::Goal {
            goal_id: goal_identity(goal_id, "goal_gate_unavailable")?,
        },
        GoalTemplateScopeDto::Session { session_id } => GoalTemplateScopeV1::Session {
            session_id: goal_identity(session_id, "goal_gate_unavailable")?,
        },
    })
}

/// Converts one durable gate template lifecycle state into its domain value.
const fn template_lifecycle_from_storage(
    state: GoalTemplateLifecycleStateDto,
) -> intention_domain::goal_domain::GoalTemplateLifecycleStateDto {
    use intention_domain::goal_domain::GoalTemplateLifecycleStateDto::{Archived, Enabled};
    match state {
        GoalTemplateLifecycleStateDto::Enabled => Enabled,
        GoalTemplateLifecycleStateDto::Archived => Archived,
    }
}

/// Converts one durable memory card record into its domain value.
///
/// # Errors
///
/// Returns `memory_reference_unavailable` for a non-canonical reference and the
/// nested domain card validation failures.
fn memory_card_from_record(record: &GoalMemoryCardRecordDto) -> DtoResult<GoalMemoryCardV1> {
    let card = GoalMemoryCardV1 {
        record_id: goal_identity(&record.record_id, "memory_reference_unavailable")?,
        revision: record.revision,
        kind: match record.kind {
            MemoryKindRecordDto::Fact => intention_domain::goal_domain::MemoryKindDto::Fact,
            MemoryKindRecordDto::Decision => intention_domain::goal_domain::MemoryKindDto::Decision,
            MemoryKindRecordDto::Preference => {
                intention_domain::goal_domain::MemoryKindDto::Preference
            }
            MemoryKindRecordDto::PastFailure => {
                intention_domain::goal_domain::MemoryKindDto::PastFailure
            }
        },
        scope: record_scope_from_storage(&record.scope)?,
        title: record.title.clone(),
        safe_purpose: record.safe_purpose.clone(),
        retained_content_reference: goal_identity(
            &record.retained_content_reference,
            "memory_reference_unavailable",
        )?,
        canonical_digest: digest_bytes(&record.canonical_digest)?,
    };
    card.validate()?;
    Ok(card)
}

/// Converts one durable Skill card record into its domain value.
///
/// # Errors
///
/// Returns `skill_reference_unavailable` for a non-canonical reference and the
/// nested domain card validation failures.
fn skill_card_from_record(record: &GoalSkillCardRecordDto) -> DtoResult<GoalSkillCardV1> {
    let card = GoalSkillCardV1 {
        skill_id: goal_identity(&record.skill_id, "skill_reference_unavailable")?,
        revision: record.revision,
        canonical_name: record.canonical_name.clone(),
        description: record.description.clone(),
        owner_scope: record_scope_from_storage(&record.owner_scope).map_err(|_| {
            goal_error(
                "skill_reference_unavailable",
                "a Skill card requires an exact owner scope identity",
            )
        })?,
        content_reference: goal_identity(&record.content_reference, "skill_reference_unavailable")?,
        canonical_digest: digest_bytes(&record.canonical_digest)?,
    };
    card.validate()?;
    Ok(card)
}

/// Converts one durable role card record into its domain value.
///
/// # Errors
///
/// Returns `delegation_role_invalid` for a non-canonical reference and the
/// nested domain card validation failures.
fn role_card_from_record(record: &GoalRoleCardRecordDto) -> DtoResult<GoalRoleCardV1> {
    let card = GoalRoleCardV1 {
        role_id: goal_identity(&record.role_id, "delegation_role_invalid")?,
        revision: record.revision,
        canonical_name: record.canonical_name.clone(),
        task: record.task.clone(),
        permitted_class: match record.permitted_class {
            GoalRoleClassDto::Light => GoalRoleClassV1::Light,
            GoalRoleClassDto::Medium => GoalRoleClassV1::Medium,
            GoalRoleClassDto::Heavy => GoalRoleClassV1::Heavy,
        },
        tool_subset: record.tool_subset.clone(),
        context_limit_bytes: record.context_limit_bytes,
        result_limit_bytes: record.result_limit_bytes,
    };
    card.validate()?;
    Ok(card)
}

/// Converts one durable refinement draft into its domain value.
///
/// # Errors
///
/// Returns `refinement_draft_conflict` for a non-canonical reference and the
/// nested domain draft validation failures.
fn draft_from_record(record: &RefinementDraftRecordDto) -> DtoResult<RefinementDraftDto> {
    let draft = RefinementDraftDto {
        draft_id: goal_identity(&record.draft_id, "refinement_draft_conflict")?,
        source_run_id: goal_identity(&record.source_run_id, "refinement_draft_conflict")?,
        leading_goal_id: goal_identity(&record.leading_goal_id, "refinement_draft_conflict")?,
        milestone: match record.milestone {
            GoalMilestoneRecordDto::TechnicalReadiness => GoalMilestoneV1::TechnicalReadiness,
            GoalMilestoneRecordDto::UserAcceptance => GoalMilestoneV1::UserAcceptance,
            GoalMilestoneRecordDto::AcceptanceWithException => {
                GoalMilestoneV1::AcceptanceWithException
            }
            GoalMilestoneRecordDto::Stop => GoalMilestoneV1::Stop,
            GoalMilestoneRecordDto::RequiredGateFailure => GoalMilestoneV1::RequiredGateFailure,
            GoalMilestoneRecordDto::ObligatoryChildTerminalOutcome => {
                GoalMilestoneV1::ObligatoryChildTerminalOutcome
            }
        },
        base_goal_revision: record.base_goal_revision,
        base_record_reference: goal_identity(
            &record.base_record_reference,
            "refinement_draft_conflict",
        )?,
        base_record_revision: record.base_record_revision,
        edits: record
            .edits
            .iter()
            .map(|edit| {
                Ok(RefinementEditV1 {
                    kind: match edit.kind {
                        RefinementEditKindDto::ReadinessClaim => {
                            intention_domain::goal_domain::RefinementEditKindV1::ReadinessClaim
                        }
                        RefinementEditKindDto::AcceptanceDecision => {
                            intention_domain::goal_domain::RefinementEditKindV1::AcceptanceDecision
                        }
                        RefinementEditKindDto::ExceptionSet => {
                            intention_domain::goal_domain::RefinementEditKindV1::ExceptionSet
                        }
                        RefinementEditKindDto::StopDecision => {
                            intention_domain::goal_domain::RefinementEditKindV1::StopDecision
                        }
                        RefinementEditKindDto::RequiredGateEvidence => {
                            intention_domain::goal_domain::RefinementEditKindV1::RequiredGateEvidence
                        }
                        RefinementEditKindDto::ChildOutcomeReference => {
                            intention_domain::goal_domain::RefinementEditKindV1::ChildOutcomeReference
                        }
                    },
                    evidence: evidence_from_storage(
                        &edit.evidence,
                        "refinement_draft_conflict",
                    )?,
                })
            })
            .collect::<DtoResult<Vec<_>>>()?,
        evidence_references: record
            .evidence_references
            .iter()
            .map(|reference| evidence_from_storage(reference, "refinement_draft_conflict"))
            .collect::<DtoResult<Vec<_>>>()?,
        safe_rationale: record.safe_rationale.clone(),
        canonical_digest: digest_bytes(&record.canonical_digest)?,
    };
    draft.validate()?;
    Ok(draft)
}

/// Converts one durable conversation summary into its domain value.
///
/// # Errors
///
/// Returns `compaction_summary_unavailable` for a non-canonical reference and
/// the nested domain summary validation failures.
fn summary_from_record(
    record: &ConversationSummaryRecordDto,
) -> DtoResult<intention_domain::goal_domain::ConversationSummaryDto> {
    let summary = intention_domain::goal_domain::ConversationSummaryDto {
        summary_id: goal_identity(&record.summary_id, "compaction_summary_unavailable")?,
        revision: record.revision,
        previous_summary_reference: record
            .previous_summary_reference
            .as_deref()
            .map(|reference| goal_identity(reference, "compaction_summary_unavailable"))
            .transpose()?,
        source_range_start: goal_identity(
            &record.source_range_start,
            "compaction_history_unavailable",
        )?,
        source_range_end: goal_identity(
            &record.source_range_end,
            "compaction_history_unavailable",
        )?,
        safe_content: record.safe_content.clone(),
        canonical_digest: digest_bytes(&record.canonical_digest)?,
    };
    summary.validate()?;
    Ok(summary)
}

/// Converts one durable compaction working form into its domain value.
///
/// # Errors
///
/// Returns the nested summary or history-reference failures.
fn working_form_from_record(
    record: &GoalCompactionWorkingFormRecordDto,
) -> DtoResult<GoalCompactionWorkingFormV1> {
    Ok(GoalCompactionWorkingFormV1 {
        current_summary: record
            .current_summary
            .as_ref()
            .map(summary_from_record)
            .transpose()?,
        uncompacted_suffix: record
            .uncompacted_suffix
            .iter()
            .map(|reference| goal_identity(reference, "compaction_history_unavailable"))
            .collect::<DtoResult<Vec<_>>>()?,
    })
}

/// Converts one durable authority operation into its domain value.
const fn verifier_operation_from_storage(operation: VerifierOperationDto) -> VerifierOperationV1 {
    match operation {
        VerifierOperationDto::MarkNeedsRework => VerifierOperationV1::MarkNeedsRework,
        VerifierOperationDto::MarkComplete => VerifierOperationV1::MarkComplete,
        VerifierOperationDto::Stop => VerifierOperationV1::Stop,
        VerifierOperationDto::ReviseFull => VerifierOperationV1::ReviseFull,
        VerifierOperationDto::ResolveUnknownEffect => VerifierOperationV1::ResolveUnknownEffect,
    }
}

/// Converts one domain authority operation into its storage value.
const fn verifier_operation_to_storage(operation: VerifierOperationV1) -> VerifierOperationDto {
    match operation {
        VerifierOperationV1::MarkNeedsRework => VerifierOperationDto::MarkNeedsRework,
        VerifierOperationV1::MarkComplete => VerifierOperationDto::MarkComplete,
        VerifierOperationV1::Stop => VerifierOperationDto::Stop,
        VerifierOperationV1::ReviseFull => VerifierOperationDto::ReviseFull,
        VerifierOperationV1::ResolveUnknownEffect => VerifierOperationDto::ResolveUnknownEffect,
    }
}

/// Converts one durable verifier evidence kind into its domain value.
const fn verifier_evidence_kind_from_storage(
    kind: VerifierEvidenceKindDto,
) -> VerifierEvidenceKindV1 {
    match kind {
        VerifierEvidenceKindDto::UnconditionalPass => VerifierEvidenceKindV1::UnconditionalPass,
        VerifierEvidenceKindDto::QualifyingFail => VerifierEvidenceKindV1::QualifyingFail,
        VerifierEvidenceKindDto::Inconclusive => VerifierEvidenceKindV1::Inconclusive,
        VerifierEvidenceKindDto::GraphTerminalizationClosure => {
            VerifierEvidenceKindV1::GraphTerminalizationClosure
        }
        VerifierEvidenceKindDto::ReconciliationStandardProof => {
            VerifierEvidenceKindV1::ReconciliationStandardProof
        }
    }
}

/// Converts one durable target lifecycle into its domain value.
const fn verifier_lifecycle_from_storage(
    lifecycle: VerifierTargetLifecycleDto,
) -> VerifierTargetLifecycleV1 {
    match lifecycle {
        VerifierTargetLifecycleDto::Draft => VerifierTargetLifecycleV1::Draft,
        VerifierTargetLifecycleDto::Active => VerifierTargetLifecycleV1::Active,
        VerifierTargetLifecycleDto::Working => VerifierTargetLifecycleV1::Working,
        VerifierTargetLifecycleDto::Paused => VerifierTargetLifecycleV1::Paused,
        VerifierTargetLifecycleDto::PausedAwaitingDecision => {
            VerifierTargetLifecycleV1::PausedAwaitingDecision
        }
        VerifierTargetLifecycleDto::NeedsRework => VerifierTargetLifecycleV1::NeedsRework,
        VerifierTargetLifecycleDto::Completed => VerifierTargetLifecycleV1::Completed,
        VerifierTargetLifecycleDto::Stopped => VerifierTargetLifecycleV1::Stopped,
        VerifierTargetLifecycleDto::Archived => VerifierTargetLifecycleV1::Archived,
    }
}

/// Converts one durable authority consumption rule into its domain value.
const fn consumption_rule_from_storage(
    rule: VerifierAuthorityConsumptionRuleDto,
) -> VerifierAuthorityConsumptionRuleV1 {
    match rule {
        VerifierAuthorityConsumptionRuleDto::SingleUse => {
            VerifierAuthorityConsumptionRuleV1::SingleUse
        }
        VerifierAuthorityConsumptionRuleDto::ReusableWhileActive => {
            VerifierAuthorityConsumptionRuleV1::ReusableWhileActive
        }
    }
}

/// Converts one domain authority consumption rule into its storage value.
const fn consumption_rule_to_storage(
    rule: VerifierAuthorityConsumptionRuleV1,
) -> VerifierAuthorityConsumptionRuleDto {
    match rule {
        VerifierAuthorityConsumptionRuleV1::SingleUse => {
            VerifierAuthorityConsumptionRuleDto::SingleUse
        }
        VerifierAuthorityConsumptionRuleV1::ReusableWhileActive => {
            VerifierAuthorityConsumptionRuleDto::ReusableWhileActive
        }
    }
}

/// Converts one durable authority reference into its domain value.
///
/// # Errors
///
/// Returns `verifier_authority_invalid` for a non-canonical identity or digest.
fn authority_reference_from_storage(
    reference: &VerifierAuthorityReferenceDto,
) -> DtoResult<VerifierAuthorityReferenceV1> {
    Ok(VerifierAuthorityReferenceV1 {
        authority_id: goal_identity(&reference.authority_id, "verifier_authority_invalid")?,
        authority_revision: reference.authority_revision,
        authority_digest: digest_bytes(&reference.canonical_authority_digest)?,
    })
}

/// Converts one durable contract reference into its domain value.
///
/// # Errors
///
/// Returns `verifier_authority_invalid` for a non-canonical identity or digest.
fn contract_reference_from_storage(
    reference: &VerifierContractReferenceDto,
) -> DtoResult<VerifierContractReferenceV1> {
    Ok(VerifierContractReferenceV1 {
        contract_id: goal_identity(&reference.contract_id, "verifier_authority_invalid")?,
        contract_revision: reference.contract_revision,
        contract_digest: digest_bytes(&reference.canonical_contract_digest)?,
    })
}

/// Converts one durable target reference into its domain value.
///
/// # Errors
///
/// Returns `verifier_target_lifecycle_invalid` for a non-canonical identity.
fn target_reference_from_storage(
    reference: &VerifierTargetReferenceDto,
) -> DtoResult<VerifierTargetReferenceV1> {
    Ok(VerifierTargetReferenceV1 {
        target_mandate_id: goal_identity(
            &reference.target_mandate_id,
            "verifier_target_lifecycle_invalid",
        )?,
        target_revision: reference.target_revision,
    })
}

/// Converts one durable frozen-reference set into its domain value.
///
/// # Errors
///
/// Returns the nested reference failures and the frozen-reference validation
/// failures.
fn frozen_references_from_storage(
    references: &VerifierFrozenReferencesDto,
) -> DtoResult<VerifierFrozenGoalGateEvidenceReferencesV1> {
    let frozen = VerifierFrozenGoalGateEvidenceReferencesV1 {
        goal_references: references
            .goal_references
            .iter()
            .map(|reference| {
                Ok(VerifierGoalReferenceV1 {
                    goal_id: goal_identity(
                        &reference.goal_id,
                        "verifier_baseline_stale_target_identity",
                    )?,
                    goal_revision: reference.goal_revision,
                })
            })
            .collect::<DtoResult<Vec<_>>>()?,
        gate_evidence_contract_references: references
            .contract_references
            .iter()
            .map(contract_reference_from_storage)
            .collect::<DtoResult<Vec<_>>>()?,
    };
    frozen.validate().map_err(|_| {
        goal_error(
            "verifier_authority_invalid",
            "the frozen verifier references are not canonical",
        )
    })?;
    Ok(frozen)
}

/// Rebuilds one durable authority record as its recorded domain value.
///
/// The rebuilt record is validated through the architecture 17 constructor,
/// and the stored digest must equal the digest of the issued authority
/// revision: revocation and consumption are lifecycle events of that same
/// revision, so they never change its identity. A mismatch fails closed as a
/// corrupt authority rather than repairing it from current state.
///
/// # Errors
///
/// Returns `verifier_authority_invalid` for a non-canonical reference and
/// `verifier_authority_digest_mismatch` for a corrupt stored digest.
fn authority_from_record(record: &VerifierAuthorityRecordDto) -> DtoResult<VerifierAuthorityV1> {
    issued_authority_from_record(record)?;
    authority_with_lifecycle(record, recorded_lifecycle_from_storage(record)?)
}

/// Rebuilds one durable authority record as its issued revision.
///
/// The issued revision carries the stored canonical digest that every
/// authority reference names: revocation, expiry, and consumption are
/// lifecycle events of that revision, so its reference identity stays stable
/// while the recorded value keeps the full lifecycle. The stored digest must
/// equal the rebuilt issued revision, and a mismatch fails closed as a corrupt
/// authority rather than repairing it from current state.
///
/// # Errors
///
/// Returns `verifier_authority_invalid` for a non-canonical reference and
/// `verifier_authority_digest_mismatch` for a corrupt stored digest.
fn issued_authority_from_record(
    record: &VerifierAuthorityRecordDto,
) -> DtoResult<VerifierAuthorityV1> {
    let issued = authority_with_lifecycle(
        record,
        VerifierAuthorityLifecycleV1 {
            issued_at_ms: record.issued_at_ms,
            expires_at_ms: record.expires_at_ms,
            revoked_at_ms: None,
            revocation_reference: None,
            consumption_rule: consumption_rule_from_storage(record.consumption_rule),
            consumption: VerifierAuthorityConsumptionV1::Unconsumed,
        },
    )?;
    if digest_bytes(&record.canonical_authority_digest)? != issued.canonical_digest() {
        return Err(goal_error(
            "verifier_authority_digest_mismatch",
            "the stored authority digest does not match the exact authority revision",
        ));
    }
    Ok(issued)
}

/// Rebuilds the recorded lifecycle of one durable authority record.
///
/// # Errors
///
/// Returns `verifier_authority_invalid` for a consumed authority that does not
/// name its consuming mutation.
fn recorded_lifecycle_from_storage(
    record: &VerifierAuthorityRecordDto,
) -> DtoResult<VerifierAuthorityLifecycleV1> {
    Ok(VerifierAuthorityLifecycleV1 {
        issued_at_ms: record.issued_at_ms,
        expires_at_ms: record.expires_at_ms,
        revoked_at_ms: record.revoked_at_ms,
        revocation_reference: record
            .revocation_reference
            .as_deref()
            .map(|reference| goal_identity(reference, "verifier_authority_invalid"))
            .transpose()?,
        consumption_rule: consumption_rule_from_storage(record.consumption_rule),
        consumption: match record.consumption_state {
            VerifierAuthorityConsumptionStateDto::Unconsumed => {
                VerifierAuthorityConsumptionV1::Unconsumed
            }
            VerifierAuthorityConsumptionStateDto::Consumed => {
                VerifierAuthorityConsumptionV1::Consumed {
                    mutation_reference: goal_identity(
                        record
                            .consumed_by_mutation_reference
                            .as_deref()
                            .ok_or_else(|| {
                                goal_error(
                                    "verifier_authority_invalid",
                                    "a consumed authority names its consuming mutation",
                                )
                            })?,
                        "verifier_authority_invalid",
                    )?,
                }
            }
        },
    })
}

/// Rebuilds one durable authority record under an explicit lifecycle.
///
/// # Errors
///
/// Returns `verifier_authority_invalid` for a non-canonical identity,
/// revision, digest, or target set, and the closed lifecycle failures of the
/// architecture 17 constructor.
fn authority_with_lifecycle(
    record: &VerifierAuthorityRecordDto,
    lifecycle: VerifierAuthorityLifecycleV1,
) -> DtoResult<VerifierAuthorityV1> {
    VerifierAuthorityV1::new(
        goal_identity(&record.authority_id, "verifier_authority_invalid")?,
        record.authority_revision,
        goal_identity(&record.verifier_mandate_id, "verifier_authority_invalid")?,
        VerifierTargetSetReferenceV1 {
            target_set_id: goal_identity(
                &record.immutable_target_set_reference.target_set_id,
                "verifier_authority_invalid",
            )?,
            target_set_digest: digest_bytes(
                &record
                    .immutable_target_set_reference
                    .canonical_target_set_digest,
            )?,
        },
        record
            .allowed_operations
            .iter()
            .copied()
            .map(verifier_operation_from_storage)
            .collect(),
        contract_reference_from_storage(&record.audit_contract_reference)?,
        lifecycle,
    )
}

/// Rebuilds one durable audit baseline as its domain value.
///
/// # Errors
///
/// Returns the nested reference failures, the domain baseline validation
/// failures, and `verifier_baseline_invalid` for a corrupt stored digest.
fn baseline_from_record(
    record: &VerifierAuditBaselineRecordDto,
) -> DtoResult<VerifierAuditBaselineV1> {
    let baseline = VerifierAuditBaselineV1::new(
        authority_reference_from_storage(&record.authority_reference)?,
        record.verifier_mandate_revision,
        goal_identity(
            &record.target_mandate_id,
            "verifier_baseline_stale_target_identity",
        )?,
        VerifierTargetRevisionAndSequenceV1 {
            revision: record.target_revision,
            aggregate_sequence: record.target_sequence,
        },
        verifier_lifecycle_from_storage(record.target_lifecycle),
        frozen_references_from_storage(&record.frozen_references)?,
        record
            .optional_unknown_effect_reference
            .as_deref()
            .map(|reference| goal_identity(reference, "verifier_baseline_invalid"))
            .transpose()?,
        contract_reference_from_storage(&record.audit_contract_reference)?,
        record.graph_epoch,
    )?;
    if digest_bytes(&record.canonical_baseline_digest)? != baseline.canonical_digest() {
        return Err(goal_error(
            "verifier_baseline_invalid",
            "the stored baseline digest does not match the frozen baseline",
        ));
    }
    Ok(baseline)
}

/// One explicit request creating a durable Goal and its first revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateGoalRequestDto {
    /// The daemon-assigned Goal identity.
    pub goal_id: String,
    /// The exactly-one Goal scope.
    pub scope: GoalScopeRecordDto,
    /// The bounded safe title.
    pub title: String,
    /// The bounded safe objective.
    pub objective: String,
    /// The inherited rule references of the exact parent revision.
    pub inherited_rule_references: Vec<String>,
    /// The local rule references added by this revision.
    pub local_rule_references: Vec<String>,
    /// The required gate revision references.
    pub required_gate_references: Vec<GoalGateRevisionReferenceDto>,
    /// The durable creation time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit request appending the next immutable Goal revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendGoalRevisionRequestDto {
    /// The owning Goal identity.
    pub goal_id: String,
    /// The exact active revision the caller observed.
    pub expected_revision: u64,
    /// The bounded safe title.
    pub title: String,
    /// The bounded safe objective.
    pub objective: String,
    /// The inherited rule references of the exact parent revision.
    pub inherited_rule_references: Vec<String>,
    /// The local rule references added by this revision.
    pub local_rule_references: Vec<String>,
    /// The required gate revision references.
    pub required_gate_references: Vec<GoalGateRevisionReferenceDto>,
    /// The durable append time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit request attaching an obligatory child Goal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachGoalChildRequestDto {
    /// The parent Goal identity.
    pub parent_goal_id: String,
    /// The child Goal identity.
    pub child_goal_id: String,
    /// The exact child revision at link time.
    pub child_revision_at_link: u64,
    /// The session link created atomically for an out-of-link session child.
    pub session_link: Option<GoalSessionLinkCreationRequestDto>,
    /// The durable link time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit durable session link created with an obligatory child.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalSessionLinkCreationRequestDto {
    /// The daemon-assigned Goal link identity.
    pub link_id: String,
    /// The linked session identity.
    pub session_id: String,
    /// The revision from which the link is effective.
    pub effective_from_revision: u64,
}

/// One explicit request applying a closed Goal lifecycle transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransitionGoalLifecycleRequestDto {
    /// The owning Goal identity.
    pub goal_id: String,
    /// The exact active revision the caller observed.
    pub expected_revision: u64,
    /// The requested closed lifecycle state.
    pub lifecycle_state: GoalLifecycleRecordDto,
    /// The durable transition time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit readiness claim against an exact Goal revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimGoalReadinessRequestDto {
    /// The owning Goal identity.
    pub goal_id: String,
    /// The exact active revision the claim observed.
    pub expected_revision: u64,
    /// The selected successful evidence references.
    pub verified_evidence_set: Vec<GoalEvidenceReferenceDto>,
    /// The durable claim time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit user decision on one Goal revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GoalUserDecisionRequestV1 {
    /// Accept a Ready Goal.
    Accept,
    /// Accept with an explicit gate exception set.
    AcceptWithException {
        /// The accepted exception evidence set.
        exception_evidence_set: Vec<GoalGateExceptionDto>,
    },
}

/// One explicit request recording a user decision on one Goal revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordGoalUserDecisionRequestDto {
    /// The owning Goal identity.
    pub goal_id: String,
    /// The exact active revision the decision observed.
    pub expected_revision: u64,
    /// The requested user decision.
    pub decision: GoalUserDecisionRequestV1,
    /// The durable decision time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One request admitting exactly one leading Goal before any external work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalRunAdmissionRequestDto {
    /// The complete frozen goal-run selection.
    pub selection: GoalRunSelectionV1,
    /// The measured canonical target-snapshot size in bytes.
    pub target_snapshot_bytes: u64,
    /// The admission time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// The durable commit input of one fully validated leading-goal admission.
///
/// The commit is all-or-nothing: the run, selection, audit evidence,
/// projections, and snapshots commit together, or no external action occurs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalRunAdmissionCommitDto {
    /// The deterministic admission identity.
    pub admission_reference: String,
    /// The exact one leading Goal identity.
    pub leading_goal_id: String,
    /// The exact frozen Goal revision.
    pub goal_revision: u64,
    /// The closed run kind.
    pub run_kind: GoalRunKindV1,
    /// The canonical target-snapshot digest.
    pub target_snapshot_digest: Digest256,
    /// The admission time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// The typed outcome of one admission commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoalRunAdmissionCommitOutcomeDto {
    /// Whether an equal repeated admission replayed its existing binding.
    pub replayed: bool,
}

/// The commit boundary of one atomic leading-goal run admission.
pub trait GoalRunAdmissionPort {
    /// Commits the run, selection, audit evidence, projections, and snapshots
    /// atomically, or nothing; an equal repeated admission replays its binding.
    ///
    /// # Errors
    ///
    /// Returns the typed daemon-owned admission failure.
    fn commit_goal_run_admission(
        &self,
        input: &GoalRunAdmissionCommitDto,
    ) -> DtoResult<GoalRunAdmissionCommitOutcomeDto>;
}

/// The committed admission of exactly one leading Goal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalRunAdmissionDto {
    /// The deterministic admission identity.
    pub admission_reference: String,
    /// The exact one leading Goal identity.
    pub leading_goal_id: String,
    /// The exact frozen Goal revision.
    pub goal_revision: u64,
    /// The closed run kind.
    pub run_kind: GoalRunKindV1,
    /// The canonical target-snapshot digest.
    pub target_snapshot_digest: Digest256,
    /// Whether an equal repeated admission replayed its existing binding.
    pub replayed: bool,
    /// The admission time in Unix milliseconds.
    pub admitted_at_ms: u64,
}

/// One explicit request creating one durable gate revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateGoalGateRequestDto {
    /// The daemon-assigned gate identity.
    pub gate_id: String,
    /// The owning Goal identity.
    pub goal_id: String,
    /// The closed gate definition.
    pub definition: GoalGateDefinitionDto,
    /// The durable creation time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit request evaluating one gate of one Goal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvaluateGoalGateRequestDto {
    /// The owning Goal identity.
    pub goal_id: String,
    /// The evaluated gate identity.
    pub gate_id: String,
    /// The producing run identity.
    pub producing_run_id: String,
    /// The closed outcome kind.
    pub outcome_kind: GoalGateOutcomeKindDto,
    /// The selected safe evidence of a passing, failing, or unknown outcome.
    pub evidence: Option<GoalEvidenceReferenceDto>,
    /// The durable evaluation time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// The committed gate evaluation with its Goal lifecycle effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalGateEvaluationDto {
    /// The committed gate result.
    pub result: GoalGateResultRecordDto,
    /// The Goal record after the required-gate effect.
    pub goal: GoalRecordDto,
    /// Whether a required-gate failure moved the Goal to `NeedsRework`.
    pub goal_moved_to_needs_rework: bool,
}

/// One explicit request creating one user-created gate template revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateGoalGateTemplateRequestDto {
    /// The daemon-assigned template identity.
    pub template_id: String,
    /// The immutable template revision.
    pub revision: u64,
    /// The template scope.
    pub scope: GoalTemplateScopeDto,
    /// The registered capability reference.
    pub capability_reference: String,
    /// The one closed typed input family.
    pub input_family: GoalGateInputFamilyDto,
    /// Whether execution requires explicit user confirmation.
    pub requires_confirmation: bool,
    /// The provenance of the creation.
    pub provenance: GoalTemplateProvenanceV1,
    /// The applicability context of the creating run.
    pub context: GoalApplicabilityContextV1,
    /// The durable creation time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit request applying a closed gate-template lifecycle transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransitionGoalGateTemplateRequestDto {
    /// The template identity.
    pub template_id: String,
    /// The exact template revision.
    pub revision: u64,
    /// The requested closed lifecycle state.
    pub lifecycle_state: GoalTemplateLifecycleStateDto,
    /// The durable transition time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit request storing one bounded memory card revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreGoalMemoryCardRequestDto {
    /// The daemon-assigned record identity.
    pub record_id: String,
    /// The exact immutable revision.
    pub revision: u64,
    /// The closed memory kind.
    pub kind: MemoryKindRecordDto,
    /// The exactly-one record scope.
    pub scope: GoalRecordScopeDto,
    /// The bounded safe title.
    pub title: String,
    /// The bounded safe purpose.
    pub safe_purpose: String,
    /// The typed retained-content reference.
    pub retained_content_reference: String,
    /// The durable creation time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit request committing a typed memory replacement relation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceGoalMemoryCardRequestDto {
    /// The replaced record identity.
    pub replaced_record_id: String,
    /// The exact replaced revision.
    pub replaced_revision: u64,
    /// The replacement card revision to commit.
    pub replacement: GoalMemoryCardRecordDto,
    /// The durable replacement time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit request committing a typed memory rollback relation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RollbackGoalMemoryCardRequestDto {
    /// The restored record identity.
    pub restored_record_id: String,
    /// The exact restored revision.
    pub restored_revision: u64,
    /// The new immutable card revision linked to the restored revision.
    pub replacement: GoalMemoryCardRecordDto,
    /// The durable rollback time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit full-record disclosure request against a frozen reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevealGoalMemoryReferenceRequestDto {
    /// The daemon-assigned record identity.
    pub record_id: String,
    /// The exact immutable revision.
    pub revision: u64,
    /// The requested retained-content reference.
    pub retained_content_reference: String,
}

/// The exact typed retained-content reference of one revealed full record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalDisclosureReferenceDto {
    /// The daemon-assigned record identity.
    pub record_id: String,
    /// The exact immutable revision.
    pub revision: u64,
    /// The exact retained-content reference.
    pub retained_content_reference: String,
}

/// One explicit request storing one bounded Skill card revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreGoalSkillCardRequestDto {
    /// The daemon-assigned Skill identity.
    pub skill_id: String,
    /// The exact immutable Skill revision.
    pub revision: u64,
    /// The canonical lowercase letters/numbers/hyphens name.
    pub canonical_name: String,
    /// The bounded safe routing description.
    pub description: String,
    /// The exactly-one durable owner scope.
    pub owner_scope: GoalRecordScopeDto,
    /// The typed retained-content reference of the Skill body.
    pub content_reference: String,
    /// The durable creation time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit request storing one bounded role card revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreGoalRoleCardRequestDto {
    /// The canonical role name.
    pub canonical_name: String,
    /// The bounded narrowed task text.
    pub task: String,
    /// The permitted child execution class.
    pub permitted_class: GoalRoleClassDto,
    /// The narrowed registered tool subset.
    pub tool_subset: Vec<String>,
    /// The narrowed context byte limit.
    pub context_limit_bytes: u64,
    /// The narrowed result byte limit.
    pub result_limit_bytes: u64,
    /// The daemon-assigned role identity.
    pub role_id: String,
    /// The exact immutable role revision.
    pub revision: u64,
    /// The durable creation time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit request narrowing a concrete role use against a base role.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NarrowGoalRoleRequestDto {
    /// The selected base role identity.
    pub base_role_id: String,
    /// The exact selected base role revision.
    pub base_revision: u64,
    /// The concrete role use to validate.
    pub concrete: GoalRoleCardRecordDto,
}

/// One bounded model refinement proposal recorded before asking the user.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProposeRefinementDraftRequestDto {
    /// The daemon-assigned draft identity.
    pub draft_id: String,
    /// The selected source run identity.
    pub source_run_id: String,
    /// The selected leading Goal identity.
    pub leading_goal_id: String,
    /// The durable Goal milestone.
    pub milestone: GoalMilestoneV1,
    /// The exact base Goal revision.
    pub base_goal_revision: u64,
    /// The exact base record reference.
    pub base_record_reference: String,
    /// The exact base record revision.
    pub base_record_revision: u64,
    /// The bounded typed edit set.
    pub edits: Vec<RefinementEditV1>,
    /// The evidence references of the proposal.
    pub evidence_references: Vec<GoalEvidenceReferenceV1>,
    /// The bounded safe rationale.
    pub safe_rationale: String,
    /// The durable proposal time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// The durably recorded coalesced proposal awaiting the user decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefinementProposalDto {
    /// The one pending draft of the Goal after coalescing.
    pub draft: RefinementDraftRecordDto,
    /// Whether the equal proposal coalesced into an existing pending draft.
    pub coalesced: bool,
    /// The proposal stays pending until the explicit user decision.
    pub user_decision_required: bool,
}

/// One explicit user decision on one pending refinement draft.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecideRefinementDraftRequestDto {
    /// The selected leading Goal identity.
    pub leading_goal_id: String,
    /// The pending draft identity.
    pub draft_id: String,
    /// The explicit user decision.
    pub decision: RefinementDecisionV1,
    /// The durable decision time in Unix milliseconds.
    pub decided_at_ms: u64,
}

/// The committed user decision with its typed refinement resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefinementDecisionOutcomeDto {
    /// The decided draft.
    pub draft: RefinementDraftRecordDto,
    /// The typed resolution of the decision.
    pub resolution: intention_domain::goal_domain::GoalRefinementResolutionV1,
}

/// One explicit request recording one completed history reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordCompactionSuffixRequestDto {
    /// The exactly-one owner scope of the working form.
    pub scope: GoalRecordScopeDto,
    /// The completed history reference to append.
    pub history_reference: String,
    /// The durable append time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit request storing one compacted conversation summary revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreConversationSummaryRequestDto {
    /// The daemon-assigned summary identity.
    pub summary_id: String,
    /// The immutable summary revision.
    pub revision: u64,
    /// The exactly-one owner scope of the working form.
    pub scope: GoalRecordScopeDto,
    /// The previous summary identity when one continues a chain.
    pub previous_summary_reference: Option<String>,
    /// The first source history reference of the covered range.
    pub source_range_start: String,
    /// The last source history reference of the covered range.
    pub source_range_end: String,
    /// The bounded safe summary content.
    pub safe_content: String,
    /// The closed origin of this compaction request.
    pub origin: GoalCompactionOriginV1,
    /// The durable creation time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit request creating an immutable correction of an earlier summary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorrectConversationSummaryRequestDto {
    /// The corrected summary identity.
    pub summary_id: String,
    /// The exact corrected revision.
    pub corrected_revision: u64,
    /// The immutable next revision with the corrected content.
    pub next_revision: u64,
    /// The bounded safe corrected content.
    pub safe_content: String,
    /// The durable correction time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit request selecting a fork's inherited summary reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForkSummaryReferenceRequestDto {
    /// The ancestor summary identity.
    pub ancestor_summary_id: String,
    /// The exact ancestor summary revision.
    pub ancestor_revision: u64,
    /// The inherited summary identity.
    pub inherited_summary_id: String,
    /// The exact inherited summary revision.
    pub inherited_revision: u64,
}

/// One explicit request issuing one delegated verifier authority revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordVerifierAuthorityRequestDto {
    /// The stable authority identity.
    pub authority_id: String,
    /// The immutable authority revision.
    pub authority_revision: u64,
    /// The exact verifier Mandate that owns this authority.
    pub verifier_mandate_id: String,
    /// The immutable explicitly enumerated target-set identity.
    pub target_set_id: String,
    /// The canonical target-set digest.
    pub target_set_digest: Digest256,
    /// The closed allowed-operation set.
    pub allowed_operations: Vec<VerifierOperationV1>,
    /// The exact audit contract reference.
    pub audit_contract_reference: VerifierContractReferenceDto,
    /// The issuance time in Unix milliseconds.
    pub issued_at_ms: u64,
    /// The optional exclusive expiry time in Unix milliseconds.
    pub expires_at_ms: Option<u64>,
    /// The selected consumption rule.
    pub consumption_rule: VerifierAuthorityConsumptionRuleV1,
}

/// One explicit request revoking one authority revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevokeVerifierAuthorityRequestDto {
    /// The authority identity.
    pub authority_id: String,
    /// The exact authority revision.
    pub authority_revision: u64,
    /// The immutable revocation reference.
    pub revocation_reference: String,
    /// The durable revocation time in Unix milliseconds.
    pub revoked_at_ms: u64,
}

/// One explicit request recording one immutable verifier audit baseline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordVerifierAuditBaselineRequestDto {
    /// The exact authority revision and digest.
    pub authority_reference: VerifierAuthorityReferenceDto,
    /// The exact verifier Mandate revision at baseline creation.
    pub verifier_mandate_revision: u64,
    /// The target Mandate identity.
    pub target_mandate_id: String,
    /// The frozen target revision.
    pub target_revision: u64,
    /// The frozen aggregate target sequence.
    pub target_sequence: u64,
    /// The frozen target lifecycle.
    pub target_lifecycle: VerifierTargetLifecycleDto,
    /// The frozen Goal and gate/evidence contract references.
    pub frozen_references: VerifierFrozenReferencesDto,
    /// The exact unknown-effect reference when the target is uncertain.
    pub optional_unknown_effect_reference: Option<String>,
    /// The exact audit contract revision and digest.
    pub audit_contract_reference: VerifierContractReferenceDto,
    /// The target graph epoch where the target participates in a child graph.
    pub graph_epoch: Option<u64>,
    /// The durable creation time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit request recording one immutable verifier audit evidence record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordVerifierEvidenceRequestDto {
    /// The stable evidence identity.
    pub evidence_id: String,
    /// The exact authority revision and digest.
    pub authority_reference: VerifierAuthorityReferenceDto,
    /// The exact target revision.
    pub target_reference: VerifierTargetReferenceDto,
    /// The frozen Goal and gate/evidence contract references.
    pub frozen_references: VerifierFrozenReferencesDto,
    /// The closed evidence kind.
    pub evidence_kind: VerifierEvidenceKindV1,
    /// The safe retained-content reference; never raw content.
    pub retained_content_reference: String,
    /// The durable creation time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit request recording one immutable verifier audit verdict.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordVerifierVerdictRequestDto {
    /// The stable verdict identity.
    pub verdict_id: String,
    /// The exact authority revision and digest.
    pub authority_reference: VerifierAuthorityReferenceDto,
    /// The exact target revision.
    pub target_reference: VerifierTargetReferenceDto,
    /// The exact frozen baseline digest.
    pub baseline_digest: Digest256,
    /// The closed verdict.
    pub verdict: VerificationAuditVerdictDto,
    /// The audit evidence references, in declared order.
    pub evidence_references: Vec<String>,
    /// The durable creation time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit request applying one verifier target mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplyVerifierTargetMutationRequestDto {
    /// The stable mutation identity.
    pub mutation_id: String,
    /// The acting verifier Mandate identity.
    pub verifier_mandate_id: String,
    /// The exact authority revision and digest.
    pub authority_reference: VerifierAuthorityReferenceDto,
    /// The exact audit contract revision and digest.
    pub audit_contract_reference: VerifierContractReferenceDto,
    /// The exact target revision.
    pub target_reference: VerifierTargetReferenceDto,
    /// The closed verifier operation.
    pub operation: VerifierOperationV1,
    /// The audit evidence references, in declared order.
    pub audit_evidence_references: Vec<String>,
    /// The expected target revision of the frozen baseline.
    pub expected_target_revision: u64,
    /// The expected aggregate target sequence of the frozen baseline.
    pub expected_target_sequence: u64,
    /// The exact frozen baseline digest.
    pub expected_baseline_digest: Digest256,
    /// The idempotent operation identity.
    pub operation_id: String,
    /// The semantic operation digest.
    pub operation_digest: Digest256,
    /// The daemon-observed committed target state.
    pub observed_committed_state: VerifierCommittedStateV1,
    /// The exact target uncertainty named by a reconciliation request.
    pub exact_uncertainty_reference: Option<String>,
    /// The closed reconciliation outcome when one is requested.
    pub reconciliation_outcome: Option<VerifierReconciliationOutcomeV1>,
    /// The durable application time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One explicit restart-recovery read of the verification history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalVerificationRecoveryRequestDto {
    /// The exact authority identity to preserve.
    pub authority_id: String,
    /// The exact authority revision to preserve.
    pub authority_revision: u64,
    /// The committed target mutation to preserve.
    pub committed_mutation_id: String,
}

/// The recovered verification history with its no-replay guarantees.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalVerificationRecoveryDto {
    /// The preserved authority projection.
    pub authority: VerificationMandateAuthorityDto,
    /// The preserved committed target mutation.
    pub committed_mutation: VerifierTargetMutationRecordDto,
    /// Whether verifier evidence work is replayed. Always false.
    pub evidence_work_replayed: bool,
    /// Whether the committed mutation is re-applied. Always false.
    pub mutation_reapplied: bool,
    /// Whether a later attempt requires a separately admitted run. Always true.
    pub later_attempt_requires_new_admission: bool,
}

/// The complete frozen DTO-only durable store of the Goal runtime.
pub trait GoalRuntimeStoreDto:
    GoalRepositoryDto
    + GoalGateRepositoryDto
    + GoalCardRepositoryDto
    + GoalProposalRepositoryDto
    + GoalCompactionRepositoryDto
    + GoalVerificationRepositoryDto
{
}

impl<Store> GoalRuntimeStoreDto for Store where
    Store: GoalRepositoryDto
        + GoalGateRepositoryDto
        + GoalCardRepositoryDto
        + GoalProposalRepositoryDto
        + GoalCompactionRepositoryDto
        + GoalVerificationRepositoryDto
{
}

/// The Goal runtime over the frozen DTO-only durable store.
pub struct GoalRuntimeService<'a, Store: GoalRuntimeStoreDto> {
    store: &'a Store,
}

impl<'a, Store: GoalRuntimeStoreDto> GoalRuntimeService<'a, Store> {
    /// Creates the Goal runtime over its durable store.
    #[must_use]
    pub const fn new(store: &'a Store) -> Self {
        Self { store }
    }

    /// Creates one durable Goal identity with its first immutable revision.
    ///
    /// The project and session Goal bounds are checked before the durable
    /// commit; the revision is typed, bounded, credential-free, canonical, and
    /// immutable.
    ///
    /// # Errors
    ///
    /// Returns `goal_limit_exceeded` when the project or session Goal bound is
    /// exceeded, `goal_not_active` for a non-canonical scope, the nested
    /// revision validation failures, and the typed repository failures of the
    /// atomic creation.
    pub fn create_goal(&self, request: &CreateGoalRequestDto) -> DtoResult<GoalRecordDto> {
        request.scope.validate()?;
        goal_identity(&request.goal_id, "goal_not_active")?;
        let goals_in_project = self
            .store
            .count_goals_in_project(request.scope.project_id().to_owned())?;
        let goals_in_session = match &request.scope {
            GoalScopeRecordDto::Session { session_id, .. } => {
                self.store.count_goals_in_session(session_id.clone())?
            }
            GoalScopeRecordDto::Project { .. } => 0,
        };
        // The observed counts exclude the Goal this request creates.
        validate_goal_allocation(
            usize::try_from(goals_in_project.saturating_add(1)).unwrap_or(usize::MAX),
            usize::try_from(goals_in_session.saturating_add(1)).unwrap_or(usize::MAX),
        )?;
        let revision = self.build_revision(
            &request.goal_id,
            1,
            &request.title,
            &request.objective,
            &request.inherited_rule_references,
            &request.local_rule_references,
            &request.required_gate_references,
            request.occurred_at_ms,
        )?;
        let goal = GoalRecordDto {
            goal_id: request.goal_id.clone(),
            scope: request.scope.clone(),
            active_revision: 1,
            lifecycle_state: GoalLifecycleRecordDto::Active,
            readiness_state: GoalReadinessRecordDto::NotReady,
            user_decision_state: GoalUserDecisionRecordDto::Unaccepted,
            created_at_ms: request.occurred_at_ms,
            updated_at_ms: request.occurred_at_ms,
        };
        let input = CreateGoalInputDto { goal, revision };
        input.validate()?;
        self.store.create_goal(input)
    }

    /// Appends the next immutable Goal revision without widening its base.
    ///
    /// # Errors
    ///
    /// Returns `goal_revision_conflict` for a stale expected revision or a
    /// revision that drops a required gate or rule reference of its base, the
    /// nested revision validation failures, and the typed repository failures.
    pub fn append_goal_revision(
        &self,
        request: &AppendGoalRevisionRequestDto,
    ) -> DtoResult<GoalRecordDto> {
        let base = self
            .store
            .load_goal_revision(request.goal_id.clone(), request.expected_revision)?;
        let next_revision = request.expected_revision.checked_add(1).ok_or_else(|| {
            goal_error(
                "goal_revision_conflict",
                "the active Goal revision cannot be continued",
            )
        })?;
        let revision = self.build_revision(
            &request.goal_id,
            next_revision,
            &request.title,
            &request.objective,
            &request.inherited_rule_references,
            &request.local_rule_references,
            &request.required_gate_references,
            request.occurred_at_ms,
        )?;
        self.validate_revision_narrowing(&base, &revision)?;
        self.store.append_goal_revision(AppendGoalRevisionInputDto {
            goal_id: request.goal_id.clone(),
            expected_revision: request.expected_revision,
            revision,
        })
    }

    /// Attaches one obligatory child Goal link atomically.
    ///
    /// # Errors
    ///
    /// Returns `goal_cycle_detected` for a self-link, cross-project, foreign
    /// session, duplicate direct child, or unlinked session child,
    /// `goal_tree_depth_limit_exceeded` and `goal_child_limit_exceeded` for the
    /// tree bounds, `goal_session_link_limit_exceeded` for the link bound,
    /// `goal_revision_conflict` for a stale child revision or duplicate link,
    /// `goal_not_active` for an unknown Goal, and the typed repository failures.
    pub fn attach_goal_child(
        &self,
        request: &AttachGoalChildRequestDto,
    ) -> DtoResult<GoalParentLinkRecordDto> {
        let parent = self.store.load_goal(request.parent_goal_id.clone())?;
        let child = self.store.load_goal(request.child_goal_id.clone())?;
        if request.child_revision_at_link != child.active_revision {
            return Err(goal_error(
                "goal_revision_conflict",
                "the child revision at link time must be the exact active child revision",
            ));
        }
        let parent_scope = goal_scope_from_storage(&parent.scope)?;
        let child_scope = goal_scope_from_storage(&child.scope)?;
        let session_links = self.store.load_goal_session_links(parent.goal_id.clone())?;
        let owner_session_link_present = match (parent_scope, child_scope) {
            (_, intention_domain::goal_domain::GoalScopeDto::Session { session_id, .. }) => {
                session_links
                    .iter()
                    .any(|link| link.session_id == identity_text(session_id))
            }
            _ => false,
        };
        let link = GoalParentLinkDto {
            parent_goal_id: goal_identity(&parent.goal_id, "goal_not_active")?,
            child_goal_id: goal_identity(&child.goal_id, "goal_not_active")?,
            child_revision_at_link: request.child_revision_at_link,
            canonical_link_digest: goal_link_digest(
                &parent.goal_id,
                &child.goal_id,
                request.child_revision_at_link,
            ),
        };
        validate_goal_no_cycle(link.parent_goal_id, &[link.child_goal_id])?;
        let disposition = validate_goal_child_link(
            &link,
            parent_scope,
            child_scope,
            owner_session_link_present,
            request.session_link.is_some(),
        )?;
        let children = self.store.list_goal_children(parent.goal_id.clone())?;
        let mut child_links = children
            .iter()
            .map(|link| {
                Ok(GoalParentLinkDto {
                    parent_goal_id: goal_identity(&link.parent_goal_id, "goal_not_active")?,
                    child_goal_id: goal_identity(&link.child_goal_id, "goal_not_active")?,
                    child_revision_at_link: link.child_revision_at_link,
                    canonical_link_digest: digest_bytes(&link.canonical_link_digest)?,
                })
            })
            .collect::<DtoResult<Vec<_>>>()?;
        child_links.push(link);
        validate_goal_direct_children(&child_links)?;
        let depth = u32::try_from(self.store.load_goal_tree_depth(parent.goal_id.clone())?)
            .map_err(|_| {
                goal_error(
                    "goal_tree_depth_limit_exceeded",
                    "the Goal-tree depth is outside its code-owned bound",
                )
            })?;
        validate_goal_tree_attachment(depth, u32::try_from(children.len()).unwrap_or(u32::MAX))?;
        match disposition {
            intention_domain::goal_domain::GoalChildLinkDispositionV1::ExistingSessionLink => {
                if request.session_link.is_some() {
                    if !parent.scope.is_project() {
                        return Err(goal_error(
                            "goal_cycle_detected",
                            "a session Goal owns no explicit project session link",
                        ));
                    }
                    return Err(goal_error(
                        "goal_revision_conflict",
                        "one project Goal holds one link per session identity",
                    ));
                }
            }
            intention_domain::goal_domain::GoalChildLinkDispositionV1::CreateSessionLinkAtomically => {
                let Some(session_link) = &request.session_link else {
                    return Err(goal_error(
                        "goal_cycle_detected",
                        "a session child requires the explicit durable link of its owner session",
                    ));
                };
                goal_identity(&session_link.link_id, "goal_not_active")?;
                let Some(child_session_id) = child_scope.session_id() else {
                    return Err(goal_error(
                        "goal_cycle_detected",
                        "an atomic session link belongs to a session child",
                    ));
                };
                if goal_identity(&session_link.session_id, "goal_not_active")? != child_session_id {
                    return Err(goal_error(
                        "goal_cycle_detected",
                        "the atomic session link belongs to the child's owner session",
                    ));
                }
                if session_links.len() >= GOAL_MAX_SESSION_LINKS_PER_PROJECT_GOAL {
                    return Err(goal_error(
                        "goal_session_link_limit_exceeded",
                        "the session link bound of one project Goal is exceeded",
                    ));
                }
                self.store
                    .create_goal_session_link(CreateGoalSessionLinkInputDto {
                        link: GoalSessionLinkRecordDto {
                            link_id: session_link.link_id.clone(),
                            project_goal_id: parent.goal_id.clone(),
                            session_id: session_link.session_id.clone(),
                            effective_from_revision: session_link.effective_from_revision,
                            canonical_link_digest: digest_text(goal_session_link_digest(
                                &session_link.link_id,
                                &parent.goal_id,
                                &session_link.session_id,
                                session_link.effective_from_revision,
                            )),
                            created_at_ms: request.occurred_at_ms,
                        },
                    })?;
            }
        }
        self.store.attach_goal_child(AttachGoalChildInputDto {
            parent_goal_id: parent.goal_id,
            child_goal_id: child.goal_id,
            child_revision_at_link: request.child_revision_at_link,
            canonical_link_digest: digest_text(link.canonical_link_digest),
            created_at_ms: request.occurred_at_ms,
        })
    }

    /// Applies one closed Goal lifecycle transition.
    ///
    /// Archive is explicit and valid only for a terminal idle Goal; restore
    /// changes presentation only and never launches a run.
    ///
    /// # Errors
    ///
    /// Returns `goal_not_active` for an undeclared transition or a non-active
    /// admission, `goal_archive_not_terminal` for an invalid archive or
    /// restore, `goal_revision_conflict` for a stale expected revision, and the
    /// typed repository failures.
    pub fn transition_goal_lifecycle(
        &self,
        request: &TransitionGoalLifecycleRequestDto,
    ) -> DtoResult<GoalRecordDto> {
        let goal = self.store.load_goal(request.goal_id.clone())?;
        let from = lifecycle_from_storage(goal.lifecycle_state);
        let to = lifecycle_from_storage(request.lifecycle_state);
        if to == intention_domain::goal_domain::GoalLifecycleStateDto::Archived {
            validate_goal_archive(from, &decision_from_storage(&goal.user_decision_state)?)?;
        } else {
            if from == intention_domain::goal_domain::GoalLifecycleStateDto::Archived {
                validate_goal_restore(from)?;
            }
            validate_goal_lifecycle_transition(from, to)?;
        }
        self.store
            .transition_goal_lifecycle(TransitionGoalLifecycleInputDto {
                goal_id: request.goal_id.clone(),
                expected_revision: request.expected_revision,
                lifecycle_state: request.lifecycle_state,
                occurred_at_ms: request.occurred_at_ms,
            })
    }

    /// Records one readiness claim against the exact active Goal revision.
    ///
    /// `Ready` requires the exact effective revision, every obligatory child in
    /// a terminal user-decision state, and selected successful evidence for
    /// every required gate.
    ///
    /// # Errors
    ///
    /// Returns `goal_not_ready` for an incomplete claim,
    /// `goal_revision_conflict` for a stale expected revision, the nested
    /// readiness validation failures, and the typed repository failures.
    pub fn claim_goal_readiness(
        &self,
        request: &ClaimGoalReadinessRequestDto,
    ) -> DtoResult<GoalRecordDto> {
        let goal = self.store.load_goal(request.goal_id.clone())?;
        self.validate_expected_revision(&goal, request.expected_revision)?;
        let revision = self
            .store
            .load_goal_revision(goal.goal_id.clone(), goal.active_revision)?;
        let unresolved = self.unresolved_obligatory_children(&goal.goal_id)?;
        let gate_evidence_missing = self.required_gate_evidence_missing(&revision)?;
        let evidence = request
            .verified_evidence_set
            .iter()
            .map(|reference| evidence_from_storage(reference, "goal_not_ready"))
            .collect::<DtoResult<Vec<_>>>()?;
        let readiness =
            validate_goal_readiness_claim(evidence, &unresolved, gate_evidence_missing)?;
        self.store.set_goal_readiness(SetGoalReadinessInputDto {
            goal_id: goal.goal_id,
            expected_revision: request.expected_revision,
            readiness_state: readiness_to_storage(&readiness),
            occurred_at_ms: request.occurred_at_ms,
        })
    }

    /// Records one user decision against the exact active Goal revision.
    ///
    /// An accepted exception set must explicitly include every inherited child
    /// exception and can never omit, cancel, or bypass an active obligatory
    /// child.
    ///
    /// # Errors
    ///
    /// Returns `goal_not_ready` when plain acceptance is requested for an
    /// unready Goal or with an unresolved child,
    /// `goal_acceptance_exception_invalid` for an invalid, incomplete, or
    /// readiness-contradicting exception set, `goal_revision_conflict` for a
    /// stale expected revision, and the typed repository failures.
    pub fn record_goal_user_decision(
        &self,
        request: &RecordGoalUserDecisionRequestDto,
    ) -> DtoResult<GoalRecordDto> {
        let goal = self.store.load_goal(request.goal_id.clone())?;
        self.validate_expected_revision(&goal, request.expected_revision)?;
        let readiness = readiness_from_storage(&goal.readiness_state, "goal_not_ready")?;
        let (unresolved, inherited) = self.child_decision_context(&goal.goal_id)?;
        let decision = match &request.decision {
            GoalUserDecisionRequestV1::Accept => GoalAcceptanceDecisionV1::Accept,
            GoalUserDecisionRequestV1::AcceptWithException {
                exception_evidence_set,
            } => GoalAcceptanceDecisionV1::AcceptWithException {
                exception_evidence_set: exception_evidence_set
                    .iter()
                    .map(exception_from_storage)
                    .collect::<DtoResult<Vec<_>>>()?,
            },
        };
        let accepted = validate_goal_acceptance(&GoalAcceptanceRequestV1 {
            decision,
            readiness_state: readiness,
            inherited_child_exceptions: inherited,
            unresolved_obligatory_children: unresolved,
        })?;
        self.store
            .record_goal_user_decision(RecordGoalUserDecisionInputDto {
                goal_id: goal.goal_id,
                expected_revision: request.expected_revision,
                user_decision_state: decision_to_storage(&accepted),
                occurred_at_ms: request.occurred_at_ms,
            })
    }

    /// Atomically admits exactly one leading Goal before any external work.
    ///
    /// The Goal, its explicit session link, the exact frozen revision, the
    /// complete target snapshot, every selected gate, template, and card
    /// reference, and all bounds are validated without reconstructing current
    /// state; the run, selection, audit evidence, projections, and snapshots
    /// then commit through [`GoalRunAdmissionPort`] or not at all.
    ///
    /// # Errors
    ///
    /// Returns `goal_not_active` for a non-Active Goal, a foreign scope, or an
    /// unavailable session link, `goal_revision_conflict` for a stale frozen
    /// revision, `goal_snapshot_too_large`, `goal_snapshot_unavailable`,
    /// `goal_limit_exceeded`, `goal_gate_limit_exceeded`, `goal_not_ready`, and
    /// `memory_entry_limit_exceeded` for the respective pre-effect rejections,
    /// `goal_gate_unavailable` for an unusable selected gate record, and the
    /// typed admission-port failure.
    pub fn admit_leading_goal_run<P: GoalRunAdmissionPort>(
        &self,
        request: &GoalRunAdmissionRequestDto,
        port: &P,
    ) -> DtoResult<GoalRunAdmissionDto> {
        let goal = self
            .store
            .load_goal(identity_text(request.selection.leading_goal_id))?;
        let domain_goal = goal_from_record(&goal)?;
        let session_links = self
            .store
            .load_goal_session_links(goal.goal_id.clone())?
            .iter()
            .map(|link| {
                Ok(GoalSessionLinkDto {
                    link_id: goal_identity(&link.link_id, "goal_not_active")?,
                    project_goal_id: goal_identity(&link.project_goal_id, "goal_not_active")?,
                    session_id: goal_identity(&link.session_id, "goal_not_active")?,
                    effective_from_revision: link.effective_from_revision,
                    canonical_link_digest: digest_bytes(&link.canonical_link_digest)?,
                })
            })
            .collect::<DtoResult<Vec<_>>>()?;
        validate_goal_run_admission(
            &domain_goal,
            &session_links,
            request.target_snapshot_bytes,
            &request.selection,
        )?;
        let context = self.admission_context(&goal, &request.selection)?;
        self.resolve_selected_gates(&request.selection)?;
        let memory_cards = request
            .selection
            .selected_memory_cards
            .iter()
            .map(|reference| {
                self.store
                    .load_goal_memory_card(
                        identity_text(reference.card_reference),
                        reference.revision,
                    )
                    .and_then(|record| memory_card_from_record(&record))
            })
            .collect::<DtoResult<Vec<_>>>()?;
        validate_memory_card_set(&memory_cards, &context)?;
        let skills = request
            .selection
            .selected_skill_cards
            .iter()
            .map(|reference| {
                self.store
                    .load_goal_skill_card(
                        identity_text(reference.card_reference),
                        reference.revision,
                    )
                    .and_then(|record| skill_card_from_record(&record))
            })
            .collect::<DtoResult<Vec<_>>>()?;
        let roles = request
            .selection
            .selected_role_cards
            .iter()
            .map(|reference| {
                self.store
                    .load_goal_role_card(
                        identity_text(reference.card_reference),
                        reference.revision,
                    )
                    .and_then(|record| role_card_from_record(&record))
            })
            .collect::<DtoResult<Vec<_>>>()?;
        validate_skill_role_card_set(&skills, &roles, &context)?;
        let admission_reference = goal_admission_reference(
            &goal.goal_id,
            request.selection.goal_revision,
            request.selection.run_kind,
            request.selection.target_snapshot_digest,
        );
        let commit = GoalRunAdmissionCommitDto {
            admission_reference: identity_text(admission_reference),
            leading_goal_id: goal.goal_id,
            goal_revision: request.selection.goal_revision,
            run_kind: request.selection.run_kind,
            target_snapshot_digest: request.selection.target_snapshot_digest,
            occurred_at_ms: request.occurred_at_ms,
        };
        let outcome = port.commit_goal_run_admission(&commit)?;
        Ok(GoalRunAdmissionDto {
            admission_reference: commit.admission_reference,
            leading_goal_id: commit.leading_goal_id,
            goal_revision: commit.goal_revision,
            run_kind: commit.run_kind,
            target_snapshot_digest: commit.target_snapshot_digest,
            replayed: outcome.replayed,
            admitted_at_ms: request.occurred_at_ms,
        })
    }

    /// Creates one durable gate revision with its first immutable definition.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_limit_exceeded` when the gate bound is exceeded,
    /// `goal_gate_unavailable` for an unusable definition, and the typed
    /// repository failures of the atomic creation.
    pub fn create_goal_gate(
        &self,
        request: &CreateGoalGateRequestDto,
    ) -> DtoResult<GoalGateRecordDto> {
        let gate_id = goal_identity(&request.gate_id, "goal_gate_unavailable")?;
        goal_identity(&request.goal_id, "goal_gate_unavailable")?;
        let definition = gate_definition_from_storage(&request.definition)?;
        validate_goal_gate_definitions(&[intention_domain::goal_domain::GoalGateDefinitionV1 {
            gate_id,
            gate: definition,
        }])?;
        self.store.create_goal_gate(CreateGoalGateInputDto {
            gate: GoalGateRecordDto {
                gate_id: request.gate_id.clone(),
                goal_id: request.goal_id.clone(),
                definition: request.definition.clone(),
                revision: 1,
                canonical_revision_digest: digest_text(goal_gate_digest(
                    &request.gate_id,
                    &request.goal_id,
                    &request.definition,
                )),
                created_at_ms: request.occurred_at_ms,
            },
        })
    }

    /// Evaluates one gate with reference evidence ordered before executable
    /// gates and a stale or missing template failing closed.
    ///
    /// A required-gate failure records its result, retains prior evidence, and
    /// moves the Goal to `NeedsRework`; no outcome triggers a hidden retry.
    ///
    /// # Errors
    ///
    /// Returns `goal_not_active` for a Goal that is not Active or NeedsRework,
    /// `goal_not_ready` when an executable gate runs before its reference gates
    /// are settled, `goal_gate_unavailable` for a missing, stale, or
    /// incompatible executable template, `goal_gate_failed` for an incoherent
    /// outcome or evidence binding, `goal_revision_conflict` for a stale
    /// expected revision, and the typed repository failures.
    pub fn evaluate_goal_gate(
        &self,
        request: &EvaluateGoalGateRequestDto,
    ) -> DtoResult<GoalGateEvaluationDto> {
        let goal = self.store.load_goal(request.goal_id.clone())?;
        if !matches!(
            goal.lifecycle_state,
            GoalLifecycleRecordDto::Active | GoalLifecycleRecordDto::NeedsRework
        ) {
            return Err(goal_error(
                "goal_not_active",
                "only an Active or NeedsRework Goal evaluates a required gate",
            ));
        }
        let gate = self.store.load_goal_gate(request.gate_id.clone())?;
        if gate.goal_id != goal.goal_id {
            return Err(goal_error(
                "goal_gate_unavailable",
                "the gate does not belong to this Goal",
            ));
        }
        let revision = self
            .store
            .load_goal_revision(goal.goal_id.clone(), goal.active_revision)?;
        let definition = gate_definition_from_storage(&gate.definition)?;
        if definition.is_executable() {
            self.validate_reference_gates_settled(&revision)?;
        }
        let evidence = request
            .evidence
            .as_ref()
            .map(|reference| evidence_from_storage(reference, "goal_gate_failed"))
            .transpose()?;
        if definition.is_executable()
            && evidence
                .is_some_and(|evidence| evidence.kind != GoalEvidenceKindV1::ExecutableGateResult)
        {
            return Err(goal_error(
                "goal_gate_failed",
                "an executable gate result carries the executable-gate evidence provenance",
            ));
        }
        let template = match &definition {
            VerificationGateDto::ExecutableGate {
                template_id,
                template_revision,
            } => Some(
                self.store
                    .load_goal_gate_template(
                        identity_text(*template_id),
                        *template_revision,
                    )
                    .and_then(|record| template_from_record(&record))
                    .map_err(|_| {
                        goal_error(
                            "goal_gate_unavailable",
                            "the executable gate template revision is missing, stale, or incompatible",
                        )
                    })?,
            ),
            VerificationGateDto::ReferenceGate { .. } => {
                let Some(evidence) = evidence else {
                    return Err(goal_error(
                        "goal_gate_failed",
                        "a reference gate validates an exact durable reference",
                    ));
                };
                validate_reference_gate_evidence(&definition, &evidence)?;
                None
            }
        };
        validate_gate_execution_preconditions(&definition, template.as_ref())?;
        let outcome = gate_outcome_from_parts(request.outcome_kind, evidence)?;
        let disposition = validate_gate_outcome(&outcome)?;
        let result_record = GoalGateResultRecordDto {
            gate_id: gate.gate_id.clone(),
            gate_revision: gate.revision,
            producing_run_id: request.producing_run_id.clone(),
            outcome_kind: request.outcome_kind,
            disposition: disposition_to_storage(disposition),
            evidence: evidence.map(evidence_to_storage),
            canonical_result_digest: String::new(),
            occurred_at_ms: request.occurred_at_ms,
        };
        let result_record = GoalGateResultRecordDto {
            canonical_result_digest: digest_text(goal_gate_result_digest(&result_record)),
            ..result_record
        };
        let result = self.store.record_goal_gate_result(result_record)?;
        let required = revision
            .required_gate_references
            .iter()
            .any(|reference| reference.gate_id == gate.gate_id);
        if !required {
            return Ok(GoalGateEvaluationDto {
                result,
                goal,
                goal_moved_to_needs_rework: false,
            });
        }
        let target =
            apply_required_gate_outcome(lifecycle_from_storage(goal.lifecycle_state), disposition)?;
        if lifecycle_to_storage(target) == goal.lifecycle_state {
            return Ok(GoalGateEvaluationDto {
                result,
                goal,
                goal_moved_to_needs_rework: false,
            });
        }
        let transitioned =
            self.store
                .transition_goal_lifecycle(TransitionGoalLifecycleInputDto {
                    goal_id: goal.goal_id,
                    expected_revision: goal.active_revision,
                    lifecycle_state: lifecycle_to_storage(target),
                    occurred_at_ms: request.occurred_at_ms,
                })?;
        Ok(GoalGateEvaluationDto {
            result,
            goal: transitioned,
            goal_moved_to_needs_rework: true,
        })
    }

    /// Creates one user-created immutable gate template revision.
    ///
    /// A model may only prepare a template proposal under the proposal rules
    /// and never enables one without explicit user confirmation.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_unavailable` for an unusable card, an unconfirmed
    /// model proposal, a template scope outside the current run, or a
    /// credential-shaped or path-shaped capability reference, and the typed
    /// repository failures.
    pub fn create_goal_gate_template(
        &self,
        request: &CreateGoalGateTemplateRequestDto,
    ) -> DtoResult<GoalGateTemplateRecordDto> {
        validate_goal_scalar(
            &request.capability_reference,
            "goal_gate_unavailable",
            "goal_gate_unavailable",
        )?;
        let scope = template_scope_from_storage(&request.scope)?;
        let card = GoalGateTemplateCardV1 {
            template_id: goal_identity(&request.template_id, "goal_gate_unavailable")?,
            revision: request.revision,
            scope,
            capability_reference: goal_identity(
                &request.capability_reference,
                "goal_gate_unavailable",
            )?,
            input_family: match request.input_family {
                GoalGateInputFamilyDto::ClosedTextV1 => GoalGateInputFamilyV1::ClosedTextV1,
                GoalGateInputFamilyDto::ClosedPathSetV1 => GoalGateInputFamilyV1::ClosedPathSetV1,
            },
            requires_confirmation: request.requires_confirmation,
            canonical_digest: Digest256::sha256(request.template_id.as_bytes()),
        };
        validate_gate_template_creation(&card, &request.provenance)?;
        validate_gate_template_scope(&scope, &request.context)?;
        let record = GoalGateTemplateRecordDto {
            template_id: request.template_id.clone(),
            revision: request.revision,
            scope: request.scope.clone(),
            capability_reference: request.capability_reference.clone(),
            input_family: request.input_family,
            requires_confirmation: request.requires_confirmation,
            lifecycle_state: GoalTemplateLifecycleStateDto::Enabled,
            provenance_kind: match &request.provenance {
                GoalTemplateProvenanceV1::User => GoalTemplateProvenanceKindDto::User,
                GoalTemplateProvenanceV1::ModelProposal { .. } => {
                    GoalTemplateProvenanceKindDto::ModelProposal
                }
            },
            provenance_draft_id: match &request.provenance {
                GoalTemplateProvenanceV1::User => None,
                GoalTemplateProvenanceV1::ModelProposal { draft_id, .. } => {
                    Some(identity_text(*draft_id))
                }
            },
            provenance_accepted_by_user: match &request.provenance {
                GoalTemplateProvenanceV1::User => false,
                GoalTemplateProvenanceV1::ModelProposal {
                    accepted_by_user, ..
                } => *accepted_by_user,
            },
            canonical_digest: String::new(),
            created_at_ms: request.occurred_at_ms,
        };
        let record = GoalGateTemplateRecordDto {
            canonical_digest: digest_text(goal_template_digest(&record)),
            ..record
        };
        self.store.create_goal_gate_template(record)
    }

    /// Applies one closed gate-template lifecycle transition.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_unavailable` for an undeclared create, archive, or
    /// restore edge and the typed repository failures.
    pub fn transition_goal_gate_template(
        &self,
        request: &TransitionGoalGateTemplateRequestDto,
    ) -> DtoResult<GoalGateTemplateRecordDto> {
        let template = self
            .store
            .load_goal_gate_template(request.template_id.clone(), request.revision)?;
        validate_gate_template_lifecycle_transition(
            Some(template_lifecycle_from_storage(template.lifecycle_state)),
            template_lifecycle_from_storage(request.lifecycle_state),
        )?;
        self.store
            .transition_goal_gate_template_lifecycle(TransitionGoalGateTemplateInputDto {
                template_id: request.template_id.clone(),
                revision: request.revision,
                lifecycle_state: request.lifecycle_state,
                occurred_at_ms: request.occurred_at_ms,
            })
    }

    /// Stores one bounded memory card revision.
    ///
    /// # Errors
    ///
    /// Returns `memory_reference_unavailable` for an inexact reference,
    /// `memory_entry_too_large` when the card exceeds its bound,
    /// `memory_entry_limit_exceeded` when the active card bound is exceeded,
    /// `credentials_forbidden` for credential-shaped text, and the typed
    /// repository failures.
    pub fn store_goal_memory_card(
        &self,
        request: &StoreGoalMemoryCardRequestDto,
    ) -> DtoResult<GoalMemoryCardRecordDto> {
        let record = GoalMemoryCardRecordDto {
            record_id: request.record_id.clone(),
            revision: request.revision,
            kind: request.kind,
            scope: request.scope.clone(),
            title: request.title.clone(),
            safe_purpose: request.safe_purpose.clone(),
            retained_content_reference: request.retained_content_reference.clone(),
            canonical_digest: String::new(),
            created_at_ms: request.occurred_at_ms,
        };
        let card = GoalMemoryCardV1 {
            record_id: goal_identity(&request.record_id, "memory_reference_unavailable")?,
            revision: request.revision,
            kind: memory_kind_from_storage(request.kind),
            scope: record_scope_from_storage(&request.scope)?,
            title: request.title.clone(),
            safe_purpose: request.safe_purpose.clone(),
            retained_content_reference: goal_identity(
                &request.retained_content_reference,
                "memory_reference_unavailable",
            )?,
            canonical_digest: goal_memory_card_digest(&record),
        };
        card.validate()?;
        let record = GoalMemoryCardRecordDto {
            canonical_digest: digest_text(goal_memory_card_digest(&record)),
            ..record
        };
        self.store.store_goal_memory_card(record)
    }

    /// Commits one explicit typed memory replacement relation.
    ///
    /// # Errors
    ///
    /// Returns `memory_replacement_conflict` for an invalid relation, the card
    /// validation failures, and the typed repository failures.
    pub fn replace_goal_memory_card(
        &self,
        request: &ReplaceGoalMemoryCardRequestDto,
    ) -> DtoResult<GoalMemoryCardRecordDto> {
        let link = GoalMemoryReplacementLinkV1 {
            replaced_record_id: goal_identity(
                &request.replaced_record_id,
                "memory_replacement_conflict",
            )?,
            replaced_revision: request.replaced_revision,
            replacement_record_id: goal_identity(
                &request.replacement.record_id,
                "memory_replacement_conflict",
            )?,
            replacement_revision: request.replacement.revision,
        };
        validate_memory_replacement(&link)?;
        memory_card_from_record(&request.replacement)?;
        self.store
            .replace_goal_memory_card(GoalMemoryCardReplacementInputDto {
                replaced_record_id: request.replaced_record_id.clone(),
                replaced_revision: request.replaced_revision,
                replacement: request.replacement.clone(),
                occurred_at_ms: request.occurred_at_ms,
            })
    }

    /// Commits one explicit typed memory rollback relation.
    ///
    /// # Errors
    ///
    /// Returns `memory_replacement_conflict` for an invalid rollback, the card
    /// validation failures, and the typed repository failures.
    pub fn rollback_goal_memory_card(
        &self,
        request: &RollbackGoalMemoryCardRequestDto,
    ) -> DtoResult<GoalMemoryCardRecordDto> {
        let link = GoalMemoryRollbackLinkV1 {
            current_record_id: goal_identity(
                &request.replacement.record_id,
                "memory_replacement_conflict",
            )?,
            current_revision: request.replacement.revision,
            restored_record_id: goal_identity(
                &request.restored_record_id,
                "memory_replacement_conflict",
            )?,
            restored_revision: request.restored_revision,
        };
        validate_memory_rollback(&link)?;
        memory_card_from_record(&request.replacement)?;
        self.store
            .rollback_goal_memory_card(GoalMemoryCardRollbackInputDto {
                restored_record_id: request.restored_record_id.clone(),
                restored_revision: request.restored_revision,
                replacement: request.replacement.clone(),
                occurred_at_ms: request.occurred_at_ms,
            })
    }

    /// Reveals one exact full-record reference against its frozen card.
    ///
    /// # Errors
    ///
    /// Returns `memory_reference_unavailable` when the requested reference is
    /// not the exact frozen reference of the card and the typed repository
    /// failures.
    pub fn reveal_goal_memory_reference(
        &self,
        request: &RevealGoalMemoryReferenceRequestDto,
    ) -> DtoResult<GoalDisclosureReferenceDto> {
        let record = self
            .store
            .load_goal_memory_card(request.record_id.clone(), request.revision)?;
        let card = memory_card_from_record(&record)?;
        let requested = goal_identity(
            &request.retained_content_reference,
            "memory_reference_unavailable",
        )?;
        validate_memory_disclosure(&card, requested)?;
        Ok(GoalDisclosureReferenceDto {
            record_id: record.record_id,
            revision: record.revision,
            retained_content_reference: record.retained_content_reference,
        })
    }

    /// Stores one bounded Skill card revision.
    ///
    /// # Errors
    ///
    /// Returns `skill_reference_unavailable` for an inexact or non-canonical
    /// reference, `skill_entry_too_large` when the card exceeds its bound,
    /// `credentials_forbidden` for credential-shaped text, and the typed
    /// repository failures.
    pub fn store_goal_skill_card(
        &self,
        request: &StoreGoalSkillCardRequestDto,
    ) -> DtoResult<GoalSkillCardRecordDto> {
        let record = GoalSkillCardRecordDto {
            skill_id: request.skill_id.clone(),
            revision: request.revision,
            canonical_name: request.canonical_name.clone(),
            description: request.description.clone(),
            owner_scope: request.owner_scope.clone(),
            content_reference: request.content_reference.clone(),
            canonical_digest: String::new(),
            created_at_ms: request.occurred_at_ms,
        };
        let card = GoalSkillCardV1 {
            skill_id: goal_identity(&request.skill_id, "skill_reference_unavailable")?,
            revision: request.revision,
            canonical_name: request.canonical_name.clone(),
            description: request.description.clone(),
            owner_scope: record_scope_from_storage(&request.owner_scope).map_err(|_| {
                goal_error(
                    "skill_reference_unavailable",
                    "a Skill card requires an exact owner scope identity",
                )
            })?,
            content_reference: goal_identity(
                &request.content_reference,
                "skill_reference_unavailable",
            )?,
            canonical_digest: goal_skill_card_digest(&record),
        };
        card.validate()?;
        let record = GoalSkillCardRecordDto {
            canonical_digest: digest_text(goal_skill_card_digest(&record)),
            ..record
        };
        self.store.store_goal_skill_card(record)
    }

    /// Reveals one exact Skill full-record reference against its frozen card.
    ///
    /// # Errors
    ///
    /// Returns `skill_reference_unavailable` when the requested reference is
    /// not the exact frozen reference of the card and the typed repository
    /// failures.
    pub fn reveal_goal_skill_reference(
        &self,
        skill_id: &str,
        revision: u64,
        content_reference: &str,
    ) -> DtoResult<GoalDisclosureReferenceDto> {
        let record = self
            .store
            .load_goal_skill_card(skill_id.to_owned(), revision)?;
        let card = skill_card_from_record(&record)?;
        let requested = goal_identity(content_reference, "skill_reference_unavailable")?;
        validate_skill_disclosure(&card, requested)?;
        Ok(GoalDisclosureReferenceDto {
            record_id: record.skill_id,
            revision: record.revision,
            retained_content_reference: record.content_reference,
        })
    }

    /// Stores one bounded role card revision.
    ///
    /// # Errors
    ///
    /// Returns `delegation_role_invalid` for an inexact or invalid role card,
    /// `credentials_forbidden` for credential-shaped text, and the typed
    /// repository failures.
    pub fn store_goal_role_card(
        &self,
        request: &StoreGoalRoleCardRequestDto,
    ) -> DtoResult<GoalRoleCardRecordDto> {
        let record = GoalRoleCardRecordDto {
            role_id: request.role_id.clone(),
            revision: request.revision,
            canonical_name: request.canonical_name.clone(),
            task: request.task.clone(),
            permitted_class: request.permitted_class,
            tool_subset: request.tool_subset.clone(),
            context_limit_bytes: request.context_limit_bytes,
            result_limit_bytes: request.result_limit_bytes,
            canonical_digest: String::new(),
            created_at_ms: request.occurred_at_ms,
        };
        role_card_from_record(&record)?;
        self.store.store_goal_role_card(GoalRoleCardRecordDto {
            canonical_digest: digest_text(goal_role_card_digest(&record)),
            ..record
        })
    }

    /// Validates one concrete role use against its selected base role.
    ///
    /// # Errors
    ///
    /// Returns `delegation_role_invalid` for an invalid concrete or base role,
    /// `delegation_role_widening_forbidden` when the concrete use adds a tool,
    /// raises a class, or widens a limit, and the typed repository failures.
    pub fn narrow_goal_role(
        &self,
        request: &NarrowGoalRoleRequestDto,
    ) -> DtoResult<GoalRoleCardRecordDto> {
        let base = self
            .store
            .load_goal_role_card(request.base_role_id.clone(), request.base_revision)?;
        let base = role_card_from_record(&base)?;
        let concrete = role_card_from_record(&request.concrete)?;
        validate_role_narrowing(Some(&base), &concrete)?;
        Ok(request.concrete.clone())
    }

    /// Records one bounded model refinement proposal before asking the user.
    ///
    /// An equal proposal coalesces into the one pending draft of the Goal; an
    /// unequal proposal for the same Goal is a typed conflict. The draft is
    /// durably recorded before the user decision and grants no execution
    /// authority.
    ///
    /// # Errors
    ///
    /// Returns `refinement_draft_conflict` for a malformed draft or an unequal
    /// proposal against a pending draft, `refinement_draft_too_large` when the
    /// safe content exceeds its bound, `goal_limit_exceeded` when a draft bound
    /// is exceeded, `credentials_forbidden` for credential-shaped text, and
    /// the typed repository failures.
    pub fn propose_refinement_draft(
        &self,
        request: &ProposeRefinementDraftRequestDto,
    ) -> DtoResult<RefinementProposalDto> {
        goal_identity(&request.draft_id, "refinement_draft_conflict")?;
        let draft = self.build_draft(request)?;
        let pending = self
            .store
            .load_pending_refinement_draft(request.leading_goal_id.clone())?;
        let coalesced = match pending.as_ref().map(draft_from_record).transpose()? {
            Some(pending) => match coalesce_refinement_draft(Some(&pending), &draft)? {
                intention_domain::goal_domain::GoalDraftCoalescingV1::CoalesceInto { .. } => true,
                intention_domain::goal_domain::GoalDraftCoalescingV1::CreateNew => false,
            },
            None => {
                coalesce_refinement_draft(None, &draft)?;
                false
            }
        };
        validate_pending_draft_limit(&[draft])?;
        let record = draft_to_record(request, self.draft_digest(request));
        let stored = self.store.propose_refinement_draft(record)?;
        Ok(RefinementProposalDto {
            draft: stored,
            coalesced,
            user_decision_required: true,
        })
    }

    /// Applies one explicit user decision to one pending refinement draft.
    ///
    /// The decision resolves through the normal user-confirmation path;
    /// acceptance validates the exact base revision and a stale base is a typed
    /// conflict. Rejection changes no active record.
    ///
    /// # Errors
    ///
    /// Returns `refinement_draft_conflict` for an absent, foreign, or stale-base
    /// draft or an unadvanceable revision, the draft validation failures, and
    /// the typed repository failures.
    pub fn decide_refinement_draft(
        &self,
        request: &DecideRefinementDraftRequestDto,
    ) -> DtoResult<RefinementDecisionOutcomeDto> {
        let pending = self
            .store
            .load_pending_refinement_draft(request.leading_goal_id.clone())?
            .ok_or_else(|| {
                goal_error(
                    "refinement_draft_conflict",
                    "no pending refinement draft exists for this Goal",
                )
            })?;
        if pending.draft_id != request.draft_id {
            return Err(goal_error(
                "refinement_draft_conflict",
                "only the one pending draft of the Goal can be decided",
            ));
        }
        let goal = self.store.load_goal(request.leading_goal_id.clone())?;
        let domain = draft_from_record(&pending)?;
        let resolution =
            resolve_refinement_draft(&domain, request.decision.clone(), goal.active_revision)?;
        let state = match resolution {
            intention_domain::goal_domain::GoalRefinementResolutionV1::Rejected => {
                RefinementDraftStateDto::Rejected
            }
            intention_domain::goal_domain::GoalRefinementResolutionV1::Accepted { .. }
            | intention_domain::goal_domain::GoalRefinementResolutionV1::EditAccepted { .. } => {
                RefinementDraftStateDto::Accepted
            }
        };
        let draft =
            self.store
                .decide_refinement_draft(pending.draft_id, state, request.decided_at_ms)?;
        Ok(RefinementDecisionOutcomeDto { draft, resolution })
    }

    /// Appends one completed history reference of one working-form scope.
    ///
    /// # Errors
    ///
    /// Returns `compaction_history_unavailable` for an unusable reference and
    /// the typed repository failures.
    pub fn record_compaction_suffix_reference(
        &self,
        request: &RecordCompactionSuffixRequestDto,
    ) -> DtoResult<()> {
        validate_goal_scalar(
            &request.history_reference,
            "compaction_history_unavailable",
            "compaction_history_unavailable",
        )?;
        self.store
            .record_compaction_suffix_reference(RecordCompactionSuffixReferenceInputDto {
                scope: request.scope.clone(),
                history_reference: request.history_reference.clone(),
                occurred_at_ms: request.occurred_at_ms,
            })
    }

    /// Stores one compacted conversation-summary revision inside an active run.
    ///
    /// A new revision uses the previous selected summary plus the next bounded
    /// completed source range; it runs only inside an active user-admitted run
    /// and never becomes recovery or continuation state.
    ///
    /// # Errors
    ///
    /// Returns `compaction_history_unavailable` outside an active admitted run,
    /// with nothing completed to compact, or when the range does not start at
    /// the first uncompacted fact, `compaction_summary_unavailable` for a
    /// predecessor or revision mismatch, `compaction_summary_too_large` when
    /// the content exceeds its bound, `credentials_forbidden` for
    /// credential-shaped content, and the typed repository failures.
    pub fn store_conversation_summary(
        &self,
        request: &StoreConversationSummaryRequestDto,
    ) -> DtoResult<ConversationSummaryRecordDto> {
        let working = self
            .store
            .load_goal_compaction_working_form(request.scope.clone())?;
        let working = working_form_from_record(&working)?;
        let summary = self.build_summary(request)?;
        validate_compaction_extension(&working, &summary, request.origin)?;
        self.store
            .store_conversation_summary(summary_to_record(&summary, request))
    }

    /// Creates one separately immutable correction of an earlier summary.
    ///
    /// # Errors
    ///
    /// Returns `compaction_summary_unavailable` when the identity, revision,
    /// source range, or predecessor reference differs from the corrected
    /// summary, `compaction_summary_too_large` when the content exceeds its
    /// bound, `credentials_forbidden` for credential-shaped content, and the
    /// typed repository failures.
    pub fn correct_conversation_summary(
        &self,
        request: &CorrectConversationSummaryRequestDto,
    ) -> DtoResult<ConversationSummaryRecordDto> {
        let stored = self
            .store
            .load_conversation_summary(request.summary_id.clone(), request.corrected_revision)?;
        let previous = summary_from_record(&stored)?;
        let corrected = intention_domain::goal_domain::ConversationSummaryDto {
            summary_id: previous.summary_id,
            revision: request.next_revision,
            previous_summary_reference: previous.previous_summary_reference,
            source_range_start: previous.source_range_start,
            source_range_end: previous.source_range_end,
            safe_content: request.safe_content.clone(),
            canonical_digest: Digest256::sha256(request.summary_id.as_bytes()),
        };
        validate_conversation_summary_correction(&previous, &corrected)?;
        let record = ConversationSummaryRecordDto {
            summary_id: request.summary_id.clone(),
            revision: request.next_revision,
            scope: stored.scope,
            previous_summary_reference: corrected.previous_summary_reference.map(identity_text),
            source_range_start: identity_text(corrected.source_range_start),
            source_range_end: identity_text(corrected.source_range_end),
            safe_content: request.safe_content.clone(),
            canonical_digest: String::new(),
            created_at_ms: request.occurred_at_ms,
        };
        self.store
            .store_conversation_summary(ConversationSummaryRecordDto {
                canonical_digest: digest_text(goal_summary_digest(&record)),
                ..record
            })
    }

    /// Validates one fork's inherited summary reference.
    ///
    /// # Errors
    ///
    /// Returns `compaction_summary_unavailable` when the inherited reference is
    /// not the exact compatible selected ancestor summary and the typed
    /// repository failures.
    pub fn validate_fork_summary_reference(
        &self,
        request: &ForkSummaryReferenceRequestDto,
    ) -> DtoResult<ConversationSummaryRecordDto> {
        let ancestor = self.store.load_conversation_summary(
            request.ancestor_summary_id.clone(),
            request.ancestor_revision,
        )?;
        let inherited = self.store.load_conversation_summary(
            request.inherited_summary_id.clone(),
            request.inherited_revision,
        )?;
        validate_fork_summary_inheritance(
            &summary_from_record(&ancestor)?,
            &summary_from_record(&inherited)?,
        )?;
        Ok(inherited)
    }

    /// Issues one immutable delegated verifier authority revision.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_invalid` for an unusable authority,
    /// `verifier_authority_revision_mismatch` for a zero revision,
    /// `credentials_forbidden` for credential-shaped references, and the
    /// typed repository failures.
    pub fn record_verifier_authority(
        &self,
        request: &RecordVerifierAuthorityRequestDto,
    ) -> DtoResult<VerificationMandateAuthorityDto> {
        let lifecycle = VerifierAuthorityLifecycleV1 {
            issued_at_ms: request.issued_at_ms,
            expires_at_ms: request.expires_at_ms,
            revoked_at_ms: None,
            revocation_reference: None,
            consumption_rule: request.consumption_rule,
            consumption: VerifierAuthorityConsumptionV1::Unconsumed,
        };
        let authority = VerifierAuthorityV1::new(
            goal_identity(&request.authority_id, "verifier_authority_invalid")?,
            request.authority_revision,
            goal_identity(&request.verifier_mandate_id, "verifier_authority_invalid")?,
            VerifierTargetSetReferenceV1 {
                target_set_id: goal_identity(&request.target_set_id, "verifier_authority_invalid")?,
                target_set_digest: request.target_set_digest,
            },
            request.allowed_operations.clone(),
            contract_reference_from_storage(&request.audit_contract_reference)?,
            lifecycle,
        )?;
        let stored = self
            .store
            .record_verifier_authority(VerifierAuthorityRecordDto {
                authority_id: request.authority_id.clone(),
                authority_revision: request.authority_revision,
                verifier_mandate_id: request.verifier_mandate_id.clone(),
                immutable_target_set_reference:
                    intention_storage::goal_repo::VerifierTargetSetReferenceDto {
                        target_set_id: request.target_set_id.clone(),
                        canonical_target_set_digest: digest_text(request.target_set_digest),
                    },
                allowed_operations: request
                    .allowed_operations
                    .iter()
                    .copied()
                    .map(verifier_operation_to_storage)
                    .collect(),
                audit_contract_reference: request.audit_contract_reference.clone(),
                issued_at_ms: request.issued_at_ms,
                expires_at_ms: request.expires_at_ms,
                revoked_at_ms: None,
                revocation_reference: None,
                consumption_rule: consumption_rule_to_storage(request.consumption_rule),
                consumption_state: VerifierAuthorityConsumptionStateDto::Unconsumed,
                consumed_by_mutation_reference: None,
                canonical_authority_digest: digest_text(authority.canonical_digest()),
            })?;
        issued_authority_from_record(&stored).map(|stored| stored.project())
    }

    /// Loads and projects one exact delegated verifier authority revision.
    ///
    /// The projection carries the issued canonical digest that authority
    /// references name; revoked, expired, or consumed lifecycle events of the
    /// same revision never change it.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_invalid` for an absent revision,
    /// `verifier_authority_digest_mismatch` for a corrupt stored digest, and
    /// the typed repository failures.
    pub fn load_verifier_authority(
        &self,
        authority_id: &str,
        authority_revision: u64,
    ) -> DtoResult<VerificationMandateAuthorityDto> {
        let record = self
            .store
            .load_verifier_authority(authority_id.to_owned(), authority_revision)?;
        issued_authority_from_record(&record).map(|authority| authority.project())
    }

    /// Revokes one authority revision without rewriting its history.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_revoked` when the revision is already
    /// revoked, `verifier_authority_invalid` for an absent revision,
    /// `credentials_forbidden` for a credential-shaped revocation reference,
    /// and the typed repository failures.
    pub fn revoke_verifier_authority(
        &self,
        request: &RevokeVerifierAuthorityRequestDto,
    ) -> DtoResult<VerificationMandateAuthorityDto> {
        validate_goal_scalar(
            &request.revocation_reference,
            "verifier_authority_invalid",
            "verifier_authority_invalid",
        )?;
        let record = self
            .store
            .revoke_verifier_authority(RevokeVerifierAuthorityInputDto {
                authority_id: request.authority_id.clone(),
                authority_revision: request.authority_revision,
                revocation_reference: request.revocation_reference.clone(),
                revoked_at_ms: request.revoked_at_ms,
            })?;
        issued_authority_from_record(&record).map(|authority| authority.project())
    }

    /// Records one immutable verifier audit baseline after an exact authority
    /// reread.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_invalid` for an absent authority and the
    /// identity, revision, and digest mismatch failures of the exact authority
    /// reference, `verifier_baseline_invalid` for an unusable baseline,
    /// `verifier_baseline_stale_target_revision` for a zero target revision,
    /// and the typed repository failures.
    pub fn record_verifier_audit_baseline(
        &self,
        request: &RecordVerifierAuditBaselineRequestDto,
    ) -> DtoResult<VerifierAuditBaselineRecordDto> {
        let expected = authority_reference_from_storage(&request.authority_reference)?;
        let record = self.store.load_verifier_authority(
            request.authority_reference.authority_id.clone(),
            request.authority_reference.authority_revision,
        )?;
        validate_verifier_authority_reference(&issued_authority_from_record(&record)?, &expected)?;
        let baseline = VerifierAuditBaselineV1::new(
            expected,
            request.verifier_mandate_revision,
            goal_identity(
                &request.target_mandate_id,
                "verifier_baseline_stale_target_identity",
            )?,
            VerifierTargetRevisionAndSequenceV1 {
                revision: request.target_revision,
                aggregate_sequence: request.target_sequence,
            },
            verifier_lifecycle_from_storage(request.target_lifecycle),
            frozen_references_from_storage(&request.frozen_references)?,
            request
                .optional_unknown_effect_reference
                .as_deref()
                .map(|reference| goal_identity(reference, "verifier_baseline_invalid"))
                .transpose()?,
            contract_reference_from_storage(&request.audit_contract_reference)?,
            request.graph_epoch,
        )?;
        self.store
            .record_verifier_audit_baseline(VerifierAuditBaselineRecordDto {
                authority_reference: request.authority_reference.clone(),
                verifier_mandate_revision: request.verifier_mandate_revision,
                target_mandate_id: request.target_mandate_id.clone(),
                target_revision: request.target_revision,
                target_sequence: request.target_sequence,
                target_lifecycle: request.target_lifecycle,
                frozen_references: request.frozen_references.clone(),
                optional_unknown_effect_reference: request
                    .optional_unknown_effect_reference
                    .clone(),
                audit_contract_reference: request.audit_contract_reference.clone(),
                graph_epoch: request.graph_epoch,
                canonical_baseline_digest: digest_text(baseline.canonical_digest()),
                created_at_ms: request.occurred_at_ms,
            })
    }

    /// Records one immutable verifier audit evidence record.
    ///
    /// Evidence grants no operation, mutation, or scheduling effect.
    ///
    /// # Errors
    ///
    /// Returns `verifier_evidence_invalid` for an unusable evidence record,
    /// `verifier_authority_invalid` for a non-canonical authority reference,
    /// and the typed repository failures.
    pub fn record_verifier_audit_evidence(
        &self,
        request: &RecordVerifierEvidenceRequestDto,
    ) -> DtoResult<VerificationAuditEvidenceDto> {
        let evidence = VerifierAuditEvidenceV1::new(
            goal_identity(&request.evidence_id, "verifier_evidence_invalid")?,
            authority_reference_from_storage(&request.authority_reference)?,
            target_reference_from_storage(&request.target_reference)?,
            frozen_references_from_storage(&request.frozen_references)?,
            request.evidence_kind,
            goal_identity(
                &request.retained_content_reference,
                "verifier_evidence_invalid",
            )?,
        )?;
        let stored = self
            .store
            .record_verifier_audit_evidence(VerifierAuditEvidenceRecordDto {
                evidence_id: request.evidence_id.clone(),
                authority_reference: request.authority_reference.clone(),
                target_reference: request.target_reference.clone(),
                frozen_references: request.frozen_references.clone(),
                evidence_kind: verifier_evidence_kind_to_storage(request.evidence_kind),
                retained_content_reference: request.retained_content_reference.clone(),
                canonical_evidence_digest: digest_text(evidence.canonical_digest()),
                created_at_ms: request.occurred_at_ms,
            })?;
        let projection = evidence_from_record(&stored)?;
        Ok(projection.project())
    }

    /// Records one immutable verifier audit verdict.
    ///
    /// A verdict is durable evidence only: it neither schedules work nor
    /// mutates a target.
    ///
    /// # Errors
    ///
    /// Returns `verifier_verdict_invalid` for an unusable verdict, an
    /// over-limit or duplicated evidence reference, and the typed repository
    /// failures.
    pub fn record_verifier_audit_verdict(
        &self,
        request: &RecordVerifierVerdictRequestDto,
    ) -> DtoResult<VerificationVerdictDto> {
        let verdict = VerifierAuditVerdictRecordV1::new(
            goal_identity(&request.verdict_id, "verifier_verdict_invalid")?,
            authority_reference_from_storage(&request.authority_reference)?,
            target_reference_from_storage(&request.target_reference)?,
            request.baseline_digest,
            request.verdict,
            request
                .evidence_references
                .iter()
                .map(|reference| goal_identity(reference, "verifier_verdict_invalid"))
                .collect::<DtoResult<Vec<_>>>()?,
        )?;
        let stored = self
            .store
            .record_verifier_audit_verdict(VerifierAuditVerdictRecordDto {
                verdict_id: request.verdict_id.clone(),
                authority_reference: request.authority_reference.clone(),
                target_reference: request.target_reference.clone(),
                baseline_digest: digest_text(request.baseline_digest),
                verdict: verdict_to_storage(request.verdict),
                evidence_references: request.evidence_references.clone(),
                canonical_verdict_digest: digest_text(verdict.canonical_digest()),
                created_at_ms: request.occurred_at_ms,
            })?;
        Ok(verdict_from_record(&stored)?.project())
    }

    /// Applies one verifier target mutation atomically or not at all.
    ///
    /// The authority is validated for the exact verifier Mandate, target,
    /// operation, contract, and lifecycle; the frozen baseline is compared
    /// against the daemon-observed committed state before any effect; an equal
    /// repeated operation replays its existing binding without applying a
    /// second mutation.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_verifier_mismatch`,
    /// `verifier_authority_self_target`, `verifier_authority_revoked`,
    /// `verifier_authority_expired`, `verifier_authority_consumed`,
    /// `verifier_authority_operation_not_allowed`,
    /// `verifier_authority_contract_mismatch`, the
    /// `verifier_baseline_stale_*` codes, the closed verifier precondition
    /// codes, `verifier_mutation_invalid` for an unusable mutation, and the
    /// typed repository failures.
    pub fn apply_verifier_target_mutation(
        &self,
        request: &ApplyVerifierTargetMutationRequestDto,
    ) -> DtoResult<ApplyVerifierMutationOutcomeDto> {
        let expected_authority = authority_reference_from_storage(&request.authority_reference)?;
        let record = self.store.load_verifier_authority(
            request.authority_reference.authority_id.clone(),
            request.authority_reference.authority_revision,
        )?;
        validate_verifier_authority_reference(
            &issued_authority_from_record(&record)?,
            &expected_authority,
        )?;
        let authority = authority_from_record(&record)?;
        let audit_contract = contract_reference_from_storage(&request.audit_contract_reference)?;
        let target = target_reference_from_storage(&request.target_reference)?;
        validate_verifier_authority_use(
            &authority,
            goal_identity(&request.verifier_mandate_id, "verifier_authority_invalid")?,
            &target,
            request.operation,
            &audit_contract,
            request.occurred_at_ms,
        )?;
        let operation = VerifierOperationIdentityV1 {
            operation_id: goal_identity(&request.operation_id, "verifier_mutation_invalid")?,
            operation_digest: request.operation_digest,
        };
        let baseline = baseline_from_record(
            &self
                .store
                .load_verifier_audit_baseline(digest_text(request.expected_baseline_digest))?,
        )?;
        validate_baseline_freshness(&baseline, &operation, &request.observed_committed_state)?;
        let audit_evidence_references = request
            .audit_evidence_references
            .iter()
            .map(|reference| goal_identity(reference, "verifier_mutation_invalid"))
            .collect::<DtoResult<Vec<_>>>()?;
        self.validate_verifier_preconditions(request, &authority, &baseline)?;
        let mutation = VerifierTargetMutationV1::new(
            goal_identity(&request.mutation_id, "verifier_mutation_invalid")?,
            expected_authority,
            audit_contract,
            target,
            request.operation,
            audit_evidence_references,
            request.expected_target_revision,
            request.expected_target_sequence,
            request.expected_baseline_digest,
            operation,
        )?;
        self.store
            .apply_verifier_target_mutation(VerifierTargetMutationRecordDto {
                mutation_id: request.mutation_id.clone(),
                authority_reference: request.authority_reference.clone(),
                audit_contract_reference: request.audit_contract_reference.clone(),
                target_reference: request.target_reference.clone(),
                operation: verifier_operation_to_storage(request.operation),
                audit_evidence_references: request.audit_evidence_references.clone(),
                expected_target_revision: request.expected_target_revision,
                expected_target_sequence: request.expected_target_sequence,
                expected_baseline_digest: digest_text(request.expected_baseline_digest),
                idempotency: VerifierOperationIdentityDto {
                    operation_id: request.operation_id.clone(),
                    operation_digest: digest_text(request.operation_digest),
                },
                canonical_mutation_digest: digest_text(mutation.canonical_digest()),
                created_at_ms: request.occurred_at_ms,
            })
    }

    /// Reads the durable verification history after a restart without replay.
    ///
    /// The authority, audit, and mutation history is preserved; no verifier
    /// evidence work is replayed and no committed mutation is re-applied. A
    /// later attempt requires a separately admitted run with a new identity.
    ///
    /// # Errors
    ///
    /// Returns the typed authority or mutation read failures and the exact
    /// authority-reference mismatch failures when the committed mutation does
    /// not belong to the preserved authority revision.
    pub fn recover_goal_verification(
        &self,
        request: &GoalVerificationRecoveryRequestDto,
    ) -> DtoResult<GoalVerificationRecoveryDto> {
        let record = self
            .store
            .load_verifier_authority(request.authority_id.clone(), request.authority_revision)?;
        let issued = issued_authority_from_record(&record)?;
        let mutation = self
            .store
            .load_verifier_target_mutation(request.committed_mutation_id.clone())?;
        let reference = authority_reference_from_storage(&mutation.authority_reference)?;
        validate_verifier_authority_reference(&issued, &reference)?;
        Ok(GoalVerificationRecoveryDto {
            authority: issued.project(),
            committed_mutation: mutation,
            evidence_work_replayed: false,
            mutation_reapplied: false,
            later_attempt_requires_new_admission: true,
        })
    }

    /// Builds one validated storage revision from the request fields.
    #[expect(
        clippy::too_many_arguments,
        reason = "The closed Goal revision field list stays one flat validating builder."
    )]
    fn build_revision(
        &self,
        goal_id: &str,
        revision: u64,
        title: &str,
        objective: &str,
        inherited_rule_references: &[String],
        local_rule_references: &[String],
        required_gate_references: &[GoalGateRevisionReferenceDto],
        occurred_at_ms: u64,
    ) -> DtoResult<GoalRevisionRecordDto> {
        let revision = GoalRevisionRecordDto {
            goal_id: goal_id.to_owned(),
            revision,
            title: title.to_owned(),
            objective: objective.to_owned(),
            inherited_rule_references: inherited_rule_references.to_vec(),
            local_rule_references: local_rule_references.to_vec(),
            required_gate_references: required_gate_references.to_vec(),
            canonical_revision_digest: String::new(),
            created_at_ms: occurred_at_ms,
        };
        let record = GoalRevisionRecordDto {
            canonical_revision_digest: digest_text(goal_revision_digest(&revision)),
            ..revision
        };
        record.validate()?;
        revision_from_record(&record)?;
        Ok(record)
    }

    /// Validates that a revision does not widen its base revision.
    fn validate_revision_narrowing(
        &self,
        base: &GoalRevisionRecordDto,
        next: &GoalRevisionRecordDto,
    ) -> DtoResult<()> {
        for gate in &base.required_gate_references {
            if !next
                .required_gate_references
                .iter()
                .any(|reference| reference.gate_id == gate.gate_id)
            {
                return Err(goal_error(
                    "goal_revision_conflict",
                    "a Goal revision cannot drop a required gate of its base revision",
                ));
            }
        }
        for rule in base
            .inherited_rule_references
            .iter()
            .chain(&base.local_rule_references)
        {
            if !next.inherited_rule_references.contains(rule)
                && !next.local_rule_references.contains(rule)
            {
                return Err(goal_error(
                    "goal_revision_conflict",
                    "a Goal revision cannot drop a rule reference of its base revision",
                ));
            }
        }
        Ok(())
    }

    /// Validates one observed expected revision against the loaded Goal.
    fn validate_expected_revision(
        &self,
        goal: &GoalRecordDto,
        expected_revision: u64,
    ) -> DtoResult<()> {
        if goal.active_revision != expected_revision {
            return Err(goal_error(
                "goal_revision_conflict",
                "the expected Goal revision is stale",
            ));
        }
        Ok(())
    }

    /// Returns the obligatory children without a terminal user decision.
    fn unresolved_obligatory_children(&self, goal_id: &str) -> DtoResult<Vec<[u8; 16]>> {
        let mut unresolved = Vec::new();
        for link in self.store.list_goal_children(goal_id.to_owned())? {
            let child = self.store.load_goal(link.child_goal_id)?;
            if !decision_from_storage(&child.user_decision_state)?.is_terminal() {
                unresolved.push(goal_identity(&child.goal_id, "goal_not_active")?);
            }
        }
        Ok(unresolved)
    }

    /// Returns the unresolved children and the inherited child exceptions.
    fn child_decision_context(
        &self,
        goal_id: &str,
    ) -> DtoResult<(Vec<[u8; 16]>, Vec<GoalInheritedExceptionV1>)> {
        let mut unresolved = Vec::new();
        let mut inherited = Vec::new();
        for link in self.store.list_goal_children(goal_id.to_owned())? {
            let child = self.store.load_goal(link.child_goal_id)?;
            let decision = decision_from_storage(&child.user_decision_state)?;
            if !decision.is_terminal() {
                unresolved.push(goal_identity(&child.goal_id, "goal_not_active")?);
                continue;
            }
            for exception in decision.exception_evidence_set() {
                inherited.push(GoalInheritedExceptionV1 {
                    child_goal_id: goal_identity(&child.goal_id, "goal_not_active")?,
                    exception: *exception,
                });
            }
        }
        Ok((unresolved, inherited))
    }

    /// Whether one required gate of the exact revision lacks passing evidence.
    fn required_gate_evidence_missing(&self, revision: &GoalRevisionRecordDto) -> DtoResult<bool> {
        for reference in &revision.required_gate_references {
            match self
                .store
                .load_goal_gate_result(reference.gate_id.clone(), reference.revision)
            {
                Ok(result) if result.disposition == GoalGateOutcomeDispositionDto::Passed => {}
                Ok(_) => return Ok(true),
                Err(error) if error.code() == "goal_gate_unavailable" => return Ok(true),
                Err(error) => return Err(error),
            }
        }
        Ok(false)
    }

    /// Whether every required reference gate of one revision is settled.
    fn validate_reference_gates_settled(&self, revision: &GoalRevisionRecordDto) -> DtoResult<()> {
        for reference in &revision.required_gate_references {
            let gate = self.store.load_goal_gate(reference.gate_id.clone())?;
            if gate.definition.is_executable() {
                continue;
            }
            let passed = match self
                .store
                .load_goal_gate_result(reference.gate_id.clone(), reference.revision)
            {
                Ok(result) => result.disposition == GoalGateOutcomeDispositionDto::Passed,
                Err(error) if error.code() == "goal_gate_unavailable" => false,
                Err(error) => return Err(error),
            };
            if !passed {
                return Err(goal_error(
                    "goal_not_ready",
                    "reference gate evidence must be settled before an executable gate runs",
                ));
            }
        }
        Ok(())
    }

    /// Builds the applicability context of one admission request.
    fn admission_context(
        &self,
        goal: &GoalRecordDto,
        selection: &GoalRunSelectionV1,
    ) -> DtoResult<GoalApplicabilityContextV1> {
        let mut selected_goal_ids = selection.obligatory_component_references.clone();
        selected_goal_ids.push(selection.leading_goal_id);
        selected_goal_ids.extend(
            selection
                .parent_revision_chain
                .iter()
                .map(|reference| reference.goal_id),
        );
        let session_id = match selection.scope_link_provenance.session_id {
            Some(session_id) => session_id,
            None => match &goal.scope {
                GoalScopeRecordDto::Session { session_id, .. } => {
                    goal_identity(session_id, "goal_not_active")?
                }
                GoalScopeRecordDto::Project { .. } => {
                    return Err(goal_error(
                        "goal_snapshot_unavailable",
                        "the Goal context has no current session identity",
                    ));
                }
            },
        };
        Ok(GoalApplicabilityContextV1 {
            project_id: selection.scope_link_provenance.project_id,
            session_id,
            selected_goal_ids,
        })
    }

    /// Resolves every selected gate and template revision of one selection.
    fn resolve_selected_gates(&self, selection: &GoalRunSelectionV1) -> DtoResult<()> {
        for reference in &selection.selected_gate_revisions {
            let gate = self
                .store
                .load_goal_gate(identity_text(reference.gate_reference))
                .map_err(|_| {
                    goal_error(
                        "goal_snapshot_unavailable",
                        "a selected gate revision is unavailable",
                    )
                })?;
            if gate.revision != reference.revision {
                return Err(goal_error(
                    "goal_snapshot_unavailable",
                    "a selected gate revision is stale or incompatible",
                ));
            }
            if let GoalGateDefinitionDto::Executable {
                template_id,
                template_revision,
            } = &gate.definition
            {
                self.store
                    .load_goal_gate_template(template_id.clone(), *template_revision)
                    .map_err(|_| {
                        goal_error(
                            "goal_snapshot_unavailable",
                            "a selected gate template revision is unavailable",
                        )
                    })?;
            }
        }
        Ok(())
    }

    /// Builds one validated bounded refinement draft.
    fn build_draft(
        &self,
        request: &ProposeRefinementDraftRequestDto,
    ) -> DtoResult<RefinementDraftDto> {
        let draft = RefinementDraftDto {
            draft_id: goal_identity(&request.draft_id, "refinement_draft_conflict")?,
            source_run_id: goal_identity(&request.source_run_id, "refinement_draft_conflict")?,
            leading_goal_id: goal_identity(&request.leading_goal_id, "refinement_draft_conflict")?,
            milestone: request.milestone,
            base_goal_revision: request.base_goal_revision,
            base_record_reference: goal_identity(
                &request.base_record_reference,
                "refinement_draft_conflict",
            )?,
            base_record_revision: request.base_record_revision,
            edits: request.edits.clone(),
            evidence_references: request.evidence_references.clone(),
            safe_rationale: request.safe_rationale.clone(),
            canonical_digest: Digest256::sha256(request.draft_id.as_bytes()),
        };
        draft.validate()?;
        Ok(draft)
    }

    /// Computes the digests of one request as a durable draft record.
    fn draft_digest(&self, request: &ProposeRefinementDraftRequestDto) -> Digest256 {
        draft_record_digest(request)
    }

    /// Builds one validated bounded conversation summary.
    ///
    /// # Errors
    ///
    /// Returns `compaction_summary_unavailable` for a non-canonical summary or
    /// predecessor identity and `compaction_history_unavailable` for a
    /// non-canonical source-range reference.
    fn build_summary(
        &self,
        request: &StoreConversationSummaryRequestDto,
    ) -> DtoResult<intention_domain::goal_domain::ConversationSummaryDto> {
        Ok(intention_domain::goal_domain::ConversationSummaryDto {
            summary_id: goal_identity(&request.summary_id, "compaction_summary_unavailable")?,
            revision: request.revision,
            previous_summary_reference: request
                .previous_summary_reference
                .as_deref()
                .map(|reference| goal_identity(reference, "compaction_summary_unavailable"))
                .transpose()?,
            source_range_start: goal_identity(
                &request.source_range_start,
                "compaction_history_unavailable",
            )?,
            source_range_end: goal_identity(
                &request.source_range_end,
                "compaction_history_unavailable",
            )?,
            safe_content: request.safe_content.clone(),
            canonical_digest: Digest256::sha256(request.summary_id.as_bytes()),
        })
    }

    /// Validates the closed verifier-operation preconditions before mutation.
    fn validate_verifier_preconditions(
        &self,
        request: &ApplyVerifierTargetMutationRequestDto,
        authority: &VerifierAuthorityV1,
        baseline: &VerifierAuditBaselineV1,
    ) -> DtoResult<()> {
        let mut qualifying_fail = false;
        let mut unconditional_pass = false;
        let mut graph_closed = false;
        let mut reconciliation_proven = false;
        for reference in &request.audit_evidence_references {
            let kind = verifier_evidence_kind_from_storage(
                self.store
                    .load_verifier_audit_evidence(reference.clone())?
                    .evidence_kind,
            );
            match kind {
                VerifierEvidenceKindV1::UnconditionalPass => unconditional_pass = true,
                VerifierEvidenceKindV1::QualifyingFail => qualifying_fail = true,
                VerifierEvidenceKindV1::Inconclusive => {}
                VerifierEvidenceKindV1::GraphTerminalizationClosure => graph_closed = true,
                VerifierEvidenceKindV1::ReconciliationStandardProof => {
                    reconciliation_proven = true;
                }
            }
        }
        let context = VerifierOperationPreconditionContextV1 {
            authority_includes_operation: authority.allows_operation(request.operation),
            target_lifecycle: baseline.target_lifecycle,
            qualifying_fail_evidence: qualifying_fail,
            unconditional_pass_evidence: unconditional_pass,
            unresolved_target_uncertainty: baseline.optional_unknown_effect_reference.is_some(),
            graph_terminalization_closed: graph_closed,
            reconciliation_standard_proven: reconciliation_proven,
            baseline_unknown_effect_reference: baseline.optional_unknown_effect_reference,
            exact_uncertainty_reference: request
                .exact_uncertainty_reference
                .as_deref()
                .map(|reference| {
                    goal_identity(reference, "verifier_reconciliation_uncertainty_mismatch")
                })
                .transpose()?,
            reconciliation_outcome: request.reconciliation_outcome,
        };
        validate_verifier_operation_preconditions(request.operation, &context).map(|_| ())
    }
}

/// Builds one durable draft record from its validated request.
fn draft_to_record(
    request: &ProposeRefinementDraftRequestDto,
    digest: Digest256,
) -> RefinementDraftRecordDto {
    RefinementDraftRecordDto {
        draft_id: request.draft_id.clone(),
        source_run_id: request.source_run_id.clone(),
        leading_goal_id: request.leading_goal_id.clone(),
        milestone: milestone_to_storage(request.milestone),
        base_goal_revision: request.base_goal_revision,
        base_record_reference: request.base_record_reference.clone(),
        base_record_revision: request.base_record_revision,
        edits: request
            .edits
            .iter()
            .map(|edit| RefinementEditRecordDto {
                kind: refinement_edit_kind_to_storage(edit.kind),
                evidence: evidence_to_storage(edit.evidence),
            })
            .collect(),
        evidence_references: request
            .evidence_references
            .iter()
            .copied()
            .map(evidence_to_storage)
            .collect(),
        safe_rationale: request.safe_rationale.clone(),
        canonical_digest: digest_text(digest),
        state: RefinementDraftStateDto::Pending,
        coalesced_evidence_count: 1,
        created_at_ms: request.occurred_at_ms,
        decided_at_ms: None,
    }
}

/// Computes the deterministic digest of one draft request.
fn draft_record_digest(request: &ProposeRefinementDraftRequestDto) -> Digest256 {
    let mut input = Vec::new();
    push_framed(&mut input, "goal-refinement-draft-request-v1");
    push_framed(&mut input, &request.draft_id);
    push_framed(&mut input, &request.source_run_id);
    push_framed(&mut input, &request.leading_goal_id);
    push_framed(&mut input, milestone_to_storage(request.milestone).name());
    push_framed(&mut input, &request.base_goal_revision.to_string());
    push_framed(&mut input, &request.base_record_reference);
    push_framed(&mut input, &request.base_record_revision.to_string());
    for edit in &request.edits {
        push_framed(
            &mut input,
            refinement_edit_kind_to_storage(edit.kind).name(),
        );
        push_framed(&mut input, &identity_text(edit.evidence.evidence_id));
    }
    push_framed(&mut input, &request.safe_rationale);
    Digest256::sha256(&input)
}

/// Converts one domain memory kind into its storage value.
const fn memory_kind_from_storage(
    kind: MemoryKindRecordDto,
) -> intention_domain::goal_domain::MemoryKindDto {
    match kind {
        MemoryKindRecordDto::Fact => intention_domain::goal_domain::MemoryKindDto::Fact,
        MemoryKindRecordDto::Decision => intention_domain::goal_domain::MemoryKindDto::Decision,
        MemoryKindRecordDto::Preference => intention_domain::goal_domain::MemoryKindDto::Preference,
        MemoryKindRecordDto::PastFailure => {
            intention_domain::goal_domain::MemoryKindDto::PastFailure
        }
    }
}

/// Converts one domain milestone into its storage value.
const fn milestone_to_storage(milestone: GoalMilestoneV1) -> GoalMilestoneRecordDto {
    match milestone {
        GoalMilestoneV1::TechnicalReadiness => GoalMilestoneRecordDto::TechnicalReadiness,
        GoalMilestoneV1::UserAcceptance => GoalMilestoneRecordDto::UserAcceptance,
        GoalMilestoneV1::AcceptanceWithException => GoalMilestoneRecordDto::AcceptanceWithException,
        GoalMilestoneV1::Stop => GoalMilestoneRecordDto::Stop,
        GoalMilestoneV1::RequiredGateFailure => GoalMilestoneRecordDto::RequiredGateFailure,
        GoalMilestoneV1::ObligatoryChildTerminalOutcome => {
            GoalMilestoneRecordDto::ObligatoryChildTerminalOutcome
        }
    }
}

/// Converts one domain refinement edit kind into its storage value.
const fn refinement_edit_kind_to_storage(
    kind: intention_domain::goal_domain::RefinementEditKindV1,
) -> RefinementEditKindDto {
    match kind {
        intention_domain::goal_domain::RefinementEditKindV1::ReadinessClaim => {
            RefinementEditKindDto::ReadinessClaim
        }
        intention_domain::goal_domain::RefinementEditKindV1::AcceptanceDecision => {
            RefinementEditKindDto::AcceptanceDecision
        }
        intention_domain::goal_domain::RefinementEditKindV1::ExceptionSet => {
            RefinementEditKindDto::ExceptionSet
        }
        intention_domain::goal_domain::RefinementEditKindV1::StopDecision => {
            RefinementEditKindDto::StopDecision
        }
        intention_domain::goal_domain::RefinementEditKindV1::RequiredGateEvidence => {
            RefinementEditKindDto::RequiredGateEvidence
        }
        intention_domain::goal_domain::RefinementEditKindV1::ChildOutcomeReference => {
            RefinementEditKindDto::ChildOutcomeReference
        }
    }
}

/// Converts one domain gate outcome disposition into its storage value.
const fn disposition_to_storage(
    disposition: GoalGateOutcomeDispositionV1,
) -> GoalGateOutcomeDispositionDto {
    match disposition {
        GoalGateOutcomeDispositionV1::Passed => GoalGateOutcomeDispositionDto::Passed,
        GoalGateOutcomeDispositionV1::Failed => GoalGateOutcomeDispositionDto::Failed,
        GoalGateOutcomeDispositionV1::Unavailable => GoalGateOutcomeDispositionDto::Unavailable,
        GoalGateOutcomeDispositionV1::UnknownEffect => GoalGateOutcomeDispositionDto::UnknownEffect,
    }
}

/// Converts one domain verifier evidence kind into its storage value.
const fn verifier_evidence_kind_to_storage(
    kind: VerifierEvidenceKindV1,
) -> VerifierEvidenceKindDto {
    match kind {
        VerifierEvidenceKindV1::UnconditionalPass => VerifierEvidenceKindDto::UnconditionalPass,
        VerifierEvidenceKindV1::QualifyingFail => VerifierEvidenceKindDto::QualifyingFail,
        VerifierEvidenceKindV1::Inconclusive => VerifierEvidenceKindDto::Inconclusive,
        VerifierEvidenceKindV1::GraphTerminalizationClosure => {
            VerifierEvidenceKindDto::GraphTerminalizationClosure
        }
        VerifierEvidenceKindV1::ReconciliationStandardProof => {
            VerifierEvidenceKindDto::ReconciliationStandardProof
        }
    }
}

/// Converts one domain verification verdict into its storage value.
const fn verdict_to_storage(
    verdict: VerificationAuditVerdictDto,
) -> intention_storage::goal_repo::VerificationAuditVerdictDto {
    use intention_storage::goal_repo::VerificationAuditVerdictDto as Record;
    match verdict {
        VerificationAuditVerdictDto::Pass => Record::Pass,
        VerificationAuditVerdictDto::Fail => Record::Fail,
        VerificationAuditVerdictDto::Inconclusive => Record::Inconclusive,
        VerificationAuditVerdictDto::TargetRevisionStale => Record::TargetRevisionStale,
        VerificationAuditVerdictDto::TargetUnavailable => Record::TargetUnavailable,
        VerificationAuditVerdictDto::VerifierUnavailable => Record::VerifierUnavailable,
        VerificationAuditVerdictDto::VerifierExternalEffectUnknown => {
            Record::VerifierExternalEffectUnknown
        }
    }
}

/// Converts one durable verification verdict into its domain value.
const fn verdict_from_storage(
    verdict: intention_storage::goal_repo::VerificationAuditVerdictDto,
) -> VerificationAuditVerdictDto {
    use intention_storage::goal_repo::VerificationAuditVerdictDto as Record;
    match verdict {
        Record::Pass => VerificationAuditVerdictDto::Pass,
        Record::Fail => VerificationAuditVerdictDto::Fail,
        Record::Inconclusive => VerificationAuditVerdictDto::Inconclusive,
        Record::TargetRevisionStale => VerificationAuditVerdictDto::TargetRevisionStale,
        Record::TargetUnavailable => VerificationAuditVerdictDto::TargetUnavailable,
        Record::VerifierUnavailable => VerificationAuditVerdictDto::VerifierUnavailable,
        Record::VerifierExternalEffectUnknown => {
            VerificationAuditVerdictDto::VerifierExternalEffectUnknown
        }
    }
}

/// Rebuilds one durable verdict record as its domain value.
///
/// # Errors
///
/// Returns the nested reference failures and `verifier_verdict_invalid` for a
/// corrupt stored digest.
fn verdict_from_record(
    record: &VerifierAuditVerdictRecordDto,
) -> DtoResult<VerifierAuditVerdictRecordV1> {
    let verdict = VerifierAuditVerdictRecordV1::new(
        goal_identity(&record.verdict_id, "verifier_verdict_invalid")?,
        authority_reference_from_storage(&record.authority_reference)?,
        target_reference_from_storage(&record.target_reference)?,
        digest_bytes(&record.baseline_digest)?,
        verdict_from_storage(record.verdict),
        record
            .evidence_references
            .iter()
            .map(|reference| goal_identity(reference, "verifier_verdict_invalid"))
            .collect::<DtoResult<Vec<_>>>()?,
    )?;
    if digest_bytes(&record.canonical_verdict_digest)? != verdict.canonical_digest() {
        return Err(goal_error(
            "verifier_verdict_invalid",
            "the stored verdict digest does not match the exact verdict",
        ));
    }
    Ok(verdict)
}

/// Rebuilds one durable evidence record as its domain value.
///
/// # Errors
///
/// Returns the nested reference failures and `verifier_evidence_invalid` for a
/// corrupt stored digest.
fn evidence_from_record(
    record: &VerifierAuditEvidenceRecordDto,
) -> DtoResult<VerifierAuditEvidenceV1> {
    let evidence = VerifierAuditEvidenceV1::new(
        goal_identity(&record.evidence_id, "verifier_evidence_invalid")?,
        authority_reference_from_storage(&record.authority_reference)?,
        target_reference_from_storage(&record.target_reference)?,
        frozen_references_from_storage(&record.frozen_references)?,
        verifier_evidence_kind_from_storage(record.evidence_kind),
        goal_identity(
            &record.retained_content_reference,
            "verifier_evidence_invalid",
        )?,
    )?;
    if digest_bytes(&record.canonical_evidence_digest)? != evidence.canonical_digest() {
        return Err(goal_error(
            "verifier_evidence_invalid",
            "the stored evidence digest does not match the exact evidence record",
        ));
    }
    Ok(evidence)
}

/// Builds one domain gate outcome from its request parts.
///
/// # Errors
///
/// Returns `goal_gate_failed` for a missing or unexpected evidence reference.
fn gate_outcome_from_parts(
    kind: GoalGateOutcomeKindDto,
    evidence: Option<GoalEvidenceReferenceV1>,
) -> DtoResult<GoalGateOutcomeV1> {
    if kind.requires_evidence() {
        let evidence = evidence.ok_or_else(|| {
            goal_error(
                "goal_gate_failed",
                "a passing, failing, or unknown outcome requires exact evidence",
            )
        })?;
        return Ok(match kind {
            GoalGateOutcomeKindDto::Passed => GoalGateOutcomeV1::Passed { evidence },
            GoalGateOutcomeKindDto::Failed => GoalGateOutcomeV1::Failed { evidence },
            GoalGateOutcomeKindDto::ExternalEffectUnknown => {
                GoalGateOutcomeV1::ExternalEffectUnknown { evidence }
            }
            GoalGateOutcomeKindDto::TimedOut
            | GoalGateOutcomeKindDto::Cancelled
            | GoalGateOutcomeKindDto::OutputBoundExceeded
            | GoalGateOutcomeKindDto::ReferenceUnavailable
            | GoalGateOutcomeKindDto::RevisionStale
            | GoalGateOutcomeKindDto::TemplateUnavailable => {
                return Err(goal_error(
                    "goal_gate_failed",
                    "this gate outcome carries no evidence reference",
                ));
            }
        });
    }
    if evidence.is_some() {
        return Err(goal_error(
            "goal_gate_failed",
            "this gate outcome carries no evidence reference",
        ));
    }
    Ok(match kind {
        GoalGateOutcomeKindDto::Passed
        | GoalGateOutcomeKindDto::Failed
        | GoalGateOutcomeKindDto::ExternalEffectUnknown => {
            return Err(goal_error(
                "goal_gate_failed",
                "a passing, failing, or unknown outcome requires exact evidence",
            ));
        }
        GoalGateOutcomeKindDto::TimedOut => GoalGateOutcomeV1::TimedOut,
        GoalGateOutcomeKindDto::Cancelled => GoalGateOutcomeV1::Cancelled,
        GoalGateOutcomeKindDto::OutputBoundExceeded => GoalGateOutcomeV1::OutputBoundExceeded,
        GoalGateOutcomeKindDto::ReferenceUnavailable => GoalGateOutcomeV1::ReferenceUnavailable,
        GoalGateOutcomeKindDto::RevisionStale => GoalGateOutcomeV1::RevisionStale,
        GoalGateOutcomeKindDto::TemplateUnavailable => GoalGateOutcomeV1::TemplateUnavailable,
    })
}

/// Builds one durable conversation summary from its validated request.
fn summary_to_record(
    summary: &intention_domain::goal_domain::ConversationSummaryDto,
    request: &StoreConversationSummaryRequestDto,
) -> ConversationSummaryRecordDto {
    let record = ConversationSummaryRecordDto {
        summary_id: identity_text(summary.summary_id),
        revision: summary.revision,
        scope: request.scope.clone(),
        previous_summary_reference: summary.previous_summary_reference.map(identity_text),
        source_range_start: identity_text(summary.source_range_start),
        source_range_end: identity_text(summary.source_range_end),
        safe_content: summary.safe_content.clone(),
        canonical_digest: String::new(),
        created_at_ms: request.occurred_at_ms,
    };
    ConversationSummaryRecordDto {
        canonical_digest: digest_text(goal_summary_digest(&record)),
        ..record
    }
}
