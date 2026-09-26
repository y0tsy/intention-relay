#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Goal runtime fixtures use expect and panic for precise diagnostics."
)]

//! Slice 3 Goal runtime tests.
//!
//! Owner: architecture 28 with ADR 0044. These tests pin goal creation and
//! immutable revision narrowing, the obligatory-child graph and session links,
//! lifecycle, readiness, and user-decision transitions, the atomic
//! leading-goal run admission, gate ordering with reference evidence first,
//! gate templates, memory/Skill/role cards, coalesced refinement proposals,
//! conversation-summary compaction, and the architecture 17 verifier
//! authority, baseline, evidence, verdict, mutation, and recovery flows.
//!
//! Every fixture is hermetic and deterministic: the in-memory repositories
//! below mirror the durable Goal, gate, card, proposal, compaction, and
//! verification invariants; time is fixed Unix milliseconds; and no test
//! sleeps, binds a port, or touches the network.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use intention_application::goal_domain::{
    AppendGoalRevisionRequestDto, ApplyVerifierTargetMutationRequestDto, AttachGoalChildRequestDto,
    ClaimGoalReadinessRequestDto, CorrectConversationSummaryRequestDto, CreateGoalGateRequestDto,
    CreateGoalGateTemplateRequestDto, CreateGoalRequestDto, DecideRefinementDraftRequestDto,
    EvaluateGoalGateRequestDto, ForkSummaryReferenceRequestDto, GoalRunAdmissionCommitDto,
    GoalRunAdmissionCommitOutcomeDto, GoalRunAdmissionPort, GoalRunAdmissionRequestDto,
    GoalRuntimeService, GoalUserDecisionRequestV1, GoalVerificationRecoveryRequestDto,
    NarrowGoalRoleRequestDto, ProposeRefinementDraftRequestDto, RecordCompactionSuffixRequestDto,
    RecordGoalUserDecisionRequestDto, RecordVerifierAuditBaselineRequestDto,
    RecordVerifierAuthorityRequestDto, RecordVerifierEvidenceRequestDto,
    RecordVerifierVerdictRequestDto, ReplaceGoalMemoryCardRequestDto,
    RevealGoalMemoryReferenceRequestDto, RevokeVerifierAuthorityRequestDto,
    RollbackGoalMemoryCardRequestDto, StoreConversationSummaryRequestDto,
    StoreGoalMemoryCardRequestDto, StoreGoalRoleCardRequestDto, StoreGoalSkillCardRequestDto,
    TransitionGoalGateTemplateRequestDto, TransitionGoalLifecycleRequestDto,
};
use intention_domain::canonical::Digest256;
use intention_domain::goal_domain::{
    GoalApplicabilityContextV1, GoalCompactionOriginV1, GoalEvidenceKindV1,
    GoalEvidenceReferenceV1, GoalMilestoneDto, GoalRefinementResolutionV1,
    GoalTemplateProvenanceV1, RefinementDecisionV1, RefinementEditKindV1, RefinementEditV1,
};
use intention_domain::slice3_selections::{
    GoalCardReferenceV1, GoalGateRevisionReferenceV1, GoalRunKindV1, GoalRunSelectionBoundsV1,
    GoalRunSelectionV1, GoalScopeLinkProvenanceV1,
};
use intention_domain::verification::{
    VerificationAuditVerdictDto, VerificationMandateAuthorityDto,
    VerifierAuthorityConsumptionRuleV1, VerifierAuthorityReferenceV1, VerifierCommittedStateV1,
    VerifierContractReferenceV1, VerifierEvidenceKindV1, VerifierOperationIdentityV1,
    VerifierOperationV1, VerifierTargetLifecycleV1,
};
use intention_storage::goal_repo::{
    AppendGoalGateRevisionInputDto, AppendGoalRevisionInputDto, ApplyVerifierMutationOutcomeDto,
    AttachGoalChildInputDto, ConversationSummaryRecordDto, CreateGoalGateInputDto,
    CreateGoalInputDto, CreateGoalSessionLinkInputDto, GoalCardRepositoryDto,
    GoalCompactionRepositoryDto, GoalCompactionWorkingFormRecordDto, GoalEvidenceKindDto,
    GoalEvidenceReferenceDto, GoalGateDefinitionDto, GoalGateExceptionDto,
    GoalGateExceptionKindDto, GoalGateInputFamilyDto, GoalGateOutcomeKindDto, GoalGateRecordDto,
    GoalGateRepositoryDto, GoalGateResultRecordDto, GoalGateRevisionReferenceDto,
    GoalGateTemplateRecordDto, GoalLifecycleStateDto, GoalMemoryCardRecordDto,
    GoalMemoryCardReplacementInputDto, GoalMemoryCardRollbackInputDto, GoalParentLinkRecordDto,
    GoalProposalRepositoryDto, GoalReadinessStateDto, GoalRecordDto, GoalRecordScopeDto,
    GoalRepositoryDto, GoalRevisionRecordDto, GoalRoleCardRecordDto, GoalRoleClassDto,
    GoalScopeDto, GoalSessionLinkRecordDto, GoalSkillCardRecordDto, GoalTemplateLifecycleStateDto,
    GoalTemplateProvenanceKindDto, GoalTemplateScopeDto, GoalUserDecisionStateDto,
    GoalVerificationRepositoryDto, MemoryKindDto, RecordCompactionSuffixReferenceInputDto,
    RecordGoalUserDecisionInputDto, RefinementDraftRecordDto, RefinementDraftStateDto,
    RevokeVerifierAuthorityInputDto, SetGoalReadinessInputDto, TransitionGoalGateTemplateInputDto,
    TransitionGoalLifecycleInputDto, VerifierAuditBaselineRecordDto,
    VerifierAuditEvidenceRecordDto, VerifierAuditVerdictRecordDto,
    VerifierAuthorityConsumptionRuleDto, VerifierAuthorityConsumptionStateDto,
    VerifierAuthorityRecordDto, VerifierAuthorityReferenceDto, VerifierContractReferenceDto,
    VerifierFrozenReferencesDto, VerifierGoalReferenceDto, VerifierTargetLifecycleDto,
    VerifierTargetMutationRecordDto, VerifierTargetReferenceDto,
};
use intention_types::{DtoResult, ErrorDto};

/// The fixture Goal identity seed.
const GOAL: u8 = 0x01;
/// The fixture project identity seed.
const PROJECT: u8 = 0x02;
/// The fixture session identity seed.
const SESSION: u8 = 0x03;
/// The fixture child Goal identity seed.
const CHILD: u8 = 0x05;
/// The fixture reference gate identity seed.
const GATE: u8 = 0x06;
/// The fixture executable gate identity seed.
const EXECUTABLE_GATE: u8 = 0x08;
/// The fixture gate template identity seed.
const TEMPLATE: u8 = 0x07;
/// The fixture project session link identity seed.
const LINK: u8 = 0x0a;
/// The fixture verifier authority identity seed.
const AUTHORITY: u8 = 0x50;
/// The fixture verifier Mandate identity seed.
const VERIFIER: u8 = 0x51;
/// The fixture target Mandate identity seed.
const TARGET: u8 = 0x60;

/// Formats one fixture identity as canonical UUID text.
fn identity_text(byte: u8) -> String {
    let hex: String = std::iter::repeat_n(format!("{byte:02x}"), 16).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// A fixed daemon-assigned identity of one repeated byte.
const fn identity(byte: u8) -> [u8; 16] {
    [byte; 16]
}

/// A fixed deterministic digest.
fn digest(seed: u8) -> Digest256 {
    Digest256::sha256(&[seed; 16])
}

/// The canonical digest text of one fixed digest.
fn digest_text(seed: u8) -> String {
    digest(seed).to_string()
}

/// Parses one stored `sha256:<64 lowercase hex>` digest text.
fn stored_digest(text: &str) -> Digest256 {
    Digest256::from_str_hex(
        text.strip_prefix("sha256:")
            .expect("stored digest text carries the sha256 namespace"),
    )
    .expect("stored digest text is canonical")
}

/// The fixture project Goal scope.
fn project_scope() -> GoalScopeDto {
    GoalScopeDto::Project {
        project_id: identity_text(PROJECT),
    }
}

/// The fixture session Goal scope of the owning project.
fn session_scope() -> GoalScopeDto {
    session_scope_in(SESSION)
}

/// One session Goal scope of the owning project in the given session.
fn session_scope_in(session_seed: u8) -> GoalScopeDto {
    GoalScopeDto::Session {
        project_id: identity_text(PROJECT),
        session_id: identity_text(session_seed),
    }
}

/// The fixture project record scope.
fn record_project_scope() -> GoalRecordScopeDto {
    GoalRecordScopeDto::Project {
        project_id: identity_text(PROJECT),
    }
}

/// The fixture active project Goal record.
fn project_goal() -> GoalRecordDto {
    GoalRecordDto {
        goal_id: identity_text(GOAL),
        scope: project_scope(),
        active_revision: 1,
        lifecycle_state: GoalLifecycleStateDto::Active,
        readiness_state: GoalReadinessStateDto::NotReady,
        user_decision_state: GoalUserDecisionStateDto::Unaccepted,
        created_at_ms: 1_000,
        updated_at_ms: 1_000,
    }
}

/// The fixture active session Goal record.
fn session_goal() -> GoalRecordDto {
    GoalRecordDto {
        scope: session_scope(),
        ..project_goal()
    }
}

/// One immutable Goal revision record.
fn revision_record(
    goal_seed: u8,
    revision: u64,
    required_gates: Vec<GoalGateRevisionReferenceDto>,
) -> GoalRevisionRecordDto {
    GoalRevisionRecordDto {
        goal_id: identity_text(goal_seed),
        revision,
        title: "Ship the Goal runtime".to_owned(),
        objective: "Admit goal-directed runs atomically".to_owned(),
        inherited_rule_references: Vec::new(),
        local_rule_references: vec![identity_text(0x11)],
        required_gate_references: required_gates,
        canonical_revision_digest: digest_text(0x12),
        created_at_ms: 1_000,
    }
}

/// One exact gate revision reference of the fixture gate.
fn gate_reference(gate_seed: u8, revision: u64) -> GoalGateRevisionReferenceDto {
    GoalGateRevisionReferenceDto {
        gate_id: identity_text(gate_seed),
        revision,
    }
}

/// One exact durable evidence reference of the fixture gate.
fn evidence(gate_seed: u8, kind: GoalEvidenceKindDto) -> GoalEvidenceReferenceDto {
    GoalEvidenceReferenceDto {
        evidence_id: identity_text(gate_seed.wrapping_add(0x20)),
        revision: 1,
        kind,
    }
}

/// The fixture explicit project Goal session link.
fn session_link() -> GoalSessionLinkRecordDto {
    GoalSessionLinkRecordDto {
        link_id: identity_text(LINK),
        project_goal_id: identity_text(GOAL),
        session_id: identity_text(SESSION),
        effective_from_revision: 1,
        canonical_link_digest: digest_text(0x13),
        created_at_ms: 1_000,
    }
}

/// The fixture reference gate of the required gate identity.
fn reference_gate(accepted: Vec<GoalEvidenceKindDto>) -> GoalGateRecordDto {
    GoalGateRecordDto {
        gate_id: identity_text(GATE),
        goal_id: identity_text(GOAL),
        definition: GoalGateDefinitionDto::Reference {
            evidence_contract_revision: 1,
            accepted_reference_kinds: accepted,
        },
        revision: 1,
        canonical_revision_digest: digest_text(0x14),
        created_at_ms: 1_000,
    }
}

/// The fixture executable gate over the fixture template revision.
fn executable_gate() -> GoalGateRecordDto {
    GoalGateRecordDto {
        gate_id: identity_text(EXECUTABLE_GATE),
        goal_id: identity_text(GOAL),
        definition: GoalGateDefinitionDto::Executable {
            template_id: identity_text(TEMPLATE),
            template_revision: 1,
        },
        revision: 1,
        canonical_revision_digest: digest_text(0x15),
        created_at_ms: 1_000,
    }
}

/// The fixture user-created enabled gate template record.
fn template_record() -> GoalGateTemplateRecordDto {
    GoalGateTemplateRecordDto {
        template_id: identity_text(TEMPLATE),
        revision: 1,
        scope: GoalTemplateScopeDto::Project {
            project_id: identity_text(PROJECT),
        },
        capability_reference: identity_text(0x0b),
        input_family: GoalGateInputFamilyDto::ClosedTextV1,
        requires_confirmation: false,
        lifecycle_state: GoalTemplateLifecycleStateDto::Enabled,
        provenance_kind: GoalTemplateProvenanceKindDto::User,
        provenance_draft_id: None,
        provenance_accepted_by_user: false,
        canonical_digest: digest_text(0x16),
        created_at_ms: 1_000,
    }
}

/// The fixture bounded memory card record of the project scope.
fn memory_card_record(revision: u64) -> GoalMemoryCardRecordDto {
    GoalMemoryCardRecordDto {
        record_id: identity_text(0x20),
        revision,
        kind: MemoryKindDto::Decision,
        scope: record_project_scope(),
        title: "Chosen admission order".to_owned(),
        safe_purpose: "Keep gate ordering stable".to_owned(),
        retained_content_reference: identity_text(0x21),
        canonical_digest: digest_text(0x22),
        created_at_ms: 1_000,
    }
}

/// The fixture bounded role card record with the given narrowing.
fn role_card_record(
    role_seed: u8,
    class: GoalRoleClassDto,
    tools: Vec<&str>,
    context_limit: u64,
) -> GoalRoleCardRecordDto {
    GoalRoleCardRecordDto {
        role_id: identity_text(role_seed),
        revision: 1,
        canonical_name: "audit-role".to_owned(),
        task: "Audit one exact frozen record".to_owned(),
        permitted_class: class,
        tool_subset: tools.into_iter().map(str::to_owned).collect(),
        context_limit_bytes: context_limit,
        result_limit_bytes: 4_096,
        canonical_digest: digest_text(role_seed.wrapping_add(1)),
        created_at_ms: 1_000,
    }
}

/// The fixture pending refinement draft of the fixture Goal.
fn draft_record(draft_seed: u8, base_revision: u64) -> RefinementDraftRecordDto {
    RefinementDraftRecordDto {
        draft_id: identity_text(draft_seed),
        source_run_id: identity_text(0x31),
        leading_goal_id: identity_text(GOAL),
        milestone: intention_storage::goal_repo::GoalMilestoneDto::TechnicalReadiness,
        base_goal_revision: base_revision,
        base_record_reference: identity_text(0x32),
        base_record_revision: 1,
        edits: vec![intention_storage::goal_repo::RefinementEditRecordDto {
            kind: intention_storage::goal_repo::RefinementEditKindDto::ReadinessClaim,
            evidence: evidence(GATE, GoalEvidenceKindDto::AcceptedUserDeclaration),
        }],
        evidence_references: vec![evidence(GATE, GoalEvidenceKindDto::AcceptedUserDeclaration)],
        safe_rationale: "Claim readiness against the frozen evidence".to_owned(),
        canonical_digest: digest_text(0x33),
        state: RefinementDraftStateDto::Pending,
        coalesced_evidence_count: 1,
        created_at_ms: 1_000,
        decided_at_ms: None,
    }
}

/// One bounded refinement proposal request of the fixture Goal.
fn propose_request(
    draft_seed: u8,
    base_revision: u64,
    rationale: &str,
) -> ProposeRefinementDraftRequestDto {
    ProposeRefinementDraftRequestDto {
        draft_id: identity_text(draft_seed),
        source_run_id: identity_text(0x31),
        leading_goal_id: identity_text(GOAL),
        milestone: GoalMilestoneDto::TechnicalReadiness,
        base_goal_revision: base_revision,
        base_record_reference: identity_text(0x32),
        base_record_revision: 1,
        edits: vec![RefinementEditV1 {
            kind: RefinementEditKindV1::ReadinessClaim,
            evidence: GoalEvidenceReferenceV1::new(
                identity(0x34),
                1,
                GoalEvidenceKindV1::AcceptedUserDeclaration,
            ),
        }],
        evidence_references: vec![GoalEvidenceReferenceV1::new(
            identity(0x34),
            1,
            GoalEvidenceKindV1::AcceptedUserDeclaration,
        )],
        safe_rationale: rationale.to_owned(),
        occurred_at_ms: 1_000,
    }
}

/// The fixture applicability context of the creating run.
fn applicability_context() -> GoalApplicabilityContextV1 {
    GoalApplicabilityContextV1 {
        project_id: identity(PROJECT),
        session_id: identity(SESSION),
        selected_goal_ids: vec![identity(GOAL)],
    }
}

/// One gate-template creation request of the fixture scope and run.
fn template_request() -> CreateGoalGateTemplateRequestDto {
    CreateGoalGateTemplateRequestDto {
        template_id: identity_text(TEMPLATE),
        revision: 1,
        scope: GoalTemplateScopeDto::Project {
            project_id: identity_text(PROJECT),
        },
        capability_reference: identity_text(0x0b),
        input_family: GoalGateInputFamilyDto::ClosedTextV1,
        requires_confirmation: true,
        provenance: GoalTemplateProvenanceV1::User,
        context: applicability_context(),
        occurred_at_ms: 2_000,
    }
}

/// One exact audit contract reference of the fixture verifier.
fn contract_reference() -> VerifierContractReferenceDto {
    VerifierContractReferenceDto {
        contract_id: identity_text(0x54),
        contract_revision: 1,
        canonical_contract_digest: digest_text(0x55),
    }
}

/// One exact target reference of the fixture target Mandate.
fn target_reference() -> VerifierTargetReferenceDto {
    VerifierTargetReferenceDto {
        target_mandate_id: identity_text(TARGET),
        target_revision: 3,
    }
}

/// One frozen reference set of the fixture Goal and audit contract.
fn frozen_references() -> VerifierFrozenReferencesDto {
    VerifierFrozenReferencesDto {
        goal_references: vec![VerifierGoalReferenceDto {
            goal_id: identity_text(GOAL),
            goal_revision: 1,
            canonical_goal_revision_digest: digest_text(0x56),
        }],
        contract_references: vec![contract_reference()],
    }
}

/// One authority issuance request of the fixture verifier.
fn authority_request(
    rule: VerifierAuthorityConsumptionRuleV1,
    expires_at_ms: Option<u64>,
) -> RecordVerifierAuthorityRequestDto {
    RecordVerifierAuthorityRequestDto {
        authority_id: identity_text(AUTHORITY),
        authority_revision: 1,
        verifier_mandate_id: identity_text(VERIFIER),
        target_set_id: identity_text(0x52),
        target_set_digest: digest(0x53),
        allowed_operations: vec![
            VerifierOperationV1::MarkNeedsRework,
            VerifierOperationV1::MarkComplete,
        ],
        audit_contract_reference: contract_reference(),
        issued_at_ms: 1_000,
        expires_at_ms,
        consumption_rule: rule,
    }
}

/// One exact authority reference of the issued fixture authority.
fn authority_reference(
    authority: &VerificationMandateAuthorityDto,
) -> VerifierAuthorityReferenceDto {
    VerifierAuthorityReferenceDto {
        authority_id: identity_text(AUTHORITY),
        authority_revision: 1,
        canonical_authority_digest: authority.canonical_authority_digest.to_string(),
    }
}

/// One audit baseline request of the fixture target.
fn baseline_request(
    reference: VerifierAuthorityReferenceDto,
) -> RecordVerifierAuditBaselineRequestDto {
    RecordVerifierAuditBaselineRequestDto {
        authority_reference: reference,
        verifier_mandate_revision: 1,
        target_mandate_id: identity_text(TARGET),
        target_revision: 3,
        target_sequence: 7,
        target_lifecycle: VerifierTargetLifecycleDto::Active,
        frozen_references: frozen_references(),
        optional_unknown_effect_reference: None,
        audit_contract_reference: contract_reference(),
        graph_epoch: None,
        occurred_at_ms: 2_000,
    }
}

/// One audit evidence request of the given closed kind.
fn evidence_request(
    evidence_seed: u8,
    reference: VerifierAuthorityReferenceDto,
    kind: VerifierEvidenceKindV1,
) -> RecordVerifierEvidenceRequestDto {
    RecordVerifierEvidenceRequestDto {
        evidence_id: identity_text(evidence_seed),
        authority_reference: reference,
        target_reference: target_reference(),
        frozen_references: frozen_references(),
        evidence_kind: kind,
        retained_content_reference: identity_text(evidence_seed.wrapping_add(1)),
        occurred_at_ms: 2_500,
    }
}

/// The daemon-observed committed target state matching the fixture baseline.
fn committed_state(authority: &VerificationMandateAuthorityDto) -> VerifierCommittedStateV1 {
    VerifierCommittedStateV1 {
        target_mandate_id: identity(TARGET),
        target_revision: 3,
        target_aggregate_sequence: 7,
        target_lifecycle: VerifierTargetLifecycleV1::Active,
        authority_reference: VerifierAuthorityReferenceV1 {
            authority_id: identity(AUTHORITY),
            authority_revision: 1,
            authority_digest: authority.canonical_authority_digest,
        },
        audit_contract_reference: VerifierContractReferenceV1 {
            contract_id: identity(0x54),
            contract_revision: 1,
            contract_digest: digest(0x55),
        },
        graph_epoch: None,
        committed_operation: None,
    }
}

/// One verifier mutation request applying the given operation.
fn mutation_request(
    mutation_seed: u8,
    operation_seed: u8,
    reference: VerifierAuthorityReferenceDto,
    baseline_digest: Digest256,
    operation: VerifierOperationV1,
    audit_evidence: Vec<String>,
    committed: VerifierCommittedStateV1,
) -> ApplyVerifierTargetMutationRequestDto {
    ApplyVerifierTargetMutationRequestDto {
        mutation_id: identity_text(mutation_seed),
        verifier_mandate_id: identity_text(VERIFIER),
        authority_reference: reference,
        audit_contract_reference: contract_reference(),
        target_reference: target_reference(),
        operation,
        audit_evidence_references: audit_evidence,
        expected_target_revision: 3,
        expected_target_sequence: 7,
        expected_baseline_digest: baseline_digest,
        operation_id: identity_text(operation_seed),
        operation_digest: digest(operation_seed.wrapping_add(1)),
        observed_committed_state: committed,
        exact_uncertainty_reference: None,
        reconciliation_outcome: None,
        occurred_at_ms: 3_000,
    }
}

/// One selection of the fixture project Goal with the given revision.
fn project_selection(goal_revision: u64) -> GoalRunSelectionV1 {
    selection(
        identity(GOAL),
        goal_revision,
        GoalScopeLinkProvenanceV1 {
            project_id: identity(PROJECT),
            session_id: Some(identity(SESSION)),
            link_id: Some(identity(LINK)),
        },
    )
}

/// One goal-run selection with the given leading Goal and provenance.
fn selection(
    leading_goal_id: [u8; 16],
    goal_revision: u64,
    provenance: GoalScopeLinkProvenanceV1,
) -> GoalRunSelectionV1 {
    GoalRunSelectionV1 {
        leading_goal_id,
        goal_revision,
        scope_link_provenance: provenance,
        parent_revision_chain: Vec::new(),
        obligatory_component_references: Vec::new(),
        selected_gate_revisions: Vec::new(),
        valid_evidence_references: Vec::new(),
        selected_memory_cards: Vec::new(),
        selected_skill_cards: Vec::new(),
        selected_role_cards: Vec::new(),
        revealed_full_record_references: Vec::new(),
        policy_snapshot_reference: identity(0x40),
        activity_selection_reference: identity(0x41),
        run_kind: GoalRunKindV1::GoalDirectedOrdinary,
        target_snapshot_digest: digest(0x42),
        bounds: GoalRunSelectionBoundsV1 {
            max_memory_cards: 8,
            max_skill_role_cards: 8,
            target_snapshot_bytes: 65_536,
            context_bytes: 4_096,
        },
    }
}

/// One admission request of the given selection.
const fn admission_request(
    selection: GoalRunSelectionV1,
    snapshot_bytes: u64,
) -> GoalRunAdmissionRequestDto {
    GoalRunAdmissionRequestDto {
        selection,
        target_snapshot_bytes: snapshot_bytes,
        occurred_at_ms: 4_000,
    }
}

/// The recording admission port of one leading-goal run admission.
struct FakeAdmissionPort {
    log: Rc<RefCell<Vec<String>>>,
    commits: RefCell<Vec<GoalRunAdmissionCommitDto>>,
    replay: Cell<bool>,
    denial: RefCell<Option<&'static str>>,
}

impl FakeAdmissionPort {
    /// Creates an admitting port sharing one deterministic call log.
    const fn new(log: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            log,
            commits: RefCell::new(Vec::new()),
            replay: Cell::new(false),
            denial: RefCell::new(None),
        }
    }

    /// Returns every committed admission in order.
    fn commits(&self) -> Vec<GoalRunAdmissionCommitDto> {
        self.commits.borrow().clone()
    }

    /// Marks every subsequent commit as an equal replay.
    fn set_replay(&self) {
        self.replay.set(true);
    }

    /// Denies the next commit with one closed code.
    fn deny(&self, code: &'static str) {
        *self.denial.borrow_mut() = Some(code);
    }
}

