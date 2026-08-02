#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Durable composition tests use explicit fixture diagnostics."
)]

use intention::DaemonApplicationFacade;
use intention_config::ConfigSnapshotDto;
use intention_domain::{
    CreateSessionCommandDto, GetSessionSnapshotQueryDto, RemoveQueuedTurnCommandDto, RunModeDto,
    SendUserTurnCommandDto, StopRunCommandDto, WorkspaceRootDto,
};
use intention_protocol::{
    ProtocolCommandDto, ProtocolCommandResultDto, ProtocolQueryDto, ProtocolQueryResultDto,
    SendUserTurnOutcomeDto, SessionResyncReasonDto, SessionSubscriptionResponseDto,
    SubscribeSessionCommandDto,
};
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

fn facade() -> (TempDir, DaemonApplicationFacade) {
    let directory = TempDir::new().expect("temporary directory exists");
    let facade =
        DaemonApplicationFacade::open_for_test(directory.path().join("relay.sqlite"), snapshot())
            .expect("durable facade opens");
    (directory, facade)
}

fn create(facade: &DaemonApplicationFacade, session_id: SessionId) {
    let workspace_root = WorkspaceRootDto::parse(format!("/workspace/composition/{session_id}"))
        .expect("fixture root is absolute");
    let result = facade.command(ProtocolCommandDto::CreateSession(
        CreateSessionCommandDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            workspace_root,
            RunModeDto::Build,
        ),
    ));
    assert!(matches!(result, ProtocolCommandResultDto::Accepted(_)));
}

#[test]
fn durable_commands_queries_and_replay_only_subscription_use_projection_and_tail() {
    let (_directory, facade) = facade();
    let session_id = SessionId::new();
    create(&facade, session_id);

    let accepted = facade.command(ProtocolCommandDto::SendUserTurn(
        SendUserTurnCommandDto::new(session_id, TurnId::new(), "durable turn")
            .expect("fixture turn is valid"),
    ));
    assert!(matches!(
        accepted,
        ProtocolCommandResultDto::Accepted(value)
            if matches!(value.result(), Some(intention_protocol::ProtocolAcceptedResultDto::SendUserTurn(turn)) if matches!(turn.outcome(), SendUserTurnOutcomeDto::Started { .. }))
    ));
    assert!(matches!(
        facade.query(ProtocolQueryDto::GetSessionSnapshot(
            GetSessionSnapshotQueryDto::new(session_id)
        )),
        ProtocolQueryResultDto::SessionSnapshot(snapshot)
            if snapshot.projection().is_some() && snapshot.at_sequence() == SessionEventSequenceDto::new(3)
    ));
    assert!(matches!(
        facade.subscribe(SubscribeSessionCommandDto::new(
            SCHEMA,
            session_id,
            Some(SessionEventSequenceDto::new(0)),
            RunModeDto::Build,
        )),
        SessionSubscriptionResponseDto::SnapshotAndTail { snapshot, tail }
            if snapshot.at_sequence() == SessionEventSequenceDto::new(0)
                && tail.events().len() == 3
                && tail.next_after_sequence() == SessionEventSequenceDto::new(3)
    ));
    assert!(matches!(
        facade.subscribe(SubscribeSessionCommandDto::new(
            SCHEMA,
            session_id,
            Some(SessionEventSequenceDto::new(4)),
            RunModeDto::Build,
        )),
        SessionSubscriptionResponseDto::ResyncRequired(resync)
            if resync.reason() == SessionResyncReasonDto::InvalidPosition
    ));
}

