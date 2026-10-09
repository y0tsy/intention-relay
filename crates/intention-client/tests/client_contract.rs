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
    AcceptProviderCatalogRemovalCommandDto, CatalogRevisionId, ClientRequestDto, ConfigRevisionId,
    ConfigurationEditAcceptedDto, ConfigurationEditDto, ConfigurationReloadAcceptedDto,
    ContextPreservationCapabilityDto, CreateSessionCommandDto, CredentialRotationAcceptedDto,
    CredentialTransportDto, DaemonHealthDto, DaemonReadinessDto, IdempotencyKey,
    InterruptRunAcceptedDto, ListProviderCatalogQueryDto, ModelCapabilitySetV1,
    ModelCapabilityTaxonomyVersionDto, ModelInputKindDto, ProtocolDaemonMessageDto,
    ProtocolResultDto, ProviderCapabilityAvailabilityDto, ProviderCatalogActivationStateDto,
    ProviderCatalogCandidateHandleDto, ProviderCatalogCandidateRejectedDto, ProviderCatalogPageDto,
    ProviderCatalogRemovalAcceptedDto, ProviderCatalogStatusDto, ProviderDiscoveryAttemptId,
    ProviderDiscoveryResultDto, ProviderDriverCapabilitiesDto, ProviderExecutionPolicyDto,
    ProviderHealthEvidenceDto, ProviderHealthStateDto, ProviderKindDescriptorRevisionId,
    ProviderKindId, ProviderModelRecordDto, ProviderProfileEntryDto, ProviderProfileId,
    ProviderProfileOverrideDto, ProviderProfileReadinessDto, ProviderProfileRevisionId,
    ReasoningCapabilityDto, ReasoningEffortLevelDto, ReasoningHistoryTransferDto,
    RejectProviderCatalogCandidateCommandDto, RemoveTurnAcceptedDto, RunId,
    SendUserTurnAcceptedDto, SendUserTurnCommandDto, SendUserTurnOutcomeDto, SessionId,
    SessionProviderProfileProjectionDto, SessionSnapshotDto, SetSessionProviderProfileAcceptedDto,
    SetSessionProviderProfileCommandDto, ToolExchangeCapabilityDto, TurnId, decode_request_line,
    encode_request,
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
    /// A valid current-wire reply sent only when the decoded request matches.
    CheckedRequest {
        expected: ClientRequestDto,
        result: ProtocolResultDto,
    },
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
    if !expected_request_matches(&response, request.request()) {
        messages
            .send_message(&fixture_reply(
                request.id(),
                &FixtureResponse::Rejected(ErrorDto::validation(
                    "fixture_request_mismatch",
                    "the fixture request does not carry the expected command",
                )),
            ))
            .await
            .expect("fixture mismatch rejection sends");
        return;
    }
    messages
        .send_message(&fixture_reply(request.id(), &response))
        .await
        .expect("fixture reply sends");
}