impl GoalRunAdmissionPort for FakeAdmissionPort {
    fn commit_goal_run_admission(
        &self,
        input: &GoalRunAdmissionCommitDto,
    ) -> DtoResult<GoalRunAdmissionCommitOutcomeDto> {
        self.log
            .borrow_mut()
            .push(format!("admit:{}", input.admission_reference));
        self.commits.borrow_mut().push(input.clone());
        if let Some(code) = *self.denial.borrow() {
            return Err(ErrorDto::validation(
                code,
                "the admission port denied the atomic commit",
            ));
        }
        Ok(GoalRunAdmissionCommitOutcomeDto {
            replayed: self.replay.get(),
        })
    }
}

/// The in-memory Goal-domain repositories shared by every runtime test.
struct FakeGoalStore {
    log: Rc<RefCell<Vec<String>>>,
    goals: RefCell<Vec<GoalRecordDto>>,
    revisions: RefCell<Vec<GoalRevisionRecordDto>>,
    children: RefCell<Vec<GoalParentLinkRecordDto>>,
    session_links: RefCell<Vec<GoalSessionLinkRecordDto>>,
    gates: RefCell<Vec<GoalGateRecordDto>>,
    templates: RefCell<Vec<GoalGateTemplateRecordDto>>,
    gate_results: RefCell<Vec<GoalGateResultRecordDto>>,
    memory_cards: RefCell<Vec<GoalMemoryCardRecordDto>>,
    skill_cards: RefCell<Vec<GoalSkillCardRecordDto>>,
    role_cards: RefCell<Vec<GoalRoleCardRecordDto>>,
    drafts: RefCell<Vec<RefinementDraftRecordDto>>,
    summaries: RefCell<Vec<ConversationSummaryRecordDto>>,
    suffix: RefCell<Vec<(GoalRecordScopeDto, String)>>,
    authorities: RefCell<Vec<VerifierAuthorityRecordDto>>,
    baselines: RefCell<Vec<VerifierAuditBaselineRecordDto>>,
    evidence: RefCell<Vec<VerifierAuditEvidenceRecordDto>>,
    verdicts: RefCell<Vec<VerifierAuditVerdictRecordDto>>,
    mutations: RefCell<Vec<VerifierTargetMutationRecordDto>>,
    forced_project_count: Cell<u64>,
    forced_session_count: Cell<u64>,
    tree_depth: Cell<u64>,
}

impl FakeGoalStore {
    /// Creates an empty durable store.
    fn new() -> Self {
        Self {
            log: Rc::new(RefCell::new(Vec::new())),
            goals: RefCell::new(Vec::new()),
            revisions: RefCell::new(Vec::new()),
            children: RefCell::new(Vec::new()),
            session_links: RefCell::new(Vec::new()),
            gates: RefCell::new(Vec::new()),
            templates: RefCell::new(Vec::new()),
            gate_results: RefCell::new(Vec::new()),
            memory_cards: RefCell::new(Vec::new()),
            skill_cards: RefCell::new(Vec::new()),
            role_cards: RefCell::new(Vec::new()),
            drafts: RefCell::new(Vec::new()),
            summaries: RefCell::new(Vec::new()),
            suffix: RefCell::new(Vec::new()),
            authorities: RefCell::new(Vec::new()),
            baselines: RefCell::new(Vec::new()),
            evidence: RefCell::new(Vec::new()),
            verdicts: RefCell::new(Vec::new()),
            mutations: RefCell::new(Vec::new()),
            forced_project_count: Cell::new(0),
            forced_session_count: Cell::new(0),
            tree_depth: Cell::new(1),
        }
    }

    /// Records one observed call.
    fn record(&self, entry: impl Into<String>) {
        self.log.borrow_mut().push(entry.into());
    }

    /// Returns one shared handle on the deterministic call log.
    fn log_handle(&self) -> Rc<RefCell<Vec<String>>> {
        Rc::clone(&self.log)
    }

    /// Returns every observed call in order.
    fn calls(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    /// Whether one call name was observed.
    fn called(&self, name: &str) -> bool {
        self.log.borrow().iter().any(|call| call == name)
    }

    /// Seeds one durable Goal record.
    fn push_goal(&self, goal: GoalRecordDto) {
        self.goals.borrow_mut().push(goal);
    }

    /// Seeds one immutable Goal revision.
    fn push_revision(&self, revision: GoalRevisionRecordDto) {
        self.revisions.borrow_mut().push(revision);
    }

    /// Seeds one explicit session link.
    fn push_session_link(&self, link: GoalSessionLinkRecordDto) {
        self.session_links.borrow_mut().push(link);
    }

    /// Seeds one durable gate record.
    fn push_gate(&self, gate: GoalGateRecordDto) {
        self.gates.borrow_mut().push(gate);
    }

    /// Seeds one gate template record.
    fn push_template(&self, template: GoalGateTemplateRecordDto) {
        self.templates.borrow_mut().push(template);
    }

    /// Overrides the observed project Goal count.
    fn set_forced_project_count(&self, count: u64) {
        self.forced_project_count.set(count);
    }

    /// Overrides the observed session Goal count.
    fn set_forced_session_count(&self, count: u64) {
        self.forced_session_count.set(count);
    }

    /// Overrides the observed Goal-tree depth.
    fn set_tree_depth(&self, depth: u64) {
        self.tree_depth.set(depth);
    }

    /// Replaces the durable user-decision state of one Goal.
    fn set_goal_decision(&self, goal_id: &str, state: GoalUserDecisionStateDto) {
        for goal in self.goals.borrow_mut().iter_mut() {
            if goal.goal_id == goal_id {
                goal.user_decision_state = state.clone();
            }
        }
    }

    /// Replaces the durable lifecycle state of one Goal.
    fn set_goal_lifecycle(&self, goal_id: &str, state: GoalLifecycleStateDto) {
        for goal in self.goals.borrow_mut().iter_mut() {
            if goal.goal_id == goal_id {
                goal.lifecycle_state = state;
            }
        }
    }

    /// Returns the durable user-decision state of one Goal.
    fn goal_decision(&self, goal_id: &str) -> GoalUserDecisionStateDto {
        self.goals
            .borrow()
            .iter()
            .find(|goal| goal.goal_id == goal_id)
            .expect("the fixture Goal exists")
            .user_decision_state
            .clone()
    }

    /// Returns the durable lifecycle state of one Goal.
    fn goal_lifecycle(&self, goal_id: &str) -> GoalLifecycleStateDto {
        self.goals
            .borrow()
            .iter()
            .find(|goal| goal.goal_id == goal_id)
            .expect("the fixture Goal exists")
            .lifecycle_state
    }

    /// Returns the durable readiness state of one Goal.
    fn goal_readiness(&self, goal_id: &str) -> GoalReadinessStateDto {
        self.goals
            .borrow()
            .iter()
            .find(|goal| goal.goal_id == goal_id)
            .expect("the fixture Goal exists")
            .readiness_state
            .clone()
    }

    /// Returns the stored memory card of the given identity and revision.
    fn memory_card(&self, record_id: &str, revision: u64) -> Option<GoalMemoryCardRecordDto> {
        self.memory_cards
            .borrow()
            .iter()
            .find(|card| card.record_id == record_id && card.revision == revision)
            .cloned()
    }

    /// Returns the durable verification mutations in application order.
    fn mutations(&self) -> Vec<VerifierTargetMutationRecordDto> {
        self.mutations.borrow().clone()
    }

    /// Returns the durable authority records in issuance order.
    fn authorities(&self) -> Vec<VerifierAuthorityRecordDto> {
        self.authorities.borrow().clone()
    }

    /// Rewrites the recorded authority reference of one committed mutation.
    fn tamper_mutation_authority(&self, mutation_id: &str) {
        for mutation in self.mutations.borrow_mut().iter_mut() {
            if mutation.mutation_id == mutation_id {
                mutation.authority_reference.authority_id = identity_text(0x7f);
            }
        }
    }
}

impl GoalRepositoryDto for FakeGoalStore {
    fn create_goal(&self, input: CreateGoalInputDto) -> DtoResult<GoalRecordDto> {
        self.record("goal-create");
        input.validate()?;
        if self
            .goals
            .borrow()
            .iter()
            .any(|goal| goal.goal_id == input.goal.goal_id)
        {
            return Err(ErrorDto::validation(
                "goal_revision_conflict",
                "the Goal identity is already bound to different content",
            ));
        }
        self.goals.borrow_mut().push(input.goal.clone());
        self.revisions.borrow_mut().push(input.revision);
        Ok(input.goal)
    }

    fn load_goal(&self, goal_id: String) -> DtoResult<GoalRecordDto> {
        self.record("goal-read");
        self.goals
            .borrow()
            .iter()
            .find(|goal| goal.goal_id == goal_id)
            .cloned()
            .ok_or_else(|| ErrorDto::validation("goal_not_active", "the durable Goal is unknown"))
    }

    fn load_goal_revision(
        &self,
        goal_id: String,
        revision: u64,
    ) -> DtoResult<GoalRevisionRecordDto> {
        self.record("revision-read");
        self.revisions
            .borrow()
            .iter()
            .find(|record| record.goal_id == goal_id && record.revision == revision)
            .cloned()
            .ok_or_else(|| {
                ErrorDto::validation(
                    "goal_revision_conflict",
                    "the exact Goal revision is absent",
                )
            })
    }

