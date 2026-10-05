#![allow(
    clippy::expect_used,
    reason = "M3 contract fixtures use expect for precise test diagnostics."
)]

use intention_domain::{
    PendingTurnProjectionDto, RunModeDto, RunProjectionDto, RunStartedEventDto, RunStatusDto,
    SessionProjectionDto, WorkspaceRootDto, validate_run_status_transition,
};
use intention_types::{
    ConfigRevisionId, ProjectId, RunId, SessionEventSequenceDto, SessionId, TimestampDto, TurnId,
    WorkspaceId,
};

fn fixture_time() -> TimestampDto {
    TimestampDto::from_unix_seconds(1).expect("fixture time is valid")
}

fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-domain-m3-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("native fixture workspace is valid")
}

#[test]
fn pending_turns_keep_their_order_but_require_unique_identities() {
    let session_id = SessionId::new();
    let older = PendingTurnProjectionDto::new(session_id, TurnId::new(), "later")
        .expect("pending turn is valid");
    let newer = PendingTurnProjectionDto::new(session_id, TurnId::new(), "latest")
        .expect("pending turn is valid");
    assert!(
        SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            workspace_root(),
            RunModeDto::Build,
            None,
            None,
            vec![older.clone(), newer],
            SessionEventSequenceDto::new(3),
        )
        .is_ok()
    );
    assert!(
        SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            workspace_root(),
            RunModeDto::Build,
            None,
            None,
            vec![older.clone(), older],
            SessionEventSequenceDto::new(3),
        )
        .is_err()
    );
}

#[test]
fn m3_event_payloads_validate_and_expose_all_public_fields() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let run_id = RunId::new();
    let occurred_at = fixture_time();
    let revision = ConfigRevisionId::new();

    assert!(PendingTurnProjectionDto::new(session_id, turn_id, " ").is_err());
    assert!(
        intention_domain::UserTurnAcceptedEventDto::new(session_id, turn_id, " ", occurred_at)
            .is_err()
    );

    let pending = intention_domain::UserTurnPendingEventDto::new(session_id, turn_id, occurred_at);
    assert_eq!(pending.session_id(), session_id);
    assert_eq!(pending.turn_id(), turn_id);
    assert_eq!(pending.occurred_at(), occurred_at);

    let removed = intention_domain::TurnRemovedEventDto::new(session_id, turn_id, occurred_at);
    assert_eq!(removed.session_id(), session_id);
    assert_eq!(removed.turn_id(), turn_id);
    assert_eq!(removed.occurred_at(), occurred_at);

    let started = RunStartedEventDto::new(session_id, run_id, turn_id, revision, occurred_at);
    assert_eq!(started.session_id(), session_id);
    assert_eq!(started.run_id(), run_id);
    assert_eq!(started.turn_id(), turn_id);
    assert_eq!(started.config_revision_id(), revision);
    assert_eq!(started.occurred_at(), occurred_at);

    let accepted = intention_domain::UserTurnAcceptedEventDto::new(
        session_id,
        turn_id,
        "accepted",
        occurred_at,
    )
    .expect("non-empty event content is valid");
    assert_eq!(accepted.session_id(), session_id);
    assert_eq!(accepted.turn_id(), turn_id);
    assert_eq!(accepted.content(), "accepted");
    assert_eq!(accepted.occurred_at(), occurred_at);
}

#[test]
fn m3_projection_rejects_a_pending_turn_from_a_foreign_session() {
    let session_id = SessionId::new();
    let pending = PendingTurnProjectionDto::new(SessionId::new(), TurnId::new(), "other session")
        .expect("pending fixture is valid");
    assert!(
        SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            workspace_root(),
            RunModeDto::Build,
            None,
            None,
            vec![pending],
            SessionEventSequenceDto::new(3),
        )
        .is_err()
    );
}

