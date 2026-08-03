//! DTO-only semantic storage contracts for durable sessions and event history.
//!
//! Implementations own transactions and backend resources. This crate exposes no
//! connection, filesystem, SQL, path, or closure-based API across its boundary.

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    CreateSessionCommandDto, DomainEventDto, RemoveQueuedTurnCommandDto, RunProjectionDto,
    RunStatusDto, SessionProjectionDto,
};
use intention_types::{
    ConfigRevisionId, DtoResult, ErrorDto, EventEnvelopeDto, QueuePositionDto, RunId,
    SessionEventSequenceDto, SessionId, TimestampDto, TurnId,
};

/// Inputs required to create one durable session at an explicit event time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateSessionInputDto {
    command: CreateSessionCommandDto,
    occurred_at: TimestampDto,
}

impl CreateSessionInputDto {
    /// Creates typed session-creation storage input.
    #[must_use]
    pub const fn new(command: CreateSessionCommandDto, occurred_at: TimestampDto) -> Self {
        Self {
            command,
            occurred_at,
        }
    }
    /// Returns the typed session creation command.
    #[must_use]
    pub const fn command(&self) -> &CreateSessionCommandDto {
        &self.command
    }
    /// Returns the externally selected durable event time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Inputs required to durably accept a turn, including its possible first-run identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptUserTurnInputDto {
    session_id: SessionId,
    turn_id: TurnId,
    content: String,
    proposed_run_id: RunId,
    config_snapshot: ConfigSnapshotDto,
    occurred_at: TimestampDto,
}

impl AcceptUserTurnInputDto {
    /// Creates complete turn-acceptance storage input.
    ///
    /// `proposed_run_id` and `config_snapshot` are committed if this turn starts
    /// immediately, or retained as the immutable future-run selection if queued.
    ///
    /// # Errors
    ///
    /// Returns a validation error when content is blank or the safe snapshot is
    /// not suitable for persistence.
    pub fn new(
        session_id: SessionId,
        turn_id: TurnId,
        content: impl Into<String>,
        proposed_run_id: RunId,
        config_snapshot: ConfigSnapshotDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<Self> {
        let content = content.into();
        if content.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_turn_content",
                "user turn content must not be empty",
            ));
        }
        config_snapshot.validate_for_persistence()?;
        Ok(Self {
            session_id,
            turn_id,
            content,
            proposed_run_id,
            config_snapshot,
            occurred_at,
        })
    }
    /// Returns the owning session.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }
    /// Returns the accepted turn identity.
    #[must_use]
    pub const fn turn_id(&self) -> TurnId {
        self.turn_id
    }
    /// Returns the user-authored content.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
    /// Returns the run identity to use if this turn starts.
    #[must_use]
    pub const fn proposed_run_id(&self) -> RunId {
        self.proposed_run_id
    }
    /// Returns the credential-free immutable configuration snapshot for the run.
    #[must_use]
    pub const fn config_snapshot(&self) -> &ConfigSnapshotDto {
        &self.config_snapshot
    }
    /// Returns the mandatory immutable configuration revision.
    #[must_use]
    pub const fn config_revision_id(&self) -> ConfigRevisionId {
        self.config_snapshot.revision_id()
    }
    /// Returns the externally selected durable event time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Inputs required to remove a queued turn at an explicit event time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoveQueuedTurnInputDto {
    command: RemoveQueuedTurnCommandDto,
    occurred_at: TimestampDto,
}
impl RemoveQueuedTurnInputDto {
    /// Creates typed queued-turn removal storage input.
    #[must_use]
    pub const fn new(command: RemoveQueuedTurnCommandDto, occurred_at: TimestampDto) -> Self {
        Self {
            command,
            occurred_at,
        }
    }
    /// Returns the typed removal command.
    #[must_use]
    pub const fn command(self) -> RemoveQueuedTurnCommandDto {
        self.command
    }
    /// Returns the externally selected durable event time.
    #[must_use]
    pub const fn occurred_at(self) -> TimestampDto {
        self.occurred_at
    }
}

/// Inputs required to commit one run-status transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransitionRunInputDto {
    session_id: SessionId,
    run_id: RunId,
    status: RunStatusDto,
    occurred_at: TimestampDto,
}
impl TransitionRunInputDto {
    /// Creates typed run-transition storage input.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        run_id: RunId,
        status: RunStatusDto,
        occurred_at: TimestampDto,
    ) -> Self {
        Self {
            session_id,
            run_id,
            status,
            occurred_at,
        }
    }
    /// Returns the owning session.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }
    /// Returns the affected run.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }
    /// Returns the requested successor status.
    #[must_use]
    pub const fn status(&self) -> RunStatusDto {
        self.status
    }
    /// Returns the externally selected durable event time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Inputs required to mark all persisted unfinished runs interrupted at recovery time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoverUnfinishedRunsInputDto {
    recovered_at: TimestampDto,
}
impl RecoverUnfinishedRunsInputDto {
    /// Creates recovery input with an explicit durable event time.
    #[must_use]
    pub const fn new(recovered_at: TimestampDto) -> Self {
        Self { recovered_at }
    }
    /// Returns the recovery event time.
    #[must_use]
    pub const fn recovered_at(self) -> TimestampDto {
        self.recovered_at
    }
}

/// Immutable outcome for an accepted user turn after committing it durably.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcceptedTurnOutcomeDto {
    /// The turn began a newly created run with its mandatory configuration revision.
    Started(RunProjectionDto),
    /// The turn was retained behind active work at this stable never-reused queue ticket.
    Queued(QueuePositionDto),
}

