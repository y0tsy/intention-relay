//! Shared fixtures for the engine integration suites.
//!
//! Each integration target compiles this module and uses a different subset of
//! the fixtures, so unused items are expected here.

#![allow(
    dead_code,
    reason = "every integration target compiles the shared module and uses a different subset"
)]

use std::collections::VecDeque;
use std::future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::task::Poll;
use std::time::Duration;

use intention_config::ConfigSnapshotDto;
use intention_engine::{
    ModelSleepFuture, ModelTimePort, RunCancellation, SessionCommitDto, SessionCommitObserver,
    ToolExecutionPort, ToolResultOutcomeDto,
};
use intention_proto::{
    CreateSessionCommandDto, DtoResult, ErrorDto, FinishReasonDto, IdempotencyKey, MessageId,
    MessageProjectionDto, NewMessageDto, PendingTurnProjectionDto, ProjectId, RemoveTurnCommandDto,
    RunId, RunModeDto, RunProjectionDto, RunStatusDto, SessionId, SessionProjectionDto,
    SessionSummariesDto, TimestampDto, ToolCallId, ToolResultMetadataEntryDto, ToolResultStatusDto,
    ToolRowDto, TurnId, UsageDto, WorkspaceBindingDto, WorkspaceId, WorkspaceRootDto,
    run_status_is_terminal,
};
use intention_providers::ToolCallDto;
use intention_storage::{
    AcceptedTurnOutcomeDto, RunOutcomeDto, StartingRunModelContextDto, StorageRepositoryDto,
};
use intention_test_support::fixture_snapshot;

/// Returns the exact fixture timestamp for one Unix second value.
pub fn time(value: i64) -> TimestampDto {
    TimestampDto::from_unix_seconds(value).expect("fixture timestamp is valid")
}

/// Returns one fresh durable row identity for a committed fixture row.
pub fn row_id() -> MessageId {
    static NEXT: AtomicI64 = AtomicI64::new(0);
    MessageId::new(NEXT.fetch_add(1, Ordering::Relaxed) + 1).expect("fixture row identity is valid")
}

/// Returns native absolute workspace root for the engine fixtures.
pub fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-engine-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("native fixture workspace is valid")
}

/// One committed tool-result row recorded by the fixture from its message.
///
/// A result is stored the way the transcript stores it: the row's committed
/// content is its message text, and its typed status, payload, and metadata
/// are the row's own projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedToolResult {
    session_id: SessionId,
    run_id: RunId,
    call_id: ToolCallId,
    tool_id: String,
    status: ToolResultStatusDto,
    content: String,
    metadata: Vec<ToolResultMetadataEntryDto>,
    occurred_at: TimestampDto,
}

