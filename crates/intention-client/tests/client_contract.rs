#![allow(
    clippy::await_holding_lock,
    reason = "Client contract fixtures use direct assertions and controlled fixture launchers; the standard fixture mutex serializes independent fixture servers, and every async test owns its own single-threaded runtime, so holding that guard across awaits cannot deadlock."
)]

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::{TEST_REPLY_BOUND, endpoint, message};

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use intention_client::{
    DaemonLauncher, IntentionClient, ProcessDaemonLauncher, SessionStreamState,
    daemon_program_beside, endpoint_from_declared,
};
use intention_proto::TimestampDto;
use intention_proto::{
    ClientRequestDto, CreateSessionAcceptedDto, CreateSessionCommandDto, CreateSessionFirstTurnDto,
    DaemonHealthDto, ProtocolDaemonMessageDto, ProtocolResultDto, RunCancelledAcceptedDto, RunId,
    RunProjectionDto, RunStatusDto, SendUserTurnAcceptedDto, SendUserTurnOutcomeDto,
    SessionFrameDto, SessionId, SessionSnapshotDto, SessionSummariesDto, SessionSummaryDto,
    TextDeltaChannelDto, TextDeltaFrameDto, TurnId, decode_request_line, encode_request,
};
use intention_proto::{DtoResult, ErrorCategoryDto, ErrorDto, ProjectId, WorkspaceId};
use intention_proto::{
    MessageId, MessageKindDto, MessageProjectionDto, RunModeDto, SessionProjectionDto,
};
use intention_transport::{AsyncLocalDaemonConnection, AsyncLocalListener, LocalEndpoint};

/// Returns one valid fixture timestamp in whole Unix seconds.
fn timestamp(seconds: i64) -> TimestampDto {
    TimestampDto::from_unix_seconds(seconds).expect("fixture timestamp is valid")
}

/// Returns the fixture workspace root shared by the session fixtures.
fn fixture_workspace_root() -> intention_proto::WorkspaceRootDto {
    intention_proto::WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-client-fixture-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("fixture workspace root is valid")
}

fn fixture_projection(session_id: SessionId) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        fixture_workspace_root(),
        RunModeDto::Build,
        None,
    )
    .expect("fixture projection is valid")
}

/// Returns the same fixture projection with one active run.
fn fixture_projection_with_run(
    session_id: SessionId,
    run_id: RunId,
    status: RunStatusDto,
) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        fixture_workspace_root(),
        RunModeDto::Build,
        Some(fixture_run(session_id, run_id, status)),
    )
    .expect("fixture projection with a run is valid")
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

/// Serves one combined creation, echoing its first turn into the acceptance.
///
/// The task returns the exact command it received, so the test asserts the
/// outbound wire shape instead of only the parsed reply.
fn start_first_turn_server(
    endpoint: LocalEndpoint,
    session_id: SessionId,
) -> tokio::task::JoinHandle<CreateSessionCommandDto> {
    let listener = AsyncLocalListener::bind(endpoint).expect("fixture listener binds");
    tokio::spawn(async move {
        let connection = listener.accept().await.expect("fixture client connects");
        let (mut requests, mut messages) = connection.split();
        let line = requests
            .receive_line()
            .await
            .expect("fixture request arrives");
        let request = decode_request_line(&line).expect("fixture request decodes");
        let ClientRequestDto::CreateSession(command) = request.request().clone() else {
            panic!("the client sends one create-session command")
        };
        let acceptance = CreateSessionAcceptedDto::new(
            command.project_id(),
            command.workspace_id(),
            command.session_id(),
            command.first_turn().map(|_| {
                SendUserTurnAcceptedDto::new(
                    session_id,
                    TurnId::new(),
                    SendUserTurnOutcomeDto::Pending,
                )
            }),
        );
        messages
            .send_message(&ProtocolDaemonMessageDto::reply(
                request.id(),
                ProtocolResultDto::SessionCreated(acceptance),
            ))
            .await
            .expect("fixture reply sends");
        command
    })
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
        Some(RunModeDto::Build),
        None,
    )
}

