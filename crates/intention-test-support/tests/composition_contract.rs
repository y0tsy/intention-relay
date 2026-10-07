#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Durable composition tests use explicit fixture diagnostics."
)]

use intention_config::ConfigSnapshotDto;
use intention_proto::{
    CreateSessionCommandDto, GetSessionSnapshotQueryDto, RemoveTurnCommandDto, RunModeDto,
    SendUserTurnCommandDto,
};
use intention_proto::{IdempotencyKey, ProjectId, RunId, SchemaVersionDto, SessionId, WorkspaceId};
use intention_proto::{
    ProtocolAcceptedResultDto, ProtocolCommandDto, ProtocolCommandResultDto, ProtocolQueryDto,
    ProtocolQueryResultDto, SendUserTurnOutcomeDto, SessionSubscriptionResponseDto,
    SubscribeSessionCommandDto,
};
use intention_test_support::{fixture_workspace_root, open_facade, session_messages};
use tempfile::TempDir;

const SCHEMA: SchemaVersionDto = SchemaVersionDto::new(1, 0);

fn snapshot() -> ConfigSnapshotDto {
    serde_json::from_str(include_str!(
        "../../intention-config/tests/fixtures/config-snapshot-v1.json"
    ))
    .expect("safe fixture snapshot decodes")
}

fn facade() -> (TempDir, intention::DaemonApplicationFacade) {
    let directory = TempDir::new().expect("temporary directory exists");
    let facade = open_facade(directory.path().join("relay.sqlite"), snapshot())
        .expect("durable facade opens");
    (directory, facade)
}

fn create(facade: &intention::DaemonApplicationFacade, session_id: SessionId) {
    let result = facade.command(ProtocolCommandDto::CreateSession(
        CreateSessionCommandDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            fixture_workspace_root(&format!("composition-{session_id}")),
            RunModeDto::Build,
        ),
    ));
    assert!(matches!(result, ProtocolCommandResultDto::Accepted(_)));
}

fn send_turn(
    facade: &intention::DaemonApplicationFacade,
    session_id: SessionId,
    content: &str,
) -> SendUserTurnOutcomeDto {
    match facade.command(ProtocolCommandDto::SendUserTurn(
        SendUserTurnCommandDto::new(session_id, IdempotencyKey::new(), content)
            .expect("turn is valid"),
    )) {
        ProtocolCommandResultDto::Accepted(accepted) => match accepted.result() {
            ProtocolAcceptedResultDto::SendUserTurn(turn) => turn.outcome(),
            _ => panic!("turn result expected"),
        },
        ProtocolCommandResultDto::Rejected(error) => panic!("turn rejected: {error}"),
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
                .session_snapshot(session_id, None)
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

    assert!(matches!(
        facade.query(ProtocolQueryDto::GetSessionSnapshot(GetSessionSnapshotQueryDto::new(
            session_id
        ))),
        ProtocolQueryResultDto::SessionSnapshot(snapshot)
            if snapshot.projection().active_run().is_some()
                && snapshot.projection().pending_turns().len() == 1
    ));
    for (scoped_session, run_id) in [
        (session_id, Some(active_run)),
        (session_id, Some(RunId::new())),
        (SessionId::new(), Some(active_run)),
        (SessionId::new(), Some(RunId::new())),
    ] {
        let response = facade.subscribe(SubscribeSessionCommandDto::with_run_id(
            SCHEMA,
            scoped_session,
            run_id,
            RunModeDto::Build,
        ));
        if scoped_session == session_id && run_id == Some(active_run) {
            assert!(matches!(
                response,
                SessionSubscriptionResponseDto::Snapshot(_)
            ));
        } else {
            assert!(
                matches!(
                    response,
                    SessionSubscriptionResponseDto::Error(error)
                        if error.code() == "storage_record_not_found"
                ),
                "a re-subscription outside durable state is refused with a typed error"
            );
        }
    }

    assert!(matches!(
        facade.command(ProtocolCommandDto::RemoveTurn(RemoveTurnCommandDto::new(
            session_id,
            pending_turn
        ))),
        ProtocolCommandResultDto::Accepted(_)
    ));
    let projection = facade
        .session_snapshot(session_id, None)
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
        let _ = facade.command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, IdempotencyKey::new(), "unfinished")
                .expect("turn valid"),
        ));
    }
    let reopened = open_facade(&database, snapshot()).expect("restart recovers");
    assert!(matches!(
        reopened.query(ProtocolQueryDto::GetSessionSnapshot(GetSessionSnapshotQueryDto::new(session_id))),
        ProtocolQueryResultDto::SessionSnapshot(snapshot)
            if snapshot.projection().active_run().is_none()
    ));
}
