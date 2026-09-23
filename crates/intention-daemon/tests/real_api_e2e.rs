//! Opt-in live-provider end-to-end tests for the real daemon binary.
//!
//! These tests spawn the real `intention-daemon` binary, drive it through the
//! real local transport, and execute the real production model-tool loop
//! against a live provider API over HTTPS. They prove that a real model tool
//! call executes through the typed tool registry, that its `ToolCallRecorded`
//! and `ToolResultRecorded` facts are durable, and that a hard daemon restart
//! replays the same run without re-executing the tool or disclosing the
//! provider credential.
//!
//! This file is the single opt-in live channel recorded by ADR 0040
//! (`docs/intention-relay/decisions/0040-opt-in-live-provider-e2e.md`). It is
//! never part of the hermetic gates: both tests carry `#[ignore]` and return
//! silently unless `INTENTION_REAL_API_E2E=1`, so an ordinary test run,
//! including `--run-ignored all`, never fails without the operator opt-in.
//!
//! Run one live pass with:
//!
//! ```text
//! INTENTION_REAL_API_E2E=1 INTENTION_REAL_API_KEY=... INTENTION_REAL_API_MODEL=... \
//!     cargo nextest run --locked -p intention-daemon --test real_api_e2e --run-ignored only
//! ```
//!
//! or with the dedicated local entry point:
//!
//! ```text
//! INTENTION_REAL_API_KEY=... INTENTION_REAL_API_MODEL=... make e2e-real-api
//! ```
//!
//! `INTENTION_REAL_API_KIND` selects `generic-chat-completion-api` (default) or
//! `openrouter`. `INTENTION_REAL_API_ENDPOINT` is an optional override for the
//! generic-chat endpoint and defaults to `https://api.openai.com/v1`; the
//! OpenRouter selection never writes an endpoint into the daemon
//! configuration. The credential comes only from `INTENTION_REAL_API_KEY`,
//! lives only in a private temporary configuration file, and is never printed
//! by the test.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Opt-in live-provider end-to-end fixtures use assertion conveniences for precise diagnostics."
)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use intention_client::{IntentionClient, ProcessDaemonLauncher, RunStreamClient};
use intention_config::{
    ConfigPathDto, ConfigSourceDto, ProviderKindDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    CreateSessionCommandDto, ModelRunFactDto, ModelRunFactInputDto, RunModeDto, RunSnapshotDto,
    RunStatusDto, SendUserTurnCommandDto, ToolResultOutcomeDto, WorkspaceRootDto,
};
use intention_protocol::{
    DaemonReadinessDto, ProtocolAcceptedResultDto, ProtocolCapabilityDto, ProtocolCommandDto,
    ProtocolCommandResultDto, ProtocolDaemonFrameDto, ProtocolHelloDto, ProtocolMessageDto,
    ProtocolRequestEnvelopeDto, ProtocolRequestPayloadDto, ProtocolResponsePayloadDto,
    RunStreamFrameDto, RunSubscriptionRequestEnvelopeDto, RunSubscriptionResponseDto,
    SendUserTurnOutcomeDto, SubscribeRunCommandDto,
};
use intention_transport::{
    AsyncLocalClientConnection, LocalConnection, LocalEndpoint, local_protocol_version,
    negotiate_client,
};
use intention_types::{
    CorrelationIdDto, DtoResult, ErrorDto, ProjectId, RunId, SessionId, TurnId, WorkspaceId,
};
use tempfile::TempDir;

/// The generic-chat endpoint used when the operator does not override it.
const DEFAULT_GENERIC_CHAT_ENDPOINT: &str = "https://api.openai.com/v1";

/// The per-turn deadline for one live provider run: two provider attempts with
/// the configured 60-second attempt timeout plus real tool execution.
const TURN_DEADLINE: Duration = Duration::from_secs(180);

/// The readiness deadline for one daemon process start.
const READINESS_DEADLINE: Duration = Duration::from_secs(30);

/// The bounded window that proves a completed durable run commits no new facts.
const REPLAY_QUIET_WINDOW: Duration = Duration::from_secs(1);

/// The bounded window used to observe the post-restart replay.
const REPLAY_DEADLINE: Duration = Duration::from_secs(15);

/// The maximum number of sequential live turns the tool-loop test accepts.
const MAX_TURNS: u8 = 2;

