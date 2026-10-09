//! Opt-in live-provider end-to-end tests for the real daemon binary.
//!
//! These tests spawn the real `intention-daemon` binary, drive it through the
//! real local transport, and execute the real production model-tool loop
//! against a live provider API over HTTPS. The positive test drives every
//! advertised tool (`read`, `write`, `edit`, `execute`, `glob`, `grep`) as one
//! live session: it proves that a real model tool call for each of them
//! executes through the typed tool registry, that every call's `tool_call`
//! transcript row and its paired `tool_result` evidence row are durable, that
//! the observed effects reach the workspace, and that a hard daemon restart
//! serves the recorded current state of a completed run without re-executing
//! the tool or disclosing the provider credential.
//!
//! When the configured model runs in thinking mode, the same tool loop also
//! proves the same-run reasoning round trip: the gateway rejects a tool-loop
//! continuation whose assistant tool-call message does not carry the round's
//! accepted `reasoning_content`, so a completed live tool loop means the
//! runtime attached that reasoning and the generic adapter serialized it.
//!
//! This file is the single opt-in live channel recorded by architecture 10. It is
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
    clippy::print_stderr,
    reason = "Opt-in live-provider end-to-end fixtures use assertion conveniences and one stderr diagnostic on the unwind path for precise failures."
)]

mod common;

use std::path::{Path, PathBuf};
use std::process::Child;
use std::thread;
use std::time::{Duration, Instant};

