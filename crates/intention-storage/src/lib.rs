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
    SessionEventSequenceDto, SessionId, TimestampDto, ToolCallId, TurnId,
};

/// Inputs required to create one durable session at an explicit event time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateSessionInputDto {
    command: CreateSessionCommandDto,
    occurred_at: TimestampDto,
}

/// Input for atomically recording one local tool lifecycle event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendToolLifecycleEventInputDto {
    event: intention_domain::ToolLifecycleEventDto,
    result: Option<ToolResultEvidenceDto>,
}

impl AppendToolLifecycleEventInputDto {
    #[must_use]
    pub const fn new(event: intention_domain::ToolLifecycleEventDto) -> Self {
        Self {
            event,
            result: None,
        }
    }
    /// Attaches typed result evidence committed in the same durable transaction.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the evidence identity does not match the
    /// lifecycle event or the event status cannot carry durable result evidence.
    pub fn with_result(mut self, result: ToolResultEvidenceDto) -> DtoResult<Self> {
        if result.session_id() != self.event.session_id()
            || result.run_id() != self.event.run_id()
            || result.call_id() != self.event.call_id()
        {
            return Err(ErrorDto::validation(
                "invalid_tool_result",
                "tool result evidence must match its lifecycle event identity",
            ));
        }
        if !is_terminal_tool_lifecycle_status(self.event.status()) {
            return Err(ErrorDto::validation(
                "invalid_tool_result",
                "tool result evidence requires a terminal lifecycle status",
            ));
        }
        self.result = Some(result);
        Ok(self)
    }
    #[must_use]
    pub const fn event(&self) -> &intention_domain::ToolLifecycleEventDto {
        &self.event
    }
    /// Returns the typed result evidence committed with this event, when any.
    #[must_use]
    pub const fn result(&self) -> Option<&ToolResultEvidenceDto> {
        self.result.as_ref()
    }
}

/// Whether one tool lifecycle status ends its call and may carry result evidence.
const fn is_terminal_tool_lifecycle_status(
    status: &intention_domain::ToolLifecycleStatusDto,
) -> bool {
    matches!(
        status,
        intention_domain::ToolLifecycleStatusDto::Rejected
            | intention_domain::ToolLifecycleStatusDto::Completed
            | intention_domain::ToolLifecycleStatusDto::Failed
            | intention_domain::ToolLifecycleStatusDto::Cancelled
            | intention_domain::ToolLifecycleStatusDto::ExternalEffectUnknown
    )
}

/// The maximum durable canonical tool result content size in bytes.
const MAX_TOOL_RESULT_CONTENT_BYTES: usize = 512 * 1024;

/// Closed durable discriminator for one committed local tool result family.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ToolResultKindDto {
    Read,
    Glob,
    Grep,
    Write,
    Edit,
    Execute,
}

impl ToolResultKindDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Glob => "glob",
            Self::Grep => "grep",
            Self::Write => "write",
            Self::Edit => "edit",
            Self::Execute => "execute",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns a validation error for an unknown discriminator name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "read" => Ok(Self::Read),
            "glob" => Ok(Self::Glob),
            "grep" => Ok(Self::Grep),
            "write" => Ok(Self::Write),
            "edit" => Ok(Self::Edit),
            "execute" => Ok(Self::Execute),
            _ => Err(ErrorDto::validation(
                "invalid_tool_result",
                "tool result kind is not a known durable discriminator",
            )),
        }
    }
}

/// Typed durable evidence of one committed local tool result.
///
/// `content` carries the bounded canonical projection of the typed result as
/// selected by the caller; it never carries credentials, absolute workspace
/// roots, or backend resources.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolResultEvidenceDto {
    session_id: SessionId,
    run_id: RunId,
    call_id: ToolCallId,
    kind: ToolResultKindDto,
    content: String,
    occurred_at: TimestampDto,
}

impl ToolResultEvidenceDto {
    /// Creates typed tool result evidence with bounded canonical content.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the content is blank, oversized, or
    /// contains interior NUL bytes.
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        call_id: ToolCallId,
        kind: ToolResultKindDto,
        content: impl Into<String>,
        occurred_at: TimestampDto,
    ) -> DtoResult<Self> {
        let content = content.into();
        if content.trim().is_empty() || content.contains('\0') {
            return Err(ErrorDto::validation(
                "invalid_tool_result",
                "tool result content must be non-empty and free of NUL bytes",
            ));
        }
        if content.len() > MAX_TOOL_RESULT_CONTENT_BYTES {
            return Err(ErrorDto::validation(
                "invalid_tool_result",
                "tool result content exceeds the durable canonical size limit",
            ));
        }
        Ok(Self {
            session_id,
            run_id,
            call_id,
            kind,
            content,
            occurred_at,
        })
    }
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
    /// Returns the closed durable result discriminator.
    #[must_use]
    pub const fn kind(&self) -> ToolResultKindDto {
        self.kind
    }
    /// Returns the bounded canonical durable result content.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
    /// Returns the durable evidence event time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
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

