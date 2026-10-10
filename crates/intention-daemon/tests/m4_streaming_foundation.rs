//! Streaming-foundation integration fixtures for the daemon host.
//!
//! The fixtures here exercise current durable state only: the current-state
//! snapshot a run subscription returns, the transcript rows holding the run
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

#[cfg(feature = "test-support")]
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[cfg(feature = "test-support")]
use common::{create_session, fixture_facade};
#[cfg(feature = "test-support")]
use futures_util::StreamExt;
use futures_util::stream;
use intention_daemon::DaemonApplicationFacade;
#[cfg(feature = "test-support")]
use intention_engine::{INTERRUPT_NOTICE, INTERRUPTED_MARKER};
#[cfg(feature = "test-support")]
use intention_proto::RunModeDto;
use intention_proto::SendUserTurnOutcomeDto;
#[cfg(feature = "test-support")]
use intention_proto::TurnId;
#[cfg(feature = "test-support")]
use intention_proto::{
    ClientRequestDto, InterruptRunCommandDto, ProtocolDaemonMessageDto, ProtocolResultDto,
    RunStreamFrameDto, RunSubscriptionSnapshotDto, SubscribeRunCommandDto, TextDeltaChannelDto,
    decode_response, encode_request, run_status_is_terminal,
};
use intention_proto::{IdempotencyKey, RunId, SessionId};
use intention_proto::{MessageKindDto, RunStatusDto, SendUserTurnCommandDto};
use intention_providers::{
    FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto, ModelEventDto,
    ModelEventStream, ModelExecutionDriver, ModelRequestDto, ModelRoleDto,
};
use intention_test_support::ScriptedDriver;
#[cfg(feature = "test-support")]
use intention_test_support::{FIXTURE_CREDENTIAL, fixture_snapshot};
#[cfg(feature = "test-support")]
use intention_transport::{AsyncLocalListener, AsyncMessageReceiver, LocalEndpoint};
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

/// Delivers a partial model answer and then blocks its first round.
///
/// The environment-stop scenario needs the answer to be *in flight* when the
/// stop arrives: this driver yields its first text delta and then waits, so the
/// cancellation commits exactly the text the model had produced. Every later
/// round completes immediately, so a drained run reaches its terminal state.
#[cfg(feature = "test-support")]
struct PartialAnswerDriver {
    executions: Mutex<usize>,
    reached: Arc<tokio::sync::Notify>,
}

#[cfg(feature = "test-support")]
impl PartialAnswerDriver {
    fn new() -> Self {
        Self {
            executions: Mutex::new(0),
            reached: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

#[cfg(feature = "test-support")]
impl ModelExecutionDriver for PartialAnswerDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    }

    fn execute(
        &self,
        _request: ModelRequestDto,
        _cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        let execution = {
            let mut executions = self
                .executions
                .lock()
                .expect("driver recorder remains available");
            *executions += 1;
            *executions
        };
        if execution == 1 {
            let reached = Arc::clone(&self.reached);
            return Box::pin(
                stream::iter(vec![
                    Ok(ModelEventDto::started()),
                    Ok(ModelEventDto::text_delta("partial answer").expect("fixture text is valid")),
                ])
                .chain(stream::once(async move {
                    // The engine holds the delta in its pending step by the time
                    // this chained future is polled, so the stop it wakes commits
                    // exactly that text.
                    reached.notify_one();
                    std::future::pending::<()>().await;
                    Ok(ModelEventDto::finished(FinishReasonDto::Stop))
                })),
            );
        }
        Box::pin(stream::iter(vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("final answer").expect("fixture text is valid")),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ]))
    }
}

/// The chunked answer the gated driver streams, as its chunks.
#[cfg(feature = "test-support")]
const GATED_CHUNKS: [&str; 3] = ["alpha ", "beta ", "gamma"];

/// The reasoning chunks the two-channel driver streams before its answer.
#[cfg(feature = "test-support")]
const GATED_REASONING_CHUNKS: [&str; 3] = ["weigh", "ing ", "the options"];

/// The answer chunks the two-channel driver streams after its reasoning.
#[cfg(feature = "test-support")]
const GATED_ANSWER_CHUNKS: [&str; 3] = ["alpha ", "beta ", "gamma"];

