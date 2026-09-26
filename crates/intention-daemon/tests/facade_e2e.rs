//! Facade-level daemon-host end-to-end tests over real IPC.
//!
//! These tests spawn the real `intention-daemon` binary over real local
//! transport, drive it with the real client transport, and execute a real
//! `read` tool through the production model-tool loop against a fake
//! OpenAI-compatible provider. They prove durable `ToolResultRecorded` facts,
//! restart the daemon, and prove the same durable run replays without
//! re-executing the tool.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Facade end-to-end fixtures use assertion conveniences for precise diagnostics."
)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use intention_client::{IntentionClient, ProcessDaemonLauncher, RunStreamClient};
use intention_domain::{
    CreateSessionCommandDto, ModelRunFactDto, ModelRunFactInputDto, RunModeDto, RunSnapshotDto,
    RunStatusDto, SendUserTurnCommandDto, ToolResultOutcomeDto, WorkspaceRootDto,
};
use intention_protocol::contract_families::{
    GetProviderCatalogStatusQueryDto, ReconcileUnavailableQueueCommandDto,
};
use intention_protocol::{
    DaemonReadinessDto, ProtocolAcceptedResultDto, ProtocolCapabilityDto, ProtocolCommandDto,
    ProtocolCommandResultDto, ProtocolDaemonFrameDto, ProtocolHelloDto, ProtocolMessageDto,
    ProtocolQueryDto, ProtocolQueryResultDto, ProtocolRequestEnvelopeDto,
    ProtocolRequestPayloadDto, ProtocolResponsePayloadDto, RunStreamFrameDto,
    RunSubscriptionRequestEnvelopeDto, RunSubscriptionResponseDto, SendUserTurnOutcomeDto,
    SubscribeRunCommandDto,
};
use intention_transport::{
    AsyncLocalClientConnection, LocalConnection, LocalEndpoint, local_protocol_version,
    negotiate_client,
};
use intention_types::{
    CorrelationIdDto, DtoResult, ErrorDto, ProjectId, RunId, SessionId, TurnId, WorkspaceId,
};
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
        let config_home = TempDir::new().expect("config directory exists");
        let state_home = TempDir::new().expect("state directory exists");
        let workspace = TempDir::new().expect("workspace directory exists");
        if let Some((name, content)) = workspace_file {
            std::fs::write(workspace.path().join(name), content).expect("workspace fixture writes");
        }
        let credential = format!("fixture-credential-{}", std::process::id());
        let provider = FakeProvider::start(tool_arguments);
        write_config(config_home.path(), provider.port(), &credential);
        let endpoint = unique_endpoint();
        let daemon = spawn_daemon(&endpoint, config_home.path(), state_home.path());
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
        ));
    }

    fn kill_daemon(&mut self) {
        let Some(mut child) = self.daemon.take() else {
            return;
        };
        let _ = child.kill();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                let _ = child.wait();
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for E2eHost {
    fn drop(&mut self) {
        self.kill_daemon();
        self.provider.stop();
        #[cfg(unix)]
        if let Some(path) = endpoint_socket_path(&self.endpoint) {
            let _ = std::fs::remove_file(path);
        }
    }
}

static NEXT_ENDPOINT: AtomicUsize = AtomicUsize::new(0);

/// Builds a unique safe endpoint instance id for this test process.
fn unique_endpoint() -> LocalEndpoint {
    let sequence = NEXT_ENDPOINT.fetch_add(1, Ordering::Relaxed);
    LocalEndpoint::from_instance_id(format!("e2e-{}-{}", std::process::id(), sequence))
        .expect("fixture endpoint is valid")
}

/// Spawns the real daemon binary with only per-process environment overrides.
fn spawn_daemon(endpoint: &LocalEndpoint, config_home: &Path, state_home: &Path) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_intention-daemon"));
    command.arg(endpoint.instance_id());
    #[cfg(target_os = "linux")]
    {
        command.env("XDG_CONFIG_HOME", config_home);
        command.env("XDG_STATE_HOME", state_home);
    }
    #[cfg(target_os = "macos")]
    {
        // Both the configuration and the state directory derive from HOME.
        command.env("HOME", config_home);
    }
    #[cfg(windows)]
    {
        command.env("APPDATA", config_home);
        command.env("LOCALAPPDATA", state_home);
    }
    command.spawn().expect("daemon binary spawns")
}

/// Returns the platform config path the daemon resolves from its environment.
fn config_path(config_home: &Path) -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        config_home.join("intention-relay").join("config.toml")
    }
    #[cfg(target_os = "macos")]
    {
        config_home
            .join("Library/Application Support/intention-relay")
            .join("config.toml")
    }
    #[cfg(windows)]
    {
        config_home.join("intention-relay").join("config.toml")
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        config_home.join("intention-relay").join("config.toml")
    }
}

/// Writes the daemon configuration file with owner-only permissions on Unix.
fn write_config(config_home: &Path, port: u16, credential: &str) {
    let config_text = format!(
        "schema_version = 1\n[provider]\nkind = \"generic-chat-completion-api\"\nmodel = \"fixture-model\"\nendpoint = \"http://127.0.0.1:{port}/v1\"\ncredential = \"{credential}\"\n"
    );
    let config_path = config_path(config_home);
    let parent = config_path.parent().expect("config path has a parent");
    std::fs::create_dir_all(parent).expect("config directory is created");
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = std::fs::OpenOptions::new();
        options.create(true).write(true).truncate(true).mode(0o600);
        let mut file = options.open(&config_path).expect("config file opens");
        file.write_all(config_text.as_bytes())
            .expect("config file writes");
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&config_path, config_text).expect("config file writes");
    }
}

/// Replicates the daemon transport's platform endpoint path resolution so the
/// fixture can remove exactly the socket file its own daemon created.
#[cfg(unix)]
fn endpoint_socket_path(endpoint: &LocalEndpoint) -> Option<PathBuf> {
    let base = {
        #[cfg(target_os = "linux")]
        {
            std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .filter(|candidate| candidate.is_absolute())
                .or_else(|| {
                    std::env::var_os("XDG_CONFIG_HOME")
                        .map(PathBuf::from)
                        .filter(|candidate| candidate.is_absolute())
                        .map(|candidate| candidate.join("intention-relay"))
                })
                .or_else(|| {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .filter(|candidate| candidate.is_absolute())
                        .map(|candidate| candidate.join(".config/intention-relay"))
                })
        }
        #[cfg(target_os = "macos")]
        {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|candidate| candidate.is_absolute())
                .map(|candidate| candidate.join("Library/Application Support/intention-relay"))
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            None
        }
    }?;
    Some(base.join(format!("{}.sock", endpoint.instance_id())))
}

