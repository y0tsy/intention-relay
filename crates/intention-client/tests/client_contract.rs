#![allow(
    clippy::await_holding_lock,
    clippy::expect_used,
    clippy::panic,
    reason = "Client contract fixtures use direct assertions and controlled fixture launchers; the standard fixture mutex serializes independent fixture servers, and every async test owns its own single-threaded runtime, so holding that guard across awaits cannot deadlock."
)]

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::{SCHEMA_VERSION, TEST_REPLY_BOUND, endpoint, hello, message};

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use intention_client::{DaemonLauncher, IntentionClient, ProcessDaemonLauncher};
use intention_proto::{
    ConfigRevisionId, CorrelationIdDto, CreateSessionAcceptedDto, CreateSessionCommandDto,
    DaemonHealthDto, DaemonReadinessDto, IdempotencyKey, ProtocolAcceptedDto,
    ProtocolAcceptedResultDto, ProtocolCommandDto, ProtocolCommandResultDto, ProtocolHelloDto,
    ProtocolQueryResultDto, ProtocolResponsePayloadDto, ProtocolVersionDto,
    SendUserTurnAcceptedDto, SendUserTurnOutcomeDto, SessionSnapshotDto,
    SessionSubscriptionResponseDto, SubscribeSessionCommandDto, TurnId, decode_request_line,
    encode_response,
};
use intention_proto::{DtoResult, ErrorDto, ProjectId, RunId, SessionId, WorkspaceId};
use intention_proto::{MessageKindDto, RunModeDto, SessionProjectionDto};
use intention_transport::{
    AsyncLocalClientConnection, AsyncLocalDaemonConnection, AsyncLocalListener, LocalEndpoint,
    local_protocol_version,
};

fn fixture_projection(session_id: SessionId) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        intention_proto::WorkspaceRootDto::parse(
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
    )
    .expect("fixture projection is valid")
}

#[derive(Clone)]
enum FixtureResponse {
    Health(DaemonHealthDto),
    Rejected(ErrorDto),
    Snapshot(SessionSnapshotDto),
    Subscription(SessionSubscriptionResponseDto),
    Command(ProtocolCommandResultDto),
    Invalid,
    CorrelationMismatch,
    /// The fixture daemon replies with a same-major minor-mismatched hello.
    MinorProtocolMismatch,
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
        // The fixture task is detached: it serves the launched endpoint until
        // the test's runtime shuts down.
        drop(start_fixture_server(
            endpoint.clone(),
            self.response.clone(),
        ));
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

static FIXTURE_CONNECTIONS: Mutex<()> = Mutex::new(());

fn fixture_guard() -> MutexGuard<'static, ()> {
    FIXTURE_CONNECTIONS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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

fn start_fixture_server(
    endpoint: LocalEndpoint,
    response: FixtureResponse,
) -> tokio::task::JoinHandle<()> {
    let listener = AsyncLocalListener::bind(endpoint).expect("fixture listener binds");
    tokio::spawn(serve_one_fixture_connection(listener, response))
}

async fn serve_one_fixture_connection(listener: AsyncLocalListener, response: FixtureResponse) {
    let connection = listener.accept().await.expect("fixture client connects");
    serve_fixture_connection(connection, response).await;
}

async fn serve_fixture_connection(
    connection: AsyncLocalDaemonConnection,
    response: FixtureResponse,
) {
    // The daemon gate checks the client's exact version before replying, so a
    // future minor version in the fixture daemon's own handshake exercises the
    // client-side exact-version gate.
    let local = if matches!(response, FixtureResponse::MinorProtocolMismatch) {
        ProtocolHelloDto::new(ProtocolVersionDto::new(2, 1), "minor-mismatched-daemon")
            .expect("fixture mismatch hello is valid")
    } else {
        hello("fixture-daemon")
    };
    let (_, mut requests, mut messages) = connection
        .negotiate(local)
        .await
        .expect("fixture hello negotiates");
    if matches!(
        response,
        FixtureResponse::MinorProtocolMismatch | FixtureResponse::Disconnect
    ) {
        return;
    }
    let line = requests
        .receive_line()
        .await
        .expect("fixture request arrives");
    let request = decode_request_line(&line).expect("fixture request decodes");
    let payload = match &response {
        FixtureResponse::Health(health) => {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(*health))
        }
        FixtureResponse::Rejected(error) => {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(error.clone()))
        }
        FixtureResponse::Snapshot(snapshot) => ProtocolResponsePayloadDto::QueryResult(
            ProtocolQueryResultDto::SessionSnapshot(snapshot.clone()),
        ),
        FixtureResponse::Subscription(subscription) => {
            ProtocolResponsePayloadDto::Subscription(subscription.clone())
        }
        FixtureResponse::Command(result) => {
            ProtocolResponsePayloadDto::CommandResult(result.clone())
        }
        FixtureResponse::Invalid
        | FixtureResponse::CorrelationMismatch
        | FixtureResponse::Disconnect => ProtocolResponsePayloadDto::CommandResult(
            intention_proto::ProtocolCommandResultDto::Rejected(ErrorDto::validation(
                "fixture_invalid_response",
                "fixture intentionally returns a mismatched payload",
            )),
        ),
        FixtureResponse::MinorProtocolMismatch => return,
    };
    let id = if matches!(response, FixtureResponse::CorrelationMismatch) {
        request.id() + 100
    } else {
        request.id()
    };
    messages
        .send_message(&encode_response(id, payload))
        .await
        .expect("fixture response sends");
}

