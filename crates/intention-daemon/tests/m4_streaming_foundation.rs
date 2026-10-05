#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Focused daemon-foundation fixtures use assertion conveniences for precise diagnostics."
)]

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[cfg(feature = "test-support")]
use futures_util::StreamExt;
use futures_util::stream;
use intention::DaemonApplicationFacade;
use intention_application::ScheduleModelRunDto;
#[cfg(feature = "test-support")]
use intention_client::RunStreamClient;
use intention_config::ConfigSnapshotDto;
use intention_daemon::DaemonToolExecutor;
#[cfg(feature = "test-support")]
use intention_domain::RunStatusDto;
use intention_domain::SendUserTurnCommandDto;
use intention_model::{
    FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto, ModelDriver, ModelEventDto,
    ModelEventStream, ModelExecutionDriver, ModelMessageDto, ModelRequestDto, ModelRoleDto,
};
use intention_protocol::{
    ProtocolAcceptedResultDto, ProtocolCommandDto, ProtocolCommandResultDto, SendUserTurnOutcomeDto,
};
#[cfg(feature = "test-support")]
use intention_protocol::{
    ProtocolHelloDto, ProtocolMethodDto, ProtocolQueryDto, ProtocolQueryResultDto,
    ProtocolRequestPayloadDto, ProtocolResponsePayloadDto, RunSubscriptionResponseDto,
    SubscribeRunCommandDto, decode_response, encode_request,
};
use intention_runtime::{
    ModelRunCommitDto, ModelRunCommitObserver, ModelSleepFuture, ModelTimePort,
};
#[cfg(feature = "test-support")]
use intention_transport::{AsyncLocalListener, LocalEndpoint};
use intention_types::{RunId, SessionId, TimestampDto, TurnId};
use tempfile::TempDir;

struct ScriptedDriver {
    events: Mutex<Vec<ModelEventDto>>,
    executions: Mutex<usize>,
}

#[cfg(feature = "test-support")]
struct BlockingDriver {
    executions: Mutex<usize>,
    requests: Mutex<Vec<ModelRequestDto>>,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

#[cfg(feature = "test-support")]
impl BlockingDriver {
    fn new() -> Self {
        Self {
            executions: Mutex::new(0),
            requests: Mutex::new(Vec::new()),
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        }
    }

    fn executions(&self) -> usize {
        *self
            .executions
            .lock()
            .expect("driver recorder remains available")
    }

    fn requests(&self) -> Vec<ModelRequestDto> {
        self.requests
            .lock()
            .expect("driver request recorder remains available")
            .clone()
    }
}

#[cfg(feature = "test-support")]
impl ModelDriver for BlockingDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    }
}

#[cfg(feature = "test-support")]
impl ModelExecutionDriver for BlockingDriver {
    fn execute(
        &self,
        request: ModelRequestDto,
        _cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        *self
            .executions
            .lock()
            .expect("driver recorder remains available") += 1;
        self.requests
            .lock()
            .expect("driver request recorder remains available")
            .push(request);
        self.entered.notify_one();
        let release = Arc::clone(&self.release);
        Box::pin(
            stream::once(async move {
                release.notified().await;
                Ok(ModelEventDto::started())
            })
            .chain(stream::iter(vec![
                Ok(ModelEventDto::text_delta("live response").expect("fixture text is valid")),
                Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
            ])),
        )
    }
}

impl ScriptedDriver {
    fn completed_text() -> Self {
        Self {
            events: Mutex::new(vec![
                ModelEventDto::started(),
                ModelEventDto::text_delta("complete response").expect("fixture text is valid"),
                ModelEventDto::finished(FinishReasonDto::Stop),
            ]),
            executions: Mutex::new(0),
        }
    }

    fn executions(&self) -> usize {
        *self
            .executions
            .lock()
            .expect("driver recorder remains available")
    }
}

impl ModelDriver for ScriptedDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    }
}

impl ModelExecutionDriver for ScriptedDriver {
    fn execute(
        &self,
        _request: ModelRequestDto,
        _cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        *self
            .executions
            .lock()
            .expect("driver recorder remains available") += 1;
        let events = std::mem::take(&mut *self.events.lock().expect("script remains available"));
        Box::pin(stream::iter(events.into_iter().map(Ok)))
    }
}

struct TokioTime;

impl ModelTimePort for TokioTime {
    fn now(&self) -> TimestampDto {
        TimestampDto::from_unix_seconds(2).expect("fixture timestamp is valid")
    }

    fn sleep(&self, duration: Duration) -> ModelSleepFuture<'_> {
        Box::pin(tokio::time::sleep(duration))
    }
}

