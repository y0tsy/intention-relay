#![allow(
    clippy::expect_used,
    reason = "Slice 3 Goal-domain contract fixtures use expect for precise test diagnostics."
)]

//! Slice 3 Goal-domain contract tests.
//!
//! Owner: architecture 28 with ADR 0044. Every test states the architecture 28
//! required-evidence row it satisfies.

use intention_domain::canonical::Digest256;
use intention_domain::goal_domain::*;

const fn project_scope() -> GoalScopeDto {
    GoalScopeDto::Project {
        project_id: [1; 16],
    }
}

const fn session_scope(session: u8) -> GoalScopeDto {
    GoalScopeDto::Session {
        project_id: [1; 16],
        session_id: [session; 16],
    }
}

const fn evidence(id: u8) -> GoalEvidenceReferenceV1 {
    GoalEvidenceReferenceV1 {
        evidence_id: [id; 16],
        revision: 1,
        kind: GoalEvidenceKindV1::TerminalChildResult,
    }
}

const fn gate_exception(gate: u8, kind: GoalGateExceptionKindV1) -> GoalGateExceptionV1 {
    GoalGateExceptionV1 {
        gate_id: [gate; 16],
        gate_revision: 1,
        kind,
        evidence: evidence(gate),
    }
}

fn goal(lifecycle_state: GoalLifecycleStateDto) -> GoalDto {
    GoalDto::new(
        [4; 16],
        project_scope(),
        2,
        lifecycle_state,
        GoalReadinessStateDto::NotReady,
        GoalUserDecisionStateDto::Unaccepted,
    )
    .expect("goal fixture is coherent")
}

fn session_link() -> GoalSessionLinkDto {
    GoalSessionLinkDto {
        link_id: [3; 16],
        project_goal_id: [4; 16],
        session_id: [2; 16],
        effective_from_revision: 1,
        canonical_link_digest: Digest256::sha256(b"session-link"),
    }
}

fn selection() -> GoalRunSelectionV1 {
    GoalRunSelectionV1 {
        leading_goal_id: [4; 16],
        goal_revision: 2,
        scope_link_provenance: GoalScopeLinkProvenanceV1 {
            project_id: [1; 16],
            session_id: Some([2; 16]),
            link_id: Some([3; 16]),
        },
        parent_revision_chain: vec![GoalRevisionReferenceV1 {
            goal_id: [9; 16],
            revision: 1,
        }],
        obligatory_component_references: vec![[11; 16]],
        selected_gate_revisions: vec![GoalGateRevisionReferenceV1 {
            gate_reference: [5; 16],
            revision: 1,
        }],
        valid_evidence_references: vec![[6; 16]],
        selected_memory_cards: vec![GoalCardReferenceV1 {
            card_reference: [12; 16],
            revision: 1,
        }],
        selected_skill_cards: Vec::new(),
        selected_role_cards: Vec::new(),
        revealed_full_record_references: vec![[13; 16]],
        policy_snapshot_reference: [14; 16],
        activity_selection_reference: [15; 16],
        run_kind: GoalRunKindV1::GoalDirectedOrdinary,
        target_snapshot_digest: Digest256::sha256(b"target"),
        bounds: GoalRunSelectionBoundsV1 {
            max_memory_cards: GOAL_MAX_ACTIVE_MEMORY_CARDS,
            max_skill_role_cards: GOAL_MAX_SELECTED_SKILL_ROLE_CARDS,
            target_snapshot_bytes: GOAL_MAX_TARGET_SNAPSHOT_BYTES,
            context_bytes: GOAL_MAX_CONTEXT_BYTES,
        },
    }
}

fn applicability_context() -> GoalApplicabilityContextV1 {
    GoalApplicabilityContextV1 {
        project_id: [1; 16],
        session_id: [2; 16],
        selected_goal_ids: vec![[4; 16]],
    }
}

fn memory_card() -> GoalMemoryCardV1 {
    GoalMemoryCardV1 {
        record_id: [40; 16],
        revision: 1,
        kind: MemoryKindDto::Decision,
        scope: GoalRecordScopeDto::Goal { goal_id: [4; 16] },
        title: "Selected approach".to_owned(),
        safe_purpose: "Keeps the accepted approach visible".to_owned(),
        retained_content_reference: [41; 16],
        canonical_digest: Digest256::sha256(b"memory-card"),
    }
}

fn skill_card() -> GoalSkillCardV1 {
    GoalSkillCardV1 {
        skill_id: [50; 16],
        revision: 1,
        canonical_name: "goal-audit".to_owned(),
        description: "Audit the selected Goal".to_owned(),
        owner_scope: GoalRecordScopeDto::Project {
            project_id: [1; 16],
        },
        content_reference: [51; 16],
        canonical_digest: Digest256::sha256(b"skill"),
    }
}

fn role_card() -> GoalRoleCardV1 {
    GoalRoleCardV1 {
        role_id: [60; 16],
        revision: 1,
        canonical_name: "auditor".to_owned(),
        task: "Audit one Goal".to_owned(),
        permitted_class: GoalRoleClassV1::Medium,
        tool_subset: vec!["read".to_owned(), "grep".to_owned()],
        context_limit_bytes: 4_096,
        result_limit_bytes: 2_048,
    }
}

fn template_card() -> GoalGateTemplateCardV1 {
    GoalGateTemplateCardV1 {
        template_id: [21; 16],
        revision: 2,
        scope: GoalTemplateScopeDto::Project {
            project_id: [1; 16],
        },
        capability_reference: [22; 16],
        input_family: GoalGateInputFamilyV1::ClosedTextV1,
        requires_confirmation: true,
        canonical_digest: Digest256::sha256(b"template"),
    }
}

fn refinement_draft() -> RefinementDraftDto {
    RefinementDraftDto {
        draft_id: [80; 16],
        source_run_id: [81; 16],
        leading_goal_id: [4; 16],
        milestone: GoalMilestoneDto::TechnicalReadiness,
        base_goal_revision: 2,
        base_record_reference: [82; 16],
        base_record_revision: 1,
        edits: vec![RefinementEditV1 {
            kind: RefinementEditKindV1::ReadinessClaim,
            evidence: evidence(1),
        }],
        evidence_references: vec![evidence(1)],
        safe_rationale: "Evidence supports readiness".to_owned(),
        canonical_digest: Digest256::sha256(b"draft"),
    }
}

fn summary() -> ConversationSummaryDto {
    ConversationSummaryDto {
        summary_id: [90; 16],
        revision: 1,
        previous_summary_reference: None,
        source_range_start: [91; 16],
        source_range_end: [92; 16],
        safe_content: "Completed range summarized".to_owned(),
        canonical_digest: Digest256::sha256(b"summary"),
    }
}