impl RecordedToolResult {
    /// Returns the owning durable session.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }
    /// Returns the run that executed the tool call.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }
    /// Returns the identified tool invocation.
    #[must_use]
    pub const fn call_id(&self) -> ToolCallId {
        self.call_id
    }
    /// Returns the wire tool identity that produced the result.
    #[must_use]
    pub fn tool_id(&self) -> &str {
        &self.tool_id
    }
    /// Returns the terminal outcome recorded for the result.
    #[must_use]
    pub const fn status(&self) -> ToolResultStatusDto {
        self.status
    }
    /// Returns the committed model-visible result content.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
    /// Returns the approved structured metadata entries.
    #[must_use]
    pub fn metadata(&self) -> &[ToolResultMetadataEntryDto] {
        &self.metadata
    }
    /// Returns the commit event time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// One recorded call to [`FakeRepository::finish_run`].
#[derive(Clone)]
pub struct RecordedFinish {
    session_id: SessionId,
    run_id: RunId,
    outcome: RunOutcomeDto,
    occurred_at: TimestampDto,
}

impl RecordedFinish {
    /// Returns the owning session.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }
    /// Returns the finished run.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }
    /// Returns the terminal status.
    #[must_use]
    pub const fn status(&self) -> RunStatusDto {
        self.outcome.status()
    }
    /// Returns the reported provider usage, when one was reported.
    #[must_use]
    pub const fn usage(&self) -> Option<&UsageDto> {
        self.outcome.usage()
    }
    /// Returns the provider finish reason, when one was reported.
    #[must_use]
    pub const fn finish_reason(&self) -> Option<FinishReasonDto> {
        self.outcome.finish_reason()
    }
    /// Returns the safe error code of a failed run, when it failed.
    #[must_use]
    pub fn error_code(&self) -> Option<&str> {
        self.outcome.error_code()
    }
    /// Returns the safe error message of a failed run, when it failed.
    #[must_use]
    pub fn error_message(&self) -> Option<&str> {
        self.outcome.error_message()
    }
    /// Returns the selected completion time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// One recorded call to [`FakeRepository::transition_run`].
pub struct RecordedTransition {
    status: RunStatusDto,
}

impl RecordedTransition {
    /// Returns the requested successor status.
    #[must_use]
    pub const fn status(&self) -> RunStatusDto {
        self.status
    }
}

/// One recorded call to [`FakeRepository::accept_user_turn`].
#[derive(Clone)]
pub struct RecordedTurn {
    proposed_run_id: RunId,
    config_snapshot: ConfigSnapshotDto,
    occurred_at: TimestampDto,
}

impl RecordedTurn {
    /// Returns the run identity selected for a possible start.
    #[must_use]
    pub const fn proposed_run_id(&self) -> RunId {
        self.proposed_run_id
    }
    /// Returns the credential-free immutable configuration selection.
    #[must_use]
    pub const fn config_snapshot(&self) -> &ConfigSnapshotDto {
        &self.config_snapshot
    }
    /// Returns the externally selected acceptance time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Records every committed transcript row, terminal outcome, and transition.
///
/// One merged repository serves the runtime, execution, and tool-loop suites:
/// the canonical run identity and its recorded commits replace every earlier
/// copy, while every per-suite failure injection stays an explicit field.
pub struct FakeRepository {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub config: ConfigSnapshotDto,
    pub status: Mutex<RunStatusDto>,
    pub messages: Mutex<Vec<MessageProjectionDto>>,
    pub finishes: Mutex<Vec<RecordedFinish>>,
    pub transitions: Mutex<Vec<RecordedTransition>>,
    pub tool_results: Mutex<Vec<RecordedToolResult>>,
    /// Counts committed tool-result rows for `Send + Sync` observers that must
    /// not borrow this fixture; mirrors `tool_results`.
    pub committed_result_rows: AtomicUsize,
    pub created: Mutex<DtoResult<SessionProjectionDto>>,
    /// The durable identity binding this fixture reports for every root.
    pub workspace_binding: Mutex<Option<WorkspaceBindingDto>>,
    /// Every creation command the facade forwarded, in order.
    pub create_commands: Mutex<Vec<CreateSessionCommandDto>>,
    /// The repository calls this fixture observed, in order.
    pub calls: Mutex<Vec<&'static str>>,
    pub accepted: Mutex<DtoResult<AcceptedTurnOutcomeDto>>,
    pub accepted_inputs: Mutex<Vec<RecordedTurn>>,
    pub removed: Mutex<Option<PendingTurnProjectionDto>>,
    pub loaded_projection: Mutex<Option<SessionProjectionDto>>,
    pub starting_context: Mutex<Option<StartingRunModelContextDto>>,
    pub run: Mutex<Option<RunProjectionDto>>,
    pub commit_calls: Mutex<usize>,
    pub commit_failures: Mutex<Vec<usize>>,
    pub commit_error: Mutex<Option<ErrorDto>>,
    pub append_failure: Mutex<Option<ErrorDto>>,
    pub append_failure_at: Mutex<Option<(usize, ErrorDto)>>,
    pub cancel_after_append: Mutex<Option<(usize, RunCancellation)>>,
    pub append_count: Mutex<usize>,
    /// The newest durable row identity this fixture assigned.
    pub next_message_id: Mutex<i64>,
    pub config_error: Mutex<Option<ErrorDto>>,
    /// Pending user messages committed by the next context boundary.
    pub pending: Mutex<VecDeque<MessageProjectionDto>>,
    pub pending_consumes: Mutex<usize>,
}

impl FakeRepository {
    /// Creates the fixture for one exact run identity in its `Starting` state.
    #[must_use]
    pub fn new(session_id: SessionId, run_id: RunId, config: ConfigSnapshotDto) -> Self {
        Self {
            session_id,
            run_id,
            turn_id: TurnId::new(),
            config,
            status: Mutex::new(RunStatusDto::Starting),
            messages: Mutex::new(Vec::new()),
            finishes: Mutex::new(Vec::new()),
            transitions: Mutex::new(Vec::new()),
            tool_results: Mutex::new(Vec::new()),
            committed_result_rows: AtomicUsize::new(0),
            created: Mutex::new(Err(ErrorDto::unavailable("fixture_unused", "unused"))),
            workspace_binding: Mutex::new(None),
            create_commands: Mutex::new(Vec::new()),
            calls: Mutex::new(Vec::new()),
            accepted: Mutex::new(Err(ErrorDto::unavailable("fixture_unused", "unused"))),
            accepted_inputs: Mutex::new(Vec::new()),
            removed: Mutex::new(None),
            loaded_projection: Mutex::new(None),
            starting_context: Mutex::new(None),
            run: Mutex::new(None),
            commit_calls: Mutex::new(0),
            commit_failures: Mutex::new(Vec::new()),
            commit_error: Mutex::new(None),
            append_failure: Mutex::new(None),
            append_failure_at: Mutex::new(None),
            cancel_after_append: Mutex::new(None),
            append_count: Mutex::new(0),
            next_message_id: Mutex::new(0),
            config_error: Mutex::new(None),
            pending: Mutex::new(VecDeque::new()),
            pending_consumes: Mutex::new(0),
        }
    }