/// Environment variables that must never reach the spawned daemon.
///
/// The live-provider variables are test-process inputs only, and the provider
/// SDK credential fallbacks must never substitute for the fixture
/// configuration.
const DAEMON_REMOVED_ENVIRONMENT: [&str; 7] = [
    "INTENTION_REAL_API_E2E",
    "INTENTION_REAL_API_KEY",
    "INTENTION_REAL_API_MODEL",
    "INTENTION_REAL_API_KIND",
    "INTENTION_REAL_API_ENDPOINT",
    "OPENAI_API_KEY",
    "OPENROUTER_API_KEY",
];

/// The opt-in live-provider selection resolved from the process environment.
struct LiveProviderConfig {
    kind: ProviderKindDto,
    model: String,
    credential: String,
    endpoint: Option<String>,
}

impl LiveProviderConfig {
    /// Reads the live-provider environment, or `None` unless
    /// `INTENTION_REAL_API_E2E` is exactly `1`.
    ///
    /// # Panics
    ///
    /// Panics naming only the missing variable when the gate is enabled but
    /// `INTENTION_REAL_API_KEY` or `INTENTION_REAL_API_MODEL` is missing or
    /// blank. No credential value is ever formatted.
    fn from_env() -> Option<Self> {
        if std::env::var("INTENTION_REAL_API_E2E").ok().as_deref() != Some("1") {
            return None;
        }
        let kind = match std::env::var("INTENTION_REAL_API_KIND").ok().as_deref() {
            None | Some("") | Some("generic-chat-completion-api") => {
                ProviderKindDto::GenericChatCompletionApi
            }
            Some("openrouter") => ProviderKindDto::Openrouter,
            Some(_) => panic!(
                "INTENTION_REAL_API_KIND must be 'generic-chat-completion-api' or 'openrouter'"
            ),
        };
        let endpoint = match kind {
            // The OpenRouter adapter owns its base URL: the frozen live
            // contract writes no endpoint for that selection, even when the
            // optional override is present in the environment.
            ProviderKindDto::Openrouter => None,
            ProviderKindDto::GenericChatCompletionApi => Some(
                std::env::var("INTENTION_REAL_API_ENDPOINT")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .map_or_else(
                        || DEFAULT_GENERIC_CHAT_ENDPOINT.to_owned(),
                        |value| value.trim().to_owned(),
                    ),
            ),
        };
        Some(Self {
            kind,
            model: required_live_environment("INTENTION_REAL_API_MODEL"),
            credential: required_live_environment("INTENTION_REAL_API_KEY"),
            endpoint,
        })
    }

    /// Renders the version-1 daemon configuration document for this selection.
    ///
    /// The document is never printed or logged. The credential is escaped for
    /// a TOML basic string and rejected when it carries control characters.
    ///
    /// # Panics
    ///
    /// Panics when a configured value carries a control character. The message
    /// never includes the value itself.
    fn config_document(&self) -> String {
        let credential = escape_toml_basic_string(&self.credential);
        let mut lines = vec![
            "schema_version = 1".to_owned(),
            "[provider]".to_owned(),
            format!("kind = \"{}\"", self.kind.as_str()),
            format!("model = \"{}\"", escape_toml_basic_string(&self.model)),
        ];
        if let Some(endpoint) = self.endpoint.as_deref() {
            lines.push(format!(
                "endpoint = \"{}\"",
                escape_toml_basic_string(endpoint)
            ));
        }
        lines.push(format!("credential = \"{credential}\""));
        lines.push("[provider.execution]".to_owned());
        lines.push("attempt_timeout_seconds = 60".to_owned());
        lines.push("max_attempts = 2".to_owned());
        let joined = lines.join("\n");
        format!("{joined}\n")
    }
}

/// Returns one required live-provider environment value.
///
/// # Panics
///
/// Panics with the variable name only when the value is missing or blank.
fn required_live_environment(name: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => panic!("{name} must be set when INTENTION_REAL_API_E2E=1"),
    }
}

/// Escapes one value for a TOML basic string and rejects control characters.
///
/// # Panics
///
/// Panics when the value carries a control character. The message never
/// includes the value itself.
fn escape_toml_basic_string(value: &str) -> String {
    assert!(
        !value.chars().any(char::is_control),
        "a live provider configuration value carries a control character"
    );
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' | '\\' => {
                escaped.push('\\');
                escaped.push(character);
            }
            _ => escaped.push(character),
        }
    }
    escaped
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

