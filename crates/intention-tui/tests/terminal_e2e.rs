//! Terminal-client process end-to-end tests over real IPC and a real daemon.
//!
//! Every scenario spawns the real `intention-tui` binary with piped standard
//! streams and drives it against a real daemon process, whose provider endpoint
//! is either the scripted fake provider or a listener that never answers. The
//! daemon is spawned by the fixture, so the binary reaches it through the
//! `INTENTION_ENDPOINT` process seam and never needs the sibling daemon program
//! or a terminal. The assertions cover the observable process contract: the
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
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use intention_client::IntentionClient;
use intention_proto::{MessageKindDto, RunStatusDto, SessionId, SessionSnapshotDto};
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
const OBSERVE_DEADLINE: Duration = Duration::from_secs(15);

/// The poll interval of the bounded process and durable-effect waits.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

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
        let daemon = spawn_daemon(&endpoint, config_home.path(), state_home.path(), None, &[]);
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
            endpoint,
            client,
        }
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
        command.env("INTENTION_ENDPOINT", self.endpoint.instance_id());
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
        let status = wait_for_exit(&mut child, Instant::now() + CLI_DEADLINE);
        let stdout = stdout_reader.join().expect("the stdout reader finishes");
        let stderr = stderr_reader.join().expect("the stderr reader finishes");
        CliOutput {
            code: status.code(),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        }
    }

    /// Reads the current durable snapshot of one session through the client.
    fn snapshot(&self, session_id: SessionId) -> SessionSnapshotDto {
        block_on(self.client.session_snapshot(session_id)).expect("the session snapshot reads")
    }

    /// Waits for the interrupt notice one interrupted call commits.
    ///
    /// The interrupt itself is a registered run signal the daemon answers with
    /// a durable notice row, so the notice is the observable effect a test can
    /// reach; the stalled round after it cannot produce a final result.
    fn wait_for_interrupt_notice(&self, session_id: SessionId) -> SessionSnapshotDto {
        let deadline = Instant::now() + OBSERVE_DEADLINE;
        loop {
            let snapshot = self.snapshot(session_id);
            if snapshot
                .messages()
                .iter()
                .any(|message| message.kind() == MessageKindDto::Notice)
            {
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "the interrupt commits its durable notice before the deadline"
            );
            thread::sleep(POLL_INTERVAL);
        }
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
    assert_eq!(output.code, Some(0), "the run completes: {}", output.stderr);
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
fn the_timeout_run_interrupts_the_stalled_round_and_exits_with_four() {
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
        Some(4),
        "the deadline ends the process with the timeout status: {}",
        output.stderr
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the deadline and its bounded interrupt grace end the wait"
    );

    let records = json_records(&output.stdout);
    let result = records.last().expect("the run reports one final result");
    assert_eq!(kind(result), "result", "the result record ends the output");
    assert_eq!(result["outcome"].as_str(), Some("timed_out"));
    assert!(
        !records.iter().any(|record| kind(record) == "error"),
        "a timed-out run reports its outcome instead of an error record"
    );
    let session_id = SessionId::parse(
        result["session_id"]
            .as_str()
            .expect("the result reports its session"),
    )
    .expect("the reported session identity is canonical");

    let snapshot = host.wait_for_interrupt_notice(session_id);
    let notice = snapshot
        .messages()
        .iter()
        .find(|message| message.kind() == MessageKindDto::Notice)
        .expect("the interrupted call commits its durable notice");
    assert_eq!(
        notice.text(),
        "[The call was stopped before a final result.]",
        "the durable notice is the engine's interruption notice"
    );
    let active = snapshot
        .projection()
        .active_run()
        .expect("the stalled run is still the session's active run");
    assert_ne!(
        active.status(),
        RunStatusDto::Completed,
        "a stalled provider round cannot complete the run"
    );
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