/// Immutable semantic evidence that one state change committed atomically.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommittedChangeDto {
    projection: SessionProjectionDto,
    position: SessionEventSequenceDto,
    events: Vec<EventEnvelopeDto<DomainEventDto>>,
    turn_outcome: Option<AcceptedTurnOutcomeDto>,
}
impl CommittedChangeDto {
    /// Creates coherent atomic commit evidence.
    ///
    /// # Errors
    ///
    /// Returns a validation error when events are not contiguous at the final
    /// projection position or a started outcome belongs to another session.
    pub fn new(
        projection: SessionProjectionDto,
        position: SessionEventSequenceDto,
        events: Vec<EventEnvelopeDto<DomainEventDto>>,
        turn_outcome: Option<AcceptedTurnOutcomeDto>,
    ) -> DtoResult<Self> {
        let session_id = projection.session_id();
        let mut expected = position
            .value()
            .checked_sub(events.len() as u64)
            .ok_or_else(|| {
                ErrorDto::validation(
                    "invalid_committed_change",
                    "event count cannot exceed final position",
                )
            })?;
        for event in &events {
            expected = expected.checked_add(1).ok_or_else(|| {
                ErrorDto::validation("invalid_committed_change", "event position overflow")
            })?;
            if event.session_id() != session_id || event.sequence().value() != expected {
                return Err(ErrorDto::validation(
                    "invalid_committed_change",
                    "committed events must be contiguous and belong to the final projection session",
                ));
            }
        }
        if projection.at_sequence() != position || turn_outcome.is_some_and(|outcome| matches!(outcome, AcceptedTurnOutcomeDto::Started(run) if run.session_id() != session_id)) { return Err(ErrorDto::validation("invalid_committed_change", "committed state must agree with its projection and session")); }
        Ok(Self {
            projection,
            position,
            events,
            turn_outcome,
        })
    }
    /// Returns the final durable session projection.
    #[must_use]
    pub const fn projection(&self) -> &SessionProjectionDto {
        &self.projection
    }
    /// Returns the final durable event position.
    #[must_use]
    pub const fn position(&self) -> SessionEventSequenceDto {
        self.position
    }
    /// Returns events committed with the projection in durable sequence order.
    #[must_use]
    pub fn events(&self) -> &[EventEnvelopeDto<DomainEventDto>] {
        &self.events
    }
    /// Returns whether a user turn began a run or was queued in this commit.
    #[must_use]
    pub const fn turn_outcome(&self) -> Option<AcceptedTurnOutcomeDto> {
        self.turn_outcome
    }
}

/// The DTO-only repository contract implemented by future durable backends.
pub trait StorageRepositoryDto {
    /// Creates a session and returns its committed projection and events.
    ///
    /// # Errors
    ///
    /// Returns a validation or conflict error when the supplied creation input
    /// cannot be committed, or an unavailable error when durable storage fails.
    fn create_session(&self, input: CreateSessionInputDto) -> DtoResult<CommittedChangeDto>;

    /// Accepts a turn and atomically records whether it starts or queues durably.
    ///
    /// # Errors
    ///
    /// Returns a validation, not-found, or conflict error when the turn cannot
    /// be accepted for its session, or an unavailable error when storage fails.
    fn accept_user_turn(&self, input: AcceptUserTurnInputDto) -> DtoResult<CommittedChangeDto>;

    /// Removes an unstarted queued turn.
    ///
    /// # Errors
    ///
    /// Returns a not-found or conflict error when the queued turn cannot be
    /// removed, or an unavailable error when durable storage fails.
    fn remove_queued_turn(&self, input: RemoveQueuedTurnInputDto) -> DtoResult<CommittedChangeDto>;

    /// Transitions a run and atomically promotes the oldest queued turn after every terminal transition.
    ///
    /// # Errors
    ///
    /// Returns a validation, not-found, or conflict error when the transition
    /// or promotion is invalid, or an unavailable error when storage fails.
    fn transition_run(&self, input: TransitionRunInputDto) -> DtoResult<CommittedChangeDto>;

    /// Marks persisted unfinished runs interrupted at the supplied durable time.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when recovery changes cannot be durably
    /// committed.
    fn recover_unfinished_runs(
        &self,
        input: RecoverUnfinishedRunsInputDto,
    ) -> DtoResult<Vec<CommittedChangeDto>>;

    /// Loads the latest durable session snapshot projection.
    ///
    /// # Errors
    ///
    /// Returns a not-found error when the session has no durable projection, or
    /// an unavailable error when storage cannot be read.
    fn load_session_snapshot(&self, session_id: SessionId) -> DtoResult<SessionProjectionDto>;

    /// Loads a contiguous tail strictly after the supplied durable position.
    ///
    /// # Errors
    ///
    /// Returns a validation error for an unusable position, a not-found error
    /// for an unknown session, or an unavailable error when storage cannot be read.
    fn load_tail(
        &self,
        session_id: SessionId,
        after_sequence: SessionEventSequenceDto,
    ) -> DtoResult<Vec<EventEnvelopeDto<DomainEventDto>>>;

    /// Records an already credential-free configuration revision snapshot.
    ///
    /// # Errors
    ///
    /// Returns a validation or conflict error when the revision cannot be
    /// recorded, or an unavailable error when durable storage fails.
    fn accept_configuration_revision(&self, snapshot: ConfigSnapshotDto) -> DtoResult<()>;
}