#[test]
fn goal_identity_scope_and_tree_link_rules() {
    // Required-evidence row: Goal identity/scope/tree, obligatory children, DAG
    // integrity, and no-cross-project fixtures.
    let project = project_scope();
    let session = session_scope(2);
    assert_eq!(project.project_id(), [1; 16]);
    assert_eq!(project.session_id(), None);
    assert!(project.is_project());
    assert_eq!(session.session_id(), Some([2; 16]));
    assert!(!session.is_project());

    let parent_link = GoalParentLinkDto {
        parent_goal_id: [10; 16],
        child_goal_id: [11; 16],
        child_revision_at_link: 1,
        canonical_link_digest: Digest256::sha256(b"parent-link"),
    };
    // A project Goal owns a project child inside the same project.
    assert_eq!(
        validate_goal_child_link(
            &parent_link,
            project,
            GoalScopeDto::Project {
                project_id: [1; 16]
            },
            false,
            false
        )
        .expect("project child is allowed"),
        GoalChildLinkDispositionV1::ExistingSessionLink
    );
    // A project Goal owns a session child when the owner session is linked ...
    assert_eq!(
        validate_goal_child_link(&parent_link, project, session, true, false)
            .expect("linked session child"),
        GoalChildLinkDispositionV1::ExistingSessionLink
    );
    // ... and may create the link atomically when the caller allows it ...
    assert_eq!(
        validate_goal_child_link(&parent_link, project, session, false, true)
            .expect("atomic link creation"),
        GoalChildLinkDispositionV1::CreateSessionLinkAtomically
    );
    // ... but an out-of-link session child otherwise fails closed.
    assert_eq!(
        validate_goal_child_link(&parent_link, project, session, false, false)
            .expect_err("out-of-link session child")
            .code(),
        GOAL_CYCLE_DETECTED
    );
    // A session Goal cannot own a project child, another session's child, or
    // cross a project.
    assert_eq!(
        validate_goal_child_link(&parent_link, session, project, true, true)
            .expect_err("session parent cannot own a project child")
            .code(),
        GOAL_CYCLE_DETECTED
    );
    assert_eq!(
        validate_goal_child_link(&parent_link, session, session_scope(3), true, true)
            .expect_err("session child must stay in its session")
            .code(),
        GOAL_CYCLE_DETECTED
    );
    assert_eq!(
        validate_goal_child_link(
            &parent_link,
            project,
            GoalScopeDto::Project {
                project_id: [9; 16]
            },
            false,
            true
        )
        .expect_err("cross-project child")
        .code(),
        GOAL_CYCLE_DETECTED
    );
    // A self-link and an unset child revision fail before a partial record.
    let self_link = GoalParentLinkDto {
        child_goal_id: [10; 16],
        ..parent_link
    };
    assert_eq!(
        validate_goal_child_link(&self_link, project, project, false, false)
            .expect_err("self link")
            .code(),
        GOAL_CYCLE_DETECTED
    );
    let zero_revision = GoalParentLinkDto {
        child_revision_at_link: 0,
        ..parent_link
    };
    assert_eq!(
        validate_goal_child_link(&zero_revision, project, project, false, false)
            .expect_err("zero child revision")
            .code(),
        GOAL_REVISION_CONFLICT
    );

    // Cycles, duplicate direct children, depth, and child bounds.
    assert!(validate_goal_no_cycle([11; 16], &[[10; 16]]).is_ok());
    assert_eq!(
        validate_goal_no_cycle([10; 16], &[[10; 16]])
            .expect_err("cycle")
            .code(),
        GOAL_CYCLE_DETECTED
    );
    assert_eq!(
        validate_goal_no_cycle([0; 16], &[])
            .expect_err("identity")
            .code(),
        GOAL_NOT_ACTIVE
    );
    let duplicate_children = vec![
        parent_link,
        GoalParentLinkDto {
            child_goal_id: [11; 16],
            ..parent_link
        },
    ];
    assert_eq!(
        validate_goal_direct_children(&duplicate_children)
            .expect_err("duplicate direct child")
            .code(),
        GOAL_CYCLE_DETECTED
    );
    let too_many_children: Vec<GoalParentLinkDto> = (1..=33u8)
        .map(|index| GoalParentLinkDto {
            child_goal_id: [index; 16],
            ..parent_link
        })
        .collect();
    assert_eq!(
        validate_goal_direct_children(&too_many_children)
            .expect_err("direct child limit")
            .code(),
        GOAL_CHILD_LIMIT_EXCEEDED
    );

    assert_eq!(validate_goal_tree_attachment(1, 0).expect("root child"), 2);
    assert_eq!(
        validate_goal_tree_attachment(GOAL_MAX_TREE_DEPTH, 0)
            .expect_err("depth limit")
            .code(),
        GOAL_TREE_DEPTH_LIMIT_EXCEEDED
    );
    assert_eq!(
        validate_goal_tree_attachment(0, 0)
            .expect_err("no parent depth")
            .code(),
        GOAL_TREE_DEPTH_LIMIT_EXCEEDED
    );
    assert_eq!(
        validate_goal_tree_attachment(1, GOAL_MAX_DIRECT_CHILDREN)
            .expect_err("child limit")
            .code(),
        GOAL_CHILD_LIMIT_EXCEEDED
    );

    assert!(validate_goal_allocation(256, 64).is_ok());
    assert_eq!(
        validate_goal_allocation(257, 0)
            .expect_err("project goal limit")
            .code(),
        GOAL_LIMIT_EXCEEDED
    );
    assert_eq!(
        validate_goal_allocation(0, 65)
            .expect_err("session goal limit")
            .code(),
        GOAL_LIMIT_EXCEEDED
    );

    let links: Vec<GoalSessionLinkDto> = (1..=64u8)
        .map(|session_id| GoalSessionLinkDto {
            session_id: [session_id; 16],
            ..session_link()
        })
        .collect();
    assert!(validate_goal_session_links(&links, [4; 16]).is_ok());
    let too_many_links: Vec<GoalSessionLinkDto> = (1..=65u8)
        .map(|session_id| GoalSessionLinkDto {
            session_id: [session_id; 16],
            ..session_link()
        })
        .collect();
    assert_eq!(
        validate_goal_session_links(&too_many_links, [4; 16])
            .expect_err("session link limit")
            .code(),
        GOAL_SESSION_LINK_LIMIT_EXCEEDED
    );
    assert_eq!(
        validate_goal_session_links(&[session_link(), session_link()], [4; 16])
            .expect_err("duplicate session link")
            .code(),
        GOAL_REVISION_CONFLICT
    );
    assert_eq!(
        validate_goal_session_links(&links, [99; 16])
            .expect_err("foreign project goal")
            .code(),
        GOAL_NOT_ACTIVE
    );
}

#[test]
fn goal_lifecycle_readiness_and_archive_state_machines() {
    // Required-evidence row: lifecycle/readiness/user-decision state machines,
    // exception evidence, and pause/stop/archive fixtures.
    use GoalLifecycleStateDto::{Active, Archived, NeedsRework, Paused, Stopped};
    for (from, to) in [
        (Active, NeedsRework),
        (Active, Paused),
        (Active, Stopped),
        (NeedsRework, Active),
        (NeedsRework, Paused),
        (NeedsRework, Stopped),
        (Paused, Active),
        (Paused, Stopped),
        (Archived, Active),
        (Archived, Stopped),
    ] {
        assert!(
            validate_goal_lifecycle_transition(from, to).is_ok(),
            "edge {from:?} -> {to:?} must be allowed"
        );
    }
    for (from, to) in [
        (Active, Active),
        (Stopped, Active),
        (Stopped, NeedsRework),
        (Active, Archived),
        (Archived, Archived),
        (Archived, NeedsRework),
    ] {
        assert_eq!(
            validate_goal_lifecycle_transition(from, to)
                .expect_err("invalid lifecycle edge")
                .code(),
            GOAL_NOT_ACTIVE
        );
    }

    assert!(!GoalReadinessStateDto::NotReady.is_ready());
    assert_eq!(
        GoalReadinessStateDto::ready(Vec::new())
            .expect_err("ready needs evidence")
            .code(),
        GOAL_NOT_READY
    );
    let ready = validate_goal_readiness_claim(vec![evidence(1)], &[], false)
        .expect("readiness claim succeeds");
    assert!(ready.is_ready());
    assert_eq!(ready.verified_evidence_set().len(), 1);
    assert_eq!(
        validate_goal_readiness_claim(vec![evidence(1)], &[[7; 16]], false)
            .expect_err("unresolved obligatory child")
            .code(),
        GOAL_NOT_READY
    );
    assert_eq!(
        validate_goal_readiness_claim(vec![evidence(1)], &[], true)
            .expect_err("missing required gate evidence")
            .code(),
        GOAL_NOT_READY
    );
    assert_eq!(
        GoalReadinessStateDto::ready(vec![evidence(1), evidence(1)])
            .expect_err("duplicate evidence identity")
            .code(),
        GOAL_REVISION_CONFLICT
    );

    // A required-gate failure moves the Goal to NeedsRework; a pass never
    // repairs rework by itself.
    assert_eq!(
        apply_required_gate_outcome(Active, GoalGateOutcomeDispositionV1::Failed)
            .expect("gate failure moves to rework"),
        NeedsRework
    );
    assert_eq!(
        apply_required_gate_outcome(NeedsRework, GoalGateOutcomeDispositionV1::Passed)
            .expect("a pass never repairs rework by itself"),
        NeedsRework
    );
    assert_eq!(
        apply_required_gate_outcome(Paused, GoalGateOutcomeDispositionV1::Failed)
            .expect_err("paused goal rejects gate evaluation")
            .code(),
        GOAL_NOT_ACTIVE
    );

    // State coherence is validated when a Goal record is created.
    assert_eq!(
        GoalDto::new(
            [4; 16],
            project_scope(),
            0,
            Active,
            GoalReadinessStateDto::NotReady,
            GoalUserDecisionStateDto::Unaccepted,
        )
        .expect_err("zero active revision")
        .code(),
        GOAL_REVISION_CONFLICT
    );
    assert_eq!(
        GoalDto::new(
            [4; 16],
            project_scope(),
            1,
            NeedsRework,
            GoalReadinessStateDto::Ready {
                verified_evidence_set: vec![evidence(1)],
            },
            GoalUserDecisionStateDto::Unaccepted,
        )
        .expect_err("rework cannot claim readiness")
        .code(),
        GOAL_NOT_READY
    );
    assert_eq!(
        GoalDto::new(
            [4; 16],
            project_scope(),
            1,
            Active,
            GoalReadinessStateDto::NotReady,
            GoalUserDecisionStateDto::Accepted,
        )
        .expect_err("acceptance requires readiness")
        .code(),
        GOAL_NOT_READY
    );
    assert_eq!(
        GoalDto::new(
            [4; 16],
            project_scope(),
            1,
            Active,
            GoalReadinessStateDto::Ready {
                verified_evidence_set: vec![evidence(1)],
            },
            GoalUserDecisionStateDto::accepted_with_exception(vec![gate_exception(
                5,
                GoalGateExceptionKindV1::Failed
            )])
            .expect("exception fixture"),
        )
        .expect_err("an exception never creates Ready")
        .code(),
        GOAL_ACCEPTANCE_EXCEPTION_INVALID
    );

    // Pause, stop, archive, and restore.
    assert!(
        validate_goal_archive(Stopped, &GoalUserDecisionStateDto::Unaccepted).is_ok(),
        "a stopped Goal archives"
    );
    assert!(
        validate_goal_archive(Active, &GoalUserDecisionStateDto::Accepted).is_ok(),
        "an accepted Goal archives"
    );
    assert_eq!(
        validate_goal_archive(Active, &GoalUserDecisionStateDto::Unaccepted)
            .expect_err("an open Goal cannot archive")
            .code(),
        GOAL_ARCHIVE_NOT_TERMINAL
    );
    assert_eq!(
        validate_goal_archive(Archived, &GoalUserDecisionStateDto::Accepted)
            .expect_err("an archived Goal cannot re-archive")
            .code(),
        GOAL_ARCHIVE_NOT_TERMINAL
    );
    assert!(validate_goal_restore(Archived).is_ok());
    assert_eq!(
        validate_goal_restore(Stopped)
            .expect_err("only an archived Goal restores")
            .code(),
        GOAL_ARCHIVE_NOT_TERMINAL
    );
}

