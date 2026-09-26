#![allow(
    clippy::expect_used,
    reason = "M5+ verification contract fixtures use expect for precise test diagnostics."
)]

//! Slice 3 unified verification contract tests.
//!
//! Owner: architecture 17 with architecture 28's Goal-facing surface and
//! ADR 0044. Every test states the required-evidence row it satisfies.

use intention_domain::canonical::{CanonicalError, Digest256};
use intention_domain::verification::{
    MAX_VERIFIER_RECORD_BYTES, UserLifecycleOperationV1, VerificationAuditEvidenceDto,
    VerificationAuditVerdictDto, VerifierAuditBaselineV1, VerifierAuditEvidenceV1,
    VerifierAuditVerdictRecordV1, VerifierAuthorityConsumptionRuleV1,
    VerifierAuthorityConsumptionV1, VerifierAuthorityLifecycleV1, VerifierAuthorityReferenceV1,
    VerifierAuthorityV1, VerifierBaselineCheckV1, VerifierBaselineElementV1,
    VerifierCommittedStateV1, VerifierCompetingMutationV1, VerifierConflictWinnerV1,
    VerifierContractReferenceV1, VerifierEvidenceKindV1, VerifierFailClosedRecoveryV1,
    VerifierFrozenGoalGateEvidenceReferencesV1, VerifierGoalReferenceV1,
    VerifierMandatorySelectionStateV1, VerifierOperationEffectV1, VerifierOperationIdentityV1,
    VerifierOperationPreconditionContextV1, VerifierOperationV1, VerifierPreconditionFailureV1,
    VerifierReconciliationOutcomeV1, VerifierReconciliationV1, VerifierSelectionDefectV1,
    VerifierTargetLifecycleV1, VerifierTargetMutationV1, VerifierTargetReferenceV1,
    VerifierTargetRevisionAndSequenceV1, VerifierTargetSetReferenceV1, evaluate_baseline_freshness,
    resolve_verifier_optimistic_conflict, validate_baseline_freshness,
    validate_verifier_authority_reference, validate_verifier_authority_use,
    validate_verifier_mandatory_selection, validate_verifier_operation_preconditions,
};

const AUTHORITY_ID: [u8; 16] = [0x11; 16];
const VERIFIER_MANDATE_ID: [u8; 16] = [0x22; 16];
const TARGET_SET_ID: [u8; 16] = [0x33; 16];
const CONTRACT_ID: [u8; 16] = [0x44; 16];
const GOAL_ID: [u8; 16] = [0x55; 16];
const TARGET_MANDATE_ID: [u8; 16] = [0x66; 16];
const UNKNOWN_EFFECT_REFERENCE: [u8; 16] = [0x77; 16];
const EVIDENCE_ID: [u8; 16] = [0x88; 16];
const MUTATION_ID: [u8; 16] = [0x99; 16];
const OPERATION_ID: [u8; 16] = [0xAA; 16];
const RECONCILIATION_ID: [u8; 16] = [0xBB; 16];
const VERDICT_ID: [u8; 16] = [0xCC; 16];
const TARGET_REVISION: u64 = 11;
const TARGET_SEQUENCE: u64 = 42;

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn contract() -> VerifierContractReferenceV1 {
    VerifierContractReferenceV1 {
        contract_id: CONTRACT_ID,
        contract_revision: 2,
        contract_digest: Digest256::sha256(b"audit-contract-v1"),
    }
}

fn frozen_references() -> VerifierFrozenGoalGateEvidenceReferencesV1 {
    VerifierFrozenGoalGateEvidenceReferencesV1 {
        goal_references: vec![VerifierGoalReferenceV1 {
            goal_id: GOAL_ID,
            goal_revision: 7,
        }],
        gate_evidence_contract_references: vec![contract()],
    }
}

const fn lifecycle() -> VerifierAuthorityLifecycleV1 {
    VerifierAuthorityLifecycleV1 {
        issued_at_ms: 1_000,
        expires_at_ms: Some(2_000_000),
        revoked_at_ms: None,
        revocation_reference: None,
        consumption_rule: VerifierAuthorityConsumptionRuleV1::SingleUse,
        consumption: VerifierAuthorityConsumptionV1::Unconsumed,
    }
}

fn authority_with_lifecycle(lifecycle: VerifierAuthorityLifecycleV1) -> VerifierAuthorityV1 {
    VerifierAuthorityV1::new(
        AUTHORITY_ID,
        5,
        VERIFIER_MANDATE_ID,
        VerifierTargetSetReferenceV1 {
            target_set_id: TARGET_SET_ID,
            target_set_digest: Digest256::sha256(b"target-set-v1"),
        },
        vec![
            VerifierOperationV1::ResolveUnknownEffect,
            VerifierOperationV1::MarkComplete,
            VerifierOperationV1::MarkNeedsRework,
            VerifierOperationV1::ReviseFull,
            VerifierOperationV1::Stop,
        ],
        contract(),
        lifecycle,
    )
    .expect("authority fixture is valid")
}

fn authority() -> VerifierAuthorityV1 {
    authority_with_lifecycle(lifecycle())
}

fn authority_reference() -> VerifierAuthorityReferenceV1 {
    VerifierAuthorityReferenceV1 {
        authority_id: AUTHORITY_ID,
        authority_revision: 5,
        authority_digest: authority().canonical_digest(),
    }
}

const fn target_reference() -> VerifierTargetReferenceV1 {
    VerifierTargetReferenceV1 {
        target_mandate_id: TARGET_MANDATE_ID,
        target_revision: TARGET_REVISION,
    }
}

fn baseline_with_frozen_references(
    frozen_references: VerifierFrozenGoalGateEvidenceReferencesV1,
) -> Result<VerifierAuditBaselineV1, intention_types::ErrorDto> {
    VerifierAuditBaselineV1::new(
        authority_reference(),
        3,
        TARGET_MANDATE_ID,
        VerifierTargetRevisionAndSequenceV1 {
            revision: TARGET_REVISION,
            aggregate_sequence: TARGET_SEQUENCE,
        },
        VerifierTargetLifecycleV1::PausedAwaitingDecision,
        frozen_references,
        Some(UNKNOWN_EFFECT_REFERENCE),
        contract(),
        Some(9),
    )
}

