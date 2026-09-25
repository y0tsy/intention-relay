//! Opt-in live-provider end-to-end tests for the real daemon binary.
//!
//! These tests spawn the real `intention-daemon` binary, drive it through the
//! real local transport, and execute the real production model-tool loop
//! against a live provider API over HTTPS. The positive test drives every
//! advertised tool (`read`, `write`, `edit`, `execute`, `glob`, `grep`) as one
//! live session: it proves that a real model tool call for each of them
//! executes through the typed tool registry, that every call's
//! `ToolCallRecorded` and `ToolResultRecorded` facts are durable, that the
//! observed effects reach the workspace, and that a hard daemon restart
//! replays a recorded run without re-executing the tool or disclosing the
//! provider credential.
//!
//! When the configured model runs in thinking mode, the same tool loop also
//! proves the ADR 0041 reasoning round trip: the gateway rejects a tool-loop
//! continuation whose assistant tool-call message does not carry the round's
//! accepted `reasoning_content`, so a completed live tool loop means the
//! runtime attached that reasoning and the generic adapter serialized it.
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
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use intention_client::{IntentionClient, ProcessDaemonLauncher, RunStreamClient};
use intention_config::{
    ConfigPathDto, ConfigSourceDto, ProviderKindDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    CreateSessionCommandDto, ModelRunFactDto, ModelRunFactInputDto, RunFailureDto, RunModeDto,
    RunSnapshotDto, RunStatusDto, SendUserTurnCommandDto, ToolResultOutcomeDto, WorkspaceRootDto,
};
use intention_protocol::{
    DaemonReadinessDto, ProtocolAcceptedResultDto, ProtocolCapabilityDto, ProtocolCommandDto,
    ProtocolCommandResultDto, ProtocolDaemonFrameDto, ProtocolHelloDto, ProtocolMessageDto,
    ProtocolRequestEnvelopeDto, ProtocolRequestPayloadDto, ProtocolResponsePayloadDto,
    RunResyncReasonDto, RunStreamFrameDto, RunSubscriptionRequestEnvelopeDto,
    RunSubscriptionResponseDto, SendUserTurnOutcomeDto, SessionSnapshotDto, SubscribeRunCommandDto,
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

/// The bounded deadline for one synchronous session snapshot read.
const SESSION_READ_DEADLINE: Duration = Duration::from_secs(15);

/// The bounded attempts one live tool turn may consume before it fails.
const TOOL_TURN_ATTEMPTS: u8 = 3;

// Worst-case live-channel budget: the six positive tool turns plus the negative
// credential run, each allowed `TOOL_TURN_ATTEMPTS` attempts of `TURN_DEADLINE`
// (7 x 3 x 180 s = 3780 s = 63 min), two `READINESS_DEADLINE` daemon starts
// (60 s), the hard-kill window (5 s), the bounded post-restart subscribe, the
// quiet window, the bounded replay observations, and the bounded session reads
// (about 1 min) add up to roughly 66 minutes.
//
// Every synchronous command (`client.health`, `create_session`,
// `send_user_turn`, and the session reads) is additionally bounded by the
// transport I/O timeout: one call spends at most the transport's 500 ms
// connect wait plus two transport read timeouts (one for the hello, one for
// the response) of 10 s each, which is at most 20.5 s. A daemon that accepts
// connections and never answers therefore fails the readiness deadline plus
// one bounded health call per `wait_until_ready` (2 x (30 s + 20.5 s)) and
// then the first bounded `create_session` or `send_user_turn` (20.5 s): the
// harness reports its own diagnostic within about two minutes instead of
// hanging until the CI timeout.
// `.github/workflows/real-api-e2e.yml` keeps its run step at 90 minutes and
// its job at 120 minutes so a degraded live run reports the harness diagnostic
// ("did not record a succeeded <tool> call within 3 turns") instead of an
// opaque GitHub timeout; a change to any budget constant must keep that
// relation.

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

/// Resolves the daemon's durable state directory for the fixture environment.
///
/// `spawn_daemon` overrides the platform state root that the daemon's private
/// `platform_state_directory` resolves (`XDG_STATE_HOME` on Linux, `HOME` on
/// macOS, and `LOCALAPPDATA` on Windows), so this mirrors that resolution for
/// the fixture directories: the credential scan must inspect the directory
/// that actually holds the SQLite state bytes on every target. On macOS the
/// fixture configuration file shares that directory with the durable state and
/// the scan excludes it as fixture input.
fn fixture_state_directory(host: &LiveE2eHost) -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        host.state_home.path().join("intention-relay")
    }
    #[cfg(target_os = "macos")]
    {
        host.config_home
            .path()
            .join("Library/Application Support/intention-relay")
    }
    #[cfg(windows)]
    {
        host.state_home.path().join("intention-relay")
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        host.state_home.path().join("intention-relay")
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
    project_id: ProjectId,
    workspace_id: WorkspaceId,
}

