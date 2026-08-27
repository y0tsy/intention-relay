//! Typed, bounded contracts for workspace tools.

use intention_types::{DtoResult, ToolCallId, WorkspaceRelativePathDto};
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

const MAX_TOOL_OUTPUT_BYTES: usize = 64 * 1024;
const EXECUTE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_GLOB_MATCHES: usize = 10_000;
const MAX_GREP_MATCHES: usize = 10_000;

/// Typed cancellation signal for one tool invocation.
#[derive(Clone, Debug, Default)]
pub struct CancellationSignal(Arc<AtomicBool>);

impl CancellationSignal {
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
    #[must_use]
    pub fn cancelled() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    /// Requests cancellation of this invocation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
}

pub const TOOL_SCHEMA_VERSION: u16 = 1;
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
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolObservability {
    pub outcome: ToolOutcome,
    pub policy: ToolPolicy,
    pub elapsed_ms: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolContext {
    pub session_id: u128,
    pub run_id: u128,
    pub call_id: ToolCallId,
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

fn bounded_output_with_timeout(
    mut child: Child,
    cancellation: CancellationSignal,
    timeout: Duration,
) -> Result<BoundedOutput, &'static str> {
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
        match child.try_wait() {
            Ok(Some(status)) => {
                let (stdout, stdout_was_truncated) = join_reader(stdout)?;
                let (stderr, stderr_was_truncated) = join_reader(stderr)?;
                return Ok(BoundedOutput {
                    status,
                    stdout,
                    stderr,
                    stdout_truncated: stdout_was_truncated,
                    stderr_truncated: stderr_was_truncated,
                });
            }
            Ok(None) if cancellation.is_cancelled() => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = join_reader(stdout);
                let _ = join_reader(stderr);
                return Err("tool_execute_external_effect_unknown");
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = join_reader(stdout);
                let _ = join_reader(stderr);
                return Err("tool_execute_external_effect_unknown");
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = join_reader(stdout);
                let _ = join_reader(stderr);
                return Err("tool_execute_wait_failed");
            }
        }
    }
}

type ReaderHandle = thread::JoinHandle<Result<(Vec<u8>, bool), &'static str>>;

fn join_reader(reader: Option<ReaderHandle>) -> Result<(Vec<u8>, bool), &'static str> {
    reader
        .map(|reader| reader.join().map_err(|_| "tool_execute_read_failed")?)
        .transpose()
        .map(|value| value.unwrap_or_default())
}

fn read_bounded(
    reader: &mut impl std::io::Read,
    output: &mut Vec<u8>,
) -> Result<bool, &'static str> {
    let mut buffer = [0_u8; 4096];
    let mut total = 0;
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| "tool_execute_read_failed")?;
        if count == 0 {
            return Ok(false);
        }
        let remaining = MAX_TOOL_OUTPUT_BYTES.saturating_sub(total);
        output.extend_from_slice(&buffer[..count.min(remaining)]);
        total += count.min(remaining);
        if total == MAX_TOOL_OUTPUT_BYTES {
            return Ok(true);
        }
    }
}

/// The fixed set of tools exposed by the product boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolId {
    Read,
    Glob,
    Grep,
    Write,
    Edit,
    Execute,
}

impl ToolId {
    /// Returns the stable wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Glob => "glob",
            Self::Grep => "grep",
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
    description: &'static str,
    schema_version: u16,
    mutation: MutationKind,
    capabilities: &'static [ToolCapability],
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
}

