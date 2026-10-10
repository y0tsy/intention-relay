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

use intention_client::{DaemonLauncher, IntentionClient, ProcessDaemonLauncher, RunStreamState};
use intention_proto::{
    ClientRequestDto, ConfigRevisionId, CreateSessionCommandDto, DaemonHealthDto,
    DaemonReadinessDto, InterruptRunAcceptedDto, ProtocolDaemonMessageDto, ProtocolResultDto,
    RemoveTurnAcceptedDto, RunId, RunProjectionDto, RunStatusDto, RunStreamFrameDto,
    RunSubscriptionSnapshotDto, SessionId, SessionSnapshotDto, SessionSummariesDto,
    SessionSummaryDto, TextDeltaChannelDto, TextDeltaFrameDto, ThemeDto, TuiSettingsDto,
    TuiThemeAcceptedDto, TurnId, decode_request_line, encode_request,
};
use intention_proto::{DtoResult, ErrorCategoryDto, ErrorDto, ProjectId, WorkspaceId};
use intention_proto::{MessageKindDto, MessageProjectionDto, RunModeDto, SessionProjectionDto};
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

fn fixture_run(session_id: SessionId, run_id: RunId, status: RunStatusDto) -> RunProjectionDto {
    RunProjectionDto::new(
        session_id,
        run_id,
        TurnId::new(),
        status,
        ConfigRevisionId::new(),
    )
}

fn fixture_summary(session_id: SessionId, updated_at: i64) -> SessionSummaryDto {
    SessionSummaryDto::new(
        session_id,
        ProjectId::new(),
        WorkspaceId::new(),
        RunModeDto::Build,
        updated_at,
        None,
    )
}