    fn append_goal_revision(&self, input: AppendGoalRevisionInputDto) -> DtoResult<GoalRecordDto> {
        self.record("revision-append");
        input.validate()?;
        let mut goals = self.goals.borrow_mut();
        let goal = goals
            .iter_mut()
            .find(|goal| goal.goal_id == input.goal_id)
            .ok_or_else(|| {
                ErrorDto::validation("goal_not_active", "the durable Goal is unknown")
            })?;
        if goal.active_revision != input.expected_revision {
            return Err(ErrorDto::validation(
                "goal_revision_conflict",
                "the expected Goal revision is stale",
            ));
        }
        self.revisions.borrow_mut().push(input.revision);
        goal.active_revision = input.expected_revision.saturating_add(1);
        goal.updated_at_ms = 2_000;
        Ok(goal.clone())
    }

    fn transition_goal_lifecycle(
        &self,
        input: TransitionGoalLifecycleInputDto,
    ) -> DtoResult<GoalRecordDto> {
        self.record("lifecycle-transition");
        let mut goals = self.goals.borrow_mut();
        let goal = goals
            .iter_mut()
            .find(|goal| goal.goal_id == input.goal_id)
            .ok_or_else(|| {
                ErrorDto::validation("goal_not_active", "the durable Goal is unknown")
            })?;
        if goal.active_revision != input.expected_revision {
            return Err(ErrorDto::validation(
                "goal_revision_conflict",
                "the expected Goal revision is stale",
            ));
        }
        goal.lifecycle_state = input.lifecycle_state;
        goal.updated_at_ms = input.occurred_at_ms;
        Ok(goal.clone())
    }

    fn set_goal_readiness(&self, input: SetGoalReadinessInputDto) -> DtoResult<GoalRecordDto> {
        self.record("readiness-set");
        let mut goals = self.goals.borrow_mut();
        let goal = goals
            .iter_mut()
            .find(|goal| goal.goal_id == input.goal_id)
            .ok_or_else(|| {
                ErrorDto::validation("goal_not_active", "the durable Goal is unknown")
            })?;
        if goal.active_revision != input.expected_revision {
            return Err(ErrorDto::validation(
                "goal_revision_conflict",
                "the expected Goal revision is stale",
            ));
        }
        goal.readiness_state = input.readiness_state;
        goal.updated_at_ms = input.occurred_at_ms;
        Ok(goal.clone())
    }

    fn record_goal_user_decision(
        &self,
        input: RecordGoalUserDecisionInputDto,
    ) -> DtoResult<GoalRecordDto> {
        self.record("decision-record");
        let mut goals = self.goals.borrow_mut();
        let goal = goals
            .iter_mut()
            .find(|goal| goal.goal_id == input.goal_id)
            .ok_or_else(|| {
                ErrorDto::validation("goal_not_active", "the durable Goal is unknown")
            })?;
        if goal.active_revision != input.expected_revision {
            return Err(ErrorDto::validation(
                "goal_revision_conflict",
                "the expected Goal revision is stale",
            ));
        }
        goal.user_decision_state = input.user_decision_state;
        goal.updated_at_ms = input.occurred_at_ms;
        Ok(goal.clone())
    }

    fn attach_goal_child(
        &self,
        input: AttachGoalChildInputDto,
    ) -> DtoResult<GoalParentLinkRecordDto> {
        self.record("child-attach");
        if self.children.borrow().iter().any(|link| {
            link.parent_goal_id == input.parent_goal_id && link.child_goal_id == input.child_goal_id
        }) {
            return Err(ErrorDto::validation(
                "goal_cycle_detected",
                "the direct child link already exists",
            ));
        }
        let link = GoalParentLinkRecordDto {
            parent_goal_id: input.parent_goal_id,
            child_goal_id: input.child_goal_id,
            child_revision_at_link: input.child_revision_at_link,
            canonical_link_digest: input.canonical_link_digest,
            created_at_ms: input.created_at_ms,
        };
        self.children.borrow_mut().push(link.clone());
        Ok(link)
    }

    fn list_goal_children(
        &self,
        parent_goal_id: String,
    ) -> DtoResult<Vec<GoalParentLinkRecordDto>> {
        self.record("children-list");
        Ok(self
            .children
            .borrow()
            .iter()
            .filter(|link| link.parent_goal_id == parent_goal_id)
            .cloned()
            .collect())
    }

    fn load_goal_tree_depth(&self, goal_id: String) -> DtoResult<u64> {
        self.record("tree-depth");
        if self
            .goals
            .borrow()
            .iter()
            .any(|goal| goal.goal_id == goal_id)
        {
            Ok(self.tree_depth.get().max(1))
        } else {
            Err(ErrorDto::validation(
                "goal_not_active",
                "the durable Goal is unknown",
            ))
        }
    }

    fn create_goal_session_link(
        &self,
        input: CreateGoalSessionLinkInputDto,
    ) -> DtoResult<GoalSessionLinkRecordDto> {
        self.record("session-link-create");
        input.link.validate()?;
        if let Some(existing) = self
            .session_links
            .borrow()
            .iter()
            .find(|link| link.link_id == input.link.link_id)
        {
            if *existing == input.link {
                return Ok(existing.clone());
            }
            return Err(ErrorDto::validation(
                "goal_revision_conflict",
                "the session link identity is already bound",
            ));
        }
        self.session_links.borrow_mut().push(input.link.clone());
        Ok(input.link)
    }

    fn load_goal_session_link(&self, link_id: String) -> DtoResult<GoalSessionLinkRecordDto> {
        self.record("session-link-read");
        self.session_links
            .borrow()
            .iter()
            .find(|link| link.link_id == link_id)
            .cloned()
            .ok_or_else(|| ErrorDto::validation("goal_not_active", "the session link is unknown"))
    }

    fn load_goal_session_links(
        &self,
        project_goal_id: String,
    ) -> DtoResult<Vec<GoalSessionLinkRecordDto>> {
        self.record("session-links-read");
        Ok(self
            .session_links
            .borrow()
            .iter()
            .filter(|link| link.project_goal_id == project_goal_id)
            .cloned()
            .collect())
    }

    fn count_goals_in_project(&self, project_id: String) -> DtoResult<u64> {
        self.record("project-count");
        let forced = self.forced_project_count.get();
        if forced > 0 {
            return Ok(forced);
        }
        let count = self
            .goals
            .borrow()
            .iter()
            .filter(|goal| goal.scope.project_id() == project_id)
            .count();
        Ok(u64::try_from(count).unwrap_or(u64::MAX))
    }

    fn count_goals_in_session(&self, session_id: String) -> DtoResult<u64> {
        self.record("session-count");
        let forced = self.forced_session_count.get();
        if forced > 0 {
            return Ok(forced);
        }
        let count = self
            .goals
            .borrow()
            .iter()
            .filter(|goal| goal.scope.session_id() == Some(session_id.as_str()))
            .count();
        Ok(u64::try_from(count).unwrap_or(u64::MAX))
    }
}

impl GoalGateRepositoryDto for FakeGoalStore {
    fn create_goal_gate(&self, input: CreateGoalGateInputDto) -> DtoResult<GoalGateRecordDto> {
        self.record("gate-create");
        if self
            .gates
            .borrow()
            .iter()
            .any(|gate| gate.gate_id == input.gate.gate_id)
        {
            return Err(ErrorDto::validation(
                "goal_revision_conflict",
                "the gate identity is already bound",
            ));
        }
        self.gates.borrow_mut().push(input.gate.clone());
        Ok(input.gate)
    }

    fn load_goal_gate(&self, gate_id: String) -> DtoResult<GoalGateRecordDto> {
        self.record("gate-read");
        self.gates
            .borrow()
            .iter()
            .find(|gate| gate.gate_id == gate_id)
            .cloned()
            .ok_or_else(|| ErrorDto::validation("goal_gate_unavailable", "the gate is unknown"))
    }

    fn append_goal_gate_revision(
        &self,
        input: AppendGoalGateRevisionInputDto,
    ) -> DtoResult<GoalGateRecordDto> {
        self.record("gate-append");
        input.validate()?;
        let mut gates = self.gates.borrow_mut();
        let gate = gates
            .iter_mut()
            .find(|gate| gate.gate_id == input.gate_id)
            .ok_or_else(|| ErrorDto::validation("goal_gate_unavailable", "the gate is unknown"))?;
        if gate.revision != input.expected_revision {
            return Err(ErrorDto::validation(
                "goal_revision_conflict",
                "the expected gate revision is stale",
            ));
        }
        gate.definition = input.definition;
        gate.revision = input.expected_revision.saturating_add(1);
        gate.canonical_revision_digest = input.canonical_revision_digest;
        Ok(gate.clone())
    }

    fn create_goal_gate_template(
        &self,
        input: GoalGateTemplateRecordDto,
    ) -> DtoResult<GoalGateTemplateRecordDto> {
        self.record("template-create");
        input.validate()?;
        if let Some(existing) = self.templates.borrow().iter().find(|template| {
            template.template_id == input.template_id && template.revision == input.revision
        }) {
            if *existing == input {
                return Ok(existing.clone());
            }
            return Err(ErrorDto::validation(
                "goal_revision_conflict",
                "the template revision is already bound to different content",
            ));
        }
        self.templates.borrow_mut().push(input.clone());
        Ok(input)
    }

    fn load_goal_gate_template(
        &self,
        template_id: String,
        revision: u64,
    ) -> DtoResult<GoalGateTemplateRecordDto> {
        self.record("template-read");
        self.templates
            .borrow()
            .iter()
            .find(|template| template.template_id == template_id && template.revision == revision)
            .cloned()
            .ok_or_else(|| {
                ErrorDto::validation(
                    "goal_gate_unavailable",
                    "the exact template revision is absent",
                )
            })
    }

    fn transition_goal_gate_template_lifecycle(
        &self,
        input: TransitionGoalGateTemplateInputDto,
    ) -> DtoResult<GoalGateTemplateRecordDto> {
        self.record("template-transition");
        let mut templates = self.templates.borrow_mut();
        let template = templates
            .iter_mut()
            .find(|template| {
                template.template_id == input.template_id && template.revision == input.revision
            })
            .ok_or_else(|| {
                ErrorDto::validation(
                    "goal_gate_unavailable",
                    "the exact template revision is absent",
                )
            })?;
        template.lifecycle_state = input.lifecycle_state;
        Ok(template.clone())
    }

    fn record_goal_gate_result(
        &self,
        input: GoalGateResultRecordDto,
    ) -> DtoResult<GoalGateResultRecordDto> {
        self.record("gate-result-record");
        input.validate()?;
        if let Some(existing) = self.gate_results.borrow().iter().find(|result| {
            result.gate_id == input.gate_id && result.gate_revision == input.gate_revision
        }) {
            if *existing == input {
                return Ok(existing.clone());
            }
            return Err(ErrorDto::validation(
                "goal_revision_conflict",
                "the gate result revision is already bound to different content",
            ));
        }
        self.gate_results.borrow_mut().push(input.clone());
        Ok(input)
    }

    fn load_goal_gate_result(
        &self,
        gate_id: String,
        gate_revision: u64,
    ) -> DtoResult<GoalGateResultRecordDto> {
        self.record("gate-result-read");
        self.gate_results
            .borrow()
            .iter()
            .find(|result| result.gate_id == gate_id && result.gate_revision == gate_revision)
            .cloned()
            .ok_or_else(|| {
                ErrorDto::validation("goal_gate_unavailable", "the gate result is absent")
            })
    }
}

impl GoalCardRepositoryDto for FakeGoalStore {
    fn store_goal_memory_card(
        &self,
        input: GoalMemoryCardRecordDto,
    ) -> DtoResult<GoalMemoryCardRecordDto> {
        self.record("memory-card-store");
        input.validate()?;
        if let Some(existing) = self
            .memory_cards
            .borrow()
            .iter()
            .find(|card| card.record_id == input.record_id && card.revision == input.revision)
        {
            if *existing == input {
                return Ok(existing.clone());
            }
            return Err(ErrorDto::validation(
                "memory_reference_unavailable",
                "the memory card revision is already bound to different content",
            ));
        }
        self.memory_cards.borrow_mut().push(input.clone());
        Ok(input)
    }

    fn load_goal_memory_card(
        &self,
        record_id: String,
        revision: u64,
    ) -> DtoResult<GoalMemoryCardRecordDto> {
        self.record("memory-card-read");
        self.memory_card(&record_id, revision).ok_or_else(|| {
            ErrorDto::validation("memory_reference_unavailable", "the memory card is absent")
        })
    }

    fn replace_goal_memory_card(
        &self,
        input: GoalMemoryCardReplacementInputDto,
    ) -> DtoResult<GoalMemoryCardRecordDto> {
        self.record("memory-card-replace");
        input.validate()?;
        self.memory_cards
            .borrow_mut()
            .push(input.replacement.clone());
        Ok(input.replacement)
    }

    fn rollback_goal_memory_card(
        &self,
        input: GoalMemoryCardRollbackInputDto,
    ) -> DtoResult<GoalMemoryCardRecordDto> {
        self.record("memory-card-rollback");
        input.validate()?;
        self.memory_cards
            .borrow_mut()
            .push(input.replacement.clone());
        Ok(input.replacement)
    }

    fn count_active_goal_memory_cards(&self, scope: GoalRecordScopeDto) -> DtoResult<u64> {
        self.record("memory-card-count");
        let count = self
            .memory_cards
            .borrow()
            .iter()
            .filter(|card| card.scope == scope)
            .count();
        Ok(u64::try_from(count).unwrap_or(u64::MAX))
    }

    fn store_goal_skill_card(
        &self,
        input: GoalSkillCardRecordDto,
    ) -> DtoResult<GoalSkillCardRecordDto> {
        self.record("skill-card-store");
        input.validate()?;
        if let Some(existing) = self
            .skill_cards
            .borrow()
            .iter()
            .find(|card| card.skill_id == input.skill_id && card.revision == input.revision)
        {
            if *existing == input {
                return Ok(existing.clone());
            }
            return Err(ErrorDto::validation(
                "skill_reference_unavailable",
                "the Skill card revision is already bound to different content",
            ));
        }
        self.skill_cards.borrow_mut().push(input.clone());
        Ok(input)
    }

    fn load_goal_skill_card(
        &self,
        skill_id: String,
        revision: u64,
    ) -> DtoResult<GoalSkillCardRecordDto> {
        self.record("skill-card-read");
        self.skill_cards
            .borrow()
            .iter()
            .find(|card| card.skill_id == skill_id && card.revision == revision)
            .cloned()
            .ok_or_else(|| {
                ErrorDto::validation("skill_reference_unavailable", "the Skill card is absent")
            })
    }

    fn store_goal_role_card(
        &self,
        input: GoalRoleCardRecordDto,
    ) -> DtoResult<GoalRoleCardRecordDto> {
        self.record("role-card-store");
        input.validate()?;
        if let Some(existing) = self
            .role_cards
            .borrow()
            .iter()
            .find(|card| card.role_id == input.role_id && card.revision == input.revision)
        {
            if *existing == input {
                return Ok(existing.clone());
            }
            return Err(ErrorDto::validation(
                "delegation_role_invalid",
                "the role card revision is already bound to different content",
            ));
        }
        self.role_cards.borrow_mut().push(input.clone());
        Ok(input)
    }

    fn load_goal_role_card(
        &self,
        role_id: String,
        revision: u64,
    ) -> DtoResult<GoalRoleCardRecordDto> {
        self.record("role-card-read");
        self.role_cards
            .borrow()
            .iter()
            .find(|card| card.role_id == role_id && card.revision == revision)
            .cloned()
            .ok_or_else(|| {
                ErrorDto::validation("delegation_role_invalid", "the role card is absent")
            })
    }
}

impl GoalProposalRepositoryDto for FakeGoalStore {
    fn propose_refinement_draft(
        &self,
        input: RefinementDraftRecordDto,
    ) -> DtoResult<RefinementDraftRecordDto> {
        self.record("draft-propose");
        input.validate()?;
        if let Some(existing) = self.drafts.borrow().iter().find(|draft| {
            draft.leading_goal_id == input.leading_goal_id
                && draft.state == RefinementDraftStateDto::Pending
        }) {
            if existing.canonical_digest == input.canonical_digest {
                return Ok(existing.clone());
            }
            return Err(ErrorDto::validation(
                "refinement_draft_conflict",
                "an unequal proposal already owns the pending slot",
            ));
        }
        self.drafts.borrow_mut().push(input.clone());
        Ok(input)
    }

    fn load_pending_refinement_draft(
        &self,
        leading_goal_id: String,
    ) -> DtoResult<Option<RefinementDraftRecordDto>> {
        self.record("draft-read");
        Ok(self
            .drafts
            .borrow()
            .iter()
            .find(|draft| {
                draft.leading_goal_id == leading_goal_id
                    && draft.state == RefinementDraftStateDto::Pending
            })
            .cloned())
    }

    fn decide_refinement_draft(
        &self,
        draft_id: String,
        state: RefinementDraftStateDto,
        decided_at_ms: u64,
    ) -> DtoResult<RefinementDraftRecordDto> {
        self.record("draft-decide");
        let mut drafts = self.drafts.borrow_mut();
        let draft = drafts
            .iter_mut()
            .find(|draft| {
                draft.draft_id == draft_id && draft.state == RefinementDraftStateDto::Pending
            })
            .ok_or_else(|| {
                ErrorDto::validation("refinement_draft_conflict", "the draft is not pending")
            })?;
        draft.state = state;
        draft.decided_at_ms = Some(decided_at_ms);
        Ok(draft.clone())
    }
}

impl GoalCompactionRepositoryDto for FakeGoalStore {
    fn record_compaction_suffix_reference(
        &self,
        input: RecordCompactionSuffixReferenceInputDto,
    ) -> DtoResult<()> {
        self.record("suffix-record");
        self.suffix
            .borrow_mut()
            .push((input.scope, input.history_reference));
        Ok(())
    }

