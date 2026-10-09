//! Shared fixtures for the engine integration suites.
//!
//! Each integration target compiles this module and uses a different subset of
//! the fixtures, so unused items are expected here.

#![allow(
    dead_code,
    reason = "every integration target compiles the shared module and uses a different subset"
)]
#![allow(
    clippy::expect_used,
    reason = "shared fixtures use expect to provide precise failures"
)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use intention_config::ConfigSnapshotDto;
use intention_engine::{
    ModelRunCommitDto, ModelRunCommitObserver, ModelSleepFuture, ModelTimePort, RunCancellation,
    ToolExecutionPort, ToolResultOutcomeDto,
};
use intention_proto::{
    CreateSessionCommandDto, DtoResult, ErrorDto, FinishReasonDto, IdempotencyKey,
    MessageProjectionDto, PendingTurnProjectionDto, ProjectId, RemoveTurnCommandDto, RunId,
    RunModeDto, RunProjectionDto, RunStatusDto, SessionId, SessionProjectionDto, TimestampDto,
    ToolCallId, TurnId, UsageDto, WorkspaceId, WorkspaceRootDto,
};
use intention_providers::ToolCallDto;
use intention_storage::{
    AcceptedTurnOutcomeDto, StartingRunModelContextDto, StorageRepositoryDto,
    ToolResultEvidenceDto, ToolResultStatusDto,
};
use intention_test_support::fixture_snapshot;