use common::{config_path, spawn_daemon, unique_endpoint, write_config_document};
use intention_client::{IntentionClient, ProcessDaemonLauncher, RunStreamClient};
use intention_config::{
    ConfigPathDto, ConfigSourceDto, ProviderKindDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_proto::{
    CreateSessionCommandDto, MessageKindDto, MessageProjectionDto, RunModeDto, RunProjectionDto,
    RunStatusDto, WorkspaceRootDto, run_status_is_terminal,
};
use intention_proto::{IdempotencyKey, ProjectId, RunId, SessionId, WorkspaceId};
use intention_proto::{SendUserTurnOutcomeDto, SessionSnapshotDto, SubscribeRunCommandDto};
use intention_transport::LocalEndpoint;
use tempfile::TempDir;

/// The generic-chat endpoint used when the operator does not override it.
const DEFAULT_GENERIC_CHAT_ENDPOINT: &str = "https://api.openai.com/v1";

/// The per-turn deadline for one live provider run: two provider attempts with
/// the configured 60-second attempt timeout plus real tool execution.
const TURN_DEADLINE: Duration = Duration::from_secs(180);

/// The readiness deadline for one daemon process start.
const READINESS_DEADLINE: Duration = Duration::from_secs(30);

/// The bounded window that proves a completed durable run commits no new
/// transcript rows.
const TERMINAL_QUIET_WINDOW: Duration = Duration::from_secs(1);

/// The bounded window used to observe current state from a fresh subscription.
const RESUBSCRIBE_DEADLINE: Duration = Duration::from_secs(15);

/// The bounded deadline for one synchronous session snapshot read.
const SESSION_READ_DEADLINE: Duration = Duration::from_secs(15);

/// The bounded window that proves a killed daemon process has exited before
/// the harness continues.
const KILL_DEADLINE: Duration = Duration::from_secs(5);

/// The bounded attempts one live tool turn may consume before it fails.
const TOOL_TURN_ATTEMPTS: u8 = 3;

// Worst-case live-channel budget: 4462 seconds (about 74 minutes), recomputed
// from the constants above, the bounded client calls, and the harness
// structure:
// - 7 tool runs (the six positive tool turns plus the negative credential
//   run) x `TOOL_TURN_ATTEMPTS` 3 attempts x `TURN_DEADLINE` 180 s = 3780 s;
// - the bounded `create_session` and `send_user_turn` of every attempt:
//   21 attempts x 2 calls x 10.5 s = 441 s;
// - three `READINESS_DEADLINE` daemon starts (3 x 30 s = 90 s);
// - three `KILL_DEADLINE` windows (3 x 5 s = 15 s);
// - the bounded post-restart subscription (15 s), the quiet window (1 s), the
//   bounded re-observation after the quiet window (15 s), and the seven
//   bounded session snapshots (7 x 15 s = 105 s).
//
// Every command (`client.health`, `create_session`, and `send_user_turn`) is
// additionally bounded by the client: one call spends at most `CONNECT_TIMEOUT`
// 500 ms on the connect plus one `REQUEST_TIMEOUT` 10 s request-and-reply
// round trip, which is at most 10.5 s. A daemon that accepts connections and
// never answers therefore fails the readiness deadline plus one bounded health
// call per `wait_until_ready` and then the first bounded `create_session` or
// `send_user_turn`: the harness reports its own diagnostic within about two
// minutes instead of hanging until the CI timeout. The session reads carry the
// harness `SESSION_READ_DEADLINE` on top of the same client bound.
//
// `.github/workflows/real-api-e2e.yml` gives the run step 120 minutes and the
// job 150 minutes: the 74-minute worst case leaves a 46-minute step margin for
// the build, and the job keeps a 30-minute overhead above the step. A degraded
// live run reports the harness diagnostic ("did not record a succeeded <tool>
// call within 3 turns") instead of an opaque GitHub timeout; a change to any
// budget constant must keep that relation.

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
        let config_path = config_path(config_home.path());
        let config_document = provider.config_document();
        preflight_config_document(&config_path, provider, &config_document);
        write_config_document(config_home.path(), &config_document);
        let endpoint = unique_endpoint("real-api-e2e");
        let log_path = config_home.path().join("daemon-live-e2e.log");
        let daemon = spawn_daemon(
            &endpoint,
            config_home.path(),
            state_home.path(),
            Some(&log_path),
            &DAEMON_REMOVED_ENVIRONMENT,
        );
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
    /// bind, exactly like the hermetic client fixture. A clean daemon exit
    /// leaves the same file now that a dropped listener never unlinks its
    /// endpoint.
    fn restart_daemon(&mut self) {
        self.kill_daemon();
        self.daemon = Some(spawn_daemon(
            &self.endpoint,
            self.config_home.path(),
            self.state_home.path(),
            Some(&self.log_path),
            &DAEMON_REMOVED_ENVIRONMENT,
        ));
    }

    /// Kills the current daemon process within the bounded kill window.
    ///
    /// The first signal normally reaps the child on the first poll. When it
    /// does not, the hard kill is retried and the fallback reap is bounded by
    /// the same deadline, so a process that never becomes reapable produces
    /// this harness diagnostic instead of hanging the live pass until the CI
    /// timeout.
    fn kill_daemon(&mut self) {
        let Some(mut child) = self.daemon.take() else {
            return;
        };
        let deadline = Instant::now() + KILL_DEADLINE;
        let _ = child.kill();
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = child.kill();
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        // `LiveE2eHost::drop` runs this on the unwind path too, where a panic
        // would abort the process and hide the original failure.
        let diagnostic = "the daemon process exits within the bounded kill window";
        if std::thread::panicking() {
            eprintln!("real-api-e2e: {diagnostic}");
        } else {
            panic!("{diagnostic}");
        }
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
        if let Some(path) = common::endpoint_socket_path(&self.endpoint) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Builds the shared typed client for one fixture endpoint.
fn live_client(endpoint: &LocalEndpoint) -> IntentionClient {
    IntentionClient::new(
        endpoint.clone(),
        Box::new(
            ProcessDaemonLauncher::new(env!("CARGO_BIN_EXE_intention-daemon"))
                .expect("daemon program is valid"),
        ),
    )
}

/// Waits out the client's readiness wait until the daemon reports `Ready`.
///
/// `await_ready()` only connects and queries; it never launches the daemon.
/// Its own bounded wait is looped under the fixture deadline, so a daemon that
/// needs longer than one client budget still becomes ready in time. A child
/// that exits before readiness fails with its exit status instead of a
/// readiness timeout.
async fn wait_until_ready(host: &mut LiveE2eHost, deadline: Instant) -> IntentionClient {
    let client = live_client(&host.endpoint);
    while Instant::now() < deadline {
        let exit_status = host
            .daemon
            .as_mut()
            .and_then(|child| child.try_wait().ok().flatten());
        assert!(
            exit_status.is_none(),
            "the daemon exits before readiness with status {exit_status:?}"
        );
        if client.await_ready().await.is_ok() {
            return client;
        }
    }
    panic!("daemon becomes ready before the deadline");
}

/// Reads one session snapshot with a harness-side deadline.
///
/// The client bounds every request with its own timeout, so the read itself
/// cannot block forever; this helper keeps the harness deadline as an outer
/// bound. The deadline is enforced by the runtime timer, so a daemon that
/// accepts the connection and never answers fails with a bounded harness panic
/// instead of hanging the run until the CI step timeout.
async fn bounded_session_snapshot(
    endpoint: &LocalEndpoint,
    session_id: SessionId,
    deadline: Instant,
) -> SessionSnapshotDto {
    let client = live_client(endpoint);
    match tokio::time::timeout_at(
        tokio::time::Instant::from_std(deadline),
        client.session_snapshot(session_id),
    )
    .await
    {
        Ok(Ok(snapshot)) => snapshot,
        Ok(Err(error)) => panic!("the daemon serves the session snapshot: {error:?}"),
        Err(_) => panic!("the session snapshot arrives before the deadline"),
    }
}

/// Creates one Build-mode session rooted at the fixture workspace.
///
/// Every session of one fixture shares its project and workspace identity:
/// the durable workspace root is unique, so a second session over the same
/// root must reuse the workspace identity instead of minting a new one.
async fn create_session(host: &LiveE2eHost, session_id: SessionId, label: &str) {
    live_client(&host.endpoint)
        .create_session(CreateSessionCommandDto::new(
            host.project_id,
            session_id,
            host.workspace_id,
            WorkspaceRootDto::parse(host.workspace.path().to_string_lossy().into_owned())
                .expect("workspace root is absolute"),
            RunModeDto::Build,
        ))
        .await
        .unwrap_or_else(|error| {
            panic!("the daemon accepts session creation for {label}: {error:?}")
        });
}

/// Sends one user turn and returns the run it started.
async fn send_user_turn(endpoint: &LocalEndpoint, session_id: SessionId, content: &str) -> RunId {
    let outcome = live_client(endpoint)
        .send_user_turn(session_id, IdempotencyKey::new(), content.to_owned())
        .await
        .expect("user turn is accepted");
    let SendUserTurnOutcomeDto::Started { run_id, .. } = outcome else {
        panic!("the user turn starts a run, got: {outcome:?}")
    };
    run_id
}

/// One observed terminal run: the authoritative current run projection and the
/// committed transcript rows that carry the run's history.
///
/// The transcript is the durable record of user, assistant, tool-call,
/// tool-result, and notice content. The repository writes each `tool_results`
/// evidence row and its answering `tool_result` transcript row in one
/// transaction, so a call row paired with a result row of the same call
/// identity is the durable evidence of one recorded tool outcome.
struct ObservedRun {
    run: RunProjectionDto,
    messages: Vec<MessageProjectionDto>,
}

impl ObservedRun {
    /// Returns the authoritative terminal run status.
    const fn status(&self) -> RunStatusDto {
        self.run.status()
    }

    /// Returns whether the run committed any assistant transcript row.
    fn has_assistant_content(&self) -> bool {
        self.messages
            .iter()
            .any(|message| message.kind() == MessageKindDto::Assistant)
    }

    /// Reports whether the run committed the named tool call row.
    fn has_tool_call(&self, tool: &str) -> bool {
        self.messages.iter().any(|message| {
            message.kind() == MessageKindDto::ToolCall && message.tool_id() == Some(tool)
        })
    }

    /// Returns the distinct tool names recorded by committed call rows, in
    /// transcript order.
    fn recorded_tool_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for message in &self.messages {
            if message.kind() != MessageKindDto::ToolCall {
                continue;
            }
            if let Some(name) = message.tool_id()
                && !names.contains(&name)
            {
                names.push(name);
            }
        }
        names
    }

    /// Returns the canonical arguments of every committed call row of one tool.
    ///
    /// A mutating turn binds its durable effect to these arguments so a
    /// leftover run from an earlier evicted attempt cannot satisfy the turn's
    /// on-disk byte check on its own.
    fn tool_call_arguments(&self, tool: &str) -> Vec<&str> {
        self.messages
            .iter()
            .filter(|message| {
                message.kind() == MessageKindDto::ToolCall && message.tool_id() == Some(tool)
            })
            .map(MessageProjectionDto::text)
            .collect()
    }

    /// Returns the committed result content of every result row that answers a
    /// call of one tool, matched to its call by identity.
    ///
    /// A failed or interrupted tool outcome terminalizes the run or ends its
    /// operation with a notice, so a completed run's matched result rows are
    /// its non-failed durable outcomes.
    fn tool_result_contents(&self, tool: &str) -> Vec<&str> {
        self.messages
            .iter()
            .filter(|result| {
                result.kind() == MessageKindDto::ToolResult
                    && result.tool_id() == Some(tool)
                    && self.messages.iter().any(|call| {
                        call.kind() == MessageKindDto::ToolCall
                            && call.tool_id() == Some(tool)
                            && call.tool_call_id() == result.tool_call_id()
                    })
            })
            .map(MessageProjectionDto::text)
            .collect()
    }
}

/// The bounded outcome of one run-stream observation.
enum RunObservation {
    /// The run delivered its authoritative terminal status with its committed
    /// transcript rows.
    Terminal(Box<ObservedRun>),
    /// The connection was lost before the terminal status, because the daemon
    /// closed this subscriber or the stream ended. The caller may observe
    /// again.
    Lost(String),
}

/// Subscribes to one run and collects its current snapshot plus the committed
/// frames delivered until the terminal status.
///
/// The daemon answers a subscription with current durable state and then
/// delivers committed transcript rows and statuses live; there is no replay
/// tail and no cursor. The daemon bounds every subscriber queue and closes one
/// that cannot keep up instead of resynchronizing it, so a long provider
/// reasoning burst can close the subscriber. The caller observes again instead
/// of failing. A real provider can pause for several seconds inside one
/// attempt, so the whole observation is bounded by the deadline instead of by
/// per-frame read timeouts.
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
    .unwrap_or_else(|_| panic!("the run reaches a terminal status before the deadline"))
}