/// Streams one step's reasoning and then its answer once the scenario releases
/// it.
///
/// The run must already be subscribed before the provider streams, so the
/// accepted turn is durable while both channels of the step are still pending.
#[cfg(feature = "test-support")]
struct GatedChannelDriver {
    release: Arc<tokio::sync::Notify>,
}

#[cfg(feature = "test-support")]
impl ModelExecutionDriver for GatedChannelDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    }

    fn execute(
        &self,
        _request: ModelRequestDto,
        _cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        let release = Arc::clone(&self.release);
        let events = GATED_REASONING_CHUNKS
            .iter()
            .map(|chunk| Ok(ModelEventDto::reasoning_delta(*chunk).expect("reasoning is valid")))
            .chain(
                GATED_ANSWER_CHUNKS
                    .iter()
                    .map(|chunk| Ok(ModelEventDto::text_delta(*chunk).expect("text is valid"))),
            )
            .chain(std::iter::once(Ok(ModelEventDto::finished(
                FinishReasonDto::Stop,
            ))))
            .collect::<Vec<_>>();
        Box::pin(
            stream::once(async move {
                release.notified().await;
                Ok(ModelEventDto::started())
            })
            .chain(stream::iter(events)),
        )
    }
}

/// Streams one chunked answer once the scenario releases it.
///
/// The run must already be subscribed before the provider streams, so the first
/// round waits for the scenario: the accepted turn is durable while every chunk
/// of the answer is still pending.
#[cfg(feature = "test-support")]
struct GatedChunksDriver {
    release: Arc<tokio::sync::Notify>,
}

#[cfg(feature = "test-support")]
impl ModelExecutionDriver for GatedChunksDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    }

    fn execute(
        &self,
        _request: ModelRequestDto,
        _cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        let release = Arc::clone(&self.release);
        Box::pin(
            stream::once(async move {
                release.notified().await;
                Ok(ModelEventDto::started())
            })
            .chain(stream::iter(vec![
                Ok(ModelEventDto::text_delta(GATED_CHUNKS[0]).expect("fixture text is valid")),
                Ok(ModelEventDto::text_delta(GATED_CHUNKS[1]).expect("fixture text is valid")),
                Ok(ModelEventDto::text_delta(GATED_CHUNKS[2]).expect("fixture text is valid")),
                Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
            ])),
        )
    }
}

/// Reads one daemon line as its decoded typed envelope.
#[cfg(feature = "test-support")]
async fn receive_message(messages: &mut AsyncMessageReceiver) -> ProtocolDaemonMessageDto {
    let line = messages.receive_line().await.expect("host message arrives");
    serde_json::from_str(&line).expect("host message decodes")
}

#[cfg(feature = "test-support")]
async fn send_request_through_host(
    endpoint: &LocalEndpoint,
    request_id: u64,
    request: ClientRequestDto,
) -> ProtocolResultDto {
    use intention_transport::AsyncLocalClientConnection;

    let connection = AsyncLocalClientConnection::connect(endpoint)
        .await
        .expect("host client connects");
    let (mut sender, mut receiver) = connection.split();
    sender
        .send_message(&encode_request(request_id, request))
        .await
        .expect("host request sends");
    let line = receiver.receive_line().await.expect("host reply arrives");
    decode_response(&line, request_id).expect("host reply decodes")
}

/// Reads one run's current snapshot from the host over the protocol.
///
/// This is the public current-state read: the snapshot carries the committed
/// run projection and the run's transcript rows, so a fixture needs no
/// daemon-internal read path to observe a run that has left the active slot.
#[cfg(feature = "test-support")]
async fn run_snapshot_through_host(
    endpoint: &LocalEndpoint,
    session_id: SessionId,
    run_id: RunId,
) -> RunSubscriptionSnapshotDto {
    let response = send_request_through_host(
        endpoint,
        1,
        ClientRequestDto::SubscribeRun(SubscribeRunCommandDto::new(session_id, run_id)),
    )
    .await;
    let ProtocolResultDto::RunSubscribed(snapshot) = response else {
        panic!("the host answers a run subscription with the current snapshot")
    };
    snapshot
}