fn baseline() -> VerifierAuditBaselineV1 {
    baseline_with_frozen_references(frozen_references()).expect("baseline fixture is valid")
}

fn evidence() -> VerifierAuditEvidenceV1 {
    VerifierAuditEvidenceV1::new(
        EVIDENCE_ID,
        authority_reference(),
        target_reference(),
        frozen_references(),
        VerifierEvidenceKindV1::UnconditionalPass,
        [0xDD; 16],
    )
    .expect("evidence fixture is valid")
}

fn verdict_record() -> VerifierAuditVerdictRecordV1 {
    VerifierAuditVerdictRecordV1::new(
        VERDICT_ID,
        authority_reference(),
        target_reference(),
        baseline().canonical_digest(),
        VerificationAuditVerdictDto::Pass,
        vec![EVIDENCE_ID],
    )
    .expect("verdict fixture is valid")
}

fn mutation() -> VerifierTargetMutationV1 {
    VerifierTargetMutationV1::new(
        MUTATION_ID,
        authority_reference(),
        contract(),
        target_reference(),
        VerifierOperationV1::MarkComplete,
        vec![EVIDENCE_ID],
        TARGET_REVISION,
        TARGET_SEQUENCE,
        baseline().canonical_digest(),
        operation_identity(),
    )
    .expect("mutation fixture is valid")
}

fn reconciliation() -> VerifierReconciliationV1 {
    VerifierReconciliationV1::new(
        RECONCILIATION_ID,
        Some(authority_reference()),
        target_reference(),
        baseline().canonical_digest(),
        UNKNOWN_EFFECT_REFERENCE,
        VerifierReconciliationOutcomeV1::ActiveForLaterFreshWork,
    )
    .expect("reconciliation fixture is valid")
}

fn operation_identity() -> VerifierOperationIdentityV1 {
    VerifierOperationIdentityV1 {
        operation_id: OPERATION_ID,
        operation_digest: Digest256::sha256(b"operation-v1"),
    }
}

fn committed_state() -> VerifierCommittedStateV1 {
    VerifierCommittedStateV1 {
        target_mandate_id: TARGET_MANDATE_ID,
        target_revision: TARGET_REVISION,
        target_aggregate_sequence: TARGET_SEQUENCE,
        target_lifecycle: VerifierTargetLifecycleV1::PausedAwaitingDecision,
        authority_reference: authority_reference(),
        audit_contract_reference: contract(),
        graph_epoch: Some(9),
        committed_operation: None,
    }
}

fn stale_committed_state(element: VerifierBaselineElementV1) -> VerifierCommittedStateV1 {
    let fresh = committed_state();
    match element {
        VerifierBaselineElementV1::TargetIdentity => VerifierCommittedStateV1 {
            target_mandate_id: [0x91; 16],
            ..fresh
        },
        VerifierBaselineElementV1::TargetRevision => VerifierCommittedStateV1 {
            target_revision: fresh.target_revision + 1,
            ..fresh
        },
        VerifierBaselineElementV1::TargetSequence => VerifierCommittedStateV1 {
            target_aggregate_sequence: fresh.target_aggregate_sequence + 1,
            ..fresh
        },
        VerifierBaselineElementV1::TargetLifecycle => VerifierCommittedStateV1 {
            target_lifecycle: VerifierTargetLifecycleV1::Active,
            ..fresh
        },
        VerifierBaselineElementV1::AuthorityIdentity => VerifierCommittedStateV1 {
            authority_reference: VerifierAuthorityReferenceV1 {
                authority_id: [0x92; 16],
                ..fresh.authority_reference
            },
            ..fresh
        },
        VerifierBaselineElementV1::AuthorityRevision => VerifierCommittedStateV1 {
            authority_reference: VerifierAuthorityReferenceV1 {
                authority_revision: fresh.authority_reference.authority_revision + 1,
                ..fresh.authority_reference
            },
            ..fresh
        },
        VerifierBaselineElementV1::AuthorityDigest => VerifierCommittedStateV1 {
            authority_reference: VerifierAuthorityReferenceV1 {
                authority_digest: Digest256::sha256(b"other-authority"),
                ..fresh.authority_reference
            },
            ..fresh
        },
        VerifierBaselineElementV1::AuditContract => VerifierCommittedStateV1 {
            audit_contract_reference: VerifierContractReferenceV1 {
                contract_revision: fresh.audit_contract_reference.contract_revision + 1,
                ..fresh.audit_contract_reference
            },
            ..fresh
        },
        VerifierBaselineElementV1::GraphEpoch => VerifierCommittedStateV1 {
            graph_epoch: Some(10),
            ..fresh
        },
        VerifierBaselineElementV1::OperationIdentity => VerifierCommittedStateV1 {
            committed_operation: Some(VerifierOperationIdentityV1 {
                operation_id: [0x93; 16],
                operation_digest: operation_identity().operation_digest,
            }),
            ..fresh
        },
        VerifierBaselineElementV1::OperationDigest => VerifierCommittedStateV1 {
            committed_operation: Some(VerifierOperationIdentityV1 {
                operation_id: operation_identity().operation_id,
                operation_digest: Digest256::sha256(b"changed-operation"),
            }),
            ..fresh
        },
    }
}

