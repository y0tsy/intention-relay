//! Client-driven daemon-host end-to-end tests over real IPC.
//!
//! These tests spawn the real `intention-daemon` binary over real local
//! transport, drive it with the real asynchronous client, and execute a real
//! `read` tool through the production model-tool loop against a fake
//! OpenAI-compatible provider. They prove the committed transcript rows, the
//! run status, and live session-frame publication, restart the daemon, and prove
//! the same durable run is replayed as current state without re-executing the
//! tool.

use std::process::Child;
use std::time::{Duration, Instant};

use intention_client::{SessionStreamClient, SessionStreamSubscription, StreamStep};
use intention_proto::{
    CreateSessionCommandDto, CreateSessionFirstTurnDto, MessageKindDto, MessageProjectionDto,
    RunModeDto, RunStatusDto, SendUserTurnOutcomeDto, SessionFrameDto, SubscribeSessionCommandDto,
    ToolIdentityDto, WorkspaceRootDto, run_status_is_terminal,
};
use intention_proto::{ProjectId, SessionId, WorkspaceId};
use intention_test_support::daemon_process::{
    self, spawn_daemon, unique_endpoint, wait_until_ready, write_config_document,
};
use intention_test_support::fake_provider::{FakeProvider, fixture_config_document};
use intention_transport::LocalEndpoint;
use tempfile::TempDir;

/// One daemon-host fixture: isolated config/state/workspace, a fake provider,
/// one spawned daemon process, and its private endpoint.
///
/// Dropping the fixture kills the daemon, stops the provider, and removes the
/// daemon-owned Unix socket so a later run can bind the same logical endpoint.
struct E2eHost {
    config_home: TempDir,
    state_home: TempDir,
    workspace: TempDir,
    credential: String,
    provider: FakeProvider,
    daemon: Option<Child>,
    endpoint: LocalEndpoint,
}

impl E2eHost {
    /// Creates a fresh isolated fixture and spawns its first daemon process.
    fn new(workspace_file: Option<(&str, &str)>, tool_arguments: &str) -> Self {
        Self::with_tool_round("read", workspace_file, tool_arguments, &[])
    }

    /// Creates a fresh isolated fixture whose tool round streams `reasoning`
    /// before the tool call it emits.
    fn with_tool_round_reasoning(
        workspace_file: Option<(&str, &str)>,
        tool_arguments: &str,
        reasoning: &[&str],
    ) -> Self {
        Self::with_tool_round("read", workspace_file, tool_arguments, reasoning)
    }

    /// Creates a fresh isolated fixture whose tool round calls `tool_name` with
    /// `tool_arguments` and streams `reasoning` before that call.
    fn with_tool_round(
        tool_name: &str,
        workspace_file: Option<(&str, &str)>,
        tool_arguments: &str,
        reasoning: &[&str],
    ) -> Self {
        let config_home = TempDir::new().expect("config directory exists");
        let state_home = TempDir::new().expect("state directory exists");
        let workspace = TempDir::new().expect("workspace directory exists");
        if let Some((name, content)) = workspace_file {
            std::fs::write(workspace.path().join(name), content).expect("workspace fixture writes");
        }
        let credential = format!("fixture-credential-{}", std::process::id());
        let provider = FakeProvider::start_tool_round(tool_name, tool_arguments, reasoning);
        let config_document = fixture_config_document(provider.port(), &credential);
        write_config_document(config_home.path(), &config_document);
        let endpoint = unique_endpoint("e2e");
        let daemon = spawn_daemon(&endpoint, config_home.path(), state_home.path(), None, &[]);
        Self {
            config_home,
            state_home,
            workspace,
            credential,
            provider,
            daemon: Some(daemon),
            endpoint,
        }
    }