/// A fake OpenAI-compatible provider serving two scripted SSE rounds.
///
/// The first request receives a tool-call round for the `read` tool, the second
/// (whose body carries the tool result) receives a text round, and any further
/// request receives an HTTP 500 and is counted as excess traffic.
struct FakeProvider {
    port: u16,
    requests: Arc<AtomicUsize>,
    excess: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl FakeProvider {
    fn start(tool_arguments: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("provider binds");
        let port = listener
            .local_addr()
            .expect("provider port is available")
            .port();
        let requests = Arc::new(AtomicUsize::new(0));
        let excess = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_requests = Arc::clone(&requests);
        let thread_excess = Arc::clone(&excess);
        let thread_stop = Arc::clone(&stop);
        let tool_body = serde_json::to_string(&serde_json::json!({
            "id": "chatcmpl-facade-e2e-1",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "fixture-model",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "read", "arguments": tool_arguments},
                    }],
                },
                "finish_reason": "tool_calls",
            }],
        }))
        .expect("tool chunk serializes");
        let usage_body = serde_json::to_string(&serde_json::json!({
            "id": "chatcmpl-facade-e2e-1",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "fixture-model",
            "choices": [],
            "usage": {"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5},
        }))
        .expect("usage chunk serializes");
        let text_body = serde_json::to_string(&serde_json::json!({
            "id": "chatcmpl-facade-e2e-2",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "fixture-model",
            "choices": [{
                "index": 0,
                "delta": {"content": "done"},
                "finish_reason": "stop",
            }],
        }))
        .expect("text chunk serializes");
        let tool_response = sse_response(&format!(
            "data: {tool_body}\n\ndata: {usage_body}\n\ndata: [DONE]\n\n"
        ));
        let text_response = sse_response(&format!(
            "data: {text_body}\n\ndata: {usage_body}\n\ndata: [DONE]\n\n"
        ));
        let thread = thread::Builder::new()
            .name("facade-e2e-provider".to_owned())
            .spawn(move || {
                listener
                    .set_nonblocking(true)
                    .expect("provider listener is non-blocking");
                while !thread_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => handle_provider_request(
                            stream,
                            &thread_requests,
                            &thread_excess,
                            &tool_response,
                            &text_response,
                        ),
                        Err(_) => thread::sleep(Duration::from_millis(10)),
                    }
                }
            })
            .expect("provider thread starts");
        Self {
            port,
            requests,
            excess,
            stop,
            thread: Some(thread),
        }
    }

    const fn port(&self) -> u16 {
        self.port
    }

    fn request_count(&self) -> usize {
        self.requests.load(Ordering::Acquire)
    }

    fn excess_count(&self) -> usize {
        self.excess.load(Ordering::Acquire)
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Reads one HTTP request head and body and answers it from the script.
fn handle_provider_request(
    mut stream: TcpStream,
    requests: &AtomicUsize,
    excess: &AtomicUsize,
    tool_response: &str,
    text_response: &str,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Some(body) = read_request_body(&mut stream) else {
        return;
    };
    // The provider paces each scripted round so the test's subscriber can
    // attach before the durable facts for that round are committed.
    thread::sleep(Duration::from_millis(500));
    let request_number = requests.fetch_add(1, Ordering::AcqRel) + 1;
    let body_text = String::from_utf8_lossy(&body);
    if request_number == 1 {
        assert!(
            body_text.contains(r#""tools":["#) && body_text.contains(r#""name":"read""#),
            "the first provider request advertises tools including read: {body_text}"
        );
    }
    if request_number <= 2 {
        let response = if body_text.contains("\"role\":\"tool\"") {
            text_response
        } else {
            tool_response
        };
        write_response(&mut stream, response);
    } else {
        excess.fetch_add(1, Ordering::AcqRel);
        write_response(&mut stream, &excess_response());
    }
}

fn write_response(stream: &mut TcpStream, response: &str) {
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// Reads one complete HTTP request body: the head up to its blank line plus
/// exactly the declared Content-Length bytes, preserving any body bytes that
/// arrived in the same read as the head.
fn read_request_body(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut head = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return None,
            Ok(read) => {
                head.extend_from_slice(&buffer[..read]);
                if let Some(end) = head.windows(4).position(|window| window == b"\r\n\r\n") {
                    let length = content_length(&head[..end]);
                    let mut body = head.split_off(end + 4);
                    if body.len() < length {
                        let mut remaining = vec![0_u8; length - body.len()];
                        if stream.read_exact(&mut remaining).is_err() {
                            return None;
                        }
                        body.extend_from_slice(&remaining);
                    }
                    body.truncate(length);
                    return Some(body);
                }
            }
            Err(_) => return None,
        }
    }
}

/// Parses the Content-Length header from an HTTP request head.
fn content_length(head: &[u8]) -> usize {
    let head = String::from_utf8_lossy(head);
    for line in head.lines() {
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            return value.trim().parse().unwrap_or(0);
        }
    }
    0
}

fn sse_response(body: &str) -> String {
    format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}")
}