/// Resolves the document through the production configuration parser and
/// asserts the safe projection matches the requested selection.
///
/// The document passes through opaque configuration input only and is never
/// printed, even on failure.
fn preflight_config_document(path: &Path, provider: &LiveProviderConfig, document: &str) {
    let source = ConfigSourceDto::Explicit(
        ConfigPathDto::parse(path.to_string_lossy().into_owned())
            .expect("the fixture configuration path is absolute"),
    );
    let resolved =
        ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(document.to_owned(), source))
            .unwrap_or_else(|_| panic!("the live provider configuration document resolves"));
    assert_eq!(
        resolved.provider().kind(),
        provider.kind,
        "the configuration selects the requested provider kind"
    );
    assert_eq!(
        resolved.provider().model(),
        provider.model.as_str(),
        "the configuration selects the requested model"
    );
    assert_eq!(
        resolved.provider().endpoint(),
        provider.endpoint.as_deref(),
        "the configuration carries the expected endpoint"
    );
    assert!(
        resolved.provider().credential_configured(),
        "the configuration carries a credential without exposing it"
    );
    assert_eq!(
        resolved.provider_execution().attempt_timeout_seconds(),
        60,
        "the live configuration bounds each provider attempt"
    );
    assert_eq!(
        resolved.provider_execution().max_attempts(),
        2,
        "the live configuration allows at most two provider attempts"
    );
}

/// Writes the daemon configuration file with owner-only permissions on Unix.
fn write_config(config_home: &Path, provider: &LiveProviderConfig) {
    let config_path = config_path(config_home);
    let document = provider.config_document();
    preflight_config_document(&config_path, provider, &document);
    let parent = config_path.parent().expect("config path has a parent");
    std::fs::create_dir_all(parent).expect("config directory is created");
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut options = std::fs::OpenOptions::new();
        options.create(true).write(true).truncate(true).mode(0o600);
        let mut file = options.open(&config_path).expect("config file opens");
        file.write_all(document.as_bytes())
            .expect("config file writes");
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&config_path, document.as_bytes()).expect("config file writes");
    }
}

static NEXT_ENDPOINT: AtomicUsize = AtomicUsize::new(0);

/// Builds a unique safe endpoint instance id for this test process.
fn unique_endpoint() -> LocalEndpoint {
    let sequence = NEXT_ENDPOINT.fetch_add(1, Ordering::Relaxed);
    LocalEndpoint::from_instance_id(format!("real-api-e2e-{}-{}", std::process::id(), sequence))
        .expect("live e2e endpoint is valid")
}