    /// Kills the current daemon and starts a fresh process with identical
    /// environment, state directories, and endpoint.
    ///
    /// The kill is a hard kill: the daemon cannot run its listener Drop, so
    /// its Unix socket file survives. The transport reclaims the stale socket
    /// on the next bind (PR24-010); the fixture no longer deletes it by hand.
    fn restart_daemon(&mut self) {
        self.kill_daemon();
        self.daemon = Some(spawn_daemon(
            &self.endpoint,
            self.config_home.path(),
            self.state_home.path(),
            None,
            &[],
        ));
    }

    fn kill_daemon(&mut self) {
        daemon_process::kill(&mut self.daemon);
    }
}

impl Drop for E2eHost {
    fn drop(&mut self) {
        self.kill_daemon();
        self.provider.stop();
        #[cfg(unix)]
        if let Some(path) = daemon_process::endpoint_socket_path(&self.endpoint) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// One live run observation: the subscription, the correlated current-state
/// snapshot it carried, and every later frame the subscriber received.
struct LiveRunObservation {
    subscription: SessionStreamSubscription,
    snapshot_status: Option<RunStatusDto>,
    snapshot_messages: Vec<MessageProjectionDto>,
    frames: Vec<SessionFrameDto>,
}

impl LiveRunObservation {
    /// Returns whether the correlated snapshot already reported a terminal run.
    fn snapshot_is_terminal(&self) -> bool {
        self.snapshot_status.is_some_and(run_status_is_terminal)
    }

    /// Returns whether any received status frame reported a committed status.
    fn received_status_frame(&self) -> bool {
        self.frames
            .iter()
            .any(|frame| matches!(frame, SessionFrameDto::Status(_)))
    }
}

/// Subscribes to one session's current state and drives the live stream until
/// its active run reports a terminal status.
///
/// The correlated first reply is the current-state session snapshot; every
/// later committed row, run status change, and transient delta arrives as a
/// live session frame on the same subscription. A re-subscribing client
/// re-reads current state and continues live, so both the snapshot and the
/// frames are authoritative.
async fn observe_run_until_terminal(
    client: &SessionStreamClient,
    session_id: SessionId,
    deadline: Instant,
) -> LiveRunObservation {
    let mut subscription = client
        .subscribe(SubscribeSessionCommandDto::new(session_id))
        .await
        .expect("session subscription arrives");
    let snapshot_status = subscription.state().status();
    let snapshot_messages = subscription.state().messages().to_vec();
    let mut frames = Vec::new();
    loop {
        if subscription
            .state()
            .status()
            .is_some_and(run_status_is_terminal)
        {
            return LiveRunObservation {
                subscription,
                snapshot_status,
                snapshot_messages,
                frames,
            };
        }
        assert!(
            Instant::now() < deadline,
            "the run reaches a terminal status before the deadline"
        );
        match tokio::time::timeout(Duration::from_secs(1), subscription.receive()).await {
            Ok(Ok(StreamStep::Frame { frame, .. })) => frames.push(frame),
            Ok(Ok(StreamStep::Closed(_))) => {
                panic!("the daemon closed the run stream before the run terminalized")
            }
            Ok(Err(error)) => panic!("run stream frame error: {}", error.code()),
            Err(_) => {}
        }
    }
}

/// Returns whether one transcript slice holds the committed assistant step
/// whose text the fixture provider streamed.
fn has_assistant_step(messages: &[MessageProjectionDto]) -> bool {
    messages
        .iter()
        .any(|message| message.kind() == MessageKindDto::Assistant && message.text() == "done")
}

#[tokio::test]
async fn real_daemon_tool_loop_executes_read_and_replays_after_restart() {
    let mut host = E2eHost::new(
        Some(("hello.txt", "hello from e2e")),
        r#"{"path":"hello.txt"}"#,
    );
    let client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20)).await;
    let workspace_root = host.workspace.path().to_string_lossy().into_owned();