/// The immutable built-in registry.
#[must_use]
pub const fn registry() -> [ToolDescriptor; 6] {
    [
        ToolDescriptor {
            id: ToolId::Read,
            description: "Read bounded text from a workspace file.",
            schema_version: TOOL_SCHEMA_VERSION,
            mutation: MutationKind::ReadOnly,
            capabilities: &[ToolCapability::Read],
        },
        ToolDescriptor {
            id: ToolId::Glob,
            description: "List workspace paths matching a pattern.",
            schema_version: TOOL_SCHEMA_VERSION,
            mutation: MutationKind::ReadOnly,
            capabilities: &[ToolCapability::Search],
        },
        ToolDescriptor {
            id: ToolId::Grep,
            description: "Search bounded workspace text.",
            schema_version: TOOL_SCHEMA_VERSION,
            mutation: MutationKind::ReadOnly,
            capabilities: &[ToolCapability::Search],
        },
        ToolDescriptor {
            id: ToolId::Write,
            description: "Write bounded text to a workspace file.",
            schema_version: TOOL_SCHEMA_VERSION,
            mutation: MutationKind::Mutating,
            capabilities: &[ToolCapability::Write],
        },
        ToolDescriptor {
            id: ToolId::Edit,
            description: "Apply a bounded text replacement.",
            schema_version: TOOL_SCHEMA_VERSION,
            mutation: MutationKind::Mutating,
            capabilities: &[ToolCapability::Edit],
        },
        ToolDescriptor {
            id: ToolId::Execute,
            description: "Execute an explicitly bounded command.",
            schema_version: TOOL_SCHEMA_VERSION,
            mutation: MutationKind::Process,
            capabilities: &[ToolCapability::Execute],
        },
    ]
}

