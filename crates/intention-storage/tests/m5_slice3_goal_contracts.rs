//! M5 Slice 3 Goal-domain storage contract tests.
//!
//! Every type below is a DTO-only durable record for the Slice 3 Goal surface,
//! so these tests execute the discriminator roundtrips, the accessors, and
//! every accept and reject branch of the `validate` methods, including the
//! closed bounds shared with the domain. They also cover the safe-text helpers
//! of the crate root.

use intention_domain::goal_domain::GOAL_MAX_GATES_PER_GOAL;
use intention_storage::goal_repo::*;
use intention_storage::{
    MAX_SAFE_CONTENT_BYTES, MAX_SAFE_LABEL_CHARS, validate_safe_content, validate_safe_digest,
    validate_safe_label, validate_safe_labels,
};
use intention_types::DtoResult;

/// Builds one canonical `sha256:<64 lowercase hex>` digest from one seed.
fn digest(seed: u8) -> String {
    let unit = format!("{seed:02x}");
    format!("sha256:{}", unit.repeat(32))
}

/// Builds one safe text value of exactly the domain record bound.
fn bounded_text() -> String {
    "x".repeat(MAX_GOAL_SAFE_TEXT_BYTES)
}

/// Builds one safe text value one byte over the domain record bound.
fn oversized_text() -> String {
    "x".repeat(MAX_GOAL_SAFE_TEXT_BYTES + 1)
}

/// Returns the stable code of a rejected contract, or a sentinel when accepted.
fn rejection_code<T>(result: DtoResult<T>) -> String {
    match result {
        Ok(_) => "unexpected_success".to_owned(),
        Err(error) => error.code().to_owned(),
    }
}

/// Builds one exact selected successful evidence reference.
fn evidence_reference() -> GoalEvidenceReferenceDto {
    GoalEvidenceReferenceDto {
        evidence_id: "evidence-1".to_owned(),
        revision: 1,
        kind: GoalEvidenceKindDto::TerminalChildResult,
    }
}

/// Builds one recorded gate exception.
fn gate_exception() -> GoalGateExceptionDto {
    GoalGateExceptionDto {
        gate_id: "gate-1".to_owned(),
        gate_revision: 1,
        kind: GoalGateExceptionKindDto::Failed,
        evidence: evidence_reference(),
    }
}

#[test]
fn goal_scope_variants_expose_project_and_session_identity() -> DtoResult<()> {
    let project = GoalScopeDto::Project {
        project_id: "project-1".to_owned(),
    };
    assert_eq!(project.project_id(), "project-1");
    assert_eq!(project.session_id(), None);
    assert!(project.is_project());
    assert_eq!(project.kind_name(), "project");
    project.validate()?;

    let session = GoalScopeDto::Session {
        project_id: "project-1".to_owned(),
        session_id: "session-1".to_owned(),
    };
    assert_eq!(session.project_id(), "project-1");
    assert_eq!(session.session_id(), Some("session-1"));
    assert!(!session.is_project());
    assert_eq!(session.kind_name(), "session");
    session.validate()?;

    let bounded = GoalScopeDto::Project {
        project_id: "p".repeat(MAX_SAFE_LABEL_CHARS),
    };
    bounded.validate()?;
    let blank_project = GoalScopeDto::Project {
        project_id: String::new(),
    };
    assert_eq!(rejection_code(blank_project.validate()), "goal_not_active");
    let blank_session = GoalScopeDto::Session {
        project_id: "project-1".to_owned(),
        session_id: "  ".to_owned(),
    };
    assert_eq!(rejection_code(blank_session.validate()), "goal_not_active");
    let over_long = GoalScopeDto::Project {
        project_id: "p".repeat(MAX_SAFE_LABEL_CHARS + 1),
    };
    assert_eq!(rejection_code(over_long.validate()), "goal_not_active");
    let credential = GoalScopeDto::Session {
        project_id: "project-1".to_owned(),
        session_id: "api_key=live".to_owned(),
    };
    assert_eq!(
        rejection_code(credential.validate()),
        "credentials_forbidden"
    );
    Ok(())
}

#[test]
fn goal_record_scope_variants_expose_owner_and_kind() -> DtoResult<()> {
    let project = GoalRecordScopeDto::Project {
        project_id: "project-1".to_owned(),
    };
    assert_eq!(project.kind_name(), "project");
    assert_eq!(project.owner_id(), "project-1");
    project.validate("memory_reference_unavailable")?;

    let goal = GoalRecordScopeDto::Goal {
        goal_id: "goal-1".to_owned(),
    };
    assert_eq!(goal.kind_name(), "goal");
    assert_eq!(goal.owner_id(), "goal-1");
    goal.validate("memory_reference_unavailable")?;

    let session = GoalRecordScopeDto::Session {
        session_id: "session-1".to_owned(),
    };
    assert_eq!(session.kind_name(), "session");
    assert_eq!(session.owner_id(), "session-1");
    session.validate("memory_reference_unavailable")?;

    let blank = GoalRecordScopeDto::Goal {
        goal_id: String::new(),
    };
    assert_eq!(
        rejection_code(blank.validate("memory_reference_unavailable")),
        "memory_reference_unavailable"
    );
    Ok(())
}

#[test]
fn goal_lifecycle_state_names_parse_and_terminals() -> DtoResult<()> {
    let states = [
        (GoalLifecycleStateDto::Active, "active", false),
        (GoalLifecycleStateDto::NeedsRework, "needs_rework", false),
        (GoalLifecycleStateDto::Paused, "paused", false),
        (GoalLifecycleStateDto::Stopped, "stopped", true),
        (GoalLifecycleStateDto::Archived, "archived", true),
    ];
    for (state, name, terminal) in states {
        assert_eq!(state.name(), name);
        assert_eq!(GoalLifecycleStateDto::parse(name)?, state);
        assert_eq!(state.is_terminal(), terminal);
    }
    assert_eq!(
        rejection_code(GoalLifecycleStateDto::parse("unknown")),
        "goal_not_active"
    );
    Ok(())
}

#[test]
fn goal_evidence_kind_names_and_parse() -> DtoResult<()> {
    let kinds = [
        (
            GoalEvidenceKindDto::TerminalChildResult,
            "terminal_child_result",
        ),
        (
            GoalEvidenceKindDto::AcceptedUserDeclaration,
            "accepted_user_declaration",
        ),
        (
            GoalEvidenceKindDto::TerminalRegisteredToolResult,
            "terminal_registered_tool_result",
        ),
        (
            GoalEvidenceKindDto::ExecutableGateResult,
            "executable_gate_result",
        ),
    ];
    for (kind, name) in kinds {
        assert_eq!(kind.name(), name);
        assert_eq!(GoalEvidenceKindDto::parse(name)?, kind);
    }
    assert_eq!(
        rejection_code(GoalEvidenceKindDto::parse("")),
        "goal_gate_failed"
    );
    Ok(())
}

#[test]
fn goal_gate_exception_kind_names_and_parse() -> DtoResult<()> {
    let kinds = [
        (GoalGateExceptionKindDto::Failed, "failed"),
        (GoalGateExceptionKindDto::Unavailable, "unavailable"),
        (GoalGateExceptionKindDto::Expired, "expired"),
        (
            GoalGateExceptionKindDto::ExternallyAmbiguous,
            "externally_ambiguous",
        ),
    ];
    for (kind, name) in kinds {
        assert_eq!(kind.name(), name);
        assert_eq!(GoalGateExceptionKindDto::parse(name)?, kind);
    }
    assert_eq!(
        rejection_code(GoalGateExceptionKindDto::parse("unknown")),
        "goal_acceptance_exception_invalid"
    );
    Ok(())
}

#[test]
fn goal_evidence_reference_validates_identity_and_revision() -> DtoResult<()> {
    evidence_reference().validate("goal_not_ready")?;

    let mut blank = evidence_reference();
    blank.evidence_id = String::new();
    assert_eq!(
        rejection_code(blank.validate("goal_not_ready")),
        "goal_not_ready"
    );
    let mut inexact = evidence_reference();
    inexact.revision = 0;
    assert_eq!(
        rejection_code(inexact.validate("goal_not_ready")),
        "goal_not_ready"
    );
    let mut credential = evidence_reference();
    credential.evidence_id = "password=hunter2".to_owned();
    assert_eq!(
        rejection_code(credential.validate("goal_not_ready")),
        "credentials_forbidden"
    );
    Ok(())
}

#[test]
fn goal_gate_exception_validates_exact_gate_and_evidence() -> DtoResult<()> {
    gate_exception().validate()?;

    let mut blank_gate = gate_exception();
    blank_gate.gate_id = " ".to_owned();
    assert_eq!(
        rejection_code(blank_gate.validate()),
        "goal_acceptance_exception_invalid"
    );
    let mut inexact_gate = gate_exception();
    inexact_gate.gate_revision = 0;
    assert_eq!(
        rejection_code(inexact_gate.validate()),
        "goal_acceptance_exception_invalid"
    );
    let mut inexact_evidence = gate_exception();
    inexact_evidence.evidence.revision = 0;
    assert_eq!(
        rejection_code(inexact_evidence.validate()),
        "goal_acceptance_exception_invalid"
    );
    Ok(())
}

#[test]
fn goal_readiness_state_requires_exact_evidence_set() -> DtoResult<()> {
    let not_ready = GoalReadinessStateDto::NotReady;
    assert_eq!(not_ready.kind_name(), "not_ready");
    assert!(!not_ready.is_ready());
    assert!(not_ready.verified_evidence_set().is_empty());
    not_ready.validate()?;

    let ready = GoalReadinessStateDto::Ready {
        verified_evidence_set: vec![evidence_reference()],
    };
    assert_eq!(ready.kind_name(), "ready");
    assert!(ready.is_ready());
    assert_eq!(ready.verified_evidence_set().len(), 1);
    ready.validate()?;

    let empty = GoalReadinessStateDto::Ready {
        verified_evidence_set: Vec::new(),
    };
    assert_eq!(rejection_code(empty.validate()), "goal_not_ready");

    let mut inexact = evidence_reference();
    inexact.revision = 0;
    let inexact_set = GoalReadinessStateDto::Ready {
        verified_evidence_set: vec![inexact],
    };
    assert_eq!(rejection_code(inexact_set.validate()), "goal_not_ready");

    let duplicate = GoalReadinessStateDto::Ready {
        verified_evidence_set: vec![evidence_reference(), evidence_reference()],
    };
    assert_eq!(rejection_code(duplicate.validate()), "goal_not_ready");

    let at_bound = GoalReadinessStateDto::Ready {
        verified_evidence_set: (0..MAX_GOAL_EVIDENCE_REFERENCES)
            .map(|index| GoalEvidenceReferenceDto {
                evidence_id: format!("evidence-{index}"),
                revision: 1,
                kind: GoalEvidenceKindDto::TerminalChildResult,
            })
            .collect(),
    };
    at_bound.validate()?;
    let over_bound = GoalReadinessStateDto::Ready {
        verified_evidence_set: (0..=MAX_GOAL_EVIDENCE_REFERENCES)
            .map(|index| GoalEvidenceReferenceDto {
                evidence_id: format!("evidence-{index}"),
                revision: 1,
                kind: GoalEvidenceKindDto::TerminalChildResult,
            })
            .collect(),
    };
    assert_eq!(rejection_code(over_bound.validate()), "goal_limit_exceeded");
    Ok(())
}

#[test]
fn goal_user_decision_state_validates_exception_sets() -> DtoResult<()> {
    let unaccepted = GoalUserDecisionStateDto::Unaccepted;
    assert_eq!(unaccepted.kind_name(), "unaccepted");
    assert!(!unaccepted.is_terminal());
    assert!(unaccepted.exception_evidence_set().is_empty());
    unaccepted.validate()?;

    let accepted = GoalUserDecisionStateDto::Accepted;
    assert_eq!(accepted.kind_name(), "accepted");
    assert!(accepted.is_terminal());
    assert!(accepted.exception_evidence_set().is_empty());
    accepted.validate()?;

    let with_exception = GoalUserDecisionStateDto::AcceptedWithException {
        exception_evidence_set: vec![gate_exception()],
    };
    assert_eq!(with_exception.kind_name(), "accepted_with_exception");
    assert!(with_exception.is_terminal());
    assert_eq!(with_exception.exception_evidence_set().len(), 1);
    with_exception.validate()?;

    let empty = GoalUserDecisionStateDto::AcceptedWithException {
        exception_evidence_set: Vec::new(),
    };
    assert_eq!(
        rejection_code(empty.validate()),
        "goal_acceptance_exception_invalid"
    );

    let at_bound = GoalUserDecisionStateDto::AcceptedWithException {
        exception_evidence_set: (0..GOAL_MAX_GATES_PER_GOAL)
            .map(|index| GoalGateExceptionDto {
                gate_id: format!("gate-{index}"),
                gate_revision: 1,
                kind: GoalGateExceptionKindDto::Failed,
                evidence: evidence_reference(),
            })
            .collect(),
    };
    at_bound.validate()?;
    let over_bound = GoalUserDecisionStateDto::AcceptedWithException {
        exception_evidence_set: (0..=GOAL_MAX_GATES_PER_GOAL)
            .map(|index| GoalGateExceptionDto {
                gate_id: format!("gate-{index}"),
                gate_revision: 1,
                kind: GoalGateExceptionKindDto::Failed,
                evidence: evidence_reference(),
            })
            .collect(),
    };
    assert_eq!(
        rejection_code(over_bound.validate()),
        "goal_gate_limit_exceeded"
    );

    let duplicate = GoalUserDecisionStateDto::AcceptedWithException {
        exception_evidence_set: vec![gate_exception(), gate_exception()],
    };
    assert_eq!(
        rejection_code(duplicate.validate()),
        "goal_acceptance_exception_invalid"
    );

    let mut inexact = gate_exception();
    inexact.gate_revision = 0;
    let inexact_set = GoalUserDecisionStateDto::AcceptedWithException {
        exception_evidence_set: vec![inexact],
    };
    assert_eq!(
        rejection_code(inexact_set.validate()),
        "goal_acceptance_exception_invalid"
    );
    Ok(())
}

/// Builds one durable Goal identity.
fn goal_record() -> GoalRecordDto {
    GoalRecordDto {
        goal_id: "goal-1".to_owned(),
        scope: GoalScopeDto::Project {
            project_id: "project-1".to_owned(),
        },
        active_revision: 1,
        lifecycle_state: GoalLifecycleStateDto::Active,
        readiness_state: GoalReadinessStateDto::NotReady,
        user_decision_state: GoalUserDecisionStateDto::Unaccepted,
        created_at_ms: 10,
        updated_at_ms: 10,
    }
}

/// Builds one immutable Goal revision record.
fn goal_revision() -> GoalRevisionRecordDto {
    GoalRevisionRecordDto {
        goal_id: "goal-1".to_owned(),
        revision: 1,
        title: "Ship the Goal surface".to_owned(),
        objective: "Deliver the DTO contracts".to_owned(),
        inherited_rule_references: vec!["rule-inherited".to_owned()],
        local_rule_references: vec!["rule-local".to_owned()],
        required_gate_references: vec![GoalGateRevisionReferenceDto {
            gate_id: "gate-1".to_owned(),
            revision: 1,
        }],
        canonical_revision_digest: digest(1),
        created_at_ms: 10,
    }
}

