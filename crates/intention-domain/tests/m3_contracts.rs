#![allow(
    clippy::expect_used,
    reason = "M3 contract fixtures use expect for precise test diagnostics."
)]

use intention_domain::{
    CreateSessionCommandDto, DomainEventDto, QueuedTurnProjectionDto, RemoveQueuedTurnCommandDto,
    RunModeDto, RunProjectionDto, RunStartedEventDto, RunStatusDto, SessionCreatedEventDto,
    SessionProjectionDto, WorkspaceRootDto, validate_run_status_transition,
};
use intention_types::{
    ConfigRevisionId, ProjectId, QueuePositionDto, RunId, SessionEventSequenceDto, SessionId,
    TimestampDto, TurnId, WorkspaceId,
};

fn fixture_time() -> TimestampDto {
    TimestampDto::from_unix_seconds(1).expect("fixture time is valid")
}

#[test]
fn m3_sessions_require_stable_workspace_identity_in_commands_events_and_projections() {
    let project_id = ProjectId::new();
    let session_id = SessionId::new();
    let workspace_id = WorkspaceId::new();
    let workspace_root =
        WorkspaceRootDto::parse("/workspace/project").expect("fixture root is valid");
    let create = CreateSessionCommandDto::new(
        project_id,
        session_id,
        workspace_id,
        workspace_root.clone(),
        RunModeDto::Build,
    );
    assert_eq!(create.workspace_id(), workspace_id);
    let created = SessionCreatedEventDto::new(
        project_id,
        session_id,
        workspace_id,
        workspace_root.clone(),
        RunModeDto::Build,
        fixture_time(),
    );
    assert_eq!(created.workspace_id(), Some(workspace_id));
    let projection = SessionProjectionDto::new(
        project_id,
        session_id,
        workspace_id,
        workspace_root,
        RunModeDto::Build,
        None,
        None,
        Vec::new(),
        SessionEventSequenceDto::new(3),
    )
    .expect("coherent projection is valid");
    assert_eq!(projection.workspace_id(), workspace_id);
    let remove = RemoveQueuedTurnCommandDto::new(session_id, TurnId::new());
    assert_eq!(remove.session_id(), session_id);
}

#[test]
fn legacy_session_created_wire_is_explicitly_distinguished_without_inventing_workspace_identity() {
    let event: DomainEventDto = serde_json::from_str(
        r#"{"kind":"session_created","data":{"project_id":"33333333-3333-4333-8333-333333333333","session_id":"22222222-2222-4222-8222-222222222222","workspace_root":"/workspace/project","mode":"plan","occurred_at":1}}"#,
    )
    .expect("legacy event decodes deliberately");
    assert!(
        matches!(event, DomainEventDto::SessionCreated(value) if value.workspace_id().is_none())
    );
}

#[test]
fn stable_queue_tickets_allow_gaps_but_require_ascending_unique_order() {
    let session_id = SessionId::new();
    let queued_three =
        QueuedTurnProjectionDto::new(session_id, TurnId::new(), "later", QueuePositionDto::new(3))
            .expect("queued turn is valid");
    let queued_seven = QueuedTurnProjectionDto::new(
        session_id,
        TurnId::new(),
        "latest",
        QueuePositionDto::new(7),
    )
    .expect("queued turn is valid");
    assert!(
        SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse("/workspace/project").expect("fixture root is valid"),
            RunModeDto::Build,
            None,
            None,
            vec![queued_three.clone(), queued_seven],
            SessionEventSequenceDto::new(3),
        )
        .is_ok()
    );
    assert!(
        SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse("/workspace/project").expect("fixture root is valid"),
            RunModeDto::Build,
            None,
            None,
            vec![queued_three.clone(), queued_three],
            SessionEventSequenceDto::new(3),
        )
        .is_err()
    );
}

#[test]
fn m3_runs_require_a_config_revision_in_projections_and_start_events() {
    let session_id = SessionId::new();
    let revision = ConfigRevisionId::new();
    let run = RunProjectionDto::new(
        session_id,
        RunId::new(),
        TurnId::new(),
        RunStatusDto::Queued,
        revision,
    );
    assert_eq!(run.config_revision_id(), revision);
    let started = RunStartedEventDto::new(
        session_id,
        run.run_id(),
        run.turn_id(),
        revision,
        fixture_time(),
    );
    assert_eq!(started.config_revision_id(), revision);
}