fn excess_response() -> String {
    let body = r#"{"error":{"message":"unexpected provider request","type":"server_error"}}"#;
    format!(
        "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

/// The exact capability list the shared client advertises and requires from
/// the daemon.
///
/// The list itself is private to `intention-client`, but its values are public
/// protocol capabilities and the daemon's negotiation only verifies protocol
/// version compatibility, so the fixture reconstructs the same hello with
/// public APIs only. `provider_profiles_v1` is part of it because the client
/// surfaces the gated control plane.
fn command_hello() -> ProtocolHelloDto {
    ProtocolHelloDto::new(
        local_protocol_version(),
        vec![
            ProtocolCapabilityDto::SessionSubscriptions,
            ProtocolCapabilityDto::CorrelatedRequests,
            ProtocolCapabilityDto::DaemonHealth,
            ProtocolCapabilityDto::ProviderProfilesV1,
        ],
        "facade-e2e",
    )
    .expect("fixture command hello is valid")
}

/// The pre-Slice-2 baseline capability set, used as the negative probe for the
/// daemon's `provider_profiles_v1` control-plane gate.
fn baseline_hello() -> ProtocolHelloDto {
    ProtocolHelloDto::new(
        local_protocol_version(),
        vec![
            ProtocolCapabilityDto::SessionSubscriptions,
            ProtocolCapabilityDto::CorrelatedRequests,
            ProtocolCapabilityDto::DaemonHealth,
        ],
        "facade-e2e-baseline",
    )
    .expect("fixture baseline hello is valid")
}

fn stream_hello() -> ProtocolHelloDto {
    ProtocolHelloDto::new(
        local_protocol_version(),
        vec![ProtocolCapabilityDto::RunStreamSubscriptions],
        "facade-e2e",
    )
    .expect("fixture stream hello is valid")
}

/// Sends one typed protocol payload over a fresh negotiated connection and
/// verifies the correlated response, replicating the client's private request
/// path with public transport and protocol APIs only.
fn send_payload(
    endpoint: &LocalEndpoint,
    hello: ProtocolHelloDto,
    payload: ProtocolRequestPayloadDto,
) -> DtoResult<ProtocolResponsePayloadDto> {
    let mut connection = LocalConnection::connect(endpoint)?;
    let remote = negotiate_client(&mut connection, hello)?;
    let correlation_id = CorrelationIdDto::new();
    connection.send_request(&ProtocolRequestEnvelopeDto::new(
        local_protocol_version(),
        correlation_id,
        ProtocolMessageDto::new(intention_protocol::CURRENT_DTO_SCHEMA_VERSION, payload),
    ))?;
    let response = connection.receive_response()?;
    if response.correlation_id() != correlation_id
        || response.protocol_version() != remote.version()
    {
        return Err(invalid_response());
    }
    Ok(response.message().payload().clone())
}

/// Sends one typed protocol command with the shared client's capability set.
fn send_command(
    endpoint: &LocalEndpoint,
    payload: ProtocolRequestPayloadDto,
) -> DtoResult<ProtocolCommandResultDto> {
    match send_payload(endpoint, command_hello(), payload)? {
        ProtocolResponsePayloadDto::CommandResult(result) => Ok(result),
        _ => Err(invalid_response()),
    }
}

/// Sends one typed protocol command with an explicit capability advertisement.
fn send_command_with_hello(
    endpoint: &LocalEndpoint,
    hello: ProtocolHelloDto,
    command: ProtocolCommandDto,
) -> DtoResult<ProtocolCommandResultDto> {
    match send_payload(endpoint, hello, ProtocolRequestPayloadDto::Command(command))? {
        ProtocolResponsePayloadDto::CommandResult(result) => Ok(result),
        _ => Err(invalid_response()),
    }
}

/// Sends one typed protocol query with an explicit capability advertisement.
fn send_query(
    endpoint: &LocalEndpoint,
    hello: ProtocolHelloDto,
    query: ProtocolQueryDto,
) -> DtoResult<ProtocolQueryResultDto> {
    match send_payload(endpoint, hello, ProtocolRequestPayloadDto::Query(query))? {
        ProtocolResponsePayloadDto::QueryResult(result) => Ok(result),
        _ => Err(invalid_response()),
    }
}

fn invalid_response() -> ErrorDto {
    ErrorDto::validation(
        "invalid_local_protocol_response",
        "the local daemon returned an unexpected protocol response",
    )
}

/// Polls the daemon health projection until it reports `Ready`.
///
/// `health()` only negotiates and queries; it never launches the daemon.
fn wait_until_ready(endpoint: &LocalEndpoint, deadline: Instant) -> IntentionClient {
    let client = IntentionClient::new(
        endpoint.clone(),
        "facade-e2e",
        Box::new(
            ProcessDaemonLauncher::new(env!("CARGO_BIN_EXE_intention-daemon"))
                .expect("daemon program is valid"),
        ),
    )
    .expect("facade e2e client is valid");
    while Instant::now() < deadline {
        match client.health() {
            Ok(health) if health.readiness() == DaemonReadinessDto::Ready => return client,
            Ok(_) => {}
            Err(_) => {}
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("daemon becomes ready before the deadline");
}

/// Drives the real `RunStreamClient` until the run reaches a terminal snapshot.
async fn observe_terminal_snapshot(
    client: &RunStreamClient,
    session_id: SessionId,
    run_id: RunId,
    deadline: Instant,
) -> RunSnapshotDto {
    let mut subscription = client
        .subscribe(SubscribeRunCommandDto::new(
            intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
            session_id,
            run_id,
            None,
        ))
        .await
        .expect("run subscription arrives");
    loop {
        if let Some(snapshot) = subscription.reducer().snapshot()
            && snapshot.run_projection().status().is_terminal()
        {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "run reaches a terminal snapshot before the deadline"
        );
        match tokio::time::timeout(Duration::from_secs(1), subscription.receive()).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => panic!("run stream frame error: {}", error.code()),
            Err(_) => {}
        }
    }
}

/// Subscribes to one run stream and collects every delivered durable fact plus
/// the terminal snapshot. The daemon only replays the snapshot and tail on
/// subscribe; facts are delivered as live batches while the run commits them.
async fn collect_run_facts(
    endpoint: &LocalEndpoint,
    session_id: SessionId,
    run_id: RunId,
    deadline: Instant,
) -> (Vec<ModelRunFactDto>, RunSnapshotDto) {
    let connection = AsyncLocalClientConnection::connect(endpoint)
        .await
        .expect("run stream connects");
    let (_remote, mut requests, mut frames) = connection
        .negotiate_daemon_frames(stream_hello())
        .await
        .expect("run stream negotiates");
    let correlation_id = CorrelationIdDto::new();
    requests
        .send_run_subscription(&RunSubscriptionRequestEnvelopeDto::new(
            local_protocol_version(),
            correlation_id,
            ProtocolMessageDto::new(
                intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
                SubscribeRunCommandDto::new(
                    intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
                    session_id,
                    run_id,
                    None,
                ),
            ),
        ))
        .await
        .expect("run subscription request sends");
    let mut facts = Vec::new();
    loop {
        assert!(
            Instant::now() < deadline,
            "run facts arrive before the deadline"
        );
        let frame = tokio::time::timeout(Duration::from_secs(1), frames.receive())
            .await
            .expect("run stream frame within the deadline")
            .expect("run stream frame is valid");
        match frame {
            ProtocolDaemonFrameDto::Response(response) => {
                assert_eq!(response.correlation_id(), correlation_id);
                match response.message().payload() {
                    ProtocolResponsePayloadDto::RunSubscription(
                        RunSubscriptionResponseDto::Replay(replay),
                    ) => {
                        facts.extend(replay.tail().facts().iter().cloned());
                        let snapshot = replay.snapshot().clone();
                        if snapshot.run_projection().status().is_terminal() {
                            return (facts, snapshot);
                        }
                    }
                    _ => panic!("run subscription reply must be a replay"),
                }
            }
            ProtocolDaemonFrameDto::RunStream(RunStreamFrameDto::LiveBatch(batch)) => {
                facts.extend(batch.facts().iter().cloned());
            }
            ProtocolDaemonFrameDto::RunStream(RunStreamFrameDto::Snapshot(frame)) => {
                let snapshot = frame.snapshot().clone();
                if snapshot.run_projection().status().is_terminal() {
                    return (facts, snapshot);
                }
            }
            ProtocolDaemonFrameDto::RunStream(RunStreamFrameDto::Resync(resync)) => {
                panic!("unexpected run resync: {:?}", resync.reason());
            }
        }
    }
}

/// Asserts the delivered durable facts are a non-empty contiguous cursor range
/// ending in a terminal fact (`Finished` or `Failed`).
///
/// The daemon replays an empty tail on subscribe and broadcasts live batches
/// only for facts committed after registration, so a subscriber can miss a
/// prefix of the run's facts but never a gap inside the delivered suffix.
fn assert_contiguous_facts(facts: &[ModelRunFactDto]) {
    assert!(!facts.is_empty(), "durable facts are delivered");
    assert!(
        facts
            .windows(2)
            .all(|pair| pair[0].cursor().value() + 1 == pair[1].cursor().value()),
        "durable facts are contiguous in cursor order"
    );
    assert!(
        matches!(
            facts.last().map(ModelRunFactDto::input),
            Some(ModelRunFactInputDto::Finished { .. }) | Some(ModelRunFactInputDto::Failed { .. })
        ),
        "the final durable fact closes the run"
    );
}

/// The real daemon accepts the shared
/// client's gated control-plane surface end to end because the client hello
/// advertises `provider_profiles_v1`, while the same request from a baseline
/// peer is rejected before any effect.
#[test]
fn real_daemon_control_plane_gate_accepts_the_client_capability_advertisement() {
    let host = E2eHost::new(None, "{}");
    let client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20));

    let session_id = SessionId::new();
    let created = send_command(
        &host.endpoint,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::CreateSession(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                WorkspaceRootDto::parse(host.workspace.path().to_string_lossy().into_owned())
                    .expect("workspace root is absolute"),
                RunModeDto::Build,
            ),
        )),
    )
    .expect("session creation is accepted");
    assert!(
        matches!(created, ProtocolCommandResultDto::Accepted(_)),
        "the daemon accepts session creation"
    );

    // A gated query through the real client succeeds because the client hello
    // advertises provider_profiles_v1.
    let status = client
        .provider_catalog_status(GetProviderCatalogStatusQueryDto {
            schema_version: "1.1".to_owned(),
        })
        .expect("the real daemon accepts a gated control-plane query");
    assert_eq!(status.schema_version, "1.1");

    // A gated command through the real client succeeds for the same reason.
    let reconciled = client
        .reconcile_unavailable_queue(ReconcileUnavailableQueueCommandDto {
            session_id: session_id.to_string(),
            operation_id: "op-w2b-reconcile".to_owned(),
        })
        .expect("the real daemon accepts a gated control-plane command");
    assert_eq!(reconciled.session_id, session_id.to_string());
    assert_eq!(reconciled.promoted_count, 0);
    assert_eq!(reconciled.page_cursor, None);

    // The negative probe: the same gated query from a baseline peer is
    // rejected before any effect with the protocol helper's stable code.
    let rejected = send_query(
        &host.endpoint,
        baseline_hello(),
        ProtocolQueryDto::GetProviderCatalogStatus(GetProviderCatalogStatusQueryDto {
            schema_version: "1.1".to_owned(),
        }),
    )
    .expect("the real daemon answers the gated query");
    match rejected {
        ProtocolQueryResultDto::Rejected(error) => {
            assert_eq!(error.code(), "provider_profiles_capability_required");
        }
        other => panic!("a peer without the capability must be rejected, got {other:?}"),
    }

    // The same gate rejects a gated command from a baseline peer before any
    // effect, through the same negotiated request path the positive command
    // above uses.
    let rejected = send_command_with_hello(
        &host.endpoint,
        baseline_hello(),
        ProtocolCommandDto::ReconcileUnavailableQueue(ReconcileUnavailableQueueCommandDto {
            session_id: session_id.to_string(),
            operation_id: "op-w2b-reconcile-baseline".to_owned(),
        }),
    )
    .expect("the real daemon answers the gated command");
    match rejected {
        ProtocolCommandResultDto::Rejected(error) => {
            assert_eq!(error.code(), "provider_profiles_capability_required");
        }
        other => panic!("a peer without the capability must be rejected, got {other:?}"),
    }

    assert_eq!(
        host.provider.request_count(),
        0,
        "control-plane calls never execute provider work"
    );
}

