#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Client contract fixtures use direct assertions and controlled fixture launchers."
)]

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use intention_client::{
    DaemonLauncher, IntentionClient, ProcessDaemonLauncher, SessionSubscriptionRecovery,
    SessionSubscriptionReducer,
};
use intention_domain::{
    DomainEventDto, RunModeDto, RunStatusChangedEventDto, RunStatusDto, SessionProjectionDto,
};
use intention_protocol::{
    DaemonHealthDto, DaemonReadinessDto, ProtocolCapabilityDto, ProtocolHelloDto,
    ProtocolMessageDto, ProtocolQueryResultDto, ProtocolResponseEnvelopeDto,
    ProtocolResponsePayloadDto, ProtocolVersionDto, SessionEventTailBatchDto, SessionResyncDto,
    SessionResyncReasonDto, SessionSnapshotDto, SessionSubscriptionResponseDto,
    SubscribeSessionCommandDto,
};
use intention_transport::{LocalEndpoint, LocalListener, local_protocol_version, negotiate_daemon};
use intention_types::{
    CorrelationIdDto, DtoResult, ErrorDto, EventEnvelopeDto, EventId, EventMetadataDto, ProjectId,
    SchemaVersionDto, SessionEventSequenceDto, SessionId, TimestampDto, WorkspaceId,
};
use tempfile::TempDir;

const SCHEMA_VERSION: SchemaVersionDto = intention_protocol::CURRENT_DTO_SCHEMA_VERSION;

fn fixture_projection(
    session_id: SessionId,
    at_sequence: SessionEventSequenceDto,
) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        intention_domain::WorkspaceRootDto::parse(
            std::env::temp_dir()
                .join("intention-client-fixture-workspace")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture workspace root is valid"),
        RunModeDto::Build,
        None,
        None,
        Vec::new(),
        at_sequence,
    )
    .expect("fixture projection is valid")
}

#[derive(Clone)]
enum FixtureResponse {
    Health(DaemonHealthDto),
    Rejected(ErrorDto),
    Snapshot(SessionSnapshotDto),
    Subscription(SessionSubscriptionResponseDto),
    SubscriptionAfter(SessionSubscriptionResponseDto, u64),
    Invalid,
    CorrelationMismatch,
    ProtocolMismatch,
    MissingCapabilities,
    ResponseVersionMismatch,
    Disconnect,
}

#[derive(Clone)]
struct FixtureLauncher {
    response: FixtureResponse,
    launches: Arc<AtomicUsize>,
}

impl DaemonLauncher for FixtureLauncher {
    fn launch(&self, endpoint: &LocalEndpoint) -> DtoResult<()> {
        self.launches.fetch_add(1, Ordering::SeqCst);
        let _ = start_fixture_server(endpoint.clone(), self.response.clone());
        Ok(())
    }
}

#[derive(Clone)]
struct RejectingLauncher;

impl DaemonLauncher for RejectingLauncher {
    fn launch(&self, _endpoint: &LocalEndpoint) -> DtoResult<()> {
        Err(ErrorDto::unavailable(
            "fixture_launch_rejected",
            "fixture launcher intentionally rejects startup",
        ))
    }
}

static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(0);
static FIXTURE_CONNECTIONS: Mutex<()> = Mutex::new(());

fn fixture_guard() -> MutexGuard<'static, ()> {
    FIXTURE_CONNECTIONS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn endpoint(_directory: &TempDir) -> LocalEndpoint {
    let sequence = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
    LocalEndpoint::from_instance_id(format!("client-fixture-{}-{sequence}", std::process::id()))
        .expect("fixture instance name is valid")
}

fn client(
    endpoint: LocalEndpoint,
    response: FixtureResponse,
    launches: Arc<AtomicUsize>,
) -> IntentionClient {
    IntentionClient::new(
        endpoint,
        "fixture-client",
        Box::new(FixtureLauncher { response, launches }),
    )
    .expect("fixture client is valid")
}