#[derive(Default)]
struct RecordingObserver {
    commits: Mutex<Vec<ModelRunCommitDto>>,
}

impl RecordingObserver {
    fn commits(&self) -> Vec<ModelRunCommitDto> {
        self.commits
            .lock()
            .expect("observer recorder remains available")
            .clone()
    }
}

impl ModelRunCommitObserver for RecordingObserver {
    fn observe_model_run_commit(&self, committed: ModelRunCommitDto) {
        self.commits
            .lock()
            .expect("observer recorder remains available")
            .push(committed);
    }
}

fn fixture_facade(
    driver: Arc<dyn ModelExecutionDriver + Send + Sync>,
) -> (TempDir, DaemonApplicationFacade, ConfigSnapshotDto) {
    let directory = TempDir::new().expect("temporary directory exists");
    let snapshot = intention_test_snapshot();
    let facade = DaemonApplicationFacade::open_for_test_support_with_driver(
        directory.path().join("foundation.sqlite"),
        snapshot.clone(),
        driver,
    )
    .expect("fixture facade opens");
    (directory, facade, snapshot)
}

fn intention_test_snapshot() -> ConfigSnapshotDto {
    intention_test_snapshot_with_credential("fixture-credential")
}

fn intention_test_snapshot_with_credential(credential: &str) -> ConfigSnapshotDto {
    let source = intention_config::ConfigSourceDto::Explicit(
        intention_config::ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-daemon-foundation.toml")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture source is absolute"),
    );
    let resolved = intention_config::ResolvedConfigDto::parse_resolve(
        intention_config::RawConfigInputDto::new(
            format!(
                "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"{credential}\""
            ),
            source,
        ),
    )
    .expect("fixture configuration resolves");
    ConfigSnapshotDto::new(
        intention_types::SchemaVersionDto::new(1, 0),
        intention_types::ConfigRevisionId::new(),
        TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid"),
        resolved,
    )
    .expect("fixture snapshot is valid")
}

fn schedule(
    session_id: SessionId,
    run_id: RunId,
    snapshot: ConfigSnapshotDto,
) -> ScheduleModelRunDto {
    ScheduleModelRunDto::new(
        session_id,
        run_id,
        ModelRequestDto::new(
            run_id,
            "fixture",
            vec![ModelMessageDto::new(ModelRoleDto::User, "turn").expect("message is valid")],
            None,
            None,
        )
        .expect("request is valid"),
        snapshot,
    )
    .expect("schedule is valid")
}

fn create_and_start(facade: &DaemonApplicationFacade) -> (SessionId, RunId) {
    let session_id = SessionId::new();
    let create = ProtocolCommandDto::CreateSession(intention_domain::CreateSessionCommandDto::new(
        intention_types::ProjectId::new(),
        session_id,
        intention_types::WorkspaceId::new(),
        intention_domain::WorkspaceRootDto::parse(
            std::env::temp_dir().to_string_lossy().into_owned(),
        )
        .expect("fixture workspace is absolute"),
        intention_domain::RunModeDto::Build,
    ));
    assert!(matches!(
        facade.command(create),
        ProtocolCommandResultDto::Accepted(_)
    ));
    let result = facade.command(ProtocolCommandDto::SendUserTurn(
        SendUserTurnCommandDto::new(session_id, TurnId::new(), "turn").expect("turn is valid"),
    ));
    let ProtocolCommandResultDto::Accepted(accepted) = result else {
        panic!("fixture turn starts")
    };
    let ProtocolAcceptedResultDto::SendUserTurn(turn) = accepted.result() else {
        panic!("fixture result is a turn")
    };
    let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
        panic!("first turn starts")
    };
    (session_id, run_id)
}

#[cfg(feature = "test-support")]
async fn send_request_through_host(
    endpoint: &LocalEndpoint,
    adapter_name: &str,
    request_id: u64,
    payload: ProtocolRequestPayloadDto,
) -> ProtocolResponsePayloadDto {
    use intention_transport::{AsyncLocalClientConnection, local_protocol_version};

    let connection = AsyncLocalClientConnection::connect(endpoint)
        .await
        .expect("host client connects");
    let (_remote, mut requests, mut messages) = connection
        .negotiate(
            ProtocolHelloDto::new(local_protocol_version(), adapter_name)
                .expect("host hello is valid"),
        )
        .await
        .expect("host client negotiates");
    let method = ProtocolMethodDto::for_payload(&payload);
    requests
        .send_message(&encode_request(request_id, payload))
        .await
        .expect("host request sends");
    let line = messages
        .receive_line()
        .await
        .expect("host response arrives");
    decode_response(&line, method, request_id).expect("host response decodes")
}