#[tokio::test]
async fn real_daemon_tool_loop_executes_read_and_replays_after_restart() {
    let mut host = E2eHost::new(
        Some(("hello.txt", "hello from e2e")),
        r#"{"path":"hello.txt"}"#,
    );
    let client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20));
    let workspace_root = host.workspace.path().to_string_lossy().into_owned();

    let session_id = SessionId::new();
    let created = send_command(
        &host.endpoint,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::CreateSession(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                WorkspaceRootDto::parse(workspace_root).expect("workspace root is absolute"),
                RunModeDto::Build,
            ),
        )),
    )
    .expect("session creation is accepted");
    assert!(
        matches!(created, ProtocolCommandResultDto::Accepted(_)),
        "the daemon accepts session creation"
    );

    let result = send_command(
        &host.endpoint,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, TurnId::new(), "Read hello.txt")
                .expect("turn is valid"),
        )),
    )
    .expect("user turn is accepted");
    let ProtocolCommandResultDto::Accepted(accepted) = result else {
        panic!("user turn starts a run, got: {result:?}")
    };
    let ProtocolAcceptedResultDto::SendUserTurn(turn) = accepted.result() else {
        panic!("user turn result starts a run, got: {accepted:?}")
    };
    let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
        panic!("first turn starts a run, got: {turn:?}")
    };

    let stream_client =
        RunStreamClient::new(host.endpoint.clone(), "facade-e2e").expect("stream client is valid");
    let live_deadline = Instant::now() + Duration::from_secs(30);
    // Subscribe for facts first: the daemon broadcasts live batches only while
    // the run commits, and the run-stream replay tail is empty by design.
    let (facts, snapshot) =
        collect_run_facts(&host.endpoint, session_id, run_id, live_deadline).await;
    let terminal_snapshot =
        observe_terminal_snapshot(&stream_client, session_id, run_id, live_deadline).await;
    assert_eq!(
        terminal_snapshot.run_projection().status(),
        RunStatusDto::Completed,
        "the real daemon completes the tool round: {:?}",
        terminal_snapshot.projection().failure()
    );
    assert_contiguous_facts(&facts);
    let tool_call = facts
        .iter()
        .find_map(|fact| match fact.input() {
            ModelRunFactInputDto::ToolCallRecorded { call } if call.name() == "read" => {
                Some(call.clone())
            }
            _ => None,
        })
        .expect("the read tool call is durable");
    assert_eq!(
        tool_call.arguments_json(),
        r#"{"path":"hello.txt"}"#,
        "the durable tool call keeps the relative workspace path"
    );
    let tool_result = facts
        .iter()
        .find_map(|fact| match fact.input() {
            ModelRunFactInputDto::ToolResultRecorded { call_id, outcome }
                if *call_id == tool_call.call_id() =>
            {
                Some(outcome)
            }
            _ => None,
        })
        .expect("the read tool result is durable");
    assert!(
        matches!(
            tool_result,
            ToolResultOutcomeDto::Succeeded { content } if content == "hello from e2e"
        ),
        "the durable tool result carries the exact file content"
    );
    assert_eq!(snapshot.run_projection().status(), RunStatusDto::Completed);
    assert_eq!(
        facts.last().expect("facts exist").cursor(),
        snapshot.cursor(),
        "the terminal fact cursor equals the authoritative snapshot cursor"
    );
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

    // Public payloads never disclose the credential; durable run facts never
    // disclose the absolute workspace path.
    let session_json = serde_json::to_string(
        &client
            .session_snapshot(session_id)
            .expect("session snapshot reads"),
    )
    .expect("session snapshot serializes");
    let facts_json = serde_json::to_string(&facts).expect("durable facts serialize");
    assert!(
        !facts_json.contains(&host.credential),
        "durable facts never disclose the provider credential"
    );
    assert!(
        !facts_json.contains(&host.workspace.path().to_string_lossy().into_owned()),
        "durable facts never disclose the absolute workspace path"
    );
    assert!(
        !session_json.contains(&host.credential),
        "the session snapshot never discloses the provider credential"
    );
    let session_sequence = client
        .session_snapshot(session_id)
        .expect("session snapshot reads")
        .at_sequence();
    let pre_restart_cursor = snapshot.cursor();

    // Restart the daemon against the identical environment and state, then
    // prove the completed run replays without any provider re-execution.
    host.restart_daemon();
    let client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20));
    let mut subscription = stream_client
        .subscribe(SubscribeRunCommandDto::new(
            intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
            session_id,
            run_id,
            None,
        ))
        .await
        .expect("restart replay arrives");
    let replay_snapshot = subscription
        .reducer()
        .snapshot()
        .expect("restart replay is authoritative");
    assert_eq!(
        replay_snapshot.run_projection().status(),
        RunStatusDto::Completed,
        "the restarted daemon replays the completed run"
    );
    assert_eq!(
        replay_snapshot.cursor(),
        pre_restart_cursor,
        "the durable run cursor replays unchanged"
    );
    subscription
        .request_replay()
        .await
        .expect("repeat replay arrives");
    assert_eq!(
        subscription
            .reducer()
            .snapshot()
            .expect("repeat replay is authoritative")
            .run_projection()
            .status(),
        RunStatusDto::Completed,
        "the same connection accepts a repeated replay"
    );
    let (replayed_facts, replayed_snapshot) = collect_run_facts(
        &host.endpoint,
        session_id,
        run_id,
        Instant::now() + Duration::from_secs(15),
    )
    .await;
    assert_eq!(
        replayed_snapshot.run_projection().status(),
        RunStatusDto::Completed
    );
    assert_eq!(
        replayed_snapshot.cursor(),
        pre_restart_cursor,
        "the replayed snapshot preserves the exact durable cursor"
    );
    assert!(
        replayed_facts.is_empty(),
        "the daemon run-stream replay tail is empty by design; live batches carry facts"
    );
    let replayed_session_sequence = client
        .session_snapshot(session_id)
        .expect("replayed session snapshot reads")
        .at_sequence();
    assert_eq!(
        replayed_session_sequence, session_sequence,
        "the durable session event sequence replays unchanged"
    );

    // Allow any late provider traffic to land before asserting the tool was
    // never re-executed after the restart.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        host.provider.request_count(),
        2,
        "the restarted daemon replays durable facts without re-executing the tool"
    );
    assert_eq!(host.provider.excess_count(), 0);
}

