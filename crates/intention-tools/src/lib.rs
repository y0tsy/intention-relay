//! Typed, bounded contracts for workspace tools.

use intention_types::{DtoResult, RunId, SessionId, ToolCallId, WorkspaceRelativePathDto};
use intention_workspace::WorkspaceRoot;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

mod execute;
mod file;
mod search;

#[cfg(test)]
#[allow(clippy::expect_used, reason = "deterministic timeout fixture setup")]
mod timeout_tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn timeout_is_classified_as_unknown_effect_without_waiting_thirty_seconds() {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("ping");
            command.args(["-n", "3", "127.0.0.1"]);
            command
        } else {
            let mut command = Command::new("sleep");
            command.arg("1");
            command
        };
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = command.spawn().expect("spawn timeout fixture");
        let started = Instant::now();
        let result = bounded_output_with_timeout(
            child,
            CancellationSignal::new(),
            Duration::from_millis(25),
        );
        assert!(matches!(
            result,
            Err("tool_execute_external_effect_unknown")
        ));
        // The deadline bound is the controlling invariant: the tool must
        // return promptly (far below the thirty-second execute default) even
        // when the pipe readers are descheduled by a loaded machine; the
        // assertion is deliberately looser than the two-second drain grace.
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[cfg(unix)]
    #[test]
    fn background_descendant_holding_pipes_cannot_exceed_the_deadline() {
        use std::os::unix::process::CommandExt;
        let marker = std::env::temp_dir().join(format!(
            "intention-relay-exec-descendant-{}.pid",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&marker);
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(format!(
                "sleep 300 & echo $! > '{}'",
                marker.to_string_lossy()
            ))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command.process_group(0);
        let child = command.spawn().expect("spawn descendant fixture");
        let started = Instant::now();
        let result = bounded_output_with_timeout(
            child,
            CancellationSignal::new(),
            Duration::from_millis(50),
        );
        assert!(matches!(
            result,
            Err("tool_execute_external_effect_unknown")
        ));
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "a descendant holding the pipes open must not extend the deadline"
        );
        // The process-group termination must also kill the background
        // descendant, not only the direct child.
        let pid_text = std::fs::read_to_string(&marker).unwrap_or_default();
        let _ = std::fs::remove_file(&marker);
        if let Ok(descendant) = pid_text.trim().parse::<i32>()
            && let Some(pid) = rustix::process::Pid::from_raw(descendant)
        {
            let mut alive = true;
            for _ in 0..40 {
                alive = rustix::process::test_kill_process(pid).is_ok();
                if !alive {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
            assert!(
                !alive,
                "the background descendant must be terminated with its process group"
            );
        }
    }
}

#[cfg(test)]
mod spawn_observation_tests {
    use super::*;

    #[test]
    fn spawn_observation_is_shared_across_clones_and_wait_returns_on_observe() {
        let signal = CancellationSignal::new();
        let observer = signal.clone();
        std::thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            observer.observe_spawn();
        });
        assert!(
            signal.wait_until_spawn_observed(Duration::from_secs(5)),
            "spawn observation must be visible once the executor records it"
        );
        assert!(
            signal.wait_until_spawn_observed(Duration::from_millis(1)),
            "an already-observed spawn must return immediately"
        );
    }

    #[test]
    fn spawn_observation_wait_times_out_when_no_spawn_is_recorded() {
        let signal = CancellationSignal::new();
        assert!(
            !signal.wait_until_spawn_observed(Duration::from_millis(10)),
            "the wait must time out when no spawn is ever recorded"
        );
        assert!(!signal.is_cancelled(), "observation must not cancel");
    }
}

const MAX_TOOL_OUTPUT_BYTES: usize = 64 * 1024;
const EXECUTE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_GLOB_MATCHES: usize = 10_000;
const MAX_GREP_MATCHES: usize = 10_000;
/// Upper bound on distinct files scanned by one directory/workspace grep.
const MAX_GREP_FILES: usize = 10_000;
/// Upper bound on the retained fragment bytes of one grep result. Per-line
/// fragment caps alone still allow a very large aggregate result; the
/// aggregate is clamped with the same truncation flag so durable, normalized
/// content stays bounded (PR24-022).
const MAX_GREP_AGGREGATE_BYTES: usize = 128 * 1024;
/// Upper bound on one edit target or write expected-content source file.
/// Larger files can be read (truncated) but never edited or equality-checked,
/// which keeps edit reads and write preflights bounded (PR24-022).
const MAX_EDIT_TARGET_BYTES: usize = 1024 * 1024;
/// Upper bound on the number of arguments of one execute invocation.
const MAX_EXECUTE_ARGUMENTS: usize = 128;
/// Upper bound on the aggregate argument bytes of one execute invocation.
const MAX_EXECUTE_ARGUMENTS_TOTAL_BYTES: usize = 256 * 1024;

/// Redacted working-directory identity recorded in durable tool metadata. It
/// marks the authorized workspace root as the effective CWD without disclosing
/// its absolute location.
pub const REDACTED_WORKSPACE_CWD: &str = "workspace_root";

/// Typed cancellation signal for one tool invocation.
///
/// Besides cancellation state, the signal records whether the process executor
/// has spawned the invocation's child, so deterministic fixtures can wait for
/// a confirmed spawn instead of blind sleeps before requesting cancellation.
#[derive(Clone, Debug, Default)]
pub struct CancellationSignal {
    cancelled: Arc<AtomicBool>,
    spawn_observed: Arc<AtomicBool>,
}

impl CancellationSignal {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            spawn_observed: Arc::new(AtomicBool::new(false)),
        }
    }
    #[must_use]
    pub fn cancelled() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(true)),
            spawn_observed: Arc::new(AtomicBool::new(false)),
        }
    }
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    /// Requests cancellation of this invocation.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    /// Records that the process executor spawned the invocation's child.
    ///
    /// Internal synchronization point for process-lifecycle fixtures; the
    /// flag is never read by production behavior.
    pub(crate) fn observe_spawn(&self) {
        self.spawn_observed.store(true, Ordering::Release);
    }
    /// Waits until the invocation's child has been spawned, or until `timeout`
    /// elapses.
    ///
    /// Returns whether the spawn was observed. This is a test-only
    /// synchronization point for deterministic cancellation fixtures: waiting
    /// on a confirmed spawn replaces blind sleeps while keeping the
    /// platform-independent unknown external-effect classification intact. It
    /// is hidden from the public documentation because no production caller
    /// should depend on it.
    #[doc(hidden)]
    #[must_use]
    pub fn wait_until_spawn_observed(&self, timeout: Duration) -> bool {
        let Some(deadline) = Instant::now().checked_add(timeout) else {
            return false;
        };
        while !self.spawn_observed.load(Ordering::Acquire) {
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(1));
        }
        true
    }
}

