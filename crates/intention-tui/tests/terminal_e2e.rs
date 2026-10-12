//! Terminal-client process end-to-end tests over real IPC and a real daemon.
//!
//! Every scenario spawns the real `intention-tui` binary with piped standard
//! streams and drives it against a real daemon process, whose provider endpoint
//! is either the scripted fake provider or a listener that never answers. The
//! daemon is spawned by the fixture, so the binary reaches it through the
//! client-declared `ENDPOINT_VARIABLE` process seam and never needs the sibling
//! daemon program or a terminal. The assertions cover the observable process contract: the
//! closed exit-status set, the NDJSON and text output, the committed
//! transcript, and the durable effect of the deadline interrupt.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Terminal end-to-end fixtures use assertion conveniences for precise diagnostics."
)]

use std::future::Future;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use intention_client::{ENDPOINT_VARIABLE, IntentionClient, RETAINED_TRANSCRIPT_MESSAGES};
use intention_proto::{MessageKindDto, SessionId, SessionSnapshotDto};
use intention_test_support::daemon_process::{
    self, spawn_daemon, unique_endpoint, wait_until_ready, write_config_document,
};
use intention_test_support::fake_provider::{FakeProvider, fixture_config_document};
use intention_transport::LocalEndpoint;
use serde_json::Value;
use tempfile::TempDir;

/// The bounded window the spawned daemon has to report readiness in.
const DAEMON_DEADLINE: Duration = Duration::from_secs(20);

/// The bounded window one spawned terminal process has to exit in.
const CLI_DEADLINE: Duration = Duration::from_secs(30);

/// The bounded window one post-run durable observation has to appear in.
///
/// The durable observation it bounds belongs to the signal scenarios, so the
/// constant is gated with them.
#[cfg(unix)]
const OBSERVE_DEADLINE: Duration = Duration::from_secs(15);

/// The poll interval of the bounded process and durable-effect waits.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// The wait after a dispatched turn before a test signals the front end.
///
/// The wait covers the window between the committed user row and the signal
/// handler the run's own waits install.
#[cfg(unix)]
const SIGNAL_WAIT: Duration = Duration::from_millis(500);

/// The prompt every headless scenario sends.
const PROMPT: &str = "read the fixture file";

/// The workspace file the scripted `read` tool call opens.
const WORKSPACE_FILE: (&str, &str) = ("hello.txt", "hello from d1");

/// The tool arguments the scripted provider round requests.
const TOOL_ARGUMENTS: &str = r#"{"path":"hello.txt"}"#;

/// The assistant text the scripted provider streams and commits.
const COMMITTED_TEXT: &str = "done";

/// The reasoning chunks the reasoning script streams before that answer.
const REASONING_CHUNKS: [&str; 2] = ["weighing ", "the fixture file"];

/// The fixture provider credential; no scenario asserts on it.
const CREDENTIAL: &str = "d1-terminal-fixture-credential";

/// The closed set of record kinds the headless script interface declares.
const RECORD_KINDS: [&str; 5] = ["delta", "message", "status", "error", "result"];

/// One terminal end-to-end host: isolated configuration, state, and workspace
/// directories, one provider endpoint, one spawned daemon process, and the
/// endpoint the terminal binary addresses.
///
/// Dropping the host kills the daemon, stops the provider, and removes the
/// daemon-owned Unix socket so a later run can bind the same logical endpoint.
struct TerminalHost {
    config_home: TempDir,
    state_home: TempDir,
    workspace: TempDir,
    provider: ProviderEndpoint,
    daemon: Option<Child>,
    /// The file the spawned daemon writes both of its output streams to.
    ///
    /// The daemon's log is where a failed run's safe reason is observable: the
    /// reason is durable in the run's own outcome, and no client-visible
    /// projection carries it, so the run's JSON output cannot name it.
    daemon_log: PathBuf,
    endpoint: LocalEndpoint,
    client: IntentionClient,
}

impl TerminalHost {
    /// Creates a fresh isolated host and waits for its daemon to report ready.
    fn new(provider: ProviderEndpoint, workspace_file: Option<(&str, &str)>) -> Self {
        assert!(
            daemon_process::daemon_binary_path().is_file(),
            "the daemon binary is built before the terminal suite runs: cargo build -p intention-daemon"
        );
        let config_home = TempDir::new().expect("config directory exists");
        let state_home = TempDir::new().expect("state directory exists");
        let workspace = TempDir::new().expect("workspace directory exists");
        if let Some((name, content)) = workspace_file {
            std::fs::write(workspace.path().join(name), content).expect("workspace fixture writes");
        }
        write_config_document(config_home.path(), &provider.config_document());
        let endpoint = unique_endpoint("d1-terminal");
        let daemon_log = state_home.path().join("daemon.log");
        let daemon = spawn_daemon(
            &endpoint,
            config_home.path(),
            state_home.path(),
            Some(daemon_log.as_path()),
            &[],
        );
        let client = block_on(wait_until_ready(
            &endpoint,
            Instant::now() + DAEMON_DEADLINE,
        ));
        Self {
            config_home,
            state_home,
            workspace,
            provider,
            daemon: Some(daemon),
            daemon_log,
            endpoint,
            client,
        }
    }

    /// Renders one finished front end and this host's daemon output.
    ///
    /// The JSON format reports a run's own account of a failure on standard
    /// output, and the daemon's log carries the safe reason of a run that
    /// failed, which no client-visible projection holds: a status assertion
    /// that named neither would hide the account of the very failure it
    /// reports.
    fn failure_diagnostic(&self, output: &CliOutput) -> String {
        let daemon = std::fs::read_to_string(&self.daemon_log).unwrap_or_default();
        format!(
            "stderr={} stdout={} daemon={}",
            output.stderr, output.stdout, daemon
        )
    }

    /// Returns the scripted provider serving this host's daemon.
    fn scripted_provider(&self) -> &FakeProvider {
        match &self.provider {
            ProviderEndpoint::Scripted(provider) => provider,
            ProviderEndpoint::Stalled(_provider) => {
                panic!("a stalled host carries no scripted provider")
            }
        }
    }