fn daemon_hello() -> ProtocolHelloDto {
    ProtocolHelloDto::new(
        local_protocol_version(),
        vec![
            ProtocolCapabilityDto::SessionSubscriptions,
            ProtocolCapabilityDto::CorrelatedRequests,
            ProtocolCapabilityDto::DaemonHealth,
        ],
        "fixture-daemon",
    )
    .expect("fixture daemon hello is valid")
}

fn start_fixture_server(
    endpoint: LocalEndpoint,
    response: FixtureResponse,
) -> thread::JoinHandle<()> {
    let listener = LocalListener::bind(endpoint).expect("fixture listener binds");
    thread::spawn(move || serve_one_fixture_connection(listener, response))
}

fn start_fixture_server_sequence(
    endpoint: LocalEndpoint,
    responses: Vec<FixtureResponse>,
) -> thread::JoinHandle<()> {
    let listener = LocalListener::bind(endpoint).expect("fixture listener binds");
    thread::spawn(move || {
        for response in responses {
            let connection = listener.accept().expect("fixture client connects");
            serve_fixture_connection(connection, response);
        }
    })
}

fn serve_one_fixture_connection(listener: LocalListener, response: FixtureResponse) {
    let connection = listener.accept().expect("fixture client connects");
    serve_fixture_connection(connection, response);
}

fn serve_fixture_connection(
    mut connection: intention_transport::LocalConnection,
    response: FixtureResponse,
) {
    if matches!(response, FixtureResponse::ProtocolMismatch) {
        connection
            .receive_hello()
            .expect("fixture client hello arrives");
        let incompatible = ProtocolHelloDto::new(
            ProtocolVersionDto::new(2, 0),
            Vec::new(),
            "incompatible-fixture-daemon",
        )
        .expect("fixture mismatch hello is valid");
        connection
            .send_hello(&incompatible)
            .expect("fixture mismatch hello sends");
        return;
    }
    let capabilities = if matches!(response, FixtureResponse::MissingCapabilities) {
        vec![ProtocolCapabilityDto::DaemonHealth]
    } else {
        daemon_hello().capabilities().to_vec()
    };
    let hello = ProtocolHelloDto::new(local_protocol_version(), capabilities, "fixture-daemon")
        .expect("fixture daemon hello is valid");
    negotiate_daemon(&mut connection, hello).expect("fixture hello negotiates");
    if matches!(response, FixtureResponse::MissingCapabilities) {
        return;
    }
    if matches!(response, FixtureResponse::Disconnect) {
        return;
    }
    let request = connection
        .receive_request()
        .expect("fixture request arrives");
    if let FixtureResponse::SubscriptionAfter(_, expected_after) = &response {
        let received_after = match request.message().payload() {
            intention_protocol::ProtocolRequestPayloadDto::Command(
                intention_protocol::ProtocolCommandDto::SubscribeSession(subscription),
            ) => subscription
                .after_sequence()
                .map_or(0, SessionEventSequenceDto::value),
            _ => panic!("recovery fixture receives a subscription request"),
        };
        assert_eq!(received_after, *expected_after);
    }
    let is_correlation_mismatch = matches!(response, FixtureResponse::CorrelationMismatch);
    let is_response_version_mismatch = matches!(response, FixtureResponse::ResponseVersionMismatch);
    let payload = match response {
        FixtureResponse::Health(health) => {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(health))
        }
        FixtureResponse::Rejected(error) => {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(error))
        }
        FixtureResponse::Snapshot(snapshot) => ProtocolResponsePayloadDto::QueryResult(
            ProtocolQueryResultDto::SessionSnapshot(snapshot),
        ),
        FixtureResponse::Subscription(subscription)
        | FixtureResponse::SubscriptionAfter(subscription, _) => {
            ProtocolResponsePayloadDto::Subscription(subscription)
        }
        FixtureResponse::Invalid
        | FixtureResponse::CorrelationMismatch
        | FixtureResponse::MissingCapabilities
        | FixtureResponse::ResponseVersionMismatch
        | FixtureResponse::Disconnect => ProtocolResponsePayloadDto::CommandResult(
            intention_protocol::ProtocolCommandResultDto::Rejected(ErrorDto::validation(
                "fixture_invalid_response",
                "fixture intentionally returns a mismatched payload",
            )),
        ),
        FixtureResponse::ProtocolMismatch => return,
    };
    let correlation_id = if is_correlation_mismatch {
        CorrelationIdDto::new()
    } else {
        request.correlation_id()
    };
    let response_version = if is_response_version_mismatch {
        ProtocolVersionDto::new(1, 1)
    } else {
        local_protocol_version()
    };
    connection
        .send_response(&ProtocolResponseEnvelopeDto::new(
            response_version,
            correlation_id,
            ProtocolMessageDto::new(SCHEMA_VERSION, payload),
        ))
        .expect("fixture response sends");
}

