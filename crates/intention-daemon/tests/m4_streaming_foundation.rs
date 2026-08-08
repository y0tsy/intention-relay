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
use intention_domain::{GetSessionSnapshotQueryDto, RunStatusDto, SendUserTurnCommandDto};
use intention_model::{
    FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto, ModelDriver, ModelEventDto,
    ModelEventStream, ModelExecutionDriver, ModelMessageDto, ModelRequestDto, ModelRoleDto,
};
#[cfg(feature = "test-support")]
use intention_protocol::ProtocolResponsePayloadDto;
#[cfg(feature = "test-support")]
use intention_protocol::SubscribeRunCommandDto;
use intention_protocol::{
    ProtocolAcceptedResultDto, ProtocolCommandDto, ProtocolCommandResultDto, ProtocolQueryDto,
    ProtocolQueryResultDto, SendUserTurnOutcomeDto,
};
#[cfg(feature = "test-support")]
use intention_runtime::ModelRunFirstAppendGate;
use intention_runtime::{
    ModelRunCommitDto, ModelRunCommitObserver, ModelSleepFuture, ModelTimePort,
};
#[cfg(feature = "test-support")]
use intention_transport::{AsyncLocalListener, LocalEndpoint};
#[cfg(feature = "test-support")]
use intention_types::{CorrelationIdDto, SchemaVersionDto};
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
struct FirstAppendBarrier {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

#[cfg(feature = "test-support")]
impl FirstAppendBarrier {
    fn new() -> Self {
        Self {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

#[cfg(feature = "test-support")]
impl ModelRunFirstAppendGate for FirstAppendBarrier {
    fn wait_before_first_append(&self) -> ModelSleepFuture<'_> {
        self.entered.notify_one();
        let release = Arc::clone(&self.release);
        Box::pin(async move { release.notified().await })
    }
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
    let Some(ProtocolAcceptedResultDto::SendUserTurn(turn)) = accepted.result() else {
        panic!("fixture result is a turn")
    };
    let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
        panic!("first turn starts")
    };
    (session_id, run_id)
}

#[cfg(feature = "test-support")]
async fn send_user_turn_through_host(endpoint: &LocalEndpoint, session_id: SessionId) -> RunId {
    use intention_protocol::{
        ProtocolHelloDto, ProtocolMessageDto, ProtocolRequestEnvelopeDto, ProtocolRequestPayloadDto,
    };
    use intention_transport::{AsyncLocalClientConnection, local_protocol_version};

    let connection = AsyncLocalClientConnection::connect(endpoint)
        .await
        .expect("ordinary client connects");
    let (_remote, mut requests, mut responses) = connection
        .negotiate(
            ProtocolHelloDto::new(
                local_protocol_version(),
                vec![
                    intention_protocol::ProtocolCapabilityDto::SessionSubscriptions,
                    intention_protocol::ProtocolCapabilityDto::CorrelatedRequests,
                    intention_protocol::ProtocolCapabilityDto::DaemonHealth,
                ],
                "m4-host-command-test",
            )
            .expect("ordinary hello is valid"),
        )
        .await
        .expect("ordinary client negotiates");
    let correlation = CorrelationIdDto::new();
    requests
        .send(&ProtocolRequestEnvelopeDto::new(
            local_protocol_version(),
            correlation,
            ProtocolMessageDto::new(
                SchemaVersionDto::new(1, 0),
                ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(
                    SendUserTurnCommandDto::new(session_id, TurnId::new(), "host turn")
                        .expect("turn is valid"),
                )),
            ),
        ))
        .await
        .expect("host turn sends");
    let response = responses
        .receive()
        .await
        .expect("host turn response arrives");
    assert_eq!(response.correlation_id(), correlation);
    let ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(accepted)) =
        response.message().payload()
    else {
        panic!("host accepts the turn")
    };
    let Some(ProtocolAcceptedResultDto::SendUserTurn(turn)) = accepted.result() else {
        panic!("host response contains a run")
    };
    let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
        panic!("host turn starts a run")
    };
    run_id
}

