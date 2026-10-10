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
    RunModeDto, RunProjectionDto, RunStatusDto, SessionId, SessionProjectionDto,
    SessionSummariesDto, TimestampDto, ToolCallId, TurnId, UsageDto, WorkspaceId, WorkspaceRootDto,
};
use intention_providers::ToolCallDto;
use intention_storage::{
    AcceptedTurnOutcomeDto, RunOutcomeDto, StartingRunModelContextDto, StorageRepositoryDto,
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
    pub tool_results: Mutex<Vec<ToolResultEvidenceDto>>,
    /// Counts committed tool-result rows for `Send + Sync` observers that must
    /// not borrow this fixture; mirrors `tool_results`.
    pub committed_result_rows: AtomicUsize,
    pub created: Mutex<Option<SessionProjectionDto>>,
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
            created: Mutex::new(None),
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
            .unwrap_or_else(PoisonError::into_inner) = status;
        repository
    }

    /// Creates the admit-turn fixture with a fresh identity.
    #[must_use]
    pub fn with_accepted(accepted: DtoResult<AcceptedTurnOutcomeDto>) -> Self {
        let repository = Self::new(SessionId::new(), RunId::new(), fixture_snapshot());
        *repository
            .accepted
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = accepted;
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
        let stored = *self.run.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(run) = stored {
            return run;
        }
        RunProjectionDto::new(
            self.session_id,
            self.run_id,
            self.turn_id,
            *self.status.lock().unwrap_or_else(PoisonError::into_inner),
            self.config.revision_id(),
        )
    }

    /// Returns every committed transcript row.
    #[must_use]
    pub fn committed_messages(&self) -> Vec<MessageProjectionDto> {
        self.messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Returns every committed tool-result evidence row.
    #[must_use]
    pub fn committed_results(&self) -> Vec<ToolResultEvidenceDto> {
        self.tool_results
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
            .unwrap_or_else(PoisonError::into_inner);
        *count += 1;
        *count
    }

    /// Returns the next one-based commit ordinal, or the selected injected failure.
    fn next_commit(&self) -> DtoResult<usize> {
        let injected = self
            .commit_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(error) = injected {
            return Err(error);
        }
        let ordinal = {
            let mut calls = self
                .commit_calls
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            *calls += 1;
            *calls
        };
        if self
            .commit_failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
    fn store_status(&self, status: RunStatusDto) {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner) = status;
        let refreshed = self
            .run
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|run| {
                RunProjectionDto::new(
                    run.session_id(),
                    run.run_id(),
                    run.turn_id(),
                    status,
                    run.config_revision_id(),
                )
            });
        if let Some(refreshed) = refreshed {
            *self.run.lock().unwrap_or_else(PoisonError::into_inner) = Some(refreshed);
        }
    }
}

impl StorageRepositoryDto for FakeRepository {
    fn create_session(
        &self,
        _command: CreateSessionCommandDto,
        _occurred_at: TimestampDto,
    ) -> DtoResult<SessionProjectionDto> {
        self.created
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| {
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
        self.accepted_inputs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(RecordedTurn {
                proposed_run_id,
                config_snapshot,
                occurred_at,
            });
        self.accepted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn remove_turn(
        &self,
        _command: RemoveTurnCommandDto,
        _occurred_at: TimestampDto,
    ) -> DtoResult<PendingTurnProjectionDto> {
        self.removed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
            .unwrap_or_else(PoisonError::into_inner) += 1;
        Ok(self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect())
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
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
            .unwrap_or_else(PoisonError::into_inner)
            .push(RecordedFinish {
                session_id,
                run_id,
                outcome,
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
        let injected = self
            .append_failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(error) = injected {
            return Err(error);
        }
        if self
            .append_failure_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|(at, _)| *at == index)
        {
            return Err(self
                .append_failure_at
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
                .expect("configured append failure exists")
                .1);
        }
        let scheduled = self
            .cancel_after_append
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some((cancel_at, signal)) = scheduled
            && cancel_at == index
        {
            signal.cancel();
        }
        self.messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(message.clone());
        Ok(message)
    }

    fn write_tool_result(
        &self,
        evidence: ToolResultEvidenceDto,
        message: MessageProjectionDto,
    ) -> DtoResult<ToolResultEvidenceDto> {
        self.next_commit()?;
        self.messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(message);
        self.tool_results
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(evidence.clone());
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
        let injected = self
            .config_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
            .unwrap_or_else(PoisonError::into_inner)
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
            .unwrap_or_else(PoisonError::into_inner)
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
            .unwrap_or_else(PoisonError::into_inner)
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
            .unwrap_or_else(PoisonError::into_inner)
            .push(duration);
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