/// A sender role in the DTO-only persisted model context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelContextRoleDto {
    /// A durable user turn that started a run.
    User,
    /// Final non-blank content from a completed assistant run.
    Assistant,
}

/// One non-blank DTO-only message in persisted model context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelContextMessageDto {
    role: ModelContextRoleDto,
    content: String,
}

impl ModelContextMessageDto {
    /// Creates one non-blank persisted model-context message.
    ///
    /// # Errors
    ///
    /// Returns a validation error when content is blank.
    pub fn new(role: ModelContextRoleDto, content: impl Into<String>) -> DtoResult<Self> {
        let content = content.into();
        if content.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_model_context_content",
                "model context content must not be empty",
            ));
        }
        Ok(Self { role, content })
    }

    /// Returns the message sender role.
    #[must_use]
    pub const fn role(&self) -> ModelContextRoleDto {
        self.role
    }

    /// Returns the non-blank model context content.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
}

/// DTO-only full session model context for one current starting run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartingRunModelContextDto {
    session_id: SessionId,
    run_id: RunId,
    safe_config: ConfigSnapshotDto,
    messages: Vec<ModelContextMessageDto>,
}

impl StartingRunModelContextDto {
    /// Creates coherent context whose final message is the current starting user turn.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the context is empty or does not end with a user message.
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        safe_config: ConfigSnapshotDto,
        messages: Vec<ModelContextMessageDto>,
    ) -> DtoResult<Self> {
        safe_config.validate_for_persistence()?;
        if messages
            .last()
            .is_none_or(|message| message.role() != ModelContextRoleDto::User)
        {
            return Err(ErrorDto::validation(
                "invalid_model_context",
                "starting run model context must end with its user message",
            ));
        }
        Ok(Self {
            session_id,
            run_id,
            safe_config,
            messages,
        })
    }

    /// Returns the session that owns this model context.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the current starting run that owns this context.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    /// Returns the run's immutable credential-free configuration selection.
    #[must_use]
    pub const fn safe_config(&self) -> &ConfigSnapshotDto {
        &self.safe_config
    }

    /// Returns ordered durable user and completed assistant messages.
    #[must_use]
    pub fn messages(&self) -> &[ModelContextMessageDto] {
        &self.messages
    }
}

/// The DTO-only repository contract implemented by future durable backends.
pub trait StorageRepositoryDto {
    /// Atomically appends one tool lifecycle event to the session event stream.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the backend cannot append the event or the run scope is invalid.
    fn append_tool_lifecycle_event(
        &self,
        input: AppendToolLifecycleEventInputDto,
    ) -> DtoResult<EventEnvelopeDto<DomainEventDto>> {
        let _ = input;
        Err(ErrorDto::unavailable(
            "tool_lifecycle_unavailable",
            "tool lifecycle storage is unavailable",
        ))
    }
    /// Loads typed evidence durably recorded with one terminal tool lifecycle event.
    ///
    /// # Errors
    ///
    /// Returns `tool_result_not_found` when no durable evidence exists for the
    /// supplied identity, or `tool_result_unavailable` when durable evidence
    /// cannot be read. No partial evidence, credentials, or backend resources
    /// cross this DTO-only boundary on failure.
    fn load_tool_result(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
        _call_id: ToolCallId,
    ) -> DtoResult<ToolResultEvidenceDto> {
        Err(ErrorDto::unavailable(
            "tool_result_unavailable",
            "tool result storage is unavailable",
        ))
    }
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

    /// Loads full ordered session model context for one current starting run.
    ///
    /// The returned safe immutable configuration belongs only to the target run.
    /// Messages are ordered by durable `RunStarted` sequence, contain every
    /// started user turn, contain assistant text only for completed runs with
    /// non-blank final content, and end with the target starting run's user turn.
    ///
    /// # Errors
    ///
    /// Returns `run_model_context_unavailable` when the run is unknown,
    /// cross-session, no longer starting, or durable context cannot be read.
    /// No partial context, credentials, raw TOML, configuration paths, or backend
    /// resources cross this DTO-only boundary on failure.
    fn load_starting_run_model_context(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<StartingRunModelContextDto> {
        Err(ErrorDto::unavailable(
            "run_model_context_unavailable",
            "the durable run model context is unavailable",
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
