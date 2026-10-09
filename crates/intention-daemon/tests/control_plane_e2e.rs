//! Client-visible control-plane end-to-end tests over the real daemon transport.
//!
//! Each test spawns the real `intention-daemon` binary with an isolated
//! configuration home, state home, and endpoint, drives it through the real
//! asynchronous client, and serves its provider traffic from one fake
//! OpenAI-compatible provider: `GET {base}/models` answers the bounded model
//! listing that health and discovery consume, and
//! `POST {base}/chat/completions` answers one scripted SSE text round for turn
//! flows. The suite proves the control-plane outcomes a client can observe over
//! the transport: catalog list/status with page tokens, optimistic session
//! defaults, per-turn overrides, reload classification, credential rotation,
//! degraded read-only removal handling, non-authorizing probes, configuration
//! edits, and that committed M3/M4 runs stay readable and keep their persisted
//! selection while the control plane moves around them.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Control-plane end-to-end fixtures use assertion conveniences for precise diagnostics."
)]

mod common;

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use common::{config_path, spawn_daemon, unique_endpoint, write_config_document};
use intention_client::{
    IntentionClient, ProcessDaemonLauncher, RunStreamClient, RunStreamSubscription,
};
use intention_proto::provider::ProviderProfileRevisionId;
use intention_proto::{
    AcceptProviderCatalogRemovalCommandDto, ApplyConfigurationDocumentCommandDto,
    ApplyConfigurationEditsCommandDto, CatalogRevisionId, ClientRequestDto, ConfigurationEditDto,
    CreateSessionCommandDto, IdempotencyKey, ListProviderCatalogQueryDto, ProjectId,
    ProtocolResultDto, ProviderCatalogActivationStateDto, ProviderCatalogDegradedReasonDto,
    ProviderHealthStateDto, ProviderProfileId, ProviderProfileOverrideDto,
    ProviderProfileReadinessDto, RejectProviderCatalogCandidateCommandDto, RunId, RunModeDto,
    SendUserTurnOutcomeDto, SessionId, SetSessionProviderProfileCommandDto, WorkspaceId,
    WorkspaceRootDto, decode_response, encode_request, run_status_is_terminal,
};
use intention_transport::LocalEndpoint;
use tempfile::TempDir;

/// The bounded deadline for one daemon process to report readiness.
const READINESS_DEADLINE: Duration = Duration::from_secs(20);
/// The bounded deadline for one run to reach a terminal status.
const RUN_DEADLINE: Duration = Duration::from_secs(20);
/// The bounded deadline for one killed daemon process to exit.
const KILL_DEADLINE: Duration = Duration::from_secs(5);

/// The fixture global context window of the startup document.
const CONTEXT_WINDOW_TOKENS: u64 = 180_000;
/// The fixture global context window one accepted reload applies.
const WIDENED_CONTEXT_WINDOW_TOKENS: u64 = 240_000;

/// The stable fixture profile identities of the startup document.
const MAIN_PROFILE: &str = "main";
const ALTERNATE_PROFILE: &str = "alternate";
const DORMANT_PROFILE: &str = "dormant";

/// The exact fixture models of the startup document.
const MAIN_MODEL: &str = "fixture-model";
const ALTERNATE_MODEL: &str = "fixture-alternate-model";
const DORMANT_MODEL: &str = "fixture-dormant-model";

/// The fixture credentials of the startup document.
const MAIN_CREDENTIAL: &str = "fixture-main-credential";
const ALTERNATE_CREDENTIAL: &str = "fixture-alternate-credential";
const DORMANT_CREDENTIAL: &str = "fixture-dormant-credential";
/// The replacement private credential one rotation re-reads from the file.
const ROTATED_MAIN_CREDENTIAL: &str = "fixture-rotated-main-credential";

/// The assistant text every scripted fake-provider round streams.
const ASSISTANT_TEXT: &str = "control plane done";

/// One isolated daemon host with a fake OpenAI-compatible provider.
///
/// Dropping the host kills the daemon, stops the provider, and removes the
/// daemon-owned Unix socket so a later run can bind the same logical endpoint.
struct ControlPlaneHost {
    config_home: TempDir,
    state_home: TempDir,
    workspace: TempDir,
    provider: FakeProvider,
    endpoint: LocalEndpoint,
    daemon: Option<Child>,
    /// The startup configuration document, kept so tests can derive edited
    /// documents from exactly the bytes the daemon first loaded.
    document: String,
}

impl ControlPlaneHost {
    /// Creates a fresh isolated fixture and spawns its first daemon process.
    fn new() -> Self {
        let config_home = TempDir::new().expect("config directory exists");
        let state_home = TempDir::new().expect("state directory exists");
        let workspace = TempDir::new().expect("workspace directory exists");
        let provider = FakeProvider::start();
        let document = catalog_document(provider.port());
        write_config_document(config_home.path(), &document);
        let endpoint = unique_endpoint("control-plane");
        let daemon = spawn_daemon(&endpoint, config_home.path(), state_home.path(), None, &[]);
        Self {
            config_home,
            state_home,
            workspace,
            provider,
            endpoint,
            daemon: Some(daemon),
            document,
        }
    }

    /// Returns the fixture workspace root a session resolves against.
    fn workspace_root(&self) -> WorkspaceRootDto {
        WorkspaceRootDto::parse(self.workspace.path().to_string_lossy().into_owned())
            .expect("fixture workspace root is absolute")
    }

    /// Returns the daemon-resolved path of the private configuration file.
    fn config_path(&self) -> std::path::PathBuf {
        config_path(self.config_home.path())
    }

    /// Replaces the private configuration file with one edited document.
    fn rewrite_document(&self, document: &str) {
        write_config_document(self.config_home.path(), document);
    }

    /// Kills the current daemon and starts a fresh process with an edited file.
    fn restart_with_document(&mut self, document: &str) {
        self.kill_daemon();
        self.rewrite_document(document);
        self.daemon = Some(spawn_daemon(
            &self.endpoint,
            self.config_home.path(),
            self.state_home.path(),
            None,
            &[],
        ));
    }

