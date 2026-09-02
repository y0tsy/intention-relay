#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Durable composition tests use explicit fixture diagnostics."
)]

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    CreateSessionCommandDto, DomainEventDto, GetSessionSnapshotQueryDto,
    RemoveQueuedTurnCommandDto, RunModeDto, RunStatusDto, SendUserTurnCommandDto,
};
use intention_protocol::{
    ProtocolCommandDto, ProtocolCommandResultDto, ProtocolQueryDto, ProtocolQueryResultDto,
    SendUserTurnOutcomeDto, SessionResyncReasonDto, SessionSubscriptionResponseDto,
    SubscribeSessionCommandDto,
};
use intention_test_support::{durable_events, fixture_workspace_root, open_facade};
use intention_types::{
    ProjectId, RunId, SchemaVersionDto, SessionEventSequenceDto, SessionId, TurnId, WorkspaceId,
};
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
    facade
        .seed_fixture_catalog_for_test_support(
            "seed-1",
            "openrouter",
            "fixture",
            "https://api.example.invalid/v1",
        )
        .expect("fixture catalog seeds");
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

#[test]
fn durable_lifecycle_and_replay_contracts_hold() {
    let (_directory, facade) = facade();
    let session_id = SessionId::new();
    create(&facade, session_id);
    let active_run = match facade.command(ProtocolCommandDto::SendUserTurn(
        SendUserTurnCommandDto::new(session_id, TurnId::new(), "active").expect("turn is valid"),
    )) {
        ProtocolCommandResultDto::Accepted(accepted) => match accepted.result() {
            Some(intention_protocol::ProtocolAcceptedResultDto::SendUserTurn(turn)) => {
                match turn.outcome() {
                    SendUserTurnOutcomeDto::Started { run_id, .. } => run_id,
                    SendUserTurnOutcomeDto::Queued { .. } => panic!("first turn starts"),
                }
            }
            _ => panic!("turn result expected"),
        },
        ProtocolCommandResultDto::Rejected(error) => panic!("turn rejected: {error}"),
    };
    let queued_turn = TurnId::new();
    assert!(matches!(
        facade.command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, queued_turn, "queued").expect("turn is valid")
        )),
        ProtocolCommandResultDto::Accepted(_)
    ));
    facade
        .stop_run_for_daemon_host(session_id, active_run)
        .expect("host stop moves the run to cancelling");
    let events = durable_events(&facade, session_id).expect("durable event tail loads");
    assert!(matches!(
        events.last().expect("cancelling event exists").payload(),
        DomainEventDto::RunStatusChanged(event)
            if event.status() == RunStatusDto::Cancelling
    ));
    // The daemon task registry owns the Cancelling -> Cancelled terminal
    // transition and the queued-turn promotion that follows it; without a
    // task registry the run stays durably Cancelling and the queued turn
    // stays queued (single execution path per operation under ADR 0038).
    assert!(events.iter().all(|event| !matches!(
        event.payload(),
        DomainEventDto::RunStatusChanged(change)
            if change.status() == RunStatusDto::Cancelled
    )));
    assert!(matches!(
        facade.query(ProtocolQueryDto::GetSessionSnapshot(GetSessionSnapshotQueryDto::new(session_id))),
        ProtocolQueryResultDto::SessionSnapshot(snapshot)
            if snapshot.projection().is_some_and(|projection| projection.active_run().is_some())
    ));
    assert!(matches!(
        facade.subscribe(SubscribeSessionCommandDto::new(
            SCHEMA, session_id, Some(SessionEventSequenceDto::new(0)), RunModeDto::Build,
        )),
        SessionSubscriptionResponseDto::SnapshotAndTail { snapshot, tail }
            if snapshot.projection().is_some() && tail.events().is_empty()
    ));
    for (scoped_session, run_id) in [
        (session_id, Some(active_run)),
        (session_id, Some(RunId::new())),
        (SessionId::new(), Some(active_run)),
        (SessionId::new(), Some(RunId::new())),
    ] {
        assert!(matches!(
            facade.subscribe(SubscribeSessionCommandDto::with_run_id(
                SCHEMA, scoped_session, run_id, Some(SessionEventSequenceDto::new(u64::MAX)), RunModeDto::Build,
            )),
            SessionSubscriptionResponseDto::ResyncRequired(resync)
                if resync.reason() == SessionResyncReasonDto::HistoryUnavailable
        ));
    }
    let _ = queued_turn;
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
            SendUserTurnCommandDto::new(session_id, TurnId::new(), "unfinished")
                .expect("turn valid"),
        ));
    }
    let reopened = open_facade(&database, snapshot()).expect("restart recovers");
    assert!(matches!(
        reopened.query(ProtocolQueryDto::GetSessionSnapshot(GetSessionSnapshotQueryDto::new(session_id))),
        ProtocolQueryResultDto::SessionSnapshot(snapshot)
            if snapshot.projection().is_some_and(|projection| projection.active_run().is_none())
    ));
}

#[test]
fn queued_turn_removal_remains_durable() {
    let (_directory, facade) = facade();
    let session_id = SessionId::new();
    create(&facade, session_id);
    let _ = facade.command(ProtocolCommandDto::SendUserTurn(
        SendUserTurnCommandDto::new(session_id, TurnId::new(), "active").expect("turn valid"),
    ));
    let queued = TurnId::new();
    let _ = facade.command(ProtocolCommandDto::SendUserTurn(
        SendUserTurnCommandDto::new(session_id, queued, "queued").expect("turn valid"),
    ));
    assert!(matches!(
        facade.command(ProtocolCommandDto::RemoveQueuedTurn(
            RemoveQueuedTurnCommandDto::new(session_id, queued)
        )),
        ProtocolCommandResultDto::Accepted(_)
    ));
}
