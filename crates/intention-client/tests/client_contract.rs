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

use common::{TEST_REPLY_BOUND, endpoint, message};

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use intention_client::{DaemonLauncher, IntentionClient, ProcessDaemonLauncher};
use intention_proto::{
    ClientRequestDto, CreateSessionCommandDto, DaemonHealthDto, DaemonReadinessDto,
    InterruptRunAcceptedDto, ProtocolDaemonMessageDto, ProtocolResultDto, RemoveTurnAcceptedDto,
    RunId, SessionId, SessionSnapshotDto, TurnId, decode_request_line, encode_request,
};
use intention_proto::{DtoResult, ErrorCategoryDto, ErrorDto, ProjectId, WorkspaceId};
use intention_proto::{MessageKindDto, RunModeDto, SessionProjectionDto};
use intention_transport::{AsyncLocalDaemonConnection, AsyncLocalListener, LocalEndpoint};

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
    /// A valid current-wire reply carrying the result the fixture selects.
    Result(ProtocolResultDto),
    /// A peer that answers a current-wire request with another dialect's line.
    Foreign,
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
    IntentionClient::new(endpoint, Box::new(FixtureLauncher { response, launches }))
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
    let (mut requests, mut messages) = connection.split();
    if matches!(response, FixtureResponse::Disconnect) {
        return;
    }
    let line = requests
        .receive_line()
        .await
        .expect("fixture request arrives");
    let request = decode_request_line(&line).expect("fixture request decodes");
    if matches!(response, FixtureResponse::Foreign) {
        // Another dialect answers with its own envelope: a typed request line is
        // not a daemon message of the current wire.
        messages
            .send_message(&encode_request(
                request.id(),
                ClientRequestDto::GetDaemonHealth,
            ))
            .await
            .expect("fixture foreign line sends");
        return;
    }
    messages
        .send_message(&fixture_reply(request.id(), &response))
        .await
        .expect("fixture reply sends");
}