#[test]
fn goal_user_decision_exception_evidence_rules() {
    // Required-evidence row: lifecycle/readiness/user-decision state machines,
    // exception evidence, and pause/stop/archive fixtures.
    let exception = gate_exception(5, GoalGateExceptionKindV1::Unavailable);
    let inherited = GoalInheritedExceptionV1 {
        child_goal_id: [7; 16],
        exception,
    };
    let request = GoalAcceptanceRequestV1 {
        decision: GoalAcceptanceDecisionV1::AcceptWithException {
            exception_evidence_set: vec![exception],
        },
        readiness_state: GoalReadinessStateDto::NotReady,
        inherited_child_exceptions: vec![inherited],
        unresolved_obligatory_children: Vec::new(),
    };
    assert_eq!(
        validate_goal_acceptance(&request).expect("carried child exception is accepted"),
        GoalUserDecisionStateDto::AcceptedWithException {
            exception_evidence_set: vec![exception],
        }
    );

    // The parent exception set must explicitly include each inherited child
    // exception.
    let missing_inherited = GoalAcceptanceRequestV1 {
        decision: GoalAcceptanceDecisionV1::AcceptWithException {
            exception_evidence_set: vec![gate_exception(6, GoalGateExceptionKindV1::Failed)],
        },
        ..request.clone()
    };
    assert_eq!(
        validate_goal_acceptance(&missing_inherited)
            .expect_err("inherited exception omitted")
            .code(),
        GOAL_ACCEPTANCE_EXCEPTION_INVALID
    );
    // An exception cannot omit, cancel, or bypass an active obligatory child.
    let unresolved_child = GoalAcceptanceRequestV1 {
        unresolved_obligatory_children: vec![[8; 16]],
        ..request
    };
    assert_eq!(
        validate_goal_acceptance(&unresolved_child)
            .expect_err("active obligatory child bypassed")
            .code(),
        GOAL_ACCEPTANCE_EXCEPTION_INVALID
    );
    // The exception set is non-empty and names each gate once.
    assert_eq!(
        GoalUserDecisionStateDto::accepted_with_exception(Vec::new())
            .expect_err("empty exception set")
            .code(),
        GOAL_ACCEPTANCE_EXCEPTION_INVALID
    );
    assert_eq!(
        GoalUserDecisionStateDto::accepted_with_exception(vec![exception, exception])
            .expect_err("duplicate gate exception")
            .code(),
        GOAL_ACCEPTANCE_EXCEPTION_INVALID
    );
    assert_eq!(
        GoalUserDecisionStateDto::accepted_with_exception(vec![GoalGateExceptionV1 {
            gate_revision: 0,
            ..exception
        }])
        .expect_err("inexact gate exception")
        .code(),
        GOAL_ACCEPTANCE_EXCEPTION_INVALID
    );

    // Plain acceptance requires a ready Goal with no inherited exception.
    let accept = GoalAcceptanceRequestV1 {
        decision: GoalAcceptanceDecisionV1::Accept,
        readiness_state: GoalReadinessStateDto::Ready {
            verified_evidence_set: vec![evidence(1)],
        },
        inherited_child_exceptions: Vec::new(),
        unresolved_obligatory_children: Vec::new(),
    };
    assert_eq!(
        validate_goal_acceptance(&accept).expect("ready acceptance"),
        GoalUserDecisionStateDto::Accepted
    );
    let accept_not_ready = GoalAcceptanceRequestV1 {
        readiness_state: GoalReadinessStateDto::NotReady,
        ..accept.clone()
    };
    assert_eq!(
        validate_goal_acceptance(&accept_not_ready)
            .expect_err("acceptance requires readiness")
            .code(),
        GOAL_NOT_READY
    );
    let accept_with_inherited = GoalAcceptanceRequestV1 {
        inherited_child_exceptions: vec![inherited],
        ..accept
    };
    assert_eq!(
        validate_goal_acceptance(&accept_with_inherited)
            .expect_err("ready acceptance cannot carry a child exception")
            .code(),
        GOAL_ACCEPTANCE_EXCEPTION_INVALID
    );
    assert!(GoalUserDecisionStateDto::Accepted.is_terminal());
    assert!(
        GoalUserDecisionStateDto::AcceptedWithException {
            exception_evidence_set: vec![exception],
        }
        .is_terminal()
    );
    assert!(!GoalUserDecisionStateDto::Unaccepted.is_terminal());
}

#[test]
fn goal_revision_records_are_typed_and_bounded() {
    // Required-evidence row: Goal identity/scope/tree, obligatory children, DAG
    // integrity, and no-cross-project fixtures.
    let revision = GoalRevisionDto {
        goal_id: [4; 16],
        revision: 2,
        title: "Deliver harness goal".to_owned(),
        objective: "Reach accepted evidence".to_owned(),
        inherited_rule_references: vec![[20; 16]],
        local_rule_references: vec![[21; 16]],
        required_gate_references: vec![GoalGateRevisionReferenceV1 {
            gate_reference: [5; 16],
            revision: 1,
        }],
        canonical_revision_digest: Digest256::sha256(b"goal-revision"),
    };
    assert!(revision.validate().is_ok());
    assert_eq!(
        GoalRevisionDto {
            revision: 0,
            ..revision.clone()
        }
        .validate()
        .expect_err("zero revision")
        .code(),
        GOAL_REVISION_CONFLICT
    );
    assert_eq!(
        GoalRevisionDto {
            title: "  ".to_owned(),
            ..revision.clone()
        }
        .validate()
        .expect_err("blank title")
        .code(),
        GOAL_REVISION_CONFLICT
    );
    assert_eq!(
        GoalRevisionDto {
            goal_id: [0; 16],
            ..revision.clone()
        }
        .validate()
        .expect_err("missing goal identity")
        .code(),
        GOAL_NOT_ACTIVE
    );
    assert_eq!(
        GoalRevisionDto {
            required_gate_references: vec![GoalGateRevisionReferenceV1 {
                gate_reference: [5; 16],
                revision: 0,
            }],
            ..revision.clone()
        }
        .validate()
        .expect_err("zero gate revision")
        .code(),
        GOAL_REVISION_CONFLICT
    );
    assert_eq!(
        GoalRevisionDto {
            required_gate_references: vec![
                GoalGateRevisionReferenceV1 {
                    gate_reference: [5; 16],
                    revision: 1,
                },
                GoalGateRevisionReferenceV1 {
                    gate_reference: [5; 16],
                    revision: 2,
                },
            ],
            ..revision.clone()
        }
        .validate()
        .expect_err("duplicate required gate")
        .code(),
        GOAL_REVISION_CONFLICT
    );
    assert_eq!(
        GoalRevisionDto {
            required_gate_references: vec![
                GoalGateRevisionReferenceV1 {
                    gate_reference: [5; 16],
                    revision: 1,
                };
                33
            ],
            ..revision
        }
        .validate()
        .expect_err("required gate limit")
        .code(),
        GOAL_GATE_LIMIT_EXCEEDED
    );
}