/// Reads one run stream until its terminal status or a closed connection.
async fn collect_run_frames(
    endpoint: &LocalEndpoint,
    session_id: SessionId,
    run_id: RunId,
) -> RunObservation {
    let client = RunStreamClient::new(endpoint.clone());
    let Ok(mut subscription) = client
        .subscribe(SubscribeRunCommandDto::new(session_id, run_id))
        .await
    else {
        return RunObservation::Lost("the run subscription failed".to_owned());
    };
    loop {
        if subscription
            .state()
            .status()
            .is_some_and(run_status_is_terminal)
        {
            let run = *subscription
                .state()
                .run()
                .expect("an accepted subscription snapshot carries the run projection");
            return RunObservation::Terminal(Box::new(ObservedRun {
                run,
                messages: subscription.state().messages().to_vec(),
            }));
        }
        match subscription.receive().await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return RunObservation::Lost(format!(
                    "the daemon closed the run stream after {} committed rows",
                    subscription.state().messages().len()
                ));
            }
            Err(_) => {
                return RunObservation::Lost("the run stream frame is invalid".to_owned());
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
/// argument differently from the prompt so the call is rejected or the durable
/// effect does not match, and the daemon closes a subscriber that cannot keep
/// up with a reasoning burst. Each of those consumes one attempt in a fresh
/// session, and `prepare` restores the workspace state that an earlier attempt
/// of a mutating turn may have changed. A run whose subscriber the daemon
/// closed is not cancelled, so it can keep modifying the workspace after the
/// next attempt's reseed; a mutating `effect` therefore binds the accepted
/// turn to this run's own committed call (its target path and byte count) and
/// never to the on-disk bytes alone, so a leftover run cannot satisfy a later
/// attempt's check by itself. A completed turn whose committed transcript
/// rows satisfy `effect` is accepted, and any other failure is still fatal.
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
        create_session(host, session_id, &format!("{tool} turn {attempt}")).await;
        let run_id = send_user_turn(&host.endpoint, session_id, prompt).await;
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
            // A failed live turn is model noise: the live model can answer
            // without a usable tool call, and an unparsable call terminalizes
            // the run as `Failed`. An interrupted run is daemon recovery,
            // which is never acceptable mid-turn, so it stays fatal.
            assert_eq!(
                observed.status(),
                RunStatusDto::Failed,
                "live provider {tool} turn {attempt} terminated without a completed run, recorded calls {:?}",
                observed.recorded_tool_names()
            );
            last_gap = Some(format!(
                "the live provider {tool} turn terminalized failed (recorded calls {:?})",
                observed.recorded_tool_names()
            ));
            continue;
        }
        if observed.has_tool_call(tool) && !observed.tool_result_contents(tool).is_empty() {
            if effect(&observed) {
                return (session_id, observed);
            }
            last_gap = Some(format!(
                "a completed run recorded a {tool} call whose durable effect was not observed"
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

/// Proves the real production tool loop against a live provider for every
/// advertised tool, then proves a restarted daemon serves a completed run's
/// recorded current state.
///
/// One live daemon drives all six active registry tools as separate live
/// sessions: `read`, `glob`, `grep`, `write`, `edit`, and `execute`. A turn is
/// accepted only when its run completed and recorded a `tool_call` transcript
/// row with a paired `tool_result` row that reached the turn's required
/// effect: the note text for `read` and `grep`, the workspace path list for
/// `glob`, the written bytes for `write` and `edit`, and the real child's
/// stdout with its typed exit status for `execute`. Every effect check runs
/// inside the bounded attempt budget, so a valid-but-different provider
/// argument consumes an attempt instead of failing the completed run. A turn
/// that misses those conditions, a turn that terminalizes `Failed`, and a turn
/// whose subscriber the daemon closed each consume one bounded attempt in a
/// fresh session; any other failed run, an interrupted run, or a timeout fails
/// the test.
#[tokio::test]
#[ignore = "opt-in live-provider e2e; see architecture 10; run via make e2e-real-api"]
async fn real_provider_tool_loop_drives_every_advertised_tool_and_survives_restart() {
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
    let _client = wait_until_ready(&mut host, Instant::now() + READINESS_DEADLINE).await;

    // `read`: the live model reads the seeded note through the real registry.
    let (read_session, read_run) = drive_tool_turn(
        &host,
        "read",
        &format!(
            "Use the read tool with the arguments {} to read that workspace file.",
            serde_json::json!({"path": "e2e-note.txt"})
        ),
        &|run| run.tool_result_contents("read").join("\n").contains(&token),
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
            run.tool_result_contents("glob")
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
        &|run| {
            run.tool_result_contents("grep")
                .join("\n")
                .contains(&token)
        },
    )
    .await;

    // `write`: the real registry creates the file with exactly those bytes.
    // Every attempt removes the target first and the effect requires the
    // exact bytes, so neither a stale artifact from an earlier attempt nor a
    // run that wrote elsewhere can satisfy the turn.
    let written = format!("written by the live provider: {token}");
    let written_file = host.workspace.path().join("e2e-written.txt");
    let reseed_written_file = || match std::fs::remove_file(&written_file) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            panic!("the write fixture target is removed before every attempt: {error}")
        }
    };
    let (write_session, write_run) = drive_tool_turn_with(
        &host,
        "write",
        &format!(
            "Use the write tool with the arguments {} to create that workspace file exactly as given.",
            serde_json::json!({"path": "e2e-written.txt", "content": written})
        ),
        &reseed_written_file,
        &|run| {
            let Ok(written_bytes) = std::fs::read_to_string(&written_file) else {
                return false;
            };
            // The effect binds to the observed run's own succeeded call for
            // the target path and byte count: an earlier attempt's still
            // running run may touch the file after the reseed, but it cannot
            // record this run's durable call.
            written_bytes == written
                && run.tool_result_contents("write").join("\n")
                    == format!("{} bytes", written_bytes.len())
                && run
                    .tool_call_arguments("write")
                    .iter()
                    .any(|arguments| arguments.contains("e2e-written.txt"))
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
            // As with the write turn, the effect binds to this run's own
            // succeeded call for the target path, so a leftover run from an
            // evicted attempt cannot satisfy the byte check alone.
            edited_bytes.contains(&edited_source)
                && run.tool_result_contents("edit").join("\n")
                    == format!("{} bytes", edited_bytes.len())
                && run
                    .tool_call_arguments("edit")
                    .iter()
                    .any(|arguments| arguments.contains("e2e-edit-source.txt"))
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
            let content = run.tool_result_contents("execute").join("\n");
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
    let run_id = read_run.run.run_id();

    host.restart_daemon();
    let _client = wait_until_ready(&mut host, Instant::now() + READINESS_DEADLINE).await;
    let stream_client = RunStreamClient::new(host.endpoint.clone());
    let subscription = tokio::time::timeout_at(
        tokio::time::Instant::from_std(Instant::now() + RESUBSCRIBE_DEADLINE),
        stream_client.subscribe(SubscribeRunCommandDto::new(read_session, run_id)),
    )
    .await
    .unwrap_or_else(|_| panic!("the post-restart subscription reply arrives before the deadline"))
    .expect("the post-restart subscription reply is valid");
    assert_eq!(
        subscription.state().status(),
        Some(RunStatusDto::Completed),
        "the restarted daemon serves the completed run"
    );
    assert_eq!(
        subscription.state().messages(),
        read_run.messages.as_slice(),
        "the restarted daemon serves the recorded transcript unchanged"
    );
    drop(subscription);

    // A completed durable run commits no further transcript rows. After a
    // bounded quiet window a fresh subscription still serves the same state.
    tokio::time::sleep(TERMINAL_QUIET_WINDOW).await;
    let mut reobserved = None;
    for _ in 1..=TOOL_TURN_ATTEMPTS {
        if let RunObservation::Terminal(observed) = collect_terminal_run(
            &host.endpoint,
            read_session,
            run_id,
            Instant::now() + RESUBSCRIBE_DEADLINE,
        )
        .await
        {
            reobserved = Some(*observed);
            break;
        }
    }
    let reobserved = reobserved.expect("the completed run stays observable after the quiet window");
    assert_eq!(reobserved.status(), RunStatusDto::Completed);
    assert_eq!(
        reobserved.messages, read_run.messages,
        "no transcript row commits after the run completed"
    );

    // Credential hygiene after the hard kill: no committed transcript row,
    // session snapshot, log, or state byte carries the credential value.
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
        let transcript_json =
            serde_json::to_string(&run.messages).expect("transcript rows serialize");
        assert_excludes_credential(
            &transcript_json,
            &credential,
            &format!("turn {} committed transcript rows", index + 1),
        );
    }
    for (index, session) in sessions.iter().enumerate() {
        let session_json = serde_json::to_string(
            &bounded_session_snapshot(
                &host.endpoint,
                *session,
                Instant::now() + SESSION_READ_DEADLINE,
            )
            .await,
        )
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
/// The run must terminalize `Failed`, the durable transcript must carry its
/// user turn and no assistant, tool, or notice row, and neither the transcript,
/// the session snapshot, nor the daemon log may carry the literal credential.
/// A model-authored row would mean raw provider output reached durable state,
/// which the containment invariant recorded in architecture 10 forbids.
#[tokio::test]
#[ignore = "opt-in live-provider e2e; see architecture 10; run via make e2e-real-api"]
async fn real_provider_rejects_invalid_credential_without_leak() {
    let Some(mut provider) = LiveProviderConfig::from_env() else {
        return;
    };
    let credential = "invalid-credential-live-e2e".to_owned();
    provider.credential.clone_from(&credential);
    let mut host = LiveE2eHost::new(&provider, &[]);
    let _client = wait_until_ready(&mut host, Instant::now() + READINESS_DEADLINE).await;

    let session_id = SessionId::new();
    create_session(&host, session_id, "the invalid-credential run").await;
    let run_id = send_user_turn(&host.endpoint, session_id, "Reply with a short greeting.").await;
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
            observed = Some(*terminal);
            break;
        }
    }
    let observed = observed
        .expect("the rejected live run reaches its terminal status within the bounded budget");

    assert_eq!(
        observed.status(),
        RunStatusDto::Failed,
        "the live provider rejects the invalid credential with a failed run"
    );
    assert!(
        !observed.messages.is_empty(),
        "the rejected live run commits its durable user turn before the provider is reached"
    );
    assert!(
        observed
            .messages
            .iter()
            .all(|message| message.kind() == MessageKindDto::User),
        "the rejected credential run records no assistant, tool, or notice row"
    );
    assert!(
        observed
            .messages
            .iter()
            .any(|message| message.text() == "Reply with a short greeting."),
        "the rejected credential run preserves its durable user turn"
    );
    assert!(
        !observed.has_assistant_content(),
        "the rejected credential run records no assistant text"
    );

    let transcript_json =
        serde_json::to_string(&observed.messages).expect("transcript rows serialize");
    let session_json = serde_json::to_string(
        &bounded_session_snapshot(
            &host.endpoint,
            session_id,
            Instant::now() + SESSION_READ_DEADLINE,
        )
        .await,
    )
    .expect("session snapshot serializes");
    assert_excludes_credential(&transcript_json, &credential, "serialized run transcript");
    assert_excludes_credential(&session_json, &credential, "session snapshot JSON");
    assert_excludes_credential(&host.captured_log(), &credential, "captured daemon log");
}