#[test]
fn goal_record_validates_identity_scope_revision_and_states() -> DtoResult<()> {
    let record = goal_record();
    assert_eq!(record.lifecycle_state.name(), "active");
    assert_eq!(record.scope.kind_name(), "project");
    record.validate()?;

    let mut blank = goal_record();
    blank.goal_id = " ".to_owned();
    assert_eq!(rejection_code(blank.validate()), "goal_not_active");

    let mut credential = goal_record();
    credential.goal_id = "Bearer live-token".to_owned();
    assert_eq!(
        rejection_code(credential.validate()),
        "credentials_forbidden"
    );

    let mut scope = goal_record();
    scope.scope = GoalScopeDto::Session {
        project_id: String::new(),
        session_id: "session-1".to_owned(),
    };
    assert_eq!(rejection_code(scope.validate()), "goal_not_active");

    let mut revision = goal_record();
    revision.active_revision = 0;
    assert_eq!(
        rejection_code(revision.validate()),
        "goal_revision_conflict"
    );

    let mut readiness = goal_record();
    readiness.readiness_state = GoalReadinessStateDto::Ready {
        verified_evidence_set: Vec::new(),
    };
    assert_eq!(rejection_code(readiness.validate()), "goal_not_ready");

    let mut decision = goal_record();
    decision.user_decision_state = GoalUserDecisionStateDto::AcceptedWithException {
        exception_evidence_set: Vec::new(),
    };
    assert_eq!(
        rejection_code(decision.validate()),
        "goal_acceptance_exception_invalid"
    );
    Ok(())
}

#[test]
fn goal_revision_validates_text_gates_digests_and_rules() -> DtoResult<()> {
    goal_revision().validate()?;

    let mut bounded = goal_revision();
    bounded.title = bounded_text();
    bounded.objective = bounded_text();
    bounded.validate()?;

    let mut blank_goal = goal_revision();
    blank_goal.goal_id = String::new();
    assert_eq!(rejection_code(blank_goal.validate()), "goal_not_active");

    let mut inexact = goal_revision();
    inexact.revision = 0;
    assert_eq!(rejection_code(inexact.validate()), "goal_revision_conflict");

    let mut blank_title = goal_revision();
    blank_title.title = "  ".to_owned();
    assert_eq!(
        rejection_code(blank_title.validate()),
        "goal_revision_conflict"
    );

    let mut oversized_title = goal_revision();
    oversized_title.title = oversized_text();
    assert_eq!(
        rejection_code(oversized_title.validate()),
        "goal_limit_exceeded"
    );

    let mut credential_title = goal_revision();
    credential_title.title = "token=live".to_owned();
    assert_eq!(
        rejection_code(credential_title.validate()),
        "credentials_forbidden"
    );

    let mut control_title = goal_revision();
    control_title.title = "line\nbreak".to_owned();
    assert_eq!(
        rejection_code(control_title.validate()),
        "goal_revision_conflict"
    );

    let mut blank_objective = goal_revision();
    blank_objective.objective = String::new();
    assert_eq!(
        rejection_code(blank_objective.validate()),
        "goal_revision_conflict"
    );

    let mut oversized_objective = goal_revision();
    oversized_objective.objective = oversized_text();
    assert_eq!(
        rejection_code(oversized_objective.validate()),
        "goal_limit_exceeded"
    );

    let mut credential_objective = goal_revision();
    credential_objective.objective = "secret plan".to_owned();
    assert_eq!(
        rejection_code(credential_objective.validate()),
        "credentials_forbidden"
    );

    let mut bad_digest = goal_revision();
    bad_digest.canonical_revision_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "goal_revision_conflict"
    );

    let mut at_bound = goal_revision();
    at_bound.required_gate_references = (0..MAX_GOAL_REQUIRED_GATES)
        .map(|index| GoalGateRevisionReferenceDto {
            gate_id: format!("gate-{index}"),
            revision: 1,
        })
        .collect();
    at_bound.validate()?;
    let mut over_bound = goal_revision();
    over_bound.required_gate_references = (0..=MAX_GOAL_REQUIRED_GATES)
        .map(|index| GoalGateRevisionReferenceDto {
            gate_id: format!("gate-{index}"),
            revision: 1,
        })
        .collect();
    assert_eq!(
        rejection_code(over_bound.validate()),
        "goal_gate_limit_exceeded"
    );

    let mut duplicate_gate = goal_revision();
    duplicate_gate.required_gate_references = vec![
        GoalGateRevisionReferenceDto {
            gate_id: "gate-1".to_owned(),
            revision: 1,
        },
        GoalGateRevisionReferenceDto {
            gate_id: "gate-1".to_owned(),
            revision: 2,
        },
    ];
    assert_eq!(
        rejection_code(duplicate_gate.validate()),
        "goal_revision_conflict"
    );

    let mut inexact_gate = goal_revision();
    inexact_gate.required_gate_references = vec![GoalGateRevisionReferenceDto {
        gate_id: "gate-1".to_owned(),
        revision: 0,
    }];
    assert_eq!(
        rejection_code(inexact_gate.validate()),
        "goal_revision_conflict"
    );

    let mut at_rule_bound = goal_revision();
    at_rule_bound.inherited_rule_references = (0..MAX_GOAL_RULE_REFERENCES)
        .map(|index| format!("rule-{index}"))
        .collect();
    at_rule_bound.local_rule_references = (0..MAX_GOAL_RULE_REFERENCES)
        .map(|index| format!("local-{index}"))
        .collect();
    at_rule_bound.validate()?;

    let mut inherited_bound = goal_revision();
    inherited_bound.inherited_rule_references = (0..=MAX_GOAL_RULE_REFERENCES)
        .map(|index| format!("rule-{index}"))
        .collect();
    assert_eq!(
        rejection_code(inherited_bound.validate()),
        "goal_not_active"
    );

    let mut local_bound = goal_revision();
    local_bound.local_rule_references = (0..=MAX_GOAL_RULE_REFERENCES)
        .map(|index| format!("local-{index}"))
        .collect();
    assert_eq!(rejection_code(local_bound.validate()), "goal_not_active");

    let mut blank_rule = goal_revision();
    blank_rule.inherited_rule_references = vec![" ".to_owned()];
    assert_eq!(rejection_code(blank_rule.validate()), "goal_not_active");

    let mut credential_rule = goal_revision();
    credential_rule.local_rule_references = vec!["password=hunter2".to_owned()];
    assert_eq!(
        rejection_code(credential_rule.validate()),
        "credentials_forbidden"
    );
    Ok(())
}

#[test]
fn goal_gate_revision_reference_requires_exact_revision() -> DtoResult<()> {
    GoalGateRevisionReferenceDto {
        gate_id: "gate-1".to_owned(),
        revision: 1,
    }
    .validate()?;

    let blank = GoalGateRevisionReferenceDto {
        gate_id: " ".to_owned(),
        revision: 1,
    };
    assert_eq!(rejection_code(blank.validate()), "goal_revision_conflict");
    let inexact = GoalGateRevisionReferenceDto {
        gate_id: "gate-1".to_owned(),
        revision: 0,
    };
    assert_eq!(rejection_code(inexact.validate()), "goal_revision_conflict");
    Ok(())
}

#[test]
fn create_goal_input_requires_matching_revision_one() -> DtoResult<()> {
    CreateGoalInputDto {
        goal: goal_record(),
        revision: goal_revision(),
    }
    .validate()?;

    let mut mismatched = goal_revision();
    mismatched.goal_id = "goal-2".to_owned();
    assert_eq!(
        rejection_code(
            CreateGoalInputDto {
                goal: goal_record(),
                revision: mismatched,
            }
            .validate()
        ),
        "goal_revision_conflict"
    );

    let mut second = goal_revision();
    second.revision = 2;
    assert_eq!(
        rejection_code(
            CreateGoalInputDto {
                goal: goal_record(),
                revision: second,
            }
            .validate()
        ),
        "goal_revision_conflict"
    );

    let mut advanced = goal_record();
    advanced.active_revision = 2;
    assert_eq!(
        rejection_code(
            CreateGoalInputDto {
                goal: advanced,
                revision: goal_revision(),
            }
            .validate()
        ),
        "goal_revision_conflict"
    );

    let mut blank_goal = goal_record();
    blank_goal.goal_id = String::new();
    assert_eq!(
        rejection_code(
            CreateGoalInputDto {
                goal: blank_goal,
                revision: goal_revision(),
            }
            .validate()
        ),
        "goal_not_active"
    );

    let mut blank_title = goal_revision();
    blank_title.title = String::new();
    assert_eq!(
        rejection_code(
            CreateGoalInputDto {
                goal: goal_record(),
                revision: blank_title,
            }
            .validate()
        ),
        "goal_revision_conflict"
    );
    Ok(())
}

#[test]
fn append_goal_revision_requires_the_next_exact_revision() -> DtoResult<()> {
    let mut next = goal_revision();
    next.revision = 2;
    AppendGoalRevisionInputDto {
        goal_id: "goal-1".to_owned(),
        expected_revision: 1,
        revision: next,
    }
    .validate()?;

    let mut mismatched = goal_revision();
    mismatched.revision = 2;
    mismatched.goal_id = "goal-2".to_owned();
    assert_eq!(
        rejection_code(
            AppendGoalRevisionInputDto {
                goal_id: "goal-1".to_owned(),
                expected_revision: 1,
                revision: mismatched,
            }
            .validate()
        ),
        "goal_revision_conflict"
    );

    let mut skipped = goal_revision();
    skipped.revision = 3;
    assert_eq!(
        rejection_code(
            AppendGoalRevisionInputDto {
                goal_id: "goal-1".to_owned(),
                expected_revision: 1,
                revision: skipped,
            }
            .validate()
        ),
        "goal_revision_conflict"
    );

    let mut overflow = goal_revision();
    overflow.revision = 2;
    assert_eq!(
        rejection_code(
            AppendGoalRevisionInputDto {
                goal_id: "goal-1".to_owned(),
                expected_revision: u64::MAX,
                revision: overflow,
            }
            .validate()
        ),
        "goal_revision_conflict"
    );

    let mut invalid = goal_revision();
    invalid.revision = 2;
    invalid.title = " ".to_owned();
    assert_eq!(
        rejection_code(
            AppendGoalRevisionInputDto {
                goal_id: "goal-1".to_owned(),
                expected_revision: 1,
                revision: invalid,
            }
            .validate()
        ),
        "goal_revision_conflict"
    );
    Ok(())
}

#[test]
fn goal_parent_link_validates_cycle_digest_and_revision() -> DtoResult<()> {
    GoalParentLinkRecordDto {
        parent_goal_id: "goal-1".to_owned(),
        child_goal_id: "goal-2".to_owned(),
        child_revision_at_link: 1,
        canonical_link_digest: digest(2),
        created_at_ms: 10,
    }
    .validate()?;

    let blank_parent = GoalParentLinkRecordDto {
        parent_goal_id: " ".to_owned(),
        child_goal_id: "goal-2".to_owned(),
        child_revision_at_link: 1,
        canonical_link_digest: digest(2),
        created_at_ms: 10,
    };
    assert_eq!(
        rejection_code(blank_parent.validate()),
        "goal_cycle_detected"
    );
    let blank_child = GoalParentLinkRecordDto {
        parent_goal_id: "goal-1".to_owned(),
        child_goal_id: String::new(),
        child_revision_at_link: 1,
        canonical_link_digest: digest(2),
        created_at_ms: 10,
    };
    assert_eq!(
        rejection_code(blank_child.validate()),
        "goal_cycle_detected"
    );
    let bad_digest = GoalParentLinkRecordDto {
        parent_goal_id: "goal-1".to_owned(),
        child_goal_id: "goal-2".to_owned(),
        child_revision_at_link: 1,
        canonical_link_digest: "sha256:xyz".to_owned(),
        created_at_ms: 10,
    };
    assert_eq!(rejection_code(bad_digest.validate()), "goal_cycle_detected");
    let self_link = GoalParentLinkRecordDto {
        parent_goal_id: "goal-1".to_owned(),
        child_goal_id: "goal-1".to_owned(),
        child_revision_at_link: 1,
        canonical_link_digest: digest(2),
        created_at_ms: 10,
    };
    assert_eq!(rejection_code(self_link.validate()), "goal_cycle_detected");
    let inexact_child = GoalParentLinkRecordDto {
        parent_goal_id: "goal-1".to_owned(),
        child_goal_id: "goal-2".to_owned(),
        child_revision_at_link: 0,
        canonical_link_digest: digest(2),
        created_at_ms: 10,
    };
    assert_eq!(
        rejection_code(inexact_child.validate()),
        "goal_revision_conflict"
    );
    Ok(())
}

#[test]
fn goal_session_link_validates_identities_digest_and_revision() -> DtoResult<()> {
    GoalSessionLinkRecordDto {
        link_id: "link-1".to_owned(),
        project_goal_id: "goal-1".to_owned(),
        session_id: "session-1".to_owned(),
        effective_from_revision: 1,
        canonical_link_digest: digest(3),
        created_at_ms: 10,
    }
    .validate()?;

    for blank in [
        GoalSessionLinkRecordDto {
            link_id: " ".to_owned(),
            project_goal_id: "goal-1".to_owned(),
            session_id: "session-1".to_owned(),
            effective_from_revision: 1,
            canonical_link_digest: digest(3),
            created_at_ms: 10,
        },
        GoalSessionLinkRecordDto {
            link_id: "link-1".to_owned(),
            project_goal_id: String::new(),
            session_id: "session-1".to_owned(),
            effective_from_revision: 1,
            canonical_link_digest: digest(3),
            created_at_ms: 10,
        },
        GoalSessionLinkRecordDto {
            link_id: "link-1".to_owned(),
            project_goal_id: "goal-1".to_owned(),
            session_id: " ".to_owned(),
            effective_from_revision: 1,
            canonical_link_digest: digest(3),
            created_at_ms: 10,
        },
    ] {
        assert_eq!(rejection_code(blank.validate()), "goal_not_active");
    }

    let bad_digest = GoalSessionLinkRecordDto {
        link_id: "link-1".to_owned(),
        project_goal_id: "goal-1".to_owned(),
        session_id: "session-1".to_owned(),
        effective_from_revision: 1,
        canonical_link_digest: "md5:abc".to_owned(),
        created_at_ms: 10,
    };
    assert_eq!(rejection_code(bad_digest.validate()), "goal_not_active");
    let inexact = GoalSessionLinkRecordDto {
        link_id: "link-1".to_owned(),
        project_goal_id: "goal-1".to_owned(),
        session_id: "session-1".to_owned(),
        effective_from_revision: 0,
        canonical_link_digest: digest(3),
        created_at_ms: 10,
    };
    assert_eq!(rejection_code(inexact.validate()), "goal_revision_conflict");
    Ok(())
}