const fn ready_health() -> DaemonHealthDto {
    DaemonHealthDto::new(
        SCHEMA_VERSION,
        local_protocol_version(),
        DaemonReadinessDto::Ready,
    )
}

const fn starting_health() -> DaemonHealthDto {
    DaemonHealthDto::new(
        SCHEMA_VERSION,
        local_protocol_version(),
        DaemonReadinessDto::Starting,
    )
}

const fn subscription(session_id: SessionId) -> SubscribeSessionCommandDto {
    SubscribeSessionCommandDto::new(SCHEMA_VERSION, session_id, RunModeDto::Build)
}

fn fixture_create_command(session_id: SessionId) -> CreateSessionCommandDto {
    CreateSessionCommandDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        intention_proto::WorkspaceRootDto::parse(
            std::env::temp_dir()
                .join("intention-client-fixture-workspace")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture workspace root is valid"),
        RunModeDto::Build,
    )
}

/// Wraps one acceptance payload as the daemon's correlated command result.
fn accepted_command(result: ProtocolAcceptedResultDto) -> ProtocolCommandResultDto {
    ProtocolCommandResultDto::Accepted(ProtocolAcceptedDto::with_result(
        CorrelationIdDto::new(),
        result,
    ))
}

/// Serves `Starting` health on every connection until `stop` is set.
///
/// The readiness wait reconnects for the whole bounded budget, so a
/// single-connection script cannot produce the final typed starting error.
fn start_starting_health_server(
    endpoint: LocalEndpoint,
    stop: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    let listener = AsyncLocalListener::bind(endpoint).expect("fixture listener binds");
    tokio::spawn(async move {
        loop {
            let connection = listener.accept().await.expect("fixture client connects");
            if stop.load(Ordering::SeqCst) {
                return;
            }
            serve_fixture_connection(connection, FixtureResponse::Health(starting_health())).await;
        }
    })
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
    let invalid_program = ProcessDaemonLauncher::new("intention-daemon-does-not-exist")
        .expect("non-empty program is accepted as configuration");
    assert_eq!(
        invalid_program
            .launch(&endpoint())
            .expect_err("missing binary must return a safe launch error")
            .code(),
        "local_daemon_launch_failed"
    );
    let invalid_adapter = IntentionClient::new(endpoint(), " ", Box::new(RejectingLauncher));
    assert!(invalid_adapter.is_err(), "blank adapter name must fail");
    let error = match invalid_adapter {
        Ok(_) => panic!("blank adapter name must fail"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "invalid_adapter_name");
}

#[tokio::test]
async fn first_ready_connection_skips_launch_and_bootstrap_launches_after_unavailable() {
    let _guard = fixture_guard();
    let launches = Arc::new(AtomicUsize::new(0));
    let ready_endpoint = endpoint();
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
    .await
    .expect("already-ready daemon must be used without launch");
    assert_eq!(health.readiness(), DaemonReadinessDto::Ready);
    assert_eq!(launches.load(Ordering::SeqCst), 0);
    server.await.expect("ready fixture server completes");

    let bootstrap_launches = Arc::new(AtomicUsize::new(0));
    let health = client(
        endpoint(),
        FixtureResponse::Health(ready_health()),
        Arc::clone(&bootstrap_launches),
    )
    .connect_or_bootstrap()
    .await
    .expect("unavailable initial endpoint must bootstrap through launcher");
    assert_eq!(health.readiness(), DaemonReadinessDto::Ready);
    assert_eq!(bootstrap_launches.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn bootstrap_propagates_typed_launch_error() {
    let _guard = fixture_guard();
    let error = IntentionClient::new(endpoint(), "fixture-client", Box::new(RejectingLauncher))
        .expect("fixture client is valid")
        .connect_or_bootstrap()
        .await
        .expect_err("launch rejection must be visible to the caller");
    assert_eq!(error.code(), "fixture_launch_rejected");
}

#[tokio::test]
async fn health_rejection_invalid_response_correlation_and_protocol_mismatch_are_typed() {
    let _guard = fixture_guard();
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
            FixtureResponse::MinorProtocolMismatch,
            "incompatible_protocol_version",
        ),
        (
            FixtureResponse::Disconnect,
            "local_daemon_connection_unavailable",
        ),
    ];
    for (response, expected_code) in scenarios {
        let endpoint = endpoint();
        let server = start_fixture_server(endpoint.clone(), response.clone());
        let error = client(endpoint, response, Arc::new(AtomicUsize::new(0)))
            .health()
            .await
            .expect_err("fixture must return the selected health failure");
        assert_eq!(error.code(), expected_code);
        server.await.expect("failure fixture server completes");
    }
}

#[tokio::test]
async fn closed_response_channel_is_a_typed_error_instead_of_a_hang() {
    let _guard = fixture_guard();
    let endpoint = endpoint();
    let server = start_fixture_server(endpoint.clone(), FixtureResponse::Disconnect);
    let error = tokio::time::timeout(
        TEST_REPLY_BOUND,
        client(
            endpoint,
            FixtureResponse::Disconnect,
            Arc::new(AtomicUsize::new(0)),
        )
        .session_snapshot(SessionId::new()),
    )
    .await
    .expect("a closed response channel must not hang the request")
    .expect_err("a closed response channel is a typed error");
    assert_eq!(error.code(), "local_daemon_connection_unavailable");
    server.await.expect("closed fixture server completes");
}

#[tokio::test]
async fn snapshot_and_subscription_validate_success_rejection_and_response_shape() {
    let _guard = fixture_guard();
    let session_id = SessionId::new();
    let snapshot = SessionSnapshotDto::with_projection(
        SCHEMA_VERSION,
        session_id,
        fixture_projection(session_id),
        vec![message(
            session_id,
            None,
            MessageKindDto::Notice,
            "fixture notice",
        )],
    )
    .expect("fixture snapshot is valid");
    let valid_snapshot_endpoint = endpoint();
    let server = start_fixture_server(
        valid_snapshot_endpoint.clone(),
        FixtureResponse::Snapshot(snapshot.clone()),
    );
    let received = client(
        valid_snapshot_endpoint,
        FixtureResponse::Snapshot(snapshot.clone()),
        Arc::new(AtomicUsize::new(0)),
    )
    .session_snapshot(session_id)
    .await
    .expect("typed snapshot response is returned");
    assert_eq!(received, snapshot);
    assert_eq!(received.session_id(), session_id);
    assert_eq!(received.messages(), snapshot.messages());
    server.await.expect("snapshot fixture server completes");

    let rejected_endpoint = endpoint();
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
        .await
        .expect_err("rejected snapshot must be propagated")
        .code(),
        "session_rejected"
    );
    server.await.expect("rejection fixture server completes");

    let invalid_snapshot_endpoint = endpoint();
    let server = start_fixture_server(invalid_snapshot_endpoint.clone(), FixtureResponse::Invalid);
    assert_eq!(
        client(
            invalid_snapshot_endpoint,
            FixtureResponse::Invalid,
            Arc::new(AtomicUsize::new(0)),
        )
        .session_snapshot(session_id)
        .await
        .expect_err("wrong snapshot payload must fail")
        .code(),
        "invalid_local_protocol_response"
    );
    server
        .await
        .expect("invalid snapshot fixture server completes");

    let response = SessionSubscriptionResponseDto::snapshot(snapshot.clone());
    let valid_subscription_endpoint = endpoint();
    let server = start_fixture_server(
        valid_subscription_endpoint.clone(),
        FixtureResponse::Subscription(response.clone()),
    );
    let received = client(
        valid_subscription_endpoint,
        FixtureResponse::Subscription(response),
        Arc::new(AtomicUsize::new(0)),
    )
    .subscribe(subscription(session_id))
    .await
    .expect("typed subscription response is returned");
    assert_eq!(received, SessionSubscriptionResponseDto::snapshot(snapshot));
    server.await.expect("subscription fixture server completes");

    let invalid_subscription_endpoint = endpoint();
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
        .subscribe(subscription(session_id))
        .await
        .expect_err("wrong subscription payload must fail")
        .code(),
        "invalid_local_protocol_response"
    );
    server
        .await
        .expect("invalid subscription fixture server completes");
}

