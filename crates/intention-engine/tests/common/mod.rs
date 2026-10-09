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
    ModelRunCommitDto, ModelRunCommitObserver, ModelSleepFuture, ModelTimePort,
    ProviderDriverRegistry, RunCancellation, SessionEventSink, ToolExecutionPort,
    ToolResultOutcomeDto,
};
use intention_proto::provider::{
    CatalogRevisionId, ProviderDiscoveryAttemptId, ProviderModelRecordDto, ProviderProfileId,
    ProviderProfilePolicyDto, ReasoningHistoryManifestDto, ResolvedRunProviderSelectionDto,
    SessionProviderProfileChangedDto,
};
use intention_proto::{
    CreateSessionCommandDto, DtoResult, ErrorDto, FinishReasonDto, IdempotencyKey,
    MessageProjectionDto, PendingTurnProjectionDto, ProjectId, RemoveTurnCommandDto, RunId,
    RunModeDto, RunProjectionDto, RunStatusDto, SessionId, SessionProjectionDto, TimestampDto,
    ToolCallId, TurnId, UsageDto, WorkspaceId, WorkspaceRootDto,
};
use intention_providers::ToolCallDto;
use intention_storage::{
    AcceptedTurnOutcomeDto, ProfileUsageAggregateDto, ProviderCatalogRevisionDto,
    ProviderCatalogStateDto, ProviderDiscoveryAttemptDto, ProviderDiscoveryFailureDto,
    RunOutcomeDto, SessionProviderProfileChangeDto, SessionProviderProfileDto,
    StartingRunModelContextDto, StorageRepositoryDto, ToolResultEvidenceDto, ToolResultStatusDto,
};
use intention_test_support::{fixture_selection, fixture_snapshot};

/// A fixture registry answering every liveness question the same way.
pub struct ScriptedRegistry {
    live: bool,
}

impl ScriptedRegistry {
    /// Creates a registry that reports the supplied liveness for every selection.
    #[must_use]
    pub const fn new(live: bool) -> Self {
        Self { live }
    }
}

impl ProviderDriverRegistry for ScriptedRegistry {
    fn has_live_driver(&self, _selection: &ResolvedRunProviderSelectionDto) -> bool {
        self.live
    }
}

/// Records every published session provider-profile change.
pub struct RecordingSessionSink {
    events: Mutex<Vec<SessionProviderProfileChangedDto>>,
}

impl RecordingSessionSink {
    /// Creates an empty recorder.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    /// Returns every published change in publication order.
    #[must_use]
    pub fn events(&self) -> Vec<SessionProviderProfileChangedDto> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl SessionEventSink for RecordingSessionSink {
    fn observe_session_provider_profile_change(&self, event: &SessionProviderProfileChangedDto) {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event.clone());
    }
}

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
    selection: ResolvedRunProviderSelectionDto,
    reasoning_history: Option<ReasoningHistoryManifestDto>,
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
    /// Returns the exact resolved provider selection committed with the turn.
    #[must_use]
    pub const fn selection(&self) -> &ResolvedRunProviderSelectionDto {
        &self.selection
    }
    /// Returns the committed reasoning history manifest, when one was supplied.
    #[must_use]
    pub const fn reasoning_history(&self) -> Option<&ReasoningHistoryManifestDto> {
        self.reasoning_history.as_ref()
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
    /// The exact selection the persisted run carries.
    pub selection: Mutex<Option<ResolvedRunProviderSelectionDto>>,
    /// One committed session provider-profile change outcome.
    pub session_change: Mutex<Option<SessionProviderProfileChangeDto>>,
    /// An injected failure of the session provider-profile change.
    pub session_change_error: Mutex<Option<ErrorDto>>,
    /// The durable session provider default read.
    pub session_profile: Mutex<Option<SessionProviderProfileDto>>,
    /// The committed usage aggregates of one profile.
    pub profile_usage: Mutex<Vec<ProfileUsageAggregateDto>>,
    /// Every catalog revision the fixture accepted.
    pub accepted_catalog_revisions: Mutex<Vec<ProviderCatalogRevisionDto>>,
    /// Every catalog revision the fixture activated.
    pub activated_catalog_revisions: Mutex<Vec<CatalogRevisionId>>,
    /// Every removal candidate the fixture recorded as rejected.
    pub rejected_candidates: Mutex<Vec<CatalogRevisionId>>,
    /// The durable reasoning source steps the fixture reports.
    pub reasoning_source: Mutex<Vec<intention_storage::ReasoningHistorySourceStepDto>>,
    /// Pending user messages committed by the next context boundary.
    pub pending: Mutex<VecDeque<MessageProjectionDto>>,
    pub pending_consumes: Mutex<usize>,
}