    fn store_conversation_summary(
        &self,
        input: ConversationSummaryRecordDto,
    ) -> DtoResult<ConversationSummaryRecordDto> {
        self.record("summary-store");
        input.validate()?;
        if let Some(existing) = self.summaries.borrow().iter().find(|summary| {
            summary.summary_id == input.summary_id && summary.revision == input.revision
        }) {
            if *existing == input {
                return Ok(existing.clone());
            }
            return Err(ErrorDto::validation(
                "compaction_summary_unavailable",
                "the summary revision is already bound to different content",
            ));
        }
        self.summaries.borrow_mut().push(input.clone());
        let scope = input.scope.clone();
        let end = input.source_range_end.clone();
        let mut remaining = Vec::new();
        let mut consumed = false;
        for (entry_scope, reference) in self.suffix.borrow_mut().drain(..) {
            if entry_scope == scope && !consumed {
                if reference == end {
                    consumed = true;
                }
            } else {
                remaining.push((entry_scope, reference));
            }
        }
        *self.suffix.borrow_mut() = remaining;
        Ok(input)
    }

    fn load_conversation_summary(
        &self,
        summary_id: String,
        revision: u64,
    ) -> DtoResult<ConversationSummaryRecordDto> {
        self.record("summary-read");
        self.summaries
            .borrow()
            .iter()
            .find(|summary| summary.summary_id == summary_id && summary.revision == revision)
            .cloned()
            .ok_or_else(|| {
                ErrorDto::validation(
                    "compaction_summary_unavailable",
                    "the exact summary revision is absent",
                )
            })
    }

    fn load_goal_compaction_working_form(
        &self,
        scope: GoalRecordScopeDto,
    ) -> DtoResult<GoalCompactionWorkingFormRecordDto> {
        self.record("working-form-read");
        let current_summary = self
            .summaries
            .borrow()
            .iter()
            .rev()
            .find(|summary| summary.scope == scope)
            .cloned();
        let uncompacted_suffix = self
            .suffix
            .borrow()
            .iter()
            .filter(|(entry_scope, _)| *entry_scope == scope)
            .map(|(_, reference)| reference.clone())
            .collect();
        Ok(GoalCompactionWorkingFormRecordDto {
            current_summary,
            uncompacted_suffix,
        })
    }
}

impl GoalVerificationRepositoryDto for FakeGoalStore {
    fn record_verifier_authority(
        &self,
        input: VerifierAuthorityRecordDto,
    ) -> DtoResult<VerifierAuthorityRecordDto> {
        self.record("authority-record");
        input.validate()?;
        if let Some(existing) = self.authorities.borrow().iter().find(|authority| {
            authority.authority_id == input.authority_id
                && authority.authority_revision == input.authority_revision
        }) {
            if *existing == input {
                return Ok(existing.clone());
            }
            return Err(ErrorDto::validation(
                "verifier_authority_invalid",
                "the authority revision is already bound to a different digest",
            ));
        }
        self.authorities.borrow_mut().push(input.clone());
        Ok(input)
    }

    fn load_verifier_authority(
        &self,
        authority_id: String,
        authority_revision: u64,
    ) -> DtoResult<VerifierAuthorityRecordDto> {
        self.record("authority-read");
        self.authorities
            .borrow()
            .iter()
            .find(|authority| {
                authority.authority_id == authority_id
                    && authority.authority_revision == authority_revision
            })
            .cloned()
            .ok_or_else(|| {
                ErrorDto::validation("verifier_authority_invalid", "the authority is absent")
            })
    }

    fn revoke_verifier_authority(
        &self,
        input: RevokeVerifierAuthorityInputDto,
    ) -> DtoResult<VerifierAuthorityRecordDto> {
        self.record("authority-revoke");
        input.validate()?;
        let mut authorities = self.authorities.borrow_mut();
        let authority = authorities
            .iter_mut()
            .find(|authority| {
                authority.authority_id == input.authority_id
                    && authority.authority_revision == input.authority_revision
            })
            .ok_or_else(|| {
                ErrorDto::validation("verifier_authority_invalid", "the authority is absent")
            })?;
        if authority.revoked_at_ms.is_some() {
            return Err(ErrorDto::validation(
                "verifier_authority_revoked",
                "the authority revision is already revoked",
            ));
        }
        authority.revoked_at_ms = Some(input.revoked_at_ms);
        authority.revocation_reference = Some(input.revocation_reference);
        Ok(authority.clone())
    }

    fn record_verifier_audit_baseline(
        &self,
        input: VerifierAuditBaselineRecordDto,
    ) -> DtoResult<VerifierAuditBaselineRecordDto> {
        self.record("baseline-record");
        input.validate()?;
        if let Some(existing) =
            self.baselines.borrow().iter().find(|baseline| {
                baseline.canonical_baseline_digest == input.canonical_baseline_digest
            })
        {
            if *existing == input {
                return Ok(existing.clone());
            }
            return Err(ErrorDto::validation(
                "verifier_baseline_invalid",
                "the baseline digest is already bound to different content",
            ));
        }
        self.baselines.borrow_mut().push(input.clone());
        Ok(input)
    }

    fn load_verifier_audit_baseline(
        &self,
        canonical_baseline_digest: String,
    ) -> DtoResult<VerifierAuditBaselineRecordDto> {
        self.record("baseline-read");
        self.baselines
            .borrow()
            .iter()
            .find(|baseline| baseline.canonical_baseline_digest == canonical_baseline_digest)
            .cloned()
            .ok_or_else(|| {
                ErrorDto::validation("verifier_baseline_invalid", "the baseline is absent")
            })
    }

    fn record_verifier_audit_evidence(
        &self,
        input: VerifierAuditEvidenceRecordDto,
    ) -> DtoResult<VerifierAuditEvidenceRecordDto> {
        self.record("evidence-record");
        input.validate()?;
        if let Some(existing) = self
            .evidence
            .borrow()
            .iter()
            .find(|evidence| evidence.evidence_id == input.evidence_id)
        {
            if *existing == input {
                return Ok(existing.clone());
            }
            return Err(ErrorDto::validation(
                "verifier_evidence_invalid",
                "the evidence identity is already bound to a different digest",
            ));
        }
        self.evidence.borrow_mut().push(input.clone());
        Ok(input)
    }

    fn load_verifier_audit_evidence(
        &self,
        evidence_id: String,
    ) -> DtoResult<VerifierAuditEvidenceRecordDto> {
        self.record("evidence-read");
        self.evidence
            .borrow()
            .iter()
            .find(|evidence| evidence.evidence_id == evidence_id)
            .cloned()
            .ok_or_else(|| {
                ErrorDto::validation("verifier_evidence_invalid", "the evidence is absent")
            })
    }

    fn record_verifier_audit_verdict(
        &self,
        input: VerifierAuditVerdictRecordDto,
    ) -> DtoResult<VerifierAuditVerdictRecordDto> {
        self.record("verdict-record");
        input.validate()?;
        if let Some(existing) = self
            .verdicts
            .borrow()
            .iter()
            .find(|verdict| verdict.verdict_id == input.verdict_id)
        {
            if *existing == input {
                return Ok(existing.clone());
            }
            return Err(ErrorDto::validation(
                "verifier_verdict_invalid",
                "the verdict identity is already bound to a different digest",
            ));
        }
        self.verdicts.borrow_mut().push(input.clone());
        Ok(input)
    }

    fn load_verifier_audit_verdict(
        &self,
        verdict_id: String,
    ) -> DtoResult<VerifierAuditVerdictRecordDto> {
        self.record("verdict-read");
        self.verdicts
            .borrow()
            .iter()
            .find(|verdict| verdict.verdict_id == verdict_id)
            .cloned()
            .ok_or_else(|| {
                ErrorDto::validation("verifier_verdict_invalid", "the verdict is absent")
            })
    }

    fn apply_verifier_target_mutation(
        &self,
        input: VerifierTargetMutationRecordDto,
    ) -> DtoResult<ApplyVerifierMutationOutcomeDto> {
        self.record("mutation-apply");
        input.validate()?;
        if let Some(existing) = self
            .mutations
            .borrow()
            .iter()
            .find(|mutation| mutation.idempotency.operation_id == input.idempotency.operation_id)
            .cloned()
        {
            let authority = self.load_verifier_authority(
                input.authority_reference.authority_id.clone(),
                input.authority_reference.authority_revision,
            )?;
            return Ok(ApplyVerifierMutationOutcomeDto {
                mutation: existing,
                authority,
                replayed: true,
            });
        }
        self.mutations.borrow_mut().push(input.clone());
        let mut authority = self.load_verifier_authority(
            input.authority_reference.authority_id.clone(),
            input.authority_reference.authority_revision,
        )?;
        if authority.consumption_rule == VerifierAuthorityConsumptionRuleDto::SingleUse {
            authority.consumption_state = VerifierAuthorityConsumptionStateDto::Consumed;
            authority.consumed_by_mutation_reference = Some(input.mutation_id.clone());
        }
        for stored in self.authorities.borrow_mut().iter_mut() {
            if stored.authority_id == authority.authority_id
                && stored.authority_revision == authority.authority_revision
            {
                *stored = authority.clone();
            }
        }
        Ok(ApplyVerifierMutationOutcomeDto {
            mutation: input,
            authority,
            replayed: false,
        })
    }

    fn load_verifier_target_mutation(
        &self,
        mutation_id: String,
    ) -> DtoResult<VerifierTargetMutationRecordDto> {
        self.record("mutation-read");
        self.mutations
            .borrow()
            .iter()
            .find(|mutation| mutation.mutation_id == mutation_id)
            .cloned()
            .ok_or_else(|| {
                ErrorDto::validation("verifier_mutation_invalid", "the mutation is absent")
            })
    }
}

/// Seeds the fixture project Goal, its revision, and its session link.
fn seed_project_goal(store: &FakeGoalStore) {
    store.push_goal(project_goal());
    store.push_revision(revision_record(GOAL, 1, Vec::new()));
    store.push_session_link(session_link());
}

/// Seeds one session child Goal of the fixture project.
fn seed_session_child(store: &FakeGoalStore, seed: u8) {
    seed_session_child_in(store, seed, SESSION);
}

/// Seeds one session child Goal of the given session identity.
fn seed_session_child_in(store: &FakeGoalStore, seed: u8, session_seed: u8) {
    store.push_goal(GoalRecordDto {
        goal_id: identity_text(seed),
        scope: session_scope_in(session_seed),
        active_revision: 1,
        lifecycle_state: GoalLifecycleStateDto::Active,
        readiness_state: GoalReadinessStateDto::NotReady,
        user_decision_state: GoalUserDecisionStateDto::Unaccepted,
        created_at_ms: 1_000,
        updated_at_ms: 1_000,
    });
    store.push_revision(revision_record(seed, 1, Vec::new()));
}

#[test]
fn create_goal_admits_first_revision_and_rejects_noncanonical_input() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);
    let created = service
        .create_goal(&CreateGoalRequestDto {
            goal_id: identity_text(GOAL),
            scope: project_scope(),
            title: "Ship the Goal runtime".to_owned(),
            objective: "Admit goal-directed runs atomically".to_owned(),
            inherited_rule_references: Vec::new(),
            local_rule_references: vec![identity_text(0x11)],
            required_gate_references: Vec::new(),
            occurred_at_ms: 1_000,
        })
        .expect("the project Goal is admitted");
    assert_eq!(created.active_revision, 1);
    assert_eq!(created.lifecycle_state, GoalLifecycleStateDto::Active);
    assert_eq!(store.revisions.borrow().len(), 1);
    assert_eq!(store.revisions.borrow()[0].revision, 1);

    let credential = service
        .create_goal(&CreateGoalRequestDto {
            goal_id: identity_text(0x04),
            scope: project_scope(),
            title: "api_key=secret-value".to_owned(),
            objective: "Admit goal-directed runs atomically".to_owned(),
            inherited_rule_references: Vec::new(),
            local_rule_references: Vec::new(),
            required_gate_references: Vec::new(),
            occurred_at_ms: 1_000,
        })
        .expect_err("credential-shaped text is rejected");
    assert_eq!(credential.code(), "credentials_forbidden");

    let duplicate = service
        .create_goal(&CreateGoalRequestDto {
            goal_id: identity_text(GOAL),
            scope: project_scope(),
            title: "Ship the Goal runtime".to_owned(),
            objective: "Admit goal-directed runs atomically".to_owned(),
            inherited_rule_references: Vec::new(),
            local_rule_references: vec![identity_text(0x11)],
            required_gate_references: Vec::new(),
            occurred_at_ms: 1_000,
        })
        .expect_err("a duplicate Goal identity is rejected");
    assert_eq!(duplicate.code(), "goal_revision_conflict");
}

#[test]
fn create_goal_rejects_project_and_session_bounds_before_the_commit() {
    let store = FakeGoalStore::new();
    store.set_forced_project_count(256);
    let service = GoalRuntimeService::new(&store);
    let error = service
        .create_goal(&CreateGoalRequestDto {
            goal_id: identity_text(GOAL),
            scope: project_scope(),
            title: "Ship the Goal runtime".to_owned(),
            objective: "Admit goal-directed runs atomically".to_owned(),
            inherited_rule_references: Vec::new(),
            local_rule_references: Vec::new(),
            required_gate_references: Vec::new(),
            occurred_at_ms: 1_000,
        })
        .expect_err("the project Goal bound is exceeded");
    assert_eq!(error.code(), "goal_limit_exceeded");
    assert!(!store.called("goal-create"));

    let store = FakeGoalStore::new();
    store.set_forced_session_count(64);
    let service = GoalRuntimeService::new(&store);
    let error = service
        .create_goal(&CreateGoalRequestDto {
            goal_id: identity_text(GOAL),
            scope: session_scope(),
            title: "Ship the Goal runtime".to_owned(),
            objective: "Admit goal-directed runs atomically".to_owned(),
            inherited_rule_references: Vec::new(),
            local_rule_references: Vec::new(),
            required_gate_references: Vec::new(),
            occurred_at_ms: 1_000,
        })
        .expect_err("the session Goal bound is exceeded");
    assert_eq!(error.code(), "goal_limit_exceeded");
    assert!(!store.called("goal-create"));
}

#[test]
fn append_goal_revision_advances_and_never_drops_a_base_reference() {
    let store = FakeGoalStore::new();
    store.push_goal(project_goal());
    store.push_revision(revision_record(GOAL, 1, vec![gate_reference(GATE, 1)]));
    store.push_gate(reference_gate(vec![
        GoalEvidenceKindDto::AcceptedUserDeclaration,
    ]));
    let service = GoalRuntimeService::new(&store);

    let appended = service
        .append_goal_revision(&AppendGoalRevisionRequestDto {
            goal_id: identity_text(GOAL),
            expected_revision: 1,
            title: "Ship the Goal runtime".to_owned(),
            objective: "Admit goal-directed runs atomically".to_owned(),
            inherited_rule_references: Vec::new(),
            local_rule_references: vec![identity_text(0x11)],
            required_gate_references: vec![gate_reference(GATE, 1)],
            occurred_at_ms: 2_000,
        })
        .expect("the next revision is admitted");
    assert_eq!(appended.active_revision, 2);
    assert_eq!(store.revisions.borrow().len(), 2);
    assert_eq!(store.revisions.borrow()[1].revision, 2);

    let dropped = service
        .append_goal_revision(&AppendGoalRevisionRequestDto {
            goal_id: identity_text(GOAL),
            expected_revision: 2,
            title: "Ship the Goal runtime".to_owned(),
            objective: "Admit goal-directed runs atomically".to_owned(),
            inherited_rule_references: Vec::new(),
            local_rule_references: vec![identity_text(0x11)],
            required_gate_references: Vec::new(),
            occurred_at_ms: 2_000,
        })
        .expect_err("a revision cannot drop a required gate of its base");
    assert_eq!(dropped.code(), "goal_revision_conflict");

    let stale = service
        .append_goal_revision(&AppendGoalRevisionRequestDto {
            goal_id: identity_text(GOAL),
            expected_revision: 1,
            title: "Ship the Goal runtime".to_owned(),
            objective: "Admit goal-directed runs atomically".to_owned(),
            inherited_rule_references: Vec::new(),
            local_rule_references: vec![identity_text(0x11)],
            required_gate_references: vec![gate_reference(GATE, 1)],
            occurred_at_ms: 2_000,
        })
        .expect_err("a stale expected revision is rejected");
    assert_eq!(stale.code(), "goal_revision_conflict");
    assert_eq!(store.revisions.borrow().len(), 2);
}