pub const TOOL_SCHEMA_VERSION: u16 = 1;
pub const TOOL_DESCRIPTOR_REVISION: u16 = 1;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationKind {
    ReadOnly,
    Mutating,
    Process,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCapability {
    Read,
    Search,
    Write,
    Edit,
    Execute,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOutcome {
    Succeeded,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPolicy {
    Allowed,
    Denied,
}

/// Typed terminal classification for one executed program.
///
/// Per the external-attempt taxonomy, a known non-zero exit or known signal
/// termination is a normalized program result carried on the typed output, not
/// a transport-level tool failure. Only lost terminal evidence (cancellation,
/// timeout) classifies as an unknown-effect error.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolProcessStatus {
    Success,
    NonZero { code: i32 },
    Signal { signal: i32 },
}

impl ToolProcessStatus {
    /// Classifies a finished child-process status.
    #[must_use]
    pub fn classify(status: std::process::ExitStatus) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(signal) = status.signal() {
                return Self::Signal { signal };
            }
        }
        match status.code() {
            Some(0) => Self::Success,
            Some(code) => Self::NonZero { code },
            // A reaped child always reports either a recorded signal (checked
            // above on Unix) or a numeric exit code (Windows always reports
            // one); a status with neither cannot occur.
            None => unreachable!("child status has neither signal nor exit code"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRegistrationStatus {
    Active,
    Reserved,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolObservability {
    pub outcome: ToolOutcome,
    pub policy: ToolPolicy,
    pub elapsed_ms: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolContext {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub call_id: ToolCallId,
}

/// Redacted execution metadata for the durable result boundary.
///
/// `cwd` is the stable [`REDACTED_WORKSPACE_CWD`] identity marker and never an
/// absolute path; `path` is the logical workspace-relative target of the
/// invocation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolExecutionMetadata {
    pub cwd: String,
    #[serde(default)]
    pub path: Option<WorkspaceRelativePathDto>,
    pub policy: ToolPolicy,
    pub elapsed_ms: u64,
    /// Typed terminal program status; populated only by Execute invocations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_status: Option<ToolProcessStatus>,
}

impl ToolExecutionMetadata {
    /// Builds redacted metadata for one workspace execution.
    ///
    /// The working-directory identity is the stable [`REDACTED_WORKSPACE_CWD`]
    /// marker: durable records identify the authorized workspace root without
    /// ever storing its absolute location.
    #[must_use]
    pub fn for_workspace(policy: ToolPolicy, elapsed_ms: u64) -> Self {
        Self {
            cwd: REDACTED_WORKSPACE_CWD.to_owned(),
            path: None,
            policy,
            elapsed_ms,
            process_status: None,
        }
    }
    /// Attaches the logical workspace-relative path targeted by the invocation.
    #[must_use]
    pub fn with_path(mut self, path: Option<WorkspaceRelativePathDto>) -> Self {
        self.path = path;
        self
    }
    /// Attaches the typed terminal program status to this metadata.
    #[must_use]
    pub const fn with_process_status(mut self, process_status: Option<ToolProcessStatus>) -> Self {
        self.process_status = process_status;
        self
    }
}

fn bounded_lossy(bytes: &[u8]) -> (String, bool) {
    if bytes.len() <= MAX_TOOL_OUTPUT_BYTES {
        return (String::from_utf8_lossy(bytes).into_owned(), false);
    }
    let mut end = MAX_TOOL_OUTPUT_BYTES;
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }
    (
        format!("{}\n[truncated]", String::from_utf8_lossy(&bytes[..end])),
        true,
    )
}

fn bounded_text(value: String) -> DtoResult<BoundedText> {
    BoundedText::new(value)
}

#[derive(Debug)]
struct BoundedOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

fn bounded_output(
    child: Child,
    cancellation: CancellationSignal,
) -> Result<BoundedOutput, &'static str> {
    bounded_output_with_timeout(child, cancellation, EXECUTE_TIMEOUT)
}

/// How long pipe readers may keep draining after the direct child has been
/// reaped or terminated. Descendants holding the pipes open are killed with
/// the child's process group on Unix; on every platform the collection is
/// deadline-bounded so Execute can never wait forever (PR24-011). The grace
/// tolerates reader-thread descheduling on loaded, instrumented machines
/// while staying far below the thirty-second execute deadline.
const READER_DRAIN_GRACE: Duration = Duration::from_secs(5);

fn bounded_output_with_timeout(
    mut child: Child,
    cancellation: CancellationSignal,
    timeout: Duration,
) -> Result<BoundedOutput, &'static str> {
    let child_id = child.id();
    let stdout = child.stdout.take().map(|pipe| {
        thread::spawn(move || {
            let mut output = Vec::new();
            read_bounded(&mut std::io::BufReader::new(pipe), &mut output)
                .map(|truncated| (output, truncated))
        })
    });
    let stderr = child.stderr.take().map(|pipe| {
        thread::spawn(move || {
            let mut output = Vec::new();
            read_bounded(&mut std::io::BufReader::new(pipe), &mut output)
                .map(|truncated| (output, truncated))
        })
    });
    let deadline = Instant::now() + timeout;
    loop {
        if cancellation.is_cancelled() || Instant::now() >= deadline {
            // The direct child is killed and, on Unix, its whole process
            // group, so a backgrounded descendant that inherited the pipes
            // cannot keep the reader threads alive after the tool returns.
            terminate_process_tree(&mut child, child_id);
            let _ = child.wait();
            let _ = drain_pipes(
                stdout,
                stderr,
                Instant::now() + READER_DRAIN_GRACE,
                &cancellation,
            );
            return Err("tool_execute_external_effect_unknown");
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                // The direct child is reaped, but a descendant may still hold
                // the output pipes open. Collect until the grace bound; a
                // stalled drain is killed and classified unknown so Execute
                // still returns within its deadline.
                let drain_until = deadline.min(Instant::now() + READER_DRAIN_GRACE);
                match drain_pipes(stdout, stderr, drain_until, &cancellation) {
                    PipeDrain::Complete { stdout, stderr } => {
                        let (stdout, stdout_was_truncated) = stdout?;
                        let (stderr, stderr_was_truncated) = stderr?;
                        return Ok(BoundedOutput {
                            status,
                            stdout,
                            stderr,
                            stdout_truncated: stdout_was_truncated,
                            stderr_truncated: stderr_was_truncated,
                        });
                    }
                    PipeDrain::Stalled => {
                        terminate_process_tree(&mut child, child_id);
                        let _ = child.wait();
                        return Err("tool_execute_external_effect_unknown");
                    }
                }
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                terminate_process_tree(&mut child, child_id);
                let _ = child.wait();
                let _ = drain_pipes(
                    stdout,
                    stderr,
                    Instant::now() + READER_DRAIN_GRACE,
                    &cancellation,
                );
                return Err("tool_execute_external_effect_unknown");
            }
        }
    }
}

/// Kills the direct child and, on Unix, its whole process group.
///
/// The execute path spawns the child as the leader of its own process group,
/// so this also terminates background descendants that inherited the output
/// pipes. A missing group (ESRCH) is harmless: the direct kill covers the
/// child. The kill is performed through the safe `rustix` process API, never
/// through raw `unsafe` syscall blocks, so the workspace `unsafe_code` deny
/// stays intact (PR24-011).
fn terminate_process_tree(child: &mut Child, child_id: u32) {
    let _ = child.kill();
    #[cfg(unix)]
    if let Some(pid) = rustix::process::Pid::from_raw(i32::try_from(child_id).unwrap_or(0)) {
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    }
}

/// Collected reader results once both pipe readers have finished.
enum PipeDrain {
    /// Both readers completed; an `Err` records a reader failure.
    Complete {
        stdout: Result<(Vec<u8>, bool), &'static str>,
        stderr: Result<(Vec<u8>, bool), &'static str>,
    },
    /// At least one reader was still pending at the deadline.
    Stalled,
}

/// Joins both pipe readers until `until`, observing cancellation.
fn drain_pipes(
    mut stdout: Option<ReaderHandle>,
    mut stderr: Option<ReaderHandle>,
    until: Instant,
    cancellation: &CancellationSignal,
) -> PipeDrain {
    let mut stdout_result = None;
    let mut stderr_result = None;
    loop {
        if let Some(handle) = stdout.take() {
            if handle.is_finished() {
                stdout_result = Some(join_reader(Some(handle)));
            } else {
                stdout = Some(handle);
            }
        }
        if let Some(handle) = stderr.take() {
            if handle.is_finished() {
                stderr_result = Some(join_reader(Some(handle)));
            } else {
                stderr = Some(handle);
            }
        }
        if let (Some(stdout), Some(stderr)) = (stdout_result.take(), stderr_result.take()) {
            return PipeDrain::Complete { stdout, stderr };
        }
        if cancellation.is_cancelled() || Instant::now() >= until {
            return PipeDrain::Stalled;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod process_failure_tests {
    use super::*;
    use std::io::{self, Read};

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("injected read failure"))
        }
    }

    #[test]
    fn read_failure_is_classified_as_read_failure() {
        let mut output = Vec::new();
        assert_eq!(
            read_bounded(&mut FailingReader, &mut output),
            Err("tool_execute_read_failed")
        );
    }

    #[test]
    fn reader_join_failure_is_classified_as_read_failure() {
        let reader = thread::spawn(|| -> Result<(Vec<u8>, bool), &'static str> {
            Err("tool_execute_read_failed")
        });
        assert_eq!(join_reader(Some(reader)), Err("tool_execute_read_failed"));
    }
}

type ReaderHandle = thread::JoinHandle<Result<(Vec<u8>, bool), &'static str>>;

fn join_reader(reader: Option<ReaderHandle>) -> Result<(Vec<u8>, bool), &'static str> {
    reader
        .map(|reader| reader.join().map_err(|_| "tool_execute_read_failed")?)
        .transpose()
        .map(|value| value.unwrap_or_default())
}

/// Outcome of a bounded whole-file read.
enum LimitedReadOutcome {
    /// The file fit within the bound and was read completely.
    Content(Vec<u8>),
    /// The file exceeds the bound; no content is retained.
    TooLarge,
    /// The file could not be opened or read.
    Unreadable,
}

/// Reads a whole file only when it fits within `limit` bytes.
///
/// Oversized files are detected without allocating or slurping their content,
/// which keeps edit and expected-content comparisons bounded (PR24-022).
fn read_limited(path: &std::path::Path, limit: usize) -> LimitedReadOutcome {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return LimitedReadOutcome::Unreadable,
    };
    let mut bytes = Vec::new();
    if file.take(limit as u64 + 1).read_to_end(&mut bytes).is_err() {
        return LimitedReadOutcome::Unreadable;
    }
    if bytes.len() > limit {
        return LimitedReadOutcome::TooLarge;
    }
    LimitedReadOutcome::Content(bytes)
}

fn read_bounded(
    reader: &mut impl std::io::Read,
    output: &mut Vec<u8>,
) -> Result<bool, &'static str> {
    let mut buffer = [0_u8; 4096];
    let mut total = 0;
    // Truncation means bytes were actually dropped: a source ending exactly at
    // the bound stays untruncated.
    let mut truncated = false;
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| "tool_execute_read_failed")?;
        if count == 0 {
            return Ok(truncated);
        }
        if truncated {
            // Drain the remainder to EOF so oversized sources still finish
            // cleanly instead of stalling writers mid-stream.
            continue;
        }
        let remaining = MAX_TOOL_OUTPUT_BYTES.saturating_sub(total);
        let kept = count.min(remaining);
        output.extend_from_slice(&buffer[..kept]);
        total += kept;
        truncated = count > kept;
    }
}