    /// Creates a fixture run in the supplied status under a fresh identity.
    #[must_use]
    pub fn with_status(session_id: SessionId, status: RunStatusDto) -> Self {
        let repository = Self::new(session_id, RunId::new(), fixture_snapshot());
        *repository
            .status
            .lock()
            .expect("the fixture mutex is not poisoned") = status;
        repository
    }

    /// Creates the admit-turn fixture with a fresh identity.
    #[must_use]
    pub fn with_accepted(accepted: DtoResult<AcceptedTurnOutcomeDto>) -> Self {
        let repository = Self::new(SessionId::new(), RunId::new(), fixture_snapshot());
        *repository
            .accepted
            .lock()
            .expect("the fixture mutex is not poisoned") = accepted;
        repository
    }

    /// Returns the run identity this fixture serves.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    /// Returns the current run projection, preferring an explicitly stored run.
    #[must_use]
    pub fn projection(&self) -> RunProjectionDto {
        let stored = *self.run.lock().expect("the fixture mutex is not poisoned");
        if let Some(run) = stored {
            return run;
        }
        let started_at = self.config.captured_at();
        let status = *self
            .status
            .lock()
            .expect("the fixture mutex is not poisoned");
        self.fixture_run(
            status,
            run_status_is_terminal(status).then_some(started_at),
            0,
        )
    }

    /// Returns one coherent fixture run projection in `status`.
    fn fixture_run(
        &self,
        status: RunStatusDto,
        finished_at: Option<TimestampDto>,
        open_tool_calls: u32,
    ) -> RunProjectionDto {
        RunProjectionDto::new(
            self.session_id,
            self.run_id,
            self.turn_id,
            status,
            self.config.revision_id(),
            None,
            self.config.captured_at(),
            finished_at,
            open_tool_calls,
        )
        .expect("fixture run projection is coherent")
    }

    /// Returns every committed transcript row.
    #[must_use]
    pub fn committed_messages(&self) -> Vec<MessageProjectionDto> {
        self.messages
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone()
    }

    /// Returns every committed tool-result row.
    #[must_use]
    pub fn committed_results(&self) -> Vec<RecordedToolResult> {
        self.tool_results
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone()
    }

    /// Returns how many committed result rows completed.
    #[must_use]
    pub fn completed_result_count(&self) -> usize {
        self.committed_results()
            .iter()
            .filter(|row| row.status() == ToolResultStatusDto::Completed)
            .count()
    }

    /// Returns the next one-based append index.
    fn next_append_index(&self) -> usize {
        let mut count = self
            .append_count
            .lock()
            .expect("the fixture mutex is not poisoned");
        *count += 1;
        *count
    }

    /// Returns the next durable row identity this fixture assigns.
    fn assign_message_id(&self) -> MessageId {
        let mut next = self
            .next_message_id
            .lock()
            .expect("the fixture mutex is not poisoned");
        *next += 1;
        MessageId::new(*next).expect("fixture row identity is positive")
    }

    /// Returns one row to commit as the committed row carrying `message_id`.
    fn committed_row(
        &self,
        message_id: MessageId,
        message: &NewMessageDto,
    ) -> DtoResult<MessageProjectionDto> {
        MessageProjectionDto::new(
            message_id,
            message.session_id(),
            message.run_id(),
            message.kind(),
            message.text(),
            message.reasoning().map(str::to_owned),
            message.tool().cloned(),
        )
    }