    /// Returns the absolute workspace root the scenario configures.
    fn workspace_root(&self) -> String {
        self.workspace.path().to_string_lossy().into_owned()
    }

    /// Runs the real terminal binary with a bounded wait and piped streams.
    ///
    /// A supplied input is written and the pipe is closed, so a line front end
    /// reads it as a script and then as end-of-input.
    fn run(&self, arguments: &[&str], input: Option<&str>) -> CliOutput {
        let mut command = Command::new(env!("CARGO_BIN_EXE_intention-tui"));
        command.args(arguments);
        command.env(ENDPOINT_VARIABLE, self.endpoint.instance_id());
        redirect_directories(
            &mut command,
            self.config_home.path(),
            self.state_home.path(),
        );
        command
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("the terminal binary spawns");
        if let Some(input) = input {
            let mut stdin = child
                .stdin
                .take()
                .expect("the piped stdin handle is present");
            stdin
                .write_all(input.as_bytes())
                .expect("the scripted input writes");
            // The dropped handle closes the pipe; the line front ends read that
            // as end-of-input after the scripted lines.
        }
        collect(&mut child, Instant::now() + CLI_DEADLINE)
    }

    /// Spawns the real terminal binary with piped streams the test drives.
    ///
    /// The stdin pipe stays open: a line front end waits for its next line
    /// while the test signals the running process. Only the signal scenarios
    /// drive a process this way.
    #[cfg(unix)]
    fn spawn(&self, arguments: &[&str]) -> Child {
        let mut command = Command::new(env!("CARGO_BIN_EXE_intention-tui"));
        command.args(arguments);
        command.env(ENDPOINT_VARIABLE, self.endpoint.instance_id());
        redirect_directories(
            &mut command,
            self.config_home.path(),
            self.state_home.path(),
        );
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command.spawn().expect("the terminal binary spawns")
    }

    /// Reads the current durable snapshot of one session through the client.
    fn snapshot(&self, session_id: SessionId) -> SessionSnapshotDto {
        block_on(self.client.session_snapshot(session_id)).expect("the session snapshot reads")
    }

    /// Replaces the endpoint path with a directory no daemon can bind again.
    ///
    /// Unlinking a bound Unix socket leaves the listener that already holds it
    /// working and its established connections open, so the live stream keeps
    /// flowing until the daemon dies; afterwards the path belongs to a directory
    /// that no new daemon can bind and no client can connect to, which is what
    /// makes a lost stream unrecoverable for the front end under test.
    #[cfg(unix)]
    fn occupy_endpoint(&self) {
        let path = daemon_process::endpoint_socket_path(&self.endpoint)
            .expect("the endpoint is a filesystem socket");
        std::fs::remove_file(&path).expect("the endpoint socket is removed");
        std::fs::create_dir(&path).expect("the endpoint path becomes a directory");
    }
}