const fn context_for(operation: VerifierOperationV1) -> VerifierOperationPreconditionContextV1 {
    match operation {
        VerifierOperationV1::MarkNeedsRework => VerifierOperationPreconditionContextV1 {
            authority_includes_operation: true,
            target_lifecycle: VerifierTargetLifecycleV1::Active,
            qualifying_fail_evidence: true,
            ..VerifierOperationPreconditionContextV1::empty()
        },
        VerifierOperationV1::MarkComplete => VerifierOperationPreconditionContextV1 {
            authority_includes_operation: true,
            target_lifecycle: VerifierTargetLifecycleV1::Active,
            unconditional_pass_evidence: true,
            graph_terminalization_closed: true,
            ..VerifierOperationPreconditionContextV1::empty()
        },
        VerifierOperationV1::Stop => VerifierOperationPreconditionContextV1 {
            authority_includes_operation: true,
            target_lifecycle: VerifierTargetLifecycleV1::Working,
            ..VerifierOperationPreconditionContextV1::empty()
        },
        VerifierOperationV1::ReviseFull => VerifierOperationPreconditionContextV1 {
            authority_includes_operation: true,
            target_lifecycle: VerifierTargetLifecycleV1::Active,
            ..VerifierOperationPreconditionContextV1::empty()
        },
        VerifierOperationV1::ResolveUnknownEffect => VerifierOperationPreconditionContextV1 {
            authority_includes_operation: true,
            target_lifecycle: VerifierTargetLifecycleV1::PausedAwaitingDecision,
            reconciliation_standard_proven: true,
            baseline_unknown_effect_reference: Some(UNKNOWN_EFFECT_REFERENCE),
            exact_uncertainty_reference: Some(UNKNOWN_EFFECT_REFERENCE),
            reconciliation_outcome: Some(VerifierReconciliationOutcomeV1::ActiveForLaterFreshWork),
            ..VerifierOperationPreconditionContextV1::empty()
        },
    }
}

#[test]
fn authority_round_trips_with_a_canonical_operation_order_and_verified_digest() {
    // Architecture 17 required evidence: canonical authority golden and
    // negative vectors.
    let record = authority();
    assert_eq!(
        record.allowed_operations,
        VerifierOperationV1::ALL.to_vec(),
        "the constructor normalizes the closed operation set into canonical order"
    );
    assert!(record.allows_operation(VerifierOperationV1::MarkComplete));
    let encoded = record.encode().expect("authority encodes");
    assert!(encoded.len() <= MAX_VERIFIER_RECORD_BYTES);
    let decoded = VerifierAuthorityV1::decode(&encoded).expect("authority decodes");
    assert_eq!(decoded, record);
    assert_eq!(decoded.canonical_digest(), record.canonical_digest());
    assert_eq!(
        record
            .identity_digest()
            .expect("authority identity digests")
            .digest,
        record.canonical_digest()
    );
    assert_eq!(decoded.encode().expect("authority re-encodes"), encoded);
}

#[test]
fn baseline_evidence_verdict_mutation_and_reconciliation_round_trip() {
    // Architecture 17 required evidence: canonical baseline/evidence/verdict/
    // mutation/reconciliation goldens.
    let baseline = baseline();
    let encoded = baseline.encode().expect("baseline encodes");
    assert_eq!(
        VerifierAuditBaselineV1::decode(&encoded).expect("baseline decodes"),
        baseline
    );

    let evidence = evidence();
    let encoded = evidence.encode().expect("evidence encodes");
    assert_eq!(
        VerifierAuditEvidenceV1::decode(&encoded).expect("evidence decodes"),
        evidence
    );

    let verdict = verdict_record();
    let encoded = verdict.encode().expect("verdict encodes");
    assert_eq!(
        VerifierAuditVerdictRecordV1::decode(&encoded).expect("verdict decodes"),
        verdict
    );

    let mutation = mutation();
    let encoded = mutation.encode().expect("mutation encodes");
    assert_eq!(
        VerifierTargetMutationV1::decode(&encoded).expect("mutation decodes"),
        mutation
    );

    let reconciliation = reconciliation();
    let encoded = reconciliation.encode().expect("reconciliation encodes");
    assert_eq!(
        VerifierReconciliationV1::decode(&encoded).expect("reconciliation decodes"),
        reconciliation
    );
}

#[test]
fn verifier_record_digests_match_their_frozen_golden_values() {
    // Architecture 17 required evidence: frozen verifier goldens so later
    // waves cannot silently change canonical bytes or identity digests.
    assert_eq!(
        hex_encode(&authority().canonical_digest().bytes()),
        "cd72ca7634c5b92370323700536ddd54c5f7dc8992b6f39458cafd557d988f43"
    );
    assert_eq!(
        hex_encode(&baseline().canonical_digest().bytes()),
        "bed3a3caa75d703ac357f521f46f6c388881644988ae7295f17b3a23122190ea"
    );
    assert_eq!(
        hex_encode(&evidence().canonical_digest().bytes()),
        "3b4721278b4ca83e34e997d02d35c71a3e01e6920eb4f5bbf094978964fc4e63"
    );
    assert_eq!(
        hex_encode(&verdict_record().canonical_digest().bytes()),
        "44e17032644a7122ab4881b5bee7054aa749790114b08c61d6563414addc95be"
    );
    assert_eq!(
        hex_encode(&mutation().canonical_digest().bytes()),
        "39d96aa60ed57cde15cded7dd910810d9056faa24824c6021ed4487ccf5b6909"
    );
    assert_eq!(
        hex_encode(&reconciliation().canonical_digest().bytes()),
        "9ef54f1235387c323ea2221d57aab35a5ef4fa674526b7ced8f4f9c5ee598c8c"
    );
}

#[test]
fn decode_rejects_tampered_digests_wrong_frames_and_over_limit_bytes() {
    // Architecture 17 required evidence: negative canonical vectors.
    let encoded = authority().encode().expect("authority encodes");
    let mut tampered = encoded.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xff;
    assert_eq!(
        VerifierAuthorityV1::decode(&tampered).expect_err("tampered digest is rejected"),
        CanonicalError::DigestMismatch
    );

    let mut wrong_tag = encoded.clone();
    wrong_tag[8..12].copy_from_slice(&1u32.to_be_bytes());
    assert_eq!(
        VerifierAuthorityV1::decode(&wrong_tag).expect_err("wrong tag is rejected"),
        CanonicalError::InvalidTag
    );

    let mut wrong_version = encoded;
    wrong_version[12..16].copy_from_slice(&2u32.to_be_bytes());
    assert_eq!(
        VerifierAuthorityV1::decode(&wrong_version).expect_err("wrong version is rejected"),
        CanonicalError::InvalidTag
    );

    let over_limit = vec![0u8; MAX_VERIFIER_RECORD_BYTES + 1];
    assert_eq!(
        VerifierAuthorityV1::decode(&over_limit).expect_err("over-limit bytes are rejected"),
        CanonicalError::OverLimit
    );
}

