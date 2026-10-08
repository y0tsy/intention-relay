//! Streaming-foundation integration fixtures for the daemon host.
//!
//! The fixtures here exercise current durable state only: live `Content` and
//! `Status` frames published from committed values, the current-state snapshot a
//! run subscription returns at registration, the transcript rows holding the run
//! history, and the run projections a restarted daemon serves. A run
//! subscription carries no cursor and no replay tail, and a slow subscriber is
//! closed instead of resynchronized: a re-subscribing client re-reads current
//! state.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Focused daemon-foundation fixtures use assertion conveniences for precise diagnostics."
)]

mod common;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use common::{RecordingObserver, TokioTime, create_session, fixture_facade, schedule, started_run};
#[cfg(feature = "test-support")]
use futures_util::StreamExt;
use futures_util::stream;
#[cfg(feature = "test-support")]
use intention_client::RunStreamClient;
use intention_daemon::DaemonApplicationFacade;
use intention_daemon::DaemonToolExecutor;
#[cfg(feature = "test-support")]
use intention_engine::INTERRUPT_NOTICE;
use intention_engine::ModelRunCommitDto;
#[cfg(feature = "test-support")]
use intention_proto::TurnId;
use intention_proto::{IdempotencyKey, RunId, SessionId};
use intention_proto::{MessageKindDto, RunStatusDto, SendUserTurnCommandDto};
use intention_proto::{
    ProtocolAcceptedResultDto, ProtocolCommandDto, ProtocolCommandResultDto, SendUserTurnOutcomeDto,
};
#[cfg(feature = "test-support")]
use intention_proto::{
    ProtocolHelloDto, ProtocolMethodDto, ProtocolQueryDto, ProtocolQueryResultDto,
    ProtocolRequestPayloadDto, ProtocolResponsePayloadDto, RunSubscriptionResponseDto,
    SubscribeRunCommandDto, decode_response, encode_request,
};
use intention_providers::{
    FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto, ModelEventDto,
    ModelEventStream, ModelExecutionDriver, ModelRequestDto, ModelRoleDto,
};
use intention_test_support::ScriptedDriver;
#[cfg(feature = "test-support")]
use intention_test_support::{FIXTURE_CREDENTIAL, fixture_snapshot};
#[cfg(feature = "test-support")]
use intention_transport::{AsyncLocalListener, LocalEndpoint};
use tempfile::TempDir;

/// Blocks a provider round until the scenario releases it.
///
/// The shared test-support driver scripts only round-by-round events; the
/// interrupt, restart, and pending-turn scenarios need a round that is still
/// streaming when the command arrives, so this driver stays local to the
/// streaming-foundation suite.
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
impl ModelExecutionDriver for BlockingDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    }

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

fn create_and_start(facade: &DaemonApplicationFacade) -> (SessionId, RunId) {
    let session_id = SessionId::new();
    create_session(facade, session_id, &std::env::temp_dir());
    (session_id, started_run(facade, session_id))
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
            SendUserTurnCommandDto::new(session_id, IdempotencyKey::new(), "host turn")
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
            intention_proto::InterruptRunCommandDto::new(session_id, run_id),
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
async fn send_pending_turn_through_host(endpoint: &LocalEndpoint, session_id: SessionId) -> TurnId {
    let response = send_request_through_host(
        endpoint,
        "m4-host-pending-test",
        1,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, IdempotencyKey::new(), "pending host turn")
                .expect("pending turn is valid"),
        )),
    )
    .await;
    let ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(accepted)) =
        response
    else {
        panic!("host accepts the pending turn")
    };
    let ProtocolAcceptedResultDto::SendUserTurn(turn) = accepted.result() else {
        panic!("host response contains a pending turn")
    };
    assert!(
        matches!(turn.outcome(), SendUserTurnOutcomeDto::Pending),
        "the host answers the pending turn with its durable identity"
    );
    turn.turn_id()
}