#[tokio::test]
async fn real_daemon_tool_loop_denies_without_provider_retry_on_tool_failure() {
    let host = E2eHost::new(None, r#"{"path":"missing.txt"}"#);
    let _client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20));

    let session_id = SessionId::new();
    let created = send_command(
        &host.endpoint,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::CreateSession(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                WorkspaceRootDto::parse(host.workspace.path().to_string_lossy().into_owned())
                    .expect("workspace root is absolute"),
                RunModeDto::Build,
            ),
        )),
    )
    .expect("session creation is accepted");
    assert!(matches!(created, ProtocolCommandResultDto::Accepted(_)));

    let result = send_command(
        &host.endpoint,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, TurnId::new(), "Read missing.txt")
                .expect("turn is valid"),
        )),
    )
    .expect("user turn is accepted");
    let ProtocolCommandResultDto::Accepted(accepted) = result else {
        panic!("user turn starts a run, got: {result:?}")
    };
    let ProtocolAcceptedResultDto::SendUserTurn(turn) = accepted.result() else {
        panic!("user turn result starts a run, got: {accepted:?}")
    };
    let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
        panic!("first turn starts a run, got: {turn:?}")
    };

    let stream_client =
        RunStreamClient::new(host.endpoint.clone(), "facade-e2e").expect("stream client is valid");
    let deadline = Instant::now() + Duration::from_secs(30);
    let (facts, snapshot) = collect_run_facts(&host.endpoint, session_id, run_id, deadline).await;
    let terminal_snapshot =
        observe_terminal_snapshot(&stream_client, session_id, run_id, deadline).await;
    assert_eq!(
        terminal_snapshot.run_projection().status(),
        RunStatusDto::Failed,
        "the real daemon terminalizes the missing-file tool round as Failed: {:?}",
        terminal_snapshot.projection().failure()
    );
    assert_contiguous_facts(&facts);
    assert!(
        facts.iter().any(|fact| matches!(
            fact.input(),
            ModelRunFactInputDto::ToolCallRecorded { call }
                if call.name() == "read"
        )),
        "the denied read tool call is durable"
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact.input(),
            ModelRunFactInputDto::ToolResultRecorded {
                outcome: ToolResultOutcomeDto::Failed { failure },
                ..
            } if failure.code() == "workspace_path_unavailable"
        )),
        "the missing-file tool result is a durable typed failure"
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact.input(),
            ModelRunFactInputDto::Failed { failure }
                if failure.code() == "workspace_path_unavailable"
        )),
        "the run terminalizes with the durable tool failure"
    );
    assert_eq!(snapshot.run_projection().status(), RunStatusDto::Failed);
    assert_eq!(
        host.provider.request_count(),
        1,
        "the typed tool failure never retries the provider"
    );
    assert_eq!(host.provider.excess_count(), 0);
}

/// In-process daemon-host plumbing over the crate's bounded test-support seams.
///
/// These fixtures drive one daemon host inside the measured test process
/// through `serve_test_async_listener`, `serve_test_connection`, and
/// `TestHostLifecycle`: the capability gates, ordinary request dispatch,
/// recovered-run admission, the exact task registry, and the daemon-owned tool
/// executor. They never load the platform configuration, spawn a daemon
/// process, or bind a real user endpoint.
#[cfg(feature = "test-support")]
mod in_process_host {
    use super::*;
    use futures_util::StreamExt;
    use intention::DaemonApplicationFacade;
    use intention_config::{
        ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
    };
    use intention_daemon::{
        DaemonToolExecutor, serve_test_async_listener, serve_test_connection, test_host_lifecycle,
    };
    use intention_domain::StopRunCommandDto;
    use intention_model::{
        FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto, ModelDriver, ModelEventDto,
        ModelEventStream, ModelExecutionDriver, ModelRequestDto,
    };
    use intention_protocol::contract_families::AdmitRecoveredRunCommandDto;
    use intention_protocol::{
        ProtocolVersionDto, SessionSubscriptionResponseDto, SubscribeSessionCommandDto,
    };
    use intention_runtime::ToolExecutionPort;
    use intention_transport::{AsyncLocalListener, AsyncRequestSender, LocalListener};
    use intention_types::{ConfigRevisionId, SchemaVersionDto, ToolCallDto, ToolCallId};