#[test]
fn run_scoped_subscriptions_resync_without_leaking_session_projection_or_tail() {
    let (_directory, facade) = facade();
    let session_id = SessionId::new();
    let other_session_id = SessionId::new();
    create(&facade, session_id);
    create(&facade, other_session_id);

    let run_id = match facade.command(ProtocolCommandDto::SendUserTurn(
        SendUserTurnCommandDto::new(session_id, TurnId::new(), "scoped")
            .expect("fixture turn is valid"),
    )) {
        ProtocolCommandResultDto::Accepted(accepted) => match accepted.result() {
            Some(intention_protocol::ProtocolAcceptedResultDto::SendUserTurn(turn)) => {
                match turn.outcome() {
                    SendUserTurnOutcomeDto::Started { run_id, .. } => run_id,
                    SendUserTurnOutcomeDto::Queued { .. } => panic!("first turn starts a run"),
                }
            }
            _ => panic!("send turn returns its accepted result"),
        },
        ProtocolCommandResultDto::Rejected(error) => panic!("scoped turn rejected: {error:?}"),
    };

    for requested_run_id in [run_id, RunId::new()] {
        assert!(matches!(
            facade.subscribe(SubscribeSessionCommandDto::with_run_id(
                SCHEMA,
                session_id,
                Some(requested_run_id),
                Some(SessionEventSequenceDto::new(0)),
                RunModeDto::Build,
            )),
            SessionSubscriptionResponseDto::ResyncRequired(resync)
                if resync.session_id() == session_id
                    && resync.reason() == SessionResyncReasonDto::HistoryUnavailable
        ));
    }
    assert!(matches!(
        facade.subscribe(SubscribeSessionCommandDto::with_run_id(
            SCHEMA,
            other_session_id,
            Some(run_id),
            Some(SessionEventSequenceDto::new(0)),
            RunModeDto::Build,
        )),
        SessionSubscriptionResponseDto::ResyncRequired(resync)
            if resync.session_id() == other_session_id
                && resync.reason() == SessionResyncReasonDto::HistoryUnavailable
    ));
    assert!(matches!(
        facade.subscribe(SubscribeSessionCommandDto::with_run_id(
            SCHEMA,
            session_id,
            Some(run_id),
            Some(SessionEventSequenceDto::new(4)),
            RunModeDto::Build,
        )),
        SessionSubscriptionResponseDto::ResyncRequired(resync)
            if resync.session_id() == session_id
                && resync.reason() == SessionResyncReasonDto::InvalidPosition
    ));
    assert!(matches!(
        facade.subscribe(SubscribeSessionCommandDto::with_run_id(
            SCHEMA,
            SessionId::new(),
            Some(run_id),
            Some(SessionEventSequenceDto::new(0)),
            RunModeDto::Build,
        )),
        SessionSubscriptionResponseDto::ResyncRequired(resync)
            if resync.reason() == SessionResyncReasonDto::HistoryUnavailable
    ));
    assert!(matches!(
        facade.subscribe(SubscribeSessionCommandDto::new(
            SCHEMA,
            session_id,
            Some(SessionEventSequenceDto::new(0)),
            RunModeDto::Build,
        )),
        SessionSubscriptionResponseDto::SnapshotAndTail { snapshot, tail }
            if snapshot.at_sequence() == SessionEventSequenceDto::new(0)
                && tail.events().len() == 3
    ));
}