fn fixture_run(session_id: SessionId, run_id: RunId, status: RunStatusDto) -> RunProjectionDto {
    let started_at = timestamp(1_726_000_000);
    let finished_at = intention_proto::run_status_is_terminal(status).then_some(started_at);
    RunProjectionDto::new(
        session_id,
        run_id,
        TurnId::new(),
        status,
        None,
        started_at,
        finished_at,
        0,
    )
    .expect("fixture run projection is valid")
}

fn fixture_summary(session_id: SessionId, updated_at: i64) -> SessionSummaryDto {
    SessionSummaryDto::new(
        session_id,
        ProjectId::new(),
        WorkspaceId::new(),
        RunModeDto::Build,
        timestamp(updated_at),
        timestamp(updated_at),
        fixture_workspace_root(),
        0,
    )
}

fn fixture_snapshot(
    session_id: SessionId,
    run_id: RunId,
    messages: Vec<MessageProjectionDto>,
) -> SessionSnapshotDto {
    SessionSnapshotDto::with_projection(
        session_id,
        fixture_projection_with_run(session_id, run_id, RunStatusDto::Running),
        messages,
    )
    .expect("fixture session snapshot is valid")
}

fn fixture_delta(session_id: SessionId, run_id: RunId, step: u32, text: &str) -> TextDeltaFrameDto {
    TextDeltaFrameDto::new(session_id, run_id, step, TextDeltaChannelDto::Answer, text)
        .expect("fixture text delta is valid")
}