#[test]
fn attach_child_creates_session_link_atomically_and_rejects_duplicates() {
    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    seed_session_child_in(&store, CHILD, 0x1c);
    let service = GoalRuntimeService::new(&store);

    let link = service
        .attach_goal_child(&AttachGoalChildRequestDto {
            parent_goal_id: identity_text(GOAL),
            child_goal_id: identity_text(CHILD),
            child_revision_at_link: 1,
            session_link: Some(
                intention_application::goal_domain::GoalSessionLinkCreationRequestDto {
                    link_id: identity_text(0x0c),
                    session_id: identity_text(0x1c),
                    effective_from_revision: 1,
                },
            ),
            occurred_at_ms: 2_000,
        })
        .expect("the session child is linked with its explicit link");
    assert_eq!(link.child_goal_id, identity_text(CHILD));
    assert_eq!(store.children.borrow().len(), 1);
    assert_eq!(store.session_links.borrow().len(), 2);

    let duplicate = service
        .attach_goal_child(&AttachGoalChildRequestDto {
            parent_goal_id: identity_text(GOAL),
            child_goal_id: identity_text(CHILD),
            child_revision_at_link: 1,
            session_link: None,
            occurred_at_ms: 2_000,
        })
        .expect_err("a duplicate direct child is rejected");
    assert_eq!(duplicate.code(), "goal_cycle_detected");
    assert_eq!(store.children.borrow().len(), 1);

    seed_session_child_in(&store, 0x1d, 0x1c);
    let redundant = service
        .attach_goal_child(&AttachGoalChildRequestDto {
            parent_goal_id: identity_text(GOAL),
            child_goal_id: identity_text(0x1d),
            child_revision_at_link: 1,
            session_link: Some(
                intention_application::goal_domain::GoalSessionLinkCreationRequestDto {
                    link_id: identity_text(0x1e),
                    session_id: identity_text(0x1c),
                    effective_from_revision: 1,
                },
            ),
            occurred_at_ms: 2_000,
        })
        .expect_err("an already-linked owner session never accepts a second link");
    assert_eq!(redundant.code(), "goal_revision_conflict");
    assert_eq!(store.session_links.borrow().len(), 2);

    let unlinked = service
        .attach_goal_child(&AttachGoalChildRequestDto {
            parent_goal_id: identity_text(GOAL),
            child_goal_id: identity_text(0x0d),
            child_revision_at_link: 1,
            session_link: None,
            occurred_at_ms: 2_000,
        })
        .expect_err("the child Goal is unknown");
    assert_eq!(unlinked.code(), "goal_not_active");
}

#[test]
fn attach_child_rejects_self_links_foreign_projects_and_session_parents() {
    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    store.push_goal(GoalRecordDto {
        goal_id: identity_text(0x0e),
        scope: GoalScopeDto::Project {
            project_id: identity_text(0x09),
        },
        active_revision: 1,
        lifecycle_state: GoalLifecycleStateDto::Active,
        readiness_state: GoalReadinessStateDto::NotReady,
        user_decision_state: GoalUserDecisionStateDto::Unaccepted,
        created_at_ms: 1_000,
        updated_at_ms: 1_000,
    });
    store.push_revision(revision_record(0x0e, 1, Vec::new()));
    let service = GoalRuntimeService::new(&store);

    let cross_project = service
        .attach_goal_child(&AttachGoalChildRequestDto {
            parent_goal_id: identity_text(GOAL),
            child_goal_id: identity_text(0x0e),
            child_revision_at_link: 1,
            session_link: None,
            occurred_at_ms: 2_000,
        })
        .expect_err("a Goal child never crosses a project");
    assert_eq!(cross_project.code(), "goal_cycle_detected");
    assert!(store.children.borrow().is_empty());

    let self_link = service
        .attach_goal_child(&AttachGoalChildRequestDto {
            parent_goal_id: identity_text(GOAL),
            child_goal_id: identity_text(GOAL),
            child_revision_at_link: 1,
            session_link: None,
            occurred_at_ms: 2_000,
        })
        .expect_err("a Goal cannot be its own child");
    assert_eq!(self_link.code(), "goal_cycle_detected");

    let store = FakeGoalStore::new();
    store.push_goal(session_goal());
    store.push_revision(revision_record(GOAL, 1, Vec::new()));
    store.push_goal(GoalRecordDto {
        goal_id: identity_text(CHILD),
        scope: project_scope(),
        active_revision: 1,
        lifecycle_state: GoalLifecycleStateDto::Active,
        readiness_state: GoalReadinessStateDto::NotReady,
        user_decision_state: GoalUserDecisionStateDto::Unaccepted,
        created_at_ms: 1_000,
        updated_at_ms: 1_000,
    });
    store.push_revision(revision_record(CHILD, 1, Vec::new()));
    let service = GoalRuntimeService::new(&store);
    let session_parent = service
        .attach_goal_child(&AttachGoalChildRequestDto {
            parent_goal_id: identity_text(GOAL),
            child_goal_id: identity_text(CHILD),
            child_revision_at_link: 1,
            session_link: None,
            occurred_at_ms: 2_000,
        })
        .expect_err("a session Goal cannot own a project child");
    assert_eq!(session_parent.code(), "goal_cycle_detected");
}

#[test]
fn transition_goal_lifecycle_archives_only_terminal_goals() {
    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    let service = GoalRuntimeService::new(&store);

    let archive = service
        .transition_goal_lifecycle(&TransitionGoalLifecycleRequestDto {
            goal_id: identity_text(GOAL),
            expected_revision: 1,
            lifecycle_state: GoalLifecycleStateDto::Archived,
            occurred_at_ms: 2_000,
        })
        .expect_err("an Active Goal cannot be archived");
    assert_eq!(archive.code(), "goal_archive_not_terminal");

    let paused = service
        .transition_goal_lifecycle(&TransitionGoalLifecycleRequestDto {
            goal_id: identity_text(GOAL),
            expected_revision: 1,
            lifecycle_state: GoalLifecycleStateDto::Paused,
            occurred_at_ms: 2_000,
        })
        .expect("an Active Goal can pause");
    assert_eq!(paused.lifecycle_state, GoalLifecycleStateDto::Paused);

    let stopped = service
        .transition_goal_lifecycle(&TransitionGoalLifecycleRequestDto {
            goal_id: identity_text(GOAL),
            expected_revision: 1,
            lifecycle_state: GoalLifecycleStateDto::Stopped,
            occurred_at_ms: 2_000,
        })
        .expect("a paused Goal can stop");
    assert_eq!(stopped.lifecycle_state, GoalLifecycleStateDto::Stopped);

    let archived = service
        .transition_goal_lifecycle(&TransitionGoalLifecycleRequestDto {
            goal_id: identity_text(GOAL),
            expected_revision: 1,
            lifecycle_state: GoalLifecycleStateDto::Archived,
            occurred_at_ms: 2_000,
        })
        .expect("a stopped Goal can archive");
    assert_eq!(archived.lifecycle_state, GoalLifecycleStateDto::Archived);

    let restored = service
        .transition_goal_lifecycle(&TransitionGoalLifecycleRequestDto {
            goal_id: identity_text(GOAL),
            expected_revision: 1,
            lifecycle_state: GoalLifecycleStateDto::Active,
            occurred_at_ms: 2_000,
        })
        .expect("an archived Goal can be restored");
    assert_eq!(restored.lifecycle_state, GoalLifecycleStateDto::Active);
}

#[test]
fn claim_goal_readiness_requires_evidence_and_terminal_children() {
    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    let service = GoalRuntimeService::new(&store);

    let empty = service
        .claim_goal_readiness(&ClaimGoalReadinessRequestDto {
            goal_id: identity_text(GOAL),
            expected_revision: 1,
            verified_evidence_set: Vec::new(),
            occurred_at_ms: 2_000,
        })
        .expect_err("Ready requires selected successful evidence");
    assert_eq!(empty.code(), "goal_not_ready");

    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    seed_session_child(&store, CHILD);
    store.children.borrow_mut().push(GoalParentLinkRecordDto {
        parent_goal_id: identity_text(GOAL),
        child_goal_id: identity_text(CHILD),
        child_revision_at_link: 1,
        canonical_link_digest: digest_text(0x17),
        created_at_ms: 1_000,
    });
    let service = GoalRuntimeService::new(&store);
    let unresolved = service
        .claim_goal_readiness(&ClaimGoalReadinessRequestDto {
            goal_id: identity_text(GOAL),
            expected_revision: 1,
            verified_evidence_set: vec![evidence(GATE, GoalEvidenceKindDto::TerminalChildResult)],
            occurred_at_ms: 2_000,
        })
        .expect_err("an unresolved obligatory child blocks Ready");
    assert_eq!(unresolved.code(), "goal_not_ready");

    store.set_goal_decision(&identity_text(CHILD), GoalUserDecisionStateDto::Accepted);
    let ready = service
        .claim_goal_readiness(&ClaimGoalReadinessRequestDto {
            goal_id: identity_text(GOAL),
            expected_revision: 1,
            verified_evidence_set: vec![evidence(GATE, GoalEvidenceKindDto::TerminalChildResult)],
            occurred_at_ms: 2_000,
        })
        .expect("a terminal child admits Ready");
    assert_eq!(
        ready.readiness_state,
        GoalReadinessStateDto::Ready {
            verified_evidence_set: vec![evidence(GATE, GoalEvidenceKindDto::TerminalChildResult)],
        }
    );
    assert_eq!(
        store.goal_readiness(&identity_text(GOAL)),
        ready.readiness_state
    );
}

#[test]
fn record_user_decision_inherits_every_child_exception() {
    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    seed_session_child(&store, CHILD);
    store.children.borrow_mut().push(GoalParentLinkRecordDto {
        parent_goal_id: identity_text(GOAL),
        child_goal_id: identity_text(CHILD),
        child_revision_at_link: 1,
        canonical_link_digest: digest_text(0x17),
        created_at_ms: 1_000,
    });
    let exception = GoalGateExceptionDto {
        gate_id: identity_text(GATE),
        gate_revision: 1,
        kind: GoalGateExceptionKindDto::Unavailable,
        evidence: evidence(GATE, GoalEvidenceKindDto::AcceptedUserDeclaration),
    };
    store.set_goal_decision(&identity_text(CHILD), GoalUserDecisionStateDto::Accepted);
    let service = GoalRuntimeService::new(&store);

    let plain = service
        .record_goal_user_decision(&RecordGoalUserDecisionRequestDto {
            goal_id: identity_text(GOAL),
            expected_revision: 1,
            decision: GoalUserDecisionRequestV1::Accept,
            occurred_at_ms: 2_000,
        })
        .expect_err("plain acceptance requires a Ready Goal");
    assert_eq!(plain.code(), "goal_not_ready");

    store.set_goal_decision(
        &identity_text(CHILD),
        GoalUserDecisionStateDto::AcceptedWithException {
            exception_evidence_set: vec![exception.clone()],
        },
    );
    let omitted = service
        .record_goal_user_decision(&RecordGoalUserDecisionRequestDto {
            goal_id: identity_text(GOAL),
            expected_revision: 1,
            decision: GoalUserDecisionRequestV1::AcceptWithException {
                exception_evidence_set: Vec::new(),
            },
            occurred_at_ms: 2_000,
        })
        .expect_err("an inherited child exception cannot be omitted");
    assert_eq!(omitted.code(), "goal_acceptance_exception_invalid");

    let accepted = service
        .record_goal_user_decision(&RecordGoalUserDecisionRequestDto {
            goal_id: identity_text(GOAL),
            expected_revision: 1,
            decision: GoalUserDecisionRequestV1::AcceptWithException {
                exception_evidence_set: vec![exception.clone()],
            },
            occurred_at_ms: 2_000,
        })
        .expect("the inherited exception set is accepted");
    assert_eq!(
        accepted.user_decision_state,
        GoalUserDecisionStateDto::AcceptedWithException {
            exception_evidence_set: vec![exception.clone()],
        }
    );
    assert_eq!(
        store.goal_decision(&identity_text(GOAL)),
        GoalUserDecisionStateDto::AcceptedWithException {
            exception_evidence_set: vec![exception],
        }
    );
}

#[test]
fn admit_leading_goal_run_commits_exactly_once_with_its_selection() {
    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    let service = GoalRuntimeService::new(&store);
    let port = FakeAdmissionPort::new(store.log_handle());

    let admitted = service
        .admit_leading_goal_run(&admission_request(project_selection(1), 4_096), &port)
        .expect("the leading project Goal admits the run");
    assert_eq!(admitted.leading_goal_id, identity_text(GOAL));
    assert_eq!(admitted.goal_revision, 1);
    assert_eq!(admitted.run_kind, GoalRunKindV1::GoalDirectedOrdinary);
    assert!(!admitted.replayed);
    let commits = port.commits();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].target_snapshot_digest, digest(0x42));

    let failure_port = FakeAdmissionPort::new(store.log_handle());
    failure_port.deny("goal_snapshot_unavailable");
    let denied = service
        .admit_leading_goal_run(
            &admission_request(project_selection(1), 4_096),
            &failure_port,
        )
        .expect_err("the admission port denial propagates");
    assert_eq!(denied.code(), "goal_snapshot_unavailable");
    assert_eq!(failure_port.commits().len(), 1);
}

#[test]
fn admit_leading_goal_run_replays_equal_admissions() {
    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    let service = GoalRuntimeService::new(&store);
    let port = FakeAdmissionPort::new(store.log_handle());
    port.set_replay();
    let admitted = service
        .admit_leading_goal_run(&admission_request(project_selection(1), 4_096), &port)
        .expect("the equal admission replays its binding");
    assert!(admitted.replayed);
}

#[test]
fn admit_leading_goal_run_fails_closed_before_any_commit() {
    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    let service = GoalRuntimeService::new(&store);
    let port = FakeAdmissionPort::new(store.log_handle());

    let stale = service
        .admit_leading_goal_run(&admission_request(project_selection(2), 4_096), &port)
        .expect_err("a stale frozen Goal revision is rejected");
    assert_eq!(stale.code(), "goal_revision_conflict");

    let oversized = service
        .admit_leading_goal_run(&admission_request(project_selection(1), 65_537), &port)
        .expect_err("an over-limit target snapshot is rejected");
    assert_eq!(oversized.code(), "goal_snapshot_too_large");

    let missing_link = GoalScopeLinkProvenanceV1 {
        project_id: identity(PROJECT),
        session_id: Some(identity(SESSION)),
        link_id: Some(identity(0x0f)),
    };
    let unlinked = service
        .admit_leading_goal_run(
            &admission_request(selection(identity(GOAL), 1, missing_link), 4_096),
            &port,
        )
        .expect_err("an unavailable explicit session link is rejected");
    assert_eq!(unlinked.code(), "goal_not_active");

    let empty_snapshot = service
        .admit_leading_goal_run(&admission_request(project_selection(1), 0), &port)
        .expect_err("an unavailable target snapshot is rejected");
    assert_eq!(empty_snapshot.code(), "goal_snapshot_unavailable");

    store.set_goal_lifecycle(&identity_text(GOAL), GoalLifecycleStateDto::NeedsRework);
    let inactive = service
        .admit_leading_goal_run(&admission_request(project_selection(1), 4_096), &port)
        .expect_err("only an Active Goal admits a run");
    assert_eq!(inactive.code(), "goal_not_active");
    assert!(port.commits().is_empty());
}

#[test]
fn admit_leading_goal_run_validates_selected_gates_templates_and_cards() {
    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    store.push_gate(reference_gate(vec![
        GoalEvidenceKindDto::AcceptedUserDeclaration,
    ]));
    let service = GoalRuntimeService::new(&store);
    let port = FakeAdmissionPort::new(store.log_handle());

    let mut stale_gate = project_selection(1);
    stale_gate.selected_gate_revisions = vec![GoalGateRevisionReferenceV1 {
        gate_reference: identity(GATE),
        revision: 2,
    }];
    let error = service
        .admit_leading_goal_run(&admission_request(stale_gate, 4_096), &port)
        .expect_err("a stale selected gate revision is rejected");
    assert_eq!(error.code(), "goal_snapshot_unavailable");

    let mut missing_template = project_selection(1);
    missing_template.selected_gate_revisions = vec![GoalGateRevisionReferenceV1 {
        gate_reference: identity(EXECUTABLE_GATE),
        revision: 1,
    }];
    store.push_gate(executable_gate());
    let error = service
        .admit_leading_goal_run(&admission_request(missing_template, 4_096), &port)
        .expect_err("a missing selected template revision is rejected");
    assert_eq!(error.code(), "goal_snapshot_unavailable");

    let mut foreign_card = project_selection(1);
    foreign_card.selected_memory_cards = vec![GoalCardReferenceV1 {
        card_reference: identity(0x28),
        revision: 1,
    }];
    store
        .memory_cards
        .borrow_mut()
        .push(GoalMemoryCardRecordDto {
            record_id: identity_text(0x28),
            scope: GoalRecordScopeDto::Project {
                project_id: identity_text(0x09),
            },
            ..memory_card_record(1)
        });
    let error = service
        .admit_leading_goal_run(&admission_request(foreign_card, 4_096), &port)
        .expect_err("a foreign memory card is not applicable to this run");
    assert_eq!(error.code(), "memory_reference_unavailable");

    let mut selected_card = project_selection(1);
    selected_card.selected_memory_cards = vec![GoalCardReferenceV1 {
        card_reference: identity(0x20),
        revision: 1,
    }];
    store.memory_cards.borrow_mut().push(memory_card_record(1));
    let admitted = service
        .admit_leading_goal_run(&admission_request(selected_card, 4_096), &port)
        .expect("an applicable memory card admits the run");
    assert_eq!(admitted.goal_revision, 1);
}