#[test]
fn commands_map_started_queued_removed_and_stopped_results_to_durable_outcomes() {
    let (_directory, facade) = facade();
    let session_id = SessionId::new();
    create(&facade, session_id);

    let active_turn = TurnId::new();
    let active_run = match facade.command(ProtocolCommandDto::SendUserTurn(
        SendUserTurnCommandDto::new(session_id, active_turn, "active")
            .expect("fixture turn is valid"),
    )) {
        ProtocolCommandResultDto::Accepted(accepted) => match accepted.result() {
            Some(intention_protocol::ProtocolAcceptedResultDto::SendUserTurn(turn)) => {
                match turn.outcome() {
                    SendUserTurnOutcomeDto::Started { run_id, .. } => run_id,
                    SendUserTurnOutcomeDto::Queued { .. } => panic!("first turn starts a run"),
                }
            }
            _ => panic!("send turn returns its accepted result"),
        },
        ProtocolCommandResultDto::Rejected(error) => panic!("active turn rejected: {error:?}"),
    };

    let queued_turn = TurnId::new();
    assert!(matches!(
        facade.command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, queued_turn, "queued")
                .expect("fixture turn is valid"),
        )),
        ProtocolCommandResultDto::Accepted(accepted)
            if matches!(accepted.result(), Some(intention_protocol::ProtocolAcceptedResultDto::SendUserTurn(turn)) if matches!(turn.outcome(), SendUserTurnOutcomeDto::Queued { queue_position } if queue_position.value() == 0))
    ));
    assert!(matches!(
        facade.command(ProtocolCommandDto::RemoveQueuedTurn(RemoveQueuedTurnCommandDto::new(
            session_id,
            queued_turn,
        ))),
        ProtocolCommandResultDto::Accepted(accepted)
            if matches!(accepted.result(), Some(intention_protocol::ProtocolAcceptedResultDto::RemoveQueuedTurn(removed)) if removed.session_id() == session_id && removed.turn_id() == queued_turn)
    ));
    assert!(matches!(
        facade.command(ProtocolCommandDto::StopRun(StopRunCommandDto::new(
            session_id, active_run,
        ))),
        ProtocolCommandResultDto::Accepted(accepted)
            if matches!(accepted.result(), Some(intention_protocol::ProtocolAcceptedResultDto::StopRun(stopped)) if stopped.session_id() == session_id && stopped.run_id() == active_run)
    ));
}

#[test]
fn unknown_queries_and_replay_positions_return_safe_composition_responses() {
    let (_directory, facade) = facade();
    let unknown = SessionId::new();
    assert!(matches!(
        facade.query(ProtocolQueryDto::GetSessionSnapshot(
            GetSessionSnapshotQueryDto::new(unknown)
        )),
        ProtocolQueryResultDto::Rejected(error) if error.code() == "storage_record_not_found"
    ));

    let session_id = SessionId::new();
    create(&facade, session_id);
    assert!(matches!(
        facade.subscribe(SubscribeSessionCommandDto::new(
            SCHEMA,
            session_id,
            None,
            RunModeDto::Build,
        )),
        SessionSubscriptionResponseDto::SnapshotAndTail { snapshot, tail }
            if snapshot.at_sequence() == SessionEventSequenceDto::new(0)
                && tail.events().len() == 1
                && tail.next_after_sequence() == SessionEventSequenceDto::new(1)
    ));
}

#[test]
fn reopening_durable_facade_interrupts_unfinished_run_before_ready_and_is_observable() {
    let directory = TempDir::new().expect("temporary directory exists");
    let database = directory.path().join("restart.sqlite");
    let session_id = SessionId::new();
    {
        let facade = DaemonApplicationFacade::open_for_test(&database, snapshot())
            .expect("first durable facade opens");
        create(&facade, session_id);
        let accepted = facade.command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, TurnId::new(), "unfinished")
                .expect("fixture turn is valid"),
        ));
        assert!(matches!(
            accepted,
            ProtocolCommandResultDto::Accepted(value)
                if matches!(value.result(), Some(intention_protocol::ProtocolAcceptedResultDto::SendUserTurn(turn)) if matches!(turn.outcome(), SendUserTurnOutcomeDto::Started { .. }))
        ));
    }
    let reopened = DaemonApplicationFacade::open_for_test(&database, snapshot())
        .expect("restart recovery opens");
    assert!(matches!(
        reopened.query(ProtocolQueryDto::GetSessionSnapshot(
            GetSessionSnapshotQueryDto::new(session_id)
        )),
        ProtocolQueryResultDto::SessionSnapshot(snapshot)
            if snapshot.projection().is_some_and(|projection| projection.active_run().is_none())
    ));
    assert!(matches!(
        reopened.subscribe(SubscribeSessionCommandDto::new(
            SCHEMA,
            session_id,
            Some(SessionEventSequenceDto::new(0)),
            RunModeDto::Build,
        )),
        SessionSubscriptionResponseDto::SnapshotAndTail { snapshot, tail }
            if snapshot.at_sequence() == SessionEventSequenceDto::new(0)
                && tail.events().len() == 4
                && tail.next_after_sequence() == SessionEventSequenceDto::new(4)
    ));
}
