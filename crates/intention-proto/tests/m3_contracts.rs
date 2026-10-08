#![allow(
    clippy::expect_used,
    reason = "M3 contract fixtures use expect for precise test diagnostics."
)]

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::{fixture_message, fixture_projection};

use intention_proto::{
    ConfigRevisionId, CreateSessionAcceptedDto, InterruptRunAcceptedDto, ProtocolResultDto,
    RemoveTurnAcceptedDto, RunId, SendUserTurnAcceptedDto, SendUserTurnOutcomeDto,
    SessionSnapshotDto, TurnId,
};
use intention_proto::{ProjectId, SessionId, WorkspaceId};

#[test]
fn typed_results_carry_required_durable_evidence() {
    let session_id = SessionId::new();
    let workspace_id = WorkspaceId::new();
    let created = CreateSessionAcceptedDto::new(ProjectId::new(), workspace_id, session_id);
    let started = SendUserTurnAcceptedDto::new(
        session_id,
        TurnId::new(),
        SendUserTurnOutcomeDto::Started {
            run_id: RunId::new(),
            config_revision_id: ConfigRevisionId::new(),
        },
    );
    let pending =
        SendUserTurnAcceptedDto::new(session_id, TurnId::new(), SendUserTurnOutcomeDto::Pending);
    let removed = RemoveTurnAcceptedDto::new(session_id, TurnId::new());
    let interrupted = InterruptRunAcceptedDto::new(session_id, RunId::new());

    for result in [
        ProtocolResultDto::SessionCreated(created),
        ProtocolResultDto::TurnAccepted(started),
        ProtocolResultDto::TurnAccepted(pending),
        ProtocolResultDto::TurnRemoved(removed),
        ProtocolResultDto::RunInterrupted(interrupted),
    ] {
        let encoded = serde_json::to_string(&result).expect("result serializes");
        assert!(
            !encoded.contains("sequence"),
            "acceptance evidence carries no committed sequence"
        );
        assert_eq!(
            serde_json::from_str::<ProtocolResultDto>(&encoded).expect("result decodes"),
            result
        );
    }
}

#[test]
fn snapshots_validate_the_required_m3_projection() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let snapshot = SessionSnapshotDto::with_projection(
        session_id,
        fixture_projection(session_id),
        vec![fixture_message(session_id, run_id)],
    )
    .expect("matching projection is valid");
    assert_eq!(snapshot.projection().session_id(), session_id);
    assert_eq!(snapshot.session_id(), session_id);
    assert_eq!(snapshot.messages().len(), 1);
    assert!(
        serde_json::from_str::<SessionSnapshotDto>(
            r#"{"session_id":"11111111-1111-4111-8111-111111111111","projection":null,"messages":[]}"#
        )
        .is_err(),
        "a snapshot without its required projection must fail closed"
    );
    assert!(
        serde_json::from_str::<SessionSnapshotDto>(
            r#"{"session_id":"11111111-1111-4111-8111-111111111111","projection":{"project_id":"11111111-1111-4111-8111-111111111111","session_id":"11111111-1111-4111-8111-111111111111","workspace_id":"11111111-1111-4111-8111-111111111111","workspace_root":"/fixture","mode":"build","pending_turns":[]}}"#
        )
        .is_err(),
        "a snapshot without its required messages must fail closed"
    );
}

#[test]
fn session_snapshot_validation_covers_m3_failure_boundaries() {
    let session_id = SessionId::new();

    assert_eq!(
        SessionSnapshotDto::with_projection(
            session_id,
            fixture_projection(SessionId::new()),
            Vec::new(),
        )
        .expect_err("projection session mismatch rejects")
        .code(),
        "invalid_session_snapshot_projection"
    );
}