#[tokio::test]
async fn injected_driver_executes_through_the_facade_bridge_and_observes_only_commits() {
    let driver = Arc::new(ScriptedDriver::completed_text());
    let (_directory, facade, snapshot) = fixture_facade("foundation", driver.clone());
    let (session_id, run_id) = create_and_start(&facade);
    let observer = RecordingObserver::default();

    let outcome = facade
        .execute_scheduled_model_run_for_daemon_with_tool_executor(
            schedule(session_id, run_id, snapshot, None),
            &TokioTime,
            &observer,
            &DaemonToolExecutor::new(facade.clone()),
        )
        .await
        .expect("scripted execution completes");

    assert!(matches!(
        outcome,
        intention_engine::ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(driver.executions(), 1);
    let commits = observer.commits();
    assert!(
        commits.len() >= 3,
        "the running status, committed content, and completed status are observed after commit"
    );
    assert!(commits.iter().all(|commit| match commit {
        ModelRunCommitDto::Content(message) =>
            message.session_id() == session_id && message.run_id() == Some(run_id),
        ModelRunCommitDto::Status {
            session_id: committed_session,
            run_id: committed_run,
            ..
        } => *committed_session == session_id && *committed_run == run_id,
    }));
    assert!(matches!(
        commits.first(),
        Some(ModelRunCommitDto::Status {
            status: RunStatusDto::Running,
            ..
        })
    ));
    assert!(commits.iter().any(|commit| matches!(
        commit,
        ModelRunCommitDto::Content(message)
            if message.kind() == MessageKindDto::Assistant
                && message.text() == "complete response"
    )));
    assert!(matches!(
        commits.last(),
        Some(ModelRunCommitDto::Status {
            status: RunStatusDto::Completed,
            ..
        })
    ));

    // The committed values the observer received re-read as current state.
    let run = facade
        .load_run_projection_for_daemon(session_id, run_id)
        .expect("completed run projection reads");
    assert_eq!(run.status(), RunStatusDto::Completed);
    let messages = facade
        .load_run_messages_for_daemon(session_id, run_id, 16)
        .expect("committed transcript reads");
    assert!(messages.iter().any(|message| {
        message.kind() == MessageKindDto::Assistant && message.text() == "complete response"
    }));
    assert!(
        messages
            .iter()
            .any(|message| { message.kind() == MessageKindDto::User && message.text() == "turn" })
    );
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn real_async_host_snapshot_answers_a_subscription_and_a_resubscribe_rereads_current_state() {
    let driver = Arc::new(ScriptedDriver::completed_text());
    let (_directory, facade, _snapshot) = fixture_facade("foundation", driver);
    let (session_id, run_id) = create_and_start(&facade);
    let endpoint = LocalEndpoint::from_instance_id(format!("m4-host-{}", RunId::new()))
        .expect("fixture endpoint is valid");
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
    let server = tokio::spawn(intention_daemon::serve_test_async_listener(
        listener, facade, 2,
    ));

    let client = RunStreamClient::new(endpoint, "m4-host-test").expect("stream client is valid");
    let subscription = client
        .subscribe(SubscribeRunCommandDto::new(
            intention_proto::CURRENT_DTO_SCHEMA_VERSION,
            session_id,
            run_id,
        ))
        .await
        .expect("the current run snapshot arrives");
    let run = *subscription
        .reducer()
        .run()
        .expect("the subscription snapshot is authoritative");
    assert_eq!(run.session_id(), session_id);
    assert_eq!(run.run_id(), run_id);
    assert_eq!(run.status(), RunStatusDto::Starting);
    drop(subscription);

    // There is no cursor and no replay tail: a fresh subscription re-reads the
    // same current durable state.
    let reconnected = client
        .subscribe(SubscribeRunCommandDto::new(
            intention_proto::CURRENT_DTO_SCHEMA_VERSION,
            session_id,
            run_id,
        ))
        .await
        .expect("the re-subscription re-reads current state");
    assert_eq!(reconnected.reducer().run(), Some(&run));
    drop(reconnected);
    server.await.expect("host serves both subscription peers");
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn accepted_host_turn_executes_once_then_streams_committed_content_and_completed_status() {
    let driver = Arc::new(BlockingDriver::new());
    let (_directory, facade, _snapshot) = fixture_facade("foundation", driver.clone());
    let session_id = SessionId::new();
    create_session(&facade, session_id, &std::env::temp_dir());
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
            intention_proto::CURRENT_DTO_SCHEMA_VERSION,
            session_id,
            run_id,
        ))
        .await
        .expect("the current run snapshot arrives");
    assert_eq!(subscription.reducer().status(), Some(RunStatusDto::Running));
    driver.release.notify_one();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while subscription.reducer().status() != Some(RunStatusDto::Completed) {
        let frame = tokio::time::timeout_at(deadline, subscription.receive())
            .await
            .expect("committed frames arrive before the deadline")
            .expect("the delivered frame is valid");
        assert!(
            frame.is_some(),
            "the stream stays open until the terminal status is published"
        );
    }
    assert_eq!(driver.executions(), 1);
    assert!(subscription.reducer().messages().iter().any(|message| {
        message.kind() == MessageKindDto::Assistant && message.text() == "live response"
    }));
    drop(subscription);

    let reconnected = client
        .subscribe(SubscribeRunCommandDto::new(
            intention_proto::CURRENT_DTO_SCHEMA_VERSION,
            session_id,
            run_id,
        ))
        .await
        .expect("a new connection receives current state");
    assert_eq!(
        reconnected.reducer().status(),
        Some(RunStatusDto::Completed)
    );
    assert!(reconnected.reducer().messages().iter().any(|message| {
        message.kind() == MessageKindDto::Assistant && message.text() == "live response"
    }));
    drop(reconnected);
    server.await.expect("host accepts command and stream peers");
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn host_interrupt_ends_the_blocked_round_and_the_same_run_continues() {
    let driver = Arc::new(BlockingDriver::new());
    let (_directory, facade, _snapshot) = fixture_facade("foundation", driver.clone());
    let session_id = SessionId::new();
    create_session(&facade, session_id, &std::env::temp_dir());
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
    let run = facade
        .load_run_projection_for_daemon(session_id, run_id)
        .expect("continuing run projection reads");
    assert_eq!(run.status(), RunStatusDto::Running);
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
    let messages = facade
        .load_run_messages_for_daemon(session_id, run_id, 64)
        .expect("committed transcript rows read");
    assert!(
        messages
            .iter()
            .any(|message| message.kind() == MessageKindDto::Notice
                && message.text() == INTERRUPT_NOTICE),
        "the interruption notice is a durable transcript row"
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
            .load_run_projection_for_daemon(session_id, run_id)
            .expect("completed run projection reads")
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
    let (_directory, facade, _snapshot) = fixture_facade("foundation", driver.clone());
    let session_id = SessionId::new();
    create_session(&facade, session_id, &std::env::temp_dir());
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
    let pending_turn_id = send_pending_turn_through_host(&endpoint, session_id).await;
    assert_eq!(
        host.task_count(),
        1,
        "a pending message never admits a second execution"
    );
    let pending = facade
        .session_snapshot(session_id, None)
        .expect("session projection reads");
    assert!(
        pending
            .projection()
            .pending_turns()
            .iter()
            .any(|turn| turn.turn_id() == pending_turn_id),
        "the pending message is durable input before it joins"
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
    let joined = facade
        .session_snapshot(session_id, None)
        .expect("joined session projection reads");
    assert!(
        joined.projection().pending_turns().is_empty(),
        "the joined message is no longer pending"
    );
    assert_eq!(
        joined.projection().active_run().map(|run| run.run_id()),
        Some(run_id),
        "the pending message never becomes its own run"
    );
    let run = facade
        .load_run_projection_for_daemon(session_id, run_id)
        .expect("continuing run projection reads");
    assert_eq!(run.status(), RunStatusDto::Running);
    let messages = facade
        .load_run_messages_for_daemon(session_id, run_id, 64)
        .expect("committed transcript rows read");
    assert!(
        messages.iter().any(|message| {
            message.kind() == MessageKindDto::User && message.text() == "pending host turn"
        }),
        "the join is a durable transcript row of the same run"
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
            .load_run_projection_for_daemon(session_id, run_id)
            .expect("completed run projection reads")
            .status(),
        RunStatusDto::Completed,
        "one run carries the whole continuous session"
    );
    assert_eq!(driver.executions(), 2);
    server.await.expect("host accepts first and pending peers");
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn restart_interrupts_in_flight_runs_without_resuming_or_exposing_fake_credentials() {
    let first_driver = Arc::new(BlockingDriver::new());
    let directory = TempDir::new().expect("temporary directory exists");
    let database = directory.path().join("restart.sqlite");
    let snapshot = fixture_snapshot();
    let first_facade = DaemonApplicationFacade::open_for_test_support_with_driver(
        &database,
        snapshot.clone(),
        first_driver.clone(),
    )
    .expect("first durable host facade opens");
    let session_id = SessionId::new();
    create_session(&first_facade, session_id, &std::env::temp_dir());
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
    let pending_turn_id = send_pending_turn_through_host(&first_endpoint, session_id).await;
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
        .load_run_projection_for_daemon(session_id, first_run)
        .expect("interrupted original projection reads");
    assert_eq!(
        interrupted.status(),
        RunStatusDto::Interrupted,
        "recovery interrupts the in-flight run before the second host is ready"
    );
    let recovered = restarted
        .session_snapshot(session_id, None)
        .expect("recovered session projection reads");
    assert!(
        recovered.projection().active_run().is_none(),
        "the interrupted run is terminal after recovery"
    );
    assert!(
        recovered
            .projection()
            .pending_turns()
            .iter()
            .any(|turn| turn.turn_id() == pending_turn_id),
        "the pending message survives restart as durable input"
    );
    let interrupted_messages = restarted
        .load_run_messages_for_daemon(session_id, first_run, 64)
        .expect("interrupted transcript rows read");
    assert!(
        !interrupted_messages
            .iter()
            .any(|message| message.text() == "pending host turn"),
        "a pending message is durable input, never part of the interrupted run"
    );
    assert_eq!(restart_driver.executions(), 0);

    let projection_json = serde_json::to_string(&interrupted).expect("projection serializes");
    let transcript_json =
        serde_json::to_string(&interrupted_messages).expect("transcript rows serialize");
    let error_json = serde_json::to_string(&intention_proto::ErrorDto::unavailable(
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
                intention_proto::CURRENT_DTO_SCHEMA_VERSION,
                session_id,
                first_run,
            )),
        ))
        .await
        .expect("restart snapshot request sends");
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
                intention_proto::CURRENT_DTO_SCHEMA_VERSION,
                session_id,
                RunId::new(),
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
    let ProtocolResponsePayloadDto::RunSubscription(RunSubscriptionResponseDto::Snapshot(snapshot)) =
        &initial_frame
    else {
        panic!("restart subscription answers with the current snapshot")
    };
    assert_eq!(
        snapshot.run().status(),
        RunStatusDto::Interrupted,
        "the restarted daemon serves the interrupted run's current state"
    );
    assert!(matches!(
        transport_error_frame,
        ProtocolResponsePayloadDto::RunSubscription(RunSubscriptionResponseDto::Error(ref error))
            if error.code() == "storage_record_not_found"
    ));
    restart_server
        .await
        .expect("restart host accepts stream peer");
    for output in [
        &projection_json,
        &transcript_json,
        &error_json,
        &initial_frame_json,
        &transport_error_json,
    ] {
        assert!(
            !output.contains(FIXTURE_CREDENTIAL),
            "actual durable projection/transcript/error fixture output never contains the credential"
        );
    }
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn host_answers_socket_level_method_not_found_and_invalid_params_errors() {
    use intention_proto::{
        JSONRPC_INVALID_PARAMS, JSONRPC_METHOD_NOT_FOUND, JsonRpcRequestDto, JsonRpcResponseDto,
    };
    use intention_transport::{AsyncLocalClientConnection, local_protocol_version};

    // W-14: the daemon loop answers a bogus method and a wrong-params request
    // with the spec-mandated codes, correlated by request id.
    let driver = Arc::new(ScriptedDriver::completed_text());
    let (_directory, facade, _snapshot) = fixture_facade("foundation", driver);
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
    use intention_proto::{JsonRpcNotificationDto, JsonRpcRequestDto, JsonRpcResponseDto};
    use intention_transport::{AsyncLocalClientConnection, local_protocol_version};

    // W-06: a request line without an id is a JSON-RPC notification, so the
    // daemon must not answer it. The next line the client reads must be the
    // reply to the correlated request that follows it.
    let driver = Arc::new(ScriptedDriver::completed_text());
    let (_directory, facade, _snapshot) = fixture_facade("foundation", driver);
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
            if health.readiness() == intention_proto::DaemonReadinessDto::Ready
    ));
    server.await.expect("host serves the notification peer");
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn host_answers_an_explicit_null_id_request_with_the_correlated_error() {
    use intention_proto::{JSONRPC_INVALID_REQUEST, JsonRpcResponseDto};
    use intention_transport::{AsyncLocalClientConnection, local_protocol_version};

    // W-06 boundary: an explicit `"id": null` member is a request rather than
    // a notification, so the daemon answers it with a null-id error reply.
    let driver = Arc::new(ScriptedDriver::completed_text());
    let (_directory, facade, _snapshot) = fixture_facade("foundation", driver);
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
