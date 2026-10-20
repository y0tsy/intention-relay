//! Durable session fixtures shared by the terminal front end's unit and render
//! tests.
//!
//! Every builder here produces a value the wire carries: a run projection, a
//! session snapshot, a session summary, a committed transcript row, or a client
//! stream state over one of them. The front end's own tests live in two crates'
//! worth of modules - the application state's unit tests, the render suite, and
//! the transcript pane's layout tests - and each needs the same values, so the
//! builders sit here instead of being restated at each site.
//!
//! A builder that two sites needed with different facts takes the fact as a
//! parameter: the run's start second, the snapshot's active run, the tool
//! result's payload, and the transcript row a retained-window fixture fills
//! with. No builder invents a default a caller cannot override.

use std::sync::atomic::{AtomicI64, Ordering};

use intention_client::SessionStreamState;
use intention_proto::{
    BoundedText, ConfigRevisionId, ExecuteResult, GrepResult, MessageId, MessageKindDto,
    MessageProjectionDto, PathsResult, ProjectId, RunId, RunModeDto, RunProjectionDto,
    RunStatusDto, SessionId, SessionProjectionDto, SessionSnapshotDto, SessionSummaryDto,
    TextResult, TimestampDto, ToolCallId, ToolCallRowDto, ToolId, ToolIdentityDto, ToolInput,
    ToolProcessStatus, ToolResult, ToolResultRowDto, ToolResultStatusDto, ToolRowDto, TurnId,
    UsageDto, WorkspaceId, WorkspaceRootDto, WriteResult,
};

/// The canonical id of the first fixture session.
pub const FIRST_SESSION: &str = "11111111-1111-4111-8111-111111111111";

/// The canonical id of the second fixture session.
pub const SECOND_SESSION: &str = "22222222-2222-4222-8222-222222222222";

/// The wall-clock second a fixture that renders relative times is placed at.
///
/// A render suite measures "23 minutes ago" against the second it hands the
/// view, so the fixtures it renders have to sit on that same second.
pub const NOW: i64 = 1_759_000_000;

/// The wall-clock second a fixture run starts at when no caller says otherwise.
///
/// The transcript and application fixtures place their runs here: the elapsed
/// values they assert are measured from a fixed start, and no view of them
/// renders a relative time.
pub const RUN_STARTED_AT: i64 = 1_726_000_000;

/// Returns the fixture session id spelled by one canonical UUID literal.
///
/// # Panics
///
/// Panics when `value` is not a canonical session identity.
#[must_use]
pub fn fixture_session(value: &str) -> SessionId {
    SessionId::parse(value).expect("the fixture session id is canonical")
}

/// Returns one fresh durable row identity for a committed fixture row.
///
/// The counter is process-wide, so two fixtures in one test binary never
/// collide on an identity the wire treats as unique.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn row_id() -> MessageId {
    static NEXT: AtomicI64 = AtomicI64::new(0);
    MessageId::new(NEXT.fetch_add(1, Ordering::Relaxed) + 1)
        .expect("the fixture row identity is positive")
}

/// Returns the process temporary directory as an absolute workspace root.
///
/// # Panics
///
/// Panics when the process temporary directory is not an absolute path.
#[must_use]
pub fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy().into_owned())
        .expect("the process temporary directory is an absolute workspace root")
}

/// Returns one run projection of `status` starting at [`RUN_STARTED_AT`].
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn run(session_id: SessionId, run_id: RunId, status: RunStatusDto) -> RunProjectionDto {
    run_at(session_id, run_id, status, RUN_STARTED_AT)
}

/// Returns one run projection of `status` starting at `started_at`.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn run_at(
    session_id: SessionId,
    run_id: RunId,
    status: RunStatusDto,
    started_at: i64,
) -> RunProjectionDto {
    let started_at =
        TimestampDto::from_unix_seconds(started_at).expect("fixture timestamp is valid");
    let finished_at = intention_proto::run_status_is_terminal(status).then_some(started_at);
    run_with(session_id, run_id, status, None, started_at, finished_at, 0)
}

/// Returns one run projection of `status` starting at [`RUN_STARTED_AT`]
/// carrying the committed usage and open calls a test needs.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn run_in(
    session_id: SessionId,
    run_id: RunId,
    status: RunStatusDto,
    usage: Option<UsageDto>,
    open_tool_calls: u32,
) -> RunProjectionDto {
    let started_at =
        TimestampDto::from_unix_seconds(RUN_STARTED_AT).expect("fixture timestamp is valid");
    let finished_at = intention_proto::run_status_is_terminal(status).then_some(started_at);
    run_with(
        session_id,
        run_id,
        status,
        usage,
        started_at,
        finished_at,
        open_tool_calls,
    )
}

