#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Durable composition tests use explicit fixture diagnostics."
)]

use intention_config::ConfigSnapshotDto;
use intention_proto::{
    CreateSessionCommandDto, ProtocolResultDto, RemoveTurnCommandDto, RunModeDto,
    SendUserTurnCommandDto, SendUserTurnOutcomeDto,
};
use intention_proto::{IdempotencyKey, ProjectId, SessionId, WorkspaceId};
use intention_test_support::{fixture_workspace_root, open_facade, session_messages};
use tempfile::TempDir;

fn snapshot() -> ConfigSnapshotDto {
    serde_json::from_str(include_str!(
        "../../intention-config/tests/fixtures/config-snapshot-v1.json"
    ))
    .expect("safe fixture snapshot decodes")
}

fn facade() -> (TempDir, intention_daemon::DaemonApplicationFacade) {
    let directory = TempDir::new().expect("temporary directory exists");
    let facade = open_facade(directory.path().join("relay.sqlite"), snapshot())
        .expect("durable facade opens");
    (directory, facade)
}

fn create(facade: &intention_daemon::DaemonApplicationFacade, session_id: SessionId) {
    let result = facade
        .create_session(CreateSessionCommandDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            fixture_workspace_root(&format!("composition-{session_id}")),
            RunModeDto::Build,
        ))
        .expect("session creation is accepted");
    assert!(matches!(result, ProtocolResultDto::SessionCreated(_)));
}

fn send_turn(
    facade: &intention_daemon::DaemonApplicationFacade,
    session_id: SessionId,
    content: &str,
) -> SendUserTurnOutcomeDto {
    let result = facade
        .send_user_turn(
            SendUserTurnCommandDto::new(session_id, IdempotencyKey::new(), content)
                .expect("turn is valid"),
        )
        .expect("turn is accepted");
    match result {
        ProtocolResultDto::TurnAccepted(accepted) => accepted.outcome(),
        _ => panic!("turn result expected"),
    }
}

#[test]
fn durable_lifecycle_reads_current_state_without_positions() {
    let (_directory, facade) = facade();
    let session_id = SessionId::new();
    create(&facade, session_id);
    let active_run = match send_turn(&facade, session_id, "active") {
        SendUserTurnOutcomeDto::Started { run_id, .. } => run_id,
        SendUserTurnOutcomeDto::Pending => panic!("first turn starts"),
    };
    let pending_content = "pending";
    let pending_turn = match send_turn(&facade, session_id, pending_content) {
        SendUserTurnOutcomeDto::Pending => {
            let snapshot = facade
                .session_snapshot(session_id)
                .expect("session projection reads");
            snapshot
                .projection()
                .pending_turns()
                .first()
                .map(intention_proto::PendingTurnProjectionDto::turn_id)
                .expect("the waiting turn is projected as pending")
        }
        SendUserTurnOutcomeDto::Started { .. } => panic!("a busy session keeps the turn pending"),
    };
    // Only the admitted turn reaches the transcript: a pending turn stays
    // durable input until a run consumes it.
    let messages = session_messages(&facade, session_id).expect("session transcript reads");
    assert_eq!(messages.len(), 1, "one admitted user row is committed");
    assert_eq!(messages[0].text(), "active");
    assert_eq!(messages[0].run_id(), Some(active_run));

    let projection = facade
        .session_snapshot(session_id)
        .expect("session projection reads")
        .projection()
        .clone();
    assert!(
        projection.active_run().is_some(),
        "the admitted turn keeps its run active"
    );
    assert_eq!(
        projection.pending_turns().len(),
        1,
        "the waiting turn is projected as pending"
    );

    let removed = facade
        .remove_turn(RemoveTurnCommandDto::new(session_id, pending_turn))
        .expect("pending turn removal is accepted");
    assert!(matches!(removed, ProtocolResultDto::TurnRemoved(_)));
    let projection = facade
        .session_snapshot(session_id)
        .expect("session projection reads")
        .projection()
        .clone();
    assert!(
        projection.pending_turns().is_empty(),
        "a removed turn leaves the pending projection and never reaches the transcript"
    );
    assert_eq!(
        session_messages(&facade, session_id)
            .expect("session transcript reads")
            .len(),
        1
    );
}

#[test]
fn restart_interrupts_unfinished_work_before_ready() {
    let directory = TempDir::new().expect("temporary directory exists");
    let database = directory.path().join("restart.sqlite");
    let session_id = SessionId::new();
    {
        let facade = open_facade(&database, snapshot()).expect("first facade opens");
        create(&facade, session_id);
        let _ = facade
            .send_user_turn(
                SendUserTurnCommandDto::new(session_id, IdempotencyKey::new(), "unfinished")
                    .expect("turn valid"),
            )
            .expect("unfinished turn is accepted");
    }
    let reopened = open_facade(&database, snapshot()).expect("restart recovers");
    let snapshot = reopened
        .session_snapshot(session_id)
        .expect("recovered session projection reads");
    assert!(
        snapshot.projection().active_run().is_none(),
        "restart terminates unfinished work before the daemon reports ready"
    );
}