fn fixture_reply(request_id: u64, response: &FixtureResponse) -> ProtocolDaemonMessageDto {
    match response {
        FixtureResponse::Health(health) => {
            ProtocolDaemonMessageDto::reply(request_id, ProtocolResultDto::DaemonHealth(*health))
        }
        FixtureResponse::Rejected(error) => {
            ProtocolDaemonMessageDto::rejection(Some(request_id), error.clone())
        }
        FixtureResponse::Snapshot(snapshot) => ProtocolDaemonMessageDto::reply(
            request_id,
            ProtocolResultDto::SessionSnapshot(snapshot.clone()),
        ),
        FixtureResponse::Result(result) => {
            ProtocolDaemonMessageDto::reply(request_id, result.clone())
        }
        // The peer that answers with a foreign line and the peer that closes the
        // channel never reach this encoder.
        FixtureResponse::Foreign | FixtureResponse::Disconnect => ProtocolDaemonMessageDto::reply(
            request_id,
            ProtocolResultDto::DaemonHealth(DaemonHealthDto::ready()),
        ),
    }
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

#[test]
fn process_launcher_and_client_construction_reject_invalid_configuration() {
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
}

#[tokio::test]
async fn first_ready_connection_skips_launch_and_bootstrap_launches_after_unavailable() {
    let _guard = fixture_guard();
    let launches = Arc::new(AtomicUsize::new(0));
    let ready_endpoint = endpoint();
    let server = start_fixture_server(
        ready_endpoint.clone(),
        FixtureResponse::Health(DaemonHealthDto::ready()),
    );
    let health = client(
        ready_endpoint,
        FixtureResponse::Health(DaemonHealthDto::ready()),
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
        FixtureResponse::Health(DaemonHealthDto::ready()),
        Arc::clone(&bootstrap_launches),
    )
    .connect_or_bootstrap()
    .await
    .expect("unavailable initial endpoint must bootstrap through launcher");
    assert_eq!(health.readiness(), DaemonReadinessDto::Ready);
    assert_eq!(bootstrap_launches.load(Ordering::SeqCst), 1);

    let error = IntentionClient::new(endpoint(), Box::new(RejectingLauncher))
        .connect_or_bootstrap()
        .await
        .expect_err("launch rejection must be visible to the caller");
    assert_eq!(error.code(), "fixture_launch_rejected");
}

#[tokio::test]
async fn connection_rejection_and_invalid_response_are_typed() {
    let _guard = fixture_guard();
    let scenarios = [
        (
            FixtureResponse::Rejected(ErrorDto::validation("fixture_rejected", "no health")),
            "fixture_rejected",
        ),
        (
            FixtureResponse::Result(ProtocolResultDto::TurnRemoved(RemoveTurnAcceptedDto::new(
                SessionId::new(),
                TurnId::new(),
            ))),
            "invalid_local_protocol_response",
        ),
    ];
    for (response, expected_code) in scenarios {
        let endpoint = endpoint();
        let server = start_fixture_server(endpoint.clone(), response.clone());
        let error = client(endpoint, response, Arc::new(AtomicUsize::new(0)))
            .await_ready()
            .await
            .expect_err("fixture must return the selected failure");
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
async fn session_snapshot_validates_success_and_rejection() {
    let _guard = fixture_guard();
    let session_id = SessionId::new();
    let snapshot = SessionSnapshotDto::with_projection(
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
}

#[tokio::test]
async fn command_rejection_is_typed() {
    let _guard = fixture_guard();
    let session_id = SessionId::new();
    let rejection = ErrorDto::validation("fixture_command_rejected", "fixture command rejected");
    let rejected_endpoint = endpoint();
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
        .create_session(fixture_create_command(session_id))
        .await
        .expect_err("a rejected command must surface the daemon error")
        .code(),
        "fixture_command_rejected"
    );
    server.await.expect("rejection fixture server completes");
}

#[tokio::test]
async fn await_ready_returns_ready_health_and_reports_an_unavailable_daemon() {
    let _guard = fixture_guard();

    let ready_endpoint = endpoint();
    let server = start_fixture_server(
        ready_endpoint.clone(),
        FixtureResponse::Health(DaemonHealthDto::ready()),
    );
    let health = client(
        ready_endpoint,
        FixtureResponse::Health(DaemonHealthDto::ready()),
        Arc::new(AtomicUsize::new(0)),
    )
    .await_ready()
    .await
    .expect("ready daemon health is returned by the readiness wait");
    assert_eq!(health.readiness(), DaemonReadinessDto::Ready);
    server.await.expect("ready fixture server completes");

    // An endpoint nobody serves ends the bounded wait with the typed
    // unavailable error instead of being reported as readiness.
    let error = client(
        endpoint(),
        FixtureResponse::Health(DaemonHealthDto::ready()),
        Arc::new(AtomicUsize::new(0)),
    )
    .await_ready()
    .await
    .expect_err("an unavailable daemon must not be reported ready");
    assert_eq!(error.code(), "local_daemon_unavailable");
}

#[tokio::test]
async fn correlated_replies_are_scope_checked() {
    let _guard = fixture_guard();
    let session_id = SessionId::new();

    // A snapshot for another session is not this request's answer.
    let other_session = SessionId::new();
    let foreign_snapshot = SessionSnapshotDto::with_projection(
        other_session,
        fixture_projection(other_session),
        Vec::new(),
    )
    .expect("fixture snapshot is valid");
    let snapshot_endpoint = endpoint();
    let server = start_fixture_server(
        snapshot_endpoint.clone(),
        FixtureResponse::Result(ProtocolResultDto::SessionSnapshot(foreign_snapshot.clone())),
    );
    assert_eq!(
        client(
            snapshot_endpoint,
            FixtureResponse::Result(ProtocolResultDto::SessionSnapshot(foreign_snapshot)),
            Arc::new(AtomicUsize::new(0)),
        )
        .session_snapshot(session_id)
        .await
        .expect_err("a snapshot for another session is not this request's answer")
        .code(),
        "invalid_local_protocol_response"
    );
    server.await.expect("scope fixture server completes");

    // A removal for another turn is not this request's answer.
    let removed_endpoint = endpoint();
    let server = start_fixture_server(
        removed_endpoint.clone(),
        FixtureResponse::Result(ProtocolResultDto::TurnRemoved(RemoveTurnAcceptedDto::new(
            session_id,
            TurnId::new(),
        ))),
    );
    assert_eq!(
        client(
            removed_endpoint,
            FixtureResponse::Result(ProtocolResultDto::TurnRemoved(RemoveTurnAcceptedDto::new(
                session_id,
                TurnId::new(),
            ))),
            Arc::new(AtomicUsize::new(0)),
        )
        .remove_turn(session_id, TurnId::new())
        .await
        .expect_err("a removal of another turn is not this request's answer")
        .code(),
        "invalid_local_protocol_response"
    );
    server.await.expect("scope fixture server completes");

    // An interruption for another run is not this request's answer.
    let interrupted_endpoint = endpoint();
    let server = start_fixture_server(
        interrupted_endpoint.clone(),
        FixtureResponse::Result(ProtocolResultDto::RunInterrupted(
            InterruptRunAcceptedDto::new(session_id, RunId::new()),
        )),
    );
    assert_eq!(
        client(
            interrupted_endpoint,
            FixtureResponse::Result(ProtocolResultDto::RunInterrupted(
                InterruptRunAcceptedDto::new(session_id, RunId::new()),
            )),
            Arc::new(AtomicUsize::new(0)),
        )
        .interrupt_run(session_id, RunId::new())
        .await
        .expect_err("an interruption of another run is not this request's answer")
        .code(),
        "invalid_local_protocol_response"
    );
    server.await.expect("scope fixture server completes");
}

#[tokio::test]
async fn invalid_response_does_not_launch_a_daemon() {
    let _guard = fixture_guard();
    let endpoint = endpoint();
    let launches = Arc::new(AtomicUsize::new(0));
    let response = FixtureResponse::Result(ProtocolResultDto::TurnRemoved(
        RemoveTurnAcceptedDto::new(SessionId::new(), TurnId::new()),
    ));
    let server = start_fixture_server(endpoint.clone(), response.clone());
    let error = client(endpoint, response, Arc::clone(&launches))
        .connect_or_bootstrap()
        .await
        .expect_err("a current-wire payload failure is not a stale peer");
    assert_eq!(error.code(), "invalid_local_protocol_response");
    assert_eq!(error.category(), ErrorCategoryDto::Validation);
    assert_eq!(
        launches.load(Ordering::SeqCst),
        0,
        "an unretryable protocol failure never launches a daemon"
    );
    server
        .await
        .expect("invalid-response fixture server completes");
}

#[tokio::test]
async fn foreign_peer_on_the_current_endpoint_is_stale() {
    let _guard = fixture_guard();
    let endpoint = endpoint();
    let server = start_fixture_server(endpoint.clone(), FixtureResponse::Foreign);
    let error = client(
        endpoint,
        FixtureResponse::Foreign,
        Arc::new(AtomicUsize::new(0)),
    )
    .session_snapshot(SessionId::new())
    .await
    .expect_err("a foreign peer on the current endpoint fails closed");
    assert_eq!(error.code(), "stale_daemon_protocol");
    assert_eq!(error.category(), ErrorCategoryDto::Unavailable);
    server.await.expect("foreign fixture server completes");
}
