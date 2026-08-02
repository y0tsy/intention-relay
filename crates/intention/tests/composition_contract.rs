#![allow(
    clippy::expect_used,
    reason = "Composition fixture tests use expect for direct diagnostics."
)]

use intention::DaemonApplicationFacade;
use intention_domain::{GetSessionSnapshotQueryDto, RunModeDto, SendUserTurnCommandDto};
use intention_protocol::{
    ProtocolCommandDto, ProtocolCommandResultDto, ProtocolQueryDto, ProtocolQueryResultDto,
    SessionResyncReasonDto, SessionSubscriptionResponseDto, SubscribeSessionCommandDto,
};
use intention_types::{SchemaVersionDto, SessionEventSequenceDto, SessionId, TurnId};

const SCHEMA: SchemaVersionDto = SchemaVersionDto::new(1, 0);

#[test]
fn composition_fixture_exercises_queries_commands_and_subscription_recovery() {
    let facade = DaemonApplicationFacade::new_fixture();
    assert_eq!(facade.fixture_mode(), RunModeDto::Build);
    assert!(!facade.fixture_project_id().to_string().is_empty());
    assert_eq!(
        facade
            .fixture_workspace()
            .expect("fixture workspace remains valid")
            .as_str(),
        "/m2-fixture-workspace"
    );

    assert!(matches!(
        facade.query(ProtocolQueryDto::GetDaemonHealth),
        ProtocolQueryResultDto::DaemonHealth(health)
            if health == facade.health()
    ));
    assert!(matches!(
        facade.query(ProtocolQueryDto::GetSessionSnapshot(
            GetSessionSnapshotQueryDto::new(facade.fixture_session_id())
        )),
        ProtocolQueryResultDto::SessionSnapshot(_)
    ));
    assert!(matches!(
        facade.query(ProtocolQueryDto::GetSessionSnapshot(
            GetSessionSnapshotQueryDto::new(SessionId::new())
        )),
        ProtocolQueryResultDto::Rejected(error) if error.code() == "session_not_found"
    ));

    let accepted = facade.subscribe(SubscribeSessionCommandDto::new(
        SCHEMA,
        facade.fixture_session_id(),
        Some(SessionEventSequenceDto::new(0)),
        RunModeDto::Build,
    ));
    assert!(matches!(
        accepted,
        SessionSubscriptionResponseDto::SnapshotAndTail { .. }
    ));
    let replayed = facade.subscribe(SubscribeSessionCommandDto::new(
        SCHEMA,
        facade.fixture_session_id(),
        Some(SessionEventSequenceDto::new(1)),
        RunModeDto::Build,
    ));
    assert!(matches!(
        replayed,
        SessionSubscriptionResponseDto::SnapshotAndTail { .. }
    ));
    let invalid_position = facade.subscribe(SubscribeSessionCommandDto::new(
        SCHEMA,
        facade.fixture_session_id(),
        Some(SessionEventSequenceDto::new(2)),
        RunModeDto::Build,
    ));
    assert!(matches!(
        invalid_position,
        SessionSubscriptionResponseDto::ResyncRequired(resync)
            if resync.reason() == SessionResyncReasonDto::InvalidPosition
    ));
    let other_session = facade.subscribe(SubscribeSessionCommandDto::new(
        SCHEMA,
        SessionId::new(),
        None,
        RunModeDto::Build,
    ));
    assert!(matches!(
        other_session,
        SessionSubscriptionResponseDto::ResyncRequired(resync)
            if resync.reason() == SessionResyncReasonDto::HistoryUnavailable
    ));

    let rejected = facade.command(ProtocolCommandDto::SendUserTurn(
        SendUserTurnCommandDto::new(facade.fixture_session_id(), TurnId::new(), "fixture")
            .expect("fixture turn is valid"),
    ));
    assert!(matches!(
        rejected,
        ProtocolCommandResultDto::Rejected(error) if error.code() == "command_unavailable_in_m2"
    ));
}