    let session_id = SessionId::new();
    client
        .create_session(CreateSessionCommandDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse(workspace_root).expect("workspace root is absolute"),
            Some(RunModeDto::Build),
            None,
        ))
        .await
        .expect("the daemon accepts session creation");

    let outcome = client
        .send_user_turn(session_id, "Read hello.txt".to_owned())
        .await
        .expect("user turn is accepted");
    let SendUserTurnOutcomeDto::Started { run_id, .. } = outcome else {
        panic!("first turn starts a run, got: {outcome:?}")
    };

    let stream_client = SessionStreamClient::new(host.endpoint.clone());
    let live_deadline = Instant::now() + Duration::from_secs(30);
    // Subscribe immediately after acceptance: the subscription snapshot is the
    // session's current state, and every later commit arrives as a live frame.
    let observation = observe_run_until_terminal(&stream_client, session_id, live_deadline).await;
    let state = observation.subscription.state();
    assert_eq!(
        state.status(),
        Some(RunStatusDto::Completed),
        "the real daemon completes the tool round"
    );
    assert_eq!(
        state
            .run()
            .expect("the subscription carries the run scope")
            .run_id(),
        run_id
    );
    // A non-terminal snapshot can only reach the terminal status through a
    // live status frame; the committed completion is published, never polled.
    if !observation.snapshot_is_terminal() {
        assert!(
            observation.received_status_frame(),
            "a live status frame carries the committed completion to the subscriber"
        );
    }
    // The committed assistant step is published live or carried by the
    // snapshot, so the subscriber always observes it exactly once.
    if !has_assistant_step(&observation.snapshot_messages) {
        assert!(
            observation.frames.iter().any(|frame| matches!(
                frame,
                SessionFrameDto::Content(message)
                    if message.kind() == MessageKindDto::Assistant && message.text() == "done"
            )),
            "the committed assistant content is published as a live content frame"
        );
    }
    // The committed tool round is current state for the subscriber: the tool
    // call and its one result row are carried by the snapshot or published as
    // live content frames after their own commits.
    let subscriber_messages = state.messages();
    assert!(
        subscriber_messages
            .iter()
            .any(|message| message.kind() == MessageKindDto::ToolCall),
        "the subscriber's transcript holds the committed tool call"
    );
    assert!(
        subscriber_messages
            .iter()
            .any(|message| message.kind() == MessageKindDto::ToolResult),
        "the subscriber's transcript holds the committed tool result"
    );
    if !observation
        .snapshot_messages
        .iter()
        .any(|message| message.kind() == MessageKindDto::ToolResult)
    {
        assert!(
            observation.frames.iter().any(|frame| matches!(
                frame,
                SessionFrameDto::Content(message)
                    if message.kind() == MessageKindDto::ToolResult
            )),
            "the committed tool result is published as a live content frame"
        );
    }
    assert!(
        has_assistant_step(state.messages()),
        "the subscriber's current transcript holds the committed assistant step"
    );

    // The session snapshot is the current-state read of the committed
    // transcript: the accepted turn, the tool call, its one result row, and the
    // assistant step, in durable insertion order.
    let session_snapshot = client
        .session_snapshot(session_id)
        .await
        .expect("session snapshot reads");
    let messages = session_snapshot.messages();
    assert_eq!(
        messages
            .iter()
            .map(|message| message.kind())
            .collect::<Vec<_>>(),
        vec![
            MessageKindDto::User,
            MessageKindDto::ToolCall,
            MessageKindDto::ToolResult,
            MessageKindDto::Assistant,
        ],
        "the committed transcript records the turn, the read call, its result, and the assistant step"
    );
    assert_eq!(messages[0].text(), "Read hello.txt");
    assert_eq!(
        messages[1].tool_id().map(ToolIdentityDto::wire_name),
        Some("read")
    );
    assert_eq!(
        messages[1].text(),
        r#"{"path":"hello.txt"}"#,
        "the durable tool call keeps the relative workspace path"
    );
    let call_id = messages[1]
        .tool_call_id()
        .expect("the tool call row carries its identity");
    assert_eq!(
        messages[2].tool_id().map(ToolIdentityDto::wire_name),
        Some("read")
    );
    assert_eq!(messages[2].tool_call_id(), Some(call_id));
    assert_eq!(
        messages[2].text(),
        "hello from e2e",
        "the tool result row carries the exact file content"
    );
    assert_eq!(messages[3].text(), "done");
    assert_eq!(
        host.provider.request_count(),
        2,
        "the provider serves exactly the tool round and its follow-up round"
    );
    assert_eq!(
        host.provider.excess_count(),
        0,
        "no further provider request follows completion"
    );

    // Public payloads never disclose the credential; the durable transcript
    // never discloses the absolute workspace path.
    let transcript_json = serde_json::to_string(messages).expect("transcript serializes");
    let session_json =
        serde_json::to_string(&session_snapshot).expect("session snapshot serializes");
    assert!(
        !transcript_json.contains(&host.credential),
        "the durable transcript never discloses the provider credential"
    );
    assert!(
        !transcript_json.contains(&host.workspace.path().to_string_lossy().into_owned()),
        "the durable transcript never discloses the absolute workspace path"
    );
    assert!(
        !session_json.contains(&host.credential),
        "the session snapshot never discloses the provider credential"
    );

    // Restart the daemon against the identical environment and state, then
    // prove the completed run's committed transcript is replayed as current
    // state without any provider re-execution. A completed run is terminal, so
    // the session carries no active run and there is no live frame left to
    // wait for: the rows are the state a fresh read returns.
    host.restart_daemon();
    let client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20)).await;
    let replayed = client
        .session_snapshot(session_id)
        .await
        .expect("replayed session snapshot reads");
    assert!(
        replayed.projection().active_run().is_none(),
        "the restarted daemon serves the completed run as terminal, so the session carries no active run"
    );
    let replayed_messages = replayed.messages();
    assert_eq!(
        replayed_messages.len(),
        4,
        "the replayed session snapshot carries the complete committed transcript"
    );
    assert_eq!(
        replayed_messages[2].text(),
        "hello from e2e",
        "the durable tool result replays unchanged"
    );
    assert_eq!(
        replayed
            .messages()
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        session_snapshot
            .messages()
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        "the durable transcript replays unchanged after the daemon restart"
    );

    // Allow any late provider traffic to land before asserting the tool was
    // never re-executed after the restart.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        host.provider.request_count(),
        2,
        "the restarted daemon replays the durable run without re-executing the tool"
    );
    assert_eq!(host.provider.excess_count(), 0);
}