/// The fixed set of tools exposed by the product boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolId {
    Read,
    Write,
    Edit,
    Execute,
    Glob,
    Grep,
    FetchUrl,
    AskUser,
    Todo,
    Retrieve,
    PlanSubmit,
    SubAgent,
    Expand,
    Mcp,
}

impl ToolId {
    /// Returns the stable wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Glob => "glob",
            Self::Grep => "grep",
            Self::FetchUrl => "fetch_url",
            Self::AskUser => "ask_user",
            Self::Todo => "todo",
            Self::Retrieve => "retrieve",
            Self::PlanSubmit => "plan_submit",
            Self::SubAgent => "sub_agent",
            Self::Expand => "expand",
            Self::Mcp => "mcp",
            Self::Write => "write",
            Self::Edit => "edit",
            Self::Execute => "execute",
        }
    }
}
impl Display for ToolId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Metadata describing one registered tool.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ToolDescriptor {
    id: ToolId,
    display_name: &'static str,
    description: &'static str,
    input_schema: Option<&'static str>,
    output_schema: Option<&'static str>,
    descriptor_revision: u16,
    schema_version: u16,
    mutation: MutationKind,
    capabilities: &'static [ToolCapability],
    observability_policy: ToolPolicy,
    status: ToolRegistrationStatus,
}
impl ToolDescriptor {
    #[must_use]
    pub const fn id(self) -> ToolId {
        self.id
    }
    #[must_use]
    pub const fn description(self) -> &'static str {
        self.description
    }
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        self.display_name
    }
    #[must_use]
    pub const fn input_schema(self) -> Option<&'static str> {
        self.input_schema
    }
    #[must_use]
    pub const fn output_schema(self) -> Option<&'static str> {
        self.output_schema
    }
    #[must_use]
    pub const fn descriptor_revision(self) -> u16 {
        self.descriptor_revision
    }
    #[must_use]
    pub const fn schema_version(self) -> u16 {
        self.schema_version
    }
    #[must_use]
    pub const fn mutation(self) -> MutationKind {
        self.mutation
    }
    #[must_use]
    pub const fn capabilities(self) -> &'static [ToolCapability] {
        self.capabilities
    }
    #[must_use]
    pub const fn observability_policy(self) -> ToolPolicy {
        self.observability_policy
    }
    #[must_use]
    pub const fn status(self) -> ToolRegistrationStatus {
        self.status
    }
}

/// The immutable built-in registry.
#[must_use]
pub const fn registry() -> [ToolDescriptor; 14] {
    [
        ToolDescriptor {
            id: ToolId::Read,
            display_name: "Read",
            input_schema: Some("ReadInput"),
            output_schema: Some("TextResult"),
            descriptor_revision: TOOL_DESCRIPTOR_REVISION,
            description: "Read bounded text from a workspace file.",
            schema_version: TOOL_SCHEMA_VERSION,
            mutation: MutationKind::ReadOnly,
            capabilities: &[ToolCapability::Read],
            observability_policy: ToolPolicy::Allowed,
            status: ToolRegistrationStatus::Active,
        },
        ToolDescriptor {
            id: ToolId::Write,
            display_name: "Write",
            input_schema: Some("WriteInput"),
            output_schema: Some("WriteResult"),
            descriptor_revision: TOOL_DESCRIPTOR_REVISION,
            description: "Write bounded text to a workspace file.",
            schema_version: TOOL_SCHEMA_VERSION,
            mutation: MutationKind::Mutating,
            capabilities: &[ToolCapability::Write],
            observability_policy: ToolPolicy::Allowed,
            status: ToolRegistrationStatus::Active,
        },
        ToolDescriptor {
            id: ToolId::Edit,
            display_name: "Edit",
            input_schema: Some("EditInput"),
            output_schema: Some("WriteResult"),
            descriptor_revision: TOOL_DESCRIPTOR_REVISION,
            description: "Apply a bounded text replacement.",
            schema_version: TOOL_SCHEMA_VERSION,
            mutation: MutationKind::Mutating,
            capabilities: &[ToolCapability::Edit],
            observability_policy: ToolPolicy::Allowed,
            status: ToolRegistrationStatus::Active,
        },
        ToolDescriptor {
            id: ToolId::Execute,
            display_name: "Execute",
            input_schema: Some("ExecuteInput"),
            output_schema: Some("TextResult"),
            descriptor_revision: TOOL_DESCRIPTOR_REVISION,
            description: "Execute an explicitly bounded command.",
            schema_version: TOOL_SCHEMA_VERSION,
            mutation: MutationKind::Process,
            capabilities: &[ToolCapability::Execute],
            observability_policy: ToolPolicy::Allowed,
            status: ToolRegistrationStatus::Active,
        },
        ToolDescriptor {
            id: ToolId::Glob,
            display_name: "Glob",
            input_schema: Some("GlobInput"),
            output_schema: Some("PathsResult"),
            descriptor_revision: TOOL_DESCRIPTOR_REVISION,
            description: "List workspace paths matching a pattern.",
            schema_version: TOOL_SCHEMA_VERSION,
            mutation: MutationKind::ReadOnly,
            capabilities: &[ToolCapability::Search],
            observability_policy: ToolPolicy::Allowed,
            status: ToolRegistrationStatus::Active,
        },
        ToolDescriptor {
            id: ToolId::Grep,
            display_name: "Grep",
            input_schema: Some("GrepInput"),
            output_schema: Some("GrepResult"),
            descriptor_revision: TOOL_DESCRIPTOR_REVISION,
            description: "Search bounded workspace text.",
            schema_version: TOOL_SCHEMA_VERSION,
            mutation: MutationKind::ReadOnly,
            capabilities: &[ToolCapability::Search],
            observability_policy: ToolPolicy::Allowed,
            status: ToolRegistrationStatus::Active,
        },
        ToolDescriptor {
            id: ToolId::FetchUrl,
            display_name: "Fetch URL",
            input_schema: None,
            output_schema: None,
            descriptor_revision: 0,
            description: "Reserved tool slot.",
            schema_version: 0,
            mutation: MutationKind::ReadOnly,
            capabilities: &[],
            observability_policy: ToolPolicy::Allowed,
            status: ToolRegistrationStatus::Reserved,
        },
        ToolDescriptor {
            id: ToolId::AskUser,
            display_name: "Ask User",
            input_schema: None,
            output_schema: None,
            descriptor_revision: 0,
            description: "Reserved tool slot.",
            schema_version: 0,
            mutation: MutationKind::ReadOnly,
            capabilities: &[],
            observability_policy: ToolPolicy::Allowed,
            status: ToolRegistrationStatus::Reserved,
        },
        ToolDescriptor {
            id: ToolId::Todo,
            display_name: "Todo",
            input_schema: None,
            output_schema: None,
            descriptor_revision: 0,
            description: "Reserved tool slot.",
            schema_version: 0,
            mutation: MutationKind::ReadOnly,
            capabilities: &[],
            observability_policy: ToolPolicy::Allowed,
            status: ToolRegistrationStatus::Reserved,
        },
        ToolDescriptor {
            id: ToolId::Retrieve,
            display_name: "Retrieve",
            input_schema: None,
            output_schema: None,
            descriptor_revision: 0,
            description: "Reserved tool slot.",
            schema_version: 0,
            mutation: MutationKind::ReadOnly,
            capabilities: &[],
            observability_policy: ToolPolicy::Allowed,
            status: ToolRegistrationStatus::Reserved,
        },
        ToolDescriptor {
            id: ToolId::PlanSubmit,
            display_name: "Plan Submit",
            input_schema: None,
            output_schema: None,
            descriptor_revision: 0,
            description: "Reserved tool slot.",
            schema_version: 0,
            mutation: MutationKind::ReadOnly,
            capabilities: &[],
            observability_policy: ToolPolicy::Allowed,
            status: ToolRegistrationStatus::Reserved,
        },
        ToolDescriptor {
            id: ToolId::SubAgent,
            display_name: "Sub-Agent",
            input_schema: None,
            output_schema: None,
            descriptor_revision: 0,
            description: "Reserved tool slot.",
            schema_version: 0,
            mutation: MutationKind::ReadOnly,
            capabilities: &[],
            observability_policy: ToolPolicy::Allowed,
            status: ToolRegistrationStatus::Reserved,
        },
        ToolDescriptor {
            id: ToolId::Expand,
            display_name: "Expand",
            input_schema: None,
            output_schema: None,
            descriptor_revision: 0,
            description: "Reserved tool slot.",
            schema_version: 0,
            mutation: MutationKind::ReadOnly,
            capabilities: &[],
            observability_policy: ToolPolicy::Allowed,
            status: ToolRegistrationStatus::Reserved,
        },
        ToolDescriptor {
            id: ToolId::Mcp,
            display_name: "MCP",
            input_schema: None,
            output_schema: None,
            descriptor_revision: 0,
            description: "Reserved tool slot.",
            schema_version: 0,
            mutation: MutationKind::ReadOnly,
            capabilities: &[],
            observability_policy: ToolPolicy::Allowed,
            status: ToolRegistrationStatus::Reserved,
        },
    ]
}