fn fixture_snapshot(
    session_id: SessionId,
    run_id: RunId,
    messages: Vec<MessageProjectionDto>,
) -> RunSubscriptionSnapshotDto {
    RunSubscriptionSnapshotDto::new(
        fixture_run(session_id, run_id, RunStatusDto::Running),
        messages,
    )
    .expect("fixture run snapshot is valid")
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

#[tokio::test]
async fn list_sessions_round_trips_and_validates_the_reply() {
    let _guard = fixture_guard();
    let newest = SessionId::new();
    let run_id = RunId::new();
    let summaries = SessionSummariesDto::new(
        vec![
            SessionSummaryDto::new(
                newest,
                ProjectId::new(),
                WorkspaceId::new(),
                RunModeDto::Build,
                2_000,
                Some(fixture_run(newest, run_id, RunStatusDto::Running)),
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
        received.sessions()[0].active_run().map(|run| run.run_id()),
        Some(run_id)
    );
    assert_eq!(
        received.sessions()[1].active_run(),
        None,
        "a session without an active run round-trips as inactive"
    );
    assert_eq!(received.omitted(), 4);
    server.await.expect("session list fixture server completes");

    // Another result kind is not this request's answer.
    let invalid_endpoint = endpoint();
    let server = start_fixture_server(
        invalid_endpoint.clone(),
        FixtureResponse::Result(ProtocolResultDto::TurnRemoved(RemoveTurnAcceptedDto::new(
            SessionId::new(),
            TurnId::new(),
        ))),
    );
    assert_eq!(
        client(
            invalid_endpoint,
            FixtureResponse::Result(ProtocolResultDto::TurnRemoved(RemoveTurnAcceptedDto::new(
                SessionId::new(),
                TurnId::new(),
            ))),
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
async fn tui_settings_and_theme_selection_round_trip_and_validate_the_reply() {
    let _guard = fixture_guard();
    let settings = TuiSettingsDto::new(ThemeDto::Dark);
    let settings_endpoint = endpoint();
    let server = start_fixture_server(
        settings_endpoint.clone(),
        FixtureResponse::Result(ProtocolResultDto::TuiSettings(settings)),
    );
    let received = client(
        settings_endpoint,
        FixtureResponse::Result(ProtocolResultDto::TuiSettings(settings)),
        Arc::new(AtomicUsize::new(0)),
    )
    .tui_settings()
    .await
    .expect("typed terminal settings are returned");
    assert_eq!(received, settings);
    assert_eq!(received.theme(), ThemeDto::Dark);
    server.await.expect("settings fixture server completes");

    // Another result kind is not the settings answer.
    let invalid_endpoint = endpoint();
    let invalid =
        ProtocolResultDto::TurnRemoved(RemoveTurnAcceptedDto::new(SessionId::new(), TurnId::new()));
    let server = start_fixture_server(
        invalid_endpoint.clone(),
        FixtureResponse::Result(invalid.clone()),
    );
    assert_eq!(
        client(
            invalid_endpoint,
            FixtureResponse::Result(invalid),
            Arc::new(AtomicUsize::new(0)),
        )
        .tui_settings()
        .await
        .expect_err("a non-settings result is not this request's answer")
        .code(),
        "invalid_local_protocol_response"
    );
    server
        .await
        .expect("invalid settings fixture server completes");

    // A theme selection round-trips its accepted theme.
    let accepted = TuiThemeAcceptedDto::new(ThemeDto::Dark);
    let accepted_endpoint = endpoint();
    let server = start_fixture_server(
        accepted_endpoint.clone(),
        FixtureResponse::Result(ProtocolResultDto::TuiThemeSet(accepted)),
    );
    let received = client(
        accepted_endpoint,
        FixtureResponse::Result(ProtocolResultDto::TuiThemeSet(accepted)),
        Arc::new(AtomicUsize::new(0)),
    )
    .set_tui_theme(ThemeDto::Dark)
    .await
    .expect("typed theme acceptance is returned");
    assert_eq!(received, accepted);
    assert_eq!(received.theme(), ThemeDto::Dark);
    server.await.expect("theme fixture server completes");

    // An acceptance that names another theme is not this command's answer.
    let mismatch_endpoint = endpoint();
    let mismatch = ProtocolResultDto::TuiThemeSet(TuiThemeAcceptedDto::new(ThemeDto::Light));
    let server = start_fixture_server(
        mismatch_endpoint.clone(),
        FixtureResponse::Result(mismatch.clone()),
    );
    assert_eq!(
        client(
            mismatch_endpoint,
            FixtureResponse::Result(mismatch),
            Arc::new(AtomicUsize::new(0)),
        )
        .set_tui_theme(ThemeDto::Dark)
        .await
        .expect_err("an acceptance of another theme is not this command's answer")
        .code(),
        "invalid_local_protocol_response"
    );
    server
        .await
        .expect("mismatched theme fixture server completes");
}

#[test]
fn state_tracks_provisional_text_per_model_step() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = RunStreamState::new(session_id, run_id);
    let committed = message(session_id, Some(run_id), MessageKindDto::User, "build it");
    state
        .apply_initial(fixture_snapshot(
            session_id,
            run_id,
            vec![committed.clone()],
        ))
        .expect("fixture snapshot applies");
    assert_eq!(state.provisional_text(), "");

    // Deltas of one step append to each other.
    state
        .apply_frame(RunStreamFrameDto::TextDelta(fixture_delta(
            session_id, run_id, 0, "Hel",
        )))
        .expect("first delta applies");
    state
        .apply_frame(RunStreamFrameDto::TextDelta(fixture_delta(
            session_id, run_id, 0, "lo",
        )))
        .expect("second delta of the same step applies");
    assert_eq!(state.provisional_text(), "Hello");
    assert_eq!(
        state.messages(),
        &[committed],
        "provisional deltas never touch the committed transcript"
    );

    // A delta for another scope is rejected without mutation.
    assert_eq!(
        state
            .apply_frame(RunStreamFrameDto::TextDelta(fixture_delta(
                session_id,
                RunId::new(),
                0,
                "stray",
            )))
            .expect_err("a delta from another run is rejected")
            .code(),
        "invalid_run_subscription"
    );
    assert_eq!(state.provisional_text(), "Hello");

    // A delta for a new step replaces the buffer.
    state
        .apply_frame(RunStreamFrameDto::TextDelta(fixture_delta(
            session_id, run_id, 1, "next",
        )))
        .expect("a delta of the next step applies");
    assert_eq!(state.provisional_text(), "next");

    // A fresh correlated snapshot resets provisional text with committed state.
    let next_committed = message(
        session_id,
        Some(run_id),
        MessageKindDto::Assistant,
        "committed by the snapshot",
    );
    state
        .apply_initial(fixture_snapshot(
            session_id,
            run_id,
            vec![next_committed.clone()],
        ))
        .expect("a fresh snapshot applies");
    assert_eq!(state.provisional_text(), "");
    assert_eq!(state.messages(), &[next_committed]);
}

#[test]
fn state_clears_provisional_text_on_the_committed_assistant_row() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = RunStreamState::new(session_id, run_id);
    state
        .apply_initial(fixture_snapshot(session_id, run_id, Vec::new()))
        .expect("fixture snapshot applies");
    state
        .apply_frame(RunStreamFrameDto::TextDelta(fixture_delta(
            session_id, run_id, 0, "half",
        )))
        .expect("provisional delta applies");
    assert_eq!(state.provisional_text(), "half");

    state
        .apply_frame(RunStreamFrameDto::Content(message(
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
        .apply_frame(RunStreamFrameDto::Content(message(
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
fn state_buffers_the_reasoning_channel_apart_from_the_answer() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = RunStreamState::new(session_id, run_id);
    state
        .apply_initial(fixture_snapshot(session_id, run_id, Vec::new()))
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
            .apply_frame(RunStreamFrameDto::TextDelta(delta))
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
        .apply_frame(RunStreamFrameDto::TextDelta(fixture_reasoning_delta(
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
        .apply_frame(RunStreamFrameDto::Content(message(
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
    let mut state = RunStreamState::new(session_id, run_id);
    state
        .apply_initial(fixture_snapshot(session_id, run_id, Vec::new()))
        .expect("fixture snapshot applies");
    state
        .apply_frame(RunStreamFrameDto::TextDelta(fixture_delta(
            session_id, run_id, 0, "half",
        )))
        .expect("provisional delta applies");
    state
        .apply_frame(RunStreamFrameDto::Status(fixture_run(
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
        let mut state = RunStreamState::new(session_id, run_id);
        state
            .apply_initial(fixture_snapshot(session_id, run_id, Vec::new()))
            .expect("fixture snapshot applies");
        state
            .apply_frame(RunStreamFrameDto::TextDelta(fixture_delta(
                session_id, run_id, 0, "half",
            )))
            .expect("provisional delta applies");
        assert_eq!(state.provisional_text(), "half");

        state
            .apply_frame(RunStreamFrameDto::Status(fixture_run(
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
