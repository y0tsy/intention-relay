#![allow(
    clippy::expect_used,
    reason = "M3 contract fixtures use expect for precise test diagnostics."
)]

use intention_domain::{RunModeDto, SessionProjectionDto, WorkspaceRootDto};
use intention_protocol::{
    CreateSessionAcceptedDto, ProtocolAcceptedDto, ProtocolAcceptedResultDto,
    SendUserTurnAcceptedDto, SendUserTurnOutcomeDto, SessionEventTailBatchDto, SessionSnapshotDto,
    SessionSubscriptionResponseDto,
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

#[test]
fn typed_acceptance_results_carry_required_durable_evidence_and_legacy_acceptance_remains_compatible()
 {
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
        Some(ProtocolAcceptedResultDto::CreateSession(_))
    ));
    let legacy: ProtocolAcceptedDto =
        serde_json::from_str(r#"{"correlation_id":"11111111-1111-4111-8111-111111111111"}"#)
            .expect("legacy accepted result remains decodable");
    assert_eq!(legacy.result(), None);
}

#[test]
fn snapshots_validate_optional_m3_projection_but_accept_legacy_shape() {
    let session_id = SessionId::new();
    let projection = SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Build,
        None,
        None,
        Vec::new(),
        SessionEventSequenceDto::new(4),
    )
    .expect("projection is coherent");
    let snapshot = SessionSnapshotDto::with_projection(
        SchemaVersionDto::new(1, 0),
        session_id,
        SessionEventSequenceDto::new(4),
        projection,
    )
    .expect("matching projection is valid");
    assert!(snapshot.projection().is_some());
    let legacy: SessionSnapshotDto = serde_json::from_str(r#"{"schema_version":{"major":1,"minor":0},"session_id":"11111111-1111-4111-8111-111111111111","at_sequence":4}"#).expect("legacy snapshot remains decodable");
    assert_eq!(legacy.projection(), None);
}

#[test]
fn acceptance_accessors_and_snapshot_validation_cover_m3_failure_boundaries() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let run_id = RunId::new();
    let sequence = SessionEventSequenceDto::new(7);
    let created =
        CreateSessionAcceptedDto::new(ProjectId::new(), WorkspaceId::new(), session_id, sequence);
    assert_eq!(created.project_id(), created.project_id());
    assert_eq!(created.session_id(), session_id);

    let accepted = SendUserTurnAcceptedDto::new(
        session_id,
        turn_id,
        sequence,
        SendUserTurnOutcomeDto::Queued {
            queue_position: QueuePositionDto::new(2),
        },
    );
    assert_eq!(accepted.turn_id(), turn_id);
    assert_eq!(accepted.committed_sequence(), sequence);

    let removed =
        intention_protocol::RemoveQueuedTurnAcceptedDto::new(session_id, turn_id, sequence);
    assert_eq!(removed.session_id(), session_id);
    assert_eq!(removed.turn_id(), turn_id);
    assert_eq!(removed.committed_sequence(), sequence);

    let stopped = intention_protocol::StopRunAcceptedDto::new(session_id, run_id, sequence);
    assert_eq!(stopped.session_id(), session_id);
    assert_eq!(stopped.run_id(), run_id);
    assert_eq!(stopped.committed_sequence(), sequence);

    let projection = SessionProjectionDto::new(
        ProjectId::new(),
        SessionId::new(),
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Build,
        None,
        None,
        Vec::new(),
        sequence,
    )
    .expect("projection is coherent");
    assert_eq!(
        SessionSnapshotDto::with_projection(
            SchemaVersionDto::new(1, 0),
            session_id,
            sequence,
            projection,
        )
        .expect_err("projection session mismatch rejects")
        .code(),
        "invalid_session_snapshot_projection"
    );
}

#[test]
fn subscription_snapshot_and_tail_remains_an_unboxed_compatible_wire_value() {
    let schema_version = SchemaVersionDto::new(1, 0);
    let session_id = SessionId::new();
    let snapshot =
        SessionSnapshotDto::new(schema_version, session_id, SessionEventSequenceDto::new(0));
    let tail = SessionEventTailBatchDto::new(
        schema_version,
        session_id,
        SessionEventSequenceDto::new(0),
        Vec::new(),
    )
    .expect("empty tail at the snapshot checkpoint is valid");
    let response = SessionSubscriptionResponseDto::snapshot_and_tail(snapshot, tail)
        .expect("matching snapshot and tail are valid");

    let wire = serde_json::to_value(&response).expect("response serializes");
    assert_eq!(wire["kind"], "snapshot_and_tail");
    assert!(wire["data"]["snapshot"].is_object());
    assert!(wire["data"]["tail"].is_object());
    assert_eq!(
        serde_json::from_value::<SessionSubscriptionResponseDto>(wire)
            .expect("response deserializes"),
        response
    );
}
