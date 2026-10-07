#![allow(
    clippy::await_holding_lock,
    clippy::expect_used,
    clippy::panic,
    reason = "Client contract fixtures use direct assertions and controlled fixture launchers; the standard fixture mutex serializes independent fixture servers, and every async test owns its own single-threaded runtime, so holding that guard across awaits cannot deadlock."
)]

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use intention_client::{DaemonLauncher, IntentionClient, ProcessDaemonLauncher};
use intention_domain::{MessageKindDto, MessageProjectionDto, RunModeDto, SessionProjectionDto};
use intention_proto::{DtoResult, ErrorDto, ProjectId, SchemaVersionDto, SessionId, WorkspaceId};
use intention_protocol::{
    DaemonHealthDto, DaemonReadinessDto, JsonRpcErrorDto, JsonRpcRequestDto, JsonRpcResponseDto,
    PROTOCOL_HELLO_METHOD, ProtocolHelloDto, ProtocolQueryResultDto, ProtocolResponsePayloadDto,
    ProtocolVersionDto, SessionSnapshotDto, SessionSubscriptionResponseDto,
    SubscribeSessionCommandDto, decode_request_line, encode_hello_response, encode_response,
};
use intention_transport::{LocalEndpoint, LocalListener, local_protocol_version, negotiate_daemon};
use tempfile::TempDir;

const SCHEMA_VERSION: SchemaVersionDto = intention_protocol::CURRENT_DTO_SCHEMA_VERSION;
/// Bound that turns a hanging client call into a visible test failure.
const TEST_REPLY_BOUND: Duration = Duration::from_secs(5);

fn fixture_projection(session_id: SessionId) -> SessionProjectionDto {
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
    )
    .expect("fixture projection is valid")
}

fn fixture_message(session_id: SessionId) -> MessageProjectionDto {
    MessageProjectionDto::new(
        session_id,
        None,
        MessageKindDto::Notice,
        "fixture notice",
        None,
        None,
        None,
    )
    .expect("fixture transcript row is valid")
}

#[derive(Clone)]
enum FixtureResponse {
    Health(DaemonHealthDto),
    Rejected(ErrorDto),
    Snapshot(SessionSnapshotDto),
    Subscription(SessionSubscriptionResponseDto),
    Invalid,
    CorrelationMismatch,
    /// The fixture daemon answers the hello with the typed version-mismatch error.
    ProtocolMismatch,
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
    ProtocolHelloDto::new(local_protocol_version(), "fixture-daemon")
        .expect("fixture daemon hello is valid")
}

fn start_fixture_server(
    endpoint: LocalEndpoint,
    response: FixtureResponse,
) -> thread::JoinHandle<()> {
    let listener = LocalListener::bind(endpoint).expect("fixture listener binds");
    thread::spawn(move || serve_one_fixture_connection(listener, response))
}

fn serve_one_fixture_connection(listener: LocalListener, response: FixtureResponse) {
    let connection = listener.accept().expect("fixture client connects");
    serve_fixture_connection(connection, response);
}

fn serve_fixture_connection(
    mut connection: intention_transport::LocalConnection,
    response: FixtureResponse,
) {
    if matches!(response, FixtureResponse::ProtocolMismatch)
        || matches!(response, FixtureResponse::MinorProtocolMismatch)
    {
        let line = connection
            .receive_line()
            .expect("fixture client hello arrives");
        let request: JsonRpcRequestDto<ProtocolHelloDto> =
            JsonRpcRequestDto::parse(&line).expect("fixture client hello parses");
        assert_eq!(request.method(), PROTOCOL_HELLO_METHOD);
        if matches!(response, FixtureResponse::ProtocolMismatch) {
            let error = JsonRpcErrorDto::from_error(
                intention_protocol::JSONRPC_VERSION_MISMATCH,
                ErrorDto::unavailable(
                    "incompatible_protocol_version",
                    "protocol version must equal the current version",
                ),
            );
            connection
                .send_message(&JsonRpcResponseDto::<ProtocolHelloDto>::error(
                    Some(request.id()),
                    error,
                ))
                .expect("fixture version-mismatch error sends");
        } else {
            let incompatible =
                ProtocolHelloDto::new(ProtocolVersionDto::new(2, 1), "minor-mismatched-daemon")
                    .expect("fixture mismatch hello is valid");
            connection
                .send_message(&encode_hello_response(request.id(), incompatible))
                .expect("fixture mismatch hello sends");
        }
        return;
    }
    negotiate_daemon(&mut connection, daemon_hello()).expect("fixture hello negotiates");
    if matches!(response, FixtureResponse::Disconnect) {
        return;
    }
    let line = connection.receive_line().expect("fixture request arrives");
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
        FixtureResponse::Invalid
        | FixtureResponse::CorrelationMismatch
        | FixtureResponse::Disconnect => ProtocolResponsePayloadDto::CommandResult(
            intention_protocol::ProtocolCommandResultDto::Rejected(ErrorDto::validation(
                "fixture_invalid_response",
                "fixture intentionally returns a mismatched payload",
            )),
        ),
        FixtureResponse::ProtocolMismatch | FixtureResponse::MinorProtocolMismatch => return,
    };
    let id = if matches!(response, FixtureResponse::CorrelationMismatch) {
        request.id() + 100
    } else {
        request.id()
    };
    connection
        .send_message(&encode_response(id, payload))
        .expect("fixture response sends");
}