impl Drop for TerminalHost {
    fn drop(&mut self) {
        daemon_process::kill(&mut self.daemon);
        #[cfg(unix)]
        if let Some(path) = daemon_process::endpoint_socket_path(&self.endpoint) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// One finished terminal process: its exit code and its two output streams.
struct CliOutput {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Waits for one spawned front end and collects its two output streams.
fn collect(child: &mut Child, deadline: Instant) -> CliOutput {
    let stdout = child
        .stdout
        .take()
        .expect("the piped stdout handle is present");
    let stderr = child
        .stderr
        .take()
        .expect("the piped stderr handle is present");
    let stdout_reader = thread::spawn(move || read_all(stdout));
    let stderr_reader = thread::spawn(move || read_all(stderr));
    let status = wait_for_exit(child, deadline);
    let stdout = stdout_reader.join().expect("the stdout reader finishes");
    let stderr = stderr_reader.join().expect("the stderr reader finishes");
    CliOutput {
        code: status.code(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    }
}

/// Waits for the turn one spawned front end sent to reach the daemon.
///
/// The committed user row proves the front end dispatched its prompt, and the
/// wait that follows it lets the run stream open before the test signals.
#[cfg(unix)]
fn wait_for_user_turn(host: &TerminalHost) -> SessionId {
    let deadline = Instant::now() + OBSERVE_DEADLINE;
    loop {
        // The client resolves the newest session: a daemon that lists none is
        // its typed `no_sessions_to_continue` failure, which this wait polls
        // through until the front end's session exists.
        if let Ok(session) = block_on(host.client.continue_session()) {
            let snapshot = host.snapshot(session);
            if snapshot
                .messages()
                .iter()
                .any(|message| message.kind() == MessageKindDto::User)
            {
                thread::sleep(SIGNAL_WAIT);
                return session;
            }
        }
        assert!(
            Instant::now() < deadline,
            "the front end's turn reaches the daemon before the deadline"
        );
        thread::sleep(POLL_INTERVAL);
    }
}

/// Sends SIGINT to one spawned front end, as the terminal itself would.
#[cfg(unix)]
fn send_sigint(child: &Child) {
    let status = Command::new("kill")
        .arg("-INT")
        .arg(child.id().to_string())
        .status()
        .expect("the signal reaches the spawned process");
    assert!(status.success(), "the signal is delivered");
}

/// Waits for the durable stop notice a stopped run commits.
fn assert_stopped_run_is_durable(host: &TerminalHost, session: SessionId) {
    let messages = host.snapshot(session).messages().to_vec();
    assert_eq!(
        messages
            .last()
            .map(|message| (message.kind(), message.text())),
        Some((
            MessageKindDto::Notice,
            "[The run was stopped before a final answer.]"
        )),
        "the stop ends the run durably instead of leaving it running"
    );
    let active = host.snapshot(session).projection().active_run().is_none();
    assert!(active, "the stop ends the run itself");
}

/// The provider endpoint one host's daemon is configured against.
enum ProviderEndpoint {
    /// The scripted fake provider answering both fixture rounds.
    Scripted(FakeProvider),
    /// A provider endpoint accepting requests without ever answering them.
    Stalled(StalledProvider),
}

impl ProviderEndpoint {
    /// Starts the scripted fake provider the fixture rounds are served from.
    fn scripted() -> Self {
        Self::Scripted(FakeProvider::start(TOOL_ARGUMENTS))
    }

    /// Starts the scripted fake provider whose text round streams the thinking
    /// channel before the answer it informs.
    fn scripted_with_reasoning() -> Self {
        Self::Scripted(FakeProvider::start_with_reasoning(
            TOOL_ARGUMENTS,
            &REASONING_CHUNKS,
        ))
    }

    /// Binds a provider endpoint that never answers a request.
    fn stalled() -> Self {
        Self::Stalled(StalledProvider::start())
    }

    /// Starts the scripted fake provider serving `rounds` tool rounds and one
    /// final text round.
    fn rounds(rounds: usize) -> Self {
        Self::Scripted(FakeProvider::start_rounds("read", TOOL_ARGUMENTS, rounds))
    }

    /// Returns the loopback port the daemon's provider configuration points at.
    const fn port(&self) -> u16 {
        match self {
            Self::Scripted(provider) => provider.port(),
            Self::Stalled(provider) => provider.port(),
        }
    }

    /// Returns the daemon configuration document addressing this endpoint.
    fn config_document(&self) -> String {
        fixture_config_document(self.port(), CREDENTIAL)
    }
}

impl Drop for ProviderEndpoint {
    fn drop(&mut self) {
        if let Self::Scripted(provider) = self {
            provider.stop();
        }
    }
}

/// A provider endpoint that accepts requests and never answers them.
///
/// The listener is never polled, so every request stays in the accept backlog
/// until the test ends: the provider round stays in flight and the run can only
/// end through the deadline interrupt.
struct StalledProvider {
    _listener: TcpListener,
    port: u16,
}

impl StalledProvider {
    /// Binds a stalled provider on one loopback port.
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("stalled provider binds");
        let port = listener
            .local_addr()
            .expect("stalled provider port is available")
            .port();
        Self {
            _listener: listener,
            port,
        }
    }

    /// Returns the loopback port the provider accepts requests on.
    const fn port(&self) -> u16 {
        self.port
    }
}

/// Redirects the platform configuration and state directories into the host's
/// temporary directories, mirroring the daemon fixture's own overrides so both
/// processes resolve the same locations.
fn redirect_directories(command: &mut Command, config_home: &Path, state_home: &Path) {
    #[cfg(target_os = "linux")]
    {
        command.env("XDG_CONFIG_HOME", config_home);
        command.env("XDG_STATE_HOME", state_home);
    }
    #[cfg(target_os = "macos")]
    {
        command.env("HOME", config_home);
    }
    #[cfg(windows)]
    {
        command.env("APPDATA", config_home);
        command.env("LOCALAPPDATA", state_home);
    }
}

/// Waits for one child process within a bounded window and reaps it on expiry.
fn wait_for_exit(child: &mut Child, deadline: Instant) -> ExitStatus {
    loop {
        if let Some(status) = child
            .try_wait()
            .expect("the terminal process wait succeeds")
        {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "the terminal client exits within {} seconds",
                CLI_DEADLINE.as_secs()
            );
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Reads every byte one child output stream produces until it closes.
fn read_all(mut stream: impl Read) -> Vec<u8> {
    let mut buffer = Vec::new();
    let _ = stream.read_to_end(&mut buffer);
    buffer
}

/// Runs one asynchronous fixture step on a current-thread test runtime.
fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the test runtime builds")
        .block_on(future)
}

/// Parses every non-empty standard-output line as one NDJSON record.
fn json_records(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .unwrap_or_else(|_| panic!("every stdout line is one JSON record: {line}"))
        })
        .collect()
}

/// Returns the record kind of one NDJSON record.
fn kind(record: &Value) -> &str {
    record["record"]
        .as_str()
        .expect("every record carries its kind")
}

/// Returns the text of one delta or message record.
fn text(record: &Value) -> &str {
    record["text"]
        .as_str()
        .expect("every delta and message record carries its text")
}

#[test]
fn the_json_run_reports_transient_deltas_before_the_committed_row() {
    let host = TerminalHost::new(ProviderEndpoint::scripted(), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    let output = host.run(
        &["run", PROMPT, "--format", "json", "--workspace", &workspace],
        None,
    );
    assert_eq!(
        output.code,
        Some(0),
        "the run completes after {} provider requests and {} excess requests: {}",
        host.scripted_provider().request_count(),
        host.scripted_provider().excess_count(),
        host.failure_diagnostic(&output)
    );
    assert!(output.stderr.is_empty(), "stderr: {}", output.stderr);

    let records = json_records(&output.stdout);
    assert!(
        records
            .iter()
            .all(|record| RECORD_KINDS.contains(&kind(record))),
        "every record kind belongs to the declared set: {records:?}"
    );
    assert!(
        !records.iter().any(|record| kind(record) == "error"),
        "a completed run reports no error record"
    );

    let result = records.last().expect("the run reports one final result");
    assert_eq!(kind(result), "result", "the result record ends the output");
    assert_eq!(result["outcome"].as_str(), Some("completed"));
    assert_eq!(
        records
            .iter()
            .filter(|record| kind(record) == "result")
            .count(),
        1,
        "the run reports exactly one final result"
    );
    let session_id = SessionId::parse(
        result["session_id"]
            .as_str()
            .expect("the result reports its session"),
    )
    .expect("the reported session identity is canonical");
    assert!(
        result["run_id"].as_str().is_some(),
        "the result reports the observed run"
    );

    assert!(
        records
            .iter()
            .any(|record| kind(record) == "status" && record["status"] == "completed"),
        "the committed completion is reported as a status record"
    );
    assert!(
        records.iter().any(|record| kind(record) == "message"
            && record["kind"] == "user"
            && text(record) == PROMPT),
        "the accepted turn is reported as its committed row"
    );
    assert!(
        records
            .iter()
            .any(|record| kind(record) == "message" && record["kind"] == "tool_call"),
        "the executed tool call is reported as its committed row"
    );
    assert!(
        records.iter().any(|record| kind(record) == "message"
            && record["kind"] == "tool_result"
            && text(record) == WORKSPACE_FILE.1),
        "the tool result row carries the exact file content"
    );

    let assistant = |record: &Value| {
        kind(record) == "message" && record["kind"] == "assistant" && text(record) == COMMITTED_TEXT
    };
    let assistant_index = records
        .iter()
        .position(assistant)
        .expect("the streamed answer is committed as one assistant row");
    assert_eq!(
        records.iter().filter(|record| assistant(record)).count(),
        1,
        "the committed answer is reported exactly once"
    );

    let deltas: Vec<&Value> = records
        .iter()
        .filter(|record| kind(record) == "delta")
        .collect();
    assert!(
        !deltas.is_empty(),
        "the provider text is reported as transient delta records"
    );
    for delta in &deltas {
        assert_eq!(
            delta["transient"].as_bool(),
            Some(true),
            "every delta marks itself transient"
        );
        assert!(
            !text(delta).is_empty(),
            "every delta carries non-empty text"
        );
    }
    let stream_step = (*deltas.last().expect("the delta set is non-empty"))["step"]
        .as_u64()
        .expect("every delta carries its model step");
    let streamed: String = deltas
        .iter()
        .filter(|delta| delta["step"].as_u64() == Some(stream_step))
        .map(|delta| text(delta))
        .collect();
    assert_eq!(
        streamed, COMMITTED_TEXT,
        "the streamed text of the step is the committed row text"
    );
    assert_eq!(
        stream_step, 1,
        "the tool round is step zero and the streamed answer is the second model step"
    );
    let last_delta = records
        .iter()
        .rposition(|record| kind(record) == "delta")
        .expect("the delta set is non-empty");
    assert!(
        last_delta < assistant_index,
        "every delta of the step precedes the committed row that supersedes it"
    );

    assert_eq!(
        host.scripted_provider().request_count(),
        2,
        "the provider serves the tool round and its follow-up round"
    );
    assert_eq!(
        host.scripted_provider().excess_count(),
        0,
        "no further provider request follows completion"
    );
    let snapshot = host.snapshot(session_id);
    assert_eq!(
        snapshot
            .messages()
            .iter()
            .map(|message| message.kind())
            .collect::<Vec<_>>(),
        vec![
            MessageKindDto::User,
            MessageKindDto::ToolCall,
            MessageKindDto::ToolResult,
            MessageKindDto::Assistant,
        ],
        "the reported session holds the committed transcript"
    );
}

#[test]
fn a_run_past_the_retention_bound_reports_every_committed_row() {
    // The reporter walks the client's rows by durable identity, so a run that
    // commits more rows than the client retains still reports every one of
    // them: a row the bound drains after it was reported never hides the rows
    // that follow it.
    let rounds = RETAINED_TRANSCRIPT_MESSAGES / 2 + 1;
    let committed = rounds * 2 + 2;
    assert!(
        committed > RETAINED_TRANSCRIPT_MESSAGES,
        "the fixture run crosses the client's retention bound"
    );
    let host = TerminalHost::new(ProviderEndpoint::rounds(rounds), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    let output = host.run(
        &["run", PROMPT, "--format", "json", "--workspace", &workspace],
        None,
    );
    assert_eq!(
        output.code,
        Some(0),
        "the run completes after {} provider requests and {} excess requests: {}",
        host.scripted_provider().request_count(),
        host.scripted_provider().excess_count(),
        host.failure_diagnostic(&output)
    );
    assert!(output.stderr.is_empty(), "stderr: {}", output.stderr);

    let records = json_records(&output.stdout);
    let messages: Vec<&Value> = records
        .iter()
        .filter(|record| kind(record) == "message")
        .collect();
    assert_eq!(
        messages.len(),
        committed,
        "every committed row is reported exactly once"
    );
    assert_eq!(
        messages
            .iter()
            .filter(|record| record["kind"] == "tool_call")
            .count(),
        rounds,
        "every tool round's call is reported"
    );
    assert_eq!(
        messages
            .iter()
            .filter(|record| record["kind"] == "tool_result")
            .count(),
        rounds,
        "every tool round's result is reported"
    );
    assert_eq!(
        messages
            .iter()
            .filter(|record| record["kind"] == "user" && text(record) == PROMPT)
            .count(),
        1,
        "the accepted turn is reported once"
    );
    assert_eq!(
        messages
            .iter()
            .filter(|record| record["kind"] == "assistant" && text(record) == COMMITTED_TEXT)
            .count(),
        1,
        "the final answer is reported once"
    );

    let result = records.last().expect("the run reports one final result");
    assert_eq!(kind(result), "result", "the result record ends the output");
    assert_eq!(result["outcome"].as_str(), Some("completed"));
    assert_eq!(
        host.scripted_provider().request_count(),
        rounds + 1,
        "the provider serves every tool round and the answer that ends the run"
    );
    assert_eq!(
        host.scripted_provider().excess_count(),
        0,
        "no request follows the script"
    );
}

#[test]
fn the_json_run_reports_the_reasoning_channel_before_the_answer_it_informs() {
    let host = TerminalHost::new(
        ProviderEndpoint::scripted_with_reasoning(),
        Some(WORKSPACE_FILE),
    );
    let workspace = host.workspace_root();
    let output = host.run(
        &["run", PROMPT, "--format", "json", "--workspace", &workspace],
        None,
    );
    assert_eq!(output.code, Some(0), "the run completes: {}", output.stderr);
    assert!(output.stderr.is_empty(), "stderr: {}", output.stderr);

    let records = json_records(&output.stdout);
    let channel = |wanted: &'static str| {
        records
            .iter()
            .filter(|record| kind(record) == "delta" && record["channel"] == wanted)
            .collect::<Vec<_>>()
    };
    let reasoning = channel("reasoning");
    let answers = channel("answer");
    assert!(
        !reasoning.is_empty(),
        "the thinking channel reaches the terminal as its own delta records"
    );
    assert_eq!(
        reasoning
            .iter()
            .map(|record| text(record))
            .collect::<String>(),
        REASONING_CHUNKS.concat(),
        "the reasoning channel keeps the provider's chunks in their order"
    );
    assert_eq!(
        answers
            .iter()
            .map(|record| text(record))
            .collect::<String>(),
        COMMITTED_TEXT,
        "the answer channel is reported apart from the reasoning it follows"
    );
    assert!(
        reasoning
            .iter()
            .chain(answers.iter())
            .all(|record| record["channel"] == "reasoning" || record["channel"] == "answer"),
        "every delta names one of the step's two channels"
    );
    let step = reasoning[0]["step"]
        .as_u64()
        .expect("every delta carries its step");
    assert!(
        reasoning
            .iter()
            .chain(answers.iter())
            .all(|record| record["step"].as_u64() == Some(step)),
        "both channels of the step carry the same model step index"
    );

    let last_reasoning = records
        .iter()
        .rposition(|record| kind(record) == "delta" && record["channel"] == "reasoning")
        .expect("the reasoning channel is non-empty");
    let last_answer = records
        .iter()
        .rposition(|record| kind(record) == "delta" && record["channel"] == "answer")
        .expect("the answer channel is non-empty");
    let assistant = records
        .iter()
        .position(|record| {
            kind(record) == "message"
                && record["kind"] == "assistant"
                && text(record) == COMMITTED_TEXT
        })
        .expect("the streamed answer is committed as one assistant row");
    assert!(
        last_reasoning < last_answer && last_answer < assistant,
        "the reasoning and the answer both precede the committed row that supersedes them"
    );

    // The channel is transient, the row is durable: the same reasoning the live
    // segment streamed is what the committed assistant row carries.
    let session_id = SessionId::parse(
        records
            .last()
            .and_then(|result| result["session_id"].as_str())
            .expect("the result reports its session"),
    )
    .expect("the reported session identity is canonical");
    let snapshot = host.snapshot(session_id);
    let committed = snapshot
        .messages()
        .iter()
        .find(|message| message.kind() == MessageKindDto::Assistant)
        .expect("the run commits one assistant row");
    assert_eq!(
        committed.reasoning(),
        Some(REASONING_CHUNKS.concat().as_str()),
        "the committed row carries the reasoning the live channel streamed"
    );
    assert_eq!(committed.text(), COMMITTED_TEXT);
}

#[test]
fn the_text_run_streams_the_answer_and_reports_completion() {
    let host = TerminalHost::new(ProviderEndpoint::scripted(), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    let output = host.run(&["run", PROMPT, "--workspace", &workspace], None);
    assert_eq!(output.code, Some(0), "the run completes: {}", output.stderr);
    assert!(output.stderr.is_empty(), "stderr: {}", output.stderr);
    assert!(
        output.stdout.ends_with("run completed\n"),
        "the text report ends with the completed outcome: {}",
        output.stdout
    );
    assert_eq!(
        output.stdout.matches(COMMITTED_TEXT).count(),
        1,
        "the committed row supersedes the streamed text instead of repeating it: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains(WORKSPACE_FILE.1),
        "the tool result row carries the exact file content: {}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("\"record\""),
        "the text format writes no NDJSON records"
    );
}

#[test]
fn the_binary_addresses_the_clients_declared_endpoint() {
    let host = TerminalHost::new(ProviderEndpoint::scripted(), Some(WORKSPACE_FILE));
    let platform = LocalEndpoint::platform_default().expect("the platform default resolves");
    assert_ne!(
        host.endpoint.instance_id(),
        platform.instance_id(),
        "the fixture endpoint differs from the platform default, so a completed run proves the binary reads the declared variable"
    );
    let workspace = host.workspace_root();
    // `TerminalHost::run` addresses the daemon through the client's declared
    // `ENDPOINT_VARIABLE` surface; the process reaches this fixture daemon and
    // completes its run only by reading that declared endpoint.
    let output = host.run(&["run", PROMPT, "--workspace", &workspace], None);
    assert_eq!(output.code, Some(0), "the run completes: {}", output.stderr);
    assert!(
        output.stdout.contains(COMMITTED_TEXT),
        "the committed row reaches the process over the declared endpoint: {}",
        output.stdout
    );
}

#[test]
fn a_multi_line_prompt_reaches_the_session_unchanged() {
    let host = TerminalHost::new(ProviderEndpoint::scripted(), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    let prompt = "first line\nsecond line";
    let output = host.run(&["run", prompt, "--workspace", &workspace], None);
    assert_eq!(output.code, Some(0), "the run completes: {}", output.stderr);
    let sessions = block_on(host.client.list_sessions()).expect("the session list reads");
    let session = sessions
        .sessions()
        .first()
        .map(|summary| summary.session_id())
        .expect("the run created its session");
    let snapshot = host.snapshot(session);
    let user = snapshot
        .messages()
        .iter()
        .find(|message| message.kind() == MessageKindDto::User)
        .expect("the prompt committed its user row");
    assert_eq!(
        user.text(),
        prompt,
        "the prompt's lines reach the session unchanged"
    );
}

#[test]
fn a_slash_prefixed_prompt_reaches_the_session_as_a_turn() {
    // The prompt is a turn and not an interactive command: `/theme light` must
    // reach the session instead of opening the picker and writing a preference.
    let host = TerminalHost::new(ProviderEndpoint::scripted(), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    let committed = block_on(host.client.tui_settings()).expect("the settings read");
    let prompt = "/theme light";
    let output = host.run(
        &["run", prompt, "--format", "json", "--workspace", &workspace],
        None,
    );
    assert_eq!(output.code, Some(0), "the run completes: {}", output.stderr);

    let records = json_records(&output.stdout);
    let result = records.last().expect("the run reports one final result");
    assert_eq!(kind(result), "result", "the result record ends the output");
    let session_id = SessionId::parse(
        result["session_id"]
            .as_str()
            .expect("the result reports its session"),
    )
    .expect("the reported session identity is canonical");
    let snapshot = host.snapshot(session_id);
    let user = snapshot
        .messages()
        .iter()
        .find(|message| message.kind() == MessageKindDto::User)
        .expect("the prompt committed its user row");
    assert_eq!(
        user.text(),
        prompt,
        "a slash-prefixed prompt reaches the session as a turn"
    );
    let sessions = block_on(host.client.list_sessions()).expect("the session list reads");
    assert_eq!(
        sessions.sessions().len(),
        1,
        "the prompt created exactly the session it needed"
    );
    let after = block_on(host.client.tui_settings()).expect("the settings read");
    assert_eq!(
        after.theme(),
        committed.theme(),
        "a prompt never writes a theme preference"
    );
}

#[test]
fn a_prompt_ending_in_a_backslash_reaches_the_session_unchanged() {
    // The backslash escape belongs to the interactive input line: a scripted
    // prompt is data, so its trailing backslash is never a line break.
    let host = TerminalHost::new(ProviderEndpoint::scripted(), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    let prompt = "keep the trailing backslash\\";
    let output = host.run(&["run", prompt, "--workspace", &workspace], None);
    assert_eq!(output.code, Some(0), "the run completes: {}", output.stderr);
    let sessions = block_on(host.client.list_sessions()).expect("the session list reads");
    let session = sessions
        .sessions()
        .first()
        .map(|summary| summary.session_id())
        .expect("the run created its session");
    let snapshot = host.snapshot(session);
    let user = snapshot
        .messages()
        .iter()
        .find(|message| message.kind() == MessageKindDto::User)
        .expect("the prompt committed its user row");
    assert_eq!(
        user.text(),
        prompt,
        "a trailing backslash is never a line break for a scripted prompt"
    );
}

#[test]
fn the_repl_streams_a_piped_turn_and_exits_on_quit() {
    let host = TerminalHost::new(ProviderEndpoint::scripted(), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    let script = format!("/new\n{PROMPT}\n/quit\n");
    let output = host.run(&["repl", "--workspace", &workspace], Some(&script));
    assert_eq!(output.code, Some(0), "the REPL exits: {}", output.stderr);
    assert!(
        output.stdout.starts_with("> "),
        "the line loop prompts on standard output: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains(" open\n"),
        "the created session is reported: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains(COMMITTED_TEXT),
        "the piped turn streams the answer: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains("run completed"),
        "the piped turn reports completion: {}",
        output.stdout
    );
    assert_eq!(
        output.stdout.matches(COMMITTED_TEXT).count(),
        1,
        "the committed row supersedes the streamed text: {}",
        output.stdout
    );
    assert!(
        !output.stderr.contains("error:"),
        "the scripted REPL reports no failure: {}",
        output.stderr
    );
}

#[test]
fn a_repl_prompt_creates_the_session_it_needs() {
    let host = TerminalHost::new(ProviderEndpoint::scripted(), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    // No `/new`: with no session open, the prompt itself must start the
    // session it needs.
    let script = format!("{PROMPT}\n/quit\n");
    let output = host.run(&["repl", "--workspace", &workspace], Some(&script));
    assert_eq!(output.code, Some(0), "the REPL exits: {}", output.stderr);
    assert!(
        output.stdout.contains(COMMITTED_TEXT),
        "the prompt streams its answer: {}",
        output.stdout
    );
    assert!(
        !output.stderr.contains("error:"),
        "the prompt reports no failure: {}",
        output.stderr
    );
    let sessions = block_on(host.client.list_sessions()).expect("the session list reads");
    assert_eq!(
        sessions.sessions().len(),
        1,
        "the prompt created exactly the session it needed"
    );
    let created = sessions.sessions()[0].session_id();
    assert_eq!(
        host.snapshot(created)
            .messages()
            .first()
            .map(|message| (message.kind(), message.text().to_owned())),
        Some((MessageKindDto::User, PROMPT.to_owned())),
        "the prompt is the created session's committed first turn"
    );
}

#[test]
fn a_first_prompt_never_lands_in_another_session_when_a_second_session_exists() {
    let host = TerminalHost::new(ProviderEndpoint::scripted(), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    // Seed one durable session, so the store lists more than the one the prompt
    // needs and the danger MR-19 named - a prompt delivered to whichever
    // session arrives first - is live.
    let seeded = host.run(&["repl", "--workspace", &workspace], Some("/new\n/quit\n"));
    assert_eq!(
        seeded.code,
        Some(0),
        "the seeding REPL exits: {}",
        seeded.stderr
    );
    let existing = block_on(host.client.list_sessions())
        .expect("the session list reads")
        .sessions()
        .first()
        .map(|summary| summary.session_id())
        .expect("the seeded session is listed");
    assert!(
        host.snapshot(existing).messages().is_empty(),
        "the seeded session starts empty"
    );

    // The first prompt with no session open creates its own session and commits
    // the prompt there; the pre-existing session never receives it.
    let script = format!("{PROMPT}\n/quit\n");
    let output = host.run(&["repl", "--workspace", &workspace], Some(&script));
    assert_eq!(output.code, Some(0), "the REPL exits: {}", output.stderr);
    assert!(
        !output.stderr.contains("error:"),
        "the prompt reports no failure: {}",
        output.stderr
    );

    let sessions = block_on(host.client.list_sessions()).expect("the session list reads");
    assert_eq!(
        sessions.sessions().len(),
        2,
        "the prompt created exactly one more session"
    );
    let created = sessions
        .sessions()
        .iter()
        .map(|summary| summary.session_id())
        .find(|session| *session != existing)
        .expect("the prompt's session is listed beside the seeded one");
    assert!(
        host.snapshot(created)
            .messages()
            .iter()
            .any(|message| { message.kind() == MessageKindDto::User && message.text() == PROMPT }),
        "the created session holds the prompt as its first turn"
    );
    assert!(
        host.snapshot(existing)
            .messages()
            .iter()
            .all(|message| message.kind() != MessageKindDto::User),
        "the pre-existing session never receives the prompt"
    );
}

#[test]
fn an_explicit_continuation_reuses_the_session_instead_of_creating_one() {
    let host = TerminalHost::new(ProviderEndpoint::scripted(), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    let created = host.run(&["repl", "--workspace", &workspace], Some("/new\n/quit\n"));
    assert_eq!(created.code, Some(0), "the REPL exits: {}", created.stderr);
    let session = block_on(host.client.list_sessions())
        .expect("the session list reads")
        .sessions()
        .first()
        .map(|summary| summary.session_id())
        .expect("the created session is listed");

    // `--continue` is an explicit request for the newest session: the prompt
    // goes to that session instead of starting another one.
    let script = format!("{PROMPT}\n/quit\n");
    let continued = host.run(
        &["repl", "--continue", "--workspace", &workspace],
        Some(&script),
    );
    assert_eq!(
        continued.code,
        Some(0),
        "the REPL exits: {}",
        continued.stderr
    );
    assert!(
        !continued.stderr.contains("error:"),
        "the continuation reports no failure: {}",
        continued.stderr
    );
    let after = block_on(host.client.list_sessions()).expect("the session list reads");
    assert_eq!(
        after.sessions().len(),
        1,
        "the continuation created no second session"
    );
    assert_eq!(after.sessions()[0].session_id(), session);
    assert!(
        host.snapshot(session)
            .messages()
            .iter()
            .any(|message| message.kind() == MessageKindDto::User),
        "the prompt reached the continued session"
    );
}

#[test]
fn a_continuation_with_no_sessions_exits_three_with_the_clients_failure() {
    // The store is empty, so no session exists to continue: the client resolves
    // the continue rule and classifies the empty list itself, and the process
    // reports that typed failure instead of a code the adapter composes.
    let host = TerminalHost::new(ProviderEndpoint::scripted(), None);
    let workspace = host.workspace_root();
    let output = host.run(
        &[
            "run",
            PROMPT,
            "--continue",
            "--format",
            "json",
            "--workspace",
            &workspace,
        ],
        None,
    );
    assert_eq!(
        output.code,
        Some(3),
        "an empty continue list is the client's typed rejection: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("panicked"),
        "the rejection is reported without a crash: {}",
        output.stderr
    );

    let records = json_records(&output.stdout);
    let error = records
        .iter()
        .find(|record| kind(record) == "error")
        .expect("the empty continue list is reported as one typed error record");
    assert_eq!(error["code"].as_str(), Some("no_sessions_to_continue"));
    assert_eq!(error["category"].as_str(), Some("not_found"));
    assert!(
        !records.iter().any(|record| kind(record) == "result"),
        "a rejected continuation reports no run result"
    );
    assert_eq!(
        host.scripted_provider().request_count(),
        0,
        "a rejected continuation reaches no provider round"
    );
}

#[test]
fn every_new_command_creates_a_fresh_session_under_one_workspace_root() {
    let host = TerminalHost::new(ProviderEndpoint::scripted(), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    // Two runs of the same line front end over one durable host: the second
    // `/new` proposes fresh identities for the same workspace root, and the
    // daemon resolves them to the root's durable binding instead of rejecting
    // the second session.
    for run in 1..=2 {
        let output = host.run(&["repl", "--workspace", &workspace], Some("/new\n/quit\n"));
        assert_eq!(
            output.code,
            Some(0),
            "the REPL exits cleanly on run {run}: {}",
            output.stderr
        );
        assert!(
            output.stdout.contains(" open\n"),
            "run {run} opens a session: {}",
            output.stdout
        );
        assert!(
            !output.stderr.contains("error:"),
            "run {run} reports no creation failure: {}",
            output.stderr
        );
    }

    let sessions = block_on(host.client.list_sessions()).expect("the session list reads");
    assert_eq!(
        sessions.sessions().len(),
        2,
        "every /new created its own session under the one workspace root"
    );
    let binding = sessions
        .sessions()
        .first()
        .map(|summary| summary.workspace_id())
        .expect("the listed sessions carry their workspace identity");
    assert!(
        sessions
            .sessions()
            .iter()
            .all(|summary| summary.workspace_id() == binding),
        "one workspace root keeps one durable workspace identity"
    );
}

#[test]
fn the_repl_exits_on_input_eof() {
    let host = TerminalHost::new(ProviderEndpoint::scripted(), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    let output = host.run(&["repl", "--workspace", &workspace], Some(""));
    assert_eq!(output.code, Some(0), "the REPL exits: {}", output.stderr);
    assert_eq!(
        output.stdout, "> ",
        "end-of-input ends the loop after its first prompt"
    );
    assert!(output.stderr.is_empty(), "stderr: {}", output.stderr);
}

#[test]
fn the_timeout_run_stops_the_stalled_round_and_exits_with_five() {
    // The deadline issues the terminal stop - the same command every stop
    // gesture uses - so the process reports the terminal status the stop
    // committed instead of a timeout of its own.
    let host = TerminalHost::new(ProviderEndpoint::stalled(), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    let started = Instant::now();
    let output = host.run(
        &[
            "run",
            PROMPT,
            "--timeout",
            "2",
            "--format",
            "json",
            "--workspace",
            &workspace,
        ],
        None,
    );
    assert_eq!(
        output.code,
        Some(5),
        "the deadline stops the run and the process reports the stop it observed: {}",
        output.stderr
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the deadline and its bounded stop grace end the wait"
    );

    let records = json_records(&output.stdout);
    let result = records.last().expect("the run reports one final result");
    assert_eq!(kind(result), "result", "the result record ends the output");
    assert_eq!(
        result["outcome"].as_str(),
        Some("interrupted"),
        "the result reports the terminal status the deadline's stop committed"
    );
    assert!(
        !records.iter().any(|record| kind(record) == "error"),
        "a stopped run reports its outcome instead of an error record"
    );
    let session_id = SessionId::parse(
        result["session_id"]
            .as_str()
            .expect("the result reports its session"),
    )
    .expect("the reported session identity is canonical");

    assert_stopped_run_is_durable(&host, session_id);
}

#[cfg(unix)]
#[test]
fn a_sigint_stops_the_live_run_and_ends_the_headless_process_with_five() {
    // SIGINT must not kill a front end that observes a run: the run would keep
    // working in the daemon with nobody left watching it. The signal stops the
    // run, the process reports the interrupted outcome it observed, and the
    // stopped run is durable.
    let host = TerminalHost::new(ProviderEndpoint::stalled(), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    let mut child = host.spawn(&["run", PROMPT, "--workspace", &workspace]);
    let session = wait_for_user_turn(&host);

    send_sigint(&child);
    let output = collect(&mut child, Instant::now() + CLI_DEADLINE);

    assert_eq!(
        output.code,
        Some(5),
        "the stopped run ends the process with the interrupted status: {}",
        output.stderr
    );
    assert!(
        output.stdout.contains("run interrupted"),
        "the front end reports the outcome it observed: {}",
        output.stdout
    );
    assert_stopped_run_is_durable(&host, session);
}

#[cfg(unix)]
#[test]
fn a_sigint_stops_the_live_run_and_returns_the_repl_to_its_prompt() {
    let host = TerminalHost::new(ProviderEndpoint::stalled(), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    let mut child = host.spawn(&["repl", "--workspace", &workspace]);
    let mut stdin = child
        .stdin
        .take()
        .expect("the piped stdin handle is present");
    stdin
        .write_all(format!("{PROMPT}\n").as_bytes())
        .expect("the prompt writes");
    stdin.flush().expect("the prompt reaches the front end");
    let session = wait_for_user_turn(&host);

    send_sigint(&child);
    // The stopped run hands the loop back to its prompt, which reads this line.
    stdin
        .write_all(b"/quit\n")
        .expect("the quit command writes");
    stdin.flush().expect("the command reaches the front end");
    drop(stdin);
    let output = collect(&mut child, Instant::now() + CLI_DEADLINE);

    assert_eq!(
        output.code,
        Some(0),
        "the loop leaves on its own command: {}",
        output.stderr
    );
    assert!(
        output.stdout.contains("run interrupted"),
        "the streamed run reports the stop it observed: {}",
        output.stdout
    );
    assert_eq!(
        output.stdout.matches("> ").count(),
        2,
        "the prompt comes back after the stopped run: {}",
        output.stdout
    );
    assert_stopped_run_is_durable(&host, session);
}

#[test]
fn an_unknown_session_is_rejected_with_a_typed_record_and_exit_three() {
    let host = TerminalHost::new(ProviderEndpoint::scripted(), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    let unknown = SessionId::new().to_string();
    let output = host.run(
        &[
            "run",
            PROMPT,
            "--session",
            &unknown,
            "--format",
            "json",
            "--workspace",
            &workspace,
        ],
        None,
    );
    assert_eq!(
        output.code,
        Some(3),
        "an unknown session is a typed rejection: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("panicked"),
        "the rejection is reported without a crash: {}",
        output.stderr
    );

    let records = json_records(&output.stdout);
    let error = records
        .iter()
        .find(|record| kind(record) == "error")
        .expect("the rejection is reported as one typed error record");
    assert_eq!(error["code"].as_str(), Some("storage_record_not_found"));
    assert_eq!(error["category"].as_str(), Some("not_found"));
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| !message.is_empty()),
        "the error record carries its safe message"
    );
    assert!(
        !records.iter().any(|record| kind(record) == "result"),
        "a rejected session selection reports no run result"
    );
    assert_eq!(
        host.scripted_provider().request_count(),
        0,
        "a rejected session selection reaches no provider round"
    );
}

#[cfg(unix)]
#[test]
fn a_stream_that_closes_before_a_terminal_status_reports_the_clients_failure() {
    // The daemon dies under a live run and cannot come back, because the
    // endpoint path is occupied by a directory. The stream is gone, so the front
    // end's one reconnect attempt fails and the run is unobservable: the process
    // reports the client's own classification of the condition instead of a
    // protocol error the daemon never sent.
    let mut host = TerminalHost::new(ProviderEndpoint::stalled(), Some(WORKSPACE_FILE));
    let workspace = host.workspace_root();
    let mut child = host.spawn(&["run", PROMPT, "--format", "json", "--workspace", &workspace]);
    let _session = wait_for_user_turn(&host);
    host.occupy_endpoint();
    daemon_process::kill(&mut host.daemon);

    let output = collect(&mut child, Instant::now() + CLI_DEADLINE);
    assert_eq!(
        output.code,
        Some(2),
        "a lost stream with no recovery is the client's unavailable failure: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("panicked"),
        "the loss is reported without a crash: {}",
        output.stderr
    );

    let records = json_records(&output.stdout);
    let error = records
        .iter()
        .find(|record| kind(record) == "error")
        .expect("the unobservable run is reported as one typed error record");
    assert_eq!(error["category"].as_str(), Some("unavailable"));
    // The reported failure is the client's own classification of the endpoint it
    // cannot reach or relaunch; which of its two transport spellings that is
    // depends on whether the program can be spawned at all, so the case pins the
    // vocabulary rather than one spelling. The adapter composes no code of its
    // own: the removed `session_stream_closed` spelling appears nowhere.
    assert!(
        error["code"]
            .as_str()
            .is_some_and(|code| code.starts_with("local_daemon_")),
        "the record carries the client's transport classification: {}",
        error["code"]
    );
    assert!(
        !output.stdout.contains("session_stream_closed"),
        "the adapter composes no error of its own: {}",
        output.stdout
    );
    assert!(
        !records.iter().any(|record| kind(record) == "result"),
        "a run that never reported a terminal status reports no result"
    );
}