/// Bounded text accepted by tool contracts.
///
/// Deserialization is validating: JSON arriving at the daemon tool boundary
/// cannot bypass the constructor's size and NUL checks (PR24-023).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct BoundedText(String);
impl BoundedText {
    /// Maximum contract text size.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the text exceeds the bound or contains a NUL byte.
    pub fn new(value: impl Into<String>) -> DtoResult<Self> {
        let value = value.into();
        if value.len() > 1_048_576 || value.contains('\0') {
            Err(intention_types::ErrorDto::validation(
                "invalid_tool_text",
                "tool text exceeds bounds or contains NUL",
            ))
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for BoundedText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(|error| {
            serde::de::Error::custom(format!("invalid tool text ({})", error.code()))
        })
    }
}

/// Typed tool input family.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tool", content = "input", rename_all = "snake_case")]
pub enum ToolInput {
    Read(ReadInput),
    Glob(GlobInput),
    Grep(GrepInput),
    Write(WriteInput),
    Edit(EditInput),
    Execute(ExecuteInput),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub schema_version: u16,
    pub context: ToolContext,
    pub input: ToolInput,
}

impl ToolInvocation {
    /// Constructs an invocation with an explicit expected call identity.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the call identity does not match.
    pub fn new(
        schema_version: u16,
        context: ToolContext,
        input: ToolInput,
        expected_call: ToolCallId,
    ) -> DtoResult<Self> {
        let invocation = Self {
            schema_version,
            context,
            input,
        };
        invocation.validate_call_id(expected_call)?;
        Ok(invocation)
    }