    /// Returns the next one-based commit ordinal, or the selected injected failure.
    fn next_commit(&self) -> DtoResult<usize> {
        let injected = self
            .commit_error
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone();
        if let Some(error) = injected {
            return Err(error);
        }
        let ordinal = {
            let mut calls = self
                .commit_calls
                .lock()
                .expect("the fixture mutex is not poisoned");
            *calls += 1;
            *calls
        };
        if self
            .commit_failures
            .lock()
            .expect("the fixture mutex is not poisoned")
            .contains(&ordinal)
        {
            return Err(ErrorDto::unavailable(
                "append_unavailable",
                "append refused at the selected call",
            ));
        }
        Ok(ordinal)
    }

    /// Stores one durable status transition and keeps an explicit run current.
    fn store_status(&self, status: RunStatusDto, occurred_at: TimestampDto) {
        *self
            .status
            .lock()
            .expect("the fixture mutex is not poisoned") = status;
        let refreshed = self
            .run
            .lock()
            .expect("the fixture mutex is not poisoned")
            .as_ref()
            .map(|run| {
                RunProjectionDto::new(
                    run.session_id(),
                    run.run_id(),
                    run.turn_id(),
                    status,
                    run.config_revision_id(),
                    run.usage(),
                    run.started_at(),
                    run_status_is_terminal(status).then_some(occurred_at),
                    run.open_tool_calls(),
                )
                .expect("fixture run projection is coherent")
            });
        if let Some(refreshed) = refreshed {
            *self.run.lock().expect("the fixture mutex is not poisoned") = Some(refreshed);
        }
    }
}

impl StorageRepositoryDto for FakeRepository {
    fn create_session(
        &self,
        command: CreateSessionCommandDto,
        _occurred_at: TimestampDto,
    ) -> DtoResult<SessionProjectionDto> {
        self.calls
            .lock()
            .expect("the fixture mutex is not poisoned")
            .push("create_session");
        self.create_commands
            .lock()
            .expect("the fixture mutex is not poisoned")
            .push(command);
        self.created
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone()
    }

    fn workspace_binding(
        &self,
        _root: &WorkspaceRootDto,
    ) -> DtoResult<Option<WorkspaceBindingDto>> {
        Ok(*self
            .workspace_binding
            .lock()
            .expect("the fixture mutex is not poisoned"))
    }