#[test]
fn leading_goal_run_selection_admission_fault_injection() {
    // Required-evidence row: leading-goal run-selection admission fault
    // injection and no-current-state reconstruction fixtures.
    let active_goal = goal(GoalLifecycleStateDto::Active);
    assert!(
        validate_goal_run_admission(&active_goal, &[session_link()], 4_096, &selection()).is_ok()
    );

    // A frozen stale revision is a typed conflict, never repaired from current
    // state.
    let stale = GoalRunSelectionV1 {
        goal_revision: 3,
        ..selection()
    };
    assert_eq!(
        validate_goal_run_admission(&active_goal, &[session_link()], 4_096, &stale)
            .expect_err("stale frozen revision")
            .code(),
        GOAL_REVISION_CONFLICT
    );
    // A non-Active Goal admits no run.
    let paused = goal(GoalLifecycleStateDto::Paused);
    assert_eq!(
        validate_goal_run_admission(&paused, &[session_link()], 4_096, &selection())
            .expect_err("paused Goal")
            .code(),
        GOAL_NOT_ACTIVE
    );
    // A project Goal enters a session only through the explicit durable link.
    assert_eq!(
        validate_goal_run_admission(&active_goal, &[], 4_096, &selection())
            .expect_err("missing session link")
            .code(),
        GOAL_NOT_ACTIVE
    );
    let foreign_link = GoalSessionLinkDto {
        link_id: [77; 16],
        ..session_link()
    };
    assert_eq!(
        validate_goal_run_admission(&active_goal, &[foreign_link], 4_096, &selection())
            .expect_err("mismatched link identity")
            .code(),
        GOAL_NOT_ACTIVE
    );
    // A session Goal admits only its own session without a project link.
    let session_goal = GoalDto::new(
        [4; 16],
        session_scope(2),
        2,
        GoalLifecycleStateDto::Active,
        GoalReadinessStateDto::NotReady,
        GoalUserDecisionStateDto::Unaccepted,
    )
    .expect("session goal fixture");
    let session_selection = GoalRunSelectionV1 {
        scope_link_provenance: GoalScopeLinkProvenanceV1 {
            project_id: [1; 16],
            session_id: Some([2; 16]),
            link_id: None,
        },
        ..selection()
    };
    assert!(validate_goal_run_admission(&session_goal, &[], 4_096, &session_selection).is_ok());
    let linked_session_selection = GoalRunSelectionV1 {
        scope_link_provenance: GoalScopeLinkProvenanceV1 {
            project_id: [1; 16],
            session_id: Some([3; 16]),
            link_id: None,
        },
        ..selection()
    };
    assert_eq!(
        validate_goal_run_admission(&session_goal, &[], 4_096, &linked_session_selection)
            .expect_err("foreign session Goal selection")
            .code(),
        GOAL_NOT_ACTIVE
    );

    // The target snapshot is either exact and bounded or refused.
    assert_eq!(
        validate_goal_run_admission(&active_goal, &[session_link()], 0, &selection())
            .expect_err("missing snapshot")
            .code(),
        GOAL_SNAPSHOT_UNAVAILABLE
    );
    assert_eq!(
        validate_goal_run_admission(
            &active_goal,
            &[session_link()],
            GOAL_MAX_TARGET_SNAPSHOT_BYTES + 1,
            &selection()
        )
        .expect_err("oversized snapshot")
        .code(),
        GOAL_SNAPSHOT_TOO_LARGE
    );

    // Selection lists are bound and DAG-integral before any write.
    let over_components = GoalRunSelectionV1 {
        obligatory_component_references: (1..=33u8).map(|index| [index; 16]).collect(),
        ..selection()
    };
    assert_eq!(
        validate_goal_run_admission(&active_goal, &[session_link()], 4_096, &over_components)
            .expect_err("component bound")
            .code(),
        GOAL_LIMIT_EXCEEDED
    );
    let over_gates = GoalRunSelectionV1 {
        selected_gate_revisions: (1..=33u8)
            .map(|index| GoalGateRevisionReferenceV1 {
                gate_reference: [index; 16],
                revision: 1,
            })
            .collect(),
        ..selection()
    };
    assert_eq!(
        validate_goal_run_admission(&active_goal, &[session_link()], 4_096, &over_gates)
            .expect_err("gate bound")
            .code(),
        GOAL_GATE_LIMIT_EXCEEDED
    );
    let over_memory = GoalRunSelectionV1 {
        selected_memory_cards: (1..=129u8)
            .map(|index| GoalCardReferenceV1 {
                card_reference: [index; 16],
                revision: 1,
            })
            .collect(),
        ..selection()
    };
    assert_eq!(
        validate_goal_run_admission(&active_goal, &[session_link()], 4_096, &over_memory)
            .expect_err("memory card bound")
            .code(),
        MEMORY_ENTRY_LIMIT_EXCEEDED
    );
    let duplicate_component = GoalRunSelectionV1 {
        obligatory_component_references: vec![[11; 16], [11; 16]],
        ..selection()
    };
    assert_eq!(
        validate_goal_run_admission(&active_goal, &[session_link()], 4_096, &duplicate_component)
            .expect_err("duplicate component")
            .code(),
        GOAL_CYCLE_DETECTED
    );
    let self_chain = GoalRunSelectionV1 {
        parent_revision_chain: vec![GoalRevisionReferenceV1 {
            goal_id: [4; 16],
            revision: 1,
        }],
        ..selection()
    };
    assert_eq!(
        validate_goal_run_admission(&active_goal, &[session_link()], 4_096, &self_chain)
            .expect_err("leading Goal inside its own parent chain")
            .code(),
        GOAL_CYCLE_DETECTED
    );
    let duplicate_gate = GoalRunSelectionV1 {
        selected_gate_revisions: vec![
            GoalGateRevisionReferenceV1 {
                gate_reference: [5; 16],
                revision: 1,
            },
            GoalGateRevisionReferenceV1 {
                gate_reference: [5; 16],
                revision: 1,
            },
        ],
        ..selection()
    };
    assert_eq!(
        validate_goal_run_admission(&active_goal, &[session_link()], 4_096, &duplicate_gate)
            .expect_err("duplicate selected gate revision")
            .code(),
        GOAL_REVISION_CONFLICT
    );
    let deep_chain = GoalRunSelectionV1 {
        parent_revision_chain: (1..=16u8)
            .map(|index| GoalRevisionReferenceV1 {
                goal_id: [index; 16],
                revision: 1,
            })
            .collect(),
        ..selection()
    };
    assert_eq!(
        validate_goal_run_admission(&active_goal, &[session_link()], 4_096, &deep_chain)
            .expect_err("selection depth bound")
            .code(),
        GOAL_TREE_DEPTH_LIMIT_EXCEEDED
    );
}