#[test]
fn verifier_operation_set_is_exactly_five_and_user_lifecycle_operations_have_no_authority() {
    // Architecture 17 required evidence: full operation/state matrix.
    assert_eq!(VerifierOperationV1::ALL.len(), 5);
    let verifier_codes = VerifierOperationV1::ALL
        .iter()
        .map(|operation| operation.code())
        .collect::<Vec<_>>();
    assert_eq!(
        verifier_codes,
        [
            "mark_needs_rework",
            "mark_complete",
            "stop",
            "revise_full",
            "resolve_unknown_effect"
        ]
    );
    assert_eq!(UserLifecycleOperationV1::ALL.len(), 2);
    let user_codes = UserLifecycleOperationV1::ALL
        .iter()
        .map(|operation| operation.code())
        .collect::<Vec<_>>();
    assert_eq!(user_codes, ["pause", "resume"]);
    for operation in UserLifecycleOperationV1::ALL {
        assert!(!operation.requires_verifier_authority());
        assert_eq!(operation.verifier_operation(), None);
    }
    assert!(verifier_codes.iter().all(|code| !user_codes.contains(code)));
    assert_eq!(VerifierOperationV1::from_discriminant(5), None);
    for operation in VerifierOperationV1::ALL {
        assert_eq!(
            VerifierOperationV1::from_discriminant(operation.discriminant()),
            Some(operation)
        );
    }
}

#[test]
fn every_verifier_operation_has_a_permitted_precondition_path() {
    // Architecture 17 required evidence: full operation/state matrix.
    let effects = [
        (
            VerifierOperationV1::MarkNeedsRework,
            VerifierOperationEffectV1::TargetNeedsRework,
        ),
        (
            VerifierOperationV1::MarkComplete,
            VerifierOperationEffectV1::TargetCompleted,
        ),
        (
            VerifierOperationV1::Stop,
            VerifierOperationEffectV1::TargetStopped,
        ),
        (
            VerifierOperationV1::ReviseFull,
            VerifierOperationEffectV1::FutureImmutableRevisionCreated,
        ),
        (
            VerifierOperationV1::ResolveUnknownEffect,
            VerifierOperationEffectV1::TargetActiveForLaterFreshWork,
        ),
    ];
    for (operation, effect) in effects {
        assert_eq!(
            validate_verifier_operation_preconditions(operation, &context_for(operation))
                .expect("permitted precondition path"),
            effect
        );
    }
}

#[test]
fn operation_preconditions_reject_missing_authority_before_any_other_check() {
    // Architecture 17 required evidence: authority/operation matrix.
    for operation in VerifierOperationV1::ALL {
        let mut context = context_for(operation);
        context.authority_includes_operation = false;
        let error = validate_verifier_operation_preconditions(operation, &context)
            .expect_err("missing authority operation fails closed");
        assert_eq!(
            error.code(),
            VerifierPreconditionFailureV1::AuthorityMissingOperation.code()
        );
    }
}

#[test]
fn mark_complete_requires_pass_evidence_no_uncertainty_and_graph_closure() {
    // Architecture 17 required evidence: MarkComplete preconditions.
    let base = context_for(VerifierOperationV1::MarkComplete);

    let mut context = base;
    context.unconditional_pass_evidence = false;
    let error =
        validate_verifier_operation_preconditions(VerifierOperationV1::MarkComplete, &context)
            .expect_err("pass evidence is required");
    assert_eq!(
        error.code(),
        VerifierPreconditionFailureV1::UnconditionalPassEvidenceMissing.code()
    );

    let mut context = base;
    context.unresolved_target_uncertainty = true;
    let error =
        validate_verifier_operation_preconditions(VerifierOperationV1::MarkComplete, &context)
            .expect_err("unresolved uncertainty blocks completion");
    assert_eq!(
        error.code(),
        VerifierPreconditionFailureV1::UnresolvedTargetUncertainty.code()
    );

    let mut context = base;
    context.graph_terminalization_closed = false;
    let error =
        validate_verifier_operation_preconditions(VerifierOperationV1::MarkComplete, &context)
            .expect_err("graph closure is required");
    assert_eq!(
        error.code(),
        VerifierPreconditionFailureV1::GraphTerminalizationClosureMissing.code()
    );

    let mut context = base;
    context.target_lifecycle = VerifierTargetLifecycleV1::PausedAwaitingDecision;
    let error =
        validate_verifier_operation_preconditions(VerifierOperationV1::MarkComplete, &context)
            .expect_err("uncertainty quarantine cannot complete");
    assert_eq!(
        error.code(),
        VerifierPreconditionFailureV1::TargetLifecycleInvalid.code()
    );

    assert_eq!(
        validate_verifier_operation_preconditions(VerifierOperationV1::MarkComplete, &base)
            .expect("complete path"),
        VerifierOperationEffectV1::TargetCompleted
    );
}

#[test]
fn mark_needs_rework_requires_qualifying_fail_evidence_and_only_moves_needs_rework() {
    // Architecture 17 required evidence: MarkNeedsRework preconditions.
    let base = context_for(VerifierOperationV1::MarkNeedsRework);

    let mut context = base;
    context.qualifying_fail_evidence = false;
    let error =
        validate_verifier_operation_preconditions(VerifierOperationV1::MarkNeedsRework, &context)
            .expect_err("qualifying fail evidence is required");
    assert_eq!(
        error.code(),
        VerifierPreconditionFailureV1::QualifyingFailEvidenceMissing.code()
    );

    let mut context = base;
    context.target_lifecycle = VerifierTargetLifecycleV1::Working;
    let error =
        validate_verifier_operation_preconditions(VerifierOperationV1::MarkNeedsRework, &context)
            .expect_err("a working target has an active run");
    assert_eq!(
        error.code(),
        VerifierPreconditionFailureV1::TargetLifecycleInvalid.code()
    );

    assert_eq!(
        validate_verifier_operation_preconditions(VerifierOperationV1::MarkNeedsRework, &base)
            .expect("needs-rework path"),
        VerifierOperationEffectV1::TargetNeedsRework
    );
}