#[tokio::test]
async fn a_create_with_turn_command_commits_session_then_turn() {
    let host = E2eHost::new(
        Some(("hello.txt", "hello from e2e")),
        r#"{"path":"hello.txt"}"#,
    );
    let client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20)).await;

    // One command creates the session and commits the prompt it carries as that
    // session's first turn: the client declares the session identity and the
    // daemon accepts the turn against the session it just created.
    let session_id = SessionId::new();
    let created = client
        .create_session(CreateSessionCommandDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse(host.workspace.path().to_string_lossy().into_owned())
                .expect("workspace root is absolute"),
            Some(RunModeDto::Build),
            Some(
                CreateSessionFirstTurnDto::new("Read hello.txt")
                    .expect("fixture first turn is valid"),
            ),
        ))
        .await
        .expect("the daemon accepts the combined creation");
    assert_eq!(
        created.session_id(),
        session_id,
        "the command's declared session identity is authoritative"
    );
    let accepted = created
        .first_turn()
        .expect("the acceptance carries the committed first turn");
    assert_eq!(accepted.session_id(), session_id);
    assert!(
        matches!(accepted.outcome(), SendUserTurnOutcomeDto::Started { .. }),
        "the created session's first turn starts its run"
    );

    // The prompt is durable in the session the one command created, so a fresh
    // read names both the session and its committed first row.
    let snapshot = client
        .session_snapshot(session_id)
        .await
        .expect("the created session reads back");
    assert!(
        snapshot.messages().iter().any(|message| {
            message.kind() == MessageKindDto::User && message.text() == "Read hello.txt"
        }),
        "the created session holds the prompt as its committed first turn"
    );
}