#[test]
fn reference_and_executable_gates_with_typed_templates() {
    // Required-evidence row: reference/executable gate fixtures with no hidden
    // retry and no raw shell/URL/JSON.
    let reference = VerificationGateDto::ReferenceGate {
        evidence_contract_revision: 1,
        accepted_reference_kinds: vec![GoalEvidenceKindV1::TerminalChildResult],
    };
    let executable = VerificationGateDto::ExecutableGate {
        template_id: [21; 16],
        template_revision: 2,
    };
    assert!(!reference.is_executable());
    assert!(executable.is_executable());
    assert!(reference.validate().is_ok());
    assert_eq!(
        VerificationGateDto::ReferenceGate {
            evidence_contract_revision: 0,
            accepted_reference_kinds: vec![GoalEvidenceKindV1::TerminalChildResult],
        }
        .validate()
        .expect_err("missing evidence contract revision")
        .code(),
        GOAL_GATE_UNAVAILABLE
    );
    assert_eq!(
        VerificationGateDto::ReferenceGate {
            evidence_contract_revision: 1,
            accepted_reference_kinds: Vec::new(),
        }
        .validate()
        .expect_err("empty accepted kind set")
        .code(),
        GOAL_GATE_UNAVAILABLE
    );
    assert_eq!(
        VerificationGateDto::ExecutableGate {
            template_id: [0; 16],
            template_revision: 1,
        }
        .validate()
        .expect_err("missing template identity")
        .code(),
        GOAL_GATE_UNAVAILABLE
    );
    assert_eq!(
        VerificationGateDto::ExecutableGate {
            template_id: [21; 16],
            template_revision: 0,
        }
        .validate()
        .expect_err("inexact template revision")
        .code(),
        GOAL_GATE_UNAVAILABLE
    );

    // Gate definitions and selected gate revisions are bounded and exact.
    let definition = GoalGateDefinitionV1 {
        gate_id: [5; 16],
        gate: reference.clone(),
    };
    let definitions: Vec<GoalGateDefinitionV1> = (1..=32u8)
        .map(|index| GoalGateDefinitionV1 {
            gate_id: [index; 16],
            gate: reference.clone(),
        })
        .collect();
    assert!(validate_goal_gate_definitions(&definitions).is_ok());
    let over: Vec<GoalGateDefinitionV1> = (1..=33u8)
        .map(|index| GoalGateDefinitionV1 {
            gate_id: [index; 16],
            gate: reference.clone(),
        })
        .collect();
    assert_eq!(
        validate_goal_gate_definitions(&over)
            .expect_err("gate definition limit")
            .code(),
        GOAL_GATE_LIMIT_EXCEEDED
    );
    assert_eq!(
        validate_goal_gate_definitions(&[definition.clone(), definition])
            .expect_err("duplicate gate identity")
            .code(),
        GOAL_REVISION_CONFLICT
    );
    let gate_revisions = vec![GoalGateRevisionReferenceV1 {
        gate_reference: [5; 16],
        revision: 1,
    }];
    assert!(validate_goal_gate_revisions(&gate_revisions).is_ok());
    let over_revisions: Vec<GoalGateRevisionReferenceV1> = (1..=33u8)
        .map(|index| GoalGateRevisionReferenceV1 {
            gate_reference: [index; 16],
            revision: 1,
        })
        .collect();
    assert_eq!(
        validate_goal_gate_revisions(&over_revisions)
            .expect_err("selected gate revision limit")
            .code(),
        GOAL_GATE_LIMIT_EXCEEDED
    );
    assert_eq!(
        validate_goal_gate_revisions(&[GoalGateRevisionReferenceV1 {
            gate_reference: [5; 16],
            revision: 0,
        }])
        .expect_err("inexact gate revision")
        .code(),
        GOAL_REVISION_CONFLICT
    );

    // Templates are user-created typed records; a model proposal needs the
    // explicit user confirmation and never enables itself.
    let template = template_card();
    assert!(template.validate().is_ok());
    assert!(validate_gate_template_creation(&template, &GoalTemplateProvenanceV1::User).is_ok());
    assert_eq!(
        validate_gate_template_creation(
            &template,
            &GoalTemplateProvenanceV1::ModelProposal {
                draft_id: [30; 16],
                accepted_by_user: false,
            }
        )
        .expect_err("unconfirmed model proposal")
        .code(),
        GOAL_GATE_UNAVAILABLE
    );
    assert_eq!(
        validate_gate_template_creation(
            &template,
            &GoalTemplateProvenanceV1::ModelProposal {
                draft_id: [0; 16],
                accepted_by_user: true,
            }
        )
        .expect_err("missing proposal identity")
        .code(),
        GOAL_GATE_UNAVAILABLE
    );
    assert!(
        validate_gate_template_creation(
            &template,
            &GoalTemplateProvenanceV1::ModelProposal {
                draft_id: [30; 16],
                accepted_by_user: true,
            }
        )
        .is_ok()
    );
    assert_eq!(
        GoalGateTemplateCardV1 {
            scope: GoalTemplateScopeDto::Session {
                session_id: [0; 16]
            },
            ..template
        }
        .validate()
        .expect_err("missing template scope identity")
        .code(),
        GOAL_GATE_UNAVAILABLE
    );

    let context = applicability_context();
    assert!(
        validate_gate_template_scope(
            &GoalTemplateScopeDto::Project {
                project_id: [1; 16]
            },
            &context
        )
        .is_ok()
    );
    assert!(
        validate_gate_template_scope(&GoalTemplateScopeDto::Goal { goal_id: [4; 16] }, &context)
            .is_ok()
    );
    assert_eq!(
        validate_gate_template_scope(&GoalTemplateScopeDto::Goal { goal_id: [8; 16] }, &context)
            .expect_err("out-of-chain template")
            .code(),
        GOAL_GATE_UNAVAILABLE
    );
    assert_eq!(
        validate_gate_template_scope(
            &GoalTemplateScopeDto::Session {
                session_id: [9; 16]
            },
            &context
        )
        .expect_err("foreign session template")
        .code(),
        GOAL_GATE_UNAVAILABLE
    );

    use GoalTemplateLifecycleStateDto::{Archived, Enabled};
    assert!(validate_gate_template_lifecycle_transition(None, Enabled).is_ok());
    assert!(validate_gate_template_lifecycle_transition(Some(Enabled), Archived).is_ok());
    assert!(validate_gate_template_lifecycle_transition(Some(Archived), Enabled).is_ok());
    assert_eq!(
        validate_gate_template_lifecycle_transition(None, Archived)
            .expect_err("a template is created enabled")
            .code(),
        GOAL_GATE_UNAVAILABLE
    );
    assert_eq!(
        validate_gate_template_lifecycle_transition(Some(Archived), Archived)
            .expect_err("a template cannot re-archive")
            .code(),
        GOAL_GATE_UNAVAILABLE
    );

    // An executable gate requires the exact template revision; a missing or
    // stale template is typed unavailability.
    assert!(validate_gate_execution_preconditions(&executable, Some(&template)).is_ok());
    assert_eq!(
        validate_gate_execution_preconditions(&executable, None)
            .expect_err("missing template")
            .code(),
        GOAL_GATE_UNAVAILABLE
    );
    let stale_template = GoalGateTemplateCardV1 {
        revision: 3,
        ..template
    };
    assert_eq!(
        validate_gate_execution_preconditions(&executable, Some(&stale_template))
            .expect_err("stale template")
            .code(),
        GOAL_GATE_UNAVAILABLE
    );
    let template_reference = GoalGateRevisionReferenceV1 {
        gate_reference: [21; 16],
        revision: 2,
    };
    assert!(validate_gate_template_reference(&template_reference, Some(&template)).is_ok());
    assert!(
        validate_gate_template_reference(&template_reference, Some(&stale_template)).is_err(),
        "a stale template reference is refused"
    );
    assert!(
        validate_gate_template_reference(&template_reference, None).is_err(),
        "a missing template reference is refused"
    );

    // A ReferenceGate accepts only its declared evidence kinds.
    let undeclared = GoalEvidenceReferenceV1 {
        kind: GoalEvidenceKindV1::AcceptedUserDeclaration,
        ..evidence(6)
    };
    assert_eq!(
        validate_reference_gate_evidence(&reference, &undeclared)
            .expect_err("undeclared evidence kind")
            .code(),
        GOAL_GATE_FAILED
    );
    let declared = GoalEvidenceReferenceV1 {
        kind: GoalEvidenceKindV1::TerminalChildResult,
        ..undeclared
    };
    assert!(validate_reference_gate_evidence(&reference, &declared).is_ok());

    // Outcomes are closed and typed; failure moves the Goal to rework and no
    // outcome is silently retried or promoted to readiness.
    assert_eq!(
        GoalGateOutcomeV1::Passed { evidence: declared }.disposition(),
        GoalGateOutcomeDispositionV1::Passed
    );
    assert_eq!(
        GoalGateOutcomeV1::Failed { evidence: declared }.disposition(),
        GoalGateOutcomeDispositionV1::Failed
    );
    assert_eq!(
        GoalGateOutcomeV1::ExternalEffectUnknown { evidence: declared }.disposition(),
        GoalGateOutcomeDispositionV1::UnknownEffect
    );
    for outcome in [
        GoalGateOutcomeV1::TimedOut,
        GoalGateOutcomeV1::Cancelled,
        GoalGateOutcomeV1::OutputBoundExceeded,
    ] {
        assert_eq!(outcome.disposition(), GoalGateOutcomeDispositionV1::Failed);
        assert!(!outcome.is_success());
    }
    for outcome in [
        GoalGateOutcomeV1::ReferenceUnavailable,
        GoalGateOutcomeV1::RevisionStale,
        GoalGateOutcomeV1::TemplateUnavailable,
    ] {
        assert_eq!(
            outcome.disposition(),
            GoalGateOutcomeDispositionV1::Unavailable
        );
    }
    assert_eq!(
        validate_gate_outcome(&GoalGateOutcomeV1::Passed {
            evidence: GoalEvidenceReferenceV1 {
                revision: 0,
                ..declared
            }
        })
        .expect_err("inexact outcome evidence")
        .code(),
        GOAL_GATE_FAILED
    );
    assert_eq!(
        apply_required_gate_outcome(
            GoalLifecycleStateDto::Active,
            validate_gate_outcome(&GoalGateOutcomeV1::ExternalEffectUnknown { evidence: declared })
                .expect("unknown effect is a typed outcome")
        )
        .expect("unknown effect moves to rework"),
        GoalLifecycleStateDto::NeedsRework
    );
}