#[test]
fn stop_never_asserts_completion_and_does_not_bypass_uncertainty_reconciliation() {
    // Architecture 17 required evidence: Stop and no-uncertainty-bypass rows.
    let base = context_for(VerifierOperationV1::Stop);
    assert_eq!(
        validate_verifier_operation_preconditions(VerifierOperationV1::Stop, &base)
            .expect("stop path"),
        VerifierOperationEffectV1::TargetStopped
    );

    let mut context = base;
    context.unresolved_target_uncertainty = true;
    let error = validate_verifier_operation_preconditions(VerifierOperationV1::Stop, &context)
        .expect_err("stop cannot bypass exact reconciliation");
    assert_eq!(
        error.code(),
        VerifierPreconditionFailureV1::UnresolvedTargetUncertainty.code()
    );

    let mut context = base;
    context.target_lifecycle = VerifierTargetLifecycleV1::PausedAwaitingDecision;
    let error = validate_verifier_operation_preconditions(VerifierOperationV1::Stop, &context)
        .expect_err("quarantined uncertainty needs ResolveUnknownEffect");
    assert_eq!(
        error.code(),
        VerifierPreconditionFailureV1::TargetLifecycleInvalid.code()
    );
}

#[test]
fn revise_full_requires_its_own_authority_and_creates_only_a_future_immutable_revision() {
    // Architecture 17 required evidence: ReviseFull preconditions.
    let base = context_for(VerifierOperationV1::ReviseFull);
    assert_eq!(
        validate_verifier_operation_preconditions(VerifierOperationV1::ReviseFull, &base)
            .expect("revise-full path"),
        VerifierOperationEffectV1::FutureImmutableRevisionCreated
    );

    let mut context = base;
    context.authority_includes_operation = false;
    let error =
        validate_verifier_operation_preconditions(VerifierOperationV1::ReviseFull, &context)
            .expect_err("ReviseFull requires its own authority");
    assert_eq!(
        error.code(),
        VerifierPreconditionFailureV1::AuthorityMissingOperation.code()
    );

    let mut context = base;
    context.target_lifecycle = VerifierTargetLifecycleV1::Archived;
    let error =
        validate_verifier_operation_preconditions(VerifierOperationV1::ReviseFull, &context)
            .expect_err("an archived target is inert");
    assert_eq!(
        error.code(),
        VerifierPreconditionFailureV1::TargetLifecycleInvalid.code()
    );
}

#[test]
fn resolve_unknown_effect_yields_only_active_for_fresh_work_or_stopped() {
    // Architecture 17 required evidence: exact reconciliation outcomes.
    assert_eq!(
        VerifierReconciliationOutcomeV1::ALL,
        [
            VerifierReconciliationOutcomeV1::ActiveForLaterFreshWork,
            VerifierReconciliationOutcomeV1::Stopped
        ]
    );

    let base = context_for(VerifierOperationV1::ResolveUnknownEffect);
    assert_eq!(
        validate_verifier_operation_preconditions(VerifierOperationV1::ResolveUnknownEffect, &base)
            .expect("active reconciliation path"),
        VerifierOperationEffectV1::TargetActiveForLaterFreshWork
    );

    let mut context = base;
    context.reconciliation_outcome = Some(VerifierReconciliationOutcomeV1::Stopped);
    assert_eq!(
        validate_verifier_operation_preconditions(
            VerifierOperationV1::ResolveUnknownEffect,
            &context
        )
        .expect("stopped reconciliation path"),
        VerifierOperationEffectV1::TargetStopped
    );

    let mut context = base;
    context.target_lifecycle = VerifierTargetLifecycleV1::Active;
    let error = validate_verifier_operation_preconditions(
        VerifierOperationV1::ResolveUnknownEffect,
        &context,
    )
    .expect_err("only quarantined targets reconcile");
    assert_eq!(
        error.code(),
        VerifierPreconditionFailureV1::TargetLifecycleInvalid.code()
    );

    let mut context = base;
    context.reconciliation_standard_proven = false;
    let error = validate_verifier_operation_preconditions(
        VerifierOperationV1::ResolveUnknownEffect,
        &context,
    )
    .expect_err("the reconciliation standard must be proven");
    assert_eq!(
        error.code(),
        VerifierPreconditionFailureV1::ReconciliationStandardUnproven.code()
    );

    let mut context = base;
    context.exact_uncertainty_reference = Some([0xEE; 16]);
    let error = validate_verifier_operation_preconditions(
        VerifierOperationV1::ResolveUnknownEffect,
        &context,
    )
    .expect_err("the exact uncertainty must match the frozen baseline");
    assert_eq!(
        error.code(),
        VerifierPreconditionFailureV1::ReconciliationUncertaintyMismatch.code()
    );

    let mut context = base;
    context.reconciliation_outcome = None;
    let error = validate_verifier_operation_preconditions(
        VerifierOperationV1::ResolveUnknownEffect,
        &context,
    )
    .expect_err("a closed reconciliation outcome is required");
    assert_eq!(
        error.code(),
        VerifierPreconditionFailureV1::ReconciliationOutcomeMissing.code()
    );
}