#[cfg(feature = "test-support")]
async fn stop_run_through_host(endpoint: &LocalEndpoint, session_id: SessionId, run_id: RunId) {
    use intention_protocol::{
        ProtocolHelloDto, ProtocolMessageDto, ProtocolRequestEnvelopeDto, ProtocolRequestPayloadDto,
    };
    use intention_transport::{AsyncLocalClientConnection, local_protocol_version};

    let connection = AsyncLocalClientConnection::connect(endpoint)
        .await
        .expect("stop client connects");
    let (_remote, mut requests, mut responses) = connection
        .negotiate(
            ProtocolHelloDto::new(
                local_protocol_version(),
                vec![
                    intention_protocol::ProtocolCapabilityDto::SessionSubscriptions,
                    intention_protocol::ProtocolCapabilityDto::CorrelatedRequests,
                    intention_protocol::ProtocolCapabilityDto::DaemonHealth,
                ],
                "m4-host-stop-test",
            )
            .expect("stop hello is valid"),
        )
        .await
        .expect("stop client negotiates");
    let correlation = CorrelationIdDto::new();
    requests
        .send(&ProtocolRequestEnvelopeDto::new(
            local_protocol_version(),
            correlation,
            ProtocolMessageDto::new(
                SchemaVersionDto::new(1, 0),
                ProtocolRequestPayloadDto::Command(ProtocolCommandDto::StopRun(
                    intention_domain::StopRunCommandDto::new(session_id, run_id),
                )),
            ),
        ))
        .await
        .expect("stop request sends");
    let response = responses.receive().await.expect("stop response arrives");
    assert_eq!(response.correlation_id(), correlation);
    assert!(matches!(
        response.message().payload(),
        ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(_))
    ));
}