#[test]
fn m3_projection_accessors_and_deserialization_cover_optional_state() {
    let session_id = SessionId::new();
    let run = RunProjectionDto::new(
        session_id,
        RunId::new(),
        TurnId::new(),
        RunStatusDto::Running,
        ConfigRevisionId::new(),
    );
    let pending = PendingTurnProjectionDto::new(session_id, TurnId::new(), "pending")
        .expect("pending turn is valid");
    let projection = SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Plan,
        Some(ConfigRevisionId::new()),
        Some(run),
        vec![pending],
        SessionEventSequenceDto::new(4),
    )
    .expect("projection is valid");
    assert!(projection.config_revision_id().is_some());
    assert!(projection.active_run().is_some());
    assert_eq!(projection.pending_turns().len(), 1);
    assert_eq!(projection.at_sequence().value(), 4);
    let wire = serde_json::to_string(&projection).expect("projection serializes");
    let decoded: SessionProjectionDto = serde_json::from_str(&wire).expect("projection decodes");
    assert_eq!(decoded, projection);
}

#[test]
fn run_status_state_machine_accepts_only_declared_edges() {
    let statuses = [
        RunStatusDto::Queued,
        RunStatusDto::Starting,
        RunStatusDto::Running,
        RunStatusDto::WaitingInput,
        RunStatusDto::Completing,
        RunStatusDto::Completed,
        RunStatusDto::Failed,
        RunStatusDto::Interrupted,
    ];
    for from in statuses {
        for to in statuses {
            let expected = matches!(
                (from, to),
                (RunStatusDto::Queued, RunStatusDto::Starting)
                    | (RunStatusDto::Queued, RunStatusDto::Interrupted)
                    | (RunStatusDto::Starting, RunStatusDto::Running)
                    | (RunStatusDto::Starting, RunStatusDto::Failed)
                    | (RunStatusDto::Starting, RunStatusDto::Interrupted)
                    | (RunStatusDto::Running, RunStatusDto::WaitingInput)
                    | (RunStatusDto::Running, RunStatusDto::Completing)
                    | (RunStatusDto::Running, RunStatusDto::Failed)
                    | (RunStatusDto::Running, RunStatusDto::Interrupted)
                    | (RunStatusDto::WaitingInput, RunStatusDto::Running)
                    | (RunStatusDto::WaitingInput, RunStatusDto::Failed)
                    | (RunStatusDto::WaitingInput, RunStatusDto::Interrupted)
                    | (RunStatusDto::Completing, RunStatusDto::Completed)
                    | (RunStatusDto::Completing, RunStatusDto::Failed)
                    | (RunStatusDto::Completing, RunStatusDto::Interrupted)
            );
            assert_eq!(validate_run_status_transition(from, to).is_ok(), expected);
        }
    }
}

#[test]
fn plan_transitions_accept_allowed_edges_and_reject_terminal_restarts() {
    use intention_domain::{PlanStatusDto, validate_plan_status_transition};
    assert!(validate_plan_status_transition(None, PlanStatusDto::Drafting).is_ok());
    for (from, to) in [
        (PlanStatusDto::Drafting, PlanStatusDto::Revising),
        (PlanStatusDto::Drafting, PlanStatusDto::Submitted),
        (PlanStatusDto::Drafting, PlanStatusDto::Abandoned),
        (PlanStatusDto::Revising, PlanStatusDto::Revising),
        (PlanStatusDto::Revising, PlanStatusDto::Submitted),
        (PlanStatusDto::Revising, PlanStatusDto::Abandoned),
        (PlanStatusDto::Submitted, PlanStatusDto::Approved),
        (PlanStatusDto::Submitted, PlanStatusDto::Rejected),
        (PlanStatusDto::Submitted, PlanStatusDto::Abandoned),
        (PlanStatusDto::Rejected, PlanStatusDto::Revising),
        (PlanStatusDto::Rejected, PlanStatusDto::Abandoned),
    ] {
        assert!(validate_plan_status_transition(Some(from), to).is_ok());
    }
    for status in [
        PlanStatusDto::Approved,
        PlanStatusDto::Superseded,
        PlanStatusDto::Abandoned,
    ] {
        assert!(validate_plan_status_transition(Some(status), PlanStatusDto::Drafting).is_err());
    }
}