/// Returns the exact fixture timestamp for one Unix second value.
pub fn time(value: i64) -> TimestampDto {
    TimestampDto::from_unix_seconds(value).expect("fixture timestamp is valid")
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

/// One recorded call to [`FakeRepository::finish_run`].
pub struct RecordedFinish {
    session_id: SessionId,
    run_id: RunId,
    status: RunStatusDto,
    usage: Option<UsageDto>,
    finish_reason: Option<FinishReasonDto>,
    error_code: Option<String>,
    error_message: Option<String>,
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
        self.status
    }
    /// Returns the reported provider usage, when one was reported.
    #[must_use]
    pub const fn usage(&self) -> Option<&UsageDto> {
        self.usage.as_ref()
    }
    /// Returns the provider finish reason, when one was reported.
    #[must_use]
    pub const fn finish_reason(&self) -> Option<FinishReasonDto> {
        self.finish_reason
    }
    /// Returns the safe error code of a failed run, when it failed.
    #[must_use]
    pub fn error_code(&self) -> Option<&str> {
        self.error_code.as_deref()
    }
    /// Returns the safe error message of a failed run, when it failed.
    #[must_use]
    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
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
    pub status: RefCell<RunStatusDto>,
    pub messages: RefCell<Vec<MessageProjectionDto>>,
    pub finishes: RefCell<Vec<RecordedFinish>>,
    pub transitions: RefCell<Vec<RecordedTransition>>,
    pub tool_results: RefCell<Vec<ToolResultEvidenceDto>>,
    /// Counts committed tool-result rows for `Send + Sync` observers that must
    /// not borrow this fixture; mirrors `tool_results`.
    pub committed_result_rows: AtomicUsize,
    pub created: RefCell<Option<SessionProjectionDto>>,
    pub accepted: RefCell<DtoResult<AcceptedTurnOutcomeDto>>,
    pub accepted_inputs: RefCell<Vec<RecordedTurn>>,
    pub removed: RefCell<Option<PendingTurnProjectionDto>>,
    pub loaded_projection: RefCell<Option<SessionProjectionDto>>,
    pub starting_context: RefCell<Option<StartingRunModelContextDto>>,
    pub run: RefCell<Option<RunProjectionDto>>,
    pub commit_calls: RefCell<usize>,
    pub commit_failures: RefCell<Vec<usize>>,
    pub commit_error: RefCell<Option<ErrorDto>>,
    pub append_failure: RefCell<Option<ErrorDto>>,
    pub append_failure_at: RefCell<Option<(usize, ErrorDto)>>,
    pub cancel_after_append: RefCell<Option<(usize, RunCancellation)>>,
    pub append_count: RefCell<usize>,
    pub config_error: RefCell<Option<ErrorDto>>,
    /// Pending user messages committed by the next context boundary.
    pub pending: RefCell<VecDeque<MessageProjectionDto>>,
    pub pending_consumes: RefCell<usize>,
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
            status: RefCell::new(RunStatusDto::Starting),
            messages: RefCell::new(Vec::new()),
            finishes: RefCell::new(Vec::new()),
            transitions: RefCell::new(Vec::new()),
            tool_results: RefCell::new(Vec::new()),
            committed_result_rows: AtomicUsize::new(0),
            created: RefCell::new(None),
            accepted: RefCell::new(Err(ErrorDto::unavailable("fixture_unused", "unused"))),
            accepted_inputs: RefCell::new(Vec::new()),
            removed: RefCell::new(None),
            loaded_projection: RefCell::new(None),
            starting_context: RefCell::new(None),
            run: RefCell::new(None),
            commit_calls: RefCell::new(0),
            commit_failures: RefCell::new(Vec::new()),
            commit_error: RefCell::new(None),
            append_failure: RefCell::new(None),
            append_failure_at: RefCell::new(None),
            cancel_after_append: RefCell::new(None),
            append_count: RefCell::new(0),
            config_error: RefCell::new(None),
            pending: RefCell::new(VecDeque::new()),
            pending_consumes: RefCell::new(0),
        }
    }

    /// Creates a fixture run in the supplied status under a fresh identity.
    #[must_use]
    pub fn with_status(session_id: SessionId, status: RunStatusDto) -> Self {
        let repository = Self::new(session_id, RunId::new(), fixture_snapshot());
        *repository.status.borrow_mut() = status;
        repository
    }

    /// Creates the admit-turn fixture with a fresh identity.
    #[must_use]
    pub fn with_accepted(accepted: DtoResult<AcceptedTurnOutcomeDto>) -> Self {
        let repository = Self::new(SessionId::new(), RunId::new(), fixture_snapshot());
        *repository.accepted.borrow_mut() = accepted;
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
        if let Some(run) = self.run.borrow().as_ref() {
            return *run;
        }
        RunProjectionDto::new(
            self.session_id,
            self.run_id,
            self.turn_id,
            *self.status.borrow(),
            self.config.revision_id(),
        )
    }

    /// Returns every committed transcript row.
    #[must_use]
    pub fn committed_messages(&self) -> Vec<MessageProjectionDto> {
        self.messages.borrow().clone()
    }

    /// Returns every committed tool-result evidence row.
    #[must_use]
    pub fn committed_results(&self) -> Vec<ToolResultEvidenceDto> {
        self.tool_results.borrow().clone()
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
        let mut count = self.append_count.borrow_mut();
        *count += 1;
        *count
    }

    /// Returns the next one-based commit ordinal, or the selected injected failure.
    fn next_commit(&self) -> DtoResult<usize> {
        if let Some(error) = self.commit_error.borrow().clone() {
            return Err(error);
        }
        let ordinal = {
            let mut calls = self.commit_calls.borrow_mut();
            *calls += 1;
            *calls
        };
        if self.commit_failures.borrow().contains(&ordinal) {
            return Err(ErrorDto::unavailable(
                "append_unavailable",
                "append refused at the selected call",
            ));
        }
        Ok(ordinal)
    }

    /// Stores one durable status transition and keeps an explicit run current.
    fn store_status(&self, status: RunStatusDto) {
        *self.status.borrow_mut() = status;
        let refreshed = self.run.borrow().as_ref().map(|run| {
            RunProjectionDto::new(
                run.session_id(),
                run.run_id(),
                run.turn_id(),
                status,
                run.config_revision_id(),
            )
        });
        if let Some(refreshed) = refreshed {
            *self.run.borrow_mut() = Some(refreshed);
        }
    }
}

impl StorageRepositoryDto for FakeRepository {
    fn create_session(
        &self,
        _command: CreateSessionCommandDto,
        _occurred_at: TimestampDto,
    ) -> DtoResult<SessionProjectionDto> {
        self.created.borrow().clone().ok_or_else(|| {
            ErrorDto::unavailable("fixture_missing_result", "fixture result missing")
        })
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
        self.accepted_inputs.borrow_mut().push(RecordedTurn {
            proposed_run_id,
            config_snapshot,
            occurred_at,
        });
        self.accepted.borrow().clone()
    }