#[test]
fn every_baseline_tuple_element_fails_closed_when_stale() {
    // Architecture 17 required evidence: stale-baseline per-element matrix.
    let baseline = baseline();
    let operation = operation_identity();
    let fresh = committed_state();
    assert_eq!(
        evaluate_baseline_freshness(&baseline, &operation, &fresh).expect("fresh baseline"),
        VerifierBaselineCheckV1::Fresh
    );
    assert_eq!(
        validate_baseline_freshness(&baseline, &operation, &fresh).expect("fresh baseline"),
        VerifierBaselineCheckV1::Fresh
    );

    for element in VerifierBaselineElementV1::ALL {
        let stale = stale_committed_state(element);
        let failure = evaluate_baseline_freshness(&baseline, &operation, &stale)
            .expect_err("stale element fails closed");
        assert_eq!(failure.element, element);
        assert_eq!(failure.code(), element.code());
        assert_eq!(
            failure.recovery(),
            VerifierFailClosedRecoveryV1::FAIL_CLOSED
        );

        let error = validate_baseline_freshness(&baseline, &operation, &stale)
            .expect_err("stale element fails closed with a typed error");
        assert_eq!(error.code(), element.code());
    }
}

#[test]
fn equal_operation_identity_returns_saved_replay_and_changed_reuse_fails() {
    // Architecture 17 required evidence: idempotency identity/digest rows.
    let baseline = baseline();
    let operation = operation_identity();

    let mut committed = committed_state();
    committed.committed_operation = Some(operation);
    assert_eq!(
        evaluate_baseline_freshness(&baseline, &operation, &committed).expect("equal replay"),
        VerifierBaselineCheckV1::IdempotentReplay
    );
    assert_eq!(
        validate_baseline_freshness(&baseline, &operation, &committed).expect("equal replay"),
        VerifierBaselineCheckV1::IdempotentReplay
    );

    let mut changed = committed_state();
    changed.committed_operation = Some(VerifierOperationIdentityV1 {
        operation_id: operation.operation_id,
        operation_digest: Digest256::sha256(b"changed-operation"),
    });
    let failure = evaluate_baseline_freshness(&baseline, &operation, &changed)
        .expect_err("changed reuse fails");
    assert_eq!(failure.element, VerifierBaselineElementV1::OperationDigest);
}

#[test]
fn user_mutations_win_optimistic_conflicts_and_the_verifier_action_is_scope_limited() {
    // Architecture 17 required evidence: user/verifier conflict races.
    for competing in VerifierCompetingMutationV1::ALL {
        let resolution = resolve_verifier_optimistic_conflict(competing);
        assert!(!resolution.verifier_action_applied);
        assert_eq!(
            resolution.recovery,
            VerifierFailClosedRecoveryV1::FAIL_CLOSED
        );
        assert!(resolution.recovery.scoped_reread_required);
        assert!(!resolution.recovery.may_merge);
        assert!(!resolution.recovery.may_retarget);
        assert!(!resolution.recovery.may_change_operation);
        assert!(!resolution.recovery.may_substitute_current_state);
        assert!(!resolution.recovery.may_retry_with_changed_meaning);
        let expected = if competing.is_user_mutation() {
            VerifierConflictWinnerV1::UserMutation
        } else {
            VerifierConflictWinnerV1::CommittedConcurrentMutation
        };
        assert_eq!(resolution.winner, expected);
    }
}

#[test]
fn verifier_mandatory_selection_defects_fail_closed_without_downgrade() {
    // Architecture 17 required evidence: no-downgrade selection rule.
    assert!(
        validate_verifier_mandatory_selection(VerifierMandatorySelectionStateV1::Valid).is_ok()
    );
    for state in VerifierMandatorySelectionStateV1::ALL {
        match state.defect() {
            None => assert_eq!(state, VerifierMandatorySelectionStateV1::Valid),
            Some(defect) => {
                assert!(!defect.permits_downgrade());
                let error = validate_verifier_mandatory_selection(state)
                    .expect_err("a selection defect fails closed");
                assert_eq!(error.code(), defect.error_code());
                assert!(error.message().contains("Mandate"));
                assert!(error.message().contains("Ordinary"));
            }
        }
    }
    for defect in VerifierSelectionDefectV1::ALL {
        assert!(!defect.permits_downgrade());
        assert!(
            VerifierMandatorySelectionStateV1::ALL
                .iter()
                .any(|state| state.defect() == Some(defect))
        );
    }
}