fn fixture_reasoning_delta(
    session_id: SessionId,
    run_id: RunId,
    step: u32,
    text: &str,
) -> TextDeltaFrameDto {
    TextDeltaFrameDto::new(
        session_id,
        run_id,
        step,
        TextDeltaChannelDto::Reasoning,
        text,
    )
    .expect("fixture reasoning delta is valid")
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

#[test]
fn endpoint_discovery_prefers_a_declared_instance_and_falls_back_to_the_platform_default() {
    let declared =
        endpoint_from_declared(Some("declared-instance")).expect("a safe instance resolves");
    assert_eq!(
        declared.instance_id(),
        "declared-instance",
        "a declared instance identifier wins over the platform default"
    );

    let platform = LocalEndpoint::platform_default().expect("the platform default resolves");
    let omitted = endpoint_from_declared(None).expect("no declared instance resolves");
    assert_eq!(
        omitted.instance_id(),
        platform.instance_id(),
        "no declared instance uses the platform-default endpoint"
    );
    let blank = endpoint_from_declared(Some("   ")).expect("a blank instance resolves");
    assert_eq!(
        blank.instance_id(),
        platform.instance_id(),
        "a blank declared instance falls back to the platform default"
    );
}

#[test]
fn the_daemon_program_defaults_to_the_terminal_binary_sibling() {
    let directory = std::env::temp_dir();
    let executable = directory.join("intention-tui");
    let expected = directory.join(format!("intention-daemon{}", std::env::consts::EXE_SUFFIX));
    let program = daemon_program_beside(&executable).expect("the sibling daemon program resolves");
    assert_eq!(
        Path::new(&program),
        expected.as_path(),
        "the daemon program is the executable's sibling"
    );
    assert_eq!(
        daemon_program_beside(Path::new(""))
            .expect_err("an executable path with no parent is unavailable")
            .code(),
        "daemon_program_unavailable"
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
    assert_eq!(health, DaemonHealthDto::ready());
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
    assert_eq!(health, DaemonHealthDto::ready());
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
            FixtureResponse::Result(ProtocolResultDto::RunCancelled(
                RunCancelledAcceptedDto::new(SessionId::new(), RunId::new()),
            )),
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
async fn a_create_command_carries_its_first_turn() {
    let _guard = fixture_guard();
    let session_id = SessionId::new();
    let command = CreateSessionCommandDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        fixture_workspace_root(),
        Some(RunModeDto::Build),
        Some(CreateSessionFirstTurnDto::new("first prompt").expect("fixture first turn is valid")),
    );

    let first_turn_endpoint = endpoint();
    let server = start_first_turn_server(first_turn_endpoint.clone(), session_id);
    let created = client(
        first_turn_endpoint,
        FixtureResponse::Disconnect,
        Arc::new(AtomicUsize::new(0)),
    )
    .create_session(command.clone())
    .await
    .expect("the daemon accepts the combined creation");
    assert_eq!(created.session_id(), session_id);
    let first_turn = created
        .first_turn()
        .expect("the acceptance echoes the first turn");
    assert_eq!(first_turn.session_id(), session_id);
    assert_eq!(first_turn.outcome(), SendUserTurnOutcomeDto::Pending);
    assert_eq!(
        server.await.expect("first-turn fixture server completes"),
        command,
        "the client carries the first turn on the wire through its one create method"
    );
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
    assert_eq!(health, DaemonHealthDto::ready());
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
async fn a_correlated_reply_is_answered_without_re_checking_its_entity_identity() {
    let _guard = fixture_guard();
    let session_id = SessionId::new();

    // The reply identity the daemon echoes is what proves this answer belongs
    // to this request; the entity the answer names is not re-validated here.
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
    let received = client(
        snapshot_endpoint,
        FixtureResponse::Result(ProtocolResultDto::SessionSnapshot(foreign_snapshot.clone())),
        Arc::new(AtomicUsize::new(0)),
    )
    .session_snapshot(session_id)
    .await
    .expect("the correlated reply is the answer to this request");
    assert_eq!(received, foreign_snapshot);
    server.await.expect("scope fixture server completes");
}

#[tokio::test]
async fn invalid_response_does_not_launch_a_daemon() {
    let _guard = fixture_guard();
    let endpoint = endpoint();
    let launches = Arc::new(AtomicUsize::new(0));
    let response = FixtureResponse::Result(ProtocolResultDto::RunCancelled(
        RunCancelledAcceptedDto::new(SessionId::new(), RunId::new()),
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
async fn foreign_peer_on_the_current_endpoint_fails_closed() {
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
    assert_eq!(error.code(), "invalid_local_protocol_response");
    assert_eq!(error.category(), ErrorCategoryDto::Validation);
    server.await.expect("foreign fixture server completes");
}

#[tokio::test]
async fn list_sessions_round_trips_and_validates_the_reply() {
    let _guard = fixture_guard();
    let newest = SessionId::new();
    let summaries = SessionSummariesDto::new(
        vec![
            SessionSummaryDto::new(
                newest,
                ProjectId::new(),
                WorkspaceId::new(),
                RunModeDto::Build,
                timestamp(2_000),
                timestamp(2_000),
                fixture_workspace_root(),
                3,
            ),
            fixture_summary(SessionId::new(), 1_000),
        ],
        4,
    )
    .expect("fixture session summaries are valid");
    let list_endpoint = endpoint();
    let server = start_fixture_server(
        list_endpoint.clone(),
        FixtureResponse::Result(ProtocolResultDto::SessionsListed(summaries.clone())),
    );
    let received = client(
        list_endpoint,
        FixtureResponse::Result(ProtocolResultDto::SessionsListed(summaries.clone())),
        Arc::new(AtomicUsize::new(0)),
    )
    .list_sessions()
    .await
    .expect("typed session list response is returned");
    assert_eq!(received, summaries);
    assert_eq!(received.sessions().len(), 2);
    assert_eq!(received.sessions()[0].session_id(), newest);
    assert_eq!(
        received.sessions()[0].message_count(),
        3,
        "a summary round-trips its committed row count"
    );
    assert_eq!(received.omitted(), 4);
    server.await.expect("session list fixture server completes");

    // Another result kind is not this request's answer.
    let invalid_endpoint = endpoint();
    let server = start_fixture_server(
        invalid_endpoint.clone(),
        FixtureResponse::Result(ProtocolResultDto::RunCancelled(
            RunCancelledAcceptedDto::new(SessionId::new(), RunId::new()),
        )),
    );
    assert_eq!(
        client(
            invalid_endpoint,
            FixtureResponse::Result(ProtocolResultDto::RunCancelled(
                RunCancelledAcceptedDto::new(SessionId::new(), RunId::new())
            )),
            Arc::new(AtomicUsize::new(0)),
        )
        .list_sessions()
        .await
        .expect_err("a non-list result is not this request's answer")
        .code(),
        "invalid_local_protocol_response"
    );
    server.await.expect("invalid list fixture server completes");
}

#[tokio::test]
async fn continue_session_takes_the_first_listed_session() {
    let _guard = fixture_guard();
    let newest = SessionId::new();
    let older = SessionId::new();
    let summaries = SessionSummariesDto::new(
        vec![
            fixture_summary(newest, 2_000),
            fixture_summary(older, 1_000),
        ],
        0,
    )
    .expect("fixture session summaries are valid");
    let list_endpoint = endpoint();
    let server = start_fixture_server(
        list_endpoint.clone(),
        FixtureResponse::Result(ProtocolResultDto::SessionsListed(summaries.clone())),
    );
    let target = client(
        list_endpoint,
        FixtureResponse::Result(ProtocolResultDto::SessionsListed(summaries)),
        Arc::new(AtomicUsize::new(0)),
    )
    .continue_session()
    .await
    .expect("a session list reply resolves the continue target");
    assert_eq!(
        target, newest,
        "the continue target is the first session of the daemon's order"
    );
    server.await.expect("session list fixture server completes");
}

#[tokio::test]
async fn continue_session_reports_no_sessions_to_continue() {
    let _guard = fixture_guard();
    let empty_endpoint = endpoint();
    let empty = SessionSummariesDto::new(Vec::new(), 0).expect("an empty session list is coherent");
    let server = start_fixture_server(
        empty_endpoint.clone(),
        FixtureResponse::Result(ProtocolResultDto::SessionsListed(empty.clone())),
    );
    let error = client(
        empty_endpoint,
        FixtureResponse::Result(ProtocolResultDto::SessionsListed(empty)),
        Arc::new(AtomicUsize::new(0)),
    )
    .continue_session()
    .await
    .expect_err("an empty session list has no continue target");
    assert_eq!(error.code(), "no_sessions_to_continue");
    assert_eq!(error.category(), ErrorCategoryDto::NotFound);
    assert!(!error.retryable());
    server.await.expect("empty list fixture server completes");
}

#[test]
fn state_tracks_provisional_text_per_model_step() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = SessionStreamState::new(session_id);
    let committed = message(session_id, Some(run_id), MessageKindDto::User, "build it");
    state
        .reconcile(SessionStreamState::from_snapshot(fixture_snapshot(
            session_id,
            run_id,
            vec![committed.clone()],
        )))
        .expect("fixture snapshot applies");
    assert_eq!(state.provisional_text(), "");

    // Deltas of one step append to each other.
    state
        .apply_frame(SessionFrameDto::TextDelta(fixture_delta(
            session_id, run_id, 0, "Hel",
        )))
        .expect("first delta applies");
    state
        .apply_frame(SessionFrameDto::TextDelta(fixture_delta(
            session_id, run_id, 0, "lo",
        )))
        .expect("second delta of the same step applies");
    assert_eq!(state.provisional_text(), "Hello");
    assert_eq!(
        state.messages(),
        &[committed],
        "provisional deltas never touch the committed transcript"
    );

    // A delta of a run this state no longer projects is a cadence frame that
    // crossed its run's own terminal commit: it changes nothing.
    state
        .apply_frame(SessionFrameDto::TextDelta(fixture_delta(
            session_id,
            RunId::new(),
            0,
            "stray",
        )))
        .expect("a delta of another run of the session applies as a no-op");
    assert_eq!(state.provisional_text(), "Hello");

    // A delta for a new step replaces the buffer.
    state
        .apply_frame(SessionFrameDto::TextDelta(fixture_delta(
            session_id, run_id, 1, "next",
        )))
        .expect("a delta of the next step applies");
    assert_eq!(state.provisional_text(), "next");

    // A fresh correlated snapshot resets provisional text with committed state:
    // the read a subscription reply is carries the snapshot's rows alone and no
    // transient text at all.
    let next_committed = message(
        session_id,
        Some(run_id),
        MessageKindDto::Assistant,
        "committed by the snapshot",
    );
    let fresh = SessionStreamState::from_snapshot(fixture_snapshot(
        session_id,
        run_id,
        vec![next_committed.clone()],
    ));
    assert_eq!(fresh.provisional_text(), "");
    assert_eq!(fresh.messages(), &[next_committed]);
}

#[test]
fn state_clears_provisional_text_on_the_committed_assistant_row() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = SessionStreamState::new(session_id);
    state
        .reconcile(SessionStreamState::from_snapshot(fixture_snapshot(
            session_id,
            run_id,
            Vec::new(),
        )))
        .expect("fixture snapshot applies");
    state
        .apply_frame(SessionFrameDto::TextDelta(fixture_delta(
            session_id, run_id, 0, "half",
        )))
        .expect("provisional delta applies");
    assert_eq!(state.provisional_text(), "half");

    state
        .apply_frame(SessionFrameDto::Content(message(
            session_id,
            Some(run_id),
            MessageKindDto::Notice,
            "still running",
        )))
        .expect("a committed notice frame applies");
    assert_eq!(
        state.provisional_text(),
        "half",
        "only the step's committed assistant row supersedes its provisional text"
    );

    state
        .apply_frame(SessionFrameDto::Content(message(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            "half",
        )))
        .expect("the committed assistant row applies");
    assert_eq!(state.provisional_text(), "");
    assert_eq!(state.messages().len(), 2);
    assert_eq!(
        state.messages().last().expect("rows remain").text(),
        "half",
        "the committed transcript carries the row while the buffer is dropped"
    );
}

#[test]
fn state_clears_provisional_reasoning_on_the_committed_reasoning_only_row() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = SessionStreamState::new(session_id);
    state
        .reconcile(SessionStreamState::from_snapshot(fixture_snapshot(
            session_id,
            run_id,
            Vec::new(),
        )))
        .expect("fixture snapshot applies");
    state
        .apply_frame(SessionFrameDto::TextDelta(fixture_reasoning_delta(
            session_id, run_id, 0, "thinking",
        )))
        .expect("provisional reasoning applies");
    assert_eq!(state.provisional_reasoning(), "thinking");

    // The step's reasoning-only assistant row commits before the tool rows of
    // the same step, so the provisional reasoning is superseded by the row
    // that now carries it.
    state
        .apply_frame(SessionFrameDto::Content(
            MessageProjectionDto::new(
                MessageId::new(1).expect("fixture row identity is valid"),
                session_id,
                Some(run_id),
                MessageKindDto::Assistant,
                "",
                Some("thinking".to_owned()),
                None,
            )
            .expect("a reasoning-only assistant row is coherent"),
        ))
        .expect("the committed reasoning-only row applies");
    assert_eq!(state.provisional_reasoning(), "");
    assert_eq!(state.provisional_text(), "");
    let row = state.messages().last().expect("the row remains");
    assert_eq!(row.kind(), MessageKindDto::Assistant);
    assert_eq!(row.text(), "");
    assert_eq!(row.reasoning(), Some("thinking"));
}

#[test]
fn state_buffers_the_reasoning_channel_apart_from_the_answer() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = SessionStreamState::new(session_id);
    state
        .reconcile(SessionStreamState::from_snapshot(fixture_snapshot(
            session_id,
            run_id,
            Vec::new(),
        )))
        .expect("fixture snapshot applies");
    assert_eq!(state.provisional_text(), "");
    assert_eq!(state.provisional_reasoning(), "");

    // The reasoning of one step accumulates on its own channel and never
    // becomes answer text, even when the two channels interleave.
    for delta in [
        fixture_reasoning_delta(session_id, run_id, 0, "weigh"),
        fixture_delta(session_id, run_id, 0, "the "),
        fixture_reasoning_delta(session_id, run_id, 0, "ing"),
        fixture_delta(session_id, run_id, 0, "answer"),
    ] {
        state
            .apply_frame(SessionFrameDto::TextDelta(delta))
            .expect("a delta of the current step applies");
    }
    assert_eq!(state.provisional_reasoning(), "weighing");
    assert_eq!(state.provisional_text(), "the answer");
    assert!(
        state.messages().is_empty(),
        "neither channel is committed state"
    );

    // A delta of the next step replaces both channels' buffers.
    state
        .apply_frame(SessionFrameDto::TextDelta(fixture_reasoning_delta(
            session_id, run_id, 1, "then",
        )))
        .expect("the next step's reasoning applies");
    assert_eq!(state.provisional_reasoning(), "then");
    assert_eq!(
        state.provisional_text(),
        "",
        "the previous step's answer does not outlive its step"
    );

    // The step's committed assistant row supersedes both channels at once.
    state
        .apply_frame(SessionFrameDto::Content(message(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            "the answer",
        )))
        .expect("the committed assistant row applies");
    assert_eq!(state.provisional_reasoning(), "");
    assert_eq!(state.provisional_text(), "");
    assert_eq!(state.messages().len(), 1);
}

#[test]
fn state_clears_provisional_text_when_the_run_reaches_a_terminal_status() {
    let session_id = SessionId::new();
    let run_id = RunId::new();

    // A non-terminal status frame keeps the current step's advance notice.
    let mut state = SessionStreamState::new(session_id);
    state
        .reconcile(SessionStreamState::from_snapshot(fixture_snapshot(
            session_id,
            run_id,
            Vec::new(),
        )))
        .expect("fixture snapshot applies");
    state
        .apply_frame(SessionFrameDto::TextDelta(fixture_delta(
            session_id, run_id, 0, "half",
        )))
        .expect("provisional delta applies");
    state
        .apply_frame(SessionFrameDto::Status(fixture_run(
            session_id,
            run_id,
            RunStatusDto::Running,
        )))
        .expect("a running status frame applies");
    assert_eq!(
        state.provisional_text(),
        "half",
        "a running run keeps its provisional text"
    );

    // A terminal status ends the run: the daemon discards its unpublished
    // delta window, so no advance notice may outlive the run.
    for terminal in [
        RunStatusDto::Failed,
        RunStatusDto::Completed,
        RunStatusDto::Interrupted,
    ] {
        let mut state = SessionStreamState::new(session_id);
        state
            .reconcile(SessionStreamState::from_snapshot(fixture_snapshot(
                session_id,
                run_id,
                Vec::new(),
            )))
            .expect("fixture snapshot applies");
        state
            .apply_frame(SessionFrameDto::TextDelta(fixture_delta(
                session_id, run_id, 0, "half",
            )))
            .expect("provisional delta applies");
        assert_eq!(state.provisional_text(), "half");

        state
            .apply_frame(SessionFrameDto::Status(fixture_run(
                session_id, run_id, terminal,
            )))
            .expect("a terminal status frame applies");
        assert_eq!(
            state.provisional_text(),
            "",
            "a {terminal:?} run must not retain provisional text"
        );
        assert_eq!(state.status(), Some(terminal));
        assert!(
            state.messages().is_empty(),
            "status frames never write transcript rows"
        );
    }
}