#[test]
fn working_memory_cards_disclosure_and_replacement() {
    // Required-evidence row: memory/role/template card, disclosure,
    // replacement, and rollback fixtures.
    let context = applicability_context();
    let card = memory_card();
    assert!(card.validate().is_ok());
    assert!(validate_memory_card_applicability(&card, &context).is_ok());
    assert_eq!(
        validate_memory_card_applicability(
            &GoalMemoryCardV1 {
                scope: GoalRecordScopeDto::Goal { goal_id: [8; 16] },
                ..card.clone()
            },
            &context
        )
        .expect_err("out-of-chain Goal memory")
        .code(),
        MEMORY_REFERENCE_UNAVAILABLE
    );
    assert_eq!(
        validate_memory_card_applicability(
            &GoalMemoryCardV1 {
                scope: GoalRecordScopeDto::Session {
                    session_id: [9; 16]
                },
                ..card.clone()
            },
            &context
        )
        .expect_err("foreign session memory")
        .code(),
        MEMORY_REFERENCE_UNAVAILABLE
    );
    assert_eq!(
        validate_memory_card_applicability(
            &GoalMemoryCardV1 {
                scope: GoalRecordScopeDto::Project {
                    project_id: [9; 16]
                },
                ..card.clone()
            },
            &context
        )
        .expect_err("foreign project memory")
        .code(),
        MEMORY_REFERENCE_UNAVAILABLE
    );

    // Full content is revealed only against the exact frozen reference.
    assert!(validate_memory_disclosure(&card, [41; 16]).is_ok());
    assert_eq!(
        validate_memory_disclosure(&card, [99; 16])
            .expect_err("unfrozen full record")
            .code(),
        MEMORY_REFERENCE_UNAVAILABLE
    );
    assert_eq!(
        validate_memory_disclosure(&card, [0; 16])
            .expect_err("missing retained content")
            .code(),
        MEMORY_REFERENCE_UNAVAILABLE
    );

    let cards: Vec<GoalMemoryCardV1> = (1..=128u8)
        .map(|index| GoalMemoryCardV1 {
            record_id: [index; 16],
            ..card.clone()
        })
        .collect();
    assert!(validate_memory_card_set(&cards, &context).is_ok());
    let over: Vec<GoalMemoryCardV1> = (1..=129u8)
        .map(|index| GoalMemoryCardV1 {
            record_id: [index; 16],
            ..card.clone()
        })
        .collect();
    assert_eq!(
        validate_memory_card_set(&over, &context)
            .expect_err("active memory card limit")
            .code(),
        MEMORY_ENTRY_LIMIT_EXCEEDED
    );
    assert_eq!(
        validate_memory_card_set(&[card.clone(), card.clone()], &context)
            .expect_err("duplicate memory card")
            .code(),
        GOAL_REVISION_CONFLICT
    );
    assert_eq!(
        GoalMemoryCardV1 {
            title: "  ".to_owned(),
            ..card.clone()
        }
        .validate()
        .expect_err("blank card title")
        .code(),
        MEMORY_REFERENCE_UNAVAILABLE
    );
    assert_eq!(
        GoalMemoryCardV1 {
            title: "x".repeat(GOAL_MAX_FULL_RECORD_BYTES as usize + 1),
            ..card.clone()
        }
        .validate()
        .expect_err("oversized memory card")
        .code(),
        MEMORY_ENTRY_TOO_LARGE
    );
    assert_eq!(
        GoalMemoryCardV1 {
            retained_content_reference: [0; 16],
            ..card
        }
        .validate()
        .expect_err("missing content reference")
        .code(),
        MEMORY_REFERENCE_UNAVAILABLE
    );

    // A newer record replaces an older one only through an exact typed link;
    // rollback never selects a future revision.
    assert!(
        validate_memory_replacement(&GoalMemoryReplacementLinkV1 {
            replaced_record_id: [40; 16],
            replaced_revision: 1,
            replacement_record_id: [42; 16],
            replacement_revision: 1,
        })
        .is_ok()
    );
    assert_eq!(
        validate_memory_replacement(&GoalMemoryReplacementLinkV1 {
            replaced_record_id: [40; 16],
            replaced_revision: 1,
            replacement_record_id: [40; 16],
            replacement_revision: 1,
        })
        .expect_err("self replacement")
        .code(),
        MEMORY_REPLACEMENT_CONFLICT
    );
    assert_eq!(
        validate_memory_replacement(&GoalMemoryReplacementLinkV1 {
            replaced_record_id: [40; 16],
            replaced_revision: 2,
            replacement_record_id: [40; 16],
            replacement_revision: 2,
        })
        .expect_err("equal revision replacement")
        .code(),
        MEMORY_REPLACEMENT_CONFLICT
    );
    assert!(
        validate_memory_rollback(&GoalMemoryRollbackLinkV1 {
            current_record_id: [40; 16],
            current_revision: 3,
            restored_record_id: [40; 16],
            restored_revision: 1,
        })
        .is_ok()
    );
    assert_eq!(
        validate_memory_rollback(&GoalMemoryRollbackLinkV1 {
            current_record_id: [40; 16],
            current_revision: 1,
            restored_record_id: [40; 16],
            restored_revision: 2,
        })
        .expect_err("rollback to a future revision")
        .code(),
        MEMORY_REPLACEMENT_CONFLICT
    );
}