#[test]
fn authority_use_is_closed_over_identity_lifecycle_target_and_contract() {
    // Architecture 17 required evidence: authority issue/revision/revocation/
    // expiry/consumption and self-target failures.
    let record = authority();
    let target = target_reference();
    assert!(
        validate_verifier_authority_use(
            &record,
            VERIFIER_MANDATE_ID,
            &target,
            VerifierOperationV1::MarkComplete,
            &contract(),
            1_500
        )
        .is_ok()
    );

    let error = validate_verifier_authority_use(
        &record,
        [0x23; 16],
        &target,
        VerifierOperationV1::MarkComplete,
        &contract(),
        1_500,
    )
    .expect_err("the named verifier Mandate must own the authority");
    assert_eq!(error.code(), "verifier_authority_verifier_mismatch");

    let self_target = VerifierTargetReferenceV1 {
        target_mandate_id: VERIFIER_MANDATE_ID,
        target_revision: TARGET_REVISION,
    };
    let error = validate_verifier_authority_use(
        &record,
        VERIFIER_MANDATE_ID,
        &self_target,
        VerifierOperationV1::MarkComplete,
        &contract(),
        1_500,
    )
    .expect_err("a verifier cannot target itself");
    assert_eq!(error.code(), "verifier_authority_self_target");

    let error = validate_verifier_authority_use(
        &record,
        VERIFIER_MANDATE_ID,
        &target,
        VerifierOperationV1::MarkComplete,
        &contract(),
        2_000_000,
    )
    .expect_err("an expired authority fails closed");
    assert_eq!(error.code(), "verifier_authority_expired");

    let revoked = authority_with_lifecycle(VerifierAuthorityLifecycleV1 {
        revoked_at_ms: Some(1_500),
        revocation_reference: Some([0xEE; 16]),
        ..lifecycle()
    });
    let error = validate_verifier_authority_use(
        &revoked,
        VERIFIER_MANDATE_ID,
        &target,
        VerifierOperationV1::MarkComplete,
        &contract(),
        1_600,
    )
    .expect_err("a revoked authority fails closed");
    assert_eq!(error.code(), "verifier_authority_revoked");

    let consumed = authority_with_lifecycle(VerifierAuthorityLifecycleV1 {
        consumption: VerifierAuthorityConsumptionV1::Consumed {
            mutation_reference: MUTATION_ID,
        },
        ..lifecycle()
    });
    let error = validate_verifier_authority_use(
        &consumed,
        VERIFIER_MANDATE_ID,
        &target,
        VerifierOperationV1::MarkComplete,
        &contract(),
        1_600,
    )
    .expect_err("a consumed authority fails closed");
    assert_eq!(error.code(), "verifier_authority_consumed");

    let narrow = VerifierAuthorityV1::new(
        AUTHORITY_ID,
        5,
        VERIFIER_MANDATE_ID,
        VerifierTargetSetReferenceV1 {
            target_set_id: TARGET_SET_ID,
            target_set_digest: Digest256::sha256(b"target-set-v1"),
        },
        vec![VerifierOperationV1::Stop],
        contract(),
        lifecycle(),
    )
    .expect("narrow authority fixture is valid");
    let error = validate_verifier_authority_use(
        &narrow,
        VERIFIER_MANDATE_ID,
        &target,
        VerifierOperationV1::MarkComplete,
        &contract(),
        1_500,
    )
    .expect_err("an operation outside the allowed set fails closed");
    assert_eq!(error.code(), "verifier_authority_operation_not_allowed");

    let other_contract = VerifierContractReferenceV1 {
        contract_id: [0x45; 16],
        contract_revision: 2,
        contract_digest: Digest256::sha256(b"other-contract"),
    };
    let error = validate_verifier_authority_use(
        &record,
        VERIFIER_MANDATE_ID,
        &target,
        VerifierOperationV1::MarkComplete,
        &other_contract,
        1_500,
    )
    .expect_err("an audit-contract mismatch fails closed");
    assert_eq!(error.code(), "verifier_authority_contract_mismatch");
}

#[test]
fn authority_reference_mismatches_fail_closed() {
    // Architecture 17 required evidence: authority revision/digest rows.
    let record = authority();
    let reference = authority_reference();
    assert!(validate_verifier_authority_reference(&record, &reference).is_ok());

    let cases = [
        (
            "verifier_authority_identity_mismatch",
            VerifierAuthorityReferenceV1 {
                authority_id: [0x12; 16],
                ..reference
            },
        ),
        (
            "verifier_authority_revision_mismatch",
            VerifierAuthorityReferenceV1 {
                authority_revision: reference.authority_revision + 1,
                ..reference
            },
        ),
        (
            "verifier_authority_digest_mismatch",
            VerifierAuthorityReferenceV1 {
                authority_digest: Digest256::sha256(b"other-authority"),
                ..reference
            },
        ),
    ];
    for (code, expected) in cases {
        let error = validate_verifier_authority_reference(&record, &expected)
            .expect_err("mismatched authority reference fails closed");
        assert_eq!(error.code(), code);
    }
}

#[test]
fn authority_lifecycle_rejects_unpaired_revocation_bad_windows_and_reusable_consumption() {
    // Architecture 17 required evidence: issuance/expiry/revocation/
    // consumption negative vectors.
    assert!(lifecycle().encode().is_ok());

    let unpaired = VerifierAuthorityLifecycleV1 {
        revoked_at_ms: Some(1_500),
        revocation_reference: None,
        ..lifecycle()
    };
    assert_eq!(
        unpaired
            .encode()
            .expect_err("unpaired revocation is rejected"),
        CanonicalError::InvalidField
    );

    let reversed_window = VerifierAuthorityLifecycleV1 {
        expires_at_ms: Some(1_000),
        ..lifecycle()
    };
    assert_eq!(
        reversed_window
            .encode()
            .expect_err("an empty expiry window is rejected"),
        CanonicalError::InvalidField
    );

    let early_revocation = VerifierAuthorityLifecycleV1 {
        revoked_at_ms: Some(999),
        revocation_reference: Some([0xEE; 16]),
        ..lifecycle()
    };
    assert_eq!(
        early_revocation
            .encode()
            .expect_err("revocation before issuance is rejected"),
        CanonicalError::InvalidField
    );

    let reusable_consumption = VerifierAuthorityLifecycleV1 {
        consumption_rule: VerifierAuthorityConsumptionRuleV1::ReusableWhileActive,
        consumption: VerifierAuthorityConsumptionV1::Consumed {
            mutation_reference: MUTATION_ID,
        },
        ..lifecycle()
    };
    assert_eq!(
        reusable_consumption
            .encode()
            .expect_err("a reusable authority is never consumed"),
        CanonicalError::InvalidField
    );

    let single_use_consumption = VerifierAuthorityLifecycleV1 {
        consumption: VerifierAuthorityConsumptionV1::Consumed {
            mutation_reference: MUTATION_ID,
        },
        ..lifecycle()
    };
    let encoded = single_use_consumption
        .encode()
        .expect("single-use consumption encodes");
    assert_eq!(
        VerifierAuthorityLifecycleV1::decode(&encoded).expect("lifecycle decodes"),
        single_use_consumption
    );
}