impl LiveE2eHost {
    /// Creates a fresh isolated fixture, seeds the requested workspace files,
    /// writes its provider configuration, and spawns its first daemon process.
    fn new(provider: &LiveProviderConfig, workspace_files: &[(&str, &str)]) -> Self {
        let config_home = TempDir::new().expect("config directory exists");
        let state_home = TempDir::new().expect("state directory exists");
        let workspace = TempDir::new().expect("workspace directory exists");
        for (name, content) in workspace_files {
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
            project_id: ProjectId::new(),
            workspace_id: WorkspaceId::new(),
        }
    }

    /// Kills the current daemon and starts a fresh process with identical
    /// environment, state directories, and endpoint.
    ///
    /// The kill is a hard kill, so the daemon cannot clean up; its Unix socket
    /// file survives, and the transport reclaims the stale socket on the next
    /// bind, exactly like the hermetic facade fixture. A clean daemon exit
    /// leaves the same file now that a dropped listener never unlinks its
    /// endpoint.
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

/// Reads one session snapshot with a harness-side deadline.
///
/// The synchronous transport bounds every read and write with its own I/O
/// timeout, so the call itself cannot block forever; this helper adds the
/// tighter harness deadline on top of it. The read runs on a worker that owns
/// its own client and the harness waits only until the deadline, so a daemon
/// that accepts the connection and never answers fails with this harness
/// diagnostic instead of hanging the run until the CI step timeout.
fn bounded_session_snapshot(
    endpoint: &LocalEndpoint,
    session_id: SessionId,
    deadline: Instant,
) -> SessionSnapshotDto {
    let endpoint = endpoint.clone();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let client = IntentionClient::new(
            endpoint,
            "real-api-e2e",
            Box::new(
                ProcessDaemonLauncher::new(env!("CARGO_BIN_EXE_intention-daemon"))
                    .expect("daemon program is valid"),
            ),
        )
        .expect("live e2e client is valid");
        let _ = sender.send(client.session_snapshot(session_id));
    });
    match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(Ok(snapshot)) => snapshot,
        Ok(Err(error)) => panic!("the daemon serves the session snapshot: {error:?}"),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("the session snapshot arrives before the deadline")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("the session snapshot worker completes")
        }
    }
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
///
/// Every session of one fixture shares its project and workspace identity:
/// the durable workspace root is unique, so a second session over the same
/// root must reuse the workspace identity instead of minting a new one.
fn create_session(host: &LiveE2eHost, session_id: SessionId, label: &str) {
    let created = send_command(
        &host.endpoint,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::CreateSession(
            CreateSessionCommandDto::new(
                host.project_id,
                session_id,
                host.workspace_id,
                WorkspaceRootDto::parse(host.workspace.path().to_string_lossy().into_owned())
                    .expect("workspace root is absolute"),
                RunModeDto::Build,
            ),
        )),
    )
    .expect("session creation is accepted");
    assert!(
        matches!(created, ProtocolCommandResultDto::Accepted(_)),
        "the daemon accepts session creation for {label}, got: {created:?}"
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

    /// Reports whether any durable tool call for the named tool was recorded.
    fn has_tool_call(&self, tool: &str) -> bool {
        self.facts.iter().any(|fact| {
            matches!(
                fact.input(),
                ModelRunFactInputDto::ToolCallRecorded { call } if call.name() == tool
            )
        })
    }

    /// Returns the distinct tool names recorded by durable calls, in fact order.
    fn recorded_tool_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for fact in &self.facts {
            if let ModelRunFactInputDto::ToolCallRecorded { call } = fact.input() {
                let name = call.name();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        names
    }

    /// Returns the succeeded durable result content of every recorded call of
    /// one tool, matched to its call by identity.
    fn succeeded_contents(&self, tool: &str) -> Vec<&str> {
        self.facts
            .iter()
            .filter_map(|result| {
                let ModelRunFactInputDto::ToolResultRecorded {
                    call_id,
                    outcome: ToolResultOutcomeDto::Succeeded { content },
                } = result.input()
                else {
                    return None;
                };
                self.facts
                    .iter()
                    .any(|call| {
                        matches!(
                            call.input(),
                            ModelRunFactInputDto::ToolCallRecorded { call }
                                if call.name() == tool && call.call_id() == *call_id
                        )
                    })
                    .then_some(content.as_str())
            })
            .collect()
    }

    /// Returns the failure code and the call arguments of every durable failed
    /// result recorded for one tool, matched to its call by identity.
    fn failed_results(&self, tool: &str) -> Vec<(&str, &str)> {
        self.facts
            .iter()
            .filter_map(|result| {
                let ModelRunFactInputDto::ToolResultRecorded {
                    call_id,
                    outcome: ToolResultOutcomeDto::Failed { failure },
                } = result.input()
                else {
                    return None;
                };
                self.facts.iter().find_map(|call| {
                    let ModelRunFactInputDto::ToolCallRecorded { call } = call.input() else {
                        return None;
                    };
                    (call.name() == tool && call.call_id() == *call_id)
                        .then_some((failure.code(), call.arguments_json()))
                })
            })
            .collect()
    }
}

/// The bounded outcome of one run-stream observation.
enum RunObservation {
    /// The run delivered its authoritative terminal snapshot.
    Terminal(Box<ObservedRun>),
    /// The connection was lost before the terminal snapshot, because the
    /// daemon evicted this subscriber or the stream closed. The caller may
    /// observe again.
    Lost(String),
}

/// Subscribes to one run and collects the delivered durable facts plus the
/// authoritative terminal snapshot.
///
/// The daemon replays an empty tail on subscribe and delivers facts only as
/// live batches while the run commits them, so a subscriber observes a suffix
/// of the run's facts but never a gap inside that suffix. A real provider can
/// pause for several seconds inside one attempt, so the whole observation is
/// bounded by the deadline instead of by per-frame read timeouts.
async fn collect_terminal_run(
    endpoint: &LocalEndpoint,
    session_id: SessionId,
    run_id: RunId,
    deadline: Instant,
) -> RunObservation {
    tokio::time::timeout_at(
        tokio::time::Instant::from_std(deadline),
        collect_run_frames(endpoint, session_id, run_id),
    )
    .await
    .unwrap_or_else(|_| panic!("run facts arrive before the deadline"))
}

/// Reads one run stream until its terminal snapshot or a lost connection.
async fn collect_run_frames(
    endpoint: &LocalEndpoint,
    session_id: SessionId,
    run_id: RunId,
) -> RunObservation {
    let Ok(connection) = AsyncLocalClientConnection::connect(endpoint).await else {
        return RunObservation::Lost("the run stream connection is unavailable".to_owned());
    };
    let Ok((_remote, mut requests, mut frames)) =
        connection.negotiate_daemon_frames(stream_hello()).await
    else {
        return RunObservation::Lost("the run stream negotiation failed".to_owned());
    };
    let correlation_id = CorrelationIdDto::new();
    if requests
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
        .is_err()
    {
        return RunObservation::Lost("the run subscription request failed".to_owned());
    }
    let mut facts = Vec::new();
    loop {
        let Ok(frame) = frames.receive().await else {
            return RunObservation::Lost(format!(
                "the run stream closed after {} facts",
                facts.len()
            ));
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
                            return RunObservation::Terminal(Box::new(ObservedRun {
                                facts,
                                snapshot,
                            }));
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
                    return RunObservation::Terminal(Box::new(ObservedRun { facts, snapshot }));
                }
            }
            ProtocolDaemonFrameDto::RunStream(RunStreamFrameDto::Resync(resync)) => {
                // The daemon bounds every subscriber queue and evicts one that
                // cannot keep up; a long provider reasoning burst can exceed
                // that bound. The caller observes again instead of failing.
                assert_eq!(
                    resync.reason(),
                    RunResyncReasonDto::SubscriberTooSlow,
                    "only a slow-subscriber eviction is an expected resync"
                );
                return RunObservation::Lost(format!(
                    "the daemon evicted the slow subscriber after {} facts",
                    facts.len()
                ));
            }
        }
    }
}