#[cfg(feature = "test-support")]
async fn send_queued_turn_through_host(
    endpoint: &LocalEndpoint,
    session_id: SessionId,
    turn_id: TurnId,
) {
    use intention_protocol::{
        ProtocolHelloDto, ProtocolMessageDto, ProtocolRequestEnvelopeDto, ProtocolRequestPayloadDto,
    };
    use intention_transport::{AsyncLocalClientConnection, local_protocol_version};

    let connection = AsyncLocalClientConnection::connect(endpoint)
        .await
        .expect("queued-turn client connects");
    let (_remote, mut requests, mut responses) = connection
        .negotiate(
            ProtocolHelloDto::new(
                local_protocol_version(),
                vec![
                    intention_protocol::ProtocolCapabilityDto::SessionSubscriptions,
                    intention_protocol::ProtocolCapabilityDto::CorrelatedRequests,
                    intention_protocol::ProtocolCapabilityDto::DaemonHealth,
                ],
                "m4-host-queue-test",
            )
            .expect("queue hello is valid"),
        )
        .await
        .expect("queued-turn client negotiates");
    let correlation = CorrelationIdDto::new();
    requests
        .send(&ProtocolRequestEnvelopeDto::new(
            local_protocol_version(),
            correlation,
            ProtocolMessageDto::new(
                SchemaVersionDto::new(1, 0),
                ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(
                    SendUserTurnCommandDto::new(session_id, turn_id, "queued host turn")
                        .expect("queued turn is valid"),
                )),
            ),
        ))
        .await
        .expect("queued turn sends");
    let response = responses
        .receive()
        .await
        .expect("queued turn response arrives");
    assert_eq!(response.correlation_id(), correlation);
    assert!(matches!(
        response.message().payload(),
        ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(accepted))
            if matches!(
                accepted.result(),
                Some(ProtocolAcceptedResultDto::SendUserTurn(turn))
                    if matches!(turn.outcome(), SendUserTurnOutcomeDto::Queued { .. })
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
        .execute_scheduled_model_run_for_daemon(
            schedule(session_id, run_id, snapshot),
            ModelCancellationSignal::new(),
            &TokioTime,
            &observer,
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
            intention_types::SchemaVersionDto::new(1, 0),
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
            SchemaVersionDto::new(1, 0),
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
            SchemaVersionDto::new(1, 0),
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
async fn host_stop_cancels_blocked_execution_without_late_facts() {
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
    let endpoint = LocalEndpoint::from_instance_id(format!("m4-host-stop-{}", RunId::new()))
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
    stop_run_through_host(&endpoint, session_id, run_id).await;
    for _ in 0..200 {
        if facade
            .load_current_run_replay_for_daemon(session_id, run_id)
            .expect("run replay reads")
            .snapshot()
            .run_projection()
            .status()
            == RunStatusDto::Cancelled
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let replay = facade
        .load_current_run_replay_for_daemon(session_id, run_id)
        .expect("cancelled run replay reads");
    assert_eq!(
        replay.snapshot().run_projection().status(),
        RunStatusDto::Cancelled
    );
    assert_eq!(
        replay.snapshot().cursor().value(),
        1,
        "only initial attempt fact persisted"
    );
    assert_eq!(driver.executions(), 1);
    driver.release.notify_one();
    assert!(
        tokio::time::timeout(
            Duration::from_secs(1),
            host.wait_for_execution_completion(session_id, run_id),
        )
        .await
        .expect("released exact host execution completes"),
        "the exact registered execution task completes after release"
    );
    server.await.expect("host accepts command and stop peers");
    let released_replay = facade
        .load_current_run_replay_for_daemon(session_id, run_id)
        .expect("released cancellation replay reads");
    assert_eq!(
        released_replay.snapshot().run_projection().status(),
        RunStatusDto::Cancelled
    );
    assert_eq!(
        released_replay.snapshot().cursor().value(),
        1,
        "released driver facts cannot arrive after cancellation"
    );
    assert_eq!(
        driver.executions(),
        1,
        "release cannot admit another provider execution"
    );
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn terminal_promotion_schedules_the_persisted_queued_run_once_through_the_real_host() {
    let driver = Arc::new(BlockingDriver::new());
    let (_directory, facade, snapshot) = fixture_facade(driver.clone());
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
    let endpoint = LocalEndpoint::from_instance_id(format!("m4-host-promotion-{}", RunId::new()))
        .expect("fixture endpoint is valid");
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
    let server = tokio::spawn(intention_daemon::serve_test_async_listener(
        listener,
        facade.clone(),
        2,
    ));

    let first_run = send_user_turn_through_host(&endpoint, session_id).await;
    tokio::time::timeout(Duration::from_secs(1), driver.entered.notified())
        .await
        .expect("first execution is blocked");
    let queued_turn_id = TurnId::new();
    let promoted_run =
        RunId::parse(&queued_turn_id.to_string()).expect("turn identity is a run identity");
    send_queued_turn_through_host(&endpoint, session_id, queued_turn_id).await;
    driver.release.notify_one();
    tokio::time::timeout(Duration::from_secs(1), driver.entered.notified())
        .await
        .expect("terminal observer schedules the persisted promoted run");
    assert_eq!(driver.executions(), 2);
    let requests = driver.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].run_id(), promoted_run);
    assert_eq!(requests[1].model(), snapshot.resolved().provider().model());
    assert!(matches!(requests[1].messages().last(), Some(message)
        if message.role() == ModelRoleDto::User && message.content() == "queued host turn"));
    let promoted_replay = facade
        .load_current_run_replay_for_daemon(session_id, promoted_run)
        .expect("promoted run has durable replay");
    assert_eq!(
        promoted_replay.snapshot().run_projection().status(),
        RunStatusDto::Running
    );
    assert_ne!(first_run, promoted_run);
    driver.release.notify_one();
    for _ in 0..200 {
        if facade
            .load_current_run_replay_for_daemon(session_id, promoted_run)
            .expect("promoted replay reads")
            .snapshot()
            .run_projection()
            .status()
            == RunStatusDto::Completed
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        driver.executions(),
        2,
        "duplicate host admission cannot execute the promoted run again"
    );
    server.await.expect("host accepts first and queued peers");
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
    let queued_turn_id = TurnId::new();
    let promoted_run =
        RunId::parse(&queued_turn_id.to_string()).expect("turn identity is a run identity");
    send_queued_turn_through_host(&first_endpoint, session_id, queued_turn_id).await;
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
        .load_current_run_replay_for_daemon(session_id, first_run)
        .expect("interrupted original replay reads");
    let successor = restarted
        .load_current_run_replay_for_daemon(session_id, promoted_run)
        .expect("recovery-promoted successor replay reads");
    assert_eq!(
        interrupted.snapshot().run_projection().status(),
        RunStatusDto::Interrupted,
        "recovery finishes before the second host becomes ready"
    );
    assert_eq!(
        successor.snapshot().run_projection().status(),
        RunStatusDto::Starting,
        "recovery promotion preserves queued durable input but does not resume it"
    );
    assert_eq!(restart_driver.executions(), 0);

    let replay_json = serde_json::to_string(&interrupted).expect("replay serializes");
    let successor_json = serde_json::to_string(&successor).expect("successor replay serializes");
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
    use intention_protocol::{
        ProtocolCapabilityDto, ProtocolHelloDto, ProtocolMessageDto,
        RunSubscriptionRequestEnvelopeDto,
    };
    use intention_transport::{AsyncLocalClientConnection, local_protocol_version};
    let connection = AsyncLocalClientConnection::connect(&restart_endpoint)
        .await
        .expect("restart stream client connects");
    let (_remote, mut requests, mut frames) = connection
        .negotiate_daemon_frames(
            ProtocolHelloDto::new(
                local_protocol_version(),
                vec![ProtocolCapabilityDto::RunStreamSubscriptions],
                "m4-restart-redaction-test",
            )
            .expect("restart stream hello is valid"),
        )
        .await
        .expect("restart stream client negotiates");
    let replay_correlation = CorrelationIdDto::new();
    requests
        .send_run_subscription(&RunSubscriptionRequestEnvelopeDto::new(
            local_protocol_version(),
            replay_correlation,
            ProtocolMessageDto::new(
                SchemaVersionDto::new(1, 0),
                SubscribeRunCommandDto::new(
                    SchemaVersionDto::new(1, 0),
                    session_id,
                    first_run,
                    None,
                ),
            ),
        ))
        .await
        .expect("restart replay request sends");
    let initial_frame = frames
        .receive()
        .await
        .expect("restart initial frame arrives");
    let error_correlation = CorrelationIdDto::new();
    requests
        .send_run_subscription(&RunSubscriptionRequestEnvelopeDto::new(
            local_protocol_version(),
            error_correlation,
            ProtocolMessageDto::new(
                SchemaVersionDto::new(1, 0),
                SubscribeRunCommandDto::new(
                    SchemaVersionDto::new(1, 0),
                    session_id,
                    RunId::new(),
                    None,
                ),
            ),
        ))
        .await
        .expect("unknown-run request sends");
    let transport_error_frame = frames.receive().await.expect("restart error frame arrives");
    let initial_frame_json =
        serde_json::to_string(&initial_frame).expect("initial frame serializes");
    let transport_error_json =
        serde_json::to_string(&transport_error_frame).expect("error frame serializes");
    assert!(matches!(
        initial_frame,
        intention_protocol::ProtocolDaemonFrameDto::Response(_)
    ));
    assert!(matches!(
        transport_error_frame,
        intention_protocol::ProtocolDaemonFrameDto::Response(ref response)
            if response.correlation_id() == error_correlation
    ));
    restart_server
        .await
        .expect("restart host accepts stream peer");
    for output in [
        &replay_json,
        &successor_json,
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
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn host_stop_before_task_registration_terminalizes_without_provider_work_or_registry_leak() {
    let driver = Arc::new(BlockingDriver::new());
    let (_directory, facade, _snapshot) = fixture_facade(driver.clone());
    let (session_id, run_id) = create_and_start(&facade);
    let endpoint = LocalEndpoint::from_instance_id(format!("m4-host-stop-before-{}", RunId::new()))
        .expect("fixture endpoint is valid");
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
    let host = intention_daemon::test_host_lifecycle(facade.clone());
    let host_server = host.clone();
    let server = tokio::spawn(async move {
        host_server.serve_connections(listener, 1).await;
    });

    stop_run_through_host(&endpoint, session_id, run_id).await;
    server.await.expect("host accepts the stop peer");
    host.admit_starting_run(session_id, run_id);
    for _ in 0..200 {
        if facade
            .load_current_run_replay_for_daemon(session_id, run_id)
            .expect("run replay reads")
            .snapshot()
            .run_projection()
            .status()
            == RunStatusDto::Cancelled
            && host.task_count() == 0
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let replay = facade
        .load_current_run_replay_for_daemon(session_id, run_id)
        .expect("cancelled run replay reads");
    assert_eq!(
        replay.snapshot().run_projection().status(),
        RunStatusDto::Cancelled
    );
    assert_eq!(replay.snapshot().cursor().value(), 0);
    assert_eq!(driver.executions(), 0, "provider work never begins");
    assert_eq!(
        host.task_count(),
        0,
        "terminalizer removes its registry entry"
    );
    let events = facade
        .durable_events_for_test_support(session_id)
        .expect("durable events read");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event.payload(),
                intention_domain::DomainEventDto::RunStatusChanged(change)
                    if event.run_id() == Some(run_id) && change.status() == RunStatusDto::Cancelled
            ))
            .count(),
        1,
        "exactly one task-owned terminal cancellation is durable"
    );
    host.shutdown().await;
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn terminalizer_retries_an_injected_durable_failure_before_registry_cleanup() {
    let driver = Arc::new(BlockingDriver::new());
    let (_directory, facade, _snapshot) = fixture_facade(driver.clone());
    let (session_id, run_id) = create_and_start(&facade);
    let endpoint =
        LocalEndpoint::from_instance_id(format!("m4-terminalizer-retry-{}", RunId::new()))
            .expect("fixture endpoint is valid");
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
    let host = intention_daemon::test_host_lifecycle(facade.clone());
    host.inject_terminalizer_failure_once();
    let host_server = host.clone();
    let server = tokio::spawn(async move {
        host_server.serve_connections(listener, 1).await;
    });

    stop_run_through_host(&endpoint, session_id, run_id).await;
    server.await.expect("host accepts the stop peer");
    tokio::time::timeout(Duration::from_secs(1), host.wait_for_terminalizer_failure())
        .await
        .expect("terminalizer observes the injected durable failure");
    assert_eq!(
        facade
            .load_current_run_replay_for_daemon(session_id, run_id)
            .expect("cancelling replay reads")
            .snapshot()
            .run_projection()
            .status(),
        RunStatusDto::Cancelling
    );
    assert_eq!(
        host.task_count(),
        1,
        "failed terminalizer retains exact task ownership"
    );
    host.release_terminalizer_retry();
    tokio::time::timeout(
        Duration::from_secs(1),
        host.wait_for_terminalizer_completion(),
    )
    .await
    .expect("terminalizer retries to durable completion");
    host.wait_for_task_cleanup().await;
    let replay = facade
        .load_current_run_replay_for_daemon(session_id, run_id)
        .expect("terminal run replay reads");
    assert_eq!(
        replay.snapshot().run_projection().status(),
        RunStatusDto::Cancelled
    );
    assert_eq!(replay.snapshot().cursor().value(), 0);
    assert_eq!(host.terminalizer_attempts(), 2);
    assert_eq!(
        host.task_count(),
        0,
        "registry cleanup follows durable terminal reread"
    );
    assert_eq!(
        driver.executions(),
        0,
        "terminalizer never invokes the provider"
    );
    host.shutdown().await;
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn host_stop_between_starting_replay_and_first_append_terminalizes_once_without_provider_work()
 {
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
    let endpoint =
        LocalEndpoint::from_instance_id(format!("m4-host-first-append-{}", RunId::new()))
            .expect("fixture endpoint is valid");
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
    let barrier = Arc::new(FirstAppendBarrier::new());
    let server = tokio::spawn(
        intention_daemon::serve_test_async_listener_with_first_append_gate(
            listener,
            facade.clone(),
            2,
            barrier.clone(),
        ),
    );

    let run_id = send_user_turn_through_host(&endpoint, session_id).await;
    tokio::time::timeout(Duration::from_secs(1), barrier.entered.notified())
        .await
        .expect("task observes Starting before its first append");
    stop_run_through_host(&endpoint, session_id, run_id).await;
    barrier.release.notify_one();
    for _ in 0..200 {
        if facade
            .load_current_run_replay_for_daemon(session_id, run_id)
            .expect("run replay reads")
            .snapshot()
            .run_projection()
            .status()
            == RunStatusDto::Cancelled
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let replay = facade
        .load_current_run_replay_for_daemon(session_id, run_id)
        .expect("cancelled run replay reads");
    assert_eq!(
        replay.snapshot().run_projection().status(),
        RunStatusDto::Cancelled
    );
    assert_eq!(replay.snapshot().cursor().value(), 0);
    assert_eq!(driver.executions(), 0, "provider work never begins");
    let events = facade
        .durable_events_for_test_support(session_id)
        .expect("durable events read");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event.payload(),
                intention_domain::DomainEventDto::RunStatusChanged(change)
                    if event.run_id() == Some(run_id) && change.status() == RunStatusDto::Cancelled
            ))
            .count(),
        1,
        "the task owns exactly one terminal cancellation write"
    );
    server.await.expect("host accepts turn and stop peers");
}

#[test]
fn daemon_stop_seam_persists_cancelling_without_direct_terminalization() {
    let driver = Arc::new(ScriptedDriver::completed_text());
    let (_directory, facade, _snapshot) = fixture_facade(driver);
    let (session_id, run_id) = create_and_start(&facade);

    let result = facade
        .stop_run_for_daemon_host(session_id, run_id)
        .expect("host stop commits cancelling");
    assert!(matches!(result, ProtocolAcceptedResultDto::StopRun(_)));
    assert!(matches!(
        facade.query(ProtocolQueryDto::GetSessionSnapshot(GetSessionSnapshotQueryDto::new(session_id))),
        ProtocolQueryResultDto::SessionSnapshot(snapshot)
            if snapshot.projection().is_some_and(|projection| {
                projection.active_run().is_some_and(|run| run.status() == RunStatusDto::Cancelling)
            })
    ));
}