    /// Kills the running daemon and waits for the process to exit.
    fn kill_daemon(&mut self) {
        let Some(mut child) = self.daemon.take() else {
            return;
        };
        let _ = child.kill();
        let deadline = Instant::now() + KILL_DEADLINE;
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

impl Drop for ControlPlaneHost {
    fn drop(&mut self) {
        self.kill_daemon();
        self.provider.stop();
        #[cfg(unix)]
        if let Some(path) = common::endpoint_socket_path(&self.endpoint) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Renders one provider profile table of the fixture document.
fn profile_table(
    profile_id: &str,
    model: &str,
    credential: &str,
    display_name: &str,
    enabled: bool,
    port: u16,
) -> String {
    format!(
        "\n[providers.profiles.{profile_id}]\n\
         kind = \"generic-chat-completion-api\"\n\
         model = \"{model}\"\n\
         endpoint = \"http://127.0.0.1:{port}/v1\"\n\
         credential = \"{credential}\"\n\
         display_name = \"{display_name}\"\n\
         enabled = {enabled}\n\
         \n[providers.profiles.{profile_id}.execution]\n\
         attempt_timeout_seconds = 30\n\
         max_attempts = 2\n\
         \n[providers.profiles.{profile_id}.capabilities]\n\
         text_streaming = true\n\
         tool_exchange = true\n"
    )
}

/// Renders the startup document: three profiles, one disabled, `main` default.
fn catalog_document(port: u16) -> String {
    let mut document = format!(
        "schema_version = 1\n\
         \n[provider]\n\
         context_window_tokens = {CONTEXT_WINDOW_TOKENS}\n\
         default_profile = \"{MAIN_PROFILE}\"\n"
    );
    document.push_str(&profile_table(
        MAIN_PROFILE,
        MAIN_MODEL,
        MAIN_CREDENTIAL,
        "Main",
        true,
        port,
    ));
    document.push_str(&profile_table(
        ALTERNATE_PROFILE,
        ALTERNATE_MODEL,
        ALTERNATE_CREDENTIAL,
        "Alternate",
        true,
        port,
    ));
    document.push_str(&profile_table(
        DORMANT_PROFILE,
        DORMANT_MODEL,
        DORMANT_CREDENTIAL,
        "Dormant",
        false,
        port,
    ));
    document
}

/// Renders the removal document: the exact `main` declaration, two profiles
/// omitted.
fn removal_document(port: u16) -> String {
    let mut document = format!(
        "schema_version = 1\n\
         \n[provider]\n\
         context_window_tokens = {CONTEXT_WINDOW_TOKENS}\n\
         default_profile = \"{MAIN_PROFILE}\"\n"
    );
    document.push_str(&profile_table(
        MAIN_PROFILE,
        MAIN_MODEL,
        MAIN_CREDENTIAL,
        "Main",
        true,
        port,
    ));
    document
}

/// Returns one fixture document with every `credential` line removed.
///
/// The edit surface is credential-free by contract; the daemon restores each
/// profile's retained private credential inside its own loading boundary.
fn without_credentials(document: &str) -> String {
    let mut stripped: String = document
        .lines()
        .filter(|line| !line.starts_with("credential = "))
        .collect::<Vec<_>>()
        .join("\n");
    stripped.push('\n');
    stripped
}

/// Returns one validated fixture profile identity.
fn fixture_profile(name: &str) -> ProviderProfileId {
    ProviderProfileId::parse(name).expect("fixture profile identity is valid")
}

/// Creates one fixture session through the real client.
async fn create_session(client: &IntentionClient, host: &ControlPlaneHost) -> SessionId {
    let session_id = SessionId::new();
    client
        .create_session(CreateSessionCommandDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            host.workspace_root(),
            RunModeDto::Build,
        ))
        .await
        .expect("the daemon accepts session creation");
    session_id
}

/// Starts one run through one user turn and returns the proposed run identity.
async fn start_run(
    client: &IntentionClient,
    session_id: SessionId,
    provider_profile: Option<ProviderProfileOverrideDto>,
    content: &str,
) -> RunId {
    let outcome = match provider_profile {
        Some(override_profile) => client
            .send_user_turn_with_profile(
                session_id,
                IdempotencyKey::new(),
                content.to_owned(),
                override_profile,
            )
            .await
            .expect("the user turn is accepted"),
        None => client
            .send_user_turn(session_id, IdempotencyKey::new(), content.to_owned())
            .await
            .expect("the user turn is accepted"),
    };
    match outcome {
        SendUserTurnOutcomeDto::Started { run_id, .. } => run_id,
        SendUserTurnOutcomeDto::Pending => panic!("a first turn starts a run, got a pending turn"),
    }
}

/// Waits for the spawned daemon to report ready through the shared client.
///
/// `await_ready()` only connects and queries; it never launches the daemon.
/// Its own bounded wait is looped under the fixture deadline, so a daemon that
/// needs longer than one client budget still becomes ready in time.
async fn wait_until_ready(endpoint: &LocalEndpoint, deadline: Instant) -> IntentionClient {
    let client = IntentionClient::new(
        endpoint.clone(),
        Box::new(
            ProcessDaemonLauncher::new(env!("CARGO_BIN_EXE_intention-daemon"))
                .expect("daemon program is valid"),
        ),
    );
    while Instant::now() < deadline {
        if client.await_ready().await.is_ok() {
            return client;
        }
    }
    panic!("daemon becomes ready before the deadline");
}

/// Subscribes to one run and drives its live stream until it is terminal.
///
/// The correlated first reply is the current-state snapshot, so a run that
/// already terminalized is observed without any frame; every later committed
/// status change arrives as a live frame on the same subscription.
async fn subscribe_until_terminal(
    stream_client: &RunStreamClient,
    session_id: SessionId,
    run_id: RunId,
    deadline: Instant,
) -> RunStreamSubscription {
    let mut subscription = stream_client
        .subscribe(intention_proto::SubscribeRunCommandDto::new(
            session_id, run_id,
        ))
        .await
        .expect("run subscription arrives");
    while !subscription
        .state()
        .status()
        .is_some_and(run_status_is_terminal)
    {
        assert!(
            Instant::now() < deadline,
            "the run reaches a terminal status before the deadline"
        );
        match tokio::time::timeout(Duration::from_secs(1), subscription.receive()).await {
            Ok(Ok(Some(_))) => {}
            Ok(Ok(None)) => {
                panic!("the daemon closed the run stream before the run terminalized")
            }
            Ok(Err(error)) => panic!("run stream frame error: {}", error.code()),
            Err(_) => {}
        }
    }
    subscription
}

/// Dispatches one raw protocol request over a fresh client connection.
///
/// The typed client hides protocol result variants, so the named-result
/// assertions of the edit surface travel over the wire directly.
async fn send_raw_request(
    endpoint: &LocalEndpoint,
    request: ClientRequestDto,
) -> ProtocolResultDto {
    use intention_transport::AsyncLocalClientConnection;

    let connection = AsyncLocalClientConnection::connect(endpoint)
        .await
        .expect("the raw client connects");
    let (mut sender, mut receiver) = connection.split();
    sender
        .send_message(&encode_request(1, request))
        .await
        .expect("the raw request sends");
    let line = receiver
        .receive_line()
        .await
        .expect("the raw reply arrives");
    decode_response(&line, 1).expect("the raw reply decodes")
}

/// Asserts that one serialized control-plane value never carries credentials.
fn assert_credential_free(encoded: &str, label: &str) {
    assert!(
        !encoded.contains(MAIN_CREDENTIAL)
            && !encoded.contains(ALTERNATE_CREDENTIAL)
            && !encoded.contains(DORMANT_CREDENTIAL)
            && !encoded.contains(ROTATED_MAIN_CREDENTIAL),
        "{label} never discloses provider credential material: {encoded}"
    );
}

/// A fake OpenAI-compatible provider serving a model listing and one SSE round.
///
/// `GET {base}/models` answers a bounded model listing that the production
/// health probe and discovery attempt consume; `POST {base}/chat/completions`
/// answers one complete text round for every run. Every request is counted and
/// its `Authorization` header recorded, so a test can prove the daemon made no
/// provider call and that a rotated credential reached the wire.
struct FakeProvider {
    port: u16,
    chat_requests: Arc<AtomicUsize>,
    model_requests: Arc<AtomicUsize>,
    other_requests: Arc<AtomicUsize>,
    last_authorization: Arc<Mutex<Option<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl FakeProvider {
    /// Binds one loopback listener and starts serving its scripted answers.
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("provider binds");
        let port = listener
            .local_addr()
            .expect("provider port is available")
            .port();
        let chat_requests = Arc::new(AtomicUsize::new(0));
        let model_requests = Arc::new(AtomicUsize::new(0));
        let other_requests = Arc::new(AtomicUsize::new(0));
        let last_authorization = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_chat = Arc::clone(&chat_requests);
        let thread_models = Arc::clone(&model_requests);
        let thread_other = Arc::clone(&other_requests);
        let thread_authorization = Arc::clone(&last_authorization);
        let thread_stop = Arc::clone(&stop);
        let models_json = serde_json::to_string(&serde_json::json!({
            "object": "list",
            "data": [
                {"id": MAIN_MODEL, "object": "model", "created": 1, "owned_by": "fixture"},
                {"id": ALTERNATE_MODEL, "object": "model", "created": 1, "owned_by": "fixture"},
            ],
        }))
        .expect("model listing serializes");
        let chat_response = sse_response(&format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            serde_json::to_string(&serde_json::json!({
                "id": "chatcmpl-control-plane",
                "object": "chat.completion.chunk",
                "created": 1,
                "model": MAIN_MODEL,
                "choices": [{
                    "index": 0,
                    "delta": {"content": ASSISTANT_TEXT},
                    "finish_reason": "stop",
                }],
            }))
            .expect("chat chunk serializes"),
            serde_json::to_string(&serde_json::json!({
                "id": "chatcmpl-control-plane",
                "object": "chat.completion.chunk",
                "created": 1,
                "model": MAIN_MODEL,
                "choices": [],
                "usage": {"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5},
            }))
            .expect("usage chunk serializes"),
        ));
        let thread = thread::Builder::new()
            .name("control-plane-provider".to_owned())
            .spawn(move || {
                listener
                    .set_nonblocking(true)
                    .expect("provider listener is non-blocking");
                while !thread_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => handle_provider_connection(
                            stream,
                            &thread_chat,
                            &thread_models,
                            &thread_other,
                            &thread_authorization,
                            &models_json,
                            &chat_response,
                        ),
                        Err(_) => thread::sleep(Duration::from_millis(10)),
                    }
                }
            })
            .expect("provider thread starts");
        Self {
            port,
            chat_requests,
            model_requests,
            other_requests,
            last_authorization,
            stop,
            thread: Some(thread),
        }
    }

    /// Returns the bound loopback port.
    const fn port(&self) -> u16 {
        self.port
    }

    /// Returns how many chat-completion rounds were served.
    fn chat_request_count(&self) -> usize {
        self.chat_requests.load(Ordering::Acquire)
    }

    /// Returns how many model-listing requests were served.
    fn model_request_count(&self) -> usize {
        self.model_requests.load(Ordering::Acquire)
    }

    /// Returns how many requests matched no scripted route.
    fn other_request_count(&self) -> usize {
        self.other_requests.load(Ordering::Acquire)
    }

    /// Returns the `Authorization` header of the newest served request.
    fn last_authorization(&self) -> Option<String> {
        self.last_authorization
            .lock()
            .expect("provider authorization gate is available")
            .clone()
    }

    /// Stops the accept loop and joins its thread.
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Serves one fake-provider connection by its request line.
fn handle_provider_connection(
    mut stream: TcpStream,
    chat_requests: &AtomicUsize,
    model_requests: &AtomicUsize,
    other_requests: &AtomicUsize,
    last_authorization: &Mutex<Option<String>>,
    models_json: &str,
    chat_response: &str,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Some(request) = read_provider_request(&mut stream) else {
        return;
    };
    if let Some(authorization) = request.authorization {
        *last_authorization
            .lock()
            .expect("provider authorization gate is available") = Some(authorization);
    }
    let path = request
        .target
        .split('?')
        .next()
        .unwrap_or(request.target.as_str())
        .trim_end_matches('/');
    if request.method == "GET" && path.ends_with("/models") {
        model_requests.fetch_add(1, Ordering::AcqRel);
        write_response(&mut stream, &json_response(models_json));
    } else if request.method == "POST" && path.ends_with("/chat/completions") {
        chat_requests.fetch_add(1, Ordering::AcqRel);
        write_response(&mut stream, chat_response);
    } else {
        other_requests.fetch_add(1, Ordering::AcqRel);
        write_response(
            &mut stream,
            &json_response_error(r#"{"error":{"message":"unexpected provider request"}}"#),
        );
    }
}

/// One parsed fake-provider request line and its authorization header.
struct ParsedProviderRequest {
    method: String,
    target: String,
    authorization: Option<String>,
}

/// Reads one complete HTTP request head and body and parses its request line.
fn read_provider_request(stream: &mut TcpStream) -> Option<ParsedProviderRequest> {
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return None,
            Ok(read) => {
                raw.extend_from_slice(&buffer[..read]);
                if let Some(end) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                    let length = content_length(&raw[..end]);
                    let body_start = end + 4;
                    let available = raw.len().saturating_sub(body_start);
                    if available < length {
                        let mut remaining = vec![0_u8; length - available];
                        if stream.read_exact(&mut remaining).is_err() {
                            return None;
                        }
                        raw.extend_from_slice(&remaining);
                    }
                    return parse_provider_request(&raw[..body_start]);
                }
            }
            Err(_) => return None,
        }
    }
}