    /// Validates the invocation schema against the active tool contract.
    /// # Errors
    ///
    /// Returns a validation error when the invocation schema version differs
    /// from the active tool schema.
    pub fn validate_schema_version(&self) -> DtoResult<()> {
        if self.schema_version == TOOL_SCHEMA_VERSION {
            Ok(())
        } else {
            Err(intention_types::ErrorDto::validation(
                "tool_schema_mismatch",
                "tool invocation schema version does not match the active schema",
            ))
        }
    }
}

impl ToolInvocation {
    /// Validates the invocation against the expected call identity.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the call identity does not match.
    pub fn validate_call_id(&self, expected: ToolCallId) -> DtoResult<()> {
        if self.context.call_id == expected {
            Ok(())
        } else {
            Err(intention_types::ErrorDto::validation(
                "tool_call_id_mismatch",
                "tool call identity does not match invocation context",
            ))
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReadInput {
    pub path: WorkspaceRelativePathDto,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GlobInput {
    pub pattern: BoundedText,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GrepInput {
    pub pattern: BoundedText,
    #[serde(default)]
    pub scope: Option<GrepScope>,
    #[serde(default)]
    pub path: Option<WorkspaceRelativePathDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GrepScope {
    File { path: WorkspaceRelativePathDto },
    Directory { path: WorkspaceRelativePathDto },
    Workspace,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WriteInput {
    pub path: WorkspaceRelativePathDto,
    pub content: BoundedText,
    #[serde(default)]
    pub expected_content: Option<BoundedText>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EditInput {
    pub path: WorkspaceRelativePathDto,
    pub old: BoundedText,
    pub new: BoundedText,
    #[serde(default)]
    pub expected_content: Option<BoundedText>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExecuteInput {
    pub program: BoundedText,
    pub args: Vec<BoundedText>,
}

impl ExecuteInput {
    /// Validates the invocation shape after each element passed its own text
    /// bound: the argument count and aggregate argument bytes stay within the
    /// execute contract (PR24-023).
    ///
    /// # Errors
    ///
    /// Returns a validation error when the argument count or aggregate bytes
    /// exceed the execute bounds.
    pub fn validate_bounds(&self) -> DtoResult<()> {
        let aggregate = self
            .args
            .iter()
            .map(|argument| argument.as_str().len())
            .sum::<usize>();
        if self.args.len() > MAX_EXECUTE_ARGUMENTS || aggregate > MAX_EXECUTE_ARGUMENTS_TOTAL_BYTES
        {
            Err(intention_types::ErrorDto::validation(
                "invalid_tool_execute_arguments",
                "execute argument count or aggregate size exceeds bounds",
            ))
        } else {
            Ok(())
        }
    }
}

impl<'de> Deserialize<'de> for ExecuteInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawExecuteInput {
            program: BoundedText,
            args: Vec<BoundedText>,
        }
        let raw = RawExecuteInput::deserialize(deserializer)?;
        let input = Self {
            program: raw.program,
            args: raw.args,
        };
        input.validate_bounds().map_err(|error| {
            serde::de::Error::custom(format!("invalid execute input ({})", error.code()))
        })?;
        Ok(input)
    }
}

impl ToolInput {
    /// Returns the logical workspace-relative path targeted by this input, when
    /// the tool operates on a single one. Glob matches by pattern and Execute
    /// runs a program, so neither targets one workspace path.
    #[must_use]
    pub const fn logical_path(&self) -> Option<&WorkspaceRelativePathDto> {
        match self {
            Self::Read(input) => Some(&input.path),
            Self::Write(input) => Some(&input.path),
            Self::Edit(input) => Some(&input.path),
            Self::Grep(input) => input.path.as_ref(),
            Self::Glob(_) | Self::Execute(_) => None,
        }
    }
}

/// Typed tool result family.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", content = "value", rename_all = "snake_case")]
pub enum ToolResult {
    Read(TextResult),
    Glob(PathsResult),
    Grep(GrepResult),
    Write(WriteResult),
    Edit(WriteResult),
    Execute(TextResult),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolResultEnvelope {
    pub schema_version: u16,
    pub context: ToolContext,
    pub result: ToolResult,
    pub observability: ToolObservability,
    #[serde(default)]
    pub execution: Option<ToolExecutionMetadata>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextResult {
    pub text: BoundedText,
    pub truncated: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GrepResult {
    pub matches: Vec<GrepMatch>,
    #[serde(default)]
    pub truncated: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GrepMatch {
    pub path: WorkspaceRelativePathDto,
    pub line: u64,
    pub column: u64,
    pub fragment: BoundedText,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PathsResult {
    pub paths: Vec<WorkspaceRelativePathDto>,
    #[serde(default)]
    pub truncated: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WriteResult {
    pub bytes: u64,
}

/// Normalized content shape of one projected concrete tool result.
///
/// The projection keeps the typed payload bounded and workspace-relative: text
/// stays in [`BoundedText`], path lists and grep matches are clamped to the
/// search bounds with an explicit truncation flag, and mutations carry only a
/// byte count.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolProjectedContent {
    /// Bounded text produced by Read or Execute.
    Text { text: BoundedText, truncated: bool },
    /// Bounded workspace-relative path list produced by Glob.
    Paths {
        paths: Vec<WorkspaceRelativePathDto>,
        truncated: bool,
    },
    /// Bounded workspace-relative matches produced by Grep.
    Matches {
        matches: Vec<GrepMatch>,
        truncated: bool,
    },
    /// Byte-count summary produced by Write or Edit.
    Mutation { bytes: u64 },
}

/// Bounded, redacted, normalized projection of one tool result envelope,
/// suitable for durable persistence and safe rendering.
///
/// The projection never carries an absolute path, OS resource detail, command
/// line, or environment value: the working-directory identity is redacted to
/// [`REDACTED_WORKSPACE_CWD`], paths stay workspace-relative, and content is
/// clamped to the search bounds.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolResultProjection {
    pub schema_version: u16,
    pub tool: ToolId,
    pub content: ToolProjectedContent,
    pub execution: ToolExecutionMetadata,
}

fn projected_content(result: &ToolResult) -> ToolProjectedContent {
    match result {
        ToolResult::Read(value) | ToolResult::Execute(value) => ToolProjectedContent::Text {
            text: value.text.clone(),
            truncated: value.truncated,
        },
        ToolResult::Glob(value) => ToolProjectedContent::Paths {
            truncated: value.truncated || value.paths.len() > MAX_GLOB_MATCHES,
            paths: value.paths.iter().take(MAX_GLOB_MATCHES).cloned().collect(),
        },
        ToolResult::Grep(value) => ToolProjectedContent::Matches {
            truncated: value.truncated || value.matches.len() > MAX_GREP_MATCHES,
            matches: value
                .matches
                .iter()
                .take(MAX_GREP_MATCHES)
                .cloned()
                .collect(),
        },
        ToolResult::Write(value) | ToolResult::Edit(value) => {
            ToolProjectedContent::Mutation { bytes: value.bytes }
        }
    }
}

impl ToolResult {
    /// Returns the concrete registered tool this result belongs to.
    #[must_use]
    pub const fn tool_id(&self) -> ToolId {
        match self {
            Self::Read(_) => ToolId::Read,
            Self::Glob(_) => ToolId::Glob,
            Self::Grep(_) => ToolId::Grep,
            Self::Write(_) => ToolId::Write,
            Self::Edit(_) => ToolId::Edit,
            Self::Execute(_) => ToolId::Execute,
        }
    }

    /// Projects this result into the bounded, redacted, normalized form.
    ///
    /// A bare result carries no invocation timing, so the execution metadata
    /// defaults to zero elapsed time; prefer
    /// [`ToolResultEnvelope::projection`] when durable records need real
    /// timing and invocation metadata.
    #[must_use]
    pub fn projection(&self) -> ToolResultProjection {
        ToolResultProjection {
            schema_version: TOOL_SCHEMA_VERSION,
            tool: self.tool_id(),
            content: projected_content(self),
            execution: ToolExecutionMetadata::for_workspace(ToolPolicy::Allowed, 0),
        }
    }
}

impl ToolResultEnvelope {
    /// Projects the envelope into the bounded, redacted, normalized form
    /// suitable for durable persistence.
    ///
    /// Timing and policy come from the recorded execution metadata, falling
    /// back to the envelope observability when metadata is absent.
    #[must_use]
    pub fn projection(&self) -> ToolResultProjection {
        ToolResultProjection {
            schema_version: self.schema_version,
            tool: self.result.tool_id(),
            content: projected_content(&self.result),
            execution: self.execution.clone().unwrap_or_else(|| {
                ToolExecutionMetadata::for_workspace(
                    self.observability.policy,
                    self.observability.elapsed_ms,
                )
            }),
        }
    }
}

/// Outcome of one admitted tool execution together with the typed terminal
/// status of the executed program, when the tool ran one.
pub(crate) struct ExecutedTool {
    pub(crate) result: ToolResult,
    pub(crate) process_status: Option<ToolProcessStatus>,
}

impl ExecutedTool {
    pub(crate) const fn bare(result: ToolResult) -> Self {
        Self {
            result,
            process_status: None,
        }
    }
}

/// Local execution service rooted at an authorized workspace.
pub struct ToolService {
    root: WorkspaceRoot,
}
impl ToolService {
    /// Creates a service.
    #[must_use]
    pub const fn new(root: WorkspaceRoot) -> Self {
        Self { root }
    }
    /// Dispatches a typed tool call with cooperative cancellation.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error when cancellation, validation, workspace resolution, or execution fails.
    pub fn dispatch_with_cancellation(
        &self,
        call: ToolCallId,
        input: ToolInput,
        cancellation: CancellationSignal,
    ) -> DtoResult<ToolResult> {
        self.execute_checked(call, input, cancellation)
            .map(|executed| executed.result)
    }

    /// Validates admission, executes one tool effect, and reports the typed
    /// terminal program status when the tool executed a program.
    fn execute_checked(
        &self,
        call: ToolCallId,
        input: ToolInput,
        cancellation: CancellationSignal,
    ) -> DtoResult<ExecutedTool> {
        let _ = call;
        // Keep the identity on the real dispatch path: adapters cannot execute
        // a call while silently substituting another call id.
        if cancellation.is_cancelled() {
            return Err(intention_types::ErrorDto::validation(
                "tool_cancelled",
                "tool invocation was cancelled before execution",
            ));
        }
        Ok(match input {
            ToolInput::Read(i) => ExecutedTool::bare(file::read(&self.root, i)?),
            ToolInput::Write(i) => ExecutedTool::bare(file::write(&self.root, i)?),
            ToolInput::Edit(i) => ExecutedTool::bare(file::edit(&self.root, i)?),
            ToolInput::Glob(i) => ExecutedTool::bare(search::glob(&self.root, i)?),
            ToolInput::Grep(i) => ExecutedTool::bare(search::grep(&self.root, i)?),
            ToolInput::Execute(i) => execute::run(&self.root, i, cancellation)?,
        })
    }

    /// Invokes a tool and returns the result-boundary envelope.
    ///
    /// The invocation context is validated before any tool effect occurs.
    ///
    /// # Errors
    ///
    /// Returns the typed error produced while validating and dispatching.
    pub fn invoke_enveloped(&self, invocation: ToolInvocation) -> DtoResult<ToolResultEnvelope> {
        self.invoke_enveloped_with_cancellation(invocation, CancellationSignal::new())
    }

    /// Invokes a tool with cancellation and records result-boundary metadata.
    ///
    /// # Errors
    ///
    /// Returns the typed error produced while validating and dispatching.
    pub fn invoke_enveloped_with_cancellation(
        &self,
        invocation: ToolInvocation,
        cancellation: CancellationSignal,
    ) -> DtoResult<ToolResultEnvelope> {
        invocation.validate_schema_version()?;
        let call_id = invocation.context.call_id;
        invocation.validate_call_id(call_id)?;
        let logical_path = invocation.input.logical_path().cloned();
        let started = Instant::now();
        let executed =
            self.execute_checked(invocation.context.call_id, invocation.input, cancellation);
        let elapsed_ms = started.elapsed().as_millis() as u64;
        let outcome = if executed.is_ok() {
            ToolOutcome::Succeeded
        } else {
            ToolOutcome::Failed
        };
        let executed = executed?;
        Ok(ToolResultEnvelope {
            schema_version: invocation.schema_version,
            context: invocation.context,
            result: executed.result,
            observability: ToolObservability {
                outcome,
                policy: ToolPolicy::Allowed,
                elapsed_ms,
            },
            execution: Some(
                ToolExecutionMetadata::for_workspace(ToolPolicy::Allowed, elapsed_ms)
                    .with_path(logical_path)
                    .with_process_status(executed.process_status),
            ),
        })
    }
}

fn read_tool(root: &WorkspaceRoot, input: ReadInput) -> DtoResult<ToolResult> {
    let mut file = std::fs::File::open(root.resolve_path(&input.path)?).map_err(|_| {
        intention_types::ErrorDto::validation("tool_read_failed", "unable to read workspace file")
    })?;
    let mut bytes = Vec::new();
    let source_truncated = read_bounded(&mut file, &mut bytes).map_err(|_| {
        intention_types::ErrorDto::validation("tool_read_failed", "unable to read workspace file")
    })?;
    let (text, truncated) = bounded_lossy(&bytes);
    Ok(ToolResult::Read(TextResult {
        text: bounded_text(text)?,
        truncated: truncated || source_truncated,
    }))
}

fn execute_tool(
    root: &WorkspaceRoot,
    input: ExecuteInput,
    cancellation: CancellationSignal,
) -> DtoResult<ExecutedTool> {
    // In-process typed construction must observe the same execute bounds as
    // the validating deserialize path.
    input.validate_bounds()?;
    let mut command = Command::new(input.program.as_str());
    command.args(input.args.iter().map(BoundedText::as_str));
    command.current_dir(root.execute_cwd());
    // Execute with the caller's environment. WorkspaceRoot scopes filesystem
    // path resolution and the child CWD, not the process environment.
    // Preserve the caller environment without requiring every inherited key
    // and value to be valid Unicode. `vars()` panics on such entries.
    command.envs(std::env::vars_os());
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // The child leads its own process group so timeout and cancellation
        // can terminate background descendants that inherited the pipes
        // (PR24-011).
        command.process_group(0);
    }
    let child = command.spawn().map_err(|_| {
        intention_types::ErrorDto::validation(
            "tool_execute_spawn_failed",
            "unable to spawn workspace command",
        )
    })?;
    cancellation.observe_spawn();
    let output = bounded_output(child, cancellation).map_err(|code| {
        intention_types::ErrorDto::validation(code, "workspace command execution failed")
    })?;
    let process_status = ToolProcessStatus::classify(output.status);
    let (stdout, _) = bounded_lossy(&output.stdout);
    let (stderr, _) = bounded_lossy(&output.stderr);
    let truncated = output.stdout_truncated || output.stderr_truncated;
    // Render the terminal status from the typed classification so the text and
    // the typed `ToolProcessStatus` can never disagree.
    let status_text = match process_status {
        ToolProcessStatus::Success => "exit_code:0".to_owned(),
        ToolProcessStatus::NonZero { code } => format!("exit_code:{code}"),
        ToolProcessStatus::Signal { signal } => format!("signal:{signal}"),
    };
    let text = format!(
        "stdout:\n{stdout}\nstderr:\n{stderr}\n{status_text}{}",
        if truncated { "\n[truncated]" } else { "" }
    );
    // A known non-zero exit or known signal termination is a normalized
    // program result, not a transport error; only the unknown-effect paths in
    // `bounded_output` turn into typed errors.
    Ok(ExecutedTool {
        result: ToolResult::Execute(TextResult {
            text: BoundedText::new(text)?,
            truncated,
        }),
        process_status: Some(process_status),
    })
}

fn write_tool(root: &WorkspaceRoot, input: WriteInput) -> DtoResult<ToolResult> {
    let bytes = input.content.as_str().len() as u64;
    let path = root.resolve_new_file_path(&input.path)?;
    // Fail closed on any final-component symlink, including dangling links:
    // `symlink_metadata` inspects the link itself, while `exists` would follow
    // it and skip this rejection. A missing entry stays writable, and any
    // other metadata failure is treated conservatively as a conflict.
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(intention_types::ErrorDto::validation(
                "tool_write_conflict",
                "workspace file changed before write",
            ));
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(intention_types::ErrorDto::validation(
                "tool_write_conflict",
                "workspace file changed before write",
            ));
        }
        _ => {}
    }
    if let Some(expected) = input.expected_content.as_ref() {
        // Expected-content equality is checked against a bounded read: a
        // larger file can never equal the bounded expected content and is
        // reported as changed instead of being slurped (PR24-022).
        let current = match read_limited(&path, MAX_EDIT_TARGET_BYTES) {
            LimitedReadOutcome::Content(bytes) => String::from_utf8(bytes).map_err(|_| {
                intention_types::ErrorDto::validation(
                    "tool_write_conflict",
                    "workspace file changed before write",
                )
            })?,
            LimitedReadOutcome::TooLarge => {
                return Err(intention_types::ErrorDto::validation(
                    "tool_write_conflict",
                    "workspace file changed before write",
                ));
            }
            LimitedReadOutcome::Unreadable => {
                return Err(intention_types::ErrorDto::validation(
                    "tool_write_conflict",
                    "workspace file changed before write",
                ));
            }
        };
        if current != expected.as_str() {
            return Err(intention_types::ErrorDto::validation(
                "tool_write_conflict",
                "workspace file changed before write",
            ));
        }
    }
    std::fs::write(path, input.content.as_str()).map_err(|_| {
        intention_types::ErrorDto::validation("tool_write_failed", "unable to write workspace file")
    })?;
    Ok(ToolResult::Write(WriteResult { bytes }))
}

fn edit_tool(root: &WorkspaceRoot, input: EditInput) -> DtoResult<ToolResult> {
    let path = root.resolve_path(&input.path)?;
    if std::fs::symlink_metadata(&path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(true)
    {
        return Err(intention_types::ErrorDto::validation(
            "tool_edit_conflict",
            "workspace file changed before edit",
        ));
    }
    // Edit reads the complete target to apply one replacement; the target is
    // therefore size-bounded so a huge file cannot allocate unboundedly
    // (PR24-022).
    let text = match read_limited(&path, MAX_EDIT_TARGET_BYTES) {
        LimitedReadOutcome::Content(bytes) => String::from_utf8(bytes).map_err(|_| {
            intention_types::ErrorDto::validation(
                "tool_read_failed",
                "unable to read workspace file",
            )
        })?,
        LimitedReadOutcome::TooLarge => {
            return Err(intention_types::ErrorDto::validation(
                "tool_edit_target_too_large",
                "edit target exceeds the workspace file size bound",
            ));
        }
        LimitedReadOutcome::Unreadable => {
            return Err(intention_types::ErrorDto::validation(
                "tool_read_failed",
                "unable to read workspace file",
            ));
        }
    };
    if !text.contains(input.old.as_str()) {
        return Err(intention_types::ErrorDto::validation(
            "edit_target_missing",
            "edit target was not found",
        ));
    }
    if let Some(expected) = input.expected_content.as_ref()
        && text != expected.as_str()
    {
        return Err(intention_types::ErrorDto::validation(
            "tool_edit_conflict",
            "workspace file changed before edit",
        ));
    }
    let replacement = text.replacen(input.old.as_str(), input.new.as_str(), 1);
    if std::fs::symlink_metadata(&path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(true)
    {
        return Err(intention_types::ErrorDto::validation(
            "tool_edit_conflict",
            "workspace file changed before edit",
        ));
    }
    std::fs::write(path, &replacement).map_err(|_| {
        intention_types::ErrorDto::validation("tool_write_failed", "unable to write workspace file")
    })?;
    Ok(ToolResult::Edit(WriteResult {
        bytes: replacement.len() as u64,
    }))
}

fn glob_tool(root: &WorkspaceRoot, input: GlobInput) -> DtoResult<ToolResult> {
    validate_search_pattern(input.pattern.as_str())?;
    let base = root.canonical_path();
    let pattern = base
        .join(input.pattern.as_str())
        .to_str()
        .ok_or_else(|| {
            intention_types::ErrorDto::validation("invalid_tool_pattern", "tool pattern is invalid")
        })?
        .to_owned();
    let mut paths = Vec::new();
    let mut truncated = false;
    for entry in glob::glob(&pattern).map_err(|_| {
        intention_types::ErrorDto::validation("invalid_tool_pattern", "tool pattern is invalid")
    })? {
        // One unreadable or raced-away entry skips itself instead of aborting
        // the whole search; listing what is safely listable keeps repeated
        // traversals deterministic.
        let Ok(path) = entry else { continue };
        // Same fail-closed symlink policy as the other file tools: links are
        // never followed, reported, or resolved into canonical targets, which
        // also rules out duplicate aliases of one logical file.
        if contains_symlink_component(base, &path) {
            continue;
        }
        let Some(relative) = path.strip_prefix(base).ok().and_then(|p| p.to_str()) else {
            continue;
        };
        let Ok(value) = WorkspaceRelativePathDto::parse(relative.replace('\\', "/")) else {
            continue;
        };
        if paths.len() >= MAX_GLOB_MATCHES {
            truncated = true;
            break;
        }
        paths.push(value);
    }
    paths.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    paths.dedup();
    Ok(ToolResult::Glob(PathsResult { paths, truncated }))
}

/// Reports whether any component of `path` beneath `root` is a symbolic link.
///
/// This mirrors [`intention_workspace`] traversal fail-closed behavior:
/// unreadable entries are treated as link-bearing so raced-away paths are
/// excluded rather than leaked through canonicalization.
fn contains_symlink_component(root: &std::path::Path, path: &std::path::Path) -> bool {
    let relative = match path.strip_prefix(root) {
        Ok(relative) => relative,
        Err(_) => return true,
    };
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return true;
                }
            }
            Err(_) => return true,
        }
    }
    false
}

fn grep_tool(root: &WorkspaceRoot, input: GrepInput) -> DtoResult<ToolResult> {
    validate_search_pattern(input.pattern.as_str())?;
    if input.scope.is_some() {
        return grep_scoped(root, input);
    }
    let path = input
        .path
        .as_ref()
        .map(|path| root.resolve_path(path))
        .transpose()?
        .ok_or_else(|| {
            intention_types::ErrorDto::validation(
                "invalid_tool_path",
                "grep requires a workspace path",
            )
        })?;
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| {
        intention_types::ErrorDto::validation("tool_search_failed", "workspace search failed")
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(intention_types::ErrorDto::validation(
            "tool_search_failed",
            "workspace search failed",
        ));
    }
    let mut file = std::fs::File::open(path).map_err(|_| {
        intention_types::ErrorDto::validation("tool_search_failed", "workspace search failed")
    })?;
    let mut bytes = Vec::new();
    let source_truncated = read_bounded(&mut file, &mut bytes).map_err(|_| {
        intention_types::ErrorDto::validation("tool_search_failed", "workspace search failed")
    })?;
    let text = String::from_utf8_lossy(&bytes);
    let lossy_truncated = std::str::from_utf8(&bytes).is_err();
    let logical = input
        .path
        .as_ref()
        .ok_or_else(|| {
            intention_types::ErrorDto::validation(
                "invalid_tool_path",
                "grep requires a workspace path",
            )
        })?
        .clone();
    let mut matches = Vec::new();
    let mut retained_bytes = 0usize;
    let mut truncated = source_truncated || lossy_truncated;
    for (line_index, line) in text.lines().enumerate() {
        let Some(column) = line.find(input.pattern.as_str()) else {
            continue;
        };
        if matches.len() >= MAX_GREP_MATCHES {
            truncated = true;
            break;
        }
        let fragment = if line.len() > MAX_TOOL_OUTPUT_BYTES {
            truncated = true;
            let end = line
                .char_indices()
                .take_while(|(index, _)| *index < MAX_TOOL_OUTPUT_BYTES)
                .map(|(index, _)| index)
                .last()
                .unwrap_or(0);
            line[..end].to_owned()
        } else {
            line.to_owned()
        };
        if !record_grep_match(
            &mut matches,
            &mut retained_bytes,
            &mut truncated,
            logical.clone(),
            line_index as u64 + 1,
            line[..column].chars().count() as u64 + 1,
            fragment,
        )? {
            break;
        }
    }
    Ok(ToolResult::Grep(GrepResult { matches, truncated }))
}

fn grep_scoped(root: &WorkspaceRoot, input: GrepInput) -> DtoResult<ToolResult> {
    let scope = input.scope.ok_or_else(|| {
        intention_types::ErrorDto::validation("invalid_tool_path", "grep requires a workspace path")
    })?;
    let (base, single) = match scope {
        GrepScope::File { path } => (root.resolve_path(&path)?, Some(path)),
        GrepScope::Directory { path } => (root.resolve_path(&path)?, None),
        GrepScope::Workspace => (root.canonical_path().to_path_buf(), None),
    };
    let metadata = std::fs::symlink_metadata(&base).map_err(|_| {
        intention_types::ErrorDto::validation("tool_search_failed", "workspace search failed")
    })?;
    if metadata.file_type().is_symlink() || (single.is_some() && !metadata.is_file()) {
        return Err(intention_types::ErrorDto::validation(
            "tool_search_failed",
            "workspace search failed",
        ));
    }
    let mut files = Vec::new();
    let mut file_scan_truncated = false;
    if metadata.is_file() && !metadata.file_type().is_symlink() {
        files.push(base);
    } else if metadata.is_dir() {
        let mut pending = vec![base];
        'traverse: while let Some(directory) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(&directory) else {
                continue;
            };
            let mut children = entries.filter_map(Result::ok).collect::<Vec<_>>();
            children.sort_by_key(|entry| entry.file_name());
            for entry in children.into_iter().rev() {
                let path = entry.path();
                let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                if metadata.file_type().is_symlink() {
                    continue;
                }
                if metadata.is_dir() {
                    pending.push(path);
                } else if metadata.is_file()
                    && !contains_symlink_component(root.canonical_path(), &path)
                {
                    if files.len() >= MAX_GREP_FILES {
                        file_scan_truncated = true;
                        break 'traverse;
                    }
                    files.push(path);
                } else {
                    continue;
                }
            }
        }
    } else {
        return Err(intention_types::ErrorDto::validation(
            "tool_search_failed",
            "workspace search failed",
        ));
    }
    files.sort();
    let mut matches = Vec::new();
    let mut retained_bytes = 0usize;
    let mut truncated = file_scan_truncated;
    for path in files {
        let mut file = std::fs::File::open(&path).map_err(|_| {
            intention_types::ErrorDto::validation("tool_search_failed", "workspace search failed")
        })?;
        let mut bytes = Vec::new();
        // Every file is read through the bounded reader, so an oversized file
        // cannot allocate unbounded memory during a directory search; the
        // truncation flag reports the dropped tail (PR24-022).
        let source_truncated = read_bounded(&mut file, &mut bytes).map_err(|_| {
            intention_types::ErrorDto::validation("tool_search_failed", "workspace search failed")
        })?;
        let text = String::from_utf8_lossy(&bytes);
        let lossy_truncated = std::str::from_utf8(&bytes).is_err();
        if source_truncated || lossy_truncated {
            truncated = true;
        }
        let logical = single.clone().or_else(|| {
            path.strip_prefix(root.canonical_path()).ok().and_then(|p| {
                WorkspaceRelativePathDto::parse(p.to_string_lossy().replace('\\', "/")).ok()
            })
        });
        let Some(logical) = logical else { continue };
        for (line_index, line) in text.lines().enumerate() {
            let Some(column) = line.find(input.pattern.as_str()) else {
                continue;
            };
            if matches.len() >= MAX_GREP_MATCHES {
                truncated = true;
                break;
            }
            let fragment = if line.len() > MAX_TOOL_OUTPUT_BYTES {
                truncated = true;
                let end = line
                    .char_indices()
                    .take_while(|(index, _)| *index < MAX_TOOL_OUTPUT_BYTES)
                    .map(|(index, _)| index)
                    .last()
                    .unwrap_or(0);
                line[..end].to_owned()
            } else {
                line.to_owned()
            };
            if !record_grep_match(
                &mut matches,
                &mut retained_bytes,
                &mut truncated,
                logical.clone(),
                line_index as u64 + 1,
                line[..column].chars().count() as u64 + 1,
                fragment,
            )? {
                return Ok(ToolResult::Grep(GrepResult { matches, truncated }));
            }
        }
        if matches.len() >= MAX_GREP_MATCHES {
            truncated = true;
            break;
        }
    }
    Ok(ToolResult::Grep(GrepResult { matches, truncated }))
}

/// Records one grep match when it fits the aggregate fragment bound.
///
/// Per-line fragment caps alone allow a very large aggregate result; the
/// aggregate retained bytes are clamped with the truncation flag (PR24-022).
///
/// # Errors
///
/// Returns a validation error when the fragment violates the bounded-text
/// contract.
fn record_grep_match(
    matches: &mut Vec<GrepMatch>,
    retained_bytes: &mut usize,
    truncated: &mut bool,
    path: WorkspaceRelativePathDto,
    line: u64,
    column: u64,
    fragment: String,
) -> DtoResult<bool> {
    if fragment.len() > MAX_GREP_AGGREGATE_BYTES.saturating_sub(*retained_bytes) {
        *truncated = true;
        return Ok(false);
    }
    *retained_bytes += fragment.len();
    matches.push(GrepMatch {
        path,
        line,
        column,
        fragment: bounded_text(fragment)?,
    });
    Ok(true)
}

fn validate_search_pattern(pattern: &str) -> DtoResult<()> {
    // Absoluteness must not depend on the host platform: Windows drive-letter
    // and UNC roots are rejected everywhere so search patterns stay strictly
    // workspace-relative on every supported target.
    let windows_rooted = (pattern.len() >= 2
        && pattern.as_bytes()[1] == b':'
        && pattern.as_bytes()[0].is_ascii_alphabetic())
        || pattern.starts_with("\\\\");
    if pattern.is_empty()
        || pattern.contains('\0')
        || std::path::Path::new(pattern).is_absolute()
        || pattern.starts_with('/')
        || pattern.starts_with('\\')
        || windows_rooted
        || pattern.split(['/', '\\']).any(|part| part == "..")
    {
        return Err(intention_types::ErrorDto::validation(
            "invalid_tool_pattern",
            "tool pattern must be relative and stay within the workspace",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod coverage_helpers {
    use super::*;
    use std::io::{self, Read};

    struct PanicReader;
    impl Read for PanicReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            std::panic::resume_unwind(Box::new("injected reader panic"))
        }
    }

    #[test]
    fn bounded_lossy_handles_invalid_utf8_at_boundary() {
        let mut bytes = vec![b'a'; MAX_TOOL_OUTPUT_BYTES];
        bytes[MAX_TOOL_OUTPUT_BYTES - 1] = 0xff;
        let (text, truncated) = bounded_lossy(&[bytes, vec![b'b']].concat());
        assert!(truncated);
        assert!(text.ends_with("\n[truncated]"));
        assert!(!text.contains('\u{fffd}'));
    }

    #[test]
    fn reader_panic_is_join_error() {
        let handle = thread::spawn(|| {
            let mut reader = PanicReader;
            let mut output = Vec::new();
            read_bounded(&mut reader, &mut output).map(|truncated| (output, truncated))
        });
        assert!(matches!(
            join_reader(Some(handle)),
            Err("tool_execute_read_failed")
        ));
    }

    #[test]
    fn symlink_component_rejects_outside_and_missing_paths() {
        let root = std::env::temp_dir().join(format!("tools-coverage-{}", std::process::id()));
        assert!(std::fs::create_dir_all(&root).is_ok());
        assert!(contains_symlink_component(&root, &root.join("missing")));
        assert!(contains_symlink_component(&root, &root.join("outside")));
        assert!(std::fs::remove_dir_all(root).is_ok());
    }

    #[test]
    fn validation_rejects_empty_nul_and_windows_patterns() {
        for pattern in ["", "a\0b", "C:/x", "\\\\server\\share", "../x", "/x", "\\x"] {
            assert!(validate_search_pattern(pattern).is_err(), "{pattern:?}");
        }
    }
}

/// Private execution boundary; adapters implement this without leaking erased values.
#[expect(
    dead_code,
    reason = "The private executor boundary is activated by the composition slice."
)]
trait ToolExecutor {
    fn execute(&self, call: ToolCallId, input: ToolInput) -> DtoResult<ToolResult>;
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "local fixture assertions"
)]
mod direct_execution_tests {
    use super::*;
    use intention_types::WorkspaceRelativePathDto;
    use tempfile::TempDir;

    fn workspace(root: &TempDir) -> intention_workspace::WorkspaceRoot {
        intention_workspace::WorkspaceRoot::resolve(
            &intention_domain::WorkspaceRootDto::parse(root.path().to_string_lossy().into_owned())
                .expect("fixture root parses"),
        )
        .expect("fixture workspace resolves")
    }

    fn relative(path: &str) -> WorkspaceRelativePathDto {
        WorkspaceRelativePathDto::parse(path).expect("fixture path parses")
    }

    #[test]
    fn direct_read_edit_and_grep_executions_cover_bounded_file_paths() {
        let dir = TempDir::new().expect("temporary fixture dir");
        std::fs::write(
            dir.path().join("source.txt"),
            "line one\nneedle here\nline three",
        )
        .expect("seed source");
        let root = workspace(&dir);
        let service = ToolService::new(root);

        // Read path.
        let read = service
            .dispatch_with_cancellation(
                ToolCallId::new(),
                ToolInput::Read(ReadInput {
                    path: relative("source.txt"),
                }),
                CancellationSignal::new(),
            )
            .expect("direct read dispatches");
        assert!(matches!(read, ToolResult::Read(_)));

        // Pattern-only grep path.
        let grep = service
            .dispatch_with_cancellation(
                ToolCallId::new(),
                ToolInput::Grep(GrepInput {
                    pattern: BoundedText::new("needle").expect("pattern"),
                    scope: None,
                    path: Some(relative("source.txt")),
                }),
                CancellationSignal::new(),
            )
            .expect("direct file grep dispatches");
        let ToolResult::Grep(grep) = grep else {
            panic!("grep result expected");
        };
        assert_eq!(grep.matches.len(), 1);

        // Edit path with expected-content verification (the expected value is
        // the complete file content before the edit); a stale expectation
        // conflicts before any mutation, then the matching edit succeeds.
        let full = "line one\nneedle here\nline three";
        let mismatch = service.dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Edit(EditInput {
                path: relative("source.txt"),
                old: BoundedText::new("needle").expect("old"),
                new: BoundedText::new("again").expect("new"),
                expected_content: Some(BoundedText::new("stale content").expect("expected")),
            }),
            CancellationSignal::new(),
        );
        assert!(
            matches!(&mismatch, Err(error) if error.code() == "tool_edit_conflict"),
            "unexpected mismatch result: {mismatch:?}"
        );
        let edit = service
            .dispatch_with_cancellation(
                ToolCallId::new(),
                ToolInput::Edit(EditInput {
                    path: relative("source.txt"),
                    old: BoundedText::new("needle").expect("old"),
                    new: BoundedText::new("replaced").expect("new"),
                    expected_content: Some(BoundedText::new(full).expect("expected")),
                }),
                CancellationSignal::new(),
            )
            .expect("direct edit dispatches");
        assert!(matches!(edit, ToolResult::Edit(_)));
    }

    #[test]
    fn direct_execute_collects_bounded_output_successfully() {
        let dir = TempDir::new().expect("temporary fixture dir");
        let service = ToolService::new(workspace(&dir));
        let result = service
            .dispatch_with_cancellation(
                ToolCallId::new(),
                ToolInput::Execute(ExecuteInput {
                    program: BoundedText::new(if cfg!(windows) { "cmd" } else { "sh" })
                        .expect("program"),
                    args: if cfg!(windows) {
                        vec![BoundedText::new("/C echo ok").expect("arg")]
                    } else {
                        vec![
                            BoundedText::new("-c").expect("flag"),
                            BoundedText::new("printf 'ok\\n'").expect("script"),
                        ]
                    },
                }),
                CancellationSignal::new(),
            )
            .expect("direct execute dispatches");
        let ToolResult::Execute(value) = result else {
            panic!("execute result expected");
        };
        assert!(value.text.as_str().contains("exit_code:0"));
    }

    #[test]
    fn direct_write_glob_and_oversized_read_paths_stay_bounded() {
        let dir = TempDir::new().expect("temporary fixture dir");
        std::fs::write(dir.path().join("existing.txt"), "before").expect("seed existing");
        std::fs::write(dir.path().join("big.bin"), vec![b'x'; 70 * 1024]).expect("seed big");
        let root = workspace(&dir);
        let service = ToolService::new(root);

        // Oversized read reports truncation within the tool output bound.
        let read = service
            .dispatch_with_cancellation(
                ToolCallId::new(),
                ToolInput::Read(ReadInput {
                    path: relative("big.bin"),
                }),
                CancellationSignal::new(),
            )
            .expect("oversized read dispatches");
        let ToolResult::Read(value) = read else {
            panic!("read result expected");
        };
        assert!(value.truncated);

        // Write with a stale expected content conflicts; then the matching
        // write succeeds.
        let stale = service.dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Write(WriteInput {
                path: relative("existing.txt"),
                content: BoundedText::new("after").expect("content"),
                expected_content: Some(BoundedText::new("stale").expect("expected")),
            }),
            CancellationSignal::new(),
        );
        assert!(
            matches!(&stale, Err(error) if error.code() == "tool_write_conflict"),
            "unexpected write conflict result: {stale:?}"
        );
        let write = service
            .dispatch_with_cancellation(
                ToolCallId::new(),
                ToolInput::Write(WriteInput {
                    path: relative("existing.txt"),
                    content: BoundedText::new("after").expect("content"),
                    expected_content: Some(BoundedText::new("before").expect("expected")),
                }),
                CancellationSignal::new(),
            )
            .expect("matching write dispatches");
        assert!(matches!(write, ToolResult::Write(_)));

        // Glob of a new path resolves to an empty result.
        let glob = service
            .dispatch_with_cancellation(
                ToolCallId::new(),
                ToolInput::Glob(GlobInput {
                    pattern: BoundedText::new("*.new").expect("pattern"),
                }),
                CancellationSignal::new(),
            )
            .expect("empty glob dispatches");
        assert!(matches!(glob, ToolResult::Glob(_)));
    }

    #[test]
    fn direct_edit_and_glob_error_and_bound_branches_are_typed() {
        let dir = TempDir::new().expect("temporary fixture dir");
        std::fs::write(dir.path().join("plain.txt"), "known content").expect("seed plain");
        std::fs::write(dir.path().join("huge.txt"), vec![b'a'; 1024 * 1024 + 16])
            .expect("seed huge");
        std::fs::create_dir(dir.path().join("files")).expect("seed directory");
        std::fs::write(dir.path().join("files/one.txt"), "one").expect("seed one");
        let service = ToolService::new(workspace(&dir));

        // A missing edit target is a typed failure.
        let missing = service.dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Edit(EditInput {
                path: relative("plain.txt"),
                old: BoundedText::new("absent").expect("old"),
                new: BoundedText::new("x").expect("new"),
                expected_content: None,
            }),
            CancellationSignal::new(),
        );
        assert!(matches!(&missing, Err(error) if error.code() == "edit_target_missing"));

        // An oversized edit target is rejected before any read-back.
        let oversized = service.dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Edit(EditInput {
                path: relative("huge.txt"),
                old: BoundedText::new("a").expect("old"),
                new: BoundedText::new("b").expect("new"),
                expected_content: None,
            }),
            CancellationSignal::new(),
        );
        assert!(matches!(
            &oversized,
            Err(error) if error.code() == "tool_edit_target_too_large"
        ));