    /// The credential-free configuration snapshot of every in-process fixture.
    fn fixture_snapshot() -> ConfigSnapshotDto {
        let source = ConfigSourceDto::Explicit(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("intention-daemon-in-process.toml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("fixture configuration path is absolute"),
        );
        let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential\"",
            source,
        ))
        .expect("fixture configuration resolves");
        ConfigSnapshotDto::new(
            SchemaVersionDto::new(1, 0),
            ConfigRevisionId::new(),
            intention_types::TimestampDto::from_unix_seconds(1)
                .expect("fixture timestamp is valid"),
            resolved,
        )
        .expect("fixture snapshot is credential-free")
    }

    /// Opens one isolated durable facade with the supplied provider driver.
    fn open_facade(
        directory: &TempDir,
        driver: Arc<dyn ModelExecutionDriver + Send + Sync>,
    ) -> DaemonApplicationFacade {
        DaemonApplicationFacade::open_for_test_support_with_driver(
            directory.path().join("in-process.sqlite"),
            fixture_snapshot(),
            driver,
        )
        .expect("fixture facade opens")
    }

    /// Seeds the one auto-accepted catalog profile every fixture turn resolves.
    fn seed_catalog(facade: &DaemonApplicationFacade) {
        facade
            .seed_fixture_catalog_for_test_support(
                "seed-1",
                "openrouter",
                "fixture",
                "https://api.example.invalid/v1",
            )
            .expect("fixture catalog seeds");
    }

    /// Creates one ordinary fixture session rooted at the supplied workspace.
    fn create_session(facade: &DaemonApplicationFacade, workspace: &Path) -> SessionId {
        let session_id = SessionId::new();
        let created = facade.command(ProtocolCommandDto::CreateSession(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                WorkspaceRootDto::parse(workspace.to_string_lossy().into_owned())
                    .expect("fixture workspace is absolute"),
                RunModeDto::Build,
            ),
        ));
        assert!(
            matches!(created, ProtocolCommandResultDto::Accepted(_)),
            "fixture session creates: {created:?}"
        );
        session_id
    }

    /// Starts one durable fixture turn and returns its `Starting` run identity.
    fn start_turn(facade: &DaemonApplicationFacade, session_id: SessionId, content: &str) -> RunId {
        let accepted = facade.command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, TurnId::new(), content)
                .expect("fixture turn is valid"),
        ));
        let ProtocolCommandResultDto::Accepted(accepted) = accepted else {
            panic!("fixture turn is accepted: {accepted:?}")
        };
        let ProtocolAcceptedResultDto::SendUserTurn(turn) = accepted.result() else {
            panic!("fixture turn result owns a run")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
            panic!("fixture first turn starts a run")
        };
        run_id
    }

    /// Polls one durable run projection until it reaches a terminal status.
    async fn wait_for_terminal_run(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        run_id: RunId,
    ) -> RunStatusDto {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = facade
                .load_current_run_replay_for_daemon(session_id, run_id)
                .expect("fixture run replay reads")
                .snapshot()
                .run_projection()
                .status();
            if status.is_terminal() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "the fixture run reaches a terminal status"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Completes every started run with one text round.
    struct CompletedDriver;

    impl ModelDriver for CompletedDriver {
        fn capabilities(&self) -> ModelCapabilitiesDto {
            ModelCapabilitiesDto::new(true, true, true, false, false, true)
        }
    }

    impl ModelExecutionDriver for CompletedDriver {
        fn execute(
            &self,
            _request: ModelRequestDto,
            _cancellation: ModelCancellationSignal,
        ) -> ModelEventStream {
            Box::pin(futures_util::stream::iter(vec![
                Ok(ModelEventDto::started()),
                Ok(ModelEventDto::text_delta("in-process fixture output")
                    .expect("fixture text is valid")),
                Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
            ]))
        }
    }

    /// Blocks one execution round until the fixture releases it.
    struct BlockingDriver {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    impl BlockingDriver {
        fn new() -> Self {
            Self {
                entered: Arc::new(tokio::sync::Notify::new()),
                release: Arc::new(tokio::sync::Notify::new()),
            }
        }
    }

    impl ModelDriver for BlockingDriver {
        fn capabilities(&self) -> ModelCapabilitiesDto {
            ModelCapabilitiesDto::new(true, true, true, false, false, true)
        }
    }

    impl ModelExecutionDriver for BlockingDriver {
        fn execute(
            &self,
            _request: ModelRequestDto,
            _cancellation: ModelCancellationSignal,
        ) -> ModelEventStream {
            self.entered.notify_one();
            let release = Arc::clone(&self.release);
            Box::pin(
                futures_util::stream::once(async move {
                    release.notified().await;
                    Ok(ModelEventDto::started())
                })
                .chain(futures_util::stream::iter(vec![
                    Ok(ModelEventDto::text_delta("released fixture output")
                        .expect("fixture text is valid")),
                    Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
                ])),
            )
        }
    }

    /// Sends one typed request over an already negotiated fixture client.
    async fn send_typed_request(
        requests: &mut AsyncRequestSender,
        payload: ProtocolRequestPayloadDto,
    ) -> CorrelationIdDto {
        let correlation_id = CorrelationIdDto::new();
        requests
            .send(&ProtocolRequestEnvelopeDto::new(
                local_protocol_version(),
                correlation_id,
                ProtocolMessageDto::new(intention_protocol::CURRENT_DTO_SCHEMA_VERSION, payload),
            ))
            .await
            .expect("fixture request sends");
        correlation_id
    }

    /// A fixture hello that is compatible in every capability but its version.
    fn version_mismatched_hello() -> ProtocolHelloDto {
        let current = local_protocol_version();
        ProtocolHelloDto::new(
            ProtocolVersionDto::new(current.major() + 1, current.minor()),
            vec![ProtocolCapabilityDto::DaemonHealth],
            "in-process-mismatched",
        )
        .expect("fixture hello is valid")
    }

    #[tokio::test]
    async fn in_process_listener_dispatches_queries_subscriptions_and_recovered_admission() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = open_facade(&directory, Arc::new(CompletedDriver));
        seed_catalog(&facade);
        let workspace = TempDir::new().expect("temporary workspace exists");
        let session_id = create_session(&facade, workspace.path());
        let run_id = start_turn(&facade, session_id, "held in-process fixture turn");
        facade
            .mark_recovered_run_held_for_daemon(session_id, run_id)
            .expect("fixture run is held for explicit admission");

        let endpoint = unique_endpoint();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
        let server = tokio::spawn(serve_test_async_listener(listener, facade.clone(), 2));

        // A peer that declares another protocol version is dropped during the
        // hello exchange, before any request could reach a gate or an effect.
        let mismatched = AsyncLocalClientConnection::connect(&endpoint)
            .await
            .expect("version-mismatched peer connects");
        assert!(
            mismatched
                .negotiate(version_mismatched_hello())
                .await
                .is_err(),
            "the daemon rejects a version-mismatched peer"
        );

        // A negotiated peer dispatches a baseline health query, the session
        // subscription command, and the held-run admission whose accepted
        // command the host then admits as one real execution.
        let connection = AsyncLocalClientConnection::connect(&endpoint)
            .await
            .expect("ordinary peer connects");
        let (_remote, mut requests, mut responses) = connection
            .negotiate(command_hello())
            .await
            .expect("ordinary peer negotiates");

        let health_correlation = send_typed_request(
            &mut requests,
            ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
        )
        .await;
        let health_response = responses.receive().await.expect("health response arrives");
        assert_eq!(health_response.correlation_id(), health_correlation);
        assert!(matches!(
            health_response.message().payload(),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(health))
                if health.readiness() == DaemonReadinessDto::Ready
        ));

        let subscription_correlation = send_typed_request(
            &mut requests,
            ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SubscribeSession(
                SubscribeSessionCommandDto::new(
                    intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
                    session_id,
                    None,
                    RunModeDto::Build,
                ),
            )),
        )
        .await;
        let subscription_response = responses
            .receive()
            .await
            .expect("subscription response arrives");
        assert_eq!(
            subscription_response.correlation_id(),
            subscription_correlation
        );
        assert!(matches!(
            subscription_response.message().payload(),
            ProtocolResponsePayloadDto::Subscription(
                SessionSubscriptionResponseDto::SnapshotAndTail { .. }
            )
        ));

        let admission_correlation = send_typed_request(
            &mut requests,
            ProtocolRequestPayloadDto::Command(ProtocolCommandDto::AdmitRecoveredRun(
                AdmitRecoveredRunCommandDto {
                    session_id: session_id.to_string(),
                    run_id: run_id.to_string(),
                    operation_id: "in-process-admit-1".to_owned(),
                },
            )),
        )
        .await;
        let admission_response = responses
            .receive()
            .await
            .expect("admission response arrives");
        assert_eq!(admission_response.correlation_id(), admission_correlation);
        assert!(matches!(
            admission_response.message().payload(),
            ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(accepted))
                if matches!(
                    accepted.result(),
                    ProtocolAcceptedResultDto::AdmitRecoveredRun(_)
                )
        ));

        server.await.expect("host accepts both fixture peers");
        assert_eq!(
            wait_for_terminal_run(&facade, session_id, run_id).await,
            RunStatusDto::Completed,
            "the admitted recovered run executes to completion"
        );
    }

    #[test]
    fn in_process_blocking_seam_serves_one_request_per_connection_and_drops_broken_peers() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = open_facade(&directory, Arc::new(CompletedDriver));
        seed_catalog(&facade);
        let workspace = TempDir::new().expect("temporary workspace exists");
        let session_id = create_session(&facade, workspace.path());

        let endpoint = unique_endpoint();
        let listener = LocalListener::bind(endpoint.clone()).expect("fixture listener binds");
        let server = thread::spawn(move || {
            for _ in 0..4 {
                let connection = listener.accept().expect("fixture peer connects");
                serve_test_connection(connection, facade.clone());
            }
        });

        // A version-mismatched peer is dropped during the hello exchange.
        let mut mismatched = LocalConnection::connect(&endpoint).expect("fixture peer connects");
        assert!(
            negotiate_client(&mut mismatched, version_mismatched_hello()).is_err(),
            "the daemon rejects a version-mismatched peer"
        );

        // A negotiated peer that never sends a request is dropped silently.
        let mut silent = LocalConnection::connect(&endpoint).expect("fixture peer connects");
        negotiate_client(&mut silent, baseline_hello()).expect("baseline peer negotiates");
        drop(silent);

        // A session subscription command is answered from the durable
        // subscription surface instead of the ordinary command dispatch.
        assert!(matches!(
            send_payload(
                &endpoint,
                command_hello(),
                ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SubscribeSession(
                    SubscribeSessionCommandDto::new(
                        intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
                        session_id,
                        None,
                        RunModeDto::Build,
                    ),
                )),
            )
            .expect("subscription response arrives"),
            ProtocolResponsePayloadDto::Subscription(
                SessionSubscriptionResponseDto::SnapshotAndTail { .. }
            )
        ));

        // An ordinary command reaches the durable command dispatch.
        let wire_workspace = TempDir::new().expect("temporary workspace exists");
        let created = send_payload(
            &endpoint,
            command_hello(),
            ProtocolRequestPayloadDto::Command(ProtocolCommandDto::CreateSession(
                CreateSessionCommandDto::new(
                    ProjectId::new(),
                    SessionId::new(),
                    WorkspaceId::new(),
                    WorkspaceRootDto::parse(wire_workspace.path().to_string_lossy().into_owned())
                        .expect("fixture workspace is absolute"),
                    RunModeDto::Build,
                ),
            )),
        )
        .expect("command response arrives");
        assert!(
            matches!(
                created,
                ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(_))
            ),
            "ordinary command dispatch accepts the create-session command: {created:?}"
        );

        server.join().expect("fixture server completes");
    }

    #[tokio::test]
    async fn in_process_terminalizer_retries_a_rearmed_injection_through_the_durable_path() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = open_facade(&directory, Arc::new(CompletedDriver));
        seed_catalog(&facade);
        let workspace = TempDir::new().expect("temporary workspace exists");
        let session_id = create_session(&facade, workspace.path());
        let run_id = start_turn(&facade, session_id, "cancelled in-process fixture turn");

        let host = test_host_lifecycle(facade.clone());
        let endpoint = unique_endpoint();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("fixture listener binds");
        let server_host = host.clone();
        let server = tokio::spawn(async move {
            server_host.serve_connections(listener, 1).await;
        });

        // The unregistered Starting run is cancelled over the wire, so the host
        // owns the terminalization and its first durable step fails.
        host.inject_terminalizer_failure_once();
        let connection = AsyncLocalClientConnection::connect(&endpoint)
            .await
            .expect("stop peer connects");
        let (_remote, mut requests, mut responses) = connection
            .negotiate(command_hello())
            .await
            .expect("stop peer negotiates");
        let correlation = send_typed_request(
            &mut requests,
            ProtocolRequestPayloadDto::Command(ProtocolCommandDto::StopRun(
                StopRunCommandDto::new(session_id, run_id),
            )),
        )
        .await;
        let response = responses.receive().await.expect("stop response arrives");
        assert_eq!(response.correlation_id(), correlation);
        assert!(matches!(
            response.message().payload(),
            ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(accepted))
                if matches!(accepted.result(), ProtocolAcceptedResultDto::StopRun(_))
        ));
        server.await.expect("host accepts the stop peer");

        tokio::time::timeout(Duration::from_secs(2), host.wait_for_terminalizer_failure())
            .await
            .expect("the injected terminalizer failure is observed");
        assert_eq!(host.terminalizer_attempts(), 1);
        assert_eq!(host.task_count(), 1);

        // Re-arming the exact injection while the terminalizer is parked makes
        // the second attempt fail too, so the retry takes the rate-limited
        // delay before the third attempt commits the durable Cancelled state.
        host.inject_terminalizer_failure_once();
        host.release_terminalizer_retry();
        tokio::time::timeout(Duration::from_secs(2), host.wait_for_terminalizer_failure())
            .await
            .expect("the re-armed terminalizer failure is observed");
        assert_eq!(host.terminalizer_attempts(), 2);
        host.release_terminalizer_retry();
        tokio::time::timeout(
            Duration::from_secs(2),
            host.wait_for_terminalizer_completion(),
        )
        .await
        .expect("the released terminalizer reaches durable completion");
        assert_eq!(host.terminalizer_attempts(), 3);
        assert_eq!(host.task_count(), 0);
        assert_eq!(
            wait_for_terminal_run(&facade, session_id, run_id).await,
            RunStatusDto::Cancelled
        );
        host.shutdown().await;
    }

    #[tokio::test]
    async fn in_process_lifecycle_waits_for_registered_work_and_reports_unknown_completions() {
        let directory = TempDir::new().expect("temporary directory exists");
        let driver = Arc::new(BlockingDriver::new());
        let facade = open_facade(&directory, driver.clone());
        seed_catalog(&facade);
        let workspace = TempDir::new().expect("temporary workspace exists");
        let session_id = create_session(&facade, workspace.path());
        let run_id = start_turn(&facade, session_id, "blocked in-process fixture turn");
        let host = test_host_lifecycle(facade.clone());

        // The completion watch is exact: an unregistered run never signals.
        assert!(
            !host
                .wait_for_execution_completion(SessionId::new(), RunId::new())
                .await,
            "an unregistered run owns no completion watch"
        );

        host.admit_starting_run(session_id, run_id);
        tokio::time::timeout(Duration::from_secs(1), driver.entered.notified())
            .await
            .expect("the admitted execution reaches the driver");
        assert_eq!(host.task_count(), 1);

        // The registry still owns the blocked execution, so fixture cleanup
        // waits for the terminal commit before it returns.
        assert!(
            tokio::time::timeout(Duration::from_millis(50), host.wait_for_task_cleanup())
                .await
                .is_err(),
            "cleanup waits while the exact execution is registered"
        );
        driver.release.notify_one();
        tokio::time::timeout(Duration::from_secs(5), host.wait_for_task_cleanup())
            .await
            .expect("the released execution cleans its registry entry");
        assert!(
            host.wait_for_execution_completion(session_id, run_id).await,
            "the registered execution reports completion"
        );
        assert_eq!(host.task_count(), 0);
        assert_eq!(
            wait_for_terminal_run(&facade, session_id, run_id).await,
            RunStatusDto::Completed
        );
        host.shutdown().await;
    }

    #[tokio::test]
    async fn in_process_tool_executor_normalizes_every_projection_and_rejects_unknown_names() {
        let workspace = TempDir::new().expect("temporary workspace exists");
        std::fs::write(
            workspace.path().join("hello.txt"),
            "hello from in-process fixture",
        )
        .expect("workspace fixture writes");
        let large_bytes = 200 * 1024;
        std::fs::write(workspace.path().join("large.txt"), "x".repeat(large_bytes))
            .expect("workspace fixture writes");
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = open_facade(&directory, Arc::new(CompletedDriver));
        seed_catalog(&facade);
        let session_id = create_session(&facade, workspace.path());
        let run_id = start_turn(&facade, session_id, "in-process tool fixture turn");
        let executor = DaemonToolExecutor::new(facade.clone());

        let read = executor
            .execute_tool(
                session_id,
                run_id,
                ToolCallDto::new(ToolCallId::new(), "read", r#"{"path":"hello.txt"}"#)
                    .expect("fixture call is valid"),
            )
            .await
            .expect("the read tool result normalizes");
        assert!(matches!(
            read,
            ToolResultOutcomeDto::Succeeded { content } if content == "hello from in-process fixture"
        ));

        // A bounded read above the tool output bound carries the explicit
        // truncation marker in its normalized durable content.
        let truncated = executor
            .execute_tool(
                session_id,
                run_id,
                ToolCallDto::new(ToolCallId::new(), "read", r#"{"path":"large.txt"}"#)
                    .expect("fixture call is valid"),
            )
            .await
            .expect("the truncated read result normalizes");
        let ToolResultOutcomeDto::Succeeded { content } = truncated else {
            panic!("the truncated read succeeds with bounded content")
        };
        assert!(content.ends_with("[truncated]"));
        assert!(
            content.len() < large_bytes,
            "the truncated read stays within the tool output bound"
        );

        // Glob normalizes a bounded workspace-relative path list.
        let glob = executor
            .execute_tool(
                session_id,
                run_id,
                ToolCallDto::new(ToolCallId::new(), "glob", r#"{"pattern":"*.txt"}"#)
                    .expect("fixture call is valid"),
            )
            .await
            .expect("the glob tool result normalizes");
        let ToolResultOutcomeDto::Succeeded { content } = glob else {
            panic!("the glob result succeeds with a path list")
        };
        let paths: Vec<String> =
            serde_json::from_str(&content).expect("normalized glob content is a path list");
        assert!(paths.contains(&"hello.txt".to_owned()));
        assert!(paths.contains(&"large.txt".to_owned()));

        // Grep normalizes its typed matches.
        let grep = executor
            .execute_tool(
                session_id,
                run_id,
                ToolCallDto::new(
                    ToolCallId::new(),
                    "grep",
                    r#"{"pattern":"in-process","path":"hello.txt"}"#,
                )
                .expect("fixture call is valid"),
            )
            .await
            .expect("the grep tool result normalizes");
        let ToolResultOutcomeDto::Succeeded { content } = grep else {
            panic!("the grep result succeeds with typed matches")
        };
        let matches: Vec<serde_json::Value> =
            serde_json::from_str(&content).expect("normalized grep content is a match list");
        assert!(
            matches
                .iter()
                .any(|entry| entry["path"] == "hello.txt" && entry["line"] == 1),
            "the normalized grep match keeps its workspace-relative identity"
        );

        // Write normalizes to its byte-count mutation summary.
        let written = "written by the daemon tool path";
        let write = executor
            .execute_tool(
                session_id,
                run_id,
                ToolCallDto::new(
                    ToolCallId::new(),
                    "write",
                    serde_json::json!({"path": "written.txt", "content": written}).to_string(),
                )
                .expect("fixture call is valid"),
            )
            .await
            .expect("the write tool result normalizes");
        assert!(matches!(
            write,
            ToolResultOutcomeDto::Succeeded { content }
                if content == format!("{} bytes", written.len())
        ));

        // An unregistered tool name is a typed decode failure, never a silent
        // fallthrough to another tool.
        let error = executor
            .execute_tool(
                session_id,
                run_id,
                ToolCallDto::new(ToolCallId::new(), "not-a-registered-tool", "{}")
                    .expect("fixture call is valid"),
            )
            .await
            .expect_err("an unregistered tool name is rejected");
        assert_eq!(error.code(), "unknown_tool");
    }
}