    fn accept_user_turn(
        &self,
        _session_id: SessionId,
        _idempotency_key: IdempotencyKey,
        _content: &str,
        proposed_run_id: RunId,
        config_snapshot: ConfigSnapshotDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<AcceptedTurnOutcomeDto> {
        self.calls
            .lock()
            .expect("the fixture mutex is not poisoned")
            .push("accept_user_turn");
        self.accepted_inputs
            .lock()
            .expect("the fixture mutex is not poisoned")
            .push(RecordedTurn {
                proposed_run_id,
                config_snapshot,
                occurred_at,
            });
        self.accepted
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone()
    }

    fn remove_turn(
        &self,
        _command: RemoveTurnCommandDto,
        _occurred_at: TimestampDto,
    ) -> DtoResult<PendingTurnProjectionDto> {
        self.removed
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone()
            .ok_or_else(|| {
                ErrorDto::unavailable("fixture_missing_result", "fixture result missing")
            })
    }

    fn consume_pending_user_turns(
        &self,
        session_id: SessionId,
        run_id: RunId,
        _occurred_at: TimestampDto,
    ) -> DtoResult<Vec<MessageProjectionDto>> {
        assert_eq!(session_id, self.session_id);
        assert_eq!(run_id, self.run_id);
        *self
            .pending_consumes
            .lock()
            .expect("the fixture mutex is not poisoned") += 1;
        Ok(self
            .pending
            .lock()
            .expect("the fixture mutex is not poisoned")
            .drain(..)
            .collect())
    }

    fn transition_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
        status: RunStatusDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<RunProjectionDto> {
        assert_eq!(session_id, self.session_id);
        assert_eq!(run_id, self.run_id);
        self.store_status(status, occurred_at);
        self.transitions
            .lock()
            .expect("the fixture mutex is not poisoned")
            .push(RecordedTransition { status });
        Ok(self.projection())
    }

    fn finish_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
        outcome: RunOutcomeDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<RunProjectionDto> {
        let status = outcome.status();
        self.finishes
            .lock()
            .expect("the fixture mutex is not poisoned")
            .push(RecordedFinish {
                session_id,
                run_id,
                outcome,
                occurred_at,
            });
        self.store_status(status, occurred_at);
        Ok(self.projection())
    }

    fn append_message(
        &self,
        message: NewMessageDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<MessageProjectionDto> {
        self.next_commit()?;
        let index = self.next_append_index();
        let injected = self
            .append_failure
            .lock()
            .expect("the fixture mutex is not poisoned")
            .take();
        if let Some(error) = injected {
            return Err(error);
        }
        if self
            .append_failure_at
            .lock()
            .expect("the fixture mutex is not poisoned")
            .as_ref()
            .is_some_and(|(at, _)| *at == index)
        {
            return Err(self
                .append_failure_at
                .lock()
                .expect("the fixture mutex is not poisoned")
                .take()
                .expect("configured append failure exists")
                .1);
        }
        let scheduled = self
            .cancel_after_append
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone();
        if let Some((cancel_at, signal)) = scheduled
            && cancel_at == index
        {
            signal.cancel();
        }
        let committed = self.committed_row(self.assign_message_id(), &message)?;
        self.messages
            .lock()
            .expect("the fixture mutex is not poisoned")
            .push(committed.clone());
        // A result row is its own durable home: the fixture records it from
        // the committed message it just appended.
        if let Some(ToolRowDto::Result(row)) = message.tool() {
            self.tool_results
                .lock()
                .expect("the fixture mutex is not poisoned")
                .push(RecordedToolResult {
                    session_id: message.session_id(),
                    run_id: message
                        .run_id()
                        .expect("a fixture tool-result row is run-bound"),
                    call_id: row.call_id(),
                    tool_id: row.tool_id().wire_name().to_owned(),
                    status: row.status(),
                    content: message.text().to_owned(),
                    metadata: row.metadata().to_vec(),
                    occurred_at,
                });
            self.committed_result_rows.fetch_add(1, Ordering::SeqCst);
        }
        Ok(committed)
    }

    fn load_run_config_snapshot(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<ConfigSnapshotDto> {
        assert_eq!((session_id, run_id), (self.session_id, self.run_id));
        let injected = self
            .config_error
            .lock()
            .expect("the fixture mutex is not poisoned")
            .take();
        if let Some(error) = injected {
            return Err(error);
        }
        Ok(self.config.clone())
    }

    fn load_starting_run_model_context(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<StartingRunModelContextDto> {
        self.starting_context
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone()
            .ok_or_else(|| {
                ErrorDto::unavailable(
                    "run_model_context_unavailable",
                    "the durable run model context is unavailable",
                )
            })
    }

    fn load_run_projection(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunProjectionDto> {
        let expected = self
            .run
            .lock()
            .expect("the fixture mutex is not poisoned")
            .as_ref()
            .map_or((self.session_id, self.run_id), |run| {
                (run.session_id(), run.run_id())
            });
        if (session_id, run_id) != expected {
            return Err(ErrorDto::validation(
                "run_not_found",
                "the requested durable run does not exist",
            ));
        }
        Ok(self.projection())
    }

    fn load_session_projection(&self, session_id: SessionId) -> DtoResult<SessionProjectionDto> {
        let loaded = self
            .loaded_projection
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone();
        if let Some(projection) = loaded {
            return Ok(projection);
        }
        SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            workspace_root(),
            RunModeDto::Build,
            Some(self.config.revision_id()),
            Some(self.projection()),
        )
    }

    fn load_recent_messages(
        &self,
        _session_id: SessionId,
        _limit: u32,
    ) -> DtoResult<Vec<MessageProjectionDto>> {
        // Suites seed the durable context explicitly and never history.
        Ok(Vec::new())
    }

    fn list_sessions(&self, _limit: u32) -> DtoResult<SessionSummariesDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "session listing is not used by this fixture",
        ))
    }

    fn load_run_messages(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
        _limit: u32,
    ) -> DtoResult<Vec<MessageProjectionDto>> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "run message loads are not used by this fixture",
        ))
    }

    fn recover_unfinished_runs(
        &self,
        _recovered_at: TimestampDto,
    ) -> DtoResult<Vec<RunProjectionDto>> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "recovery is not used by this fixture",
        ))
    }

    fn accept_configuration_revision(&self, _snapshot: ConfigSnapshotDto) -> DtoResult<()> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "configuration acceptance is not used by this fixture",
        ))
    }
}