#[tokio::test]
async fn non_ready_health_is_not_returned_as_a_successful_connection() {
    let _guard = fixture_guard();
    for readiness in [
        DaemonReadinessDto::Starting,
        DaemonReadinessDto::Draining,
        DaemonReadinessDto::Unavailable,
    ] {
        let endpoint = endpoint();
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
        .await
        .expect_err("only ready health can establish a client connection");
        let expected = if readiness == DaemonReadinessDto::Starting {
            "local_daemon_starting"
        } else {
            "local_daemon_not_ready"
        };
        assert_eq!(error.code(), expected);
        server.await.expect("non-ready fixture server completes");
    }
}

#[tokio::test]
async fn command_and_conveniences_round_trip_typed_acceptances() {
    let _guard = fixture_guard();
    let session_id = SessionId::new();
    let created = CreateSessionAcceptedDto::new(ProjectId::new(), WorkspaceId::new(), session_id);

    let command_endpoint = endpoint();
    let accepted = accepted_command(ProtocolAcceptedResultDto::CreateSession(created));
    let server = start_fixture_server(
        command_endpoint.clone(),
        FixtureResponse::Command(accepted.clone()),
    );
    let received = client(
        command_endpoint,
        FixtureResponse::Command(accepted.clone()),
        Arc::new(AtomicUsize::new(0)),
    )
    .command(ProtocolCommandDto::CreateSession(fixture_create_command(
        session_id,
    )))
    .await
    .expect("an accepted command result is returned as decoded data");
    assert_eq!(received, accepted);
    server.await.expect("command fixture server completes");

    let create_endpoint = endpoint();
    let accepted = accepted_command(ProtocolAcceptedResultDto::CreateSession(created));
    let server = start_fixture_server(
        create_endpoint.clone(),
        FixtureResponse::Command(accepted.clone()),
    );
    let received = client(
        create_endpoint,
        FixtureResponse::Command(accepted),
        Arc::new(AtomicUsize::new(0)),
    )
    .create_session(fixture_create_command(session_id))
    .await
    .expect("session creation returns the daemon acceptance evidence");
    assert_eq!(received, created);
    assert_eq!(received.session_id(), session_id);
    server
        .await
        .expect("create-session fixture server completes");

    let outcome = SendUserTurnOutcomeDto::Started {
        run_id: RunId::new(),
        config_revision_id: ConfigRevisionId::new(),
    };
    let accepted = accepted_command(ProtocolAcceptedResultDto::SendUserTurn(
        SendUserTurnAcceptedDto::new(session_id, TurnId::new(), outcome),
    ));
    let turn_endpoint = endpoint();
    let server = start_fixture_server(
        turn_endpoint.clone(),
        FixtureResponse::Command(accepted.clone()),
    );
    let received = client(
        turn_endpoint,
        FixtureResponse::Command(accepted),
        Arc::new(AtomicUsize::new(0)),
    )
    .send_user_turn(
        session_id,
        IdempotencyKey::new(),
        "fixture user turn".to_owned(),
    )
    .await
    .expect("the user turn outcome is returned as decoded data");
    assert_eq!(received, outcome);
    server
        .await
        .expect("send-user-turn fixture server completes");
}