/// Returns one run projection carrying every fact a view reads.
///
/// `finished_at` is the caller's: a run that completed after an interval
/// carries the second it finished at, which is not its start second.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn run_with(
    session_id: SessionId,
    run_id: RunId,
    status: RunStatusDto,
    usage: Option<UsageDto>,
    started_at: TimestampDto,
    finished_at: Option<TimestampDto>,
    open_tool_calls: u32,
) -> RunProjectionDto {
    RunProjectionDto::new(
        session_id,
        run_id,
        TurnId::new(),
        status,
        ConfigRevisionId::new(),
        usage,
        started_at,
        finished_at,
        open_tool_calls,
    )
    .expect("fixture run projection is valid")
}

/// Returns one session projection in the fixture's run mode.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn session_projection(
    session_id: SessionId,
    active_run: Option<RunProjectionDto>,
) -> SessionProjectionDto {
    session_projection_in_mode(session_id, RunModeDto::Build, active_run)
}

/// Returns one session projection declaring `mode`.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn session_projection_in_mode(
    session_id: SessionId,
    mode: RunModeDto,
    active_run: Option<RunProjectionDto>,
) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        mode,
        None,
        active_run,
    )
    .expect("the fixture session projection is coherent")
}

/// Returns one session snapshot in the fixture's run mode.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn snapshot(
    session_id: SessionId,
    active_run: Option<RunProjectionDto>,
    messages: Vec<MessageProjectionDto>,
) -> SessionSnapshotDto {
    snapshot_in_mode(session_id, RunModeDto::Build, active_run, messages)
}

/// Returns one session snapshot whose projection declares `mode`.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn snapshot_in_mode(
    session_id: SessionId,
    mode: RunModeDto,
    active_run: Option<RunProjectionDto>,
    messages: Vec<MessageProjectionDto>,
) -> SessionSnapshotDto {
    snapshot_with_model(session_id, mode, None, active_run, messages)
}

/// Returns one session snapshot whose projection declares `mode` and, when
/// given, the effective provider model.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn snapshot_with_model(
    session_id: SessionId,
    mode: RunModeDto,
    model: Option<&str>,
    active_run: Option<RunProjectionDto>,
    messages: Vec<MessageProjectionDto>,
) -> SessionSnapshotDto {
    let mut projection = session_projection_in_mode(session_id, mode, active_run);
    if let Some(model) = model {
        projection = projection
            .with_provider_model(model)
            .expect("the fixture provider model is valid");
    }
    SessionSnapshotDto::with_projection(session_id, projection, messages)
        .expect("the fixture session snapshot is coherent")
}

/// Returns one session summary last updated at `updated_at`.
///
/// A summary carries no message count of its own beyond the one the caller
/// states, so this is the form a list fixture uses.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn summary(session_id: SessionId, updated_at: i64) -> SessionSummaryDto {
    summary_in_root(session_id, updated_at, workspace_root())
}

/// Returns one session summary with every durable field the table renders.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn summary_of(
    session_id: SessionId,
    created_at: i64,
    updated_at: i64,
    message_count: u64,
) -> SessionSummaryDto {
    let created_at =
        TimestampDto::from_unix_seconds(created_at).expect("the fixture time is valid");
    let updated_at =
        TimestampDto::from_unix_seconds(updated_at).expect("the fixture time is valid");
    SessionSummaryDto::new(
        session_id,
        ProjectId::new(),
        WorkspaceId::new(),
        RunModeDto::Build,
        created_at,
        updated_at,
        workspace_root(),
        message_count,
    )
}

/// Returns one session summary durably bound to `root`.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn summary_in_root(
    session_id: SessionId,
    updated_at: i64,
    root: WorkspaceRootDto,
) -> SessionSummaryDto {
    let updated_at = TimestampDto::from_unix_seconds(updated_at).expect("fixture time is valid");
    SessionSummaryDto::new(
        session_id,
        ProjectId::new(),
        WorkspaceId::new(),
        RunModeDto::Build,
        updated_at,
        updated_at,
        root,
        0,
    )
}

/// Returns one client stream state over `messages` with a run in `status`.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn stream_state(
    session_id: SessionId,
    run_id: RunId,
    status: RunStatusDto,
    messages: Vec<MessageProjectionDto>,
) -> SessionStreamState {
    SessionStreamState::from_snapshot(snapshot(
        session_id,
        Some(run(session_id, run_id, status)),
        messages,
    ))
}

/// Returns one client state holding exactly the rows it retains.
///
/// `row` builds the committed row of one read index. Rows are read into the
/// state until its retained count stops growing, so the fixture measures the
/// client's own window through the client instead of restating its bound.
///
/// # Panics
///
/// Panics when the client rejects a read that belongs to the state's own session.
#[must_use]
pub fn full_transcript(
    session_id: SessionId,
    run_id: RunId,
    row: impl Fn(usize) -> MessageProjectionDto,
) -> SessionStreamState {
    let mut state = SessionStreamState::new(session_id);
    let mut retained = 0;
    loop {
        let read = SessionStreamState::from_snapshot(snapshot(
            session_id,
            Some(run(session_id, run_id, RunStatusDto::Running)),
            vec![row(retained)],
        ));
        state
            .reconcile(read)
            .expect("the fixture read belongs to the state's own session");
        if state.messages().len() == retained {
            return state;
        }
        retained = state.messages().len();
    }
}