#[test]
fn goal_gate_definition_variants_validate() -> DtoResult<()> {
    let reference = GoalGateDefinitionDto::Reference {
        evidence_contract_revision: 1,
        accepted_reference_kinds: vec![
            GoalEvidenceKindDto::TerminalChildResult,
            GoalEvidenceKindDto::ExecutableGateResult,
        ],
    };
    assert_eq!(reference.kind_name(), "reference");
    assert!(!reference.is_executable());
    reference.validate()?;

    let executable = GoalGateDefinitionDto::Executable {
        template_id: "template-1".to_owned(),
        template_revision: 1,
    };
    assert_eq!(executable.kind_name(), "executable");
    assert!(executable.is_executable());
    executable.validate()?;

    let zero_contract = GoalGateDefinitionDto::Reference {
        evidence_contract_revision: 0,
        accepted_reference_kinds: vec![GoalEvidenceKindDto::TerminalChildResult],
    };
    assert_eq!(
        rejection_code(zero_contract.validate()),
        "goal_gate_unavailable"
    );
    let empty_kinds = GoalGateDefinitionDto::Reference {
        evidence_contract_revision: 1,
        accepted_reference_kinds: Vec::new(),
    };
    assert_eq!(
        rejection_code(empty_kinds.validate()),
        "goal_gate_unavailable"
    );
    let duplicate_kinds = GoalGateDefinitionDto::Reference {
        evidence_contract_revision: 1,
        accepted_reference_kinds: vec![
            GoalEvidenceKindDto::AcceptedUserDeclaration,
            GoalEvidenceKindDto::AcceptedUserDeclaration,
        ],
    };
    assert_eq!(
        rejection_code(duplicate_kinds.validate()),
        "goal_gate_unavailable"
    );
    let blank_template = GoalGateDefinitionDto::Executable {
        template_id: " ".to_owned(),
        template_revision: 1,
    };
    assert_eq!(
        rejection_code(blank_template.validate()),
        "goal_gate_unavailable"
    );
    let inexact_template = GoalGateDefinitionDto::Executable {
        template_id: "template-1".to_owned(),
        template_revision: 0,
    };
    assert_eq!(
        rejection_code(inexact_template.validate()),
        "goal_gate_unavailable"
    );
    Ok(())
}

/// Builds one durable gate record.
fn gate_record() -> GoalGateRecordDto {
    GoalGateRecordDto {
        gate_id: "gate-1".to_owned(),
        goal_id: "goal-1".to_owned(),
        definition: GoalGateDefinitionDto::Executable {
            template_id: "template-1".to_owned(),
            template_revision: 1,
        },
        revision: 1,
        canonical_revision_digest: digest(4),
        created_at_ms: 10,
    }
}

#[test]
fn goal_gate_record_validates_identity_revision_digest_and_definition() -> DtoResult<()> {
    gate_record().validate()?;

    let mut blank_gate = gate_record();
    blank_gate.gate_id = " ".to_owned();
    assert_eq!(
        rejection_code(blank_gate.validate()),
        "goal_gate_unavailable"
    );
    let mut blank_goal = gate_record();
    blank_goal.goal_id = String::new();
    assert_eq!(
        rejection_code(blank_goal.validate()),
        "goal_gate_unavailable"
    );
    let mut bad_digest = gate_record();
    bad_digest.canonical_revision_digest = "sha256:abc".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "goal_gate_unavailable"
    );
    let mut inexact = gate_record();
    inexact.revision = 0;
    assert_eq!(rejection_code(inexact.validate()), "goal_revision_conflict");
    let mut invalid_definition = gate_record();
    invalid_definition.definition = GoalGateDefinitionDto::Reference {
        evidence_contract_revision: 1,
        accepted_reference_kinds: Vec::new(),
    };
    assert_eq!(
        rejection_code(invalid_definition.validate()),
        "goal_gate_unavailable"
    );
    Ok(())
}

#[test]
fn append_goal_gate_revision_validates_gate_digest_and_definition() -> DtoResult<()> {
    AppendGoalGateRevisionInputDto {
        gate_id: "gate-1".to_owned(),
        expected_revision: 1,
        definition: GoalGateDefinitionDto::Executable {
            template_id: "template-1".to_owned(),
            template_revision: 1,
        },
        canonical_revision_digest: digest(5),
        occurred_at_ms: 10,
    }
    .validate()?;

    let blank_gate = AppendGoalGateRevisionInputDto {
        gate_id: " ".to_owned(),
        expected_revision: 1,
        definition: GoalGateDefinitionDto::Executable {
            template_id: "template-1".to_owned(),
            template_revision: 1,
        },
        canonical_revision_digest: digest(5),
        occurred_at_ms: 10,
    };
    assert_eq!(
        rejection_code(blank_gate.validate()),
        "goal_revision_conflict"
    );
    let bad_digest = AppendGoalGateRevisionInputDto {
        gate_id: "gate-1".to_owned(),
        expected_revision: 1,
        definition: GoalGateDefinitionDto::Executable {
            template_id: "template-1".to_owned(),
            template_revision: 1,
        },
        canonical_revision_digest: "sha256:xyz".to_owned(),
        occurred_at_ms: 10,
    };
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "goal_revision_conflict"
    );
    let invalid_definition = AppendGoalGateRevisionInputDto {
        gate_id: "gate-1".to_owned(),
        expected_revision: 1,
        definition: GoalGateDefinitionDto::Executable {
            template_id: String::new(),
            template_revision: 1,
        },
        canonical_revision_digest: digest(5),
        occurred_at_ms: 10,
    };
    assert_eq!(
        rejection_code(invalid_definition.validate()),
        "goal_gate_unavailable"
    );
    let overflow = AppendGoalGateRevisionInputDto {
        gate_id: "gate-1".to_owned(),
        expected_revision: u64::MAX,
        definition: GoalGateDefinitionDto::Executable {
            template_id: "template-1".to_owned(),
            template_revision: 1,
        },
        canonical_revision_digest: digest(5),
        occurred_at_ms: 10,
    };
    assert_eq!(
        rejection_code(overflow.validate()),
        "goal_revision_conflict"
    );
    Ok(())
}

#[test]
fn goal_template_scope_variants_expose_owner() -> DtoResult<()> {
    let project = GoalTemplateScopeDto::Project {
        project_id: "project-1".to_owned(),
    };
    assert_eq!(project.kind_name(), "project");
    assert_eq!(project.owner_id(), "project-1");
    project.validate()?;

    let goal = GoalTemplateScopeDto::Goal {
        goal_id: "goal-1".to_owned(),
    };
    assert_eq!(goal.kind_name(), "goal");
    assert_eq!(goal.owner_id(), "goal-1");
    goal.validate()?;

    let session = GoalTemplateScopeDto::Session {
        session_id: "session-1".to_owned(),
    };
    assert_eq!(session.kind_name(), "session");
    assert_eq!(session.owner_id(), "session-1");
    session.validate()?;

    let blank = GoalTemplateScopeDto::Session {
        session_id: " ".to_owned(),
    };
    assert_eq!(rejection_code(blank.validate()), "goal_gate_unavailable");
    let credential = GoalTemplateScopeDto::Goal {
        goal_id: "api_key=live".to_owned(),
    };
    assert_eq!(
        rejection_code(credential.validate()),
        "credentials_forbidden"
    );
    Ok(())
}

#[test]
fn goal_gate_input_family_names_and_parse() -> DtoResult<()> {
    let families = [
        (GoalGateInputFamilyDto::ClosedTextV1, "closed_text_v1"),
        (
            GoalGateInputFamilyDto::ClosedPathSetV1,
            "closed_path_set_v1",
        ),
    ];
    for (family, name) in families {
        assert_eq!(family.name(), name);
        assert_eq!(GoalGateInputFamilyDto::parse(name)?, family);
    }
    assert_eq!(
        rejection_code(GoalGateInputFamilyDto::parse("unknown")),
        "goal_gate_unavailable"
    );
    Ok(())
}

#[test]
fn goal_template_lifecycle_names_and_parse() -> DtoResult<()> {
    let states = [
        (GoalTemplateLifecycleStateDto::Enabled, "enabled"),
        (GoalTemplateLifecycleStateDto::Archived, "archived"),
    ];
    for (state, name) in states {
        assert_eq!(state.name(), name);
        assert_eq!(GoalTemplateLifecycleStateDto::parse(name)?, state);
    }
    assert_eq!(
        rejection_code(GoalTemplateLifecycleStateDto::parse("unknown")),
        "goal_gate_unavailable"
    );
    Ok(())
}

#[test]
fn goal_template_provenance_names_and_parse() -> DtoResult<()> {
    let kinds = [
        (GoalTemplateProvenanceKindDto::User, "user"),
        (
            GoalTemplateProvenanceKindDto::ModelProposal,
            "model_proposal",
        ),
    ];
    for (kind, name) in kinds {
        assert_eq!(kind.name(), name);
        assert_eq!(GoalTemplateProvenanceKindDto::parse(name)?, kind);
    }
    assert_eq!(
        rejection_code(GoalTemplateProvenanceKindDto::parse("unknown")),
        "goal_gate_unavailable"
    );
    Ok(())
}

/// Builds one user-created gate template card.
fn gate_template() -> GoalGateTemplateRecordDto {
    GoalGateTemplateRecordDto {
        template_id: "template-1".to_owned(),
        revision: 1,
        scope: GoalTemplateScopeDto::Project {
            project_id: "project-1".to_owned(),
        },
        capability_reference: "capability-1".to_owned(),
        input_family: GoalGateInputFamilyDto::ClosedTextV1,
        requires_confirmation: false,
        lifecycle_state: GoalTemplateLifecycleStateDto::Enabled,
        provenance_kind: GoalTemplateProvenanceKindDto::User,
        provenance_draft_id: None,
        provenance_accepted_by_user: false,
        canonical_digest: digest(6),
        created_at_ms: 10,
    }
}

#[test]
fn goal_gate_template_validates_identity_provenance_and_scope() -> DtoResult<()> {
    gate_template().validate()?;

    let mut proposed = gate_template();
    proposed.provenance_kind = GoalTemplateProvenanceKindDto::ModelProposal;
    proposed.provenance_draft_id = Some("draft-1".to_owned());
    proposed.provenance_accepted_by_user = true;
    proposed.validate()?;

    let mut user_with_draft = gate_template();
    user_with_draft.provenance_draft_id = Some("draft-1".to_owned());
    assert_eq!(
        rejection_code(user_with_draft.validate()),
        "goal_gate_unavailable"
    );

    let mut proposal_without_draft = gate_template();
    proposal_without_draft.provenance_kind = GoalTemplateProvenanceKindDto::ModelProposal;
    proposal_without_draft.provenance_accepted_by_user = true;
    assert_eq!(
        rejection_code(proposal_without_draft.validate()),
        "goal_gate_unavailable"
    );

    let mut proposal_blank_draft = gate_template();
    proposal_blank_draft.provenance_kind = GoalTemplateProvenanceKindDto::ModelProposal;
    proposal_blank_draft.provenance_draft_id = Some(" ".to_owned());
    proposal_blank_draft.provenance_accepted_by_user = true;
    assert_eq!(
        rejection_code(proposal_blank_draft.validate()),
        "goal_gate_unavailable"
    );

    let mut proposal_unconfirmed = gate_template();
    proposal_unconfirmed.provenance_kind = GoalTemplateProvenanceKindDto::ModelProposal;
    proposal_unconfirmed.provenance_draft_id = Some("draft-1".to_owned());
    assert_eq!(
        rejection_code(proposal_unconfirmed.validate()),
        "goal_gate_unavailable"
    );

    let mut credential_proposal = gate_template();
    credential_proposal.provenance_kind = GoalTemplateProvenanceKindDto::ModelProposal;
    credential_proposal.provenance_draft_id = Some("password=hunter2".to_owned());
    credential_proposal.provenance_accepted_by_user = true;
    assert_eq!(
        rejection_code(credential_proposal.validate()),
        "credentials_forbidden"
    );

    let mut blank_template = gate_template();
    blank_template.template_id = " ".to_owned();
    assert_eq!(
        rejection_code(blank_template.validate()),
        "goal_gate_unavailable"
    );
    let mut blank_capability = gate_template();
    blank_capability.capability_reference = String::new();
    assert_eq!(
        rejection_code(blank_capability.validate()),
        "goal_gate_unavailable"
    );
    let mut bad_digest = gate_template();
    bad_digest.canonical_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "goal_gate_unavailable"
    );
    let mut inexact = gate_template();
    inexact.revision = 0;
    assert_eq!(rejection_code(inexact.validate()), "goal_gate_unavailable");
    let mut invalid_scope = gate_template();
    invalid_scope.scope = GoalTemplateScopeDto::Project {
        project_id: " ".to_owned(),
    };
    assert_eq!(
        rejection_code(invalid_scope.validate()),
        "goal_gate_unavailable"
    );
    Ok(())
}

#[test]
fn goal_gate_outcome_kinds_map_names_dispositions_and_evidence() -> DtoResult<()> {
    let outcomes = [
        (
            GoalGateOutcomeKindDto::Passed,
            "passed",
            GoalGateOutcomeDispositionDto::Passed,
            true,
        ),
        (
            GoalGateOutcomeKindDto::Failed,
            "failed",
            GoalGateOutcomeDispositionDto::Failed,
            true,
        ),
        (
            GoalGateOutcomeKindDto::TimedOut,
            "timed_out",
            GoalGateOutcomeDispositionDto::Failed,
            false,
        ),
        (
            GoalGateOutcomeKindDto::Cancelled,
            "cancelled",
            GoalGateOutcomeDispositionDto::Failed,
            false,
        ),
        (
            GoalGateOutcomeKindDto::OutputBoundExceeded,
            "output_bound_exceeded",
            GoalGateOutcomeDispositionDto::Failed,
            false,
        ),
        (
            GoalGateOutcomeKindDto::ExternalEffectUnknown,
            "external_effect_unknown",
            GoalGateOutcomeDispositionDto::UnknownEffect,
            true,
        ),
        (
            GoalGateOutcomeKindDto::ReferenceUnavailable,
            "reference_unavailable",
            GoalGateOutcomeDispositionDto::Unavailable,
            false,
        ),
        (
            GoalGateOutcomeKindDto::RevisionStale,
            "revision_stale",
            GoalGateOutcomeDispositionDto::Unavailable,
            false,
        ),
        (
            GoalGateOutcomeKindDto::TemplateUnavailable,
            "template_unavailable",
            GoalGateOutcomeDispositionDto::Unavailable,
            false,
        ),
    ];
    for (kind, name, disposition, requires_evidence) in outcomes {
        assert_eq!(kind.name(), name);
        assert_eq!(GoalGateOutcomeKindDto::parse(name)?, kind);
        assert_eq!(kind.disposition(), disposition);
        assert_eq!(kind.requires_evidence(), requires_evidence);
    }
    assert_eq!(
        rejection_code(GoalGateOutcomeKindDto::parse("unknown")),
        "goal_gate_failed"
    );
    Ok(())
}