#[cfg(feature = "test-support")]
async fn send_user_turn_through_host(endpoint: &LocalEndpoint, session_id: SessionId) -> RunId {
    let response = send_request_through_host(
        endpoint,
        1,
        ClientRequestDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, IdempotencyKey::new(), "host turn")
                .expect("turn is valid"),
        ),
    )
    .await;
    let ProtocolResultDto::TurnAccepted(turn) = response else {
        panic!("host accepts the turn")
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
        1,
        ClientRequestDto::InterruptRun(InterruptRunCommandDto::new(session_id, run_id)),
    )
    .await;
    assert!(matches!(response, ProtocolResultDto::RunInterrupted(_)));
}

#[cfg(feature = "test-support")]
async fn send_pending_turn_through_host(endpoint: &LocalEndpoint, session_id: SessionId) -> TurnId {
    let response = send_request_through_host(
        endpoint,
        1,
        ClientRequestDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, IdempotencyKey::new(), "pending host turn")
                .expect("pending turn is valid"),
        ),
    )
    .await;
    let ProtocolResultDto::TurnAccepted(turn) = response else {
        panic!("host accepts the pending turn")
    };
    assert!(
        matches!(turn.outcome(), SendUserTurnOutcomeDto::Pending),
        "the host answers the pending turn with its durable identity"
    );
    turn.turn_id()
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
        host_server.serve_connections(listener, 3).await;
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
    // The interrupt travels through the host's blocking dispatch and a durable
    // validation read, so under load it can land after the continuation round
    // already started: the invariant is that the run continues (never a
    // terminal cancellation) and that the signal was cleared exactly once, not
    // the wall-clock number of rounds that began.
    assert!(
        driver.executions() >= 2,
        "the interrupted run starts its continuation round"
    );
    let continuing = facade
        .session_snapshot(session_id)
        .expect("continuing run state reads");
    assert_eq!(
        continuing
            .projection()
            .active_run()
            .expect("the interrupted run stays active")
            .status(),
        RunStatusDto::Running
    );
    assert_eq!(
        continuing
            .messages()
            .iter()
            .filter(|message| {
                message.kind() == MessageKindDto::Notice && message.text() == INTERRUPT_NOTICE
            })
            .count(),
        1,
        "the run's signal is cleared once, so the interruption records exactly one notice"
    );
    let requests = driver.requests();
    assert_eq!(requests[1].run_id(), run_id);
    assert!(
        requests[1]
            .messages()
            .iter()
            .any(|message| message.role() == ModelRoleDto::Notice),
        "the continuation carries the interruption notice"
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
    // A run that left the active slot is read from the host's current-state
    // subscription snapshot: its terminal status and transcript rows are the
    // committed values the daemon serves.
    let completed = run_snapshot_through_host(&endpoint, session_id, run_id).await;
    assert_eq!(completed.run().status(), RunStatusDto::Completed);
    assert!(completed.messages().iter().any(|message| {
        message.kind() == MessageKindDto::Assistant && message.text() == "live response"
    }));
    server
        .await
        .expect("host accepts command and interrupt peers");
    host.shutdown().await;
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn a_stopped_answer_is_durable_with_its_marker_after_a_graceful_shutdown() {
    // An environment stop takes the same path as a user interrupt: every
    // registered run is cancelled and drained, so the answer the model had
    // produced is durable, with its marker, before the host returns.
    let driver = Arc::new(PartialAnswerDriver::new());
    let (_directory, facade, _snapshot) = fixture_facade("graceful-shutdown", driver.clone());
    let session_id = SessionId::new();
    create_session(&facade, session_id, &std::env::temp_dir());
    let endpoint =
        LocalEndpoint::from_instance_id(format!("m4-graceful-shutdown-{}", RunId::new()))
            .expect("fixture endpoint is valid");
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
    let host = intention_daemon::test_host_lifecycle(facade.clone());
    let host_server = host.clone();
    let server = tokio::spawn(async move {
        host_server.serve_connections(listener, 1).await;
    });
    let run_id = send_user_turn_through_host(&endpoint, session_id).await;
    tokio::time::timeout(Duration::from_secs(1), driver.reached.notified())
        .await
        .expect("the driver delivers its partial answer before the stop");

    host.shutdown_gracefully().await;

    let snapshot = facade
        .session_snapshot(session_id)
        .expect("durable state reads");
    assert!(
        snapshot.messages().iter().any(|message| {
            message.run_id() == Some(run_id)
                && message.kind() == MessageKindDto::Assistant
                && message.text().contains("partial answer")
                && message.text().contains(INTERRUPTED_MARKER)
        }),
        "the stopped answer is durable with its marker"
    );
    assert!(
        snapshot.messages().iter().any(|message| {
            message.run_id() == Some(run_id)
                && message.kind() == MessageKindDto::Notice
                && message.text() == INTERRUPT_NOTICE
        }),
        "the stopped answer is followed by its durable notice"
    );
    assert_eq!(
        snapshot.projection().active_run().map(|run| run.status()),
        None,
        "the drained run reached its terminal state"
    );
    server.await.expect("host serves the stopped run's peer");
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
        host_server.serve_connections(listener, 3).await;
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
        .session_snapshot(session_id)
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
        .session_snapshot(session_id)
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
    let continuing = facade
        .session_snapshot(session_id)
        .expect("continuing run state reads");
    assert_eq!(
        continuing
            .projection()
            .active_run()
            .expect("the run stays active while the joined turn streams")
            .status(),
        RunStatusDto::Running
    );
    assert!(
        continuing.messages().iter().any(|message| {
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
    let completed = run_snapshot_through_host(&endpoint, session_id, run_id).await;
    assert_eq!(
        completed.run().status(),
        RunStatusDto::Completed,
        "one run carries the whole continuous session"
    );
    assert_eq!(driver.executions(), 2);
    server.await.expect("host accepts first and pending peers");
    host.shutdown().await;
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
    let recovered = restarted
        .session_snapshot(session_id)
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
    assert_eq!(restart_driver.executions(), 0);

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
    let restart_host = intention_daemon::test_host_lifecycle(restarted.clone());
    let restart_host_server = restart_host.clone();
    let restart_server = tokio::spawn(async move {
        restart_host_server
            .serve_connections(restart_listener, 1)
            .await;
    });
    use intention_transport::AsyncLocalClientConnection;
    let connection = AsyncLocalClientConnection::connect(&restart_endpoint)
        .await
        .expect("restart stream client connects");
    let (mut requests, mut messages) = connection.split();
    requests
        .send_message(&encode_request(
            1,
            ClientRequestDto::SubscribeRun(SubscribeRunCommandDto::new(session_id, first_run)),
        ))
        .await
        .expect("restart snapshot request sends");
    let initial_line = messages
        .receive_line()
        .await
        .expect("restart initial response arrives");
    let initial_frame =
        decode_response(&initial_line, 1).expect("restart initial response decodes");
    requests
        .send_message(&encode_request(
            2,
            ClientRequestDto::SubscribeRun(SubscribeRunCommandDto::new(session_id, RunId::new())),
        ))
        .await
        .expect("unknown-run request sends");
    let error_line = messages
        .receive_line()
        .await
        .expect("restart error response arrives");
    let transport_error =
        decode_response(&error_line, 2).expect_err("an unknown run is refused with a typed error");
    let initial_frame_json =
        serde_json::to_string(&initial_frame).expect("initial frame serializes");
    let transport_error_json = serde_json::to_string(&transport_error).expect("error serializes");
    let ProtocolResultDto::RunSubscribed(snapshot) = &initial_frame else {
        panic!("restart subscription answers with the current snapshot")
    };
    assert_eq!(
        snapshot.run().status(),
        RunStatusDto::Interrupted,
        "the restarted daemon serves the interrupted run's current state"
    );
    assert!(
        !snapshot
            .messages()
            .iter()
            .any(|message| message.text() == "pending host turn"),
        "a pending message is durable input, never part of the interrupted run"
    );
    assert_eq!(transport_error.code(), "storage_record_not_found");
    restart_server
        .await
        .expect("restart host accepts stream peer");
    restart_host.shutdown().await;
    let projection_json = serde_json::to_string(snapshot.run()).expect("projection serializes");
    let transcript_json =
        serde_json::to_string(snapshot.messages()).expect("transcript rows serialize");
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
async fn streamed_provider_text_reaches_a_subscriber_before_its_committed_row_and_stays_transient()
{
    let release = Arc::new(tokio::sync::Notify::new());
    let driver = Arc::new(GatedChunksDriver {
        release: Arc::clone(&release),
    });
    let (_directory, facade, _snapshot) = fixture_facade("text-delta", driver);
    let session_id = SessionId::new();
    create_session(&facade, session_id, &std::env::temp_dir());
    let endpoint = LocalEndpoint::from_instance_id(format!("m4-text-delta-{}", RunId::new()))
        .expect("fixture endpoint is valid");
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
    let host = intention_daemon::test_host_lifecycle(facade.clone());
    let host_server = host.clone();
    let server = tokio::spawn(async move {
        host_server.serve_connections(listener, 3).await;
    });

    let run_id = send_user_turn_through_host(&endpoint, session_id).await;
    // The subscription is registered before the provider streams its first
    // chunk, so every delta of the run reaches a live subscriber.
    let connection = intention_transport::AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("stream subscriber connects");
    let (mut subscriber_requests, mut frames) = connection.split();
    subscriber_requests
        .send_message(&encode_request(
            1,
            ClientRequestDto::SubscribeRun(SubscribeRunCommandDto::new(session_id, run_id)),
        ))
        .await
        .expect("subscription request sends");
    let reply = frames
        .receive_line()
        .await
        .expect("subscription reply arrives");
    assert!(
        matches!(
            decode_response(&reply, 1).expect("subscription reply decodes"),
            ProtocolResultDto::RunSubscribed(_)
        ),
        "a live subscription is what transient frames are published to"
    );

    release.notify_one();

    // The stream ends with the committed assistant row and the terminal status,
    // so every frame the run published was received in its publication order.
    let mut received: Vec<RunStreamFrameDto> = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let ProtocolDaemonMessageDto::Frame(frame) = receive_message(&mut frames).await else {
                continue;
            };
            let terminal = matches!(
                &frame,
                RunStreamFrameDto::Status(run) if run_status_is_terminal(run.status())
            );
            received.push(frame);
            if terminal {
                break;
            }
        }
    })
    .await
    .expect("the streamed run reaches its terminal frame");

    let published: Vec<(u32, String)> = received
        .iter()
        .filter_map(|frame| match frame {
            RunStreamFrameDto::TextDelta(delta) => {
                assert_eq!(delta.session_id(), session_id);
                assert_eq!(delta.run_id(), run_id);
                Some((delta.step(), delta.text().to_owned()))
            }
            _ => None,
        })
        .collect();
    assert!(
        !published.is_empty(),
        "the subscriber receives the provider's streamed text"
    );
    assert!(
        published.iter().all(|(step, _)| *step == 0),
        "one provider round is the run's first model step"
    );
    let (committed_index, committed_text) = received
        .iter()
        .enumerate()
        .find_map(|(index, frame)| match frame {
            RunStreamFrameDto::Content(message) if message.kind() == MessageKindDto::Assistant => {
                Some((index, message.text().to_owned()))
            }
            _ => None,
        })
        .expect("the run commits its assistant row");
    let newest_delta = received
        .iter()
        .rposition(|frame| matches!(frame, RunStreamFrameDto::TextDelta(_)))
        .expect("every advertised delta is a received frame");
    assert!(
        newest_delta < committed_index,
        "the committed row of the step is published behind the provisional text it supersedes"
    );
    let provisional: String = published
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .concat();
    assert_eq!(
        provisional, committed_text,
        "the provisional text of the step is the text its committed row carries"
    );
    assert_eq!(committed_text, GATED_CHUNKS.concat());

    // A bounded read window past the terminal frame proves the transient path
    // publishes nothing behind the committed row: the coalescing cadence keeps
    // ticking for the run's last step, and its window is already drained.
    let mut trailing: Vec<RunStreamFrameDto> = Vec::new();
    let _late = tokio::time::timeout(Duration::from_millis(150), async {
        loop {
            if let ProtocolDaemonMessageDto::Frame(frame) = receive_message(&mut frames).await {
                trailing.push(frame);
            }
        }
    })
    .await;
    assert!(
        !trailing
            .iter()
            .any(|frame| matches!(frame, RunStreamFrameDto::TextDelta(_))),
        "no provisional text follows the committed row it belongs to"
    );

    // Transient means never durable: the current-state snapshot the host serves
    // carries the committed row alone, with no provisional fragment anywhere.
    let snapshot = run_snapshot_through_host(&endpoint, session_id, run_id).await;
    assert_eq!(snapshot.run().status(), RunStatusDto::Completed);
    assert_eq!(
        snapshot
            .messages()
            .iter()
            .filter(|message| message.kind() == MessageKindDto::Assistant)
            .map(|message| message.text().to_owned())
            .collect::<Vec<_>>(),
        vec![committed_text],
        "the committed row carries the whole step and no provisional fragment survives"
    );
    let snapshot_json = serde_json::to_string(&snapshot).expect("the current snapshot serializes");
    assert!(
        !snapshot_json.contains("text_delta"),
        "a current-state snapshot carries no transient frame"
    );

    server.await.expect("host accepts every fixture peer");
    host.shutdown().await;
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn both_channels_of_a_step_reach_a_subscriber_before_the_row_they_become() {
    let release = Arc::new(tokio::sync::Notify::new());
    let driver = Arc::new(GatedChannelDriver {
        release: Arc::clone(&release),
    });
    let (_directory, facade, _snapshot) = fixture_facade("two-channels", driver);
    let session_id = SessionId::new();
    create_session(&facade, session_id, &std::env::temp_dir());
    let endpoint = LocalEndpoint::from_instance_id(format!("m4-two-channels-{}", RunId::new()))
        .expect("fixture endpoint is valid");
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
    let host = intention_daemon::test_host_lifecycle(facade.clone());
    let host_server = host.clone();
    let server = tokio::spawn(async move {
        host_server.serve_connections(listener, 3).await;
    });

    let run_id = send_user_turn_through_host(&endpoint, session_id).await;
    let connection = intention_transport::AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("stream subscriber connects");
    let (mut subscriber_requests, mut frames) = connection.split();
    subscriber_requests
        .send_message(&encode_request(
            1,
            ClientRequestDto::SubscribeRun(SubscribeRunCommandDto::new(session_id, run_id)),
        ))
        .await
        .expect("subscription request sends");
    let reply = frames
        .receive_line()
        .await
        .expect("subscription reply arrives");
    assert!(
        matches!(
            decode_response(&reply, 1).expect("subscription reply decodes"),
            ProtocolResultDto::RunSubscribed(_)
        ),
        "a live subscription is what transient frames are published to"
    );

    release.notify_one();

    let mut received: Vec<RunStreamFrameDto> = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let ProtocolDaemonMessageDto::Frame(frame) = receive_message(&mut frames).await else {
                continue;
            };
            let terminal = matches!(
                &frame,
                RunStreamFrameDto::Status(run) if run_status_is_terminal(run.status())
            );
            received.push(frame);
            if terminal {
                break;
            }
        }
    })
    .await
    .expect("the streamed run reaches its terminal frame");

    let channels: Vec<(TextDeltaChannelDto, String)> = received
        .iter()
        .filter_map(|frame| match frame {
            RunStreamFrameDto::TextDelta(delta) => {
                assert_eq!(delta.session_id(), session_id);
                assert_eq!(delta.run_id(), run_id);
                assert_eq!(delta.step(), 0, "one provider round is one model step");
                Some((delta.channel(), delta.text().to_owned()))
            }
            _ => None,
        })
        .collect();
    let joined = |channel: TextDeltaChannelDto| {
        channels
            .iter()
            .filter(|(published, _)| *published == channel)
            .map(|(_, text)| text.as_str())
            .collect::<String>()
    };
    assert_eq!(
        joined(TextDeltaChannelDto::Reasoning),
        GATED_REASONING_CHUNKS.concat(),
        "the reasoning channel is coalesced in the order the provider streamed it"
    );
    assert_eq!(
        joined(TextDeltaChannelDto::Answer),
        GATED_ANSWER_CHUNKS.concat(),
        "the answer channel is never reordered by the reasoning beside it"
    );

    let (committed_index, committed) = received
        .iter()
        .enumerate()
        .find_map(|(index, frame)| match frame {
            RunStreamFrameDto::Content(message) if message.kind() == MessageKindDto::Assistant => {
                Some((index, message.clone()))
            }
            _ => None,
        })
        .expect("the run commits its assistant row");
    let newest_delta = received
        .iter()
        .rposition(|frame| matches!(frame, RunStreamFrameDto::TextDelta(_)))
        .expect("every advertised delta is a received frame");
    assert!(
        newest_delta < committed_index,
        "both channels flush before the committed row that supersedes them"
    );
    assert_eq!(
        committed.reasoning(),
        Some(GATED_REASONING_CHUNKS.concat().as_str()),
        "the committed row carries the reasoning the live segment streamed"
    );
    assert_eq!(
        committed.text(),
        GATED_ANSWER_CHUNKS.concat(),
        "the committed row carries the answer the live segment streamed"
    );
    assert!(
        channels
            .iter()
            .position(|(channel, _)| *channel == TextDeltaChannelDto::Answer)
            > channels
                .iter()
                .rposition(|(channel, _)| *channel == TextDeltaChannelDto::Reasoning),
        "the reasoning channel is published before the answer it precedes"
    );

    // Transient means never durable: the current-state snapshot carries the
    // committed row alone, with no provisional fragment of either channel.
    let snapshot = run_snapshot_through_host(&endpoint, session_id, run_id).await;
    let snapshot_json = serde_json::to_string(&snapshot).expect("the current snapshot serializes");
    assert!(
        !snapshot_json.contains("text_delta"),
        "a current-state snapshot carries no transient frame"
    );

    server.await.expect("host accepts every fixture peer");
    host.shutdown().await;
}