/// Returns one committed user row carrying `text`.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn user_row(session_id: SessionId, run_id: Option<RunId>, text: &str) -> MessageProjectionDto {
    row(session_id, run_id, MessageKindDto::User, text)
}

/// Returns one committed assistant row carrying `text`.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn assistant_row(session_id: SessionId, run_id: RunId, text: &str) -> MessageProjectionDto {
    row(session_id, Some(run_id), MessageKindDto::Assistant, text)
}

/// Returns one committed assistant row carrying `reasoning` above its text.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn reasoning_row(
    session_id: SessionId,
    run_id: RunId,
    text: &str,
    reasoning: &str,
) -> MessageProjectionDto {
    MessageProjectionDto::new(
        row_id(),
        session_id,
        Some(run_id),
        MessageKindDto::Assistant,
        text,
        Some(reasoning.to_owned()),
        None,
    )
    .expect("the fixture reasoning row is coherent")
}

/// Returns one committed row of `kind` carrying `text` and no attachment.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn row(
    session_id: SessionId,
    run_id: Option<RunId>,
    kind: MessageKindDto,
    text: &str,
) -> MessageProjectionDto {
    MessageProjectionDto::new(row_id(), session_id, run_id, kind, text, None, None)
        .expect("the fixture transcript row is coherent")
}

/// Returns one parsed fixture tool identity.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn identity(tool_id: &str) -> ToolIdentityDto {
    ToolIdentityDto::parse(tool_id).expect("the fixture tool identity is well-formed")
}

/// Returns one bounded fixture text.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn bounded(text: &str) -> BoundedText {
    BoundedText::new(text).expect("the fixture text is bounded")
}

/// Returns one typed result payload of the declared `tool_id` fixture.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn payload(tool_id: ToolId, content: &str) -> ToolResult {
    match tool_id {
        ToolId::Read => ToolResult::Read(TextResult {
            text: bounded(content),
            truncated: false,
        }),
        ToolId::Glob => ToolResult::Glob(PathsResult {
            paths: Vec::new(),
            truncated: false,
        }),
        ToolId::Grep => ToolResult::Grep(GrepResult {
            matches: Vec::new(),
            truncated: false,
        }),
        ToolId::Write => ToolResult::Write(WriteResult { bytes: 0 }),
        ToolId::Edit => ToolResult::Edit(WriteResult { bytes: 0 }),
        ToolId::Execute => ToolResult::Execute(ExecuteResult {
            text: TextResult {
                text: bounded(content),
                truncated: false,
            },
            process: Some(ToolProcessStatus::Success),
        }),
    }
}

/// Returns one committed tool-call row carrying `arguments` for `call_id`.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn tool_call_row(
    session_id: SessionId,
    run_id: RunId,
    tool_id: &str,
    arguments: &str,
    call_id: ToolCallId,
) -> MessageProjectionDto {
    let tool = ToolRowDto::Call(
        ToolCallRowDto::new(
            call_id,
            identity(tool_id),
            ToolInput::from_arguments_json(tool_id, arguments).ok(),
        )
        .expect("the fixture call row is coherent"),
    );
    MessageProjectionDto::new(
        row_id(),
        session_id,
        Some(run_id),
        MessageKindDto::ToolCall,
        arguments,
        None,
        Some(tool),
    )
    .expect("the fixture tool-call row is coherent")
}

/// Returns one committed completed tool-result row carrying `content` for
/// `call_id`, with the payload its own tool identity declares.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn tool_result_row(
    session_id: SessionId,
    run_id: RunId,
    tool_id: &str,
    content: &str,
    call_id: ToolCallId,
) -> MessageProjectionDto {
    let id = ToolId::parse(tool_id).expect("the fixture tool is one of the declared tools");
    tool_result_row_with(
        session_id,
        run_id,
        tool_id,
        content,
        payload(id, content),
        call_id,
    )
}

/// Returns one committed completed tool-result row carrying `text` and the
/// payload a caller supplies.
///
/// # Panics
///
/// Panics when a fixture value fails the wire's own validation.
#[must_use]
pub fn tool_result_row_with(
    session_id: SessionId,
    run_id: RunId,
    tool_id: &str,
    text: &str,
    result: ToolResult,
    call_id: ToolCallId,
) -> MessageProjectionDto {
    let tool = ToolRowDto::Result(
        ToolResultRowDto::new(
            call_id,
            identity(tool_id),
            ToolResultStatusDto::Completed,
            Some(result),
            Vec::new(),
        )
        .expect("the fixture result row is coherent"),
    );
    MessageProjectionDto::new(
        row_id(),
        session_id,
        Some(run_id),
        MessageKindDto::ToolResult,
        text,
        None,
        Some(tool),
    )
    .expect("the fixture tool-result row is coherent")
}