#[cfg(feature = "test-support")]
async fn send_user_turn_through_host(endpoint: &LocalEndpoint, session_id: SessionId) -> RunId {
    let response = send_request_through_host(
        endpoint,
        "m4-host-command-test",
        1,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, TurnId::new(), "host turn")
                .expect("turn is valid"),
        )),
    )
    .await;
    let ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(accepted)) =
        response
    else {
        panic!("host accepts the turn")
    };
    let ProtocolAcceptedResultDto::SendUserTurn(turn) = accepted.result() else {
        panic!("host response contains a run")
    };
    let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
        panic!("host turn starts a run")
    };
    run_id
}

#[cfg(feature = "test-support")]
async fn interrupt_run_through_host(
    endpoint: &LocalEndpoint,
    session_id: SessionId,
    run_id: RunId,
) {
    let response = send_request_through_host(
        endpoint,
        "m4-host-interrupt-test",
        1,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::InterruptRun(
            intention_domain::InterruptRunCommandDto::new(session_id, run_id),
        )),
    )
    .await;
    assert!(matches!(
        response,
        ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(accepted))
            if matches!(accepted.result(), ProtocolAcceptedResultDto::InterruptRun(_))
    ));
}

#[cfg(feature = "test-support")]
async fn send_pending_turn_through_host(
    endpoint: &LocalEndpoint,
    session_id: SessionId,
    turn_id: TurnId,
) {
    let response = send_request_through_host(
        endpoint,
        "m4-host-pending-test",
        1,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, turn_id, "pending host turn")
                .expect("pending turn is valid"),
        )),
    )
    .await;
    assert!(matches!(
        response,
        ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(accepted))
            if matches!(
                accepted.result(),
                ProtocolAcceptedResultDto::SendUserTurn(turn)
                    if matches!(turn.outcome(), SendUserTurnOutcomeDto::Pending)
            )
    ));
}