/// A time port that answers immediately and records every requested delay.
pub struct ImmediateTime {
    pub sleeps: Mutex<Vec<Duration>>,
}

impl ImmediateTime {
    /// Creates an empty immediate clock.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sleeps: Mutex::new(Vec::new()),
        }
    }
}

impl ModelTimePort for ImmediateTime {
    fn now(&self) -> TimestampDto {
        time(2)
    }

    fn sleep(&self, duration: Duration) -> ModelSleepFuture<'_> {
        self.sleeps
            .lock()
            .expect("the fixture mutex is not poisoned")
            .push(duration);
        Box::pin(future::ready(()))
    }
}

/// A manual clock whose elapsed time advances only when a scripted provider
/// stream reports a silence.
///
/// The seconds-scale attempt window resolves only against the silences the
/// fixture drives, so a test can model a slow provider without waiting. A
/// sub-second delay is the runtime's own retry delay, which no provider
/// silence drives: it completes immediately and advances the clock by its own
/// duration, exactly like the m5 fixtures that classify delays by their scale.
pub struct ManualTime {
    elapsed: Mutex<Duration>,
    sleeps: Mutex<Vec<Duration>>,
}

impl ManualTime {
    /// Creates a manual clock at zero elapsed time.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            elapsed: Mutex::new(Duration::ZERO),
            sleeps: Mutex::new(Vec::new()),
        }
    }

    /// Advances the clock by one provider silence.
    pub fn advance(&self, silence: Duration) {
        let mut elapsed = self
            .elapsed
            .lock()
            .expect("the fixture mutex is not poisoned");
        *elapsed += silence;
    }

    /// Returns the elapsed virtual time.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        *self
            .elapsed
            .lock()
            .expect("the fixture mutex is not poisoned")
    }

    /// Returns every armed delay in arming order.
    #[must_use]
    pub fn sleeps(&self) -> Vec<Duration> {
        self.sleeps
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone()
    }
}

impl ModelTimePort for ManualTime {
    fn now(&self) -> TimestampDto {
        time(2)
    }

    fn sleep(&self, duration: Duration) -> ModelSleepFuture<'_> {
        self.sleeps
            .lock()
            .expect("the fixture mutex is not poisoned")
            .push(duration);
        if duration < Duration::from_secs(1) {
            self.advance(duration);
            return Box::pin(future::ready(()));
        }
        let deadline = self.elapsed() + duration;
        Box::pin(future::poll_fn(move |_| {
            if self.elapsed() >= deadline {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }))
    }
}

/// Records every committed transcript row and run status.
pub struct RecordingCommitObserver {
    commits: Mutex<Vec<SessionCommitDto>>,
}

impl RecordingCommitObserver {
    /// Creates an empty recorder.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            commits: Mutex::new(Vec::new()),
        }
    }

    /// Returns every observed commit in publication order.
    #[must_use]
    pub fn commits(&self) -> Vec<SessionCommitDto> {
        self.commits
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone()
    }
}

impl SessionCommitObserver for RecordingCommitObserver {
    fn observe_session_commit(&self, committed: &SessionCommitDto) {
        self.commits
            .lock()
            .expect("the fixture mutex is not poisoned")
            .push(committed.clone());
    }
}

/// Executes scripted tool outcomes and records every port invocation.
pub struct ScriptedPort {
    pub calls: Mutex<Vec<(SessionId, RunId, ToolCallDto)>>,
    outcomes: Mutex<VecDeque<DtoResult<ToolResultOutcomeDto>>>,
}

impl ScriptedPort {
    /// Creates a port answering each invocation from the scripted outcome queue.
    #[must_use]
    pub fn new(outcomes: Vec<DtoResult<ToolResultOutcomeDto>>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            outcomes: Mutex::new(outcomes.into()),
        }
    }

    /// Returns every recorded invocation in execution order.
    #[must_use]
    pub fn calls(&self) -> Vec<(SessionId, RunId, ToolCallDto)> {
        self.calls
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone()
    }
}

impl ToolExecutionPort for ScriptedPort {
    fn execute_tool(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call: ToolCallDto,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = DtoResult<ToolResultOutcomeDto>> + Send + '_>,
    > {
        self.calls
            .lock()
            .expect("the fixture mutex is not poisoned")
            .push((session_id, run_id, call));
        let outcome = self
            .outcomes
            .lock()
            .expect("the fixture mutex is not poisoned")
            .pop_front()
            .expect("scripted tool outcome exists");
        Box::pin(future::ready(outcome))
    }
}
