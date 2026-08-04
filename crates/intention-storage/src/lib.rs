//! DTO-only semantic storage contracts for durable sessions and event history.
//!
//! Implementations own transactions and backend resources. This crate exposes no
//! connection, filesystem, SQL, path, or closure-based API across its boundary.

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    CreateSessionCommandDto, DomainEventDto, ModelRunFactInputDto, RemoveQueuedTurnCommandDto,
    RunEventCursorDto, RunEventTailPageDto, RunProjectionDto, RunReplayDto, RunSnapshotDto,
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

/// Inputs required to atomically append one non-empty batch of durable model facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendModelRunFactsInputDto {
    session_id: SessionId,
    run_id: RunId,
    expected_cursor: RunEventCursorDto,
    facts: Vec<ModelRunFactInputDto>,
    status: Option<RunStatusDto>,
    occurred_at: TimestampDto,
}

impl AppendModelRunFactsInputDto {
    /// Creates an explicit durable model-fact append request.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the batch is empty.
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        expected_cursor: RunEventCursorDto,
        facts: Vec<ModelRunFactInputDto>,
        status: Option<RunStatusDto>,
        occurred_at: TimestampDto,
    ) -> DtoResult<Self> {
        if facts.is_empty() {
            return Err(ErrorDto::validation(
                "invalid_run_event_cursor",
                "model fact append batch must not be empty",
            ));
        }
        if facts.iter().enumerate().any(|(index, fact)| {
            index + 1 < facts.len()
                && matches!(
                    fact,
                    ModelRunFactInputDto::Finished { .. } | ModelRunFactInputDto::Failed { .. }
                )
        }) {
            return Err(ErrorDto::validation(
                "invalid_run_event_cursor",
                "terminal model facts must be last in an append batch",
            ));
        }
        Ok(Self {
            session_id,
            run_id,
            expected_cursor,
            facts,
            status,
            occurred_at,
        })
    }

    /// Returns the owning session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the target run identity.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    /// Returns the required current cursor before appending.
    #[must_use]
    pub const fn expected_cursor(&self) -> RunEventCursorDto {
        self.expected_cursor
    }

    /// Returns the non-empty input facts in their requested order.
    #[must_use]
    pub fn facts(&self) -> &[ModelRunFactInputDto] {
        &self.facts
    }

    /// Returns an optional status transition committed in the same transaction.
    #[must_use]
    pub const fn status(&self) -> Option<RunStatusDto> {
        self.status
    }

    /// Returns the selected durable fact timestamp.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Evidence that a model-fact batch, run projection, and snapshots committed together.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendModelRunFactsOutcomeDto {
    cursor: RunEventCursorDto,
    snapshot: RunSnapshotDto,
    facts: Vec<intention_domain::ModelRunFactDto>,
}

impl AppendModelRunFactsOutcomeDto {
    /// Creates coherent atomic model-fact append evidence.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the facts do not end at the snapshot cursor.
    pub fn new(
        cursor: RunEventCursorDto,
        snapshot: RunSnapshotDto,
        facts: Vec<intention_domain::ModelRunFactDto>,
    ) -> DtoResult<Self> {
        if snapshot.cursor() != cursor
            || facts
                .last()
                .map_or_else(|| cursor.value(), |fact| fact.cursor().value())
                != cursor.value()
        {
            return Err(ErrorDto::validation(
                "invalid_run_event_cursor",
                "model fact append outcome must end at its snapshot cursor",
            ));
        }
        Ok(Self {
            cursor,
            snapshot,
            facts,
        })
    }

    /// Returns the resulting current run cursor.
    #[must_use]
    pub const fn cursor(&self) -> RunEventCursorDto {
        self.cursor
    }

    /// Returns the committed current run snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> &RunSnapshotDto {
        &self.snapshot
    }

    /// Returns the appended typed durable facts.
    #[must_use]
    pub fn facts(&self) -> &[intention_domain::ModelRunFactDto] {
        &self.facts
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

    /// Appends typed durable model facts using an exact expected run cursor and optional status transition.
    ///
    /// # Errors
    ///
    /// Returns stable M4 validation, conflict, not-found, or unavailable errors
    /// when the scoped run cannot append atomically.
    fn append_model_run_facts(
        &self,
        _input: AppendModelRunFactsInputDto,
    ) -> DtoResult<AppendModelRunFactsOutcomeDto> {
        Err(ErrorDto::unavailable(
            "run_history_unavailable",
            "the durable run history is unavailable",
        ))
    }

    /// Loads the persisted credential-free configuration selected for a matching run.
    ///
    /// # Errors
    ///
    /// Returns `run_configuration_not_found` for unknown or cross-session run
    /// identity, or `run_configuration_unavailable` when the persisted safe
    /// selection cannot be loaded. Credentials, raw TOML, configuration paths,
    /// and backend resources never cross this boundary.
    fn load_run_config_snapshot(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<ConfigSnapshotDto> {
        Err(ErrorDto::unavailable(
            "run_configuration_unavailable",
            "the durable run configuration is unavailable",
        ))
    }

    /// Loads the current matching run snapshot at its cursor and an empty tail after it.
    ///
    /// # Errors
    ///
    /// Returns `run_replay_not_found` for unknown or cross-session run identity,
    /// or `run_history_unavailable` when durable M4 replay cannot be loaded.
    fn load_current_run_replay(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<RunReplayDto> {
        Err(ErrorDto::unavailable(
            "run_history_unavailable",
            "the durable run history is unavailable",
        ))
    }

    /// Loads a bounded contiguous run-fact page strictly after a matching cursor.
    ///
    /// # Errors
    ///
    /// Returns `invalid_run_event_cursor` for an unusable cursor,
    /// `run_replay_not_found` for unknown or cross-session identity, or
    /// `run_history_unavailable` when durable M4 history cannot be loaded.
    fn load_run_tail(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
        _after_cursor: RunEventCursorDto,
    ) -> DtoResult<RunEventTailPageDto> {
        Err(ErrorDto::unavailable(
            "run_history_unavailable",
            "the durable run history is unavailable",
        ))
    }

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
