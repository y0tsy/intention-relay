#![allow(
    clippy::expect_used,
    reason = "M3 contract fixtures use expect for precise test diagnostics."
)]

use intention_domain::{RunModeDto, SessionProjectionDto, WorkspaceRootDto};
use intention_protocol::{
    CURRENT_PROTOCOL_VERSION, CreateSessionAcceptedDto, ProtocolAcceptedDto,
    ProtocolAcceptedResultDto, ProtocolVersionDto, RemoveQueuedTurnAcceptedDto,
    SendUserTurnAcceptedDto, SendUserTurnOutcomeDto, SessionSnapshotDto,
    SessionSubscriptionResponseDto, StopRunAcceptedDto,
};
use intention_types::{
    ConfigRevisionId, CorrelationIdDto, ProjectId, QueuePositionDto, RunId, SchemaVersionDto,
    SessionEventSequenceDto, SessionId, TurnId, WorkspaceId,
};

fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-protocol-m3-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("native fixture workspace is valid")
}

fn fixture_projection(
    session_id: SessionId,
    at_sequence: SessionEventSequenceDto,
) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Build,
        None,
        None,
        Vec::new(),
        at_sequence,
    )
    .expect("fixture projection is coherent")
}

#[test]
fn typed_acceptance_results_carry_required_durable_evidence() {
    let correlation = CorrelationIdDto::new();
    let session_id = SessionId::new();
    let workspace_id = WorkspaceId::new();
    let created = CreateSessionAcceptedDto::new(
        ProjectId::new(),
        workspace_id,
        session_id,
        SessionEventSequenceDto::new(2),
    );
    assert_eq!(created.workspace_id(), workspace_id);
    assert_eq!(created.committed_sequence().value(), 2);
    let started = SendUserTurnAcceptedDto::new(
        session_id,
        TurnId::new(),
        SessionEventSequenceDto::new(5),
        SendUserTurnOutcomeDto::Started {
            run_id: RunId::new(),
            config_revision_id: ConfigRevisionId::new(),
        },
    );
    assert!(matches!(
        started.outcome(),
        SendUserTurnOutcomeDto::Started { .. }
    ));
    let queued = SendUserTurnAcceptedDto::new(
        session_id,
        TurnId::new(),
        SessionEventSequenceDto::new(6),
        SendUserTurnOutcomeDto::Queued {
            queue_position: QueuePositionDto::new(3),
        },
    );
    assert!(
        matches!(queued.outcome(), SendUserTurnOutcomeDto::Queued { queue_position } if queue_position.value() == 3)
    );
    let accepted = ProtocolAcceptedDto::with_result(
        correlation,
        ProtocolAcceptedResultDto::CreateSession(created),
    );
    assert!(matches!(
        accepted.result(),
        ProtocolAcceptedResultDto::CreateSession(_)
    ));
    let encoded = serde_json::to_string(&accepted).expect("accepted result serializes");
    assert!(
        serde_json::from_str::<ProtocolAcceptedDto>(&encoded).expect("accepted result decodes")
            == accepted
    );
}

#[test]
fn current_protocol_version_is_pinned_to_the_wire_literal() {
    assert_eq!(
        CURRENT_PROTOCOL_VERSION,
        ProtocolVersionDto::new(2, 0),
        "the wire protocol version is pinned independently of the fixture constant"
    );
}

#[test]
fn accepted_dto_accessors_preserve_typed_identity() {
    let project_id = ProjectId::new();
    let workspace_id = WorkspaceId::new();
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let run_id = RunId::new();
    let sequence = SessionEventSequenceDto::new(7);

    let created = CreateSessionAcceptedDto::new(project_id, workspace_id, session_id, sequence);
    assert_eq!(created.project_id(), project_id);
    assert_eq!(created.workspace_id(), workspace_id);
    assert_eq!(created.session_id(), session_id);
    assert_eq!(created.committed_sequence(), sequence);

    let accepted = SendUserTurnAcceptedDto::new(
        session_id,
        turn_id,
        sequence,
        SendUserTurnOutcomeDto::Queued {
            queue_position: QueuePositionDto::new(2),
        },
    );
    assert_eq!(accepted.session_id(), session_id);
    assert_eq!(accepted.turn_id(), turn_id);
    assert_eq!(accepted.committed_sequence(), sequence);

    let removed = RemoveQueuedTurnAcceptedDto::new(session_id, turn_id, sequence);
    assert_eq!(removed.session_id(), session_id);
    assert_eq!(removed.turn_id(), turn_id);
    assert_eq!(removed.committed_sequence(), sequence);

    let stopped = StopRunAcceptedDto::new(session_id, run_id, sequence);
    assert_eq!(stopped.session_id(), session_id);
    assert_eq!(stopped.run_id(), run_id);
    assert_eq!(stopped.committed_sequence(), sequence);
}

#[test]
fn snapshots_validate_the_required_m3_projection() {
    let session_id = SessionId::new();
    let snapshot = SessionSnapshotDto::with_projection(
        SchemaVersionDto::new(1, 1),
        session_id,
        SessionEventSequenceDto::new(4),
        fixture_projection(session_id, SessionEventSequenceDto::new(4)),
    )
    .expect("matching projection is valid");
    assert_eq!(snapshot.projection().session_id(), session_id);
    assert_eq!(
        snapshot.projection().at_sequence(),
        SessionEventSequenceDto::new(4)
    );
    assert!(
        serde_json::from_str::<SessionSnapshotDto>(
            r#"{"schema_version":{"major":1,"minor":1},"session_id":"11111111-1111-4111-8111-111111111111","at_sequence":4}"#
        )
        .is_err(),
        "a snapshot without its required projection must fail closed"
    );
}

#[test]
fn session_snapshot_validation_covers_m3_failure_boundaries() {
    let session_id = SessionId::new();
    let sequence = SessionEventSequenceDto::new(7);

    assert_eq!(
        SessionSnapshotDto::with_projection(
            SchemaVersionDto::new(1, 1),
            session_id,
            sequence,
            fixture_projection(SessionId::new(), sequence),
        )
        .expect_err("projection session mismatch rejects")
        .code(),
        "invalid_session_snapshot_projection"
    );
}

#[test]
fn subscription_snapshot_remains_an_unboxed_compatible_wire_value() {
    let schema_version = SchemaVersionDto::new(1, 1);
    let session_id = SessionId::new();
    let snapshot = SessionSnapshotDto::with_projection(
        schema_version,
        session_id,
        SessionEventSequenceDto::new(0),
        fixture_projection(session_id, SessionEventSequenceDto::new(0)),
    )
    .expect("fixture snapshot is valid");
    let response = SessionSubscriptionResponseDto::snapshot(snapshot);

    let wire = serde_json::to_value(&response).expect("response serializes");
    assert_eq!(wire["kind"], "snapshot");
    assert!(wire["data"].is_object());
    assert_eq!(
        serde_json::from_value::<SessionSubscriptionResponseDto>(wire)
            .expect("response deserializes"),
        response
    );
}