#[tokio::test]
async fn command_rejection_and_unexpected_acceptance_payloads_are_typed() {
    let _guard = fixture_guard();
    let session_id = SessionId::new();

    let rejection = ErrorDto::validation("fixture_command_rejected", "fixture command rejected");
    let rejected_endpoint = endpoint();
    let rejected = ProtocolCommandResultDto::Rejected(rejection.clone());
    let server = start_fixture_server(
        rejected_endpoint.clone(),
        FixtureResponse::Command(rejected.clone()),
    );
    assert_eq!(
        client(
            rejected_endpoint,
            FixtureResponse::Command(rejected),
            Arc::new(AtomicUsize::new(0)),
        )
        .create_session(fixture_create_command(session_id))
        .await
        .expect_err("a rejected command must surface the daemon error")
        .code(),
        "fixture_command_rejected"
    );
    server.await.expect("rejection fixture server completes");

    let wrong_turn = accepted_command(ProtocolAcceptedResultDto::SendUserTurn(
        SendUserTurnAcceptedDto::new(session_id, TurnId::new(), SendUserTurnOutcomeDto::Pending),
    ));
    let create_shape_endpoint = endpoint();
    let server = start_fixture_server(
        create_shape_endpoint.clone(),
        FixtureResponse::Command(wrong_turn.clone()),
    );
    assert_eq!(
        client(
            create_shape_endpoint,
            FixtureResponse::Command(wrong_turn),
            Arc::new(AtomicUsize::new(0)),
        )
        .create_session(fixture_create_command(session_id))
        .await
        .expect_err("user-turn evidence must not satisfy session creation")
        .code(),
        "local_command_shape_mismatch"
    );
    server
        .await
        .expect("create-session shape fixture server completes");

    let wrong_create = accepted_command(ProtocolAcceptedResultDto::CreateSession(
        CreateSessionAcceptedDto::new(ProjectId::new(), WorkspaceId::new(), session_id),
    ));
    let turn_shape_endpoint = endpoint();
    let server = start_fixture_server(
        turn_shape_endpoint.clone(),
        FixtureResponse::Command(wrong_create.clone()),
    );
    assert_eq!(
        client(
            turn_shape_endpoint,
            FixtureResponse::Command(wrong_create),
            Arc::new(AtomicUsize::new(0)),
        )
        .send_user_turn(
            session_id,
            IdempotencyKey::new(),
            "fixture user turn".to_owned()
        )
        .await
        .expect_err("session-creation evidence must not satisfy a user turn")
        .code(),
        "local_command_shape_mismatch"
    );
    server
        .await
        .expect("send-user-turn shape fixture server completes");
}