/// Bounded text accepted by tool contracts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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
    pub path: Option<WorkspaceRelativePathDto>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WriteInput {
    pub path: WorkspaceRelativePathDto,
    pub content: BoundedText,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EditInput {
    pub path: WorkspaceRelativePathDto,
    pub old: BoundedText,
    pub new: BoundedText,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecuteInput {
    pub program: BoundedText,
    pub args: Vec<BoundedText>,
}

/// Typed tool result family.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", content = "value", rename_all = "snake_case")]
pub enum ToolResult {
    Read(TextResult),
    Glob(PathsResult),
    Grep(TextResult),
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
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextResult {
    pub text: BoundedText,
    pub truncated: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PathsResult {
    pub paths: Vec<WorkspaceRelativePathDto>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WriteResult {
    pub bytes: u64,
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
    /// Dispatches a typed call.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error when validation, workspace resolution, or execution fails.
    pub fn dispatch(&self, call: ToolCallId, input: ToolInput) -> DtoResult<ToolResult> {
        self.dispatch_with_cancellation(call, input, CancellationSignal::new())
    }

    /// Dispatches a typed tool call with cooperative cancellation.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error when cancellation, validation, workspace resolution, or execution fails.
    pub fn dispatch_with_cancellation(
        &self,
        _call: ToolCallId,
        input: ToolInput,
        cancellation: CancellationSignal,
    ) -> DtoResult<ToolResult> {
        if cancellation.is_cancelled() {
            return Err(intention_types::ErrorDto::validation(
                "tool_cancelled",
                "tool invocation was cancelled before execution",
            ));
        }
        match input {
            ToolInput::Read(i) => file::read(&self.root, i),
            ToolInput::Write(i) => file::write(&self.root, i),
            ToolInput::Edit(i) => file::edit(&self.root, i),
            ToolInput::Glob(i) => search::glob(&self.root, i),
            ToolInput::Grep(i) => search::grep(&self.root, i),
            ToolInput::Execute(i) => execute::run(&self.root, i, cancellation),
        }
    }

    /// Invokes exactly one explicitly admitted local tool call.
    ///
    /// This deliberately does not implement a model/tool loop.
    ///
    /// # Errors
    ///
    /// Returns the typed error produced while dispatching the tool call.
    pub fn invoke(&self, call: ToolCallId, input: ToolInput) -> DtoResult<ToolResult> {
        self.dispatch(call, input)
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
) -> DtoResult<ToolResult> {
    let mut command = Command::new(input.program.as_str());
    command.args(input.args.iter().map(BoundedText::as_str));
    command.current_dir(root.execute_cwd());
    command.envs(std::env::vars());
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = command.spawn().map_err(|_| {
        intention_types::ErrorDto::validation(
            "tool_execute_spawn_failed",
            "unable to spawn workspace command",
        )
    })?;
    let output = bounded_output(child, cancellation).map_err(|code| {
        intention_types::ErrorDto::validation(code, "workspace command execution failed")
    })?;
    let (stdout, _) = bounded_lossy(&output.stdout);
    let (stderr, _) = bounded_lossy(&output.stderr);
    let truncated = output.stdout_truncated || output.stderr_truncated;
    let text = format!(
        "stdout:\n{stdout}\nstderr:\n{stderr}\nexit_code:{}{}",
        output.status.code().unwrap_or(-1),
        if truncated { "\n[truncated]" } else { "" }
    );
    if !output.status.success() {
        return Err(intention_types::ErrorDto::validation(
            "tool_execute_nonzero",
            "workspace command exited unsuccessfully",
        ));
    }
    Ok(ToolResult::Execute(TextResult {
        text: BoundedText::new(text)?,
        truncated,
    }))
}

fn write_tool(root: &WorkspaceRoot, input: WriteInput) -> DtoResult<ToolResult> {
    let bytes = input.content.as_str().len() as u64;
    std::fs::write(
        root.resolve_new_file_path(&input.path)?,
        input.content.as_str(),
    )
    .map_err(|_| {
        intention_types::ErrorDto::validation("tool_write_failed", "unable to write workspace file")
    })?;
    Ok(ToolResult::Write(WriteResult { bytes }))
}

fn edit_tool(root: &WorkspaceRoot, input: EditInput) -> DtoResult<ToolResult> {
    let path = root.resolve_path(&input.path)?;
    let text = std::fs::read_to_string(&path).map_err(|_| {
        intention_types::ErrorDto::validation("tool_read_failed", "unable to read workspace file")
    })?;
    if !text.contains(input.old.as_str()) {
        return Err(intention_types::ErrorDto::validation(
            "edit_target_missing",
            "edit target was not found",
        ));
    }
    let replacement = text.replacen(input.old.as_str(), input.new.as_str(), 1);
    std::fs::write(path, &replacement).map_err(|_| {
        intention_types::ErrorDto::validation("tool_write_failed", "unable to write workspace file")
    })?;
    Ok(ToolResult::Edit(WriteResult {
        bytes: replacement.len() as u64,
    }))
}

fn glob_tool(root: &WorkspaceRoot, input: GlobInput) -> DtoResult<ToolResult> {
    let pattern = root
        .canonical_path()
        .join(input.pattern.as_str())
        .to_str()
        .ok_or_else(|| {
            intention_types::ErrorDto::validation("invalid_tool_pattern", "tool pattern is invalid")
        })?
        .to_owned();
    let mut paths = Vec::new();
    for entry in glob::glob(&pattern).map_err(|_| {
        intention_types::ErrorDto::validation("invalid_tool_pattern", "tool pattern is invalid")
    })? {
        let path = entry.map_err(|_| {
            intention_types::ErrorDto::validation("tool_search_failed", "workspace search failed")
        })?;
        if paths.len() >= MAX_GLOB_MATCHES {
            break;
        }
        if let Ok(relative) = path.strip_prefix(root.canonical_path())
            && let Ok(value) =
                WorkspaceRelativePathDto::parse(relative.to_string_lossy().replace('\\', "/"))
        {
            paths.push(value);
        }
    }
    paths.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    Ok(ToolResult::Glob(PathsResult { paths }))
}

fn grep_tool(root: &WorkspaceRoot, input: GrepInput) -> DtoResult<ToolResult> {
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
    let mut file = std::fs::File::open(path).map_err(|_| {
        intention_types::ErrorDto::validation("tool_search_failed", "workspace search failed")
    })?;
    let mut bytes = Vec::new();
    let source_truncated = read_bounded(&mut file, &mut bytes).map_err(|_| {
        intention_types::ErrorDto::validation("tool_search_failed", "workspace search failed")
    })?;
    let text = String::from_utf8_lossy(&bytes);
    let matches = text
        .lines()
        .filter(|line| line.contains(input.pattern.as_str()))
        .take(MAX_GREP_MATCHES)
        .collect::<Vec<_>>()
        .join("\n");
    let (matches, truncated) = bounded_lossy(matches.as_bytes());
    Ok(ToolResult::Grep(TextResult {
        text: bounded_text(matches)?,
        truncated: truncated || source_truncated,
    }))
}

/// Private execution boundary; adapters implement this without leaking erased values.
#[expect(
    dead_code,
    reason = "The private executor boundary is activated by the composition slice."
)]
trait ToolExecutor {
    fn execute(&self, call: ToolCallId, input: ToolInput) -> DtoResult<ToolResult>;
}