#[test]
fn frozen_reference_bounds_and_duplicate_keys_fail_closed() {
    // Architecture 17 required evidence: bound and explicit-target-set rows.
    let duplicate_goals = VerifierFrozenGoalGateEvidenceReferencesV1 {
        goal_references: vec![
            VerifierGoalReferenceV1 {
                goal_id: GOAL_ID,
                goal_revision: 7,
            },
            VerifierGoalReferenceV1 {
                goal_id: GOAL_ID,
                goal_revision: 8,
            },
        ],
        gate_evidence_contract_references: vec![contract()],
    };
    let error = baseline_with_frozen_references(duplicate_goals)
        .expect_err("duplicate frozen Goal keys fail closed");
    assert_eq!(error.code(), "verifier_baseline_invalid");

    let duplicate_contracts = VerifierFrozenGoalGateEvidenceReferencesV1 {
        goal_references: vec![VerifierGoalReferenceV1 {
            goal_id: GOAL_ID,
            goal_revision: 7,
        }],
        gate_evidence_contract_references: vec![contract(), contract()],
    };
    let error = baseline_with_frozen_references(duplicate_contracts)
        .expect_err("duplicate frozen contract keys fail closed");
    assert_eq!(error.code(), "verifier_baseline_invalid");

    let over_limit = VerifierFrozenGoalGateEvidenceReferencesV1 {
        goal_references: (0..=32u8)
            .map(|byte| VerifierGoalReferenceV1 {
                goal_id: [byte; 16],
                goal_revision: 1,
            })
            .collect(),
        gate_evidence_contract_references: vec![],
    };
    let error = baseline_with_frozen_references(over_limit)
        .expect_err("an over-limit frozen Goal list fails closed");
    assert_eq!(error.code(), "verifier_baseline_invalid");

    let mut tampered = baseline();
    tampered.target_revision_and_sequence = VerifierTargetRevisionAndSequenceV1 {
        revision: 0,
        aggregate_sequence: TARGET_SEQUENCE,
    };
    assert_eq!(
        tampered
            .encode()
            .expect_err("zero target revision is rejected"),
        CanonicalError::InvalidField
    );
}

#[test]
fn goal_facing_projections_expose_only_safe_identity_revision_and_digest_references() {
    // Architecture 28 required evidence: Goal-facing verification surface.
    let record = authority();
    let projection = record.project();
    assert_eq!(projection.authority_id, AUTHORITY_ID);
    assert_eq!(projection.verifier_mandate_id, VERIFIER_MANDATE_ID);
    assert_eq!(projection.authority_revision, 5);
    assert_eq!(
        projection.target_set_reference,
        record.immutable_target_set_reference
    );
    assert_eq!(projection.allowed_operations, record.allowed_operations);
    assert_eq!(projection.audit_contract_reference, contract());
    assert_eq!(
        projection.canonical_authority_digest,
        record.canonical_digest()
    );

    let evidence = evidence();
    let projection: VerificationAuditEvidenceDto = evidence.project();
    assert_eq!(projection.evidence_id, EVIDENCE_ID);
    assert_eq!(
        projection.evidence_kind,
        VerifierEvidenceKindV1::UnconditionalPass
    );
    assert_eq!(
        projection.frozen_goal_references,
        evidence.frozen_references.goal_references
    );
    assert_eq!(
        projection.frozen_gate_evidence_contract_references,
        evidence.frozen_references.gate_evidence_contract_references
    );
    assert_eq!(
        projection.canonical_evidence_digest,
        evidence.canonical_digest()
    );

    let verdict = verdict_record();
    let projection = verdict.project();
    assert_eq!(projection.verdict_id, VERDICT_ID);
    assert_eq!(projection.verdict, VerificationAuditVerdictDto::Pass);
    assert_eq!(projection.evidence_references, vec![EVIDENCE_ID]);
    assert_eq!(
        projection.canonical_verdict_digest,
        verdict.canonical_digest()
    );

    let reconciliation = reconciliation();
    let projection = reconciliation.project();
    assert_eq!(projection.reconciliation_id, RECONCILIATION_ID);
    assert_eq!(
        projection.outcome,
        VerifierReconciliationOutcomeV1::ActiveForLaterFreshWork
    );
    assert_eq!(
        projection.target_unknown_effect_reference,
        UNKNOWN_EFFECT_REFERENCE
    );
    assert_eq!(
        projection.canonical_reconciliation_digest,
        reconciliation.canonical_digest()
    );
}

#[test]
fn user_reconciliation_uses_the_same_record_family_without_verifier_authority() {
    // Architecture 28 required evidence: user reconciliation record family.
    let user_record = VerifierReconciliationV1::new(
        RECONCILIATION_ID,
        None,
        target_reference(),
        baseline().canonical_digest(),
        UNKNOWN_EFFECT_REFERENCE,
        VerifierReconciliationOutcomeV1::Stopped,
    )
    .expect("user reconciliation fixture is valid");
    assert_eq!(user_record.authority_reference, None);
    let encoded = user_record.encode().expect("user reconciliation encodes");
    let decoded = VerifierReconciliationV1::decode(&encoded).expect("user reconciliation decodes");
    assert_eq!(decoded, user_record);
    assert_eq!(decoded.project().authority_reference, None);
}

#[test]
fn evidence_kinds_map_only_to_their_own_verifier_operations() {
    // Architecture 17 required evidence: operation/evidence matrix.
    assert!(VerifierEvidenceKindV1::UnconditionalPass.supports(VerifierOperationV1::MarkComplete));
    assert!(
        VerifierEvidenceKindV1::GraphTerminalizationClosure
            .supports(VerifierOperationV1::MarkComplete)
    );
    assert!(VerifierEvidenceKindV1::QualifyingFail.supports(VerifierOperationV1::MarkNeedsRework));
    assert!(
        VerifierEvidenceKindV1::ReconciliationStandardProof
            .supports(VerifierOperationV1::ResolveUnknownEffect)
    );
    assert!(!VerifierEvidenceKindV1::Inconclusive.supports(VerifierOperationV1::MarkComplete));
    assert!(!VerifierEvidenceKindV1::UnconditionalPass.supports(VerifierOperationV1::Stop));
    assert_eq!(VerifierEvidenceKindV1::ALL.len(), 5);
    assert_eq!(VerificationAuditVerdictDto::ALL.len(), 7);
    assert_eq!(VerifierTargetLifecycleV1::ALL.len(), 9);
    assert_eq!(VerifierPreconditionFailureV1::ALL.len(), 9);
    assert_eq!(VerifierBaselineElementV1::ALL.len(), 11);
}