#[test]
fn goal_gate_outcome_disposition_names_and_parse() -> DtoResult<()> {
    let dispositions = [
        (GoalGateOutcomeDispositionDto::Passed, "passed"),
        (GoalGateOutcomeDispositionDto::Failed, "failed"),
        (GoalGateOutcomeDispositionDto::Unavailable, "unavailable"),
        (
            GoalGateOutcomeDispositionDto::UnknownEffect,
            "unknown_effect",
        ),
    ];
    for (disposition, name) in dispositions {
        assert_eq!(disposition.name(), name);
        assert_eq!(GoalGateOutcomeDispositionDto::parse(name)?, disposition);
    }
    assert_eq!(
        rejection_code(GoalGateOutcomeDispositionDto::parse("unknown")),
        "goal_gate_failed"
    );
    Ok(())
}

/// Builds one durable passing gate result.
fn gate_result() -> GoalGateResultRecordDto {
    GoalGateResultRecordDto {
        gate_id: "gate-1".to_owned(),
        gate_revision: 1,
        producing_run_id: "run-1".to_owned(),
        outcome_kind: GoalGateOutcomeKindDto::Passed,
        disposition: GoalGateOutcomeDispositionDto::Passed,
        evidence: Some(evidence_reference()),
        canonical_result_digest: digest(7),
        occurred_at_ms: 10,
    }
}

#[test]
fn goal_gate_result_validates_disposition_and_evidence_binding() -> DtoResult<()> {
    gate_result().validate()?;

    let mut failing = gate_result();
    failing.outcome_kind = GoalGateOutcomeKindDto::Failed;
    failing.disposition = GoalGateOutcomeDispositionDto::Failed;
    failing.validate()?;

    let mut unknown_effect = gate_result();
    unknown_effect.outcome_kind = GoalGateOutcomeKindDto::ExternalEffectUnknown;
    unknown_effect.disposition = GoalGateOutcomeDispositionDto::UnknownEffect;
    unknown_effect.validate()?;

    let mut timed_out = gate_result();
    timed_out.outcome_kind = GoalGateOutcomeKindDto::TimedOut;
    timed_out.disposition = GoalGateOutcomeDispositionDto::Failed;
    timed_out.evidence = None;
    timed_out.validate()?;

    let mut unavailable = gate_result();
    unavailable.outcome_kind = GoalGateOutcomeKindDto::TemplateUnavailable;
    unavailable.disposition = GoalGateOutcomeDispositionDto::Unavailable;
    unavailable.evidence = None;
    unavailable.validate()?;

    let mut blank_gate = gate_result();
    blank_gate.gate_id = " ".to_owned();
    assert_eq!(rejection_code(blank_gate.validate()), "goal_gate_failed");

    let mut blank_run = gate_result();
    blank_run.producing_run_id = String::new();
    assert_eq!(rejection_code(blank_run.validate()), "goal_gate_failed");

    let mut bad_digest = gate_result();
    bad_digest.canonical_result_digest = "sha256:xyz".to_owned();
    assert_eq!(rejection_code(bad_digest.validate()), "goal_gate_failed");

    let mut inexact_revision = gate_result();
    inexact_revision.gate_revision = 0;
    assert_eq!(
        rejection_code(inexact_revision.validate()),
        "goal_revision_conflict"
    );

    let mut mismatched_disposition = gate_result();
    mismatched_disposition.disposition = GoalGateOutcomeDispositionDto::Failed;
    assert_eq!(
        rejection_code(mismatched_disposition.validate()),
        "goal_gate_failed"
    );

    let mut unexpected_evidence = gate_result();
    unexpected_evidence.outcome_kind = GoalGateOutcomeKindDto::Cancelled;
    unexpected_evidence.disposition = GoalGateOutcomeDispositionDto::Failed;
    assert_eq!(
        rejection_code(unexpected_evidence.validate()),
        "goal_gate_failed"
    );

    let mut missing_evidence = gate_result();
    missing_evidence.evidence = None;
    assert_eq!(
        rejection_code(missing_evidence.validate()),
        "goal_gate_failed"
    );

    let mut inexact_evidence = gate_result();
    inexact_evidence.evidence = Some(GoalEvidenceReferenceDto {
        evidence_id: "evidence-1".to_owned(),
        revision: 0,
        kind: GoalEvidenceKindDto::TerminalChildResult,
    });
    assert_eq!(
        rejection_code(inexact_evidence.validate()),
        "goal_gate_failed"
    );

    let mut credential_evidence = gate_result();
    credential_evidence.evidence = Some(GoalEvidenceReferenceDto {
        evidence_id: "password=hunter2".to_owned(),
        revision: 1,
        kind: GoalEvidenceKindDto::TerminalChildResult,
    });
    assert_eq!(
        rejection_code(credential_evidence.validate()),
        "credentials_forbidden"
    );
    Ok(())
}

#[test]
fn memory_kind_names_and_parse() -> DtoResult<()> {
    let kinds = [
        (MemoryKindDto::Fact, "fact"),
        (MemoryKindDto::Decision, "decision"),
        (MemoryKindDto::Preference, "preference"),
        (MemoryKindDto::PastFailure, "past_failure"),
    ];
    for (kind, name) in kinds {
        assert_eq!(kind.name(), name);
        assert_eq!(MemoryKindDto::parse(name)?, kind);
    }
    assert_eq!(
        rejection_code(MemoryKindDto::parse("unknown")),
        "memory_reference_unavailable"
    );
    Ok(())
}

/// Builds one bounded durable memory card revision.
fn memory_card() -> GoalMemoryCardRecordDto {
    GoalMemoryCardRecordDto {
        record_id: "memory-1".to_owned(),
        revision: 1,
        kind: MemoryKindDto::Fact,
        scope: GoalRecordScopeDto::Goal {
            goal_id: "goal-1".to_owned(),
        },
        title: "Retain the accepted decision".to_owned(),
        safe_purpose: "Explain the retained durable fact".to_owned(),
        retained_content_reference: "content-1".to_owned(),
        canonical_digest: digest(8),
        created_at_ms: 10,
    }
}

#[test]
fn goal_memory_card_validates_identity_revision_scope_and_text() -> DtoResult<()> {
    memory_card().validate()?;

    let mut bounded = memory_card();
    bounded.title = bounded_text();
    bounded.safe_purpose = bounded_text();
    bounded.validate()?;

    let mut blank_record = memory_card();
    blank_record.record_id = " ".to_owned();
    assert_eq!(
        rejection_code(blank_record.validate()),
        "memory_reference_unavailable"
    );
    let mut blank_reference = memory_card();
    blank_reference.retained_content_reference = String::new();
    assert_eq!(
        rejection_code(blank_reference.validate()),
        "memory_reference_unavailable"
    );
    let mut bad_digest = memory_card();
    bad_digest.canonical_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "memory_reference_unavailable"
    );
    let mut inexact = memory_card();
    inexact.revision = 0;
    assert_eq!(
        rejection_code(inexact.validate()),
        "memory_reference_unavailable"
    );
    let mut invalid_scope = memory_card();
    invalid_scope.scope = GoalRecordScopeDto::Session {
        session_id: " ".to_owned(),
    };
    assert_eq!(
        rejection_code(invalid_scope.validate()),
        "memory_reference_unavailable"
    );
    let mut blank_title = memory_card();
    blank_title.title = String::new();
    assert_eq!(
        rejection_code(blank_title.validate()),
        "memory_reference_unavailable"
    );
    let mut oversized_title = memory_card();
    oversized_title.title = oversized_text();
    assert_eq!(
        rejection_code(oversized_title.validate()),
        "memory_entry_too_large"
    );
    let mut blank_purpose = memory_card();
    blank_purpose.safe_purpose = " ".to_owned();
    assert_eq!(
        rejection_code(blank_purpose.validate()),
        "memory_reference_unavailable"
    );
    let mut oversized_purpose = memory_card();
    oversized_purpose.safe_purpose = oversized_text();
    assert_eq!(
        rejection_code(oversized_purpose.validate()),
        "memory_entry_too_large"
    );
    let mut credential_title = memory_card();
    credential_title.title = "token=live".to_owned();
    assert_eq!(
        rejection_code(credential_title.validate()),
        "credentials_forbidden"
    );
    let mut credential_purpose = memory_card();
    credential_purpose.safe_purpose = "api_key=live".to_owned();
    assert_eq!(
        rejection_code(credential_purpose.validate()),
        "credentials_forbidden"
    );
    Ok(())
}

#[test]
fn goal_memory_card_replacement_validates_relation() -> DtoResult<()> {
    let mut replacement = memory_card();
    replacement.revision = 2;
    GoalMemoryCardReplacementInputDto {
        replaced_record_id: "memory-1".to_owned(),
        replaced_revision: 1,
        replacement,
        occurred_at_ms: 20,
    }
    .validate()?;

    GoalMemoryCardReplacementInputDto {
        replaced_record_id: "memory-0".to_owned(),
        replaced_revision: 5,
        replacement: memory_card(),
        occurred_at_ms: 20,
    }
    .validate()?;

    let blank_record = GoalMemoryCardReplacementInputDto {
        replaced_record_id: " ".to_owned(),
        replaced_revision: 1,
        replacement: memory_card(),
        occurred_at_ms: 20,
    };
    assert_eq!(
        rejection_code(blank_record.validate()),
        "memory_replacement_conflict"
    );

    let zero_replaced = GoalMemoryCardReplacementInputDto {
        replaced_record_id: "memory-1".to_owned(),
        replaced_revision: 0,
        replacement: memory_card(),
        occurred_at_ms: 20,
    };
    assert_eq!(
        rejection_code(zero_replaced.validate()),
        "memory_replacement_conflict"
    );

    let mut zero_replacement = memory_card();
    zero_replacement.revision = 0;
    let zero_revision = GoalMemoryCardReplacementInputDto {
        replaced_record_id: "memory-1".to_owned(),
        replaced_revision: 1,
        replacement: zero_replacement,
        occurred_at_ms: 20,
    };
    assert_eq!(
        rejection_code(zero_revision.validate()),
        "memory_replacement_conflict"
    );

    let mut invalid_card = memory_card();
    invalid_card.title = String::new();
    let invalid = GoalMemoryCardReplacementInputDto {
        replaced_record_id: "memory-1".to_owned(),
        replaced_revision: 1,
        replacement: invalid_card,
        occurred_at_ms: 20,
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "memory_reference_unavailable"
    );

    let mut equal = memory_card();
    equal.revision = 1;
    let same_revision = GoalMemoryCardReplacementInputDto {
        replaced_record_id: "memory-1".to_owned(),
        replaced_revision: 1,
        replacement: equal,
        occurred_at_ms: 20,
    };
    assert_eq!(
        rejection_code(same_revision.validate()),
        "memory_replacement_conflict"
    );

    let older = GoalMemoryCardReplacementInputDto {
        replaced_record_id: "memory-1".to_owned(),
        replaced_revision: 2,
        replacement: memory_card(),
        occurred_at_ms: 20,
    };
    assert_eq!(
        rejection_code(older.validate()),
        "memory_replacement_conflict"
    );
    Ok(())
}

#[test]
fn goal_memory_card_rollback_validates_relation() -> DtoResult<()> {
    let mut restored = memory_card();
    restored.revision = 3;
    GoalMemoryCardRollbackInputDto {
        restored_record_id: "memory-1".to_owned(),
        restored_revision: 2,
        replacement: restored,
        occurred_at_ms: 20,
    }
    .validate()?;

    let blank_record = GoalMemoryCardRollbackInputDto {
        restored_record_id: " ".to_owned(),
        restored_revision: 2,
        replacement: memory_card(),
        occurred_at_ms: 20,
    };
    assert_eq!(
        rejection_code(blank_record.validate()),
        "memory_replacement_conflict"
    );

    let zero_restored = GoalMemoryCardRollbackInputDto {
        restored_record_id: "memory-1".to_owned(),
        restored_revision: 0,
        replacement: memory_card(),
        occurred_at_ms: 20,
    };
    assert_eq!(
        rejection_code(zero_restored.validate()),
        "memory_replacement_conflict"
    );

    let mut zero_replacement = memory_card();
    zero_replacement.revision = 0;
    let zero_revision = GoalMemoryCardRollbackInputDto {
        restored_record_id: "memory-1".to_owned(),
        restored_revision: 2,
        replacement: zero_replacement,
        occurred_at_ms: 20,
    };
    assert_eq!(
        rejection_code(zero_revision.validate()),
        "memory_replacement_conflict"
    );

    let mut invalid_card = memory_card();
    invalid_card.safe_purpose = String::new();
    let invalid = GoalMemoryCardRollbackInputDto {
        restored_record_id: "memory-1".to_owned(),
        restored_revision: 2,
        replacement: invalid_card,
        occurred_at_ms: 20,
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "memory_reference_unavailable"
    );

    let different_record = GoalMemoryCardRollbackInputDto {
        restored_record_id: "memory-0".to_owned(),
        restored_revision: 2,
        replacement: memory_card(),
        occurred_at_ms: 20,
    };
    assert_eq!(
        rejection_code(different_record.validate()),
        "memory_replacement_conflict"
    );

    let older_revision = GoalMemoryCardRollbackInputDto {
        restored_record_id: "memory-1".to_owned(),
        restored_revision: 3,
        replacement: memory_card(),
        occurred_at_ms: 20,
    };
    assert_eq!(
        rejection_code(older_revision.validate()),
        "memory_replacement_conflict"
    );

    let mut equal = memory_card();
    equal.revision = 2;
    let same_revision = GoalMemoryCardRollbackInputDto {
        restored_record_id: "memory-1".to_owned(),
        restored_revision: 2,
        replacement: equal,
        occurred_at_ms: 20,
    };
    assert_eq!(
        rejection_code(same_revision.validate()),
        "memory_replacement_conflict"
    );
    Ok(())
}

/// Builds one bounded durable Skill card revision.
fn skill_card() -> GoalSkillCardRecordDto {
    GoalSkillCardRecordDto {
        skill_id: "skill-1".to_owned(),
        revision: 1,
        canonical_name: "safe-review".to_owned(),
        description: "Routes safe review work".to_owned(),
        owner_scope: GoalRecordScopeDto::Project {
            project_id: "project-1".to_owned(),
        },
        content_reference: "content-1".to_owned(),
        canonical_digest: digest(9),
        created_at_ms: 10,
    }
}