/// The session count one daemon session-list reply is read over.
///
/// The daemon's list window is the composition root's `SESSION_LIST_ROWS`, so
/// the fixture creates one session beyond it and observes the explicit omitted
/// count instead of a silently truncated list.
#[cfg(feature = "test-support")]
const SESSIONS_OVER_THE_LIST_WINDOW: usize = 257;

#[cfg(feature = "test-support")]
#[tokio::test]
async fn listing_sessions_answers_with_the_ordered_window_and_its_omitted_count() {
    let driver = Arc::new(ScriptedDriver::completed_text());
    let (_directory, facade, _snapshot) = fixture_facade("session-list", driver);
    let endpoint = LocalEndpoint::from_instance_id(format!("m4-session-list-{}", RunId::new()))
        .expect("fixture endpoint is valid");
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
    let host = intention_daemon::test_host_lifecycle(facade.clone());
    let host_server = host.clone();
    let server = tokio::spawn(async move {
        host_server.serve_connections(listener, 1).await;
    });

    let mut created = Vec::new();
    for index in 0..SESSIONS_OVER_THE_LIST_WINDOW {
        // A durable session binds one workspace identity, and the workspace
        // root is what it binds that identity to.
        let workspace = std::env::temp_dir().join(format!("m4-session-list-{index}"));
        let session_id = SessionId::new();
        create_session(&facade, session_id, &workspace);
        created.push(session_id);
    }

    let response = send_request_through_host(&endpoint, 1, ClientRequestDto::ListSessions).await;
    let ProtocolResultDto::SessionsListed(summaries) = response else {
        panic!("the host answers a session list request with its bounded list")
    };

    assert_eq!(
        summaries.sessions().len(),
        SESSIONS_OVER_THE_LIST_WINDOW - 1,
        "the host answers with its full retained window"
    );
    assert_eq!(
        summaries.omitted(),
        1,
        "the session beyond the window is reported instead of dropped"
    );
    for summary in summaries.sessions() {
        assert_eq!(summary.mode(), RunModeDto::Build);
        assert!(
            summary.active_run().is_none(),
            "a session without a run reports no active run"
        );
    }
    // The window is newest-first with the durable identity breaking an update
    // time tie, and it is a coherent window of distinct durable sessions.
    for pair in summaries.sessions().windows(2) {
        let newer = &pair[0];
        let older = &pair[1];
        assert!(
            newer.updated_at() > older.updated_at()
                || (newer.updated_at() == older.updated_at()
                    && newer.session_id() < older.session_id()),
            "each summary follows the newest-first identity-ordered contract"
        );
    }
    let listed: Vec<SessionId> = summaries
        .sessions()
        .iter()
        .map(|summary| summary.session_id())
        .collect();
    let mut distinct = listed.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        listed.len(),
        "every listed session is a distinct durable session"
    );
    let beyond = created
        .iter()
        .copied()
        .filter(|session_id| !listed.contains(session_id))
        .count();
    assert_eq!(
        beyond, 1,
        "exactly the reported omitted session is kept out of the window"
    );

    server.await.expect("host accepts the list peer");
    host.shutdown().await;
}