const fn ready_health() -> DaemonHealthDto {
    DaemonHealthDto::new(
        SCHEMA_VERSION,
        local_protocol_version(),
        DaemonReadinessDto::Ready,
    )
}

const fn subscription(session_id: SessionId) -> SubscribeSessionCommandDto {
    SubscribeSessionCommandDto::new(SCHEMA_VERSION, session_id, RunModeDto::Build)
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

#[tokio::test]
async fn first_ready_connection_skips_launch_and_bootstrap_launches_after_unavailable() {
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
    .await
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
    .await
    .expect("unavailable initial endpoint must bootstrap through launcher");
    assert_eq!(health.readiness(), DaemonReadinessDto::Ready);
    assert_eq!(bootstrap_launches.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn bootstrap_propagates_typed_launch_error() {
    let _guard = fixture_guard();
    let directory = TempDir::new().expect("temporary directory is available");
    let error = IntentionClient::new(
        endpoint(&directory),
        "fixture-client",
        Box::new(RejectingLauncher),
    )
    .expect("fixture client is valid")
    .connect_or_bootstrap()
    .await
    .expect_err("launch rejection must be visible to the caller");
    assert_eq!(error.code(), "fixture_launch_rejected");
}

#[tokio::test]
async fn health_rejection_invalid_response_correlation_and_protocol_mismatch_are_typed() {
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
            FixtureResponse::MinorProtocolMismatch,
            "incompatible_protocol_version",
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
            .await
            .expect_err("fixture must return the selected health failure");
        assert_eq!(error.code(), expected_code);
        server.join().expect("failure fixture server completes");
    }
}

#[tokio::test]
async fn closed_response_channel_is_a_typed_error_instead_of_a_hang() {
    let _guard = fixture_guard();
    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
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
    server.join().expect("closed fixture server completes");
}

#[tokio::test]
async fn snapshot_and_subscription_validate_success_rejection_and_response_shape() {
    let _guard = fixture_guard();
    let directory = TempDir::new().expect("temporary directory is available");
    let session_id = SessionId::new();
    let snapshot = SessionSnapshotDto::with_projection(
        SCHEMA_VERSION,
        session_id,
        fixture_projection(session_id),
        vec![fixture_message(session_id)],
    )
    .expect("fixture snapshot is valid");
    let valid_snapshot_endpoint = endpoint(&directory);
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
        .await
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
        .await
        .expect_err("wrong snapshot payload must fail")
        .code(),
        "invalid_local_protocol_response"
    );
    server
        .join()
        .expect("invalid snapshot fixture server completes");

    let response = SessionSubscriptionResponseDto::snapshot(snapshot.clone());
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
    .subscribe(subscription(session_id))
    .await
    .expect("typed subscription response is returned");
    assert_eq!(received, SessionSubscriptionResponseDto::snapshot(snapshot));
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
        .subscribe(subscription(session_id))
        .await
        .expect_err("wrong subscription payload must fail")
        .code(),
        "invalid_local_protocol_response"
    );
    server
        .join()
        .expect("invalid subscription fixture server completes");
}

#[tokio::test]
async fn non_ready_health_is_not_returned_as_a_successful_connection() {
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
        .await
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