/// Spawns the real daemon binary with per-process environment overrides and
/// both output streams redirected into the fixture log file.
///
/// The daemon never inherits the live-provider variables or the provider SDK
/// credential fallbacks, and it never inherits stdout/stderr, so the capture
/// file is the only place its text can land.
fn spawn_daemon(
    endpoint: &LocalEndpoint,
    config_home: &Path,
    state_home: &Path,
    log_path: &Path,
) -> Child {
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .expect("daemon log file opens");
    let log_errors = log.try_clone().expect("daemon log file clones");
    let mut command = Command::new(env!("CARGO_BIN_EXE_intention-daemon"));
    command.arg(endpoint.instance_id());
    command.stdout(Stdio::from(log));
    command.stderr(Stdio::from(log_errors));
    for variable in DAEMON_REMOVED_ENVIRONMENT {
        command.env_remove(variable);
    }
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

/// One live-provider fixture: isolated config/state/workspace directories, one
/// spawned daemon process, and its private capture log.
///
/// Dropping the fixture kills the daemon and removes the daemon-owned Unix
/// socket so a later run can bind the same logical endpoint.
struct LiveE2eHost {
    config_home: TempDir,
    state_home: TempDir,
    workspace: TempDir,
    credential: String,
    daemon: Option<Child>,
    endpoint: LocalEndpoint,
    log_path: PathBuf,
}

impl LiveE2eHost {
    /// Creates a fresh isolated fixture, writes its provider configuration,
    /// and spawns its first daemon process.
    fn new(provider: &LiveProviderConfig, workspace_file: Option<(&str, &str)>) -> Self {
        let config_home = TempDir::new().expect("config directory exists");
        let state_home = TempDir::new().expect("state directory exists");
        let workspace = TempDir::new().expect("workspace directory exists");
        if let Some((name, content)) = workspace_file {
            std::fs::write(workspace.path().join(name), content).expect("workspace fixture writes");
        }
        write_config(config_home.path(), provider);
        let endpoint = unique_endpoint();
        let log_path = config_home.path().join("daemon-live-e2e.log");
        let daemon = spawn_daemon(&endpoint, config_home.path(), state_home.path(), &log_path);
        Self {
            config_home,
            state_home,
            workspace,
            credential: provider.credential.clone(),
            daemon: Some(daemon),
            endpoint,
            log_path,
        }
    }

    /// Kills the current daemon and starts a fresh process with identical
    /// environment, state directories, and endpoint.
    ///
    /// The kill is a hard kill, so the daemon cannot run its listener Drop and
    /// its Unix socket file survives; the transport reclaims the stale socket
    /// on the next bind, exactly like the hermetic facade fixture.
    fn restart_daemon(&mut self) {
        self.kill_daemon();
        self.daemon = Some(spawn_daemon(
            &self.endpoint,
            self.config_home.path(),
            self.state_home.path(),
            &self.log_path,
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

    /// Returns the captured daemon stdout/stderr text.
    fn captured_log(&self) -> String {
        let bytes = std::fs::read(&self.log_path).expect("daemon log reads");
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl Drop for LiveE2eHost {
    fn drop(&mut self) {
        self.kill_daemon();
        #[cfg(unix)]
        if let Some(path) = endpoint_socket_path(&self.endpoint) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Polls the daemon health projection until it reports `Ready`.
///
/// `health()` only negotiates and queries; it never launches the daemon. A
/// child that exits before readiness fails with its exit status instead of a
/// readiness timeout.
fn wait_until_ready(host: &mut LiveE2eHost, deadline: Instant) -> IntentionClient {
    let client = IntentionClient::new(
        host.endpoint.clone(),
        "real-api-e2e",
        Box::new(
            ProcessDaemonLauncher::new(env!("CARGO_BIN_EXE_intention-daemon"))
                .expect("daemon program is valid"),
        ),
    )
    .expect("live e2e client is valid");
    while Instant::now() < deadline {
        let exit_status = host
            .daemon
            .as_mut()
            .and_then(|child| child.try_wait().ok().flatten());
        assert!(
            exit_status.is_none(),
            "the daemon exits before readiness with status {exit_status:?}"
        );
        match client.health() {
            Ok(health) if health.readiness() == DaemonReadinessDto::Ready => return client,
            Ok(_) | Err(_) => {}
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("daemon becomes ready before the deadline");
}

/// The exact capability list the fixture command client needs from the daemon.
fn command_hello() -> ProtocolHelloDto {
    ProtocolHelloDto::new(
        local_protocol_version(),
        vec![
            ProtocolCapabilityDto::SessionSubscriptions,
            ProtocolCapabilityDto::CorrelatedRequests,
            ProtocolCapabilityDto::DaemonHealth,
        ],
        "real-api-e2e",
    )
    .expect("fixture command hello is valid")
}

/// The exact capability list the fixture run-stream client needs from the daemon.
fn stream_hello() -> ProtocolHelloDto {
    ProtocolHelloDto::new(
        local_protocol_version(),
        vec![ProtocolCapabilityDto::RunStreamSubscriptions],
        "real-api-e2e",
    )
    .expect("fixture stream hello is valid")
}

fn invalid_response() -> ErrorDto {
    ErrorDto::validation(
        "invalid_local_protocol_response",
        "the local daemon returned an unexpected protocol response",
    )
}

/// Sends one typed protocol command over a fresh negotiated connection and
/// verifies the correlated response, replicating the shared client's private
/// request path with public transport and protocol APIs only.
fn send_command(
    endpoint: &LocalEndpoint,
    payload: ProtocolRequestPayloadDto,
) -> DtoResult<ProtocolCommandResultDto> {
    let mut connection = LocalConnection::connect(endpoint)?;
    let remote = negotiate_client(&mut connection, command_hello())?;
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
    match response.message().payload() {
        ProtocolResponsePayloadDto::CommandResult(result) => Ok(result.clone()),
        _ => Err(invalid_response()),
    }
}

/// Creates one Build-mode session rooted at the fixture workspace.
fn create_session(endpoint: &LocalEndpoint, session_id: SessionId, workspace: &Path) {
    let created = send_command(
        endpoint,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::CreateSession(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                WorkspaceRootDto::parse(workspace.to_string_lossy().into_owned())
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
}

/// Sends one user turn and returns the run it started.
fn send_user_turn(endpoint: &LocalEndpoint, session_id: SessionId, content: &str) -> RunId {
    let result = send_command(
        endpoint,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, TurnId::new(), content).expect("turn is valid"),
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
        panic!("the user turn starts a run, got: {turn:?}")
    };
    run_id
}

/// One observed terminal run: the delivered durable facts and the
/// authoritative terminal snapshot.
struct ObservedRun {
    facts: Vec<ModelRunFactDto>,
    snapshot: RunSnapshotDto,
}

impl ObservedRun {
    /// Returns the authoritative terminal run status.
    const fn status(&self) -> RunStatusDto {
        self.snapshot.run_projection().status()
    }

    /// Returns the authoritative assistant content accumulated by the run.
    fn assistant_content(&self) -> &str {
        self.snapshot.projection().assistant_content()
    }

    /// Returns the safe terminal failure code recorded by the run, if failed.
    fn failure_code(&self) -> Option<&str> {
        self.snapshot
            .projection()
            .failure()
            .map(|failure| failure.code())
    }

    /// Reports whether the delivered facts form one non-empty contiguous
    /// cursor range ending in the terminal fact at the snapshot cursor.
    fn facts_end_at_snapshot_cursor(&self) -> bool {
        !self.facts.is_empty()
            && self
                .facts
                .windows(2)
                .all(|pair| pair[0].cursor().value() + 1 == pair[1].cursor().value())
            && self.facts.last().is_some_and(|fact| {
                matches!(
                    fact.input(),
                    ModelRunFactInputDto::Finished { .. } | ModelRunFactInputDto::Failed { .. }
                ) && fact.cursor() == self.snapshot.cursor()
            })
    }

    /// Reports whether any durable `read` tool call was recorded.
    fn has_read_tool_call(&self) -> bool {
        self.facts.iter().any(|fact| {
            matches!(
                fact.input(),
                ModelRunFactInputDto::ToolCallRecorded { call } if call.name() == "read"
            )
        })
    }

    /// Reports whether any durable tool result was recorded.
    fn has_tool_result(&self) -> bool {
        self.facts.iter().any(|fact| {
            matches!(
                fact.input(),
                ModelRunFactInputDto::ToolResultRecorded { .. }
            )
        })
    }

    /// Reports whether the observed run satisfies every live tool-loop
    /// acceptance condition.
    fn accepts_tool_loop(&self) -> bool {
        self.status() == RunStatusDto::Completed
            && self.facts_end_at_snapshot_cursor()
            && matches!(
                self.facts.last().map(ModelRunFactDto::input),
                Some(ModelRunFactInputDto::Finished { .. })
            )
            && self.has_read_tool_call()
            && self.has_tool_result()
            && self
                .assistant_content()
                .to_ascii_uppercase()
                .contains("READY")
    }

    /// Reports whether a durable `read` tool result for a recorded `read` call
    /// succeeded with content carrying the fixture token.
    fn read_result_carries_token(&self, token: &str) -> bool {
        self.facts.iter().any(|result| {
            let ModelRunFactInputDto::ToolResultRecorded {
                call_id,
                outcome: ToolResultOutcomeDto::Succeeded { content },
            } = result.input()
            else {
                return false;
            };
            content.contains(token)
                && self.facts.iter().any(|call| {
                    matches!(
                        call.input(),
                        ModelRunFactInputDto::ToolCallRecorded { call }
                            if call.name() == "read" && call.call_id() == *call_id
                    )
                })
        })
    }
}

/// Subscribes to one run and collects every delivered durable fact plus the
/// authoritative terminal snapshot.
///
/// The daemon replays an empty tail on subscribe and delivers facts only as
/// live batches while the run commits them, so a subscriber can observe a
/// suffix of the run's facts but never a gap inside that suffix. A quiet
/// receive window is retried until the deadline instead of failing, because a
/// real provider can pause for several seconds inside one attempt.
async fn collect_terminal_run(
    endpoint: &LocalEndpoint,
    session_id: SessionId,
    run_id: RunId,
    deadline: Instant,
) -> ObservedRun {
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
        let frame = match tokio::time::timeout(Duration::from_secs(1), frames.receive()).await {
            Ok(Ok(frame)) => frame,
            Ok(Err(error)) => panic!("run stream frame error: {}", error.code()),
            Err(_) => continue,
        };
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
                            return ObservedRun { facts, snapshot };
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
                    return ObservedRun { facts, snapshot };
                }
            }
            ProtocolDaemonFrameDto::RunStream(RunStreamFrameDto::Resync(resync)) => {
                panic!("unexpected run resync: {:?}", resync.reason());
            }
        }
    }
}

/// Asserts one text surface never discloses the credential value.
///
/// The assertion message names only the surface label, never the credential.
fn assert_excludes_credential(surface: &str, credential: &str, label: &str) {
    assert!(
        !surface.contains(credential),
        "the {label} never discloses the provider credential"
    );
}

/// Reports whether the byte buffer contains the credential value.
fn bytes_contain_credential(bytes: &[u8], credential: &str) -> bool {
    bytes
        .windows(credential.len())
        .any(|window| window == credential.as_bytes())
}

/// Asserts no file under the state directory contains the credential bytes.
///
/// The assertion message names only the file path, never the credential.
fn assert_state_directory_excludes_credential(directory: &Path, credential: &str) {
    let mut pending = vec![directory.to_path_buf()];
    while let Some(current) = pending.pop() {
        let entries = std::fs::read_dir(&current).expect("state directory is readable");
        for entry in entries {
            let path = entry.expect("state entry is readable").path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let bytes = std::fs::read(&path).expect("state file is readable");
            assert!(
                !bytes_contain_credential(&bytes, credential),
                "the state file never discloses the provider credential: {}",
                path.display()
            );
        }
    }
}

/// Proves the real production tool loop against a live provider, then proves
/// the durable run replays across a hard daemon restart.
///
/// The run is accepted only when it completed, delivered a contiguous fact
/// suffix ending in `Finished` at the snapshot cursor, recorded a `read` tool
/// call and a tool result, and accumulated a `READY` reply. A completed run
/// that misses those conditions consumes the next sequential turn; a failed
/// run or a timeout fails the test without a test-level retry.
#[tokio::test]
#[ignore = "opt-in live-provider e2e; see ADR 0040; run via make e2e-real-api"]
async fn real_provider_tool_loop_completes_and_replays_after_restart() {
    let Some(provider) = LiveProviderConfig::from_env() else {
        return;
    };
    let token = format!("real-e2e-note-{}", std::process::id());
    let note = format!("live provider tool-loop note: {token}\n");
    let mut host = LiveE2eHost::new(&provider, Some(("e2e-note.txt", note.as_str())));
    let credential = host.credential.clone();
    let _client = wait_until_ready(&mut host, Instant::now() + READINESS_DEADLINE);

    let session_id = SessionId::new();
    create_session(&host.endpoint, session_id, host.workspace.path());

    let mut accepted: Option<(RunId, ObservedRun)> = None;
    let mut last_observed: Option<ObservedRun> = None;
    for attempt in 1..=MAX_TURNS {
        let run_id = send_user_turn(
            &host.endpoint,
            session_id,
            "Use the read tool with the relative path e2e-note.txt to read that workspace file, then reply with exactly READY.",
        );
        let observed = collect_terminal_run(
            &host.endpoint,
            session_id,
            run_id,
            Instant::now() + TURN_DEADLINE,
        )
        .await;
        assert_eq!(
            observed.status(),
            RunStatusDto::Completed,
            "live provider turn {attempt} terminated with failure code {:?}",
            observed.failure_code()
        );
        if observed.accepts_tool_loop() {
            accepted = Some((run_id, observed));
            break;
        }
        last_observed = Some(observed);
    }
    let Some((run_id, observed)) = accepted else {
        let last = last_observed.expect("two live turns are observed");
        panic!(
            "the live provider tool loop did not satisfy the acceptance conditions within two turns: status={:?} facts={} read_call={} tool_result={} ready_reply={}",
            last.status(),
            last.facts.len(),
            last.has_read_tool_call(),
            last.has_tool_result(),
            last.assistant_content()
                .to_ascii_uppercase()
                .contains("READY")
        );
    };
    assert!(
        observed.read_result_carries_token(&token),
        "a durable read tool result carries the fixture note content"
    );
    assert!(
        observed.facts_end_at_snapshot_cursor(),
        "the accepted run delivers one contiguous fact range ending in the terminal fact at the snapshot cursor"
    );
    let facts_json = serde_json::to_string(&observed.facts).expect("durable facts serialize");
    let pre_restart_cursor = observed.snapshot.cursor();

    host.restart_daemon();
    let client = wait_until_ready(&mut host, Instant::now() + READINESS_DEADLINE);
    let stream_client = RunStreamClient::new(host.endpoint.clone(), "real-api-e2e")
        .expect("stream client is valid");
    let subscription = stream_client
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

    // A completed durable run commits no further facts. After a bounded quiet
    // window a fresh subscription still replays the identical cursor.
    tokio::time::sleep(REPLAY_QUIET_WINDOW).await;
    let replayed = collect_terminal_run(
        &host.endpoint,
        session_id,
        run_id,
        Instant::now() + REPLAY_DEADLINE,
    )
    .await;
    assert!(
        replayed.facts.is_empty(),
        "the run-stream replay tail is empty by design; live batches carry facts"
    );
    assert_eq!(replayed.status(), RunStatusDto::Completed);
    assert_eq!(
        replayed.snapshot.cursor(),
        pre_restart_cursor,
        "no durable fact commits after the run completed"
    );

    // Credential hygiene after the hard kill: no durable fact, snapshot, log,
    // or state byte carries the credential value.
    let session_json = serde_json::to_string(
        &client
            .session_snapshot(session_id)
            .expect("session snapshot reads"),
    )
    .expect("session snapshot serializes");
    let log_text = host.captured_log();
    assert_excludes_credential(&facts_json, &credential, "serialized durable run facts");
    assert_excludes_credential(&session_json, &credential, "replayed session snapshot JSON");
    assert_excludes_credential(&log_text, &credential, "captured daemon log");
    assert_state_directory_excludes_credential(host.state_home.path(), &credential);
}

/// Proves the live provider channel fails closed on a recognizable invalid
/// credential and never echoes the credential or raw provider text.
///
/// The run must terminalize `Failed` with one normalized closed-set code, and
/// neither the durable facts, the session snapshot, nor the daemon log may
/// carry the literal credential or a raw provider message field.
#[tokio::test]
#[ignore = "opt-in live-provider e2e; see ADR 0040; run via make e2e-real-api"]
async fn real_provider_rejects_invalid_credential_without_leak() {
    let Some(mut provider) = LiveProviderConfig::from_env() else {
        return;
    };
    let credential = "invalid-credential-live-e2e".to_owned();
    provider.credential.clone_from(&credential);
    let mut host = LiveE2eHost::new(&provider, None);
    let client = wait_until_ready(&mut host, Instant::now() + READINESS_DEADLINE);

    let session_id = SessionId::new();
    create_session(&host.endpoint, session_id, host.workspace.path());
    let run_id = send_user_turn(
        &host.endpoint,
        session_id,
        "Reply with the single word READY.",
    );
    let observed = collect_terminal_run(
        &host.endpoint,
        session_id,
        run_id,
        Instant::now() + TURN_DEADLINE,
    )
    .await;

    assert_eq!(
        observed.status(),
        RunStatusDto::Failed,
        "the live provider rejects the invalid credential with {:?}",
        observed.failure_code()
    );
    let failure_code = observed
        .failure_code()
        .expect("a failed live run records a safe failure code");
    assert!(
        matches!(
            failure_code,
            "generic_chat_provider_request_rejected"
                | "generic_chat_provider_unavailable"
                | "openrouter_provider_request_rejected"
                | "openrouter_provider_unavailable"
        ),
        "the live provider failure is one normalized closed-set code, got: {failure_code}"
    );

    let facts_json = serde_json::to_string(&observed.facts).expect("durable facts serialize");
    let session_json = serde_json::to_string(
        &client
            .session_snapshot(session_id)
            .expect("session snapshot reads"),
    )
    .expect("session snapshot serializes");
    assert_excludes_credential(&facts_json, &credential, "serialized durable run facts");
    assert_excludes_credential(&session_json, &credential, "session snapshot JSON");
    assert_excludes_credential(&host.captured_log(), &credential, "captured daemon log");
    assert!(
        !facts_json.contains("\"message\""),
        "durable run facts never carry raw provider text"
    );
}