#[test]
fn goal_skill_card_validates_name_shape_and_text() -> DtoResult<()> {
    skill_card().validate()?;

    let mut digit_leading = skill_card();
    digit_leading.canonical_name = "1abc-2".to_owned();
    digit_leading.validate()?;

    let mut bounded_name = skill_card();
    bounded_name.canonical_name = "a".repeat(MAX_GOAL_SAFE_TEXT_BYTES);
    bounded_name.validate()?;

    for invalid_name in [
        "-leading-dash",
        "Uppercase",
        "under_score",
        "trailing-",
        "dot.name",
        "space name",
    ] {
        let mut invalid = skill_card();
        invalid.canonical_name = invalid_name.to_owned();
        assert_eq!(
            rejection_code(invalid.validate()),
            "skill_reference_unavailable"
        );
    }

    let mut blank_name = skill_card();
    blank_name.canonical_name = String::new();
    assert_eq!(
        rejection_code(blank_name.validate()),
        "skill_reference_unavailable"
    );

    let mut oversized_name = skill_card();
    oversized_name.canonical_name = oversized_text();
    assert_eq!(
        rejection_code(oversized_name.validate()),
        "skill_entry_too_large"
    );

    let mut blank_skill = skill_card();
    blank_skill.skill_id = " ".to_owned();
    assert_eq!(
        rejection_code(blank_skill.validate()),
        "skill_reference_unavailable"
    );
    let mut blank_reference = skill_card();
    blank_reference.content_reference = String::new();
    assert_eq!(
        rejection_code(blank_reference.validate()),
        "skill_reference_unavailable"
    );
    let mut bad_digest = skill_card();
    bad_digest.canonical_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "skill_reference_unavailable"
    );
    let mut inexact = skill_card();
    inexact.revision = 0;
    assert_eq!(
        rejection_code(inexact.validate()),
        "skill_reference_unavailable"
    );
    let mut blank_description = skill_card();
    blank_description.description = " ".to_owned();
    assert_eq!(
        rejection_code(blank_description.validate()),
        "skill_reference_unavailable"
    );
    let mut oversized_description = skill_card();
    oversized_description.description = oversized_text();
    assert_eq!(
        rejection_code(oversized_description.validate()),
        "skill_entry_too_large"
    );
    let mut credential_name = skill_card();
    credential_name.canonical_name = "sk-live".to_owned();
    assert_eq!(
        rejection_code(credential_name.validate()),
        "credentials_forbidden"
    );
    let mut invalid_scope = skill_card();
    invalid_scope.owner_scope = GoalRecordScopeDto::Goal {
        goal_id: String::new(),
    };
    assert_eq!(
        rejection_code(invalid_scope.validate()),
        "skill_reference_unavailable"
    );
    Ok(())
}

#[test]
fn goal_role_class_names_parse_and_ranks() -> DtoResult<()> {
    let classes = [
        (GoalRoleClassDto::Light, "light", 0),
        (GoalRoleClassDto::Medium, "medium", 1),
        (GoalRoleClassDto::Heavy, "heavy", 2),
    ];
    for (class, name, rank) in classes {
        assert_eq!(class.name(), name);
        assert_eq!(GoalRoleClassDto::parse(name)?, class);
        assert_eq!(class.rank(), rank);
    }
    assert_eq!(
        rejection_code(GoalRoleClassDto::parse("unknown")),
        "delegation_role_invalid"
    );
    Ok(())
}

/// Builds one bounded durable role card revision.
fn role_card() -> GoalRoleCardRecordDto {
    GoalRoleCardRecordDto {
        role_id: "role-1".to_owned(),
        revision: 1,
        canonical_name: "primary".to_owned(),
        task: "Narrow the delegated task".to_owned(),
        permitted_class: GoalRoleClassDto::Medium,
        tool_subset: vec!["read".to_owned(), "grep".to_owned()],
        context_limit_bytes: 1024,
        result_limit_bytes: 1024,
        canonical_digest: digest(10),
        created_at_ms: 10,
    }
}

#[test]
fn goal_role_card_validates_identity_text_limits_and_tools() -> DtoResult<()> {
    role_card().validate()?;

    let mut at_bound = role_card();
    at_bound.tool_subset = (0..MAX_GOAL_ROLE_TOOLS)
        .map(|index| format!("tool-{index}"))
        .collect();
    at_bound.validate()?;

    let mut blank_role = role_card();
    blank_role.role_id = " ".to_owned();
    assert_eq!(
        rejection_code(blank_role.validate()),
        "delegation_role_invalid"
    );
    let mut bad_digest = role_card();
    bad_digest.canonical_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "delegation_role_invalid"
    );
    let mut inexact = role_card();
    inexact.revision = 0;
    assert_eq!(
        rejection_code(inexact.validate()),
        "delegation_role_invalid"
    );
    let mut blank_name = role_card();
    blank_name.canonical_name = String::new();
    assert_eq!(
        rejection_code(blank_name.validate()),
        "delegation_role_invalid"
    );
    let mut oversized_name = role_card();
    oversized_name.canonical_name = oversized_text();
    assert_eq!(
        rejection_code(oversized_name.validate()),
        "delegation_role_invalid"
    );
    let mut blank_task = role_card();
    blank_task.task = " ".to_owned();
    assert_eq!(
        rejection_code(blank_task.validate()),
        "delegation_role_invalid"
    );
    let mut oversized_task = role_card();
    oversized_task.task = oversized_text();
    assert_eq!(
        rejection_code(oversized_task.validate()),
        "delegation_role_invalid"
    );
    let mut zero_context = role_card();
    zero_context.context_limit_bytes = 0;
    assert_eq!(
        rejection_code(zero_context.validate()),
        "delegation_role_invalid"
    );
    let mut zero_result = role_card();
    zero_result.result_limit_bytes = 0;
    assert_eq!(
        rejection_code(zero_result.validate()),
        "delegation_role_invalid"
    );
    let mut over_bound = role_card();
    over_bound.tool_subset = (0..=MAX_GOAL_ROLE_TOOLS)
        .map(|index| format!("tool-{index}"))
        .collect();
    assert_eq!(
        rejection_code(over_bound.validate()),
        "delegation_role_invalid"
    );
    let mut duplicate_tools = role_card();
    duplicate_tools.tool_subset = vec!["read".to_owned(), "read".to_owned()];
    assert_eq!(
        rejection_code(duplicate_tools.validate()),
        "delegation_role_invalid"
    );
    let mut blank_tool = role_card();
    blank_tool.tool_subset = vec![" ".to_owned()];
    assert_eq!(
        rejection_code(blank_tool.validate()),
        "delegation_role_invalid"
    );
    let mut credential_task = role_card();
    credential_task.task = "password=hunter2".to_owned();
    assert_eq!(
        rejection_code(credential_task.validate()),
        "credentials_forbidden"
    );
    Ok(())
}

#[test]
fn goal_milestone_names_and_parse() -> DtoResult<()> {
    let milestones = [
        (GoalMilestoneDto::TechnicalReadiness, "technical_readiness"),
        (GoalMilestoneDto::UserAcceptance, "user_acceptance"),
        (
            GoalMilestoneDto::AcceptanceWithException,
            "acceptance_with_exception",
        ),
        (GoalMilestoneDto::Stop, "stop"),
        (
            GoalMilestoneDto::RequiredGateFailure,
            "required_gate_failure",
        ),
        (
            GoalMilestoneDto::ObligatoryChildTerminalOutcome,
            "obligatory_child_terminal_outcome",
        ),
    ];
    for (milestone, name) in milestones {
        assert_eq!(milestone.name(), name);
        assert_eq!(GoalMilestoneDto::parse(name)?, milestone);
    }
    assert_eq!(
        rejection_code(GoalMilestoneDto::parse("unknown")),
        "refinement_draft_conflict"
    );
    Ok(())
}

#[test]
fn refinement_edit_kind_names_and_parse() -> DtoResult<()> {
    let kinds = [
        (RefinementEditKindDto::ReadinessClaim, "readiness_claim"),
        (
            RefinementEditKindDto::AcceptanceDecision,
            "acceptance_decision",
        ),
        (RefinementEditKindDto::ExceptionSet, "exception_set"),
        (RefinementEditKindDto::StopDecision, "stop_decision"),
        (
            RefinementEditKindDto::RequiredGateEvidence,
            "required_gate_evidence",
        ),
        (
            RefinementEditKindDto::ChildOutcomeReference,
            "child_outcome_reference",
        ),
    ];
    for (kind, name) in kinds {
        assert_eq!(kind.name(), name);
        assert_eq!(RefinementEditKindDto::parse(name)?, kind);
    }
    assert_eq!(
        rejection_code(RefinementEditKindDto::parse("unknown")),
        "refinement_draft_conflict"
    );
    Ok(())
}

#[test]
fn refinement_draft_state_names_and_parse() -> DtoResult<()> {
    let states = [
        (RefinementDraftStateDto::Pending, "pending"),
        (RefinementDraftStateDto::Accepted, "accepted"),
        (RefinementDraftStateDto::Rejected, "rejected"),
    ];
    for (state, name) in states {
        assert_eq!(state.name(), name);
        assert_eq!(RefinementDraftStateDto::parse(name)?, state);
    }
    assert_eq!(
        rejection_code(RefinementDraftStateDto::parse("unknown")),
        "refinement_draft_conflict"
    );
    Ok(())
}

/// Builds one pending coalesced refinement draft.
fn refinement_draft() -> RefinementDraftRecordDto {
    RefinementDraftRecordDto {
        draft_id: "draft-1".to_owned(),
        source_run_id: "run-1".to_owned(),
        leading_goal_id: "goal-1".to_owned(),
        milestone: GoalMilestoneDto::TechnicalReadiness,
        base_goal_revision: 1,
        base_record_reference: "record-1".to_owned(),
        base_record_revision: 1,
        edits: vec![RefinementEditRecordDto {
            kind: RefinementEditKindDto::ReadinessClaim,
            evidence: evidence_reference(),
        }],
        evidence_references: vec![evidence_reference()],
        safe_rationale: "Coalesces the readiness evidence".to_owned(),
        canonical_digest: digest(11),
        state: RefinementDraftStateDto::Pending,
        coalesced_evidence_count: 1,
        created_at_ms: 10,
        decided_at_ms: None,
    }
}

#[test]
fn refinement_draft_validates_edits_evidence_and_decision_time() -> DtoResult<()> {
    refinement_draft().validate()?;

    let mut accepted = refinement_draft();
    accepted.state = RefinementDraftStateDto::Accepted;
    accepted.decided_at_ms = Some(30);
    accepted.validate()?;

    let mut rejected = refinement_draft();
    rejected.state = RefinementDraftStateDto::Rejected;
    rejected.decided_at_ms = Some(30);
    rejected.validate()?;

    let mut at_bound = refinement_draft();
    at_bound.edits = (0..MAX_GOAL_DRAFT_EDITS)
        .map(|index| RefinementEditRecordDto {
            kind: RefinementEditKindDto::RequiredGateEvidence,
            evidence: GoalEvidenceReferenceDto {
                evidence_id: format!("evidence-{index}"),
                revision: 1,
                kind: GoalEvidenceKindDto::ExecutableGateResult,
            },
        })
        .collect();
    at_bound.evidence_references = (0..MAX_GOAL_EVIDENCE_REFERENCES)
        .map(|index| GoalEvidenceReferenceDto {
            evidence_id: format!("evidence-{index}"),
            revision: 1,
            kind: GoalEvidenceKindDto::TerminalChildResult,
        })
        .collect();
    at_bound.validate()?;

    for blank in [
        RefinementDraftRecordDto {
            draft_id: " ".to_owned(),
            ..refinement_draft()
        },
        RefinementDraftRecordDto {
            source_run_id: String::new(),
            ..refinement_draft()
        },
        RefinementDraftRecordDto {
            leading_goal_id: " ".to_owned(),
            ..refinement_draft()
        },
        RefinementDraftRecordDto {
            base_record_reference: String::new(),
            ..refinement_draft()
        },
    ] {
        assert_eq!(
            rejection_code(blank.validate()),
            "refinement_draft_conflict"
        );
    }

    let mut bad_digest = refinement_draft();
    bad_digest.canonical_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "refinement_draft_conflict"
    );

    let mut zero_goal_base = refinement_draft();
    zero_goal_base.base_goal_revision = 0;
    assert_eq!(
        rejection_code(zero_goal_base.validate()),
        "refinement_draft_conflict"
    );
    let mut zero_record_base = refinement_draft();
    zero_record_base.base_record_revision = 0;
    assert_eq!(
        rejection_code(zero_record_base.validate()),
        "refinement_draft_conflict"
    );

    let mut no_edits = refinement_draft();
    no_edits.edits = Vec::new();
    assert_eq!(
        rejection_code(no_edits.validate()),
        "refinement_draft_conflict"
    );

    let mut too_many_edits = refinement_draft();
    too_many_edits.edits = (0..=MAX_GOAL_DRAFT_EDITS)
        .map(|index| RefinementEditRecordDto {
            kind: RefinementEditKindDto::ExceptionSet,
            evidence: GoalEvidenceReferenceDto {
                evidence_id: format!("evidence-{index}"),
                revision: 1,
                kind: GoalEvidenceKindDto::ExecutableGateResult,
            },
        })
        .collect();
    assert_eq!(
        rejection_code(too_many_edits.validate()),
        "goal_limit_exceeded"
    );

    let mut too_many_evidence = refinement_draft();
    too_many_evidence.evidence_references = (0..=MAX_GOAL_EVIDENCE_REFERENCES)
        .map(|index| GoalEvidenceReferenceDto {
            evidence_id: format!("evidence-{index}"),
            revision: 1,
            kind: GoalEvidenceKindDto::TerminalChildResult,
        })
        .collect();
    assert_eq!(
        rejection_code(too_many_evidence.validate()),
        "goal_limit_exceeded"
    );

    let mut inexact_edit = refinement_draft();
    inexact_edit.edits[0].evidence.revision = 0;
    assert_eq!(
        rejection_code(inexact_edit.validate()),
        "refinement_draft_conflict"
    );

    let mut duplicate_evidence = refinement_draft();
    duplicate_evidence.evidence_references = vec![evidence_reference(), evidence_reference()];
    assert_eq!(
        rejection_code(duplicate_evidence.validate()),
        "refinement_draft_conflict"
    );

    let mut blank_rationale = refinement_draft();
    blank_rationale.safe_rationale = " ".to_owned();
    assert_eq!(
        rejection_code(blank_rationale.validate()),
        "refinement_draft_conflict"
    );
    let mut oversized_rationale = refinement_draft();
    oversized_rationale.safe_rationale = oversized_text();
    assert_eq!(
        rejection_code(oversized_rationale.validate()),
        "refinement_draft_too_large"
    );
    let mut credential_rationale = refinement_draft();
    credential_rationale.safe_rationale = "token=live".to_owned();
    assert_eq!(
        rejection_code(credential_rationale.validate()),
        "credentials_forbidden"
    );

    let mut pending_decided = refinement_draft();
    pending_decided.decided_at_ms = Some(30);
    assert_eq!(
        rejection_code(pending_decided.validate()),
        "refinement_draft_conflict"
    );
    let mut accepted_undecided = refinement_draft();
    accepted_undecided.state = RefinementDraftStateDto::Accepted;
    assert_eq!(
        rejection_code(accepted_undecided.validate()),
        "refinement_draft_conflict"
    );
    let mut rejected_undecided = refinement_draft();
    rejected_undecided.state = RefinementDraftStateDto::Rejected;
    assert_eq!(
        rejection_code(rejected_undecided.validate()),
        "refinement_draft_conflict"
    );
    Ok(())
}

/// Builds one immutable conversation summary revision.
fn conversation_summary() -> ConversationSummaryRecordDto {
    ConversationSummaryRecordDto {
        summary_id: "summary-1".to_owned(),
        revision: 1,
        scope: GoalRecordScopeDto::Session {
            session_id: "session-1".to_owned(),
        },
        previous_summary_reference: None,
        source_range_start: "history-1".to_owned(),
        source_range_end: "history-2".to_owned(),
        safe_content: "Compacted the completed range".to_owned(),
        canonical_digest: digest(12),
        created_at_ms: 10,
    }
}