impl FakeRepository {
    /// Creates the fixture for one exact run identity in its `Starting` state.
    ///
    /// The persisted selection carries the fixture model, so a run that passes
    /// the matching exact selection executes.
    #[must_use]
    pub fn new(session_id: SessionId, run_id: RunId, config: ConfigSnapshotDto) -> Self {
        Self::with_selection(
            session_id,
            run_id,
            config,
            fixture_selection("fixture-openrouter", "openrouter", "fixture")
                .expect("the fixture selection is valid"),
        )
    }

    /// Creates the fixture with one explicit persisted selection.
    #[must_use]
    pub fn with_selection(
        session_id: SessionId,
        run_id: RunId,
        config: ConfigSnapshotDto,
        selection: ResolvedRunProviderSelectionDto,
    ) -> Self {
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
            selection: Mutex::new(Some(selection)),
            session_change: Mutex::new(None),
            session_change_error: Mutex::new(None),
            session_profile: Mutex::new(None),
            profile_usage: Mutex::new(Vec::new()),
            accepted_catalog_revisions: Mutex::new(Vec::new()),
            activated_catalog_revisions: Mutex::new(Vec::new()),
            rejected_candidates: Mutex::new(Vec::new()),
            reasoning_source: Mutex::new(Vec::new()),
            pending: Mutex::new(VecDeque::new()),
            pending_consumes: Mutex::new(0),
        }
    }

    /// Returns the exact selection the persisted run carries.
    ///
    /// # Panics
    ///
    /// Panics when the fixture holds no selection; the constructor always sets
    /// one.
    #[must_use]
    pub fn selection(&self) -> ResolvedRunProviderSelectionDto {
        self.selection
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .expect("the fixture always holds a persisted selection")
    }

    /// Clears the persisted selection so the run has none.
    pub fn clear_selection(&self) {
        *self
            .selection
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
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
        let stored = self
            .run
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
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
        selection: ResolvedRunProviderSelectionDto,
        reasoning_history: Option<ReasoningHistoryManifestDto>,
        occurred_at: TimestampDto,
    ) -> DtoResult<AcceptedTurnOutcomeDto> {
        self.accepted_inputs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(RecordedTurn {
                proposed_run_id,
                config_snapshot,
                selection,
                reasoning_history,
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

    fn accept_catalog_revision(
        &self,
        revision: ProviderCatalogRevisionDto,
    ) -> DtoResult<ProviderCatalogStateDto> {
        let revision_id = revision.catalog_revision_id();
        self.accepted_catalog_revisions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(revision);
        ProviderCatalogStateDto::new(Some(revision_id), None)
    }

    fn mark_catalog_activated(
        &self,
        catalog_revision_id: CatalogRevisionId,
        _occurred_at: TimestampDto,
    ) -> DtoResult<ProviderCatalogStateDto> {
        self.activated_catalog_revisions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(catalog_revision_id);
        ProviderCatalogStateDto::new(Some(catalog_revision_id), Some(catalog_revision_id))
    }

    fn record_catalog_candidate_rejected(
        &self,
        candidate_revision_id: CatalogRevisionId,
        _occurred_at: TimestampDto,
    ) -> DtoResult<()> {
        self.rejected_candidates
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(candidate_revision_id);
        Ok(())
    }

    fn load_catalog_state(&self) -> DtoResult<ProviderCatalogStateDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "catalog state reads are not used by this fixture",
        ))
    }

    fn load_catalog_revision(
        &self,
        _catalog_revision_id: CatalogRevisionId,
    ) -> DtoResult<ProviderCatalogRevisionDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "catalog revision reads are not used by this fixture",
        ))
    }

    fn store_profile_policy(
        &self,
        _profile_id: ProviderProfileId,
        _policy: ProviderProfilePolicyDto,
        _occurred_at: TimestampDto,
    ) -> DtoResult<ProviderProfilePolicyDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "profile policy storage is not used by this fixture",
        ))
    }

    fn set_session_provider_profile(
        &self,
        session_id: SessionId,
        _profile_id: ProviderProfileId,
        _expected_session_projection_revision: u64,
        _occurred_at: TimestampDto,
    ) -> DtoResult<SessionProviderProfileChangeDto> {
        assert_eq!(session_id, self.session_id);
        let injected = self
            .session_change_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(error) = injected {
            return Err(error);
        }
        (*self
            .session_change
            .lock()
            .unwrap_or_else(PoisonError::into_inner))
        .ok_or_else(|| {
            ErrorDto::unavailable("fixture_unused", "the fixture stages no session change")
        })
    }

    fn load_session_provider_profile(
        &self,
        _session_id: SessionId,
    ) -> DtoResult<SessionProviderProfileDto> {
        let profile = self
            .session_profile
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        profile.ok_or_else(|| {
            ErrorDto::unavailable("session_not_found", "the session holds no provider default")
        })
    }

    fn load_run_provider_selection(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<ResolvedRunProviderSelectionDto> {
        self.selection
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| {
                ErrorDto::unavailable(
                    "run_provider_selection_not_found",
                    "the run carries no persisted provider selection",
                )
            })
    }

    fn begin_discovery_attempt(
        &self,
        _attempt_id: ProviderDiscoveryAttemptId,
        _profile_id: ProviderProfileId,
        _occurred_at: TimestampDto,
    ) -> DtoResult<ProviderDiscoveryAttemptDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "discovery is not used by this fixture",
        ))
    }

    fn mark_discovery_started(
        &self,
        _attempt_id: ProviderDiscoveryAttemptId,
        _occurred_at: TimestampDto,
    ) -> DtoResult<ProviderDiscoveryAttemptDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "discovery is not used by this fixture",
        ))
    }

    fn complete_discovery_attempt(
        &self,
        _attempt_id: ProviderDiscoveryAttemptId,
        _records: Vec<ProviderModelRecordDto>,
        _occurred_at: TimestampDto,
    ) -> DtoResult<intention_proto::provider::ProviderDiscoveryResultDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "discovery is not used by this fixture",
        ))
    }

    fn fail_discovery_attempt(
        &self,
        _attempt_id: ProviderDiscoveryAttemptId,
        _failure: ProviderDiscoveryFailureDto,
        _occurred_at: TimestampDto,
    ) -> DtoResult<ProviderDiscoveryAttemptDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "discovery is not used by this fixture",
        ))
    }

    fn recover_unfinished_discovery_attempts(
        &self,
        _recovered_at: TimestampDto,
    ) -> DtoResult<Vec<ProviderDiscoveryAttemptDto>> {
        Ok(Vec::new())
    }

    fn load_profile_usage(
        &self,
        _profile_id: ProviderProfileId,
    ) -> DtoResult<Vec<ProfileUsageAggregateDto>> {
        Ok(self
            .profile_usage
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
    }

    fn load_reasoning_history_source(
        &self,
        _session_id: SessionId,
    ) -> DtoResult<Vec<intention_storage::ReasoningHistorySourceStepDto>> {
        Ok(self
            .reasoning_source
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
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