const fn ready_health() -> DaemonHealthDto {
    DaemonHealthDto::new(
        SCHEMA_VERSION,
        local_protocol_version(),
        DaemonReadinessDto::Ready,
    )
}

fn fixture_event(session_id: SessionId, sequence: u64) -> EventEnvelopeDto<DomainEventDto> {
    let occurred_at = TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid");
    EventEnvelopeDto::new(
        EventMetadataDto::new(
            SCHEMA_VERSION,
            EventId::new(),
            session_id,
            None,
            None,
            SessionEventSequenceDto::new(sequence),
            occurred_at,
        ),
        DomainEventDto::RunStatusChanged(RunStatusChangedEventDto::new(
            session_id,
            intention_types::RunId::new(),
            RunStatusDto::Running,
            occurred_at,
        )),
    )
}

const fn subscription(session_id: SessionId, after: u64) -> SubscribeSessionCommandDto {
    SubscribeSessionCommandDto::new(
        SCHEMA_VERSION,
        session_id,
        Some(SessionEventSequenceDto::new(after)),
        intention_domain::RunModeDto::Build,
    )
}

#[test]
fn process_launcher_and_client_metadata_reject_invalid_configuration() {
    let _guard = fixture_guard();
    assert_eq!(
        ProcessDaemonLauncher::new(" \t ")
            .expect_err("blank daemon program must fail")
            .code(),
        "invalid_daemon_program"
    );
    let directory = TempDir::new().expect("temporary directory is available");
    let invalid_program = ProcessDaemonLauncher::new("intention-daemon-does-not-exist")
        .expect("non-empty program is accepted as configuration");
    assert_eq!(
        invalid_program
            .launch(&endpoint(&directory))
            .expect_err("missing binary must return a safe launch error")
            .code(),
        "local_daemon_launch_failed"
    );
    let invalid_adapter =
        IntentionClient::new(endpoint(&directory), " ", Box::new(RejectingLauncher));
    assert!(invalid_adapter.is_err(), "blank adapter name must fail");
    let error = match invalid_adapter {
        Ok(_) => panic!("blank adapter name must fail"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "invalid_adapter_name");
}

#[test]
fn first_ready_connection_skips_launch_and_bootstrap_launches_after_unavailable() {
    let _guard = fixture_guard();
    let directory = TempDir::new().expect("temporary directory is available");
    let launches = Arc::new(AtomicUsize::new(0));
    let ready_endpoint = endpoint(&directory);
    let server = start_fixture_server(
        ready_endpoint.clone(),
        FixtureResponse::Health(ready_health()),
    );
    let health = client(
        ready_endpoint,
        FixtureResponse::Health(ready_health()),
        Arc::clone(&launches),
    )
    .connect_or_bootstrap()
    .expect("already-ready daemon must be used without launch");
    assert_eq!(health.readiness(), DaemonReadinessDto::Ready);
    assert_eq!(launches.load(Ordering::SeqCst), 0);
    server.join().expect("ready fixture server completes");

    let bootstrap_launches = Arc::new(AtomicUsize::new(0));
    let health = client(
        endpoint(&directory),
        FixtureResponse::Health(ready_health()),
        Arc::clone(&bootstrap_launches),
    )
    .connect_or_bootstrap()
    .expect("unavailable initial endpoint must bootstrap through launcher");
    assert_eq!(health.readiness(), DaemonReadinessDto::Ready);
    assert_eq!(bootstrap_launches.load(Ordering::SeqCst), 1);
}

#[test]
fn bootstrap_propagates_typed_launch_error() {
    let _guard = fixture_guard();
    let directory = TempDir::new().expect("temporary directory is available");
    let error = IntentionClient::new(
        endpoint(&directory),
        "fixture-client",
        Box::new(RejectingLauncher),
    )
    .expect("fixture client is valid")
    .connect_or_bootstrap()
    .expect_err("launch rejection must be visible to the caller");
    assert_eq!(error.code(), "fixture_launch_rejected");
}

#[test]
fn health_rejection_invalid_response_correlation_and_protocol_mismatch_are_typed() {
    let _guard = fixture_guard();
    let directory = TempDir::new().expect("temporary directory is available");
    let scenarios = [
        (
            FixtureResponse::Rejected(ErrorDto::validation("fixture_rejected", "no health")),
            "fixture_rejected",
        ),
        (FixtureResponse::Invalid, "invalid_local_protocol_response"),
        (
            FixtureResponse::CorrelationMismatch,
            "invalid_local_protocol_response",
        ),
        (
            FixtureResponse::ProtocolMismatch,
            "incompatible_protocol_version",
        ),
        (
            FixtureResponse::MissingCapabilities,
            "incompatible_protocol_capabilities",
        ),
        (
            FixtureResponse::ResponseVersionMismatch,
            "invalid_local_protocol_response",
        ),
        (
            FixtureResponse::Disconnect,
            "local_daemon_connection_unavailable",
        ),
    ];
    for (response, expected_code) in scenarios {
        let endpoint = endpoint(&directory);
        let server = start_fixture_server(endpoint.clone(), response.clone());
        let error = client(endpoint, response, Arc::new(AtomicUsize::new(0)))
            .health()
            .expect_err("fixture must return the selected health failure");
        assert_eq!(error.code(), expected_code);
        server.join().expect("failure fixture server completes");
    }
}

#[test]
fn snapshot_and_subscription_validate_success_rejection_and_response_shape() {
    let _guard = fixture_guard();
    let directory = TempDir::new().expect("temporary directory is available");
    let session_id = SessionId::new();
    let snapshot = SessionSnapshotDto::with_projection(
        SCHEMA_VERSION,
        session_id,
        SessionEventSequenceDto::new(4),
        fixture_projection(session_id, SessionEventSequenceDto::new(4)),
    )
    .expect("fixture snapshot is valid");
    let valid_snapshot_endpoint = endpoint(&directory);
    let server = start_fixture_server(
        valid_snapshot_endpoint.clone(),
        FixtureResponse::Snapshot(snapshot.clone()),
    );
    assert_eq!(
        client(
            valid_snapshot_endpoint,
            FixtureResponse::Snapshot(snapshot.clone()),
            Arc::new(AtomicUsize::new(0)),
        )
        .session_snapshot(session_id)
        .expect("typed snapshot response is returned"),
        snapshot
    );
    server.join().expect("snapshot fixture server completes");

    let rejected_endpoint = endpoint(&directory);
    let rejection = ErrorDto::validation("session_rejected", "fixture session rejected");
    let server = start_fixture_server(
        rejected_endpoint.clone(),
        FixtureResponse::Rejected(rejection.clone()),
    );
    assert_eq!(
        client(
            rejected_endpoint,
            FixtureResponse::Rejected(rejection),
            Arc::new(AtomicUsize::new(0)),
        )
        .session_snapshot(session_id)
        .expect_err("rejected snapshot must be propagated")
        .code(),
        "session_rejected"
    );
    server.join().expect("rejection fixture server completes");

    let invalid_snapshot_endpoint = endpoint(&directory);
    let server = start_fixture_server(invalid_snapshot_endpoint.clone(), FixtureResponse::Invalid);
    assert_eq!(
        client(
            invalid_snapshot_endpoint,
            FixtureResponse::Invalid,
            Arc::new(AtomicUsize::new(0)),
        )
        .session_snapshot(session_id)
        .expect_err("wrong snapshot payload must fail")
        .code(),
        "invalid_local_protocol_response"
    );
    server
        .join()
        .expect("invalid snapshot fixture server completes");

    let tail = SessionEventTailBatchDto::new(
        SCHEMA_VERSION,
        session_id,
        snapshot.at_sequence(),
        Vec::new(),
    )
    .expect("empty fixture tail is valid");
    let response = SessionSubscriptionResponseDto::snapshot_and_tail(snapshot.clone(), tail)
        .expect("matching snapshot and tail are valid");
    let valid_subscription_endpoint = endpoint(&directory);
    let server = start_fixture_server(
        valid_subscription_endpoint.clone(),
        FixtureResponse::Subscription(response.clone()),
    );
    let received = client(
        valid_subscription_endpoint,
        FixtureResponse::Subscription(response),
        Arc::new(AtomicUsize::new(0)),
    )
    .subscribe(subscription(session_id, 4))
    .expect("typed subscription response is returned");
    assert_eq!(
        received,
        SessionSubscriptionResponseDto::snapshot_and_tail(
            snapshot.clone(),
            SessionEventTailBatchDto::new(
                SCHEMA_VERSION,
                session_id,
                snapshot.at_sequence(),
                Vec::new(),
            )
            .expect("empty fixture tail is valid"),
        )
        .expect("fixture subscription is valid")
    );
    server
        .join()
        .expect("subscription fixture server completes");

    let invalid_subscription_endpoint = endpoint(&directory);
    let server = start_fixture_server(
        invalid_subscription_endpoint.clone(),
        FixtureResponse::Invalid,
    );
    assert_eq!(
        client(
            invalid_subscription_endpoint,
            FixtureResponse::Invalid,
            Arc::new(AtomicUsize::new(0)),
        )
        .subscribe(subscription(session_id, 4))
        .expect_err("wrong subscription payload must fail")
        .code(),
        "invalid_local_protocol_response"
    );
    server
        .join()
        .expect("invalid subscription fixture server completes");
}

#[test]
fn non_ready_health_is_not_returned_as_a_successful_connection() {
    let _guard = fixture_guard();
    let directory = TempDir::new().expect("temporary directory is available");
    for readiness in [
        DaemonReadinessDto::Starting,
        DaemonReadinessDto::Draining,
        DaemonReadinessDto::Unavailable,
    ] {
        let endpoint = endpoint(&directory);
        let server = start_fixture_server(
            endpoint.clone(),
            FixtureResponse::Health(DaemonHealthDto::new(
                SCHEMA_VERSION,
                local_protocol_version(),
                readiness,
            )),
        );
        let error = client(
            endpoint,
            FixtureResponse::Health(ready_health()),
            Arc::new(AtomicUsize::new(0)),
        )
        .health()
        .expect_err("only ready health can establish a client connection");
        let expected = if readiness == DaemonReadinessDto::Starting {
            "local_daemon_starting"
        } else {
            "local_daemon_not_ready"
        };
        assert_eq!(error.code(), expected);
        server.join().expect("non-ready fixture server completes");
    }
}

#[test]
fn stateful_recovery_reuses_the_last_sequence_or_clears_on_resync() {
    let _guard = fixture_guard();
    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
    let session_id = SessionId::new();
    let snapshot = SessionSnapshotDto::with_projection(
        SCHEMA_VERSION,
        session_id,
        SessionEventSequenceDto::new(0),
        fixture_projection(session_id, SessionEventSequenceDto::new(0)),
    )
    .expect("fixture snapshot is valid");
    let first = SessionSubscriptionResponseDto::snapshot_and_tail(
        snapshot.clone(),
        SessionEventTailBatchDto::new(
            SCHEMA_VERSION,
            session_id,
            snapshot.at_sequence(),
            vec![fixture_event(session_id, 1)],
        )
        .expect("fixture tail is contiguous"),
    )
    .expect("fixture subscription is valid");
    let resync = SessionSubscriptionResponseDto::resync_required(SessionResyncDto::new(
        SCHEMA_VERSION,
        session_id,
        SessionResyncReasonDto::HistoryUnavailable,
    ));
    let server = start_fixture_server_sequence(
        endpoint.clone(),
        vec![
            FixtureResponse::SubscriptionAfter(first, 0),
            FixtureResponse::SubscriptionAfter(resync, 1),
        ],
    );
    let client = client(
        endpoint,
        FixtureResponse::Health(ready_health()),
        Arc::new(AtomicUsize::new(0)),
    );
    let mut recovery = SessionSubscriptionRecovery::new(subscription(session_id, 0));
    assert!(
        !recovery
            .recover(&client)
            .expect("first recovery accepts snapshot and tail")
    );
    assert_eq!(
        recovery.last_sequence(),
        Some(SessionEventSequenceDto::new(1))
    );
    assert_eq!(recovery.snapshot(), Some(snapshot));
    assert!(
        recovery
            .recover(&client)
            .expect("typed resync clears local projection")
    );
    assert_eq!(recovery.snapshot(), None);
    assert_eq!(recovery.last_sequence(), None);
    server.join().expect("recovery fixture server completes");
}

#[test]
fn subscription_reducer_accepts_ordered_state_and_requires_resync_for_bad_state() {
    let _guard = fixture_guard();
    let session_id = SessionId::new();
    let snapshot = SessionSnapshotDto::with_projection(
        SCHEMA_VERSION,
        session_id,
        SessionEventSequenceDto::new(0),
        fixture_projection(session_id, SessionEventSequenceDto::new(0)),
    )
    .expect("fixture snapshot is valid");
    let event = fixture_event(session_id, 1);
    let tail = SessionEventTailBatchDto::new(
        SCHEMA_VERSION,
        session_id,
        snapshot.at_sequence(),
        vec![event.clone()],
    )
    .expect("fixture tail is contiguous");
    let mut reducer = SessionSubscriptionReducer::new(session_id);
    assert!(
        !reducer
            .apply(
                SessionSubscriptionResponseDto::snapshot_and_tail(snapshot.clone(), tail)
                    .expect("fixture subscription is valid"),
            )
            .expect("ordered snapshot and tail are accepted")
    );
    assert_eq!(reducer.snapshot(), Some(snapshot));
    assert_eq!(
        reducer.last_sequence(),
        Some(SessionEventSequenceDto::new(1))
    );
    reducer
        .apply_event(&event)
        .expect("duplicate event is ignored");
    reducer
        .apply_event(&fixture_event(session_id, 0))
        .expect("stale event is ignored");
    assert_eq!(
        reducer
            .apply_event(&fixture_event(session_id, 3))
            .expect_err("sequence gap requires recovery")
            .code(),
        "subscription_sequence_gap"
    );
    assert_eq!(
        reducer
            .apply_event(&fixture_event(SessionId::new(), 2))
            .expect_err("another session is rejected")
            .code(),
        "invalid_subscription_session"
    );
    assert!(
        reducer
            .apply(SessionSubscriptionResponseDto::resync_required(
                SessionResyncDto::new(
                    SCHEMA_VERSION,
                    session_id,
                    SessionResyncReasonDto::HistoryUnavailable,
                )
            ))
            .expect("matching resync clears local state")
    );
    assert_eq!(reducer.snapshot(), None);
    assert_eq!(reducer.last_sequence(), None);
    assert_eq!(
        reducer
            .apply(SessionSubscriptionResponseDto::resync_required(
                SessionResyncDto::new(
                    SCHEMA_VERSION,
                    SessionId::new(),
                    SessionResyncReasonDto::InvalidPosition,
                )
            ))
            .expect_err("wrong-session resync is rejected")
            .code(),
        "invalid_subscription_session"
    );
}