/// Reports whether one decoded request is the command the fixture checks.
///
/// A fixture that checks no request accepts every decoded request.
fn expected_request_matches(response: &FixtureResponse, request: &ClientRequestDto) -> bool {
    match response {
        FixtureResponse::CheckedRequest { expected, .. } => request == expected,
        _ => true,
    }
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
        FixtureResponse::CheckedRequest { result, .. } => {
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

fn fixture_profile_id(value: &str) -> ProviderProfileId {
    ProviderProfileId::parse(value).expect("fixture profile identity is valid")
}

fn fixture_kind_id() -> ProviderKindId {
    ProviderKindId::parse("openrouter").expect("fixture kind identity is valid")
}

fn fixture_execution_policy() -> ProviderExecutionPolicyDto {
    ProviderExecutionPolicyDto::new(30, 2).expect("fixture execution policy is valid")
}

fn fixture_capability_subset() -> ModelCapabilitySetV1 {
    ModelCapabilitySetV1::new(
        ModelCapabilityTaxonomyVersionDto::current(),
        ModelInputKindDto::TextOnly,
        ProviderCapabilityAvailabilityDto::Enabled,
        ProviderCapabilityAvailabilityDto::Disabled,
        ReasoningCapabilityDto::textual_reasoning_v1(vec![ReasoningEffortLevelDto::Medium], true)
            .expect("fixture reasoning capability is valid"),
        ToolExchangeCapabilityDto::model_tool_loop_v1("fixture-tool-loop-v1")
            .expect("fixture tool loop is valid"),
        ContextPreservationCapabilityDto::local_durable_history_v1(
            ReasoningHistoryTransferDto::textual_history_v1("fixture-compatibility-v1")
                .expect("fixture transfer contract is valid"),
        ),
    )
    .expect("fixture capability subset is valid")
}

fn fixture_entry(profile_id: ProviderProfileId) -> ProviderProfileEntryDto {
    ProviderProfileEntryDto::new(
        profile_id,
        "Main",
        true,
        fixture_kind_id(),
        ProviderKindDescriptorRevisionId::new(),
        "fixture-model",
        Some("https://provider.example/v1".to_owned()),
        fixture_execution_policy(),
        fixture_capability_subset(),
        CredentialTransportDto::bearer(),
        true,
        ProviderDriverCapabilitiesDto::new(true, true, true),
        ProviderProfileReadinessDto::Ready,
        None,
    )
    .expect("fixture provider entry is valid")
}

fn fixture_catalog_page() -> ProviderCatalogPageDto {
    ProviderCatalogPageDto::new(
        Some(CatalogRevisionId::new()),
        Some(fixture_profile_id("main")),
        vec![fixture_entry(fixture_profile_id("main"))],
        None,
        false,
    )
    .expect("fixture catalog page is valid")
}

fn fixture_catalog_status() -> ProviderCatalogStatusDto {
    ProviderCatalogStatusDto::new(
        ProviderCatalogActivationStateDto::Active,
        None,
        Some(CatalogRevisionId::new()),
        None,
        Some(fixture_profile_id("main")),
        Vec::new(),
    )
    .expect("fixture catalog status is valid")
}

fn fixture_candidate_handle() -> ProviderCatalogCandidateHandleDto {
    ProviderCatalogCandidateHandleDto::new(CatalogRevisionId::new(), CatalogRevisionId::new())
        .expect("fixture candidate handle is valid")
}

fn fixture_session_profile(session_id: SessionId) -> SessionProviderProfileProjectionDto {
    SessionProviderProfileProjectionDto::new(
        session_id,
        Some(fixture_profile_id("main")),
        Some(fixture_entry(fixture_profile_id("main"))),
        None,
        1,
        Some(fixture_profile_id("main")),
    )
    .expect("fixture session provider projection is valid")
}

fn fixture_discovery_result() -> ProviderDiscoveryResultDto {
    ProviderDiscoveryResultDto::new(
        ProviderDiscoveryAttemptId::new(),
        vec![
            ProviderModelRecordDto::new("fixture-model", None)
                .expect("fixture model record is valid"),
        ],
    )
    .expect("fixture discovery result is valid")
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
async fn list_provider_catalog_validates_success_and_rejection() {
    let _guard = fixture_guard();
    let page = fixture_catalog_page();

    let page_endpoint = endpoint();
    let response = FixtureResponse::Result(ProtocolResultDto::ProviderCatalogPage(page.clone()));
    let server = start_fixture_server(page_endpoint.clone(), response.clone());
    let received = client(page_endpoint, response, Arc::new(AtomicUsize::new(0)))
        .list_provider_catalog(
            ListProviderCatalogQueryDto::new(None).expect("fixture catalog query is valid"),
        )
        .await
        .expect("a typed catalog page is returned");
    assert_eq!(received, page);
    assert_eq!(received.entries().len(), 1);
    server.await.expect("catalog fixture server completes");

    // A reply of another operation is not this request's answer.
    let foreign_endpoint = endpoint();
    let response =
        FixtureResponse::Result(ProtocolResultDto::DaemonHealth(DaemonHealthDto::ready()));
    let server = start_fixture_server(foreign_endpoint.clone(), response.clone());
    assert_eq!(
        client(foreign_endpoint, response, Arc::new(AtomicUsize::new(0)))
            .list_provider_catalog(
                ListProviderCatalogQueryDto::new(None).expect("fixture catalog query is valid"),
            )
            .await
            .expect_err("a reply of another operation is not this request's answer")
            .code(),
        "invalid_local_protocol_response"
    );
    server.await.expect("foreign fixture server completes");
}

#[tokio::test]
async fn provider_catalog_status_validates_success_and_rejection() {
    let _guard = fixture_guard();
    let status = fixture_catalog_status();

    let status_endpoint = endpoint();
    let response =
        FixtureResponse::Result(ProtocolResultDto::ProviderCatalogStatus(status.clone()));
    let server = start_fixture_server(status_endpoint.clone(), response.clone());
    let received = client(status_endpoint, response, Arc::new(AtomicUsize::new(0)))
        .provider_catalog_status()
        .await
        .expect("a typed catalog status is returned");
    assert_eq!(received, status);
    assert_eq!(
        received.activation_state(),
        ProviderCatalogActivationStateDto::Active
    );
    server
        .await
        .expect("catalog status fixture server completes");

    // A reply of another operation is not this request's answer.
    let foreign_endpoint = endpoint();
    let response =
        FixtureResponse::Result(ProtocolResultDto::DaemonHealth(DaemonHealthDto::ready()));
    let server = start_fixture_server(foreign_endpoint.clone(), response.clone());
    assert_eq!(
        client(foreign_endpoint, response, Arc::new(AtomicUsize::new(0)))
            .provider_catalog_status()
            .await
            .expect_err("a reply of another operation is not this request's answer")
            .code(),
        "invalid_local_protocol_response"
    );
    server.await.expect("foreign fixture server completes");
}

#[tokio::test]
async fn set_session_provider_profile_validates_success_and_foreign_scope_rejection() {
    let _guard = fixture_guard();
    let session_id = SessionId::new();
    let profile_id = fixture_profile_id("main");
    let command = SetSessionProviderProfileCommandDto::new(
        session_id,
        profile_id.clone(),
        2,
        IdempotencyKey::new(),
    );
    let accepted = SetSessionProviderProfileAcceptedDto::new(session_id, true, 3);

    let accepted_endpoint = endpoint();
    let response = FixtureResponse::Result(ProtocolResultDto::SessionProviderProfileSet(accepted));
    let server = start_fixture_server(accepted_endpoint.clone(), response.clone());
    let received = client(accepted_endpoint, response, Arc::new(AtomicUsize::new(0)))
        .set_session_provider_profile(command)
        .await
        .expect("typed session default acceptance is returned");
    assert!(received.changed());
    assert_eq!(received.session_projection_revision(), 3);
    server
        .await
        .expect("session default fixture server completes");

    // An acceptance for another session is not this request's answer.
    let foreign_endpoint = endpoint();
    let response = FixtureResponse::Result(ProtocolResultDto::SessionProviderProfileSet(
        SetSessionProviderProfileAcceptedDto::new(SessionId::new(), true, 3),
    ));
    let server = start_fixture_server(foreign_endpoint.clone(), response.clone());
    assert_eq!(
        client(foreign_endpoint, response, Arc::new(AtomicUsize::new(0)))
            .set_session_provider_profile(SetSessionProviderProfileCommandDto::new(
                session_id,
                profile_id,
                2,
                IdempotencyKey::new(),
            ))
            .await
            .expect_err("an acceptance for another session is not this request's answer")
            .code(),
        "invalid_local_protocol_response"
    );
    server
        .await
        .expect("foreign scope fixture server completes");
}

#[tokio::test]
async fn session_provider_profile_validates_success_and_foreign_scope_rejection() {
    let _guard = fixture_guard();
    let session_id = SessionId::new();
    let projection = fixture_session_profile(session_id);

    let projection_endpoint = endpoint();
    let response = FixtureResponse::Result(ProtocolResultDto::SessionProviderProfile(
        projection.clone(),
    ));
    let server = start_fixture_server(projection_endpoint.clone(), response.clone());
    let received = client(projection_endpoint, response, Arc::new(AtomicUsize::new(0)))
        .session_provider_profile(session_id)
        .await
        .expect("the requested session provider projection is returned");
    assert_eq!(received, projection);
    assert_eq!(
        received.durable_profile_id(),
        Some(&fixture_profile_id("main"))
    );
    server
        .await
        .expect("session provider fixture server completes");

    // A projection for another session is not this request's answer.
    let foreign_endpoint = endpoint();
    let response = FixtureResponse::Result(ProtocolResultDto::SessionProviderProfile(
        fixture_session_profile(SessionId::new()),
    ));
    let server = start_fixture_server(foreign_endpoint.clone(), response.clone());
    assert_eq!(
        client(foreign_endpoint, response, Arc::new(AtomicUsize::new(0)))
            .session_provider_profile(session_id)
            .await
            .expect_err("a projection for another session is not this request's answer")
            .code(),
        "invalid_local_protocol_response"
    );
    server
        .await
        .expect("foreign scope fixture server completes");
}

#[tokio::test]
async fn reload_configuration_validates_success_and_rejection() {
    let _guard = fixture_guard();
    let accepted = ConfigurationReloadAcceptedDto::new(
        ConfigRevisionId::new(),
        Some(CatalogRevisionId::new()),
    );

    let accepted_endpoint = endpoint();
    let response = FixtureResponse::Result(ProtocolResultDto::ConfigurationReloaded(accepted));
    let server = start_fixture_server(accepted_endpoint.clone(), response.clone());
    let received = client(accepted_endpoint, response, Arc::new(AtomicUsize::new(0)))
        .reload_configuration(IdempotencyKey::new())
        .await
        .expect("typed configuration reload acceptance is returned");
    assert_eq!(received.config_revision_id(), accepted.config_revision_id());
    assert_eq!(
        received.catalog_revision_id(),
        accepted.catalog_revision_id()
    );
    server.await.expect("reload fixture server completes");

    // A reply of another operation is not this request's answer.
    let foreign_endpoint = endpoint();
    let response =
        FixtureResponse::Result(ProtocolResultDto::DaemonHealth(DaemonHealthDto::ready()));
    let server = start_fixture_server(foreign_endpoint.clone(), response.clone());
    assert_eq!(
        client(foreign_endpoint, response, Arc::new(AtomicUsize::new(0)))
            .reload_configuration(IdempotencyKey::new())
            .await
            .expect_err("a reply of another operation is not this request's answer")
            .code(),
        "invalid_local_protocol_response"
    );
    server.await.expect("foreign fixture server completes");
}

#[tokio::test]
async fn rotate_provider_credential_validates_success_and_foreign_scope_rejection() {
    let _guard = fixture_guard();
    let profile_id = fixture_profile_id("main");
    let accepted = CredentialRotationAcceptedDto::new(profile_id.clone());

    let accepted_endpoint = endpoint();
    let response = FixtureResponse::Result(ProtocolResultDto::ProviderCredentialRotated(
        accepted.clone(),
    ));
    let server = start_fixture_server(accepted_endpoint.clone(), response.clone());
    let received = client(accepted_endpoint, response, Arc::new(AtomicUsize::new(0)))
        .rotate_provider_credential(profile_id.clone(), IdempotencyKey::new())
        .await
        .expect("typed credential rotation acceptance is returned");
    assert_eq!(received.profile_id(), &profile_id);
    server.await.expect("rotation fixture server completes");

    // An acceptance for another profile is not this request's answer.
    let foreign_endpoint = endpoint();
    let response = FixtureResponse::Result(ProtocolResultDto::ProviderCredentialRotated(
        CredentialRotationAcceptedDto::new(fixture_profile_id("secondary")),
    ));
    let server = start_fixture_server(foreign_endpoint.clone(), response.clone());
    assert_eq!(
        client(foreign_endpoint, response, Arc::new(AtomicUsize::new(0)))
            .rotate_provider_credential(profile_id, IdempotencyKey::new())
            .await
            .expect_err("an acceptance for another profile is not this request's answer")
            .code(),
        "invalid_local_protocol_response"
    );
    server
        .await
        .expect("foreign scope fixture server completes");
}

#[tokio::test]
async fn check_provider_health_validates_success_and_foreign_scope_rejection() {
    let _guard = fixture_guard();
    let profile_id = fixture_profile_id("main");
    let evidence =
        ProviderHealthEvidenceDto::new(profile_id.clone(), ProviderHealthStateDto::Available, None)
            .expect("fixture health evidence is valid");

    let evidence_endpoint = endpoint();
    let response = FixtureResponse::Result(ProtocolResultDto::ProviderHealth(evidence.clone()));
    let server = start_fixture_server(evidence_endpoint.clone(), response.clone());
    let received = client(evidence_endpoint, response, Arc::new(AtomicUsize::new(0)))
        .check_provider_health(profile_id.clone())
        .await
        .expect("typed health evidence is returned");
    assert_eq!(received, evidence);
    assert_eq!(received.state(), ProviderHealthStateDto::Available);
    server.await.expect("health fixture server completes");

    // Evidence for another provider is not this request's answer.
    let foreign_endpoint = endpoint();
    let response = FixtureResponse::Result(ProtocolResultDto::ProviderHealth(
        ProviderHealthEvidenceDto::new(
            fixture_profile_id("secondary"),
            ProviderHealthStateDto::Available,
            None,
        )
        .expect("fixture health evidence is valid"),
    ));
    let server = start_fixture_server(foreign_endpoint.clone(), response.clone());
    assert_eq!(
        client(foreign_endpoint, response, Arc::new(AtomicUsize::new(0)))
            .check_provider_health(profile_id)
            .await
            .expect_err("evidence for another provider is not this request's answer")
            .code(),
        "invalid_local_protocol_response"
    );
    server
        .await
        .expect("foreign scope fixture server completes");
}

#[tokio::test]
async fn discover_provider_models_validates_success_and_rejection() {
    let _guard = fixture_guard();
    let result = fixture_discovery_result();

    let result_endpoint = endpoint();
    let response =
        FixtureResponse::Result(ProtocolResultDto::ProviderModelsDiscovered(result.clone()));
    let server = start_fixture_server(result_endpoint.clone(), response.clone());
    let received = client(result_endpoint, response, Arc::new(AtomicUsize::new(0)))
        .discover_provider_models(fixture_profile_id("main"))
        .await
        .expect("typed discovery result is returned");
    assert_eq!(received, result);
    assert_eq!(received.records().len(), 1);
    server.await.expect("discovery fixture server completes");

    // A reply of another operation is not this request's answer.
    let foreign_endpoint = endpoint();
    let response =
        FixtureResponse::Result(ProtocolResultDto::DaemonHealth(DaemonHealthDto::ready()));
    let server = start_fixture_server(foreign_endpoint.clone(), response.clone());
    assert_eq!(
        client(foreign_endpoint, response, Arc::new(AtomicUsize::new(0)))
            .discover_provider_models(fixture_profile_id("main"))
            .await
            .expect_err("a reply of another operation is not this request's answer")
            .code(),
        "invalid_local_protocol_response"
    );
    server.await.expect("foreign fixture server completes");
}

#[tokio::test]
async fn apply_configuration_document_validates_success_and_rejection() {
    let _guard = fixture_guard();
    let accepted = ConfigurationEditAcceptedDto::new(ConfigRevisionId::new(), false);

    let accepted_endpoint = endpoint();
    let response =
        FixtureResponse::Result(ProtocolResultDto::ConfigurationDocumentApplied(accepted));
    let server = start_fixture_server(accepted_endpoint.clone(), response.clone());
    let received = client(accepted_endpoint, response, Arc::new(AtomicUsize::new(0)))
        .apply_configuration_document("schema_version = 1\n".to_owned(), IdempotencyKey::new())
        .await
        .expect("typed applied-document acceptance is returned");
    assert_eq!(received.config_revision_id(), accepted.config_revision_id());
    server
        .await
        .expect("applied document fixture server completes");

    assert_eq!(
        client(
            endpoint(),
            FixtureResponse::Result(ProtocolResultDto::DaemonHealth(DaemonHealthDto::ready())),
            Arc::new(AtomicUsize::new(0)),
        )
        .apply_configuration_document(" ".to_owned(), IdempotencyKey::new())
        .await
        .expect_err("a blank candidate document fails before any request")
        .code(),
        "invalid_configuration_edit"
    );

    // A reply of another operation is not this request's answer.
    let foreign_endpoint = endpoint();
    let response =
        FixtureResponse::Result(ProtocolResultDto::DaemonHealth(DaemonHealthDto::ready()));
    let server = start_fixture_server(foreign_endpoint.clone(), response.clone());
    assert_eq!(
        client(foreign_endpoint, response, Arc::new(AtomicUsize::new(0)))
            .apply_configuration_document("schema_version = 1\n".to_owned(), IdempotencyKey::new())
            .await
            .expect_err("a reply of another operation is not this request's answer")
            .code(),
        "invalid_local_protocol_response"
    );
    server.await.expect("foreign fixture server completes");
}

#[tokio::test]
async fn apply_configuration_edits_validates_success_and_rejection() {
    let _guard = fixture_guard();
    let accepted = ConfigurationEditAcceptedDto::new(ConfigRevisionId::new(), true);

    let accepted_endpoint = endpoint();
    let response = FixtureResponse::Result(ProtocolResultDto::ConfigurationEditsApplied(accepted));
    let server = start_fixture_server(accepted_endpoint.clone(), response.clone());
    let received = client(accepted_endpoint, response, Arc::new(AtomicUsize::new(0)))
        .apply_configuration_edits(
            vec![ConfigurationEditDto::set_profile_enabled(
                fixture_profile_id("main"),
                true,
            )],
            IdempotencyKey::new(),
        )
        .await
        .expect("typed applied-edits acceptance is returned");
    assert_eq!(received.config_revision_id(), accepted.config_revision_id());
    server
        .await
        .expect("applied edits fixture server completes");

    // A reply of another operation is not this request's answer.
    let foreign_endpoint = endpoint();
    let response =
        FixtureResponse::Result(ProtocolResultDto::DaemonHealth(DaemonHealthDto::ready()));
    let server = start_fixture_server(foreign_endpoint.clone(), response.clone());
    assert_eq!(
        client(foreign_endpoint, response, Arc::new(AtomicUsize::new(0)))
            .apply_configuration_edits(
                vec![ConfigurationEditDto::set_profile_enabled(
                    fixture_profile_id("main"),
                    true,
                )],
                IdempotencyKey::new(),
            )
            .await
            .expect_err("a reply of another operation is not this request's answer")
            .code(),
        "invalid_local_protocol_response"
    );
    server.await.expect("foreign fixture server completes");
}

#[tokio::test]
async fn accept_provider_catalog_removal_validates_success_and_foreign_scope_rejection() {
    let _guard = fixture_guard();
    let handle = fixture_candidate_handle();
    let command = AcceptProviderCatalogRemovalCommandDto::new(handle, IdempotencyKey::new());
    let accepted = ProviderCatalogRemovalAcceptedDto::new(handle.candidate_revision_id());

    let accepted_endpoint = endpoint();
    let response =
        FixtureResponse::Result(ProtocolResultDto::ProviderCatalogRemovalAccepted(accepted));
    let server = start_fixture_server(accepted_endpoint.clone(), response.clone());
    let received = client(accepted_endpoint, response, Arc::new(AtomicUsize::new(0)))
        .accept_provider_catalog_removal(command)
        .await
        .expect("typed removal acceptance is returned");
    assert_eq!(
        received.catalog_revision_id(),
        accepted.catalog_revision_id()
    );
    server.await.expect("removal fixture server completes");

    // An acceptance of another catalog revision is not this request's answer.
    let foreign_endpoint = endpoint();
    let response = FixtureResponse::Result(ProtocolResultDto::ProviderCatalogRemovalAccepted(
        ProviderCatalogRemovalAcceptedDto::new(CatalogRevisionId::new()),
    ));
    let server = start_fixture_server(foreign_endpoint.clone(), response.clone());
    assert_eq!(
        client(foreign_endpoint, response, Arc::new(AtomicUsize::new(0)))
            .accept_provider_catalog_removal(AcceptProviderCatalogRemovalCommandDto::new(
                handle,
                IdempotencyKey::new(),
            ))
            .await
            .expect_err("an acceptance of another revision is not this request's answer")
            .code(),
        "invalid_local_protocol_response"
    );
    server
        .await
        .expect("foreign scope fixture server completes");
}

#[tokio::test]
async fn reject_provider_catalog_candidate_validates_success_and_foreign_scope_rejection() {
    let _guard = fixture_guard();
    let handle = fixture_candidate_handle();
    let command = RejectProviderCatalogCandidateCommandDto::new(handle, IdempotencyKey::new());
    let rejected =
        ProviderCatalogCandidateRejectedDto::new(Some(handle.expected_active_revision_id()));

    let rejected_endpoint = endpoint();
    let response = FixtureResponse::Result(ProtocolResultDto::ProviderCatalogCandidateRejected(
        rejected,
    ));
    let server = start_fixture_server(rejected_endpoint.clone(), response.clone());
    let received = client(rejected_endpoint, response, Arc::new(AtomicUsize::new(0)))
        .reject_provider_catalog_candidate(command)
        .await
        .expect("typed candidate rejection evidence is returned");
    assert_eq!(
        received.active_catalog_revision_id(),
        rejected.active_catalog_revision_id()
    );
    server.await.expect("rejection fixture server completes");

    // Evidence for another active catalog revision is not this request's answer.
    let foreign_endpoint = endpoint();
    let response = FixtureResponse::Result(ProtocolResultDto::ProviderCatalogCandidateRejected(
        ProviderCatalogCandidateRejectedDto::new(Some(CatalogRevisionId::new())),
    ));
    let server = start_fixture_server(foreign_endpoint.clone(), response.clone());
    assert_eq!(
        client(foreign_endpoint, response, Arc::new(AtomicUsize::new(0)))
            .reject_provider_catalog_candidate(RejectProviderCatalogCandidateCommandDto::new(
                handle,
                IdempotencyKey::new(),
            ))
            .await
            .expect_err("evidence for another revision is not this request's answer")
            .code(),
        "invalid_local_protocol_response"
    );
    server
        .await
        .expect("foreign scope fixture server completes");
}

#[tokio::test]
async fn send_user_turn_with_profile_validates_success_and_foreign_scope_rejection() {
    let _guard = fixture_guard();
    let session_id = SessionId::new();
    let profile_id = fixture_profile_id("main");
    let provider_profile =
        ProviderProfileOverrideDto::new(profile_id.clone(), Some(ProviderProfileRevisionId::new()));
    let idempotency_key = IdempotencyKey::new();
    let content = "fixture turn".to_owned();
    let expected_command =
        SendUserTurnCommandDto::new(session_id, idempotency_key, content.clone())
            .expect("fixture turn command is valid")
            .with_provider_profile(Some(provider_profile.clone()));
    let outcome = SendUserTurnOutcomeDto::Started {
        run_id: RunId::new(),
        config_revision_id: ConfigRevisionId::new(),
    };
    let accepted = SendUserTurnAcceptedDto::new(session_id, TurnId::new(), outcome);

    let accepted_endpoint = endpoint();
    let response = FixtureResponse::CheckedRequest {
        expected: ClientRequestDto::SendUserTurn(expected_command.clone()),
        result: ProtocolResultDto::TurnAccepted(accepted),
    };
    let server = start_fixture_server(accepted_endpoint.clone(), response.clone());
    let received = client(accepted_endpoint, response, Arc::new(AtomicUsize::new(0)))
        .send_user_turn_with_profile(
            session_id,
            idempotency_key,
            content.clone(),
            provider_profile.clone(),
        )
        .await
        .expect("a checked turn command is accepted");
    assert_eq!(received, outcome);
    server
        .await
        .expect("turn override fixture server completes");

    // An accepted turn of another session is not this request's answer.
    let foreign_endpoint = endpoint();
    let response = FixtureResponse::Result(ProtocolResultDto::TurnAccepted(
        SendUserTurnAcceptedDto::new(
            SessionId::new(),
            TurnId::new(),
            SendUserTurnOutcomeDto::Pending,
        ),
    ));
    let server = start_fixture_server(foreign_endpoint.clone(), response.clone());
    assert_eq!(
        client(foreign_endpoint, response, Arc::new(AtomicUsize::new(0)))
            .send_user_turn_with_profile(
                session_id,
                IdempotencyKey::new(),
                content,
                provider_profile,
            )
            .await
            .expect_err("an accepted turn of another session is not this request's answer")
            .code(),
        "invalid_local_protocol_response"
    );
    server
        .await
        .expect("foreign scope fixture server completes");
}
