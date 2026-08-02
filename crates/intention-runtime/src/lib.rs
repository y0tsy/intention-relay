//! Deterministic M3 run lifecycle decisions over DTO-only storage.
//!
//! This crate has no provider, tool, timer, worker-loop, or scheduling
//! dependency. It decides durable transitions and delegates atomic commits to
//! the semantic storage repository.

use intention_config::ConfigSnapshotDto;
use intention_domain::{RunProjectionDto, RunStatusDto, validate_run_status_transition};
use intention_storage::{
    CommittedChangeDto, PromotedQueuedTurnInputDto, RecoverUnfinishedRunsInputDto,
    StorageRepositoryDto, TransitionRunInputDto,
};
use intention_types::{DtoResult, RunId, SessionId, TimestampDto};

/// Explicit values for deterministic runtime lifecycle decisions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeValuesDto {
    next_run_id: RunId,
    config_snapshot: ConfigSnapshotDto,
    occurred_at: TimestampDto,
}

impl RuntimeValuesDto {
    /// Creates deterministic DTO-only lifecycle values.
    #[must_use]
    pub const fn new(
        next_run_id: RunId,
        config_snapshot: ConfigSnapshotDto,
        occurred_at: TimestampDto,
    ) -> Self {
        Self {
            next_run_id,
            config_snapshot,
            occurred_at,
        }
    }

    /// Returns the supplied identity for the next promoted run.
    ///
    /// Queue promotion does not use this identity: queued turns retain their
    /// already-persisted proposed run identity.
    #[must_use]
    pub const fn next_run_id(&self) -> RunId {
        self.next_run_id
    }

    /// Returns the immutable snapshot to attach to a newly promoted run.
    #[must_use]
    pub const fn config_snapshot(&self) -> &ConfigSnapshotDto {
        &self.config_snapshot
    }

    /// Returns the explicit timestamp for lifecycle commits and recovery.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Deterministic lifecycle service over a DTO-only storage repository.
pub struct RuntimeService<'a, Repository> {
    repository: &'a Repository,
    values: RuntimeValuesDto,
}

impl<'a, Repository> RuntimeService<'a, Repository>
where
    Repository: StorageRepositoryDto,
{
    /// Creates a runtime lifecycle service with caller-supplied deterministic values.
    #[must_use]
    pub const fn new(repository: &'a Repository, values: RuntimeValuesDto) -> Self {
        Self { repository, values }
    }

    /// Returns whether a durable state graph edge is declared by the domain.
    #[must_use]
    pub fn can_transition(from: RunStatusDto, to: RunStatusDto) -> bool {
        validate_run_status_transition(from, to).is_ok()
    }

    /// Commits cancellation for an active run.
    ///
    /// A starting run follows `Starting -> Cancelling`; final cancellation and
    /// any queue promotion are committed later through [`Self::complete_terminal`].
    ///
    /// # Errors
    ///
    /// Returns a typed domain transition or repository error.
    pub fn stop_run(&self, session_id: SessionId, run_id: RunId) -> DtoResult<CommittedChangeDto> {
        let active = self.active_run(session_id, run_id)?;
        self.transition(
            session_id,
            run_id,
            active.status(),
            RunStatusDto::Cancelling,
            None,
        )
    }

    /// Commits a terminal transition and atomically promotes the next queued turn.
    ///
    /// # Errors
    ///
    /// Returns typed domain transition or repository errors, including invalid
    /// terminal status requests.
    pub fn complete_terminal(
        &self,
        session_id: SessionId,
        run_id: RunId,
        terminal_status: RunStatusDto,
    ) -> DtoResult<CommittedChangeDto> {
        if !terminal_status.is_terminal() {
            return Err(intention_types::ErrorDto::validation(
                "invalid_terminal_run_status",
                "runtime completion requires a terminal run status",
            ));
        }
        let projection = self.repository.load_session_snapshot(session_id)?;
        let active = self.active_run_from_projection(&projection, run_id)?;
        let promoted_turn = projection
            .queued_turns()
            .first()
            .map(|turn| PromotedQueuedTurnInputDto::new(turn.turn_id()));
        self.transition(
            session_id,
            run_id,
            active.status(),
            terminal_status,
            promoted_turn,
        )
    }

    /// Marks all unfinished durable runs interrupted before an owning facade is ready.
    ///
    /// External execution is deliberately never resumed by recovery.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when durable recovery cannot complete.
    pub fn recover_before_ready(&self) -> DtoResult<Vec<CommittedChangeDto>> {
        self.repository
            .recover_unfinished_runs(RecoverUnfinishedRunsInputDto::new(self.values.occurred_at))
    }

    fn active_run(&self, session_id: SessionId, run_id: RunId) -> DtoResult<RunProjectionDto> {
        let projection = self.repository.load_session_snapshot(session_id)?;
        self.active_run_from_projection(&projection, run_id)
    }

    fn active_run_from_projection(
        &self,
        projection: &intention_domain::SessionProjectionDto,
        run_id: RunId,
    ) -> DtoResult<RunProjectionDto> {
        projection
            .active_run()
            .filter(|active| active.run_id() == run_id)
            .ok_or_else(|| {
                intention_types::ErrorDto::validation(
                    "active_run_not_found",
                    "the requested run is not active in the session",
                )
            })
    }

    fn transition(
        &self,
        session_id: SessionId,
        run_id: RunId,
        from: RunStatusDto,
        to: RunStatusDto,
        promoted_turn: Option<PromotedQueuedTurnInputDto>,
    ) -> DtoResult<CommittedChangeDto> {
        validate_run_status_transition(from, to)?;
        self.repository.transition_run(TransitionRunInputDto::new(
            session_id,
            run_id,
            to,
            self.values.occurred_at,
            promoted_turn,
        ))
    }
}