#[test]
fn skill_role_cards_and_delegation_narrowing() {
    // Required-evidence row: memory/role/template card, disclosure,
    // replacement, and rollback fixtures.
    let context = applicability_context();
    let skill = skill_card();
    assert!(skill.validate().is_ok());
    assert!(validate_goal_skill_name("goal-audit").is_ok());
    for invalid in ["Goal-Audit", "goal_audit", "", "goal audit", "-goal"] {
        assert_eq!(
            validate_goal_skill_name(invalid)
                .expect_err("invalid canonical skill name")
                .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
    }
    assert_eq!(
        GoalSkillCardV1 {
            canonical_name: "Bad Name".to_owned(),
            ..skill.clone()
        }
        .validate()
        .expect_err("invalid card name")
        .code(),
        SKILL_REFERENCE_UNAVAILABLE
    );
    assert_eq!(
        GoalSkillCardV1 {
            content_reference: [0; 16],
            ..skill.clone()
        }
        .validate()
        .expect_err("missing skill content reference")
        .code(),
        SKILL_REFERENCE_UNAVAILABLE
    );
    assert_eq!(
        GoalSkillCardV1 {
            description: "x".repeat(GOAL_MAX_FULL_RECORD_BYTES as usize + 1),
            ..skill.clone()
        }
        .validate()
        .expect_err("oversized skill card")
        .code(),
        SKILL_ENTRY_TOO_LARGE
    );
    assert!(validate_skill_content_size(GOAL_MAX_FULL_RECORD_BYTES).is_ok());
    assert_eq!(
        validate_skill_content_size(GOAL_MAX_FULL_RECORD_BYTES + 1)
            .expect_err("oversized skill body")
            .code(),
        SKILL_ENTRY_TOO_LARGE
    );
    assert_eq!(
        validate_skill_card_applicability(
            &GoalSkillCardV1 {
                owner_scope: GoalRecordScopeDto::Session {
                    session_id: [9; 16]
                },
                ..skill.clone()
            },
            &context
        )
        .expect_err("foreign session skill")
        .code(),
        SKILL_REFERENCE_UNAVAILABLE
    );
    assert!(validate_skill_disclosure(&skill, [51; 16]).is_ok());
    assert_eq!(
        validate_skill_disclosure(&skill, [52; 16])
            .expect_err("unfrozen skill content")
            .code(),
        SKILL_REFERENCE_UNAVAILABLE
    );

    // Roles narrow only.
    let role = role_card();
    assert!(role.validate().is_ok());
    let narrowed = GoalRoleCardV1 {
        tool_subset: vec!["read".to_owned()],
        context_limit_bytes: 1_024,
        result_limit_bytes: 512,
        ..role.clone()
    };
    assert!(validate_role_narrowing(Some(&role), &narrowed).is_ok());
    assert!(validate_role_narrowing(None, &role).is_ok());
    let widened_tools = GoalRoleCardV1 {
        tool_subset: vec!["read".to_owned(), "execute".to_owned()],
        ..narrowed.clone()
    };
    assert_eq!(
        validate_role_narrowing(Some(&role), &widened_tools)
            .expect_err("role adds a tool")
            .code(),
        DELEGATION_ROLE_WIDENING_FORBIDDEN
    );
    let widened_class = GoalRoleCardV1 {
        permitted_class: GoalRoleClassV1::Heavy,
        ..narrowed.clone()
    };
    assert_eq!(
        validate_role_narrowing(Some(&role), &widened_class)
            .expect_err("role raises its class")
            .code(),
        DELEGATION_ROLE_WIDENING_FORBIDDEN
    );
    let widened_context = GoalRoleCardV1 {
        context_limit_bytes: 8_192,
        ..narrowed.clone()
    };
    assert_eq!(
        validate_role_narrowing(Some(&role), &widened_context)
            .expect_err("role widens its context limit")
            .code(),
        DELEGATION_ROLE_WIDENING_FORBIDDEN
    );
    assert_eq!(
        GoalRoleCardV1 {
            task: "  ".to_owned(),
            ..role.clone()
        }
        .validate()
        .expect_err("blank role task")
        .code(),
        DELEGATION_ROLE_INVALID
    );
    assert_eq!(
        GoalRoleCardV1 {
            revision: 0,
            ..role.clone()
        }
        .validate()
        .expect_err("inexact role revision")
        .code(),
        DELEGATION_ROLE_INVALID
    );
    assert_eq!(
        GoalRoleCardV1 {
            tool_subset: vec!["read".to_owned(), "read".to_owned()],
            ..role.clone()
        }
        .validate()
        .expect_err("duplicate role tool")
        .code(),
        DELEGATION_ROLE_INVALID
    );

    // The child delegation snapshot is frozen, bounded, and narrowing-only.
    let snapshot = GoalDelegationSnapshotV1 {
        parent_task: "Audit child work".to_owned(),
        leading_goal_id: [4; 16],
        goal_revision: 2,
        required_constraints: vec!["read-only".to_owned()],
        memory_card_references: vec![GoalCardReferenceV1 {
            card_reference: [40; 16],
            revision: 1,
        }],
        skill_card_references: vec![GoalCardReferenceV1 {
            card_reference: [50; 16],
            revision: 1,
        }],
        selected_role: Some(narrowed),
        policy_snapshot_reference: [70; 16],
        activity_selection_reference: [71; 16],
        canonical_digest: Digest256::sha256(b"delegation"),
    };
    assert!(validate_goal_delegation_snapshot(&snapshot, Some(&role)).is_ok());
    assert_eq!(
        validate_goal_delegation_snapshot(
            &GoalDelegationSnapshotV1 {
                selected_role: Some(widened_tools),
                ..snapshot.clone()
            },
            Some(&role)
        )
        .expect_err("child role widens the parent role")
        .code(),
        DELEGATION_ROLE_WIDENING_FORBIDDEN
    );
    assert_eq!(
        validate_goal_delegation_snapshot(
            &GoalDelegationSnapshotV1 {
                goal_revision: 0,
                ..snapshot.clone()
            },
            Some(&role)
        )
        .expect_err("inexact delegated revision")
        .code(),
        GOAL_REVISION_CONFLICT
    );
    assert_eq!(
        validate_goal_delegation_snapshot(
            &GoalDelegationSnapshotV1 {
                parent_task: "  ".to_owned(),
                ..snapshot
            },
            Some(&role)
        )
        .expect_err("missing parent task")
        .code(),
        GOAL_SNAPSHOT_UNAVAILABLE
    );

    let many_skills: Vec<GoalSkillCardV1> = (1..=32u8)
        .map(|index| GoalSkillCardV1 {
            skill_id: [index; 16],
            ..skill.clone()
        })
        .collect();
    assert!(validate_skill_role_card_set(&many_skills, &[], &context).is_ok());
    let many_roles: Vec<GoalRoleCardV1> = (1..=32u8)
        .map(|index| GoalRoleCardV1 {
            role_id: [index; 16],
            ..role.clone()
        })
        .collect();
    assert_eq!(
        validate_skill_role_card_set(&many_skills, &many_roles, &context)
            .expect_err("combined skill and role card limit")
            .code(),
        GOAL_LIMIT_EXCEEDED
    );
}

#[test]
fn refinement_draft_coalescing_accept_edit_reject_and_stale_base() {
    // Required-evidence row: proposal coalescing, accept/edit/reject, and
    // stale-base conflict fixtures.
    let draft = refinement_draft();
    assert!(draft.validate().is_ok());
    let equal = RefinementDraftDto {
        draft_id: [83; 16],
        ..draft.clone()
    };
    assert_eq!(
        coalesce_refinement_draft(Some(&draft), &equal).expect("equal proposal coalesces"),
        GoalDraftCoalescingV1::CoalesceInto { draft_id: [80; 16] }
    );
    assert_eq!(
        coalesce_refinement_draft(None, &draft).expect("first proposal"),
        GoalDraftCoalescingV1::CreateNew
    );
    let different_milestone = RefinementDraftDto {
        milestone: GoalMilestoneDto::UserAcceptance,
        ..equal.clone()
    };
    assert_eq!(
        coalesce_refinement_draft(Some(&draft), &different_milestone)
            .expect_err("one pending draft per Goal")
            .code(),
        REFINEMENT_DRAFT_CONFLICT
    );
    let other_goal = RefinementDraftDto {
        leading_goal_id: [8; 16],
        ..equal.clone()
    };
    assert_eq!(
        coalesce_refinement_draft(Some(&draft), &other_goal).expect("another Goal is a new draft"),
        GoalDraftCoalescingV1::CreateNew
    );
    assert!(validate_pending_draft_limit(&[draft.clone(), other_goal]).is_ok());
    assert_eq!(
        validate_pending_draft_limit(&[draft.clone(), equal])
            .expect_err("pending draft limit")
            .code(),
        REFINEMENT_DRAFT_CONFLICT
    );

    // The exact base revision is validated; a stale base is a typed conflict.
    assert_eq!(
        resolve_refinement_draft(&draft, RefinementDecisionV1::Accept, 3)
            .expect_err("stale base revision")
            .code(),
        REFINEMENT_DRAFT_CONFLICT
    );
    assert_eq!(
        resolve_refinement_draft(&draft, RefinementDecisionV1::Accept, 2).expect("accept"),
        GoalRefinementResolutionV1::Accepted { goal_revision: 3 }
    );
    let edit = draft.edits[0];
    assert_eq!(
        resolve_refinement_draft(
            &draft,
            RefinementDecisionV1::EditAndAccept { edits: vec![edit] },
            2
        )
        .expect("edit and accept"),
        GoalRefinementResolutionV1::EditAccepted {
            edits: vec![edit],
            goal_revision: 3,
        }
    );
    assert_eq!(
        resolve_refinement_draft(
            &draft,
            RefinementDecisionV1::EditAndAccept { edits: Vec::new() },
            2
        )
        .expect_err("empty edit-and-accept")
        .code(),
        REFINEMENT_DRAFT_CONFLICT
    );
    assert_eq!(
        resolve_refinement_draft(&draft, RefinementDecisionV1::Reject, 9)
            .expect("rejection changes no active record"),
        GoalRefinementResolutionV1::Rejected
    );

    // Draft shape and size are bounded; the draft never becomes a record.
    assert_eq!(
        RefinementDraftDto {
            edits: Vec::new(),
            ..draft.clone()
        }
        .validate()
        .expect_err("draft without edits")
        .code(),
        REFINEMENT_DRAFT_CONFLICT
    );
    assert_eq!(
        RefinementDraftDto {
            safe_rationale: "  ".to_owned(),
            ..draft.clone()
        }
        .validate()
        .expect_err("blank rationale")
        .code(),
        REFINEMENT_DRAFT_CONFLICT
    );
    assert_eq!(
        RefinementDraftDto {
            safe_rationale: "x".repeat(GOAL_MAX_FULL_RECORD_BYTES as usize + 1),
            ..draft.clone()
        }
        .validate()
        .expect_err("oversized draft")
        .code(),
        REFINEMENT_DRAFT_TOO_LARGE
    );
    assert_eq!(
        RefinementDraftDto {
            draft_id: [0; 16],
            ..draft
        }
        .validate()
        .expect_err("missing draft identity")
        .code(),
        REFINEMENT_DRAFT_CONFLICT
    );
}

#[test]
fn compaction_working_form_correction_and_fork_references() {
    // Required-evidence row: compaction working-form, correction,
    // fork-reference, and bound fixtures.
    let first = summary();
    assert!(first.validate().is_ok());
    assert_eq!(
        ConversationSummaryDto {
            revision: 0,
            ..first.clone()
        }
        .validate()
        .expect_err("zero summary revision")
        .code(),
        COMPACTION_SUMMARY_UNAVAILABLE
    );
    assert_eq!(
        ConversationSummaryDto {
            source_range_start: [0; 16],
            ..first.clone()
        }
        .validate()
        .expect_err("missing source range")
        .code(),
        COMPACTION_HISTORY_UNAVAILABLE
    );
    assert_eq!(
        ConversationSummaryDto {
            safe_content: "x".repeat(GOAL_MAX_FULL_RECORD_BYTES as usize + 1),
            ..summary()
        }
        .validate()
        .expect_err("oversized summary")
        .code(),
        COMPACTION_SUMMARY_TOO_LARGE
    );

    // The working form extends one cumulative summary over the next completed
    // uncompacted suffix only inside an active admitted run.
    let next = ConversationSummaryDto {
        summary_id: first.summary_id,
        revision: 2,
        previous_summary_reference: Some([90; 16]),
        source_range_start: [93; 16],
        source_range_end: [94; 16],
        safe_content: "Next completed range".to_owned(),
        canonical_digest: first.canonical_digest,
    };
    let working = GoalCompactionWorkingFormV1 {
        current_summary: Some(first.clone()),
        uncompacted_suffix: vec![[93; 16], [94; 16], [95; 16]],
    };
    assert!(
        validate_compaction_extension(&working, &next, GoalCompactionOriginV1::ActiveRun).is_ok()
    );
    for origin in [
        GoalCompactionOriginV1::BeforeAdmission,
        GoalCompactionOriginV1::AfterTerminalization,
        GoalCompactionOriginV1::AfterRestart,
    ] {
        assert_eq!(
            validate_compaction_extension(&working, &next, origin)
                .expect_err("compaction outside an active admitted run")
                .code(),
            COMPACTION_HISTORY_UNAVAILABLE
        );
    }
    let skipped = GoalCompactionWorkingFormV1 {
        uncompacted_suffix: vec![[94; 16], [95; 16]],
        ..working.clone()
    };
    assert_eq!(
        validate_compaction_extension(&skipped, &next, GoalCompactionOriginV1::ActiveRun)
            .expect_err("summary skips the first uncompacted fact")
            .code(),
        COMPACTION_HISTORY_UNAVAILABLE
    );
    let outside_suffix = ConversationSummaryDto {
        source_range_end: [96; 16],
        ..next.clone()
    };
    assert_eq!(
        validate_compaction_extension(&working, &outside_suffix, GoalCompactionOriginV1::ActiveRun)
            .expect_err("summary ends outside the completed suffix")
            .code(),
        COMPACTION_HISTORY_UNAVAILABLE
    );
    let empty_suffix = GoalCompactionWorkingFormV1 {
        uncompacted_suffix: Vec::new(),
        ..working.clone()
    };
    assert_eq!(
        validate_compaction_extension(&empty_suffix, &next, GoalCompactionOriginV1::ActiveRun)
            .expect_err("nothing completed to compact")
            .code(),
        COMPACTION_HISTORY_UNAVAILABLE
    );
    let wrong_predecessor = ConversationSummaryDto {
        previous_summary_reference: Some([99; 16]),
        ..next.clone()
    };
    assert_eq!(
        validate_compaction_extension(
            &working,
            &wrong_predecessor,
            GoalCompactionOriginV1::ActiveRun
        )
        .expect_err("predecessor mismatch")
        .code(),
        COMPACTION_SUMMARY_UNAVAILABLE
    );
    let wrong_revision = ConversationSummaryDto {
        revision: 3,
        ..next
    };
    assert_eq!(
        validate_compaction_extension(&working, &wrong_revision, GoalCompactionOriginV1::ActiveRun)
            .expect_err("summary revision gap")
            .code(),
        COMPACTION_SUMMARY_UNAVAILABLE
    );

    // A first summary over an empty prefix starts at revision one.
    let fresh_working = GoalCompactionWorkingFormV1 {
        current_summary: None,
        uncompacted_suffix: vec![[91; 16], [92; 16]],
    };
    assert!(
        validate_compaction_extension(&fresh_working, &first, GoalCompactionOriginV1::ActiveRun)
            .is_ok()
    );

    // Correction creates a later immutable revision and preserves the earlier
    // one; forks carry only the exact selected summary reference.
    let corrected = ConversationSummaryDto {
        revision: 2,
        safe_content: "Corrected summary".to_owned(),
        ..summary()
    };
    assert!(validate_conversation_summary_correction(&first, &corrected).is_ok());
    assert_eq!(
        validate_conversation_summary_correction(
            &first,
            &ConversationSummaryDto {
                source_range_end: [99; 16],
                ..corrected.clone()
            }
        )
        .expect_err("correction changes the source range")
        .code(),
        COMPACTION_SUMMARY_UNAVAILABLE
    );
    assert_eq!(
        validate_conversation_summary_correction(
            &first,
            &ConversationSummaryDto {
                summary_id: [97; 16],
                ..corrected
            }
        )
        .expect_err("correction changes the summary identity")
        .code(),
        COMPACTION_SUMMARY_UNAVAILABLE
    );
    assert!(validate_fork_summary_inheritance(&first, &first).is_ok());
    assert_eq!(
        validate_fork_summary_inheritance(
            &first,
            &ConversationSummaryDto {
                revision: 2,
                ..first.clone()
            }
        )
        .expect_err("fork imports a future summary")
        .code(),
        COMPACTION_SUMMARY_UNAVAILABLE
    );
}

#[test]
fn code_owned_bounds_match_architecture_28() {
    // Required-evidence row: bound/closed-safe-failure and fake-secret
    // regression across logs, errors, snapshots, events, and adapter DTOs.
    assert_eq!(GOAL_MAX_GOALS_PER_PROJECT, 256);
    assert_eq!(GOAL_MAX_GOALS_PER_SESSION, 64);
    assert_eq!(GOAL_MAX_TREE_DEPTH, 16);
    assert_eq!(GOAL_MAX_DIRECT_CHILDREN, 32);
    assert_eq!(GOAL_MAX_SESSION_LINKS_PER_PROJECT_GOAL, 64);
    assert_eq!(GOAL_MAX_GATES_PER_GOAL, 32);
    assert_eq!(GOAL_MAX_ACTIVE_MEMORY_CARDS, 128);
    assert_eq!(GOAL_MAX_SELECTED_SKILL_ROLE_CARDS, 32);
    assert_eq!(GOAL_MAX_FULL_RECORD_BYTES, 512 * 1024);
    assert_eq!(GOAL_MAX_TARGET_SNAPSHOT_BYTES, 1024 * 1024);
    assert_eq!(GOAL_MAX_CONTEXT_BYTES, 4 * 1024 * 1024);
    assert_eq!(GOAL_MAX_PENDING_PROPOSALS, 1);
    assert_eq!(GOAL_MAX_PARENT_CHAIN, 16);
    assert_eq!(GOAL_MAX_COMPONENT_REFERENCES, 32);
    assert_eq!(GOAL_MAX_GATE_REVISIONS, 32);
    assert_eq!(GOAL_MAX_EVIDENCE_REFERENCES, 512);
    assert_eq!(GOAL_MAX_REVEALED_RECORDS, 512);
}

#[test]
fn closed_goal_failure_codes_are_unique_constants() {
    // Required-evidence row: bound/closed-safe-failure and fake-secret
    // regression across logs, errors, snapshots, events, and adapter DTOs.
    assert_eq!(GOAL_FAILURE_CODES.len(), 28);
    let mut unique = std::collections::BTreeSet::new();
    for code in GOAL_FAILURE_CODES {
        assert!(!code.is_empty(), "failure codes are non-empty");
        assert!(unique.insert(*code), "failure code {code} is declared once");
    }
    assert_eq!(GOAL_LIMIT_EXCEEDED, "goal_limit_exceeded");
    assert_eq!(
        GOAL_TREE_DEPTH_LIMIT_EXCEEDED,
        "goal_tree_depth_limit_exceeded"
    );
    assert_eq!(GOAL_CHILD_LIMIT_EXCEEDED, "goal_child_limit_exceeded");
    assert_eq!(
        GOAL_SESSION_LINK_LIMIT_EXCEEDED,
        "goal_session_link_limit_exceeded"
    );
    assert_eq!(GOAL_CYCLE_DETECTED, "goal_cycle_detected");
    assert_eq!(GOAL_NOT_ACTIVE, "goal_not_active");
    assert_eq!(GOAL_REVISION_CONFLICT, "goal_revision_conflict");
    assert_eq!(GOAL_SNAPSHOT_TOO_LARGE, "goal_snapshot_too_large");
    assert_eq!(GOAL_SNAPSHOT_UNAVAILABLE, "goal_snapshot_unavailable");
    assert_eq!(GOAL_GATE_LIMIT_EXCEEDED, "goal_gate_limit_exceeded");
    assert_eq!(GOAL_GATE_UNAVAILABLE, "goal_gate_unavailable");
    assert_eq!(GOAL_GATE_FAILED, "goal_gate_failed");
    assert_eq!(GOAL_NOT_READY, "goal_not_ready");
    assert_eq!(
        GOAL_ACCEPTANCE_EXCEPTION_INVALID,
        "goal_acceptance_exception_invalid"
    );
    assert_eq!(GOAL_ARCHIVE_NOT_TERMINAL, "goal_archive_not_terminal");
    assert_eq!(MEMORY_ENTRY_LIMIT_EXCEEDED, "memory_entry_limit_exceeded");
    assert_eq!(MEMORY_ENTRY_TOO_LARGE, "memory_entry_too_large");
    assert_eq!(MEMORY_REFERENCE_UNAVAILABLE, "memory_reference_unavailable");
    assert_eq!(MEMORY_REPLACEMENT_CONFLICT, "memory_replacement_conflict");
    assert_eq!(SKILL_ENTRY_TOO_LARGE, "skill_entry_too_large");
    assert_eq!(SKILL_REFERENCE_UNAVAILABLE, "skill_reference_unavailable");
    assert_eq!(DELEGATION_ROLE_INVALID, "delegation_role_invalid");
    assert_eq!(
        DELEGATION_ROLE_WIDENING_FORBIDDEN,
        "delegation_role_widening_forbidden"
    );
    assert_eq!(COMPACTION_SUMMARY_TOO_LARGE, "compaction_summary_too_large");
    assert_eq!(
        COMPACTION_SUMMARY_UNAVAILABLE,
        "compaction_summary_unavailable"
    );
    assert_eq!(
        COMPACTION_HISTORY_UNAVAILABLE,
        "compaction_history_unavailable"
    );
    assert_eq!(REFINEMENT_DRAFT_CONFLICT, "refinement_draft_conflict");
    assert_eq!(REFINEMENT_DRAFT_TOO_LARGE, "refinement_draft_too_large");
}

#[test]
fn fake_secrets_are_rejected_without_disclosure() {
    // Required-evidence row: bound/closed-safe-failure and fake-secret
    // regression across logs, errors, snapshots, events, and adapter DTOs.
    let secret = "api_key=secret-value";

    let card = GoalMemoryCardV1 {
        title: secret.to_owned(),
        ..memory_card()
    };
    let error = card.validate().expect_err("credential-shaped memory card");
    assert_eq!(error.code(), CREDENTIALS_FORBIDDEN);
    assert!(!error.message().contains(secret));

    let skill = GoalSkillCardV1 {
        description: secret.to_owned(),
        ..skill_card()
    };
    let error = skill.validate().expect_err("credential-shaped skill card");
    assert_eq!(error.code(), CREDENTIALS_FORBIDDEN);
    assert!(!error.message().contains(secret));

    let role = GoalRoleCardV1 {
        task: secret.to_owned(),
        ..role_card()
    };
    let error = role.validate().expect_err("credential-shaped role task");
    assert_eq!(error.code(), CREDENTIALS_FORBIDDEN);
    assert!(!error.message().contains(secret));

    let draft = RefinementDraftDto {
        safe_rationale: secret.to_owned(),
        ..refinement_draft()
    };
    let error = draft
        .validate()
        .expect_err("credential-shaped draft rationale");
    assert_eq!(error.code(), CREDENTIALS_FORBIDDEN);
    assert!(!error.message().contains(secret));

    let summary = ConversationSummaryDto {
        safe_content: secret.to_owned(),
        ..summary()
    };
    let error = summary.validate().expect_err("credential-shaped summary");
    assert_eq!(error.code(), CREDENTIALS_FORBIDDEN);
    assert!(!error.message().contains(secret));

    assert!(
        !GOAL_FAILURE_CODES
            .iter()
            .any(|code| code.contains("secret") || code.contains("key")),
        "closed failure codes never disclose credential material"
    );
}