#[test]
fn gate_ordering_settles_reference_evidence_before_executable_gates() {
    let store = FakeGoalStore::new();
    store.push_goal(project_goal());
    store.push_revision(revision_record(
        GOAL,
        1,
        vec![gate_reference(GATE, 1), gate_reference(EXECUTABLE_GATE, 1)],
    ));
    store.push_gate(reference_gate(vec![
        GoalEvidenceKindDto::TerminalRegisteredToolResult,
    ]));
    store.push_gate(executable_gate());
    store.push_template(template_record());
    let service = GoalRuntimeService::new(&store);

    let early = service
        .evaluate_goal_gate(&EvaluateGoalGateRequestDto {
            goal_id: identity_text(GOAL),
            gate_id: identity_text(EXECUTABLE_GATE),
            producing_run_id: identity_text(0x18),
            outcome_kind: GoalGateOutcomeKindDto::Passed,
            evidence: Some(evidence(GATE, GoalEvidenceKindDto::ExecutableGateResult)),
            occurred_at_ms: 2_000,
        })
        .expect_err("reference evidence must settle before an executable gate");
    assert_eq!(early.code(), "goal_not_ready");
    assert!(store.gate_results.borrow().is_empty());

    let settled = service
        .evaluate_goal_gate(&EvaluateGoalGateRequestDto {
            goal_id: identity_text(GOAL),
            gate_id: identity_text(GATE),
            producing_run_id: identity_text(0x18),
            outcome_kind: GoalGateOutcomeKindDto::Passed,
            evidence: Some(evidence(
                GATE,
                GoalEvidenceKindDto::TerminalRegisteredToolResult,
            )),
            occurred_at_ms: 2_000,
        })
        .expect("the reference gate settles its exact evidence");
    assert_eq!(
        settled.result.disposition,
        intention_storage::goal_repo::GoalGateOutcomeDispositionDto::Passed
    );

    store.templates.borrow_mut().clear();
    let missing_template = service
        .evaluate_goal_gate(&EvaluateGoalGateRequestDto {
            goal_id: identity_text(GOAL),
            gate_id: identity_text(EXECUTABLE_GATE),
            producing_run_id: identity_text(0x18),
            outcome_kind: GoalGateOutcomeKindDto::Passed,
            evidence: Some(evidence(GATE, GoalEvidenceKindDto::ExecutableGateResult)),
            occurred_at_ms: 2_000,
        })
        .expect_err("a missing executable template fails closed");
    assert_eq!(missing_template.code(), "goal_gate_unavailable");
}

#[test]
fn required_gate_failure_moves_the_goal_to_needs_rework() {
    let store = FakeGoalStore::new();
    store.push_goal(project_goal());
    store.push_revision(revision_record(GOAL, 1, vec![gate_reference(GATE, 1)]));
    store.push_gate(reference_gate(vec![
        GoalEvidenceKindDto::TerminalRegisteredToolResult,
    ]));
    let service = GoalRuntimeService::new(&store);

    let evaluation = service
        .evaluate_goal_gate(&EvaluateGoalGateRequestDto {
            goal_id: identity_text(GOAL),
            gate_id: identity_text(GATE),
            producing_run_id: identity_text(0x18),
            outcome_kind: GoalGateOutcomeKindDto::Failed,
            evidence: Some(evidence(
                GATE,
                GoalEvidenceKindDto::TerminalRegisteredToolResult,
            )),
            occurred_at_ms: 2_000,
        })
        .expect("the required gate failure is recorded");
    assert!(evaluation.goal_moved_to_needs_rework);
    assert_eq!(
        evaluation.goal.lifecycle_state,
        GoalLifecycleStateDto::NeedsRework
    );
    assert_eq!(
        store.goal_lifecycle(&identity_text(GOAL)),
        GoalLifecycleStateDto::NeedsRework
    );
    assert_eq!(store.gate_results.borrow().len(), 1);
}

#[test]
fn gate_outcomes_require_exact_evidence_of_the_accepted_kind() {
    let store = FakeGoalStore::new();
    store.push_goal(project_goal());
    store.push_revision(revision_record(GOAL, 1, Vec::new()));
    store.push_gate(reference_gate(vec![
        GoalEvidenceKindDto::TerminalChildResult,
    ]));
    let service = GoalRuntimeService::new(&store);

    let missing = service
        .evaluate_goal_gate(&EvaluateGoalGateRequestDto {
            goal_id: identity_text(GOAL),
            gate_id: identity_text(GATE),
            producing_run_id: identity_text(0x18),
            outcome_kind: GoalGateOutcomeKindDto::Passed,
            evidence: None,
            occurred_at_ms: 2_000,
        })
        .expect_err("a passing outcome requires exact evidence");
    assert_eq!(missing.code(), "goal_gate_failed");

    let wrong_kind = service
        .evaluate_goal_gate(&EvaluateGoalGateRequestDto {
            goal_id: identity_text(GOAL),
            gate_id: identity_text(GATE),
            producing_run_id: identity_text(0x18),
            outcome_kind: GoalGateOutcomeKindDto::Passed,
            evidence: Some(evidence(
                GATE,
                GoalEvidenceKindDto::TerminalRegisteredToolResult,
            )),
            occurred_at_ms: 2_000,
        })
        .expect_err("the evidence kind must be accepted by the gate");
    assert_eq!(wrong_kind.code(), "goal_gate_failed");
    assert!(store.gate_results.borrow().is_empty());
}

#[test]
fn gate_templates_require_user_provenance_and_closed_lifecycle_edges() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);

    let unconfirmed = service
        .create_goal_gate_template(&CreateGoalGateTemplateRequestDto {
            provenance: GoalTemplateProvenanceV1::ModelProposal {
                draft_id: identity(0x19),
                accepted_by_user: false,
            },
            ..template_request()
        })
        .expect_err("an unconfirmed model proposal never enables a template");
    assert_eq!(unconfirmed.code(), "goal_gate_unavailable");

    let created = service
        .create_goal_gate_template(&CreateGoalGateTemplateRequestDto {
            provenance: GoalTemplateProvenanceV1::ModelProposal {
                draft_id: identity(0x19),
                accepted_by_user: true,
            },
            ..template_request()
        })
        .expect("a user-confirmed model proposal enables a template");
    assert_eq!(
        created.lifecycle_state,
        GoalTemplateLifecycleStateDto::Enabled
    );

    let repeated = service
        .transition_goal_gate_template(&TransitionGoalGateTemplateRequestDto {
            template_id: identity_text(TEMPLATE),
            revision: 1,
            lifecycle_state: GoalTemplateLifecycleStateDto::Enabled,
            occurred_at_ms: 2_000,
        })
        .expect_err("an undeclared template lifecycle edge is rejected");
    assert_eq!(repeated.code(), "goal_gate_unavailable");

    let archived = service
        .transition_goal_gate_template(&TransitionGoalGateTemplateRequestDto {
            template_id: identity_text(TEMPLATE),
            revision: 1,
            lifecycle_state: GoalTemplateLifecycleStateDto::Archived,
            occurred_at_ms: 2_000,
        })
        .expect("an enabled template archives");
    assert_eq!(
        archived.lifecycle_state,
        GoalTemplateLifecycleStateDto::Archived
    );

    let path = service
        .create_goal_gate_template(&CreateGoalGateTemplateRequestDto {
            capability_reference: "/usr/bin/run-gate".to_owned(),
            ..template_request()
        })
        .expect_err("a filesystem path never crosses the boundary");
    assert_eq!(path.code(), "goal_gate_unavailable");

    let credential = service
        .create_goal_gate_template(&CreateGoalGateTemplateRequestDto {
            capability_reference: "api_key=secret-value".to_owned(),
            ..template_request()
        })
        .expect_err("credential-shaped text is rejected");
    assert_eq!(credential.code(), "credentials_forbidden");
}

#[test]
fn memory_cards_disclose_replace_and_roll_back_against_exact_references() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);

    let stored = service
        .store_goal_memory_card(&StoreGoalMemoryCardRequestDto {
            record_id: identity_text(0x20),
            revision: 1,
            kind: MemoryKindDto::Decision,
            scope: record_project_scope(),
            title: "Chosen admission order".to_owned(),
            safe_purpose: "Keep gate ordering stable".to_owned(),
            retained_content_reference: identity_text(0x21),
            occurred_at_ms: 2_000,
        })
        .expect("the bounded memory card is stored");
    assert!(!stored.canonical_digest.is_empty());

    let wrong = service
        .reveal_goal_memory_reference(&RevealGoalMemoryReferenceRequestDto {
            record_id: identity_text(0x20),
            revision: 1,
            retained_content_reference: identity_text(0x26),
        })
        .expect_err("full content is revealed only against its exact reference");
    assert_eq!(wrong.code(), "memory_reference_unavailable");

    let revealed = service
        .reveal_goal_memory_reference(&RevealGoalMemoryReferenceRequestDto {
            record_id: identity_text(0x20),
            revision: 1,
            retained_content_reference: identity_text(0x21),
        })
        .expect("the exact retained-content reference is revealed");
    assert_eq!(revealed.retained_content_reference, identity_text(0x21));

    let advanced = service
        .replace_goal_memory_card(&ReplaceGoalMemoryCardRequestDto {
            replaced_record_id: identity_text(0x20),
            replaced_revision: 1,
            replacement: GoalMemoryCardRecordDto {
                revision: 2,
                title: "Revised admission order".to_owned(),
                ..memory_card_record(1)
            },
            occurred_at_ms: 2_000,
        })
        .expect("a same-record replacement advances the revision");
    assert_eq!(advanced.revision, 2);

    let not_advancing = service
        .replace_goal_memory_card(&ReplaceGoalMemoryCardRequestDto {
            replaced_record_id: identity_text(0x20),
            replaced_revision: 2,
            replacement: memory_card_record(2),
            occurred_at_ms: 2_000,
        })
        .expect_err("a same-record replacement must advance");
    assert_eq!(not_advancing.code(), "memory_replacement_conflict");

    let restored = service
        .rollback_goal_memory_card(&RollbackGoalMemoryCardRequestDto {
            restored_record_id: identity_text(0x20),
            restored_revision: 1,
            replacement: GoalMemoryCardRecordDto {
                revision: 3,
                title: "Restored admission order".to_owned(),
                ..memory_card_record(1)
            },
            occurred_at_ms: 2_000,
        })
        .expect("a rollback creates the next revision of the restored record");
    assert_eq!(restored.revision, 3);

    let invalid_rollback = service
        .rollback_goal_memory_card(&RollbackGoalMemoryCardRequestDto {
            restored_record_id: identity_text(0x20),
            restored_revision: 3,
            replacement: memory_card_record(3),
            occurred_at_ms: 2_000,
        })
        .expect_err("a rollback cannot restore the current revision");
    assert_eq!(invalid_rollback.code(), "memory_replacement_conflict");
}

#[test]
fn skill_cards_disclose_exactly_and_roles_only_narrow() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);

    service
        .store_goal_skill_card(&StoreGoalSkillCardRequestDto {
            skill_id: identity_text(0x23),
            revision: 1,
            canonical_name: "goal-runtime-audit".to_owned(),
            description: "Audit the goal runtime deterministically".to_owned(),
            owner_scope: GoalRecordScopeDto::Goal {
                goal_id: identity_text(GOAL),
            },
            content_reference: identity_text(0x24),
            occurred_at_ms: 2_000,
        })
        .expect("the bounded Skill card is stored");

    let wrong = service
        .reveal_goal_skill_reference(&identity_text(0x23), 1, &identity_text(0x27))
        .expect_err("a Skill body is disclosed only against its exact reference");
    assert_eq!(wrong.code(), "skill_reference_unavailable");
    let revealed = service
        .reveal_goal_skill_reference(&identity_text(0x23), 1, &identity_text(0x24))
        .expect("the exact Skill reference is disclosed");
    assert_eq!(revealed.retained_content_reference, identity_text(0x24));

    let base = role_card_record(0x28, GoalRoleClassDto::Light, vec!["read"], 1_024);
    service
        .store_goal_role_card(&StoreGoalRoleCardRequestDto {
            canonical_name: base.canonical_name.clone(),
            task: base.task.clone(),
            permitted_class: base.permitted_class,
            tool_subset: base.tool_subset.clone(),
            context_limit_bytes: base.context_limit_bytes,
            result_limit_bytes: base.result_limit_bytes,
            role_id: base.role_id,
            revision: 1,
            occurred_at_ms: 2_000,
        })
        .expect("the bounded role card is stored");

    let widened = service
        .narrow_goal_role(&NarrowGoalRoleRequestDto {
            base_role_id: identity_text(0x28),
            base_revision: 1,
            concrete: role_card_record(0x29, GoalRoleClassDto::Light, vec!["read", "write"], 1_024),
        })
        .expect_err("a concrete role never widens its tool subset");
    assert_eq!(widened.code(), "delegation_role_widening_forbidden");

    let narrowed = service
        .narrow_goal_role(&NarrowGoalRoleRequestDto {
            base_role_id: identity_text(0x28),
            base_revision: 1,
            concrete: role_card_record(0x2a, GoalRoleClassDto::Light, vec!["read"], 512),
        })
        .expect("a narrowed concrete role is admitted");
    assert_eq!(narrowed.role_id, identity_text(0x2a));
}

#[test]
fn refinement_proposals_coalesce_once_and_conflict_when_unequal() {
    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    let service = GoalRuntimeService::new(&store);

    let first = service
        .propose_refinement_draft(&propose_request(
            0x30,
            1,
            "Claim readiness against the frozen evidence",
        ))
        .expect("the first proposal owns the pending slot");
    assert!(!first.coalesced);
    assert!(first.user_decision_required);
    assert_eq!(first.draft.state, RefinementDraftStateDto::Pending);

    let equal = service
        .propose_refinement_draft(&propose_request(
            0x30,
            1,
            "Claim readiness against the frozen evidence",
        ))
        .expect("an equal proposal coalesces");
    assert!(equal.coalesced);
    assert_eq!(store.drafts.borrow().len(), 1);

    let unequal = service
        .propose_refinement_draft(&propose_request(
            0x30,
            1,
            "Claim a different readiness state",
        ))
        .expect_err("an unequal proposal conflicts until the pending draft is decided");
    assert_eq!(unequal.code(), "refinement_draft_conflict");
    assert_eq!(store.drafts.borrow().len(), 1);
}

#[test]
fn refinement_decisions_validate_the_base_and_rejection_changes_nothing() {
    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    store.drafts.borrow_mut().push(draft_record(0x30, 2));
    let service = GoalRuntimeService::new(&store);

    let stale = service
        .decide_refinement_draft(&DecideRefinementDraftRequestDto {
            leading_goal_id: identity_text(GOAL),
            draft_id: identity_text(0x30),
            decision: RefinementDecisionV1::Accept,
            decided_at_ms: 2_000,
        })
        .expect_err("a stale draft base revision is a typed conflict");
    assert_eq!(stale.code(), "refinement_draft_conflict");

    let rejected = service
        .decide_refinement_draft(&DecideRefinementDraftRequestDto {
            leading_goal_id: identity_text(GOAL),
            draft_id: identity_text(0x30),
            decision: RefinementDecisionV1::Reject,
            decided_at_ms: 2_000,
        })
        .expect("rejection resolves the pending draft");
    assert_eq!(rejected.resolution, GoalRefinementResolutionV1::Rejected);
    assert_eq!(rejected.draft.state, RefinementDraftStateDto::Rejected);
    assert!(
        service
            .decide_refinement_draft(&DecideRefinementDraftRequestDto {
                leading_goal_id: identity_text(GOAL),
                draft_id: identity_text(0x30),
                decision: RefinementDecisionV1::Reject,
                decided_at_ms: 2_000,
            })
            .is_err()
    );
}

#[test]
fn conversation_summaries_extend_the_working_form_and_correct_immutably() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);

    let outside = service
        .store_conversation_summary(&StoreConversationSummaryRequestDto {
            summary_id: identity_text(0x20),
            revision: 1,
            scope: record_project_scope(),
            previous_summary_reference: None,
            source_range_start: identity_text(0x21),
            source_range_end: identity_text(0x22),
            safe_content: "Compacted the first range".to_owned(),
            origin: GoalCompactionOriginV1::BeforeAdmission,
            occurred_at_ms: 2_000,
        })
        .expect_err("compaction runs only inside an active admitted run");
    assert_eq!(outside.code(), "compaction_history_unavailable");

    let empty = service
        .store_conversation_summary(&StoreConversationSummaryRequestDto {
            summary_id: identity_text(0x20),
            revision: 1,
            scope: record_project_scope(),
            previous_summary_reference: None,
            source_range_start: identity_text(0x21),
            source_range_end: identity_text(0x22),
            safe_content: "Compacted the first range".to_owned(),
            origin: GoalCompactionOriginV1::ActiveRun,
            occurred_at_ms: 2_000,
        })
        .expect_err("nothing completed means nothing to compact");
    assert_eq!(empty.code(), "compaction_history_unavailable");

    service
        .record_compaction_suffix_reference(&RecordCompactionSuffixRequestDto {
            scope: record_project_scope(),
            history_reference: identity_text(0x21),
            occurred_at_ms: 1_500,
        })
        .expect("the first completed reference is recorded");
    service
        .record_compaction_suffix_reference(&RecordCompactionSuffixRequestDto {
            scope: record_project_scope(),
            history_reference: identity_text(0x22),
            occurred_at_ms: 1_500,
        })
        .expect("the second completed reference is recorded");

    let stored = service
        .store_conversation_summary(&StoreConversationSummaryRequestDto {
            summary_id: identity_text(0x20),
            revision: 1,
            scope: record_project_scope(),
            previous_summary_reference: None,
            source_range_start: identity_text(0x21),
            source_range_end: identity_text(0x22),
            safe_content: "Compacted the first range".to_owned(),
            origin: GoalCompactionOriginV1::ActiveRun,
            occurred_at_ms: 2_000,
        })
        .expect("the first summary starts the chain at revision one");
    assert_eq!(stored.revision, 1);
    assert!(stored.previous_summary_reference.is_none());

    let wrong_revision = service
        .correct_conversation_summary(&CorrectConversationSummaryRequestDto {
            summary_id: identity_text(0x20),
            corrected_revision: 1,
            next_revision: 3,
            safe_content: "Corrected content".to_owned(),
            occurred_at_ms: 2_000,
        })
        .expect_err("a correction continues the exact corrected revision");
    assert_eq!(wrong_revision.code(), "compaction_summary_unavailable");

    let corrected = service
        .correct_conversation_summary(&CorrectConversationSummaryRequestDto {
            summary_id: identity_text(0x20),
            corrected_revision: 1,
            next_revision: 2,
            safe_content: "Corrected the first range".to_owned(),
            occurred_at_ms: 2_000,
        })
        .expect("the correction creates a separately immutable revision");
    assert_eq!(corrected.revision, 2);
    assert_eq!(corrected.source_range_start, identity_text(0x21));
}