#[tokio::test]
async fn real_daemon_tool_loop_answers_a_failed_call_without_stopping() {
    let host = E2eHost::new(None, r#"{"path":"missing.txt"}"#);
    let client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20)).await;

    let session_id = SessionId::new();
    client
        .create_session(CreateSessionCommandDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse(host.workspace.path().to_string_lossy().into_owned())
                .expect("workspace root is absolute"),
            Some(RunModeDto::Build),
            None,
        ))
        .await
        .expect("the daemon accepts session creation");

    let outcome = client
        .send_user_turn(session_id, "Read missing.txt".to_owned())
        .await
        .expect("user turn is accepted");
    let SendUserTurnOutcomeDto::Started { .. } = outcome else {
        panic!("first turn starts a run, got: {outcome:?}")
    };

    let stream_client = SessionStreamClient::new(host.endpoint.clone());
    let deadline = Instant::now() + Duration::from_secs(30);
    let observation = observe_run_until_terminal(&stream_client, session_id, deadline).await;
    assert_eq!(
        observation.subscription.state().status(),
        Some(RunStatusDto::Completed),
        "the real daemon answers the failed tool call and continues the loop"
    );
    if !observation.snapshot_is_terminal() {
        assert!(
            observation.received_status_frame(),
            "a live status frame carries the committed completion to the subscriber"
        );
    }
    assert_eq!(
        host.provider.request_count(),
        2,
        "the failed call is answered with a tool message and the loop continues once"
    );
    assert_eq!(host.provider.excess_count(), 0);

    // The completed run's transcript carries the model's call row, the
    // guidance-bearing failure result committed for that exact invocation, and
    // the answer the loop produced after reading both.
    let session_snapshot = client
        .session_snapshot(session_id)
        .await
        .expect("session snapshot reads");
    let messages = session_snapshot.messages();
    assert_eq!(
        messages
            .iter()
            .map(|message| message.kind())
            .collect::<Vec<_>>(),
        vec![
            MessageKindDto::User,
            MessageKindDto::ToolCall,
            MessageKindDto::ToolResult,
            MessageKindDto::Assistant,
        ],
        "the failed call is answered and the loop continues to a completed answer"
    );
    assert_eq!(messages[0].text(), "Read missing.txt");
    assert_eq!(
        messages[1].tool_id().map(ToolIdentityDto::wire_name),
        Some("read")
    );
    assert_eq!(
        messages[1].text(),
        r#"{"path":"missing.txt"}"#,
        "the tool call keeps the relative workspace path"
    );
    let call_id = messages[1]
        .tool_call_id()
        .expect("the tool call row carries its identity");
    assert_eq!(
        messages[2].tool_id().map(ToolIdentityDto::wire_name),
        Some("read")
    );
    assert_eq!(messages[2].tool_call_id(), Some(call_id));
    assert!(
        messages[2].text().starts_with("the tool call failed\n")
            && messages[2].text().contains("tool_read_failed"),
        "the durable failed result carries the model-visible guidance: {}",
        messages[2].text()
    );
    assert_eq!(messages[3].text(), "done");
}