#[tokio::test]
async fn await_ready_returns_ready_health_and_reports_starting() {
    let _guard = fixture_guard();

    let ready_endpoint = endpoint();
    let server = start_fixture_server(
        ready_endpoint.clone(),
        FixtureResponse::Health(ready_health()),
    );
    let health = client(
        ready_endpoint,
        FixtureResponse::Health(ready_health()),
        Arc::new(AtomicUsize::new(0)),
    )
    .await_ready()
    .await
    .expect("ready daemon health is returned by the readiness wait");
    assert_eq!(health.readiness(), DaemonReadinessDto::Ready);
    server.await.expect("ready fixture server completes");

    let starting_endpoint = endpoint();
    let stop = Arc::new(AtomicBool::new(false));
    let server = start_starting_health_server(starting_endpoint.clone(), Arc::clone(&stop));
    let error = client(
        starting_endpoint.clone(),
        FixtureResponse::Health(ready_health()),
        Arc::new(AtomicUsize::new(0)),
    )
    .await_ready()
    .await
    .expect_err("a starting daemon must not be reported ready");
    assert_eq!(error.code(), "local_daemon_starting");
    stop.store(true, Ordering::SeqCst);
    let _final_connection = AsyncLocalClientConnection::connect(&starting_endpoint)
        .await
        .expect("fixture listener accepts the final connection");
    server.await.expect("starting fixture server stops");
}