#[test]
fn conversation_summary_validates_identity_scope_range_and_content() -> DtoResult<()> {
    conversation_summary().validate()?;

    let mut chained = conversation_summary();
    chained.previous_summary_reference = Some("summary-0".to_owned());
    chained.validate()?;

    let mut bounded = conversation_summary();
    bounded.safe_content = bounded_text();
    bounded.validate()?;

    let mut blank_summary = conversation_summary();
    blank_summary.summary_id = " ".to_owned();
    assert_eq!(
        rejection_code(blank_summary.validate()),
        "compaction_summary_unavailable"
    );
    let mut bad_digest = conversation_summary();
    bad_digest.canonical_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "compaction_summary_unavailable"
    );
    let mut inexact = conversation_summary();
    inexact.revision = 0;
    assert_eq!(
        rejection_code(inexact.validate()),
        "compaction_summary_unavailable"
    );
    let mut invalid_scope = conversation_summary();
    invalid_scope.scope = GoalRecordScopeDto::Project {
        project_id: String::new(),
    };
    assert_eq!(
        rejection_code(invalid_scope.validate()),
        "compaction_summary_unavailable"
    );
    let mut blank_previous = conversation_summary();
    blank_previous.previous_summary_reference = Some(" ".to_owned());
    assert_eq!(
        rejection_code(blank_previous.validate()),
        "compaction_summary_unavailable"
    );
    let mut blank_start = conversation_summary();
    blank_start.source_range_start = String::new();
    assert_eq!(
        rejection_code(blank_start.validate()),
        "compaction_history_unavailable"
    );
    let mut blank_end = conversation_summary();
    blank_end.source_range_end = " ".to_owned();
    assert_eq!(
        rejection_code(blank_end.validate()),
        "compaction_history_unavailable"
    );
    let mut blank_content = conversation_summary();
    blank_content.safe_content = " ".to_owned();
    assert_eq!(
        rejection_code(blank_content.validate()),
        "compaction_summary_unavailable"
    );
    let mut oversized_content = conversation_summary();
    oversized_content.safe_content = oversized_text();
    assert_eq!(
        rejection_code(oversized_content.validate()),
        "compaction_summary_too_large"
    );
    let mut credential_content = conversation_summary();
    credential_content.safe_content = "password=hunter2".to_owned();
    assert_eq!(
        rejection_code(credential_content.validate()),
        "credentials_forbidden"
    );
    Ok(())
}

/// Builds one exact authority revision reference.
fn authority_reference() -> VerifierAuthorityReferenceDto {
    VerifierAuthorityReferenceDto {
        authority_id: "authority-1".to_owned(),
        authority_revision: 1,
        canonical_authority_digest: digest(13),
    }
}

/// Builds one exact target-set reference.
fn target_set_reference() -> VerifierTargetSetReferenceDto {
    VerifierTargetSetReferenceDto {
        target_set_id: "target-set-1".to_owned(),
        canonical_target_set_digest: digest(14),
    }
}

/// Builds one exact contract revision reference.
fn contract_reference() -> VerifierContractReferenceDto {
    VerifierContractReferenceDto {
        contract_id: "contract-1".to_owned(),
        contract_revision: 1,
        canonical_contract_digest: digest(15),
    }
}

/// Builds one exact target revision reference.
fn target_reference() -> VerifierTargetReferenceDto {
    VerifierTargetReferenceDto {
        target_mandate_id: "mandate-1".to_owned(),
        target_revision: 1,
    }
}

/// Builds one frozen Goal reference.
fn goal_reference() -> VerifierGoalReferenceDto {
    VerifierGoalReferenceDto {
        goal_id: "goal-1".to_owned(),
        goal_revision: 1,
        canonical_goal_revision_digest: digest(16),
    }
}

/// Builds one coherent frozen reference set.
fn frozen_references() -> VerifierFrozenReferencesDto {
    VerifierFrozenReferencesDto {
        goal_references: vec![goal_reference()],
        contract_references: vec![contract_reference()],
    }
}

#[test]
fn verifier_authority_reference_validates_identity_digest_revision() -> DtoResult<()> {
    authority_reference().validate()?;

    let mut blank = authority_reference();
    blank.authority_id = " ".to_owned();
    assert_eq!(
        rejection_code(blank.validate()),
        "verifier_authority_invalid"
    );
    let mut bad_digest = authority_reference();
    bad_digest.canonical_authority_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "verifier_authority_invalid"
    );
    let mut inexact = authority_reference();
    inexact.authority_revision = 0;
    assert_eq!(
        rejection_code(inexact.validate()),
        "verifier_authority_revision_mismatch"
    );
    Ok(())
}

#[test]
fn verifier_target_set_reference_validates_identity_and_digest() -> DtoResult<()> {
    target_set_reference().validate()?;

    let mut blank = target_set_reference();
    blank.target_set_id = String::new();
    assert_eq!(
        rejection_code(blank.validate()),
        "verifier_authority_invalid"
    );
    let mut bad_digest = target_set_reference();
    bad_digest.canonical_target_set_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "verifier_authority_invalid"
    );
    Ok(())
}

#[test]
fn verifier_contract_reference_validates_identity_digest_revision() -> DtoResult<()> {
    contract_reference().validate()?;

    let mut blank = contract_reference();
    blank.contract_id = " ".to_owned();
    assert_eq!(
        rejection_code(blank.validate()),
        "verifier_authority_invalid"
    );
    let mut bad_digest = contract_reference();
    bad_digest.canonical_contract_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "verifier_authority_invalid"
    );
    let mut inexact = contract_reference();
    inexact.contract_revision = 0;
    assert_eq!(
        rejection_code(inexact.validate()),
        "verifier_authority_contract_mismatch"
    );
    Ok(())
}

#[test]
fn verifier_target_reference_validates_identity_and_revision() -> DtoResult<()> {
    target_reference().validate()?;

    let mut blank = target_reference();
    blank.target_mandate_id = String::new();
    assert_eq!(
        rejection_code(blank.validate()),
        "verifier_target_lifecycle_invalid"
    );
    let mut inexact = target_reference();
    inexact.target_revision = 0;
    assert_eq!(
        rejection_code(inexact.validate()),
        "verifier_baseline_stale_target_revision"
    );
    Ok(())
}

#[test]
fn verifier_goal_reference_validates_identity_digest_revision() -> DtoResult<()> {
    goal_reference().validate()?;

    let mut blank = goal_reference();
    blank.goal_id = " ".to_owned();
    assert_eq!(
        rejection_code(blank.validate()),
        "verifier_baseline_stale_target_identity"
    );
    let mut bad_digest = goal_reference();
    bad_digest.canonical_goal_revision_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "verifier_baseline_stale_target_identity"
    );
    let mut inexact = goal_reference();
    inexact.goal_revision = 0;
    assert_eq!(
        rejection_code(inexact.validate()),
        "verifier_baseline_stale_target_revision"
    );
    Ok(())
}

#[test]
fn verifier_frozen_references_validate_bounds_and_nested() -> DtoResult<()> {
    VerifierFrozenReferencesDto {
        goal_references: Vec::new(),
        contract_references: Vec::new(),
    }
    .validate()?;

    let at_goal_bound = VerifierFrozenReferencesDto {
        goal_references: (0..MAX_VERIFIER_FROZEN_GOAL_REFERENCES)
            .map(|index| VerifierGoalReferenceDto {
                goal_id: format!("goal-{index}"),
                goal_revision: 1,
                canonical_goal_revision_digest: digest(16),
            })
            .collect(),
        contract_references: Vec::new(),
    };
    at_goal_bound.validate()?;
    let at_contract_bound = VerifierFrozenReferencesDto {
        goal_references: Vec::new(),
        contract_references: (0..MAX_VERIFIER_FROZEN_CONTRACT_REFERENCES)
            .map(|index| VerifierContractReferenceDto {
                contract_id: format!("contract-{index}"),
                contract_revision: 1,
                canonical_contract_digest: digest(15),
            })
            .collect(),
    };
    at_contract_bound.validate()?;

    let over_goal_bound = VerifierFrozenReferencesDto {
        goal_references: (0..=MAX_VERIFIER_FROZEN_GOAL_REFERENCES)
            .map(|index| VerifierGoalReferenceDto {
                goal_id: format!("goal-{index}"),
                goal_revision: 1,
                canonical_goal_revision_digest: digest(16),
            })
            .collect(),
        contract_references: Vec::new(),
    };
    assert_eq!(
        rejection_code(over_goal_bound.validate()),
        "verifier_authority_invalid"
    );
    let over_contract_bound = VerifierFrozenReferencesDto {
        goal_references: Vec::new(),
        contract_references: (0..=MAX_VERIFIER_FROZEN_CONTRACT_REFERENCES)
            .map(|index| VerifierContractReferenceDto {
                contract_id: format!("contract-{index}"),
                contract_revision: 1,
                canonical_contract_digest: digest(15),
            })
            .collect(),
    };
    assert_eq!(
        rejection_code(over_contract_bound.validate()),
        "verifier_authority_invalid"
    );

    let mut inexact_goal = goal_reference();
    inexact_goal.goal_revision = 0;
    let nested_goal = VerifierFrozenReferencesDto {
        goal_references: vec![inexact_goal],
        contract_references: Vec::new(),
    };
    assert_eq!(
        rejection_code(nested_goal.validate()),
        "verifier_baseline_stale_target_revision"
    );
    let mut blank_contract = contract_reference();
    blank_contract.contract_id = String::new();
    let nested_contract = VerifierFrozenReferencesDto {
        goal_references: Vec::new(),
        contract_references: vec![blank_contract],
    };
    assert_eq!(
        rejection_code(nested_contract.validate()),
        "verifier_authority_invalid"
    );
    Ok(())
}

#[test]
fn verifier_operation_names_and_parse() -> DtoResult<()> {
    let operations = [
        (VerifierOperationDto::MarkNeedsRework, "mark_needs_rework"),
        (VerifierOperationDto::MarkComplete, "mark_complete"),
        (VerifierOperationDto::Stop, "stop"),
        (VerifierOperationDto::ReviseFull, "revise_full"),
        (
            VerifierOperationDto::ResolveUnknownEffect,
            "resolve_unknown_effect",
        ),
    ];
    for (operation, name) in operations {
        assert_eq!(operation.name(), name);
        assert_eq!(VerifierOperationDto::parse(name)?, operation);
    }
    assert_eq!(
        rejection_code(VerifierOperationDto::parse("unknown")),
        "verifier_authority_operation_not_allowed"
    );
    Ok(())
}

#[test]
fn verifier_target_lifecycle_names_parse_and_terminals() -> DtoResult<()> {
    let lifecycles = [
        (VerifierTargetLifecycleDto::Draft, "draft", false),
        (VerifierTargetLifecycleDto::Active, "active", false),
        (VerifierTargetLifecycleDto::Working, "working", false),
        (VerifierTargetLifecycleDto::Paused, "paused", false),
        (
            VerifierTargetLifecycleDto::PausedAwaitingDecision,
            "paused_awaiting_decision",
            false,
        ),
        (
            VerifierTargetLifecycleDto::NeedsRework,
            "needs_rework",
            false,
        ),
        (VerifierTargetLifecycleDto::Completed, "completed", true),
        (VerifierTargetLifecycleDto::Stopped, "stopped", true),
        (VerifierTargetLifecycleDto::Archived, "archived", true),
    ];
    for (lifecycle, name, terminal) in lifecycles {
        assert_eq!(lifecycle.name(), name);
        assert_eq!(VerifierTargetLifecycleDto::parse(name)?, lifecycle);
        assert_eq!(lifecycle.is_terminal(), terminal);
    }
    assert_eq!(
        rejection_code(VerifierTargetLifecycleDto::parse("unknown")),
        "verifier_target_lifecycle_invalid"
    );
    Ok(())
}

#[test]
fn verifier_evidence_kind_names_and_parse() -> DtoResult<()> {
    let kinds = [
        (
            VerifierEvidenceKindDto::UnconditionalPass,
            "unconditional_pass",
        ),
        (VerifierEvidenceKindDto::QualifyingFail, "qualifying_fail"),
        (VerifierEvidenceKindDto::Inconclusive, "inconclusive"),
        (
            VerifierEvidenceKindDto::GraphTerminalizationClosure,
            "graph_terminalization_closure",
        ),
        (
            VerifierEvidenceKindDto::ReconciliationStandardProof,
            "reconciliation_standard_proof",
        ),
    ];
    for (kind, name) in kinds {
        assert_eq!(kind.name(), name);
        assert_eq!(VerifierEvidenceKindDto::parse(name)?, kind);
    }
    assert_eq!(
        rejection_code(VerifierEvidenceKindDto::parse("unknown")),
        "verifier_qualifying_fail_evidence_missing"
    );
    Ok(())
}

#[test]
fn verification_audit_verdict_names_and_parse() -> DtoResult<()> {
    let verdicts = [
        (VerificationAuditVerdictDto::Pass, "pass"),
        (VerificationAuditVerdictDto::Fail, "fail"),
        (VerificationAuditVerdictDto::Inconclusive, "inconclusive"),
        (
            VerificationAuditVerdictDto::TargetRevisionStale,
            "target_revision_stale",
        ),
        (
            VerificationAuditVerdictDto::TargetUnavailable,
            "target_unavailable",
        ),
        (
            VerificationAuditVerdictDto::VerifierUnavailable,
            "verifier_unavailable",
        ),
        (
            VerificationAuditVerdictDto::VerifierExternalEffectUnknown,
            "verifier_external_effect_unknown",
        ),
    ];
    for (verdict, name) in verdicts {
        assert_eq!(verdict.name(), name);
        assert_eq!(VerificationAuditVerdictDto::parse(name)?, verdict);
    }
    assert_eq!(
        rejection_code(VerificationAuditVerdictDto::parse("unknown")),
        "verifier_selection_unsupported"
    );
    Ok(())
}

#[test]
fn verifier_authority_consumption_rule_names_and_parse() -> DtoResult<()> {
    let rules = [
        (VerifierAuthorityConsumptionRuleDto::SingleUse, "single_use"),
        (
            VerifierAuthorityConsumptionRuleDto::ReusableWhileActive,
            "reusable_while_active",
        ),
    ];
    for (rule, name) in rules {
        assert_eq!(rule.name(), name);
        assert_eq!(VerifierAuthorityConsumptionRuleDto::parse(name)?, rule);
    }
    assert_eq!(
        rejection_code(VerifierAuthorityConsumptionRuleDto::parse("unknown")),
        "verifier_authority_invalid"
    );
    Ok(())
}

#[test]
fn verifier_authority_consumption_state_names_and_parse() -> DtoResult<()> {
    let states = [
        (
            VerifierAuthorityConsumptionStateDto::Unconsumed,
            "unconsumed",
        ),
        (VerifierAuthorityConsumptionStateDto::Consumed, "consumed"),
    ];
    for (state, name) in states {
        assert_eq!(state.name(), name);
        assert_eq!(VerifierAuthorityConsumptionStateDto::parse(name)?, state);
    }
    assert_eq!(
        rejection_code(VerifierAuthorityConsumptionStateDto::parse("unknown")),
        "verifier_authority_invalid"
    );
    Ok(())
}