#[tokio::test]
async fn real_daemon_reports_a_declared_execute_timeout_as_a_named_partial() {
    // The call's own deadline ends the command, and the durable result the
    // model reads names the timeout instead of reading as lost evidence.
    let arguments = if cfg!(windows) {
        r#"{"command":"ping -n 5 127.0.0.1","timeout_seconds":1}"#
    } else {
        r#"{"command":"sleep 5","timeout_seconds":1}"#
    };
    let host = E2eHost::with_tool_round("execute", None, arguments, &[]);
    let client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20)).await;

    let session_id = SessionId::new();
    client
        .create_session(CreateSessionCommandDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse(host.workspace.path().to_string_lossy().into_owned())
                .expect("workspace root is absolute"),
            Some(RunModeDto::Build),
            None,
        ))
        .await
        .expect("the daemon accepts session creation");

    let outcome = client
        .send_user_turn(session_id, "Run a long command".to_owned())
        .await
        .expect("user turn is accepted");
    let SendUserTurnOutcomeDto::Started { .. } = outcome else {
        panic!("first turn starts a run, got: {outcome:?}")
    };

    let stream_client = SessionStreamClient::new(host.endpoint.clone());
    let deadline = Instant::now() + Duration::from_secs(30);
    let observation = observe_run_until_terminal(&stream_client, session_id, deadline).await;
    assert_eq!(
        observation.subscription.state().status(),
        Some(RunStatusDto::Completed),
        "the declared deadline ends the call, not the run"
    );
    assert_eq!(
        host.provider.request_count(),
        2,
        "the partial call is answered with its notice and the loop continues once"
    );
    assert_eq!(host.provider.excess_count(), 0);

    let session_snapshot = client
        .session_snapshot(session_id)
        .await
        .expect("session snapshot reads");
    let messages = session_snapshot.messages();
    assert_eq!(
        messages
            .iter()
            .map(|message| message.kind())
            .collect::<Vec<_>>(),
        vec![
            MessageKindDto::User,
            MessageKindDto::ToolCall,
            MessageKindDto::ToolResult,
            MessageKindDto::Assistant,
        ],
        "the timed-out call is answered and the loop continues to a completed answer"
    );
    assert_eq!(
        messages[1].tool_id().map(ToolIdentityDto::wire_name),
        Some("execute")
    );
    let result = messages[2].text();
    assert!(
        result.contains("hit its declared timeout of 1s and was terminated before a final result"),
        "the durable result names the deadline the model set: {result}"
    );
    assert!(
        !result.contains("did not receive a final result"),
        "a declared deadline never reaches the model as lost evidence: {result}"
    );
    assert_eq!(messages[3].text(), "done");
}

#[tokio::test]
async fn real_daemon_stops_a_running_tool_and_ends_the_run_interrupted() {
    // The stop is terminal where an interruption is not: a command that is
    // still running is killed, its partial result answers the call, the run
    // records its stop notice, and the loop never asks the provider again.
    let arguments = if cfg!(windows) {
        r#"{"command":"ping -n 11 127.0.0.1"}"#
    } else {
        r#"{"command":"sleep 10"}"#
    };
    let host = E2eHost::with_tool_round("execute", None, arguments, &[]);
    let client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20)).await;

    let session_id = SessionId::new();
    client
        .create_session(CreateSessionCommandDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse(host.workspace.path().to_string_lossy().into_owned())
                .expect("workspace root is absolute"),
            Some(RunModeDto::Build),
            None,
        ))
        .await
        .expect("the daemon accepts session creation");

    let outcome = client
        .send_user_turn(session_id, "Run a long command".to_owned())
        .await
        .expect("user turn is accepted");
    let SendUserTurnOutcomeDto::Started { run_id, .. } = outcome else {
        panic!("first turn starts a run, got: {outcome:?}")
    };

    // The tool call commits before the command is dispatched, so waiting for
    // its row proves the stop lands while the command runs.
    let call_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let snapshot = client
            .session_snapshot(session_id)
            .await
            .expect("session snapshot reads");
        if snapshot
            .messages()
            .iter()
            .any(|message| message.kind() == MessageKindDto::ToolCall)
        {
            break;
        }
        assert!(
            Instant::now() < call_deadline,
            "the tool call row never committed"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let stopped = client
        .cancel_run(session_id, run_id)
        .await
        .expect("the daemon accepts the stop");
    assert_eq!(stopped.session_id(), session_id);
    assert_eq!(stopped.run_id(), run_id);

    let stream_client = SessionStreamClient::new(host.endpoint.clone());
    let deadline = Instant::now() + Duration::from_secs(30);
    let observation = observe_run_until_terminal(&stream_client, session_id, deadline).await;
    assert_eq!(
        observation.subscription.state().status(),
        Some(RunStatusDto::Interrupted),
        "the stop ends the run instead of continuing it"
    );
    assert_eq!(
        host.provider.request_count(),
        1,
        "a stopped run asks the provider no further round"
    );
    assert_eq!(host.provider.excess_count(), 0);

    let session_snapshot = client
        .session_snapshot(session_id)
        .await
        .expect("session snapshot reads");
    let messages = session_snapshot.messages();
    assert_eq!(
        messages
            .iter()
            .map(|message| message.kind())
            .collect::<Vec<_>>(),
        vec![
            MessageKindDto::User,
            MessageKindDto::ToolCall,
            MessageKindDto::ToolResult,
            MessageKindDto::Notice,
        ],
        "the stopped call is answered and the run records its own stop"
    );
    let result = messages[2].text();
    assert!(
        result.contains("was stopped before a final result"),
        "the durable result carries the stopped-call notice: {result}"
    );
    assert_eq!(
        messages[3].text(),
        "[The run was stopped before a final answer.]"
    );
}