/// Parses one request head into its method, target, and authorization header.
fn parse_provider_request(head: &[u8]) -> Option<ParsedProviderRequest> {
    let head = String::from_utf8_lossy(head);
    let mut lines = head.lines();
    let mut request_line = lines.next()?.split_whitespace();
    let method = request_line.next()?.to_owned();
    let target = request_line.next()?.to_owned();
    let authorization = lines.find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("authorization")
            .then(|| value.trim().to_owned())
    });
    Some(ParsedProviderRequest {
        method,
        target,
        authorization,
    })
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

/// Writes one complete HTTP response and closes nothing explicitly: dropping
/// the stream after the write ends the connection.
fn write_response(stream: &mut TcpStream, response: &str) {
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn sse_response(body: &str) -> String {
    format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}")
}

fn json_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn json_response_error(body: &str) -> String {
    format!(
        "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn catalog_list_and_status_project_the_active_revision() {
    let host = ControlPlaneHost::new();
    let client = wait_until_ready(&host.endpoint, Instant::now() + READINESS_DEADLINE).await;

    let status = client
        .provider_catalog_status()
        .await
        .expect("catalog status reads");
    assert_eq!(
        status.activation_state(),
        ProviderCatalogActivationStateDto::Active,
        "the startup catalog is active"
    );
    assert_eq!(status.degraded_reason(), None);
    assert!(status.candidate().is_none(), "no candidate is pending");
    assert!(status.validation_issues().is_empty());
    assert_eq!(
        status.default_profile_id(),
        Some(&fixture_profile(MAIN_PROFILE))
    );
    let active_revision = status
        .active_catalog_revision_id()
        .expect("an active catalog names its revision");
    assert_credential_free(
        &serde_json::to_string(&status).expect("the catalog status serializes"),
        "the catalog status",
    );

    let page = client
        .list_provider_catalog(ListProviderCatalogQueryDto::new(None).expect("empty query"))
        .await
        .expect("catalog page reads");
    assert_eq!(page.catalog_revision_id(), Some(active_revision));
    assert_eq!(
        page.default_profile_id(),
        Some(&fixture_profile(MAIN_PROFILE))
    );
    assert!(!page.has_more(), "one small catalog fits one page");
    assert!(page.next_page_token().is_none());
    let entries: Vec<(&str, ProviderProfileReadinessDto)> = page
        .entries()
        .iter()
        .map(|entry| (entry.profile_id().as_str(), entry.readiness()))
        .collect();
    assert_eq!(
        entries,
        vec![
            (ALTERNATE_PROFILE, ProviderProfileReadinessDto::Ready),
            (DORMANT_PROFILE, ProviderProfileReadinessDto::Disabled),
            (MAIN_PROFILE, ProviderProfileReadinessDto::Ready),
        ],
        "entries are ordered by stable profile identity with their local readiness"
    );
    assert!(
        page.entries()
            .iter()
            .all(|entry| entry.credential_configured()),
        "every fixture profile has a configured private credential"
    );
    assert_credential_free(
        &serde_json::to_string(&page).expect("the catalog page serializes"),
        "the catalog page",
    );

    // A well-formed token names the last entry of a page: the continuation
    // starts after that entry on the same active revision.
    let continuation = format!("{active_revision}:{ALTERNATE_PROFILE}");
    let next = client
        .list_provider_catalog(
            ListProviderCatalogQueryDto::new(Some(continuation)).expect("token shape is safe"),
        )
        .await
        .expect("a same-revision token continues the active page");
    let next_entries: Vec<&str> = next
        .entries()
        .iter()
        .map(|entry| entry.profile_id().as_str())
        .collect();
    assert_eq!(next_entries, vec![DORMANT_PROFILE, MAIN_PROFILE]);
    assert_eq!(next.catalog_revision_id(), Some(active_revision));
    assert!(!next.has_more());

    // A token naming the final entry yields the (empty) tail of the page.
    let tail = client
        .list_provider_catalog(
            ListProviderCatalogQueryDto::new(Some(format!("{active_revision}:{MAIN_PROFILE}")))
                .expect("token shape is safe"),
        )
        .await
        .expect("the tail token continues the active page");
    assert!(tail.entries().is_empty());
    assert!(!tail.has_more());

    // An undecodable token is rejected by the daemon, not the client.
    let error = client
        .list_provider_catalog(
            ListProviderCatalogQueryDto::new(Some("not-a-decoded-token".to_owned()))
                .expect("token shape is safe"),
        )
        .await
        .expect_err("a malformed token fails");
    assert_eq!(error.code(), "provider_catalog_page_token_invalid");

    // A well-formed token naming a profile outside the revision is malformed.
    let error = client
        .list_provider_catalog(
            ListProviderCatalogQueryDto::new(Some(format!("{active_revision}:ghost")))
                .expect("token shape is safe"),
        )
        .await
        .expect_err("an unknown profile token fails");
    assert_eq!(error.code(), "provider_catalog_page_token_invalid");

    // A token from another catalog revision is a typed stale read.
    let foreign = format!("{}:{MAIN_PROFILE}", CatalogRevisionId::new());
    let error = client
        .list_provider_catalog(
            ListProviderCatalogQueryDto::new(Some(foreign)).expect("token shape is safe"),
        )
        .await
        .expect_err("a foreign revision token fails");
    assert_eq!(error.code(), "provider_catalog_changed");
}

#[tokio::test]
async fn session_default_changes_are_optimistic_and_observable() {
    let host = ControlPlaneHost::new();
    let client = wait_until_ready(&host.endpoint, Instant::now() + READINESS_DEADLINE).await;
    let session_id = create_session(&client, &host).await;
    let main = fixture_profile(MAIN_PROFILE);
    let alternate = fixture_profile(ALTERNATE_PROFILE);

    let initial = client
        .session_provider_profile(session_id)
        .await
        .expect("the session profile projection reads");
    assert_eq!(initial.durable_profile_id(), None);
    assert_eq!(
        initial.unavailability(),
        Some(intention_proto::ProviderSelectionUnavailabilityDto::Missing),
        "a session without a durable default reports the closed missing reason"
    );
    assert_eq!(initial.global_default_profile_id(), Some(&main));
    let baseline_revision = initial.session_projection_revision();

    let accepted = client
        .set_session_provider_profile(SetSessionProviderProfileCommandDto::new(
            session_id,
            alternate.clone(),
            baseline_revision,
            IdempotencyKey::new(),
        ))
        .await
        .expect("a real session default change is accepted");
    assert!(accepted.changed());
    assert!(
        accepted.session_projection_revision() > baseline_revision,
        "a real change advances the durable session projection revision"
    );

    let projection = client
        .session_provider_profile(session_id)
        .await
        .expect("the changed session profile projection reads");
    assert_eq!(projection.durable_profile_id(), Some(&alternate));
    assert_eq!(
        projection
            .resolved_entry()
            .map(intention_proto::ProviderProfileEntryDto::profile_id),
        Some(&alternate)
    );
    assert_eq!(projection.unavailability(), None);
    assert_eq!(
        projection.session_projection_revision(),
        accepted.session_projection_revision(),
        "the projection carries the committed revision"
    );

    // A turn without an override resolves the durable session default.
    let stream_client = RunStreamClient::new(host.endpoint.clone());
    let run_id = start_run(&client, session_id, None, "session default turn").await;
    let subscription = subscribe_until_terminal(
        &stream_client,
        session_id,
        run_id,
        Instant::now() + RUN_DEADLINE,
    )
    .await;
    assert_eq!(
        subscription
            .state()
            .run()
            .expect("the subscription carries the run scope")
            .provider_profile_id(),
        Some(&alternate),
        "the accepted run resolves the durable session default"
    );
    assert_eq!(
        subscription.state().status(),
        Some(intention_proto::RunStatusDto::Completed)
    );

    // Setting the same profile again is an accepted no-change outcome that
    // leaves the durable revision untouched.
    let repeated = client
        .set_session_provider_profile(SetSessionProviderProfileCommandDto::new(
            session_id,
            alternate.clone(),
            accepted.session_projection_revision(),
            IdempotencyKey::new(),
        ))
        .await
        .expect("setting the same session default is accepted");
    assert!(!repeated.changed());
    assert_eq!(
        repeated.session_projection_revision(),
        accepted.session_projection_revision()
    );
    let projection = client
        .session_provider_profile(session_id)
        .await
        .expect("the unchanged session profile projection reads");
    assert_eq!(projection.durable_profile_id(), Some(&alternate));
    assert_eq!(
        projection.session_projection_revision(),
        accepted.session_projection_revision(),
        "a no-op change does not bump the projection revision"
    );
}

#[tokio::test]
async fn turn_profile_overrides_are_honored_or_fail_typed_before_any_provider_call() {
    let host = ControlPlaneHost::new();
    let client = wait_until_ready(&host.endpoint, Instant::now() + READINESS_DEADLINE).await;
    let session_id = create_session(&client, &host).await;
    let main = fixture_profile(MAIN_PROFILE);
    let dormant = fixture_profile(DORMANT_PROFILE);

    // A pinned override on the happy path is the run's recorded selection.
    let stream_client = RunStreamClient::new(host.endpoint.clone());
    let run_id = start_run(
        &client,
        session_id,
        Some(ProviderProfileOverrideDto::new(main.clone(), None)),
        "override turn",
    )
    .await;
    let subscription = subscribe_until_terminal(
        &stream_client,
        session_id,
        run_id,
        Instant::now() + RUN_DEADLINE,
    )
    .await;
    assert_eq!(
        subscription
            .state()
            .run()
            .expect("the subscription carries the run scope")
            .provider_profile_id(),
        Some(&main),
        "the accepted run carries the turn override"
    );
    assert_eq!(
        subscription.state().status(),
        Some(intention_proto::RunStatusDto::Completed)
    );
    assert_eq!(host.provider.chat_request_count(), 1);

    // A stale expected revision is rejected before any commit.
    let error = client
        .send_user_turn_with_profile(
            session_id,
            IdempotencyKey::new(),
            "stale override turn".to_owned(),
            ProviderProfileOverrideDto::new(main.clone(), Some(ProviderProfileRevisionId::new())),
        )
        .await
        .expect_err("a stale expected revision fails");
    assert_eq!(error.code(), "provider_profile_revision_mismatch");

    // A declared but disabled profile has no live runtime entry.
    let error = client
        .send_user_turn_with_profile(
            session_id,
            IdempotencyKey::new(),
            "unavailable override turn".to_owned(),
            ProviderProfileOverrideDto::new(dormant, None),
        )
        .await
        .expect_err("an unavailable override fails");
    assert_eq!(error.code(), "provider_profile_runtime_unavailable");

    // Neither rejection called the provider or committed a run.
    assert_eq!(
        host.provider.chat_request_count(),
        1,
        "a rejected override never reaches the provider"
    );
    assert_eq!(host.provider.model_request_count(), 0);
    assert_eq!(host.provider.other_request_count(), 0);
    let snapshot = client
        .session_snapshot(session_id)
        .await
        .expect("the session snapshot reads");
    assert_eq!(
        snapshot.messages().len(),
        2,
        "only the accepted run committed transcript rows"
    );
    assert_eq!(
        snapshot
            .messages()
            .iter()
            .map(intention_proto::MessageProjectionDto::kind)
            .collect::<Vec<_>>(),
        vec![
            intention_proto::MessageKindDto::User,
            intention_proto::MessageKindDto::Assistant,
        ]
    );
    assert_eq!(
        snapshot.messages()[1].text(),
        ASSISTANT_TEXT,
        "the scripted assistant step is durable"
    );
}

#[tokio::test]
async fn reload_applies_context_window_only_and_rejects_catalog_changes() {
    let host = ControlPlaneHost::new();
    let client = wait_until_ready(&host.endpoint, Instant::now() + READINESS_DEADLINE).await;
    let catalog_revision = client
        .provider_catalog_status()
        .await
        .expect("catalog status reads")
        .active_catalog_revision_id();

    // A semantically equal reload is a no-op success.
    let equal = client
        .reload_configuration(IdempotencyKey::new())
        .await
        .expect("an equal reload succeeds");
    assert_eq!(equal.catalog_revision_id(), catalog_revision);
    let unchanged_revision = equal.config_revision_id();

    // A context-window-only change commits a fresh configuration revision.
    let widened = host.document.replace(
        &format!("context_window_tokens = {CONTEXT_WINDOW_TOKENS}"),
        &format!("context_window_tokens = {WIDENED_CONTEXT_WINDOW_TOKENS}"),
    );
    host.rewrite_document(&widened);
    let changed = client
        .reload_configuration(IdempotencyKey::new())
        .await
        .expect("a context-window-only reload succeeds");
    assert_ne!(changed.config_revision_id(), unchanged_revision);
    assert_eq!(
        changed.catalog_revision_id(),
        catalog_revision,
        "the active catalog revision is untouched"
    );

    // The daemon keeps serving fresh runs after the reload.
    let session_id = create_session(&client, &host).await;
    let stream_client = RunStreamClient::new(host.endpoint.clone());
    let run_id = start_run(&client, session_id, None, "post-reload turn").await;
    let subscription = subscribe_until_terminal(
        &stream_client,
        session_id,
        run_id,
        Instant::now() + RUN_DEADLINE,
    )
    .await;
    assert_eq!(
        subscription.state().status(),
        Some(intention_proto::RunStatusDto::Completed)
    );
    assert_eq!(host.provider.chat_request_count(), 1);

    // A catalog-affecting change is rejected and commits nothing.
    let catalog_changed = widened.replace(MAIN_MODEL, "changed-fixture-model");
    host.rewrite_document(&catalog_changed);
    let error = client
        .reload_configuration(IdempotencyKey::new())
        .await
        .expect_err("a catalog-affecting reload is refused");
    assert_eq!(error.code(), "catalog_change_requires_restart");

    // Restoring the accepted document proves the rejection committed nothing:
    // the accepted configuration revision is still the widened one.
    host.rewrite_document(&widened);
    let restored = client
        .reload_configuration(IdempotencyKey::new())
        .await
        .expect("the accepted document still reloads as a no-op");
    assert_eq!(restored.config_revision_id(), changed.config_revision_id());
    assert_eq!(restored.catalog_revision_id(), catalog_revision);
}

#[tokio::test]
async fn credential_rotation_rereads_the_file_and_requires_frozen_meaning() {
    let host = ControlPlaneHost::new();
    let client = wait_until_ready(&host.endpoint, Instant::now() + READINESS_DEADLINE).await;
    let session_id = create_session(&client, &host).await;
    let main = fixture_profile(MAIN_PROFILE);

    // The startup credential reaches the provider.
    let evidence = client
        .check_provider_health(main.clone())
        .await
        .expect("the health probe answers");
    assert_eq!(evidence.state(), ProviderHealthStateDto::Available);
    assert_eq!(
        host.provider.last_authorization(),
        Some(format!("Bearer {MAIN_CREDENTIAL}"))
    );
    assert_eq!(host.provider.model_request_count(), 1);

    // Only the credential changed in the file: the rotation is accepted.
    let rotated_document = host
        .document
        .replace(MAIN_CREDENTIAL, ROTATED_MAIN_CREDENTIAL);
    host.rewrite_document(&rotated_document);
    let accepted = client
        .rotate_provider_credential(main.clone(), IdempotencyKey::new())
        .await
        .expect("a credential-only rotation succeeds");
    assert_eq!(accepted.profile_id(), &main);
    assert_credential_free(
        &serde_json::to_string(&accepted).expect("the rotation acceptance serializes"),
        "the rotation acceptance",
    );

    // The rebuilt private client serves health and a real run with the new
    // credential on the wire.
    let evidence = client
        .check_provider_health(main.clone())
        .await
        .expect("the post-rotation health probe answers");
    assert_eq!(evidence.state(), ProviderHealthStateDto::Available);
    assert_eq!(
        host.provider.last_authorization(),
        Some(format!("Bearer {ROTATED_MAIN_CREDENTIAL}")),
        "the rebuilt client applies the re-read credential"
    );

    let stream_client = RunStreamClient::new(host.endpoint.clone());
    let run_id = start_run(&client, session_id, None, "post-rotation turn").await;
    let subscription = subscribe_until_terminal(
        &stream_client,
        session_id,
        run_id,
        Instant::now() + RUN_DEADLINE,
    )
    .await;
    assert_eq!(
        subscription.state().status(),
        Some(intention_proto::RunStatusDto::Completed)
    );
    assert_eq!(
        host.provider.last_authorization(),
        Some(format!("Bearer {ROTATED_MAIN_CREDENTIAL}"))
    );
    assert_eq!(host.provider.chat_request_count(), 1);

    // A frozen-meaning mismatch refuses the rotation.
    let mismatched = rotated_document.replace(MAIN_MODEL, "rotated-fixture-model");
    host.rewrite_document(&mismatched);
    let error = client
        .rotate_provider_credential(main, IdempotencyKey::new())
        .await
        .expect_err("a changed model refuses rotation");
    assert_eq!(error.code(), "credential_rotation_frozen_meaning_mismatch");
    assert!(
        !error.message().contains(ROTATED_MAIN_CREDENTIAL),
        "a rotation failure never discloses credential material"
    );
}

#[tokio::test]
async fn removal_candidate_degrades_read_only_until_it_is_accepted() {
    let mut host = ControlPlaneHost::new();
    let client = wait_until_ready(&host.endpoint, Instant::now() + READINESS_DEADLINE).await;
    let session_id = create_session(&client, &host).await;
    let active_revision = client
        .provider_catalog_status()
        .await
        .expect("catalog status reads")
        .active_catalog_revision_id()
        .expect("an active catalog names its revision");

    // Restarting against a document that omits two accepted profiles creates
    // the one process-local pending removal candidate.
    host.restart_with_document(&removal_document(host.provider.port()));
    let client = wait_until_ready(&host.endpoint, Instant::now() + READINESS_DEADLINE).await;
    let status = client
        .provider_catalog_status()
        .await
        .expect("degraded catalog status reads");
    assert_eq!(
        status.activation_state(),
        ProviderCatalogActivationStateDto::PendingRemoval
    );
    assert_eq!(
        status.degraded_reason(),
        Some(ProviderCatalogDegradedReasonDto::RemovalCandidatePending)
    );
    let handle = status
        .candidate()
        .expect("a pending removal names its exact candidate");
    assert_eq!(handle.expected_active_revision_id(), active_revision);
    assert_eq!(status.active_catalog_revision_id(), Some(active_revision));
    assert_credential_free(
        &serde_json::to_string(&status).expect("the pending-removal status serializes"),
        "the pending-removal status",
    );

    // Reads still serve the active revision.
    let page = client
        .list_provider_catalog(ListProviderCatalogQueryDto::new(None).expect("empty query"))
        .await
        .expect("the read-only catalog page serves");
    assert_eq!(page.catalog_revision_id(), Some(active_revision));

    // Admission and provider state changes are refused while degraded.
    let error = client
        .send_user_turn(
            session_id,
            IdempotencyKey::new(),
            "degraded turn".to_owned(),
        )
        .await
        .expect_err("admission is refused while degraded");
    assert_eq!(error.code(), "execution_not_ready");
    let error = client
        .set_session_provider_profile(SetSessionProviderProfileCommandDto::new(
            session_id,
            fixture_profile(MAIN_PROFILE),
            0,
            IdempotencyKey::new(),
        ))
        .await
        .expect_err("default changes are refused while degraded");
    assert_eq!(error.code(), "execution_not_ready");

    // Accepting the exact candidate commits it and restores readiness.
    let accepted = client
        .accept_provider_catalog_removal(AcceptProviderCatalogRemovalCommandDto::new(
            handle,
            IdempotencyKey::new(),
        ))
        .await
        .expect("the pending removal accepts");
    assert_eq!(
        accepted.catalog_revision_id(),
        handle.candidate_revision_id()
    );
    assert_credential_free(
        &serde_json::to_string(&accepted).expect("the removal acceptance serializes"),
        "the removal acceptance",
    );
    let status = client
        .provider_catalog_status()
        .await
        .expect("the restored catalog status reads");
    assert_eq!(
        status.activation_state(),
        ProviderCatalogActivationStateDto::Active
    );
    assert_eq!(status.degraded_reason(), None);
    assert!(status.candidate().is_none());
    assert_eq!(
        status.active_catalog_revision_id(),
        Some(handle.candidate_revision_id())
    );

    let page = client
        .list_provider_catalog(ListProviderCatalogQueryDto::new(None).expect("empty query"))
        .await
        .expect("the accepted catalog page reads");
    let remaining: Vec<&str> = page
        .entries()
        .iter()
        .map(|entry| entry.profile_id().as_str())
        .collect();
    assert_eq!(remaining, vec![MAIN_PROFILE]);

    // Admission resumes through the promoted candidate registry.
    let stream_client = RunStreamClient::new(host.endpoint.clone());
    let run_id = start_run(&client, session_id, None, "post-removal turn").await;
    let subscription = subscribe_until_terminal(
        &stream_client,
        session_id,
        run_id,
        Instant::now() + RUN_DEADLINE,
    )
    .await;
    assert_eq!(
        subscription.state().status(),
        Some(intention_proto::RunStatusDto::Completed)
    );
    assert_eq!(host.provider.chat_request_count(), 1);
}

#[tokio::test]
async fn rejected_removal_candidate_stays_degraded() {
    let mut host = ControlPlaneHost::new();
    let client = wait_until_ready(&host.endpoint, Instant::now() + READINESS_DEADLINE).await;
    let session_id = create_session(&client, &host).await;
    let active_revision = client
        .provider_catalog_status()
        .await
        .expect("catalog status reads")
        .active_catalog_revision_id()
        .expect("an active catalog names its revision");

    host.restart_with_document(&removal_document(host.provider.port()));
    let client = wait_until_ready(&host.endpoint, Instant::now() + READINESS_DEADLINE).await;
    let handle = client
        .provider_catalog_status()
        .await
        .expect("degraded catalog status reads")
        .candidate()
        .expect("a pending removal names its exact candidate");

    let rejected = client
        .reject_provider_catalog_candidate(RejectProviderCatalogCandidateCommandDto::new(
            handle,
            IdempotencyKey::new(),
        ))
        .await
        .expect("the pending candidate rejects");
    assert_eq!(rejected.active_catalog_revision_id(), Some(active_revision));

    // The rejection drops the candidate and keeps the daemon degraded until a
    // restart; the active revision is unchanged.
    let status = client
        .provider_catalog_status()
        .await
        .expect("the rejected catalog status reads");
    assert_eq!(
        status.activation_state(),
        ProviderCatalogActivationStateDto::Active
    );
    assert_eq!(
        status.degraded_reason(),
        Some(ProviderCatalogDegradedReasonDto::RemovalCandidateRejected)
    );
    assert!(status.candidate().is_none());
    assert_eq!(status.active_catalog_revision_id(), Some(active_revision));

    let error = client
        .send_user_turn(
            session_id,
            IdempotencyKey::new(),
            "rejected turn".to_owned(),
        )
        .await
        .expect_err("admission stays refused after a rejection");
    assert_eq!(error.code(), "execution_not_ready");

    // The rejected candidate cannot be accepted afterwards.
    let error = client
        .accept_provider_catalog_removal(AcceptProviderCatalogRemovalCommandDto::new(
            handle,
            IdempotencyKey::new(),
        ))
        .await
        .expect_err("a rejected candidate is gone");
    assert_eq!(error.code(), "provider_catalog_changed");
    assert_eq!(host.provider.chat_request_count(), 0);
}

#[tokio::test]
async fn probes_are_typed_non_authorizing_evidence() {
    let host = ControlPlaneHost::new();
    let client = wait_until_ready(&host.endpoint, Instant::now() + READINESS_DEADLINE).await;
    let session_id = create_session(&client, &host).await;
    let main = fixture_profile(MAIN_PROFILE);
    let before = client
        .session_snapshot(session_id)
        .await
        .expect("the session snapshot reads");

    // One bounded health probe: typed evidence, one provider request, no retry.
    let evidence = client
        .check_provider_health(main.clone())
        .await
        .expect("the health probe answers");
    assert_eq!(evidence.provider_id(), &main);
    assert_eq!(evidence.state(), ProviderHealthStateDto::Available);
    assert_eq!(evidence.reason(), None);
    assert_credential_free(
        &serde_json::to_string(&evidence).expect("the health evidence serializes"),
        "the health evidence",
    );
    assert_eq!(host.provider.model_request_count(), 1);

    // One bounded discovery attempt: validated records and a durable attempt
    // identity, with no automatic continuation.
    let discovered = client
        .discover_provider_models(main.clone())
        .await
        .expect("the discovery attempt answers");
    let model_ids: Vec<&str> = discovered
        .records()
        .iter()
        .map(intention_proto::ProviderModelRecordDto::model_id)
        .collect();
    assert_eq!(model_ids, vec![MAIN_MODEL, ALTERNATE_MODEL]);
    assert_credential_free(
        &serde_json::to_string(&discovered).expect("the discovery result serializes"),
        "the discovery result",
    );
    assert_eq!(
        host.provider.model_request_count(),
        2,
        "each probe makes exactly one bounded provider request"
    );
    assert_eq!(host.provider.chat_request_count(), 0);
    assert_eq!(host.provider.other_request_count(), 0);

    // A repeated attempt carries its own durable attempt identity.
    let repeated = client
        .discover_provider_models(main)
        .await
        .expect("a repeated discovery attempt answers");
    assert_ne!(
        repeated.attempt_id(),
        discovered.attempt_id(),
        "every attempt is its own durable record"
    );
    assert_eq!(host.provider.model_request_count(), 3);

    // Neither probe is authoritative: no run, selection, or session state moved.
    let after = client
        .session_snapshot(session_id)
        .await
        .expect("the session snapshot re-reads");
    assert_eq!(
        after.projection().session_projection_revision(),
        before.projection().session_projection_revision()
    );
    assert_eq!(
        after.projection().provider_profile_id(),
        before.projection().provider_profile_id()
    );
    assert!(after.projection().active_run().is_none());
    assert_eq!(after.messages().to_vec(), before.messages().to_vec());
    assert!(after.messages().is_empty());
}

#[tokio::test]
async fn configuration_edits_reply_their_results_and_keep_the_file_credentialed() {
    let host = ControlPlaneHost::new();
    let client = wait_until_ready(&host.endpoint, Instant::now() + READINESS_DEADLINE).await;
    let alternate = fixture_profile(ALTERNATE_PROFILE);

    // Applying a credential-free document that only widens the global context
    // window takes effect without a restart.
    let context_window_only = without_credentials(&host.document).replace(
        &format!("context_window_tokens = {CONTEXT_WINDOW_TOKENS}"),
        &format!("context_window_tokens = {WIDENED_CONTEXT_WINDOW_TOKENS}"),
    );
    assert!(!context_window_only.contains(MAIN_CREDENTIAL));
    let applied = client
        .apply_configuration_document(context_window_only.clone(), IdempotencyKey::new())
        .await
        .expect("a credential-free document applies");
    assert!(
        !applied.requires_restart(),
        "a context-window-only document applies to fresh runs immediately"
    );
    assert_credential_free(
        &serde_json::to_string(&applied).expect("the document acceptance serializes"),
        "the document acceptance",
    );

    // The wire names the document result: the same credential-free candidate
    // dispatched raw answers `ConfigurationDocumentApplied` without a restart.
    let dispatched = send_raw_request(
        &host.endpoint,
        ClientRequestDto::ApplyConfigurationDocument(
            ApplyConfigurationDocumentCommandDto::new(context_window_only, IdempotencyKey::new())
                .expect("the command shape is valid"),
        ),
    )
    .await;
    let ProtocolResultDto::ConfigurationDocumentApplied(raw_applied) = dispatched else {
        panic!("a document apply answers with its named result");
    };
    assert!(!raw_applied.requires_restart());
    assert_credential_free(
        &serde_json::to_string(&raw_applied).expect("the raw document acceptance serializes"),
        "the raw document acceptance",
    );

    // The written file stays valid and keeps every private credential.
    let written = fs::read_to_string(host.config_path()).expect("the configuration file reads");
    assert!(written.contains(&format!(
        "context_window_tokens = {WIDENED_CONTEXT_WINDOW_TOKENS}"
    )));
    assert!(written.contains(&format!("credential = \"{MAIN_CREDENTIAL}\"")));
    assert!(written.contains(&format!("credential = \"{ALTERNATE_CREDENTIAL}\"")));
    assert!(written.contains(&format!("credential = \"{DORMANT_CREDENTIAL}\"")));

    // Reloading the file the daemon wrote is a semantically equal no-op, which
    // proves the file is a valid document of the committed configuration.
    let reloaded = client
        .reload_configuration(IdempotencyKey::new())
        .await
        .expect("the written document reloads as an equal candidate");
    assert_eq!(
        reloaded.config_revision_id(),
        raw_applied.config_revision_id()
    );

    // A typed edit of startup-applied catalog state reports its restart
    // requirement and keeps the file credential-bearing.
    let edits = vec![
        ConfigurationEditDto::set_profile_display_name(alternate.clone(), "Renamed Alternate")
            .expect("the edit is valid"),
    ];
    let edited = client
        .apply_configuration_edits(edits.clone(), IdempotencyKey::new())
        .await
        .expect("the typed edit applies");
    assert!(
        edited.requires_restart(),
        "a display-name edit is startup-applied catalog state"
    );
    assert_ne!(
        edited.config_revision_id(),
        raw_applied.config_revision_id()
    );
    assert_credential_free(
        &serde_json::to_string(&edited).expect("the edit acceptance serializes"),
        "the edit acceptance",
    );

    // The wire names the edit result too.
    let dispatched = send_raw_request(
        &host.endpoint,
        ClientRequestDto::ApplyConfigurationEdits(
            ApplyConfigurationEditsCommandDto::new(edits, IdempotencyKey::new())
                .expect("the command shape is valid"),
        ),
    )
    .await;
    let ProtocolResultDto::ConfigurationEditsApplied(raw_edited) = dispatched else {
        panic!("a typed edit answers with its named result");
    };
    assert!(raw_edited.requires_restart());
    assert_credential_free(
        &serde_json::to_string(&raw_edited).expect("the raw edit acceptance serializes"),
        "the raw edit acceptance",
    );

    let written = fs::read_to_string(host.config_path()).expect("the configuration file re-reads");
    assert!(written.contains("display_name = \"Renamed Alternate\""));
    assert!(written.contains(&format!("credential = \"{MAIN_CREDENTIAL}\"")));

    // The startup-applied edit is exactly why the reload surface now refuses:
    // the file differs from the active catalog in a catalog-affecting way.
    let error = client
        .reload_configuration(IdempotencyKey::new())
        .await
        .expect_err("the startup-applied edit is not live");
    assert_eq!(error.code(), "catalog_change_requires_restart");

    // A document carrying credential material is refused without disclosure.
    let error = client
        .apply_configuration_document(host.document.clone(), IdempotencyKey::new())
        .await
        .expect_err("a credential-bearing document is refused");
    assert_eq!(error.code(), "configuration_edit_contains_credential");
    assert!(
        !error.message().contains(MAIN_CREDENTIAL),
        "the rejection never discloses credential material"
    );
}

#[tokio::test]
async fn control_plane_calls_preserve_committed_runs_and_selections() {
    let host = ControlPlaneHost::new();
    let client = wait_until_ready(&host.endpoint, Instant::now() + READINESS_DEADLINE).await;
    let session_id = create_session(&client, &host).await;
    let main = fixture_profile(MAIN_PROFILE);
    let alternate = fixture_profile(ALTERNATE_PROFILE);
    let stream_client = RunStreamClient::new(host.endpoint.clone());

    // One committed run under the global default.
    let first_run = start_run(&client, session_id, None, "first turn").await;
    let first = subscribe_until_terminal(
        &stream_client,
        session_id,
        first_run,
        Instant::now() + RUN_DEADLINE,
    )
    .await;
    assert_eq!(
        first
            .state()
            .run()
            .expect("the subscription carries the run scope")
            .provider_profile_id(),
        Some(&main)
    );
    let first_messages = first.state().messages().to_vec();
    assert_eq!(first_messages.len(), 2);

    // Control-plane traffic interleaves with the committed run.
    let _ = client
        .list_provider_catalog(ListProviderCatalogQueryDto::new(None).expect("empty query"))
        .await
        .expect("the catalog page reads");
    let _ = client
        .provider_catalog_status()
        .await
        .expect("the catalog status reads");
    let _ = client
        .reload_configuration(IdempotencyKey::new())
        .await
        .expect("the equal reload succeeds");
    let _ = client
        .check_provider_health(main.clone())
        .await
        .expect("the health probe answers");
    let _ = client
        .discover_provider_models(main.clone())
        .await
        .expect("the discovery attempt answers");
    client
        .set_session_provider_profile(SetSessionProviderProfileCommandDto::new(
            session_id,
            alternate.clone(),
            0,
            IdempotencyKey::new(),
        ))
        .await
        .expect("the session default changes");
    let edits = vec![
        ConfigurationEditDto::set_profile_display_name(alternate.clone(), "Renamed Alternate")
            .expect("the edit is valid"),
    ];
    let _ = client
        .apply_configuration_edits(edits, IdempotencyKey::new())
        .await
        .expect("the typed edit applies");

    // The committed run stays readable, unchanged, and keeps its own selection:
    // the later control-plane moves never synthesize a new one for it.
    let snapshot = client
        .session_snapshot(session_id)
        .await
        .expect("the session snapshot reads");
    assert_eq!(snapshot.messages().to_vec(), first_messages);
    let replayed = subscribe_until_terminal(
        &stream_client,
        session_id,
        first_run,
        Instant::now() + RUN_DEADLINE,
    )
    .await;
    assert_eq!(replayed.state().messages().to_vec(), first_messages);
    assert_eq!(
        replayed.state().status(),
        Some(intention_proto::RunStatusDto::Completed)
    );
    assert_eq!(
        replayed
            .state()
            .run()
            .expect("the subscription carries the run scope")
            .provider_profile_id(),
        Some(&main),
        "the committed run keeps the selection it was admitted with"
    );

    // The changed session default applies to fresh runs only.
    let second_run = start_run(&client, session_id, None, "second turn").await;
    let second = subscribe_until_terminal(
        &stream_client,
        session_id,
        second_run,
        Instant::now() + RUN_DEADLINE,
    )
    .await;
    assert_eq!(
        second
            .state()
            .run()
            .expect("the subscription carries the run scope")
            .provider_profile_id(),
        Some(&alternate),
        "a fresh run resolves the changed session default"
    );
    let replayed_first = subscribe_until_terminal(
        &stream_client,
        session_id,
        first_run,
        Instant::now() + RUN_DEADLINE,
    )
    .await;
    assert_eq!(
        replayed_first
            .state()
            .run()
            .expect("the first run still resolves")
            .provider_profile_id(),
        Some(&main)
    );

    // Exactly the two admitted runs reached the provider; nothing re-executed.
    assert_eq!(host.provider.chat_request_count(), 2);
    assert_eq!(host.provider.other_request_count(), 0);
}