/// Builds one active single-use delegated verifier authority.
fn authority_record() -> VerifierAuthorityRecordDto {
    VerifierAuthorityRecordDto {
        authority_id: "authority-1".to_owned(),
        authority_revision: 1,
        verifier_mandate_id: "mandate-verifier-1".to_owned(),
        immutable_target_set_reference: target_set_reference(),
        allowed_operations: vec![VerifierOperationDto::MarkComplete],
        audit_contract_reference: contract_reference(),
        issued_at_ms: 100,
        expires_at_ms: None,
        revoked_at_ms: None,
        revocation_reference: None,
        consumption_rule: VerifierAuthorityConsumptionRuleDto::SingleUse,
        consumption_state: VerifierAuthorityConsumptionStateDto::Unconsumed,
        consumed_by_mutation_reference: None,
        canonical_authority_digest: digest(17),
    }
}

#[test]
fn verifier_authority_record_validates_operations_lifecycle_and_consumption() -> DtoResult<()> {
    authority_record().validate()?;

    let mut all_operations = authority_record();
    all_operations.allowed_operations = vec![
        VerifierOperationDto::MarkNeedsRework,
        VerifierOperationDto::MarkComplete,
        VerifierOperationDto::Stop,
        VerifierOperationDto::ReviseFull,
        VerifierOperationDto::ResolveUnknownEffect,
    ];
    all_operations.validate()?;

    let mut revoked = authority_record();
    revoked.revoked_at_ms = Some(150);
    revoked.revocation_reference = Some("revocation-1".to_owned());
    revoked.validate()?;

    let mut expiring = authority_record();
    expiring.expires_at_ms = Some(200);
    expiring.validate()?;

    let mut consumed = authority_record();
    consumed.consumption_state = VerifierAuthorityConsumptionStateDto::Consumed;
    consumed.consumed_by_mutation_reference = Some("mutation-1".to_owned());
    consumed.validate()?;

    let mut blank_authority = authority_record();
    blank_authority.authority_id = " ".to_owned();
    assert_eq!(
        rejection_code(blank_authority.validate()),
        "verifier_authority_invalid"
    );
    let mut blank_mandate = authority_record();
    blank_mandate.verifier_mandate_id = String::new();
    assert_eq!(
        rejection_code(blank_mandate.validate()),
        "verifier_authority_invalid"
    );
    let mut bad_digest = authority_record();
    bad_digest.canonical_authority_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "verifier_authority_invalid"
    );
    let mut inexact = authority_record();
    inexact.authority_revision = 0;
    assert_eq!(
        rejection_code(inexact.validate()),
        "verifier_authority_revision_mismatch"
    );
    let mut invalid_target_set = authority_record();
    invalid_target_set.immutable_target_set_reference = VerifierTargetSetReferenceDto {
        target_set_id: " ".to_owned(),
        canonical_target_set_digest: digest(14),
    };
    assert_eq!(
        rejection_code(invalid_target_set.validate()),
        "verifier_authority_invalid"
    );
    let mut invalid_contract = authority_record();
    invalid_contract.audit_contract_reference = VerifierContractReferenceDto {
        contract_id: "contract-1".to_owned(),
        contract_revision: 0,
        canonical_contract_digest: digest(15),
    };
    assert_eq!(
        rejection_code(invalid_contract.validate()),
        "verifier_authority_contract_mismatch"
    );
    let mut empty_operations = authority_record();
    empty_operations.allowed_operations = Vec::new();
    assert_eq!(
        rejection_code(empty_operations.validate()),
        "verifier_authority_invalid"
    );
    let mut six_operations = authority_record();
    six_operations.allowed_operations = vec![
        VerifierOperationDto::MarkNeedsRework,
        VerifierOperationDto::MarkComplete,
        VerifierOperationDto::Stop,
        VerifierOperationDto::ReviseFull,
        VerifierOperationDto::ResolveUnknownEffect,
        VerifierOperationDto::Stop,
    ];
    assert_eq!(
        rejection_code(six_operations.validate()),
        "verifier_authority_invalid"
    );
    let mut duplicate_operations = authority_record();
    duplicate_operations.allowed_operations = vec![
        VerifierOperationDto::MarkComplete,
        VerifierOperationDto::MarkComplete,
    ];
    assert_eq!(
        rejection_code(duplicate_operations.validate()),
        "verifier_authority_invalid"
    );
    let mut time_without_reference = authority_record();
    time_without_reference.revoked_at_ms = Some(150);
    assert_eq!(
        rejection_code(time_without_reference.validate()),
        "verifier_authority_revoked"
    );
    let mut reference_without_time = authority_record();
    reference_without_time.revocation_reference = Some("revocation-1".to_owned());
    assert_eq!(
        rejection_code(reference_without_time.validate()),
        "verifier_authority_revoked"
    );
    let mut early_revocation = authority_record();
    early_revocation.revoked_at_ms = Some(50);
    early_revocation.revocation_reference = Some("revocation-1".to_owned());
    assert_eq!(
        rejection_code(early_revocation.validate()),
        "verifier_authority_invalid"
    );
    let mut zero_window = authority_record();
    zero_window.expires_at_ms = Some(100);
    assert_eq!(
        rejection_code(zero_window.validate()),
        "verifier_authority_invalid"
    );
    let mut past_expiry = authority_record();
    past_expiry.expires_at_ms = Some(50);
    assert_eq!(
        rejection_code(past_expiry.validate()),
        "verifier_authority_invalid"
    );
    let mut unconsumed_with_mutation = authority_record();
    unconsumed_with_mutation.consumed_by_mutation_reference = Some("mutation-1".to_owned());
    assert_eq!(
        rejection_code(unconsumed_with_mutation.validate()),
        "verifier_authority_invalid"
    );
    let mut consumed_without_reference = authority_record();
    consumed_without_reference.consumption_state = VerifierAuthorityConsumptionStateDto::Consumed;
    assert_eq!(
        rejection_code(consumed_without_reference.validate()),
        "verifier_authority_invalid"
    );
    let mut consumed_blank_reference = authority_record();
    consumed_blank_reference.consumption_state = VerifierAuthorityConsumptionStateDto::Consumed;
    consumed_blank_reference.consumed_by_mutation_reference = Some(" ".to_owned());
    assert_eq!(
        rejection_code(consumed_blank_reference.validate()),
        "verifier_authority_invalid"
    );
    let mut reusable_consumed = authority_record();
    reusable_consumed.consumption_state = VerifierAuthorityConsumptionStateDto::Consumed;
    reusable_consumed.consumed_by_mutation_reference = Some("mutation-1".to_owned());
    reusable_consumed.consumption_rule = VerifierAuthorityConsumptionRuleDto::ReusableWhileActive;
    assert_eq!(
        rejection_code(reusable_consumed.validate()),
        "verifier_authority_consumed"
    );
    Ok(())
}

#[test]
fn revoke_verifier_authority_validates_identities_and_revision() -> DtoResult<()> {
    RevokeVerifierAuthorityInputDto {
        authority_id: "authority-1".to_owned(),
        authority_revision: 1,
        revocation_reference: "revocation-1".to_owned(),
        revoked_at_ms: 150,
    }
    .validate()?;

    let blank_authority = RevokeVerifierAuthorityInputDto {
        authority_id: " ".to_owned(),
        authority_revision: 1,
        revocation_reference: "revocation-1".to_owned(),
        revoked_at_ms: 150,
    };
    assert_eq!(
        rejection_code(blank_authority.validate()),
        "verifier_authority_invalid"
    );
    let blank_reference = RevokeVerifierAuthorityInputDto {
        authority_id: "authority-1".to_owned(),
        authority_revision: 1,
        revocation_reference: String::new(),
        revoked_at_ms: 150,
    };
    assert_eq!(
        rejection_code(blank_reference.validate()),
        "verifier_authority_invalid"
    );
    let inexact = RevokeVerifierAuthorityInputDto {
        authority_id: "authority-1".to_owned(),
        authority_revision: 0,
        revocation_reference: "revocation-1".to_owned(),
        revoked_at_ms: 150,
    };
    assert_eq!(
        rejection_code(inexact.validate()),
        "verifier_authority_revision_mismatch"
    );
    Ok(())
}

/// Builds one immutable verifier audit baseline.
fn audit_baseline() -> VerifierAuditBaselineRecordDto {
    VerifierAuditBaselineRecordDto {
        authority_reference: authority_reference(),
        verifier_mandate_revision: 1,
        target_mandate_id: "mandate-1".to_owned(),
        target_revision: 1,
        target_sequence: 1,
        target_lifecycle: VerifierTargetLifecycleDto::Active,
        frozen_references: frozen_references(),
        optional_unknown_effect_reference: None,
        audit_contract_reference: contract_reference(),
        graph_epoch: Some(1),
        canonical_baseline_digest: digest(18),
        created_at_ms: 10,
    }
}

#[test]
fn verifier_audit_baseline_validates_digest_revisions_and_nested() -> DtoResult<()> {
    audit_baseline().validate()?;

    let mut uncertain = audit_baseline();
    uncertain.optional_unknown_effect_reference = Some("unknown-effect-1".to_owned());
    uncertain.validate()?;

    let mut blank_mandate = audit_baseline();
    blank_mandate.target_mandate_id = " ".to_owned();
    assert_eq!(
        rejection_code(blank_mandate.validate()),
        "verifier_baseline_invalid"
    );
    let mut bad_digest = audit_baseline();
    bad_digest.canonical_baseline_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "verifier_baseline_invalid"
    );
    let mut zero_mandate_revision = audit_baseline();
    zero_mandate_revision.verifier_mandate_revision = 0;
    assert_eq!(
        rejection_code(zero_mandate_revision.validate()),
        "verifier_baseline_invalid"
    );
    let mut zero_target_revision = audit_baseline();
    zero_target_revision.target_revision = 0;
    assert_eq!(
        rejection_code(zero_target_revision.validate()),
        "verifier_baseline_stale_target_revision"
    );
    let mut blank_unknown_effect = audit_baseline();
    blank_unknown_effect.optional_unknown_effect_reference = Some(" ".to_owned());
    assert_eq!(
        rejection_code(blank_unknown_effect.validate()),
        "verifier_baseline_invalid"
    );
    let mut inexact_authority = audit_baseline();
    inexact_authority.authority_reference.authority_revision = 0;
    assert_eq!(
        rejection_code(inexact_authority.validate()),
        "verifier_authority_revision_mismatch"
    );
    let mut inexact_contract = audit_baseline();
    inexact_contract.audit_contract_reference.contract_revision = 0;
    assert_eq!(
        rejection_code(inexact_contract.validate()),
        "verifier_authority_contract_mismatch"
    );
    let mut inexact_frozen = audit_baseline();
    inexact_frozen.frozen_references.goal_references[0].goal_revision = 0;
    assert_eq!(
        rejection_code(inexact_frozen.validate()),
        "verifier_baseline_stale_target_revision"
    );
    Ok(())
}

/// Builds one immutable audit evidence record.
fn audit_evidence() -> VerifierAuditEvidenceRecordDto {
    VerifierAuditEvidenceRecordDto {
        evidence_id: "evidence-1".to_owned(),
        authority_reference: authority_reference(),
        target_reference: target_reference(),
        frozen_references: frozen_references(),
        evidence_kind: VerifierEvidenceKindDto::UnconditionalPass,
        retained_content_reference: "content-1".to_owned(),
        canonical_evidence_digest: digest(19),
        created_at_ms: 10,
    }
}

#[test]
fn verifier_audit_evidence_validates_identity_digest_and_nested() -> DtoResult<()> {
    audit_evidence().validate()?;

    let mut blank_evidence = audit_evidence();
    blank_evidence.evidence_id = " ".to_owned();
    assert_eq!(
        rejection_code(blank_evidence.validate()),
        "verifier_evidence_invalid"
    );
    let mut blank_reference = audit_evidence();
    blank_reference.retained_content_reference = String::new();
    assert_eq!(
        rejection_code(blank_reference.validate()),
        "verifier_evidence_invalid"
    );
    let mut bad_digest = audit_evidence();
    bad_digest.canonical_evidence_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "verifier_evidence_invalid"
    );
    let mut inexact_authority = audit_evidence();
    inexact_authority.authority_reference.authority_revision = 0;
    assert_eq!(
        rejection_code(inexact_authority.validate()),
        "verifier_authority_revision_mismatch"
    );
    let mut inexact_target = audit_evidence();
    inexact_target.target_reference.target_revision = 0;
    assert_eq!(
        rejection_code(inexact_target.validate()),
        "verifier_baseline_stale_target_revision"
    );
    let mut blank_frozen = audit_evidence();
    blank_frozen.frozen_references.goal_references[0].goal_id = String::new();
    assert_eq!(
        rejection_code(blank_frozen.validate()),
        "verifier_baseline_stale_target_identity"
    );
    Ok(())
}

/// Builds one immutable audit verdict record.
fn audit_verdict() -> VerifierAuditVerdictRecordDto {
    VerifierAuditVerdictRecordDto {
        verdict_id: "verdict-1".to_owned(),
        authority_reference: authority_reference(),
        target_reference: target_reference(),
        baseline_digest: digest(20),
        verdict: VerificationAuditVerdictDto::Pass,
        evidence_references: vec!["evidence-1".to_owned()],
        canonical_verdict_digest: digest(21),
        created_at_ms: 10,
    }
}

#[test]
fn verifier_audit_verdict_validates_references_bound_and_uniqueness() -> DtoResult<()> {
    audit_verdict().validate()?;

    let mut at_bound = audit_verdict();
    at_bound.evidence_references = (0..MAX_VERIFIER_EVIDENCE_REFERENCES)
        .map(|index| format!("evidence-{index}"))
        .collect();
    at_bound.validate()?;

    let mut blank_verdict = audit_verdict();
    blank_verdict.verdict_id = " ".to_owned();
    assert_eq!(
        rejection_code(blank_verdict.validate()),
        "verifier_verdict_invalid"
    );
    let mut bad_baseline = audit_verdict();
    bad_baseline.baseline_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_baseline.validate()),
        "verifier_verdict_invalid"
    );
    let mut bad_digest = audit_verdict();
    bad_digest.canonical_verdict_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "verifier_verdict_invalid"
    );
    let mut inexact_authority = audit_verdict();
    inexact_authority.authority_reference.authority_revision = 0;
    assert_eq!(
        rejection_code(inexact_authority.validate()),
        "verifier_authority_revision_mismatch"
    );
    let mut inexact_target = audit_verdict();
    inexact_target.target_reference.target_revision = 0;
    assert_eq!(
        rejection_code(inexact_target.validate()),
        "verifier_baseline_stale_target_revision"
    );
    let mut over_bound = audit_verdict();
    over_bound.evidence_references = (0..=MAX_VERIFIER_EVIDENCE_REFERENCES)
        .map(|index| format!("evidence-{index}"))
        .collect();
    assert_eq!(
        rejection_code(over_bound.validate()),
        "verifier_verdict_invalid"
    );
    let mut blank_reference = audit_verdict();
    blank_reference.evidence_references = vec![" ".to_owned()];
    assert_eq!(
        rejection_code(blank_reference.validate()),
        "verifier_verdict_invalid"
    );
    let mut duplicate_reference = audit_verdict();
    duplicate_reference.evidence_references =
        vec!["evidence-1".to_owned(), "evidence-1".to_owned()];
    assert_eq!(
        rejection_code(duplicate_reference.validate()),
        "verifier_verdict_invalid"
    );
    Ok(())
}