#[tokio::test]
async fn real_daemon_commits_step_reasoning_above_its_tool_exchange() {
    let host = E2eHost::with_tool_round_reasoning(
        Some(("hello.txt", "hello world")),
        r#"{"path":"hello.txt"}"#,
        &["weigh ", "the call"],
    );
    let client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20)).await;

    let session_id = SessionId::new();
    client
        .create_session(CreateSessionCommandDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse(host.workspace.path().to_string_lossy().into_owned())
                .expect("workspace root is absolute"),
            Some(RunModeDto::Build),
            None,
        ))
        .await
        .expect("the daemon accepts session creation");

    let outcome = client
        .send_user_turn(session_id, "Read hello.txt".to_owned())
        .await
        .expect("user turn is accepted");
    let SendUserTurnOutcomeDto::Started { .. } = outcome else {
        panic!("first turn starts a run, got: {outcome:?}")
    };

    let stream_client = SessionStreamClient::new(host.endpoint.clone());
    let deadline = Instant::now() + Duration::from_secs(30);
    let observation = observe_run_until_terminal(&stream_client, session_id, deadline).await;
    assert_eq!(
        observation.subscription.state().status(),
        Some(RunStatusDto::Completed),
        "the reasoned round completes after its tool exchange"
    );

    // The step's reasoning commits as its own assistant row before the tool
    // rows of that same step; the live subscription observes the same order.
    let expected = [
        (MessageKindDto::User, "Read hello.txt".to_owned(), None),
        (
            MessageKindDto::Assistant,
            String::new(),
            Some("weigh the call".to_owned()),
        ),
        (
            MessageKindDto::ToolCall,
            r#"{"path":"hello.txt"}"#.to_owned(),
            None,
        ),
        (MessageKindDto::ToolResult, "hello world".to_owned(), None),
        (MessageKindDto::Assistant, "done".to_owned(), None),
    ];
    let observed = observation
        .subscription
        .state()
        .messages()
        .iter()
        .map(|message| {
            (
                message.kind(),
                message.text().to_owned(),
                message.reasoning().map(str::to_owned),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        observed, expected,
        "the live frames carry the committed order"
    );
    let snapshot = client
        .session_snapshot(session_id)
        .await
        .expect("session snapshot reads");
    let durable = snapshot
        .messages()
        .iter()
        .map(|message| {
            (
                message.kind(),
                message.text().to_owned(),
                message.reasoning().map(str::to_owned),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        durable, expected,
        "the durable transcript carries the same order the live frames did"
    );
    assert_eq!(host.provider.request_count(), 2);
    assert_eq!(host.provider.excess_count(), 0);
}