#[test]
fn fork_summary_references_name_the_exact_compatible_ancestor() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);
    service
        .record_compaction_suffix_reference(&RecordCompactionSuffixRequestDto {
            scope: record_project_scope(),
            history_reference: identity_text(0x21),
            occurred_at_ms: 1_500,
        })
        .expect("the completed reference is recorded");
    service
        .record_compaction_suffix_reference(&RecordCompactionSuffixRequestDto {
            scope: record_project_scope(),
            history_reference: identity_text(0x22),
            occurred_at_ms: 1_500,
        })
        .expect("the completed reference is recorded");
    service
        .store_conversation_summary(&StoreConversationSummaryRequestDto {
            summary_id: identity_text(0x20),
            revision: 1,
            scope: record_project_scope(),
            previous_summary_reference: None,
            source_range_start: identity_text(0x21),
            source_range_end: identity_text(0x22),
            safe_content: "Compacted the first range".to_owned(),
            origin: GoalCompactionOriginV1::ActiveRun,
            occurred_at_ms: 2_000,
        })
        .expect("the ancestor summary exists");

    let inherited = service
        .validate_fork_summary_reference(&ForkSummaryReferenceRequestDto {
            ancestor_summary_id: identity_text(0x20),
            ancestor_revision: 1,
            inherited_summary_id: identity_text(0x20),
            inherited_revision: 1,
        })
        .expect("the exact ancestor summary is inherited");
    assert_eq!(inherited.revision, 1);

    let mismatched = service
        .validate_fork_summary_reference(&ForkSummaryReferenceRequestDto {
            ancestor_summary_id: identity_text(0x20),
            ancestor_revision: 1,
            inherited_summary_id: identity_text(0x23),
            inherited_revision: 1,
        })
        .expect_err("the inherited summary identity is absent");
    assert_eq!(mismatched.code(), "compaction_summary_unavailable");
}

#[test]
fn verifier_authority_lifecycle_is_closed_and_history_preserving() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);
    let issued = service
        .record_verifier_authority(&authority_request(
            VerifierAuthorityConsumptionRuleV1::ReusableWhileActive,
            None,
        ))
        .expect("the delegated authority revision is issued");
    assert_eq!(issued.verifier_mandate_id, identity(VERIFIER));
    assert_eq!(
        issued.allowed_operations,
        vec![
            VerifierOperationV1::MarkNeedsRework,
            VerifierOperationV1::MarkComplete
        ]
    );

    let loaded = service
        .load_verifier_authority(&identity_text(AUTHORITY), 1)
        .expect("the exact authority revision is loaded");
    assert_eq!(
        loaded.canonical_authority_digest,
        issued.canonical_authority_digest
    );

    let revoked = service
        .revoke_verifier_authority(&RevokeVerifierAuthorityRequestDto {
            authority_id: identity_text(AUTHORITY),
            authority_revision: 1,
            revocation_reference: identity_text(0x57),
            revoked_at_ms: 2_000,
        })
        .expect("the authority revision is revoked");
    assert_eq!(revoked.authority_revision, 1);

    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);
    let expired = service
        .record_verifier_authority(&authority_request(
            VerifierAuthorityConsumptionRuleV1::ReusableWhileActive,
            Some(1_500),
        ))
        .expect("the expiring authority revision is issued");
    let reference = authority_reference(&expired);
    let evidence_id = service
        .record_verifier_audit_evidence(&evidence_request(
            0x70,
            reference.clone(),
            VerifierEvidenceKindV1::UnconditionalPass,
        ))
        .expect("the audit evidence is recorded");
    assert_eq!(
        evidence_id.evidence_kind,
        VerifierEvidenceKindV1::UnconditionalPass
    );
    let denied = service
        .apply_verifier_target_mutation(&mutation_request(
            0x61,
            0x62,
            reference,
            digest(0x63),
            VerifierOperationV1::MarkComplete,
            vec![identity_text(0x70)],
            committed_state(&expired),
        ))
        .expect_err("the authority is expired at the application time");
    assert_eq!(denied.code(), "verifier_authority_expired");

    let credential = service
        .revoke_verifier_authority(&RevokeVerifierAuthorityRequestDto {
            authority_id: identity_text(AUTHORITY),
            authority_revision: 1,
            revocation_reference: "api_key=secret-value".to_owned(),
            revoked_at_ms: 2_000,
        })
        .expect_err("credential-shaped references are rejected");
    assert_eq!(credential.code(), "credentials_forbidden");
}

#[test]
fn baseline_creation_rereads_the_exact_authority_revision() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);
    let authority = service
        .record_verifier_authority(&authority_request(
            VerifierAuthorityConsumptionRuleV1::ReusableWhileActive,
            None,
        ))
        .expect("the authority is issued");

    let stale_reference = VerifierAuthorityReferenceDto {
        authority_id: identity_text(AUTHORITY),
        authority_revision: 1,
        canonical_authority_digest: digest_text(0x58),
    };
    let mismatch = service
        .record_verifier_audit_baseline(&baseline_request(stale_reference))
        .expect_err("the stale authority digest fails closed");
    assert_eq!(mismatch.code(), "verifier_authority_digest_mismatch");

    let baseline = service
        .record_verifier_audit_baseline(&baseline_request(authority_reference(&authority)))
        .expect("the exact authority revision freezes its baseline");
    assert_eq!(baseline.target_revision, 3);
    assert!(!baseline.canonical_baseline_digest.is_empty());
}

#[test]
fn verifier_mutation_replays_equal_operations_and_consumes_single_use() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);
    let authority = service
        .record_verifier_authority(&authority_request(
            VerifierAuthorityConsumptionRuleV1::SingleUse,
            None,
        ))
        .expect("the single-use authority is issued");
    let reference = authority_reference(&authority);
    service
        .record_verifier_audit_evidence(&evidence_request(
            0x70,
            reference.clone(),
            VerifierEvidenceKindV1::QualifyingFail,
        ))
        .expect("the qualifying-fail evidence is recorded");
    let baseline = service
        .record_verifier_audit_baseline(&baseline_request(reference.clone()))
        .expect("the baseline is frozen");
    let baseline_digest = stored_digest(&baseline.canonical_baseline_digest);

    let applied = service
        .apply_verifier_target_mutation(&mutation_request(
            0x61,
            0x62,
            reference.clone(),
            baseline_digest,
            VerifierOperationV1::MarkNeedsRework,
            vec![identity_text(0x70)],
            committed_state(&authority),
        ))
        .expect("the mutation applies once");
    assert!(!applied.replayed);
    assert_eq!(store.mutations().len(), 1);
    assert_eq!(
        store.authorities()[0].consumption_state,
        VerifierAuthorityConsumptionStateDto::Consumed
    );

    let second = service
        .apply_verifier_target_mutation(&mutation_request(
            0x64,
            0x65,
            reference,
            baseline_digest,
            VerifierOperationV1::MarkNeedsRework,
            vec![identity_text(0x70)],
            committed_state(&authority),
        ))
        .expect_err("a single-use authority is consumed");
    assert_eq!(second.code(), "verifier_authority_consumed");
    assert_eq!(store.mutations().len(), 1);
}

#[test]
fn verifier_mutation_replays_an_equal_committed_operation() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);
    let authority = service
        .record_verifier_authority(&authority_request(
            VerifierAuthorityConsumptionRuleV1::ReusableWhileActive,
            None,
        ))
        .expect("the reusable authority is issued");
    let reference = authority_reference(&authority);
    service
        .record_verifier_audit_evidence(&evidence_request(
            0x70,
            reference.clone(),
            VerifierEvidenceKindV1::UnconditionalPass,
        ))
        .expect("the unconditional-pass evidence is recorded");
    service
        .record_verifier_audit_evidence(&evidence_request(
            0x71,
            reference.clone(),
            VerifierEvidenceKindV1::GraphTerminalizationClosure,
        ))
        .expect("the graph-terminalization closure evidence is recorded");
    let baseline = service
        .record_verifier_audit_baseline(&baseline_request(reference.clone()))
        .expect("the baseline is frozen");
    let baseline_digest = stored_digest(&baseline.canonical_baseline_digest);
    let request = mutation_request(
        0x61,
        0x62,
        reference.clone(),
        baseline_digest,
        VerifierOperationV1::MarkComplete,
        vec![identity_text(0x70), identity_text(0x71)],
        committed_state(&authority),
    );
    let first = service
        .apply_verifier_target_mutation(&request)
        .expect("the operation commits once");
    assert!(!first.replayed);
    let operation = VerifierOperationIdentityV1 {
        operation_id: identity(0x62),
        operation_digest: digest(0x63),
    };
    let replay_request = ApplyVerifierTargetMutationRequestDto {
        observed_committed_state: VerifierCommittedStateV1 {
            committed_operation: Some(operation),
            ..committed_state(&authority)
        },
        ..mutation_request(
            0x61,
            0x62,
            reference,
            baseline_digest,
            VerifierOperationV1::MarkComplete,
            vec![identity_text(0x70), identity_text(0x71)],
            committed_state(&authority),
        )
    };
    let replay = service
        .apply_verifier_target_mutation(&replay_request)
        .expect("an equal committed operation replays its binding");
    assert!(replay.replayed);
    assert_eq!(store.mutations().len(), 1);
}

#[test]
fn verifier_mutation_rejects_a_stale_baseline_before_any_effect() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);
    let authority = service
        .record_verifier_authority(&authority_request(
            VerifierAuthorityConsumptionRuleV1::ReusableWhileActive,
            None,
        ))
        .expect("the authority is issued");
    let reference = authority_reference(&authority);
    service
        .record_verifier_audit_evidence(&evidence_request(
            0x70,
            reference.clone(),
            VerifierEvidenceKindV1::QualifyingFail,
        ))
        .expect("the evidence is recorded");
    let baseline = service
        .record_verifier_audit_baseline(&baseline_request(reference.clone()))
        .expect("the baseline is frozen");
    let baseline_digest = stored_digest(&baseline.canonical_baseline_digest);

    let stale = service
        .apply_verifier_target_mutation(&mutation_request(
            0x61,
            0x62,
            reference,
            baseline_digest,
            VerifierOperationV1::MarkNeedsRework,
            vec![identity_text(0x70)],
            VerifierCommittedStateV1 {
                target_revision: 4,
                ..committed_state(&authority)
            },
        ))
        .expect_err("a stale target revision fails closed");
    assert_eq!(stale.code(), "verifier_baseline_stale_target_revision");
    assert!(store.mutations().is_empty());
}

#[test]
fn verifier_evidence_and_verdict_records_are_immutable() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);
    let authority = service
        .record_verifier_authority(&authority_request(
            VerifierAuthorityConsumptionRuleV1::ReusableWhileActive,
            None,
        ))
        .expect("the authority is issued");
    let reference = authority_reference(&authority);
    service
        .record_verifier_audit_evidence(&evidence_request(
            0x70,
            reference.clone(),
            VerifierEvidenceKindV1::QualifyingFail,
        ))
        .expect("the evidence is recorded");

    let verdict = service
        .record_verifier_audit_verdict(&RecordVerifierVerdictRequestDto {
            verdict_id: identity_text(0x71),
            authority_reference: reference.clone(),
            target_reference: target_reference(),
            baseline_digest: digest(0x72),
            verdict: VerificationAuditVerdictDto::Fail,
            evidence_references: vec![identity_text(0x70)],
            occurred_at_ms: 3_000,
        })
        .expect("the verdict is durable evidence");
    assert_eq!(verdict.verdict, VerificationAuditVerdictDto::Fail);

    let duplicated = service
        .record_verifier_audit_verdict(&RecordVerifierVerdictRequestDto {
            verdict_id: identity_text(0x73),
            authority_reference: reference,
            target_reference: target_reference(),
            baseline_digest: digest(0x72),
            verdict: VerificationAuditVerdictDto::Fail,
            evidence_references: vec![identity_text(0x70), identity_text(0x70)],
            occurred_at_ms: 3_000,
        })
        .expect_err("a verdict names each evidence reference once");
    assert_eq!(duplicated.code(), "verifier_verdict_invalid");
}

#[test]
fn recovery_preserves_authority_evidence_and_mutation_history() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);
    let authority = service
        .record_verifier_authority(&authority_request(
            VerifierAuthorityConsumptionRuleV1::SingleUse,
            None,
        ))
        .expect("the authority is issued");
    let reference = authority_reference(&authority);
    service
        .record_verifier_audit_evidence(&evidence_request(
            0x70,
            reference.clone(),
            VerifierEvidenceKindV1::QualifyingFail,
        ))
        .expect("the evidence is recorded");
    let baseline = service
        .record_verifier_audit_baseline(&baseline_request(reference.clone()))
        .expect("the baseline is frozen");
    let baseline_digest = stored_digest(&baseline.canonical_baseline_digest);
    service
        .apply_verifier_target_mutation(&mutation_request(
            0x61,
            0x62,
            reference,
            baseline_digest,
            VerifierOperationV1::MarkNeedsRework,
            vec![identity_text(0x70)],
            committed_state(&authority),
        ))
        .expect("the mutation commits");

    let recovered = service
        .recover_goal_verification(&GoalVerificationRecoveryRequestDto {
            authority_id: identity_text(AUTHORITY),
            authority_revision: 1,
            committed_mutation_id: identity_text(0x61),
        })
        .expect("the durable history is read without replay");
    assert!(!recovered.evidence_work_replayed);
    assert!(!recovered.mutation_reapplied);
    assert!(recovered.later_attempt_requires_new_admission);
    assert_eq!(
        recovered.committed_mutation.mutation_id,
        identity_text(0x61)
    );

    store.tamper_mutation_authority(&identity_text(0x61));
    let mismatch = service
        .recover_goal_verification(&GoalVerificationRecoveryRequestDto {
            authority_id: identity_text(AUTHORITY),
            authority_revision: 1,
            committed_mutation_id: identity_text(0x61),
        })
        .expect_err("a foreign mutation authority fails closed");
    assert_eq!(mismatch.code(), "verifier_authority_identity_mismatch");
}

#[test]
fn boundary_rejects_paths_and_credentials_before_any_repository_call() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);

    let path = service
        .record_compaction_suffix_reference(&RecordCompactionSuffixRequestDto {
            scope: record_project_scope(),
            history_reference: "/var/lib/intention/history".to_owned(),
            occurred_at_ms: 1_500,
        })
        .expect_err("a filesystem path never crosses the boundary");
    assert_eq!(path.code(), "compaction_history_unavailable");

    let credential = service
        .record_compaction_suffix_reference(&RecordCompactionSuffixRequestDto {
            scope: record_project_scope(),
            history_reference: "api_key=secret-value".to_owned(),
            occurred_at_ms: 1_500,
        })
        .expect_err("credential-shaped text is rejected");
    assert_eq!(credential.code(), "credentials_forbidden");

    assert!(store.calls().is_empty());
}

#[test]
fn attach_child_fails_closed_at_the_tree_depth_bound() {
    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    seed_session_child(&store, CHILD);
    store.set_tree_depth(16);
    let service = GoalRuntimeService::new(&store);

    let depth = service
        .attach_goal_child(&AttachGoalChildRequestDto {
            parent_goal_id: identity_text(GOAL),
            child_goal_id: identity_text(CHILD),
            child_revision_at_link: 1,
            session_link: Some(
                intention_application::goal_domain::GoalSessionLinkCreationRequestDto {
                    link_id: identity_text(0x0c),
                    session_id: identity_text(SESSION),
                    effective_from_revision: 1,
                },
            ),
            occurred_at_ms: 2_000,
        })
        .expect_err("the Goal-tree depth bound is exceeded");
    assert_eq!(depth.code(), "goal_tree_depth_limit_exceeded");
    assert!(store.children.borrow().is_empty());
    assert_eq!(store.session_links.borrow().len(), 1);
}

#[test]
fn create_goal_hashes_every_inherited_rule_reference() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);
    let created = service
        .create_goal(&CreateGoalRequestDto {
            goal_id: identity_text(GOAL),
            scope: project_scope(),
            title: "Ship the Goal runtime".to_owned(),
            objective: "Admit goal-directed runs atomically".to_owned(),
            inherited_rule_references: vec![identity_text(0x11), identity_text(0x12)],
            local_rule_references: vec![identity_text(0x13)],
            required_gate_references: Vec::new(),
            occurred_at_ms: 1_000,
        })
        .expect("the inherited and local rule references are hashed into the revision");
    assert_eq!(created.active_revision, 1);
    assert_eq!(
        store.revisions.borrow()[0].inherited_rule_references.len(),
        2
    );
    assert_eq!(store.revisions.borrow()[0].local_rule_references.len(), 1);
}