/// Drives one live turn that must record a durable succeeded call of the named
/// tool and reach the durable effect `effect` requires, leaving the workspace
/// untouched between attempts.
async fn drive_tool_turn(
    host: &LiveE2eHost,
    tool: &str,
    prompt: &str,
    effect: &(dyn Fn(&ObservedRun) -> bool + Sync),
) -> (SessionId, ObservedRun) {
    drive_tool_turn_with(host, tool, prompt, &|| {}, effect).await
}

/// Drives one live turn that must record a durable succeeded call of the named
/// tool and reach the durable effect `effect` requires.
///
/// The same prompt is retried within the bounded attempt budget, because the
/// live channel has three acceptably lossy steps: a live model occasionally
/// answers without calling the tool at all, it occasionally phrases a tool
/// argument differently from the prompt so the tool rejects the call or the
/// durable effect does not match, and the daemon drops a subscriber that
/// cannot keep up with a reasoning burst. Each of those consumes one attempt
/// in a fresh session, and `prepare` restores the workspace state that an
/// earlier attempt of a mutating turn may have changed. A completed turn whose
/// durable facts satisfy `effect` is accepted, and any other failure is still
/// fatal.
async fn drive_tool_turn_with(
    host: &LiveE2eHost,
    tool: &str,
    prompt: &str,
    prepare: &(dyn Fn() + Sync),
    effect: &(dyn Fn(&ObservedRun) -> bool + Sync),
) -> (SessionId, ObservedRun) {
    let mut last_gap = None;
    for attempt in 1..=TOOL_TURN_ATTEMPTS {
        prepare();
        let session_id = SessionId::new();
        create_session(host, session_id, &format!("{tool} turn {attempt}"));
        let run_id = send_user_turn(&host.endpoint, session_id, prompt);
        let observed = match collect_terminal_run(
            &host.endpoint,
            session_id,
            run_id,
            Instant::now() + TURN_DEADLINE,
        )
        .await
        {
            RunObservation::Terminal(observed) => *observed,
            RunObservation::Lost(reason) => {
                last_gap = Some(reason);
                continue;
            }
        };
        if observed.status() != RunStatusDto::Completed {
            // A durable failed result for the tool proves the run was
            // terminalized by the tool's own rejection of a live model
            // argument, which is model noise; a run that fails without one is
            // a product failure and stays fatal.
            let rejected = observed.failed_results(tool);
            assert!(
                !rejected.is_empty(),
                "live provider {tool} turn {attempt} terminated without a completed run, failure code {:?}, recorded calls {:?}",
                observed.failure_code(),
                observed.recorded_tool_names()
            );
            last_gap = Some(format!(
                "a {tool} call was rejected with {rejected:?} (run failure {:?})",
                observed.failure_code()
            ));
            continue;
        }
        assert!(
            observed.facts_end_at_snapshot_cursor(),
            "the {tool} turn delivers one contiguous fact range ending in its terminal fact at the snapshot cursor"
        );
        if observed.has_tool_call(tool) && !observed.succeeded_contents(tool).is_empty() {
            if effect(&observed) {
                return (session_id, observed);
            }
            last_gap = Some(format!(
                "a completed run recorded a succeeded {tool} call whose durable effect was not observed"
            ));
            continue;
        }
        last_gap = Some(format!(
            "a completed run recorded calls {:?}",
            observed.recorded_tool_names()
        ));
    }
    panic!(
        "the live provider did not record a succeeded {tool} call within {TOOL_TURN_ATTEMPTS} turns: {}",
        last_gap.unwrap_or_else(|| "no turn was observed".to_owned())
    );
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

/// Asserts no daemon-written file under the resolved durable state directory
/// contains the credential bytes, and that the scan inspected at least one
/// file.
///
/// `fixture_config` is the fixture's own provider configuration file, which
/// legitimately carries the credential and shares the durable state directory
/// on macOS; it is operator input rather than a daemon-written state byte and
/// is skipped. The final non-empty assertion makes an empty or wrongly
/// resolved target fail loudly instead of passing vacuously. The assertion
/// messages name only the file path, never the credential.
fn assert_state_directory_excludes_credential(
    directory: &Path,
    fixture_config: &Path,
    credential: &str,
) {
    let mut pending = vec![directory.to_path_buf()];
    let mut scanned_files = 0_usize;
    while let Some(current) = pending.pop() {
        let entries = std::fs::read_dir(&current).expect("state directory is readable");
        for entry in entries {
            let path = entry.expect("state entry is readable").path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path == fixture_config {
                continue;
            }
            let bytes = std::fs::read(&path).expect("state file is readable");
            assert!(
                !bytes_contain_credential(&bytes, credential),
                "the state file never discloses the provider credential: {}",
                path.display()
            );
            scanned_files += 1;
        }
    }
    assert!(
        scanned_files > 0,
        "the resolved durable state directory holds at least one daemon-written file: {}",
        directory.display()
    );
}

/// The normalized closed-set provider failure codes the invalid-credential
/// live run may record, for either selectable provider kind.
const INVALID_CREDENTIAL_FAILURE_CODES: [&str; 4] = [
    "generic_chat_provider_request_rejected",
    "generic_chat_provider_unavailable",
    "openrouter_provider_request_rejected",
    "openrouter_provider_unavailable",
];

/// Asserts the invalid-credential run delivered a non-empty set of durable
/// facts and that every one of them is a normalized failure fact.
///
/// The provider rejects the request before any assistant, reasoning, usage, or
/// tool output can exist, so every delivered fact must be an attempt-lifecycle
/// or terminal-failure shape whose failure re-validates through the durable
/// DTO constructors and carries one of the normalized closed-set provider
/// failure codes. A content-bearing fact would mean raw provider output
/// reached durable state, which the containment invariant of ADR 0040
/// decision 7 forbids; the guard fails rather than silently accepting a new
/// fact shape. The non-empty requirement keeps the invariant from passing
/// vacuously when the terminal snapshot carries no delivered facts.
fn assert_invalid_credential_facts_are_normalized(facts: &[ModelRunFactDto]) {
    assert!(
        !facts.is_empty(),
        "the invalid-credential run delivers at least one durable normalized failure fact"
    );
    for fact in facts {
        match fact.input() {
            ModelRunFactInputDto::ProviderAttemptStarted { attempt } => {
                assert!(
                    ModelRunFactInputDto::provider_attempt_started(*attempt).is_ok(),
                    "the recorded provider attempt re-validates through its durable DTO"
                );
            }
            ModelRunFactInputDto::ProviderAttemptFailed { attempt, failure } => {
                assert!(
                    ModelRunFactInputDto::provider_attempt_failed(*attempt, failure.clone())
                        .is_ok(),
                    "the recorded provider attempt failure re-validates through its durable DTO"
                );
                assert_normalized_provider_failure(failure);
            }
            ModelRunFactInputDto::RetryScheduled {
                failed_attempt,
                next_attempt,
            } => {
                assert!(
                    ModelRunFactInputDto::retry_scheduled(*failed_attempt, *next_attempt).is_ok(),
                    "the recorded retry re-validates through its durable DTO"
                );
            }
            ModelRunFactInputDto::Failed { failure } => {
                assert!(
                    RunFailureDto::new(
                        failure.code().to_owned(),
                        failure.retry(),
                        failure.correlation_id(),
                    )
                    .is_ok(),
                    "the recorded terminal failure re-validates through its durable DTO"
                );
                assert_normalized_provider_failure(failure);
            }
            other => panic!(
                "the invalid-credential run records only normalized failure facts, got kind {}",
                other.kind().as_str()
            ),
        }
    }
}

/// Asserts one recorded provider failure carries a normalized closed-set code.
fn assert_normalized_provider_failure(failure: &RunFailureDto) {
    assert!(
        INVALID_CREDENTIAL_FAILURE_CODES.contains(&failure.code()),
        "the recorded provider failure code is in the normalized closed set, got: {}",
        failure.code()
    );
}

/// Proves the real production tool loop against a live provider for every
/// advertised tool, then proves a durable run replays across a hard daemon
/// restart.
///
/// One live daemon drives all six active registry tools as separate live
/// sessions: `read`, `glob`, `grep`, `write`, `edit`, and `execute`. A turn is
/// accepted only when its run completed, delivered a contiguous fact range
/// ending in the terminal fact at the snapshot cursor, and recorded a durable
/// call with a succeeded durable result that reached the turn's required
/// effect: the note text for `read` and `grep`, the workspace path list for
/// `glob`, the written bytes for `write` and `edit`, and the real child's
/// stdout with its typed exit status for `execute`. Every effect check runs
/// inside the bounded attempt budget, so a valid-but-different provider
/// argument consumes an attempt instead of failing the completed run. A turn
/// that misses those conditions, a turn whose tool call the tool itself
/// rejected, and a turn whose subscriber the daemon evicted each consume one
/// bounded attempt in a fresh session; any other failed run or a timeout fails
/// the test.
#[tokio::test]
#[ignore = "opt-in live-provider e2e; see ADR 0040; run via make e2e-real-api"]
async fn real_provider_tool_loop_drives_every_advertised_tool_and_replays_after_restart() {
    let Some(provider) = LiveProviderConfig::from_env() else {
        return;
    };
    let token = format!("real-e2e-note-{}", std::process::id());
    let note = format!("live provider tool-loop note: {token}\n");
    let edit_source = format!("editable live line: {token}\n");
    let mut host = LiveE2eHost::new(
        &provider,
        &[
            ("e2e-note.txt", note.as_str()),
            ("e2e-edit-source.txt", edit_source.as_str()),
        ],
    );
    let credential = host.credential.clone();
    let _client = wait_until_ready(&mut host, Instant::now() + READINESS_DEADLINE);

    // `read`: the live model reads the seeded note through the real registry.
    let (read_session, read_run) = drive_tool_turn(
        &host,
        "read",
        &format!(
            "Use the read tool with the arguments {} to read that workspace file.",
            serde_json::json!({"path": "e2e-note.txt"})
        ),
        &|run| run.succeeded_contents("read").join("\n").contains(&token),
    )
    .await;

    // `glob`: the real registry lists the workspace-relative path.
    let (glob_session, glob_run) = drive_tool_turn(
        &host,
        "glob",
        &format!(
            "Use the glob tool with the arguments {} to list the workspace text files.",
            serde_json::json!({"pattern": "*.txt"})
        ),
        &|run| {
            run.succeeded_contents("glob")
                .join("\n")
                .contains("e2e-note.txt")
        },
    )
    .await;

    // `grep`: the real registry finds the token under the workspace scope.
    let (grep_session, grep_run) = drive_tool_turn(
        &host,
        "grep",
        &format!(
            "Use the grep tool with the arguments {} to find the note token anywhere in the workspace.",
            serde_json::json!({"pattern": token, "scope": {"kind": "workspace"}})
        ),
        &|run| run.succeeded_contents("grep").join("\n").contains(&token),
    )
    .await;

    // `write`: the real registry creates the file with exactly those bytes.
    let written = format!("written by the live provider: {token}");
    let written_file = host.workspace.path().join("e2e-written.txt");
    let (write_session, write_run) = drive_tool_turn(
        &host,
        "write",
        &format!(
            "Use the write tool with the arguments {} to create that workspace file exactly as given.",
            serde_json::json!({"path": "e2e-written.txt", "content": written})
        ),
        &|run| {
            let Ok(written_bytes) = std::fs::read_to_string(&written_file) else {
                return false;
            };
            written_bytes.contains(&token)
                && written_bytes.contains("written by the live provider")
                && run.succeeded_contents("write").join("\n")
                    == format!("{} bytes", written_bytes.len())
        },
    )
    .await;

    // `edit`: the real registry replaces the seeded token in place. A retry
    // starts from the seeded file again, because a lost observation may follow
    // an attempt whose edit already reached the workspace.
    let edited = format!("{token}-edited");
    let edited_source = edit_source.replacen(&token, &edited, 1);
    let reseed_edit_source = || {
        std::fs::write(
            host.workspace.path().join("e2e-edit-source.txt"),
            edit_source.as_str(),
        )
        .expect("the edit fixture reseeds before every attempt");
    };
    let edited_file = host.workspace.path().join("e2e-edit-source.txt");
    let (edit_session, edit_run) = drive_tool_turn_with(
        &host,
        "edit",
        &format!(
            "Use the edit tool with the arguments {} to update that workspace file.",
            serde_json::json!({"path": "e2e-edit-source.txt", "old": token, "new": edited})
        ),
        &reseed_edit_source,
        &|run| {
            let Ok(edited_bytes) = std::fs::read_to_string(&edited_file) else {
                return false;
            };
            edited_bytes.contains(&edited_source)
                && run.succeeded_contents("edit").join("\n")
                    == format!("{} bytes", edited_bytes.len())
        },
    )
    .await;

    // `execute`: the real registry runs a child process in the workspace.
    let (program, args) = if cfg!(windows) {
        ("cmd", vec!["/C".to_owned(), format!("echo {token}")])
    } else {
        ("sh", vec!["-c".to_owned(), format!("echo {token}")])
    };
    let (execute_session, execute_run) = drive_tool_turn(
        &host,
        "execute",
        &format!(
            "Use the execute tool with the arguments {} to print the token with a real child process.",
            serde_json::json!({"program": program, "args": args})
        ),
        &|run| {
            let content = run.succeeded_contents("execute").join("\n");
            content.contains(&token) && content.contains("exit_code:0")
        },
    )
    .await;

    let runs = [
        &read_run,
        &glob_run,
        &grep_run,
        &write_run,
        &edit_run,
        &execute_run,
    ];
    let run_id = read_run.snapshot.run_id();
    let pre_restart_cursor = read_run.snapshot.cursor();

    host.restart_daemon();
    let _client = wait_until_ready(&mut host, Instant::now() + READINESS_DEADLINE);
    let stream_client = RunStreamClient::new(host.endpoint.clone(), "real-api-e2e")
        .expect("stream client is valid");
    let subscription = tokio::time::timeout_at(
        tokio::time::Instant::from_std(Instant::now() + REPLAY_DEADLINE),
        stream_client.subscribe(SubscribeRunCommandDto::new(
            intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
            read_session,
            run_id,
            None,
        )),
    )
    .await
    .unwrap_or_else(|_| panic!("restart replay arrives before the deadline"))
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
    let mut replayed = None;
    for _ in 1..=TOOL_TURN_ATTEMPTS {
        if let RunObservation::Terminal(observed) = collect_terminal_run(
            &host.endpoint,
            read_session,
            run_id,
            Instant::now() + REPLAY_DEADLINE,
        )
        .await
        {
            replayed = Some(*observed);
            break;
        }
    }
    let replayed = replayed.expect("the completed run stays observable after the quiet window");
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

    // Credential hygiene after the hard kill: no durable fact, session
    // snapshot, log, or state byte carries the credential value.
    let sessions = [
        read_session,
        glob_session,
        grep_session,
        write_session,
        edit_session,
        execute_session,
    ];
    let log_text = host.captured_log();
    for (index, run) in runs.iter().enumerate() {
        let facts_json = serde_json::to_string(&run.facts).expect("durable facts serialize");
        assert_excludes_credential(
            &facts_json,
            &credential,
            &format!("turn {} durable run facts", index + 1),
        );
    }
    for (index, session) in sessions.iter().enumerate() {
        let session_json = serde_json::to_string(&bounded_session_snapshot(
            &host.endpoint,
            *session,
            Instant::now() + SESSION_READ_DEADLINE,
        ))
        .expect("session snapshot serializes");
        assert_excludes_credential(
            &session_json,
            &credential,
            &format!("turn {} session snapshot JSON", index + 1),
        );
    }
    assert_excludes_credential(&log_text, &credential, "captured daemon log");
    assert_state_directory_excludes_credential(
        &fixture_state_directory(&host),
        &config_path(host.config_home.path()),
        &credential,
    );
}

/// Proves the live provider channel fails closed on a recognizable invalid
/// credential and never echoes the credential or raw provider text.
///
/// The run must terminalize `Failed` with one normalized closed-set code, and
/// neither the durable facts, the session snapshot, nor the daemon log may
/// carry the literal credential. The run must deliver at least one durable
/// fact, every delivered durable fact must be a normalized failure fact, and
/// the terminal projection must carry no assistant text, so raw provider
/// output cannot have reached durable state.
#[tokio::test]
#[ignore = "opt-in live-provider e2e; see ADR 0040; run via make e2e-real-api"]
async fn real_provider_rejects_invalid_credential_without_leak() {
    let Some(mut provider) = LiveProviderConfig::from_env() else {
        return;
    };
    let credential = "invalid-credential-live-e2e".to_owned();
    provider.credential.clone_from(&credential);
    let mut host = LiveE2eHost::new(&provider, &[]);
    let _client = wait_until_ready(&mut host, Instant::now() + READINESS_DEADLINE);

    let session_id = SessionId::new();
    create_session(&host, session_id, "the invalid-credential run");
    let run_id = send_user_turn(&host.endpoint, session_id, "Reply with a short greeting.");
    // A subscriber that attaches after the run terminalized receives the
    // empty replay tail, and an empty delivered fact set would make the
    // normalized-fact invariant vacuous. Observe again until the run delivers
    // its durable failure facts within the bounded budget.
    let mut observed = None;
    for _ in 1..=TOOL_TURN_ATTEMPTS {
        if let RunObservation::Terminal(terminal) = collect_terminal_run(
            &host.endpoint,
            session_id,
            run_id,
            Instant::now() + TURN_DEADLINE,
        )
        .await
        {
            if terminal.facts.is_empty() {
                continue;
            }
            observed = Some(*terminal);
            break;
        }
    }
    let observed = observed.expect(
        "the rejected live run delivers its durable normalized failure facts within the bounded budget",
    );

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
        INVALID_CREDENTIAL_FAILURE_CODES.contains(&failure_code),
        "the live provider failure is one normalized closed-set code, got: {failure_code}"
    );
    assert_invalid_credential_facts_are_normalized(&observed.facts);
    assert!(
        observed.assistant_content().is_empty(),
        "the rejected credential run records no assistant text"
    );

    let facts_json = serde_json::to_string(&observed.facts).expect("durable facts serialize");
    let session_json = serde_json::to_string(&bounded_session_snapshot(
        &host.endpoint,
        session_id,
        Instant::now() + SESSION_READ_DEADLINE,
    ))
    .expect("session snapshot serializes");
    assert_excludes_credential(&facts_json, &credential, "serialized durable run facts");
    assert_excludes_credential(&session_json, &credential, "session snapshot JSON");
    assert_excludes_credential(&host.captured_log(), &credential, "captured daemon log");
}