#[tokio::test]
async fn injected_driver_executes_through_the_facade_bridge_and_observes_only_commits() {
    let driver = Arc::new(ScriptedDriver::completed_text());
    let (_directory, facade, snapshot) = fixture_facade(driver.clone());
    let (session_id, run_id) = create_and_start(&facade);
    let observer = RecordingObserver::default();

    let outcome = facade
        .execute_scheduled_model_run_for_daemon_with_tool_executor(
            schedule(session_id, run_id, snapshot),
            ModelCancellationSignal::new(),
            &TokioTime,
            &observer,
            &DaemonToolExecutor::new(facade.clone()),
        )
        .await
        .expect("scripted execution completes");

    assert!(matches!(
        outcome,
        intention_runtime::ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(driver.executions(), 1);
    let commits = observer.commits();
    assert!(
        commits.len() >= 3,
        "facts and completion are observed after commit"
    );
    assert!(
        commits
            .iter()
            .all(|commit| { commit.session_id() == session_id && commit.run_id() == run_id })
    );
    assert!(
        commits
            .windows(2)
            .all(|pair| pair[0].cursor() <= pair[1].cursor())
    );
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn real_async_host_returns_current_run_snapshot_and_accepts_repeated_replay_requests() {
    let driver = Arc::new(ScriptedDriver::completed_text());
    let (_directory, facade, _snapshot) = fixture_facade(driver);
    let (session_id, run_id) = create_and_start(&facade);
    let endpoint = LocalEndpoint::from_instance_id(format!("m4-host-{}", RunId::new()))
        .expect("fixture endpoint is valid");
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("fixture peer connects");
        intention_daemon::serve_test_async_connection(connection, facade).await;
    });

    let client = RunStreamClient::new(endpoint, "m4-host-test").expect("stream client is valid");
    let mut subscription = client
        .subscribe(SubscribeRunCommandDto::new(
            intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
            session_id,
            run_id,
            None,
        ))
        .await
        .expect("current replay arrives");
    assert_eq!(
        subscription.reducer().last_cursor(),
        Some(intention_domain::RunEventCursorDto::new(0))
    );
    subscription
        .request_replay()
        .await
        .expect("repeat replay arrives");
    assert_eq!(
        subscription.reducer().last_cursor(),
        Some(intention_domain::RunEventCursorDto::new(0))
    );
    server.abort();
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn accepted_host_turn_executes_once_then_streams_durable_facts_and_completed_snapshot() {
    let driver = Arc::new(BlockingDriver::new());
    let (_directory, facade, _snapshot) = fixture_facade(driver.clone());
    let session_id = SessionId::new();
    assert!(matches!(
        facade.command(ProtocolCommandDto::CreateSession(
            intention_domain::CreateSessionCommandDto::new(
                intention_types::ProjectId::new(),
                session_id,
                intention_types::WorkspaceId::new(),
                intention_domain::WorkspaceRootDto::parse(
                    std::env::temp_dir().to_string_lossy().into_owned(),
                )
                .expect("fixture workspace is absolute"),
                intention_domain::RunModeDto::Build,
            ),
        )),
        ProtocolCommandResultDto::Accepted(_)
    ));
    let endpoint = LocalEndpoint::from_instance_id(format!("m4-host-outcome-{}", RunId::new()))
        .expect("fixture endpoint is valid");
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
    let server = tokio::spawn(intention_daemon::serve_test_async_listener(
        listener, facade, 3,
    ));

    let run_id = send_user_turn_through_host(&endpoint, session_id).await;
    tokio::time::timeout(Duration::from_secs(1), driver.entered.notified())
        .await
        .expect("host invokes the driver after durable admission");
    let client =
        RunStreamClient::new(endpoint, "m4-host-outcome-test").expect("stream client is valid");
    let mut subscription = client
        .subscribe(SubscribeRunCommandDto::new(
            intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
            session_id,
            run_id,
            None,
        ))
        .await
        .expect("current replay arrives");
    assert_eq!(
        subscription
            .reducer()
            .snapshot()
            .expect("initial replay is authoritative")
            .run_projection()
            .status(),
        RunStatusDto::Running
    );
    driver.release.notify_one();
    let mut completed = false;
    for _ in 0..6 {
        if completed {
            break;
        }
        let _ = tokio::time::timeout(Duration::from_secs(1), subscription.receive())
            .await
            .expect("stream delivers committed frame")
            .expect("stream frame is valid");
        completed = subscription
            .reducer()
            .snapshot()
            .is_some_and(|snapshot| snapshot.run_projection().status() == RunStatusDto::Completed);
    }
    assert!(
        completed,
        "same persistent connection receives completed state"
    );
    assert_eq!(driver.executions(), 1);
    assert!(
        subscription
            .reducer()
            .last_cursor()
            .is_some_and(|cursor| cursor.value() > 0)
    );
    drop(subscription);

    let mut reconnected = client
        .subscribe(SubscribeRunCommandDto::new(
            intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
            session_id,
            run_id,
            None,
        ))
        .await
        .expect("new connection receives a current snapshot");
    assert_eq!(
        reconnected
            .reducer()
            .snapshot()
            .expect("reconnect replay is authoritative")
            .run_projection()
            .status(),
        RunStatusDto::Completed
    );
    reconnected
        .request_replay()
        .await
        .expect("same connection accepts a repeated replay");
    server.await.expect("host accepts command and stream peers");
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn host_interrupt_ends_the_blocked_round_and_the_same_run_continues() {
    let driver = Arc::new(BlockingDriver::new());
    let (_directory, facade, _snapshot) = fixture_facade(driver.clone());
    let session_id = SessionId::new();
    assert!(matches!(
        facade.command(ProtocolCommandDto::CreateSession(
            intention_domain::CreateSessionCommandDto::new(
                intention_types::ProjectId::new(),
                session_id,
                intention_types::WorkspaceId::new(),
                intention_domain::WorkspaceRootDto::parse(
                    std::env::temp_dir().to_string_lossy().into_owned(),
                )
                .expect("fixture workspace is absolute"),
                intention_domain::RunModeDto::Build,
            ),
        )),
        ProtocolCommandResultDto::Accepted(_)
    ));
    let endpoint = LocalEndpoint::from_instance_id(format!("m4-host-interrupt-{}", RunId::new()))
        .expect("fixture endpoint is valid");
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
    let host = intention_daemon::test_host_lifecycle(facade.clone());
    let host_server = host.clone();
    let server = tokio::spawn(async move {
        host_server.serve_connections(listener, 2).await;
    });
    let run_id = send_user_turn_through_host(&endpoint, session_id).await;
    tokio::time::timeout(Duration::from_secs(1), driver.entered.notified())
        .await
        .expect("driver begins the blocked stream");
    interrupt_run_through_host(&endpoint, session_id, run_id).await;
    // The interrupted round ends with a durable notice, so the interrupted
    // signal is replaced and the same run admits its continuation instead of
    // reaching a terminal cancellation.
    tokio::time::timeout(Duration::from_secs(2), driver.entered.notified())
        .await
        .expect("the same run continues after the interruption notice");
    assert_eq!(driver.executions(), 2);
    let replay = facade
        .load_current_run_snapshot_for_daemon(session_id, run_id)
        .expect("continuing run replay reads");
    assert_eq!(replay.run_projection().status(), RunStatusDto::Running);
    let requests = driver.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].run_id(), run_id);
    assert!(
        requests[1]
            .messages()
            .iter()
            .any(|message| message.role() == ModelRoleDto::Notice),
        "the continuation carries the interruption notice"
    );
    let events = facade
        .durable_events_for_test_support(session_id)
        .expect("durable events read");
    assert!(
        events.iter().any(|event| matches!(
            event.payload(),
            intention_domain::DomainEventDto::InterruptNoticeRecorded(fact)
                if event.run_id() == Some(run_id)
                    && matches!(
                        fact.fact().input(),
                        intention_domain::ModelRunFactInputDto::InterruptNoticeRecorded { content }
                            if content == intention_runtime::INTERRUPT_NOTICE
                    )
        )),
        "the interruption notice is durable"
    );

    // Releasing the continuation completes the original run.
    driver.release.notify_one();
    assert!(
        tokio::time::timeout(
            Duration::from_secs(2),
            host.wait_for_execution_completion(session_id, run_id),
        )
        .await
        .expect("the continuation completes"),
        "the exact registered execution task completes"
    );
    assert_eq!(
        facade
            .load_current_run_snapshot_for_daemon(session_id, run_id)
            .expect("completed run replay reads")
            .run_projection()
            .status(),
        RunStatusDto::Completed
    );
    server
        .await
        .expect("host accepts command and interrupt peers");
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn a_pending_turn_joins_the_running_execution_without_a_second_run() {
    let driver = Arc::new(BlockingDriver::new());
    let (_directory, facade, _snapshot) = fixture_facade(driver.clone());
    let session_id = SessionId::new();
    assert!(matches!(
        facade.command(ProtocolCommandDto::CreateSession(
            intention_domain::CreateSessionCommandDto::new(
                intention_types::ProjectId::new(),
                session_id,
                intention_types::WorkspaceId::new(),
                intention_domain::WorkspaceRootDto::parse(
                    std::env::temp_dir().to_string_lossy().into_owned(),
                )
                .expect("fixture workspace is absolute"),
                intention_domain::RunModeDto::Build,
            ),
        )),
        ProtocolCommandResultDto::Accepted(_)
    ));
    let endpoint = LocalEndpoint::from_instance_id(format!("m4-host-pending-{}", RunId::new()))
        .expect("fixture endpoint is valid");
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
    let host = intention_daemon::test_host_lifecycle(facade.clone());
    let host_server = host.clone();
    let server = tokio::spawn(async move {
        host_server.serve_connections(listener, 2).await;
    });

    let run_id = send_user_turn_through_host(&endpoint, session_id).await;
    tokio::time::timeout(Duration::from_secs(1), driver.entered.notified())
        .await
        .expect("first execution is blocked");
    let pending_turn_id = TurnId::new();
    send_pending_turn_through_host(&endpoint, session_id, pending_turn_id).await;
    assert_eq!(
        host.task_count(),
        1,
        "a pending message never admits a second execution"
    );
    driver.release.notify_one();
    tokio::time::timeout(Duration::from_secs(2), driver.entered.notified())
        .await
        .expect("the finished round joins the pending message and continues");
    assert_eq!(driver.executions(), 2);
    let requests = driver.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].run_id(),
        run_id,
        "the pending message joins the same run"
    );
    assert!(
        requests[1].messages().iter().any(|message| {
            message.role() == ModelRoleDto::User && message.content() == "pending host turn"
        }),
        "the joined message is part of the live context"
    );
    let replay = facade
        .load_current_run_snapshot_for_daemon(session_id, run_id)
        .expect("continuing run replay reads");
    assert_eq!(replay.run_projection().status(), RunStatusDto::Running);
    assert!(
        facade
            .load_current_run_snapshot_for_daemon(
                session_id,
                RunId::parse(&pending_turn_id.to_string()).expect("turn identity shape"),
            )
            .is_err(),
        "the pending message never becomes its own run"
    );
    let events = facade
        .durable_events_for_test_support(session_id)
        .expect("durable events read");
    assert!(
        events.iter().any(|event| matches!(
            event.payload(),
            intention_domain::DomainEventDto::UserMessageAppended(fact)
                if event.run_id() == Some(run_id)
                    && matches!(
                        fact.fact().input(),
                        intention_domain::ModelRunFactInputDto::UserMessageAppended { content, .. }
                            if content == "pending host turn"
                    )
        )),
        "the join is a durable run fact"
    );

    driver.release.notify_one();
    assert!(
        tokio::time::timeout(
            Duration::from_secs(2),
            host.wait_for_execution_completion(session_id, run_id),
        )
        .await
        .expect("the continuing execution completes"),
        "the exact run execution completes"
    );
    assert_eq!(
        facade
            .load_current_run_snapshot_for_daemon(session_id, run_id)
            .expect("completed run replay reads")
            .run_projection()
            .status(),
        RunStatusDto::Completed,
        "one run carries the whole continuous session"
    );
    assert_eq!(driver.executions(), 2);
    server.await.expect("host accepts first and pending peers");
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn restart_interrupts_in_flight_and_recovery_promoted_runs_without_resuming_or_exposing_fake_credentials()
 {
    const FAKE_CREDENTIAL: &str = "F-STREAM-RESTART-FAKE-CREDENTIAL-48271";
    let first_driver = Arc::new(BlockingDriver::new());
    let directory = TempDir::new().expect("temporary directory exists");
    let database = directory.path().join("restart.sqlite");
    let snapshot = intention_test_snapshot_with_credential(FAKE_CREDENTIAL);
    let first_facade = DaemonApplicationFacade::open_for_test_support_with_driver(
        &database,
        snapshot.clone(),
        first_driver.clone(),
    )
    .expect("first durable host facade opens");
    let session_id = SessionId::new();
    assert!(matches!(
        first_facade.command(ProtocolCommandDto::CreateSession(
            intention_domain::CreateSessionCommandDto::new(
                intention_types::ProjectId::new(),
                session_id,
                intention_types::WorkspaceId::new(),
                intention_domain::WorkspaceRootDto::parse(
                    std::env::temp_dir().to_string_lossy().into_owned(),
                )
                .expect("fixture workspace is absolute"),
                intention_domain::RunModeDto::Build,
            ),
        )),
        ProtocolCommandResultDto::Accepted(_)
    ));
    let first_endpoint =
        LocalEndpoint::from_instance_id(format!("m4-host-restart-first-{}", RunId::new()))
            .expect("fixture endpoint is valid");
    let listener =
        AsyncLocalListener::bind(first_endpoint.clone()).expect("fixture listener binds");
    let first_host = intention_daemon::test_host_lifecycle(first_facade.clone());
    let first_host_server = first_host.clone();
    let first_server = tokio::spawn(async move {
        first_host_server.serve_connections(listener, 2).await;
    });
    let first_run = send_user_turn_through_host(&first_endpoint, session_id).await;
    tokio::time::timeout(Duration::from_secs(1), first_driver.entered.notified())
        .await
        .expect("first host reaches an in-flight provider stream");
    let pending_turn_id = TurnId::new();
    send_pending_turn_through_host(&first_endpoint, session_id, pending_turn_id).await;
    first_server.await.expect("first host accepted its peers");
    first_host.shutdown().await;
    drop(first_facade);
    assert_eq!(
        first_driver.executions(),
        1,
        "test-only shutdown aborts the original host execution before reopen"
    );

    let restart_driver = Arc::new(ScriptedDriver::completed_text());
    let restarted = DaemonApplicationFacade::open_for_test_support_with_driver(
        &database,
        snapshot,
        restart_driver.clone(),
    )
    .expect("restart recovery opens the existing durable host state");
    let interrupted = restarted
        .load_current_run_snapshot_for_daemon(session_id, first_run)
        .expect("interrupted original replay reads");
    assert_eq!(
        interrupted.run_projection().status(),
        RunStatusDto::Interrupted,
        "recovery interrupts the in-flight run before the second host is ready"
    );
    assert!(
        restarted
            .load_current_run_snapshot_for_daemon(
                session_id,
                RunId::parse(&pending_turn_id.to_string()).expect("turn identity shape"),
            )
            .is_err(),
        "a pending message is durable input, never a run of its own"
    );
    assert_eq!(restart_driver.executions(), 0);

    let replay_json = serde_json::to_string(&interrupted).expect("replay serializes");
    let events = restarted
        .durable_events_for_test_support(session_id)
        .expect("durable restart events read");
    let events_json = serde_json::to_string(&events).expect("durable events serialize");
    let error_json = serde_json::to_string(&intention_types::ErrorDto::unavailable(
        "restart_fixture_error",
        "safe restart fixture error",
    ))
    .expect("safe error serializes");
    let restart_endpoint =
        LocalEndpoint::from_instance_id(format!("m4-host-restart-second-{}", RunId::new()))
            .expect("fixture endpoint is valid");
    let restart_listener =
        AsyncLocalListener::bind(restart_endpoint.clone()).expect("restart listener binds");
    let restart_server = tokio::spawn(intention_daemon::serve_test_async_listener(
        restart_listener,
        restarted,
        1,
    ));
    use intention_transport::{AsyncLocalClientConnection, local_protocol_version};
    let connection = AsyncLocalClientConnection::connect(&restart_endpoint)
        .await
        .expect("restart stream client connects");
    let (_remote, mut requests, mut messages) = connection
        .negotiate(
            ProtocolHelloDto::new(local_protocol_version(), "m4-restart-redaction-test")
                .expect("restart stream hello is valid"),
        )
        .await
        .expect("restart stream client negotiates");
    requests
        .send_message(&encode_request(
            1,
            ProtocolRequestPayloadDto::RunSubscription(SubscribeRunCommandDto::new(
                intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
                session_id,
                first_run,
                None,
            )),
        ))
        .await
        .expect("restart replay request sends");
    let initial_line = messages
        .receive_line()
        .await
        .expect("restart initial response arrives");
    let initial_frame = decode_response(&initial_line, ProtocolMethodDto::RunSubscribe, 1)
        .expect("restart initial response decodes");
    requests
        .send_message(&encode_request(
            2,
            ProtocolRequestPayloadDto::RunSubscription(SubscribeRunCommandDto::new(
                intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
                session_id,
                RunId::new(),
                None,
            )),
        ))
        .await
        .expect("unknown-run request sends");
    let error_line = messages
        .receive_line()
        .await
        .expect("restart error response arrives");
    let transport_error_frame = decode_response(&error_line, ProtocolMethodDto::RunSubscribe, 2)
        .expect("restart error response decodes");
    let initial_frame_json =
        serde_json::to_string(&initial_frame).expect("initial frame serializes");
    let transport_error_json =
        serde_json::to_string(&transport_error_frame).expect("error frame serializes");
    assert!(matches!(
        initial_frame,
        ProtocolResponsePayloadDto::RunSubscription(_)
    ));
    assert!(matches!(
        transport_error_frame,
        ProtocolResponsePayloadDto::RunSubscription(RunSubscriptionResponseDto::Error(ref error))
            if error.code() == "run_replay_not_found"
    ));
    restart_server
        .await
        .expect("restart host accepts stream peer");
    for output in [
        &replay_json,
        &events_json,
        &error_json,
        &initial_frame_json,
        &transport_error_json,
    ] {
        assert!(
            !output.contains(FAKE_CREDENTIAL),
            "actual durable replay/event/error fixture output never contains the credential"
        );
    }
    assert!(
        events.iter().any(|event| matches!(
            event.payload(),
            intention_domain::DomainEventDto::RunStatusChanged(change)
                if change.status() == RunStatusDto::Interrupted
        )),
        "the interrupted terminal transition is durable"
    );
    assert!(
        events.iter().any(|event| matches!(
            event.payload(),
            intention_domain::DomainEventDto::UserTurnPending(pending)
                if pending.turn_id() == pending_turn_id
        )),
        "the pending message survives restart as durable input"
    );
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn host_answers_socket_level_method_not_found_and_invalid_params_errors() {
    use intention_protocol::{
        JSONRPC_INVALID_PARAMS, JSONRPC_METHOD_NOT_FOUND, JsonRpcRequestDto, JsonRpcResponseDto,
    };
    use intention_transport::{AsyncLocalClientConnection, local_protocol_version};

    // W-14: the daemon loop answers a bogus method and a wrong-params request
    // with the spec-mandated codes, correlated by request id.
    let driver = Arc::new(ScriptedDriver::completed_text());
    let (_directory, facade, _snapshot) = fixture_facade(driver);
    let endpoint =
        LocalEndpoint::from_instance_id(format!("m4-host-jsonrpc-errors-{}", RunId::new()))
            .expect("fixture endpoint is valid");
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
    let server = tokio::spawn(intention_daemon::serve_test_async_listener(
        listener, facade, 1,
    ));

    let connection = AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("jsonrpc error client connects");
    let (_remote, mut requests, mut messages) = connection
        .negotiate(
            ProtocolHelloDto::new(local_protocol_version(), "m4-jsonrpc-error-test")
                .expect("jsonrpc error hello is valid"),
        )
        .await
        .expect("jsonrpc error client negotiates");

    let health = ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth);
    for (request_id, method, expected_code, expected_data_code) in [
        (
            11_u64,
            "workspace.bogus",
            JSONRPC_METHOD_NOT_FOUND,
            "jsonrpc_method_not_found",
        ),
        (
            12,
            "turn.send",
            JSONRPC_INVALID_PARAMS,
            "jsonrpc_invalid_params",
        ),
    ] {
        requests
            .send_message(&JsonRpcRequestDto::new(request_id, method, health.clone()))
            .await
            .expect("typed error request sends");
        let line = messages.receive_line().await.expect("error reply arrives");
        let response: JsonRpcResponseDto<ProtocolResponsePayloadDto> =
            JsonRpcResponseDto::parse(&line).expect("the reply is a JSON-RPC response");
        assert_eq!(
            response.id(),
            Some(request_id),
            "the error reply for {method} echoes its request id"
        );
        assert!(
            response.result_value().is_none(),
            "the error reply for {method} carries no result"
        );
        let error = response
            .error_value()
            .expect("an error reply carries the error object");
        assert_eq!(error.code(), expected_code, "method {method}");
        assert_eq!(
            error.to_error().code(),
            expected_data_code,
            "method {method}"
        );
    }
    server
        .await
        .expect("host serves the rejected jsonrpc peers");
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn host_does_not_answer_an_id_less_jsonrpc_notification() {
    use intention_protocol::{JsonRpcNotificationDto, JsonRpcRequestDto, JsonRpcResponseDto};
    use intention_transport::{AsyncLocalClientConnection, local_protocol_version};

    // W-06: a request line without an id is a JSON-RPC notification, so the
    // daemon must not answer it. The next line the client reads must be the
    // reply to the correlated request that follows it.
    let driver = Arc::new(ScriptedDriver::completed_text());
    let (_directory, facade, _snapshot) = fixture_facade(driver);
    let endpoint =
        LocalEndpoint::from_instance_id(format!("m4-host-jsonrpc-notification-{}", RunId::new()))
            .expect("fixture endpoint is valid");
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
    let server = tokio::spawn(intention_daemon::serve_test_async_listener(
        listener, facade, 1,
    ));

    let connection = AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("notification client connects");
    let (_remote, mut requests, mut messages) = connection
        .negotiate(
            ProtocolHelloDto::new(local_protocol_version(), "m4-jsonrpc-notification-test")
                .expect("notification hello is valid"),
        )
        .await
        .expect("notification client negotiates");

    requests
        .send_message(&JsonRpcNotificationDto::new(
            "workspace.bogus",
            ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
        ))
        .await
        .expect("id-less notification sends");
    requests
        .send_message(&JsonRpcRequestDto::new(
            13_u64,
            "daemon.health",
            ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
        ))
        .await
        .expect("correlated health request sends");
    let line = messages.receive_line().await.expect("health reply arrives");
    let response: JsonRpcResponseDto<ProtocolResponsePayloadDto> =
        JsonRpcResponseDto::parse(&line).expect("the reply is a JSON-RPC response");
    assert_eq!(
        response.id(),
        Some(13),
        "the notification must not be answered, and must not consume the correlated reply"
    );
    assert!(matches!(
        decode_response(&line, ProtocolMethodDto::DaemonHealth, 13),
        Ok(ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(health)))
            if health.readiness() == intention_protocol::DaemonReadinessDto::Ready
    ));
    server.await.expect("host serves the notification peer");
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn host_answers_an_explicit_null_id_request_with_the_correlated_error() {
    use intention_protocol::{JSONRPC_INVALID_REQUEST, JsonRpcResponseDto};
    use intention_transport::{AsyncLocalClientConnection, local_protocol_version};

    // W-06 boundary: an explicit `"id": null` member is a request rather than
    // a notification, so the daemon answers it with a null-id error reply.
    let driver = Arc::new(ScriptedDriver::completed_text());
    let (_directory, facade, _snapshot) = fixture_facade(driver);
    let endpoint =
        LocalEndpoint::from_instance_id(format!("m4-host-jsonrpc-null-id-{}", RunId::new()))
            .expect("fixture endpoint is valid");
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
    let server = tokio::spawn(intention_daemon::serve_test_async_listener(
        listener, facade, 1,
    ));

    let connection = AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("null-id client connects");
    let (_remote, mut requests, mut messages) = connection
        .negotiate(
            ProtocolHelloDto::new(local_protocol_version(), "m4-jsonrpc-null-id-test")
                .expect("null-id hello is valid"),
        )
        .await
        .expect("null-id client negotiates");

    requests
        .send_message(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": null,
            "method": "daemon.health",
            "params": {"kind": "query", "data": {"kind": "get_daemon_health"}},
        }))
        .await
        .expect("null-id request sends");
    let line = messages
        .receive_line()
        .await
        .expect("null-id reply arrives");
    let response: JsonRpcResponseDto<ProtocolResponsePayloadDto> =
        JsonRpcResponseDto::parse(&line).expect("the reply is a JSON-RPC response");
    assert_eq!(
        response.id(),
        None,
        "an explicit null id is answered with a null id instead of silence"
    );
    assert!(response.result_value().is_none());
    assert_eq!(
        response
            .error_value()
            .expect("a null-id request receives an error object")
            .code(),
        JSONRPC_INVALID_REQUEST
    );
    server.await.expect("host serves the null-id peer");
}