#[test]
fn admission_rejects_an_unusable_durable_session_link_identity() {
    for link_id in ["not-an-identity".to_owned(), "a".repeat(257)] {
        let store = FakeGoalStore::new();
        seed_project_goal(&store);
        store
            .session_links
            .borrow_mut()
            .push(GoalSessionLinkRecordDto {
                link_id,
                ..session_link()
            });
        let service = GoalRuntimeService::new(&store);
        let port = FakeAdmissionPort::new(store.log_handle());
        let error = service
            .admit_leading_goal_run(&admission_request(project_selection(1), 4_096), &port)
            .expect_err("a durable session link identity is canonical");
        assert_eq!(error.code(), "goal_not_active");
        assert!(port.commits().is_empty());
    }
}

#[test]
fn create_goal_gate_derives_the_definition_digest_of_every_definition() {
    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    let service = GoalRuntimeService::new(&store);

    let unusable = service
        .create_goal_gate(&CreateGoalGateRequestDto {
            gate_id: "not-an-identity".to_owned(),
            goal_id: identity_text(GOAL),
            definition: GoalGateDefinitionDto::Executable {
                template_id: identity_text(TEMPLATE),
                template_revision: 1,
            },
            occurred_at_ms: 2_000,
        })
        .expect_err("a durable gate identity is canonical daemon-assigned text");
    assert_eq!(unusable.code(), "goal_gate_unavailable");
    assert!(store.gates.borrow().is_empty());

    let reference = service
        .create_goal_gate(&CreateGoalGateRequestDto {
            gate_id: identity_text(GATE),
            goal_id: identity_text(GOAL),
            definition: GoalGateDefinitionDto::Reference {
                evidence_contract_revision: 2,
                accepted_reference_kinds: vec![
                    GoalEvidenceKindDto::TerminalRegisteredToolResult,
                    GoalEvidenceKindDto::AcceptedUserDeclaration,
                ],
            },
            occurred_at_ms: 2_000,
        })
        .expect("the reference gate revision is created");
    assert_eq!(reference.revision, 1);
    assert_eq!(reference.canonical_revision_digest.len(), 71);

    let executable = service
        .create_goal_gate(&CreateGoalGateRequestDto {
            gate_id: identity_text(EXECUTABLE_GATE),
            goal_id: identity_text(GOAL),
            definition: GoalGateDefinitionDto::Executable {
                template_id: identity_text(TEMPLATE),
                template_revision: 1,
            },
            occurred_at_ms: 2_000,
        })
        .expect("the executable gate revision is created");
    assert_eq!(
        executable.definition,
        GoalGateDefinitionDto::Executable {
            template_id: identity_text(TEMPLATE),
            template_revision: 1,
        }
    );
    assert_eq!(store.gates.borrow().len(), 2);
    assert!(store.called("gate-create"));
}

#[test]
fn gate_outcome_kinds_cover_the_closed_dispositions() {
    let store = FakeGoalStore::new();
    store.push_goal(project_goal());
    store.push_revision(revision_record(GOAL, 1, Vec::new()));
    store.push_template(template_record());
    let service = GoalRuntimeService::new(&store);

    let evaluate = |gate_seed: u8,
                    kind: GoalGateOutcomeKindDto,
                    evidence: Option<GoalEvidenceReferenceDto>| {
        store.push_gate(GoalGateRecordDto {
            gate_id: identity_text(gate_seed),
            goal_id: identity_text(GOAL),
            definition: GoalGateDefinitionDto::Executable {
                template_id: identity_text(TEMPLATE),
                template_revision: 1,
            },
            revision: 1,
            canonical_revision_digest: digest_text(gate_seed),
            created_at_ms: 1_000,
        });
        service.evaluate_goal_gate(&EvaluateGoalGateRequestDto {
            goal_id: identity_text(GOAL),
            gate_id: identity_text(gate_seed),
            producing_run_id: identity_text(0x18),
            outcome_kind: kind,
            evidence,
            occurred_at_ms: 2_000,
        })
    };
    let mut gate_seed = 0x70_u8;

    for (kind, disposition) in [
        (
            GoalGateOutcomeKindDto::TimedOut,
            intention_storage::goal_repo::GoalGateOutcomeDispositionDto::Failed,
        ),
        (
            GoalGateOutcomeKindDto::Cancelled,
            intention_storage::goal_repo::GoalGateOutcomeDispositionDto::Failed,
        ),
        (
            GoalGateOutcomeKindDto::OutputBoundExceeded,
            intention_storage::goal_repo::GoalGateOutcomeDispositionDto::Failed,
        ),
        (
            GoalGateOutcomeKindDto::ReferenceUnavailable,
            intention_storage::goal_repo::GoalGateOutcomeDispositionDto::Unavailable,
        ),
        (
            GoalGateOutcomeKindDto::RevisionStale,
            intention_storage::goal_repo::GoalGateOutcomeDispositionDto::Unavailable,
        ),
        (
            GoalGateOutcomeKindDto::TemplateUnavailable,
            intention_storage::goal_repo::GoalGateOutcomeDispositionDto::Unavailable,
        ),
    ] {
        gate_seed = gate_seed.wrapping_add(1);
        let evaluation =
            evaluate(gate_seed, kind, None).expect("a bounded outcome records its disposition");
        assert_eq!(evaluation.result.disposition, disposition);
        assert!(!evaluation.goal_moved_to_needs_rework);
        assert_eq!(
            evaluation.goal.lifecycle_state,
            GoalLifecycleStateDto::Active
        );
    }

    gate_seed = gate_seed.wrapping_add(1);
    let unknown = evaluate(
        gate_seed,
        GoalGateOutcomeKindDto::ExternalEffectUnknown,
        Some(evidence(GATE, GoalEvidenceKindDto::ExecutableGateResult)),
    )
    .expect("an unknown external effect binds its exact evidence");
    assert_eq!(
        unknown.result.disposition,
        intention_storage::goal_repo::GoalGateOutcomeDispositionDto::UnknownEffect
    );

    gate_seed = gate_seed.wrapping_add(1);
    let missing = evaluate(gate_seed, GoalGateOutcomeKindDto::Passed, None)
        .expect_err("a passing outcome requires exact evidence");
    assert_eq!(missing.code(), "goal_gate_failed");

    gate_seed = gate_seed.wrapping_add(1);
    let unexpected = evaluate(
        gate_seed,
        GoalGateOutcomeKindDto::TimedOut,
        Some(evidence(GATE, GoalEvidenceKindDto::ExecutableGateResult)),
    )
    .expect_err("a timed-out outcome carries no evidence reference");
    assert_eq!(unexpected.code(), "goal_gate_failed");
}

#[test]
fn evaluate_goal_gate_rejects_inactive_foreign_and_incoherent_evaluations() {
    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    store.push_gate(reference_gate(vec![
        GoalEvidenceKindDto::AcceptedUserDeclaration,
    ]));
    store.set_goal_lifecycle(&identity_text(GOAL), GoalLifecycleStateDto::Paused);
    let service = GoalRuntimeService::new(&store);
    let paused = service
        .evaluate_goal_gate(&EvaluateGoalGateRequestDto {
            goal_id: identity_text(GOAL),
            gate_id: identity_text(GATE),
            producing_run_id: identity_text(0x18),
            outcome_kind: GoalGateOutcomeKindDto::Passed,
            evidence: Some(evidence(GATE, GoalEvidenceKindDto::AcceptedUserDeclaration)),
            occurred_at_ms: 2_000,
        })
        .expect_err("only an Active or NeedsRework Goal evaluates a required gate");
    assert_eq!(paused.code(), "goal_not_active");
    assert!(store.gate_results.borrow().is_empty());

    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    store.push_gate(GoalGateRecordDto {
        goal_id: identity_text(CHILD),
        ..reference_gate(vec![GoalEvidenceKindDto::AcceptedUserDeclaration])
    });
    let service = GoalRuntimeService::new(&store);
    let foreign = service
        .evaluate_goal_gate(&EvaluateGoalGateRequestDto {
            goal_id: identity_text(GOAL),
            gate_id: identity_text(GATE),
            producing_run_id: identity_text(0x18),
            outcome_kind: GoalGateOutcomeKindDto::Passed,
            evidence: Some(evidence(GATE, GoalEvidenceKindDto::AcceptedUserDeclaration)),
            occurred_at_ms: 2_000,
        })
        .expect_err("the gate does not belong to this Goal");
    assert_eq!(foreign.code(), "goal_gate_unavailable");
    assert!(store.gate_results.borrow().is_empty());

    let store = FakeGoalStore::new();
    store.push_goal(project_goal());
    store.push_revision(revision_record(GOAL, 1, Vec::new()));
    store.push_gate(executable_gate());
    store.push_template(template_record());
    let service = GoalRuntimeService::new(&store);
    let incoherent = service
        .evaluate_goal_gate(&EvaluateGoalGateRequestDto {
            goal_id: identity_text(GOAL),
            gate_id: identity_text(EXECUTABLE_GATE),
            producing_run_id: identity_text(0x18),
            outcome_kind: GoalGateOutcomeKindDto::Passed,
            evidence: Some(evidence(GATE, GoalEvidenceKindDto::AcceptedUserDeclaration)),
            occurred_at_ms: 2_000,
        })
        .expect_err("an executable gate result carries the executable-gate evidence provenance");
    assert_eq!(incoherent.code(), "goal_gate_failed");
    assert!(store.gate_results.borrow().is_empty());
}

#[test]
fn goal_and_session_gate_templates_bind_their_scopes_and_scalars() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);

    let blank = service
        .create_goal_gate_template(&CreateGoalGateTemplateRequestDto {
            capability_reference: "   ".to_owned(),
            ..template_request()
        })
        .expect_err("a gate template names a non-blank capability reference");
    assert_eq!(blank.code(), "goal_gate_unavailable");

    let oversized = service
        .create_goal_gate_template(&CreateGoalGateTemplateRequestDto {
            capability_reference: "a".repeat(257),
            ..template_request()
        })
        .expect_err("a gate template capability stays inside its closed scalar bound");
    assert_eq!(oversized.code(), "goal_gate_unavailable");

    let goal_scoped = service
        .create_goal_gate_template(&CreateGoalGateTemplateRequestDto {
            scope: GoalTemplateScopeDto::Goal {
                goal_id: identity_text(GOAL),
            },
            input_family: GoalGateInputFamilyDto::ClosedPathSetV1,
            requires_confirmation: false,
            ..template_request()
        })
        .expect("a Goal-scoped template is created without confirmation");
    assert_eq!(
        goal_scoped.input_family,
        GoalGateInputFamilyDto::ClosedPathSetV1
    );
    assert!(!goal_scoped.requires_confirmation);
    assert_eq!(goal_scoped.canonical_digest.len(), 71);

    let session_scoped = service
        .create_goal_gate_template(&CreateGoalGateTemplateRequestDto {
            template_id: identity_text(0x0c),
            scope: GoalTemplateScopeDto::Session {
                session_id: identity_text(SESSION),
            },
            ..template_request()
        })
        .expect("a Session-scoped template is created");
    assert_eq!(
        session_scoped.scope,
        GoalTemplateScopeDto::Session {
            session_id: identity_text(SESSION),
        }
    );

    store.push_goal(project_goal());
    store.push_revision(revision_record(GOAL, 1, Vec::new()));
    store.push_gate(GoalGateRecordDto {
        gate_id: identity_text(0x0d),
        goal_id: identity_text(GOAL),
        definition: GoalGateDefinitionDto::Executable {
            template_id: identity_text(TEMPLATE),
            template_revision: 1,
        },
        revision: 1,
        canonical_revision_digest: digest_text(0x17),
        created_at_ms: 1_000,
    });
    store.push_gate(GoalGateRecordDto {
        gate_id: identity_text(0x0e),
        goal_id: identity_text(GOAL),
        definition: GoalGateDefinitionDto::Executable {
            template_id: identity_text(0x0c),
            template_revision: 1,
        },
        revision: 1,
        canonical_revision_digest: digest_text(0x18),
        created_at_ms: 1_000,
    });
    for gate in [0x0d_u8, 0x0e_u8] {
        let evaluation = service
            .evaluate_goal_gate(&EvaluateGoalGateRequestDto {
                goal_id: identity_text(GOAL),
                gate_id: identity_text(gate),
                producing_run_id: identity_text(0x1a),
                outcome_kind: GoalGateOutcomeKindDto::Passed,
                evidence: Some(evidence(gate, GoalEvidenceKindDto::ExecutableGateResult)),
                occurred_at_ms: 2_000,
            })
            .expect("the scoped executable template resolves");
        assert_eq!(
            evaluation.result.disposition,
            intention_storage::goal_repo::GoalGateOutcomeDispositionDto::Passed
        );
    }
}

#[test]
fn admission_consumes_a_ready_goal_and_every_session_memory_kind() {
    let store = FakeGoalStore::new();
    seed_project_goal(&store);
    let service = GoalRuntimeService::new(&store);
    let ready = service
        .claim_goal_readiness(&ClaimGoalReadinessRequestDto {
            goal_id: identity_text(GOAL),
            expected_revision: 1,
            verified_evidence_set: vec![evidence(
                GATE,
                GoalEvidenceKindDto::AcceptedUserDeclaration,
            )],
            occurred_at_ms: 2_000,
        })
        .expect("a childless Goal claims Ready against selected evidence");
    assert!(matches!(
        ready.readiness_state,
        GoalReadinessStateDto::Ready { .. }
    ));

    for (seed, kind) in [
        (0x20_u8, MemoryKindDto::Fact),
        (0x23_u8, MemoryKindDto::Preference),
        (0x24_u8, MemoryKindDto::PastFailure),
    ] {
        store
            .memory_cards
            .borrow_mut()
            .push(GoalMemoryCardRecordDto {
                record_id: identity_text(seed),
                revision: 1,
                kind,
                scope: GoalRecordScopeDto::Session {
                    session_id: identity_text(SESSION),
                },
                ..memory_card_record(1)
            });
    }
    let mut selection = project_selection(1);
    selection.selected_memory_cards = vec![
        GoalCardReferenceV1 {
            card_reference: identity(0x20),
            revision: 1,
        },
        GoalCardReferenceV1 {
            card_reference: identity(0x23),
            revision: 1,
        },
        GoalCardReferenceV1 {
            card_reference: identity(0x24),
            revision: 1,
        },
    ];
    let port = FakeAdmissionPort::new(store.log_handle());
    let admitted = service
        .admit_leading_goal_run(&admission_request(selection, 4_096), &port)
        .expect("a Ready Goal with session-scoped memory cards admits the run");
    assert_eq!(admitted.goal_revision, 1);
    assert_eq!(port.commits().len(), 1);
}

#[test]
fn every_verifier_evidence_kind_and_verdict_round_trips_through_storage() {
    let store = FakeGoalStore::new();
    let service = GoalRuntimeService::new(&store);
    let authority = service
        .record_verifier_authority(&authority_request(
            VerifierAuthorityConsumptionRuleV1::ReusableWhileActive,
            None,
        ))
        .expect("the authority is issued");
    let reference = authority_reference(&authority);

    for (index, kind) in [
        VerifierEvidenceKindV1::UnconditionalPass,
        VerifierEvidenceKindV1::QualifyingFail,
        VerifierEvidenceKindV1::Inconclusive,
        VerifierEvidenceKindV1::GraphTerminalizationClosure,
        VerifierEvidenceKindV1::ReconciliationStandardProof,
    ]
    .into_iter()
    .enumerate()
    {
        let seed = 0x70_u8 + u8::try_from(index).expect("the fixture index fits one byte");
        let recorded = service
            .record_verifier_audit_evidence(&evidence_request(seed, reference.clone(), kind))
            .expect("the closed evidence kind is durable");
        assert_eq!(recorded.evidence_kind, kind);
        assert_eq!(recorded.evidence_id, identity(seed));
    }

    for (index, verdict_kind) in [
        VerificationAuditVerdictDto::Pass,
        VerificationAuditVerdictDto::Fail,
        VerificationAuditVerdictDto::Inconclusive,
        VerificationAuditVerdictDto::TargetRevisionStale,
        VerificationAuditVerdictDto::TargetUnavailable,
        VerificationAuditVerdictDto::VerifierUnavailable,
        VerificationAuditVerdictDto::VerifierExternalEffectUnknown,
    ]
    .into_iter()
    .enumerate()
    {
        let seed = 0x80_u8 + u8::try_from(index).expect("the fixture index fits one byte");
        let verdict = service
            .record_verifier_audit_verdict(&RecordVerifierVerdictRequestDto {
                verdict_id: identity_text(seed),
                authority_reference: reference.clone(),
                target_reference: target_reference(),
                baseline_digest: digest(0x72),
                verdict: verdict_kind,
                evidence_references: vec![identity_text(0x70)],
                occurred_at_ms: 3_000,
            })
            .expect("the closed verdict is durable evidence");
        assert_eq!(verdict.verdict, verdict_kind);
        assert_eq!(store.verdicts.borrow().len(), index + 1);
    }
    assert_eq!(store.evidence.borrow().len(), 5);
    assert_eq!(store.verdicts.borrow().len(), 7);
}