    fn remove_turn(
        &self,
        _command: RemoveTurnCommandDto,
        _occurred_at: TimestampDto,
    ) -> DtoResult<PendingTurnProjectionDto> {
        self.removed.borrow().clone().ok_or_else(|| {
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
        *self.pending_consumes.borrow_mut() += 1;
        Ok(self.pending.borrow_mut().drain(..).collect())
    }

    fn transition_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
        status: RunStatusDto,
        _occurred_at: TimestampDto,
    ) -> DtoResult<RunProjectionDto> {
        assert_eq!(session_id, self.session_id);
        assert_eq!(run_id, self.run_id);
        self.store_status(status);
        self.transitions
            .borrow_mut()
            .push(RecordedTransition { status });
        Ok(self.projection())
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "One flat terminal outcome keeps the single transaction at one call site."
    )]
    fn finish_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
        status: RunStatusDto,
        usage: Option<UsageDto>,
        finish_reason: Option<FinishReasonDto>,
        error_code: Option<String>,
        error_message: Option<String>,
        occurred_at: TimestampDto,
    ) -> DtoResult<RunProjectionDto> {
        self.finishes.borrow_mut().push(RecordedFinish {
            session_id,
            run_id,
            status,
            usage,
            finish_reason,
            error_code,
            error_message,
            occurred_at,
        });
        self.store_status(status);
        Ok(self.projection())
    }

    fn append_message(
        &self,
        message: MessageProjectionDto,
        _occurred_at: TimestampDto,
    ) -> DtoResult<MessageProjectionDto> {
        self.next_commit()?;
        let index = self.next_append_index();
        if let Some(error) = self.append_failure.borrow_mut().take() {
            return Err(error);
        }
        if self
            .append_failure_at
            .borrow()
            .as_ref()
            .is_some_and(|(at, _)| *at == index)
        {
            return Err(self
                .append_failure_at
                .borrow_mut()
                .take()
                .expect("configured append failure exists")
                .1);
        }
        if let Some((cancel_at, signal)) = self.cancel_after_append.borrow().as_ref()
            && cancel_at == &index
        {
            signal.cancel();
        }
        self.messages.borrow_mut().push(message.clone());
        Ok(message)
    }

    fn write_tool_result(
        &self,
        evidence: ToolResultEvidenceDto,
        message: MessageProjectionDto,
    ) -> DtoResult<ToolResultEvidenceDto> {
        self.next_commit()?;
        self.messages.borrow_mut().push(message);
        self.tool_results.borrow_mut().push(evidence.clone());
        self.committed_result_rows.fetch_add(1, Ordering::SeqCst);
        Ok(evidence)
    }

    fn load_tool_result(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
        _call_id: ToolCallId,
    ) -> DtoResult<ToolResultEvidenceDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "tool result loads are not used by this fixture",
        ))
    }

    fn load_run_config_snapshot(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<ConfigSnapshotDto> {
        assert_eq!((session_id, run_id), (self.session_id, self.run_id));
        if let Some(error) = self.config_error.borrow_mut().take() {
            return Err(error);
        }
        Ok(self.config.clone())
    }

    fn load_starting_run_model_context(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<StartingRunModelContextDto> {
        self.starting_context.borrow().clone().ok_or_else(|| {
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
            .borrow()
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
        if let Some(projection) = self.loaded_projection.borrow().clone() {
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
            Vec::new(),
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
    pub sleeps: RefCell<Vec<Duration>>,
}

impl ImmediateTime {
    /// Creates an empty immediate clock.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sleeps: RefCell::new(Vec::new()),
        }
    }
}

impl ModelTimePort for ImmediateTime {
    fn now(&self) -> TimestampDto {
        time(2)
    }

    fn sleep(&self, duration: Duration) -> ModelSleepFuture<'_> {
        self.sleeps.borrow_mut().push(duration);
        Box::pin(future::ready(()))
    }
}

/// Records every committed transcript row and run status.
pub struct RecordingCommitObserver {
    commits: Mutex<Vec<ModelRunCommitDto>>,
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
    pub fn commits(&self) -> Vec<ModelRunCommitDto> {
        self.commits
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl ModelRunCommitObserver for RecordingCommitObserver {
    fn observe_model_run_commit(&self, committed: &ModelRunCommitDto) {
        self.commits
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
            .unwrap_or_else(PoisonError::into_inner)
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
            .unwrap_or_else(PoisonError::into_inner)
            .push((session_id, run_id, call));
        let outcome = self
            .outcomes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .expect("scripted tool outcome exists");
        Box::pin(future::ready(outcome))
    }
}