#[test]
fn m3_event_payloads_validate_and_expose_all_public_fields() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let run_id = RunId::new();
    let occurred_at = fixture_time();
    let revision = ConfigRevisionId::new();

    assert!(
        QueuedTurnProjectionDto::new(session_id, turn_id, " ", QueuePositionDto::new(0),).is_err()
    );
    assert!(
        intention_domain::UserTurnAcceptedEventDto::new(session_id, turn_id, " ", occurred_at)
            .is_err()
    );

    let queued = intention_domain::UserTurnQueuedEventDto::new(
        session_id,
        turn_id,
        QueuePositionDto::new(3),
        occurred_at,
    );
    assert_eq!(queued.session_id(), session_id);
    assert_eq!(queued.turn_id(), turn_id);
    assert_eq!(queued.position(), QueuePositionDto::new(3));
    assert_eq!(queued.occurred_at(), occurred_at);

    let removed =
        intention_domain::QueuedTurnRemovedEventDto::new(session_id, turn_id, occurred_at);
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
fn m3_projection_deserialization_rejects_invalid_nested_turns() {
    let session_id = SessionId::new();
    let queued = QueuedTurnProjectionDto::new(
        SessionId::new(),
        TurnId::new(),
        "other session",
        QueuePositionDto::new(0),
    )
    .expect("queued fixture is valid");
    assert!(
        SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse("/workspace/project").expect("fixture root is valid"),
            RunModeDto::Build,
            None,
            None,
            vec![queued],
            SessionEventSequenceDto::new(3),
        )
        .is_err()
    );
}

#[test]
fn run_status_terminal_classification_is_complete() {
    for status in [
        RunStatusDto::Completed,
        RunStatusDto::Cancelled,
        RunStatusDto::Failed,
        RunStatusDto::Interrupted,
    ] {
        assert!(status.is_terminal());
    }
    assert!(!RunStatusDto::Starting.is_terminal());
}

#[test]
fn run_status_state_machine_accepts_only_declared_edges() {
    let statuses = [
        RunStatusDto::Queued,
        RunStatusDto::Starting,
        RunStatusDto::Running,
        RunStatusDto::WaitingInput,
        RunStatusDto::Completing,
        RunStatusDto::Cancelling,
        RunStatusDto::Completed,
        RunStatusDto::Cancelled,
        RunStatusDto::Failed,
        RunStatusDto::Interrupted,
    ];
    for from in statuses {
        for to in statuses {
            let expected = matches!(
                (from, to),
                (RunStatusDto::Queued, RunStatusDto::Starting)
                    | (RunStatusDto::Queued, RunStatusDto::Cancelled)
                    | (RunStatusDto::Queued, RunStatusDto::Interrupted)
                    | (RunStatusDto::Starting, RunStatusDto::Running)
                    | (RunStatusDto::Starting, RunStatusDto::Cancelling)
                    | (RunStatusDto::Starting, RunStatusDto::Failed)
                    | (RunStatusDto::Starting, RunStatusDto::Interrupted)
                    | (RunStatusDto::Running, RunStatusDto::WaitingInput)
                    | (RunStatusDto::Running, RunStatusDto::Completing)
                    | (RunStatusDto::Running, RunStatusDto::Cancelling)
                    | (RunStatusDto::Running, RunStatusDto::Failed)
                    | (RunStatusDto::Running, RunStatusDto::Interrupted)
                    | (RunStatusDto::WaitingInput, RunStatusDto::Running)
                    | (RunStatusDto::WaitingInput, RunStatusDto::Cancelling)
                    | (RunStatusDto::WaitingInput, RunStatusDto::Failed)
                    | (RunStatusDto::WaitingInput, RunStatusDto::Interrupted)
                    | (RunStatusDto::Completing, RunStatusDto::Completed)
                    | (RunStatusDto::Completing, RunStatusDto::Failed)
                    | (RunStatusDto::Completing, RunStatusDto::Interrupted)
                    | (RunStatusDto::Cancelling, RunStatusDto::Cancelled)
                    | (RunStatusDto::Cancelling, RunStatusDto::Failed)
                    | (RunStatusDto::Cancelling, RunStatusDto::Interrupted)
            );
            assert_eq!(validate_run_status_transition(from, to).is_ok(), expected);
        }
    }
}