        // Reading a directory fails closed.
        let directory = service.dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Read(ReadInput {
                path: relative("files"),
            }),
            CancellationSignal::new(),
        );
        assert!(matches!(&directory, Err(error) if error.code() == "tool_read_failed"));

        // A successful glob returns the matching workspace-relative paths.
        let matched = service
            .dispatch_with_cancellation(
                ToolCallId::new(),
                ToolInput::Glob(GlobInput {
                    pattern: BoundedText::new("files/*.txt").expect("pattern"),
                }),
                CancellationSignal::new(),
            )
            .expect("matching glob dispatches");
        let ToolResult::Glob(glob) = matched else {
            panic!("glob result expected");
        };
        assert_eq!(glob.paths.len(), 1);
    }

    #[test]
    fn direct_grep_clamps_long_lines_and_execute_rejects_oversized_shapes() {
        let dir = TempDir::new().expect("temporary fixture dir");
        let mut long_line = vec![b'x'; 70 * 1024];
        long_line.splice(70 * 1024 - 8.., b"needle!".iter().copied());
        std::fs::write(dir.path().join("long.txt"), &long_line).expect("seed long line");
        let service = ToolService::new(workspace(&dir));

        let grep = service
            .dispatch_with_cancellation(
                ToolCallId::new(),
                ToolInput::Grep(GrepInput {
                    pattern: BoundedText::new("needle").expect("pattern"),
                    scope: None,
                    path: Some(relative("long.txt")),
                }),
                CancellationSignal::new(),
            )
            .expect("long-line grep dispatches");
        let ToolResult::Grep(grep) = grep else {
            panic!("grep result expected");
        };
        assert!(
            grep.truncated,
            "a match beyond the fragment window truncates"
        );
        for matched in &grep.matches {
            assert!(matched.fragment.as_str().len() <= 64 * 1024);
        }

        let oversized = service.dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Execute(ExecuteInput {
                program: BoundedText::new("echo").expect("program"),
                args: (0..129)
                    .map(|index| BoundedText::new(format!("arg-{index}")).expect("argument"))
                    .collect(),
            }),
            CancellationSignal::new(),
        );
        assert!(matches!(
            &oversized,
            Err(error) if error.code() == "invalid_tool_execute_arguments"
        ));
    }

    #[test]
    fn execute_input_serde_validates_argument_shapes() {
        let valid = serde_json::from_str::<ExecuteInput>(r#"{"program":"echo","args":["a","b"]}"#)
            .expect("valid execute input decodes");
        assert_eq!(valid.args.len(), 2);
        let too_many = serde_json::from_str::<ExecuteInput>(&format!(
            r#"{{"program":"echo","args":[{}]}}"#,
            (0..129)
                .map(|index| format!("\"arg-{index}\""))
                .collect::<Vec<_>>()
                .join(",")
        ))
        .expect_err("129 arguments fail validation at decode time");
        assert!(too_many.to_string().contains("invalid execute input"));
    }
}