#[test]
fn verifier_operation_identity_validates_identity_and_digest() -> DtoResult<()> {
    VerifierOperationIdentityDto {
        operation_id: "operation-1".to_owned(),
        operation_digest: digest(22),
    }
    .validate()?;

    let blank = VerifierOperationIdentityDto {
        operation_id: " ".to_owned(),
        operation_digest: digest(22),
    };
    assert_eq!(
        rejection_code(blank.validate()),
        "verifier_mutation_invalid"
    );
    let bad_digest = VerifierOperationIdentityDto {
        operation_id: "operation-1".to_owned(),
        operation_digest: "sha256:xyz".to_owned(),
    };
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "verifier_mutation_invalid"
    );
    Ok(())
}

/// Builds one immutable target-mutation request.
fn target_mutation() -> VerifierTargetMutationRecordDto {
    VerifierTargetMutationRecordDto {
        mutation_id: "mutation-1".to_owned(),
        authority_reference: authority_reference(),
        audit_contract_reference: contract_reference(),
        target_reference: target_reference(),
        operation: VerifierOperationDto::MarkComplete,
        audit_evidence_references: vec!["evidence-1".to_owned()],
        expected_target_revision: 1,
        expected_target_sequence: 1,
        expected_baseline_digest: digest(23),
        idempotency: VerifierOperationIdentityDto {
            operation_id: "operation-1".to_owned(),
            operation_digest: digest(22),
        },
        canonical_mutation_digest: digest(24),
        created_at_ms: 10,
    }
}

#[test]
fn verifier_target_mutation_validates_identity_digest_and_evidence() -> DtoResult<()> {
    target_mutation().validate()?;

    let mut zero_sequence = target_mutation();
    zero_sequence.expected_target_sequence = 0;
    zero_sequence.validate()?;

    let mut at_bound = target_mutation();
    at_bound.audit_evidence_references = (0..MAX_VERIFIER_EVIDENCE_REFERENCES)
        .map(|index| format!("evidence-{index}"))
        .collect();
    at_bound.validate()?;

    let mut blank_mutation = target_mutation();
    blank_mutation.mutation_id = " ".to_owned();
    assert_eq!(
        rejection_code(blank_mutation.validate()),
        "verifier_mutation_invalid"
    );
    let mut bad_baseline = target_mutation();
    bad_baseline.expected_baseline_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_baseline.validate()),
        "verifier_mutation_invalid"
    );
    let mut bad_digest = target_mutation();
    bad_digest.canonical_mutation_digest = "sha256:xyz".to_owned();
    assert_eq!(
        rejection_code(bad_digest.validate()),
        "verifier_mutation_invalid"
    );
    let mut inexact_target = target_mutation();
    inexact_target.expected_target_revision = 0;
    assert_eq!(
        rejection_code(inexact_target.validate()),
        "verifier_mutation_invalid"
    );
    let mut inexact_authority = target_mutation();
    inexact_authority.authority_reference.authority_revision = 0;
    assert_eq!(
        rejection_code(inexact_authority.validate()),
        "verifier_authority_revision_mismatch"
    );
    let mut inexact_contract = target_mutation();
    inexact_contract.audit_contract_reference.contract_revision = 0;
    assert_eq!(
        rejection_code(inexact_contract.validate()),
        "verifier_authority_contract_mismatch"
    );
    let mut blank_target = target_mutation();
    blank_target.target_reference.target_mandate_id = String::new();
    assert_eq!(
        rejection_code(blank_target.validate()),
        "verifier_target_lifecycle_invalid"
    );
    let mut invalid_idempotency = target_mutation();
    invalid_idempotency.idempotency.operation_id = String::new();
    assert_eq!(
        rejection_code(invalid_idempotency.validate()),
        "verifier_mutation_invalid"
    );
    let mut over_bound = target_mutation();
    over_bound.audit_evidence_references = (0..=MAX_VERIFIER_EVIDENCE_REFERENCES)
        .map(|index| format!("evidence-{index}"))
        .collect();
    assert_eq!(
        rejection_code(over_bound.validate()),
        "verifier_mutation_invalid"
    );
    let mut blank_reference = target_mutation();
    blank_reference.audit_evidence_references = vec![" ".to_owned()];
    assert_eq!(
        rejection_code(blank_reference.validate()),
        "verifier_mutation_invalid"
    );
    let mut duplicate_reference = target_mutation();
    duplicate_reference.audit_evidence_references =
        vec!["evidence-1".to_owned(), "evidence-1".to_owned()];
    assert_eq!(
        rejection_code(duplicate_reference.validate()),
        "verifier_mutation_invalid"
    );
    Ok(())
}

#[test]
fn goal_input_dtos_expose_their_typed_fields() {
    let transition = TransitionGoalLifecycleInputDto {
        goal_id: "goal-1".to_owned(),
        expected_revision: 1,
        lifecycle_state: GoalLifecycleStateDto::Paused,
        occurred_at_ms: 20,
    };
    assert_eq!(transition.goal_id, "goal-1");
    assert_eq!(transition.lifecycle_state.name(), "paused");

    let readiness = SetGoalReadinessInputDto {
        goal_id: "goal-1".to_owned(),
        expected_revision: 1,
        readiness_state: GoalReadinessStateDto::NotReady,
        occurred_at_ms: 20,
    };
    assert_eq!(readiness.readiness_state.kind_name(), "not_ready");

    let decision = RecordGoalUserDecisionInputDto {
        goal_id: "goal-1".to_owned(),
        expected_revision: 1,
        user_decision_state: GoalUserDecisionStateDto::Unaccepted,
        occurred_at_ms: 20,
    };
    assert_eq!(decision.expected_revision, 1);

    let attach = AttachGoalChildInputDto {
        parent_goal_id: "goal-1".to_owned(),
        child_goal_id: "goal-2".to_owned(),
        child_revision_at_link: 1,
        canonical_link_digest: digest(2),
        created_at_ms: 20,
    };
    assert_eq!(attach.child_goal_id, "goal-2");

    let create_link = CreateGoalSessionLinkInputDto {
        link: GoalSessionLinkRecordDto {
            link_id: "link-1".to_owned(),
            project_goal_id: "goal-1".to_owned(),
            session_id: "session-1".to_owned(),
            effective_from_revision: 1,
            canonical_link_digest: digest(3),
            created_at_ms: 20,
        },
    };
    assert_eq!(create_link.link.session_id, "session-1");

    let create_gate = CreateGoalGateInputDto {
        gate: gate_record(),
    };
    assert_eq!(create_gate.gate.revision, 1);

    let transition_template = TransitionGoalGateTemplateInputDto {
        template_id: "template-1".to_owned(),
        revision: 1,
        lifecycle_state: GoalTemplateLifecycleStateDto::Archived,
        occurred_at_ms: 20,
    };
    assert_eq!(transition_template.lifecycle_state.name(), "archived");

    let suffix = RecordCompactionSuffixReferenceInputDto {
        scope: GoalRecordScopeDto::Session {
            session_id: "session-1".to_owned(),
        },
        history_reference: "history-3".to_owned(),
        occurred_at_ms: 20,
    };
    assert_eq!(suffix.history_reference, "history-3");

    let working_form = GoalCompactionWorkingFormRecordDto {
        current_summary: Some(conversation_summary()),
        uncompacted_suffix: vec!["history-3".to_owned()],
    };
    assert!(working_form.current_summary.is_some());
    assert_eq!(working_form.uncompacted_suffix.len(), 1);

    let mutation = target_mutation();
    assert_eq!(mutation.operation, VerifierOperationDto::MarkComplete);
    let outcome = ApplyVerifierMutationOutcomeDto {
        mutation: target_mutation(),
        authority: authority_record(),
        replayed: false,
    };
    assert!(!outcome.replayed);
    assert_eq!(outcome.mutation.mutation_id, "mutation-1");
    assert_eq!(outcome.authority.authority_id, "authority-1");
}

#[test]
fn safe_label_accepts_exact_bound_and_rejects_closed_boundaries() -> DtoResult<()> {
    validate_safe_label("invalid_harness_rule", &"x".repeat(MAX_SAFE_LABEL_CHARS))?;

    for blank in [String::new(), " ".to_owned()] {
        assert_eq!(
            rejection_code(validate_safe_label("invalid_harness_rule", &blank)),
            "invalid_harness_rule"
        );
    }
    assert_eq!(
        rejection_code(validate_safe_label("invalid_harness_rule", "nul\0byte")),
        "invalid_harness_rule"
    );
    assert_eq!(
        rejection_code(validate_safe_label("invalid_harness_rule", "bell\u{7}")),
        "invalid_harness_rule"
    );
    assert_eq!(
        rejection_code(validate_safe_label(
            "invalid_harness_rule",
            &"x".repeat(MAX_SAFE_LABEL_CHARS + 1)
        )),
        "invalid_harness_rule"
    );
    assert_eq!(
        rejection_code(validate_safe_label("invalid_harness_rule", "sk-live-key")),
        "credentials_forbidden"
    );
    assert_eq!(
        rejection_code(validate_safe_label("invalid_harness_rule", "Bearer live")),
        "credentials_forbidden"
    );
    Ok(())
}

#[test]
fn safe_label_list_accepts_empty_and_exact_bound_and_rejects_invalid_entries() -> DtoResult<()> {
    validate_safe_labels("harness_source_unavailable", &[], 0)?;
    validate_safe_labels(
        "harness_source_unavailable",
        &["source-a".to_owned(), "source-b".to_owned()],
        2,
    )?;

    assert_eq!(
        rejection_code(validate_safe_labels(
            "harness_source_unavailable",
            &["a".to_owned(), "b".to_owned(), "c".to_owned()],
            2
        )),
        "harness_source_unavailable"
    );
    assert_eq!(
        rejection_code(validate_safe_labels(
            "harness_source_unavailable",
            &["source-a".to_owned(), " ".to_owned()],
            2
        )),
        "harness_source_unavailable"
    );
    assert_eq!(
        rejection_code(validate_safe_labels(
            "harness_source_unavailable",
            &["password=hunter2".to_owned()],
            2
        )),
        "credentials_forbidden"
    );
    assert_eq!(
        rejection_code(validate_safe_labels(
            "harness_source_unavailable",
            &["x".repeat(MAX_SAFE_LABEL_CHARS + 1)],
            2
        )),
        "harness_source_unavailable"
    );
    Ok(())
}

#[test]
fn safe_digest_requires_canonical_sha256_text() -> DtoResult<()> {
    validate_safe_digest("harness_revision_conflict", &digest(0))?;
    validate_safe_digest(
        "harness_revision_conflict",
        &format!("sha256:{}", "0123456789abcdef".repeat(4)),
    )?;

    for invalid in [
        String::new(),
        "md5:abc".to_owned(),
        "sha256:".to_owned(),
        format!("sha256:{}", "a".repeat(63)),
        format!("sha256:{}", "a".repeat(65)),
        format!("sha256:{}", "A".repeat(64)),
        format!("sha256:{}", "g".repeat(64)),
        format!("sha256:{}!", "a".repeat(63)),
    ] {
        assert_eq!(
            rejection_code(validate_safe_digest("harness_revision_conflict", &invalid)),
            "harness_revision_conflict"
        );
    }
    Ok(())
}

#[test]
fn safe_content_accepts_exact_bound_and_rejects_closed_boundaries() -> DtoResult<()> {
    validate_safe_content(
        "harness_source_unavailable",
        "harness_result_too_large",
        "safe harness summary",
    )?;
    validate_safe_content(
        "harness_source_unavailable",
        "harness_result_too_large",
        &"x".repeat(MAX_SAFE_CONTENT_BYTES),
    )?;

    for blank in [String::new(), " ".to_owned()] {
        assert_eq!(
            rejection_code(validate_safe_content(
                "harness_source_unavailable",
                "harness_result_too_large",
                &blank
            )),
            "harness_source_unavailable"
        );
    }
    assert_eq!(
        rejection_code(validate_safe_content(
            "harness_source_unavailable",
            "harness_result_too_large",
            "line\nbreak"
        )),
        "harness_source_unavailable"
    );
    assert_eq!(
        rejection_code(validate_safe_content(
            "harness_source_unavailable",
            "harness_result_too_large",
            "nul\0byte"
        )),
        "harness_source_unavailable"
    );
    assert_eq!(
        rejection_code(validate_safe_content(
            "harness_source_unavailable",
            "harness_result_too_large",
            &"x".repeat(MAX_SAFE_CONTENT_BYTES + 1)
        )),
        "harness_result_too_large"
    );
    assert_eq!(
        rejection_code(validate_safe_content(
            "harness_source_unavailable",
            "harness_result_too_large",
            "password=hunter2"
        )),
        "credentials_forbidden"
    );
    Ok(())
}

#[test]
fn goal_repo_bounds_match_their_domain_sources() {
    assert_eq!(MAX_GOAL_RECORD_PAGE, 64);
    assert_eq!(MAX_GOAL_PARENT_CHAIN, 16);
    assert_eq!(MAX_GOAL_SESSION_LINKS, 64);
    assert_eq!(MAX_GOAL_REQUIRED_GATES, GOAL_MAX_GATES_PER_GOAL);
    assert_eq!(MAX_GOAL_RULE_REFERENCES, 32);
    assert_eq!(MAX_GOAL_CARD_REFERENCES, 32);
    assert_eq!(
        MAX_GOAL_SAFE_TEXT_BYTES as u64,
        intention_domain::goal_domain::GOAL_MAX_FULL_RECORD_BYTES
    );
    assert_eq!(MAX_GOAL_DRAFT_EDITS, 128);
    assert_eq!(MAX_GOAL_ROLE_TOOLS, 16);
    assert_eq!(
        MAX_GOAL_EVIDENCE_REFERENCES,
        intention_domain::verification::VERIFIER_MAX_EVIDENCE_REFERENCES
    );
    assert_eq!(
        MAX_VERIFIER_FROZEN_GOAL_REFERENCES,
        intention_domain::verification::VERIFIER_MAX_FROZEN_GOAL_REFERENCES
    );
    assert_eq!(
        MAX_VERIFIER_FROZEN_CONTRACT_REFERENCES,
        intention_domain::verification::VERIFIER_MAX_FROZEN_CONTRACT_REFERENCES
    );
    assert_eq!(
        MAX_VERIFIER_EVIDENCE_REFERENCES,
        intention_domain::verification::VERIFIER_MAX_EVIDENCE_REFERENCES
    );
}
