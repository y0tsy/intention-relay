//! DTO-only semantic storage contracts for durable sessions and event history.
//!
//! Implementations own transactions and backend resources. This crate exposes no
//! connection, filesystem, SQL, path, or closure-based API across its boundary.

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    CreateSessionCommandDto, DomainEventDto, LegacyM4SelectionBindingDto, ModelRunFactInputDto,
    ProviderKindDescriptorRevisionV1, ProviderProfileRevisionV1, ProviderSelectionV1,
    RemoveQueuedTurnCommandDto, RunEventCursorDto, RunEventTailPageDto, RunProjectionDto,
    RunReplayDto, RunSnapshotDto, RunStatusDto, SessionProjectionDto,
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
    selection: Option<ProviderSelectionV1>,
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
            selection: None,
        })
    }
    /// Attaches the credential-free provider selection bound to this turn's
    /// fresh run, committed in the same durable transaction as the run and its
    /// `RunStarted` event when the turn starts immediately.
    #[must_use]
    pub fn with_provider_selection(mut self, selection: ProviderSelectionV1) -> Self {
        self.selection = Some(selection);
        self
    }
    /// Returns the credential-free provider selection bound to this turn, if any.
    #[must_use]
    pub const fn provider_selection(&self) -> Option<&ProviderSelectionV1> {
        self.selection.as_ref()
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

    /// Loads every persisted credential-free configuration revision with its
    /// original persisted snapshot bytes.
    ///
    /// The snapshot bytes are the exact persisted JSON document, returned
    /// unchanged so historical replays never rewrite legacy bytes. `snapshot`
    /// carries the decoded credential-free snapshot when the persisted bytes
    /// decode and validate, and `None` when the historical record is
    /// unsupported or malformed; `snapshot_bytes_digest` always covers the
    /// original bytes. Revisions are returned in deterministic order.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the persisted revisions cannot be read.
    fn load_config_revision_records(&self) -> DtoResult<Vec<PersistedConfigRevisionRecordDto>> {
        Err(ErrorDto::unavailable(
            "config_revision_records_unavailable",
            "persisted configuration revision records are unavailable",
        ))
    }
}

/// One persisted credential-free configuration revision and its original bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedConfigRevisionRecordDto {
    /// The immutable persisted configuration revision identity.
    pub revision_id: String,
    /// The digest over the exact original persisted snapshot bytes.
    pub snapshot_bytes_digest: String,
    /// The decoded credential-free snapshot, or `None` when the historical
    /// bytes are unsupported or malformed.
    pub snapshot: Option<ConfigSnapshotDto>,
}

// ============================================================================
// Schema-4 control-plane DTOs and repository contracts.
//
// These contracts are DTO-only: no connection, SQL, path, or closure crosses
// the boundary. Record JSON columns are opaque safe strings produced and
// consumed by the backend; credentials never enter any schema-4 column.
// ============================================================================

/// The durable provider catalog lifecycle status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderCatalogStatusDto {
    /// No catalog has been accepted yet.
    Preparing,
    /// The active catalog is fully committed and serving.
    Active,
    /// A removal candidate is pending against the active catalog.
    PendingRemoval,
    /// The active catalog requires explicit activation recovery.
    ActivationRecoveryRequired,
}

/// The durable provider catalog state singleton.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderCatalogStateDto {
    /// The last accepted catalog revision, if any.
    pub active_catalog_revision_id: Option<u64>,
    /// The prepared-but-not-accepted candidate revision, if any.
    pub candidate_catalog_revision_id: Option<u64>,
    /// The closed lifecycle status.
    pub status: ProviderCatalogStatusDto,
    /// The active default profile id, if any.
    pub active_default_profile_id: Option<String>,
    /// The prepared candidate handle, if any.
    pub candidate_handle: Option<String>,
    /// The safe degraded reason, if any.
    pub degraded_reason: Option<String>,
    /// The last state update time in Unix seconds.
    pub updated_at: i64,
}

/// The readiness of one projected provider profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderReadinessDto {
    Ready,
    Disabled,
    Unavailable,
}

/// One safe projected provider profile entry in a catalog projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderCatalogProfileEntryDto {
    pub profile_id: String,
    pub profile_revision_id: String,
    pub kind_id: String,
    pub kind_descriptor_revision_id: String,
    pub display_name: Option<String>,
    pub enabled: bool,
    pub credential_configured: bool,
    pub readiness: ProviderReadinessDto,
    /// The opaque safe projection JSON produced by the backend.
    pub safe_projection_json: String,
}

/// One bounded page of a provider catalog projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderCatalogPageDto {
    pub entries: Vec<ProviderCatalogProfileEntryDto>,
    /// The opaque next-page token; `None` when the page is the last.
    pub next_token: Option<String>,
    pub has_more: bool,
}

/// A provider profile revision plus its resolved safe projection values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderProfileCandidateDto {
    /// The credential-free domain profile revision identity.
    pub profile: ProviderProfileRevisionV1,
    pub declared_model_capability_subset: Vec<String>,
    pub resolved_reasoning_policy: String,
    pub effective_execution_policy: String,
    pub effective_loopback_policy_or_not_applicable: String,
    pub display_name: Option<String>,
    pub enabled: bool,
    pub credential_configured: bool,
    pub readiness: ProviderReadinessDto,
}

/// A provider kind descriptor revision plus its explicit revision identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderKindDescriptorCandidateDto {
    pub descriptor_revision_id: String,
    pub descriptor: ProviderKindDescriptorRevisionV1,
}

/// Input appending one provider kind descriptor revision to catalog history.
///
/// Appending is the preparation step: it also marks the candidate revision in
/// the durable catalog state and records one `ProviderCatalogCandidatePrepared`
/// audit per operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendProviderKindDescriptorRevisionInputDto {
    pub descriptor_revision_id: String,
    pub descriptor: ProviderKindDescriptorRevisionV1,
    pub catalog_revision_id: u64,
    pub accepted_at: i64,
    pub operation_id: String,
}

/// Input appending one provider profile revision to catalog history.
///
/// Appending is the preparation step: it also marks the candidate revision in
/// the durable catalog state and records one `ProviderCatalogCandidatePrepared`
/// audit per operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendProviderProfileRevisionInputDto {
    pub profile: ProviderProfileCandidateDto,
    pub catalog_revision_id: u64,
    pub accepted_at: i64,
    pub operation_id: String,
}

/// Input atomically accepting one prepared provider catalog candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptProviderCatalogInputDto {
    pub catalog_revision_id: u64,
    pub candidate_handle: String,
    pub kind_descriptors: Vec<ProviderKindDescriptorCandidateDto>,
    pub profiles: Vec<ProviderProfileCandidateDto>,
    pub default_profile_id: String,
    pub accepted_at: i64,
    pub operation_id: String,
}

/// Input rejecting one prepared provider catalog candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RejectProviderCatalogCandidateInputDto {
    pub catalog_revision_id: u64,
    pub candidate_handle: String,
    pub rejected_at: i64,
    pub operation_id: String,
}

/// Input expiring one prepared provider catalog candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpireProviderCatalogCandidateInputDto {
    pub catalog_revision_id: u64,
    pub expired_at: i64,
    pub operation_id: String,
}

/// Input loading one bounded page of the active provider catalog projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadProviderCatalogPageInputDto {
    /// The opaque next-page token from a prior page, if any.
    pub token: Option<String>,
    /// The maximum number of entries to return.
    pub limit: u64,
}

/// The durable session provider default selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionProviderDefaultDto {
    pub session_id: SessionId,
    pub profile_id: String,
    pub projection_revision: u64,
    pub last_operation_id: String,
    pub updated_at: i64,
}

/// Input setting one session provider default with optimistic concurrency.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetSessionProviderProfileInputDto {
    pub session_id: SessionId,
    pub profile_id: String,
    pub expected_projection_revision: u64,
    pub operation_id: String,
    pub updated_at: i64,
}

/// The outcome of one session provider default update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SetSessionProviderProfileOutcomeDto {
    /// Whether the durable default changed.
    pub changed: bool,
    /// The resulting optimistic projection revision.
    pub projection_revision: u64,
}

/// Input atomically committing one configuration reload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitConfigurationReloadInputDto {
    pub snapshot: ConfigSnapshotDto,
    pub operation_id: String,
    pub reloaded_at: i64,
}

/// Input persisting the resolved provider selection of one fresh run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistResolvedRunProviderSelectionInputDto {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub selection: ProviderSelectionV1,
    pub occurred_at: i64,
}

/// The closed state of one unavailable provider queue entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnavailableQueueStateDto {
    Queued,
    Terminalized,
    Promoted,
}

/// One durable unavailable-provider queue entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnavailableRunQueueEntryDto {
    pub queue_id: i64,
    pub run_id: RunId,
    pub session_id: SessionId,
    pub profile_id: String,
    pub provider_profile_revision_id: String,
    pub unavailable_reason: String,
    pub first_unavailable_at: i64,
    pub promotion_attempts: u64,
    pub state: UnavailableQueueStateDto,
    pub last_operation_id: Option<String>,
    pub selection_json: String,
}

/// Input enqueueing one unavailable provider run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnqueueUnavailableRunInputDto {
    pub run_id: RunId,
    pub session_id: SessionId,
    pub profile_id: String,
    pub provider_profile_revision_id: String,
    pub unavailable_reason: String,
    pub first_unavailable_at: i64,
    pub operation_id: String,
    pub selection_json: String,
}

/// Input loading one bounded FIFO page of the unavailable queue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadUnavailableQueuePageInputDto {
    pub after_queue_id: Option<i64>,
    pub limit: u64,
}

/// Input promoting up to a bounded number of queued unavailable runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromoteUnavailableRunsInputDto {
    pub now: i64,
    pub operation_id: String,
    pub max: u64,
}

/// The outcome of one unavailable-run promotion pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromoteUnavailableRunsOutcomeDto {
    pub promoted: Vec<UnavailableRunQueueEntryDto>,
    /// Whether a reconciliation marker was created because the queue was exhausted.
    pub reconciliation_marker_created: bool,
}

/// Input reconciling up to a bounded number of queued unavailable runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconcileUnavailableQueueInputDto {
    pub now: i64,
    pub operation_id: String,
    pub max: u64,
}

/// The outcome of one unavailable-queue reconciliation pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconcileUnavailableQueueOutcomeDto {
    /// Every queued entry inspected in FIFO order.
    pub processed: Vec<UnavailableRunQueueEntryDto>,
    /// The entries whose runs reached a terminal state and were marked terminalized.
    pub terminalized: Vec<UnavailableRunQueueEntryDto>,
}

/// One durable unavailable-queue reconciliation marker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueReconciliationMarkerDto {
    pub marker_id: i64,
    pub session_id: SessionId,
    pub created_at: i64,
    pub reason: String,
    pub next_page_cursor: Option<String>,
    pub resolved_at: Option<i64>,
}

/// One credential-free provider usage event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderUsageEventInputDto {
    pub run_id: RunId,
    pub usage_event_id: String,
    pub profile_id: String,
    pub provider_profile_revision_id: String,
    pub model_id: String,
    pub input_units: u64,
    pub output_units: u64,
    pub reasoning_units: u64,
    pub occurred_at: i64,
    pub usage_json: String,
}

/// Input recording a batch of provider usage events for one usage period.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordProviderUsageInputDto {
    pub session_id: SessionId,
    pub usage_period_start: i64,
    pub usage_period_end: i64,
    pub recorded_at: i64,
    pub events: Vec<ProviderUsageEventInputDto>,
}

/// One durable provider usage aggregate for a usage period.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderUsageAggregateDto {
    pub profile_id: String,
    pub provider_profile_revision_id: String,
    pub model_id: String,
    pub usage_period_start: i64,
    pub usage_period_end: i64,
    pub request_count: u64,
    pub input_units: u64,
    pub output_units: u64,
    pub reasoning_units: u64,
    pub last_run_id: Option<RunId>,
    pub updated_at: i64,
}

/// The closed status of one provider catalog removal candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderCatalogRemovalStatusDto {
    Pending,
    Accepted,
    Rejected,
    Expired,
}

/// One durable provider catalog removal candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderCatalogRemovalCandidateDto {
    pub candidate_handle: String,
    pub candidate_catalog_revision_id: u64,
    pub active_catalog_revision_id: u64,
    pub created_at: i64,
    pub expires_at: i64,
    pub source_recheck: String,
    pub status: ProviderCatalogRemovalStatusDto,
    pub candidate_json: String,
    pub operation_id: Option<String>,
    pub completed_at: Option<i64>,
}

/// Input creating one provider catalog removal candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateProviderCatalogRemovalCandidateInputDto {
    pub candidate_handle: String,
    pub candidate_catalog_revision_id: u64,
    pub active_catalog_revision_id: u64,
    pub created_at: i64,
    pub source_recheck: String,
    pub candidate_json: String,
    pub operation_id: String,
}

/// Input accepting one pending provider catalog removal candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptProviderCatalogRemovalInputDto {
    pub candidate_handle: String,
    pub accepted_at: i64,
    pub operation_id: String,
}

/// Input rejecting one pending provider catalog removal candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RejectProviderCatalogRemovalInputDto {
    pub candidate_handle: String,
    pub rejected_at: i64,
    pub operation_id: String,
}

/// Input expiring overdue pending provider catalog removal candidates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpireProviderCatalogRemovalCandidateInputDto {
    pub now: i64,
    pub operation_id: String,
}

/// The closed admission state of one held recovered run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeldRunAdmissionStateDto {
    Held,
    Admitted,
    Rejected,
}

/// One durable held recovered run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeldRecoveredRunDto {
    pub run_id: RunId,
    pub session_id: SessionId,
    pub held_at: i64,
    pub reason: String,
    pub admission_state: HeldRunAdmissionStateDto,
    pub admission_operation_id: Option<String>,
    pub admitted_at: Option<i64>,
}

/// Input marking one recovered run as held pending explicit admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkRecoveredRunHeldInputDto {
    pub run_id: RunId,
    pub session_id: SessionId,
    pub held_at: i64,
    pub operation_id: String,
}

/// Input admitting one held recovered run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmitHeldRecoveredRunInputDto {
    pub run_id: RunId,
    pub session_id: SessionId,
    pub admitted_at: i64,
    pub operation_id: String,
}

/// The closed validation status of one legacy M4 selection binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegacyBindingValidationStatusDto {
    Validated,
    Corrupt,
}

/// Input appending one deterministic legacy M4 selection binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendLegacyM4SelectionBindingInputDto {
    pub config_revision_id: String,
    /// The valid legacy binding, or `None` when the historical selection is
    /// unsupported or malformed and must be recorded as corrupt.
    pub binding: Option<LegacyM4SelectionBindingDto>,
    pub snapshot_bytes_digest: String,
    pub validation_status: LegacyBindingValidationStatusDto,
    pub created_at: i64,
}

/// One durable legacy M4 selection binding record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyM4SelectionBindingRecordDto {
    pub config_revision_id: String,
    pub profile_id: String,
    pub provider_profile_revision_id: String,
    pub kind_id: String,
    pub kind_descriptor_revision_id: String,
    pub provider_driver_contract_revision: String,
    pub binding_digest: String,
    pub snapshot_bytes_digest: String,
    pub validation_status: LegacyBindingValidationStatusDto,
    pub binding_json: String,
    pub created_at: i64,
}

/// DTO-only repository contract for the provider catalog control plane.
pub trait ProviderCatalogRepositoryDto {
    /// Appends one provider kind descriptor revision to append-only catalog history.
    ///
    /// # Errors
    ///
    /// Returns a conflict error when the descriptor digest is already bound to
    /// a different kind identity, or an unavailable error when storage fails.
    fn append_provider_kind_descriptor_revision(
        &self,
        input: AppendProviderKindDescriptorRevisionInputDto,
    ) -> DtoResult<()>;
    /// Appends one provider profile revision to append-only catalog history.
    ///
    /// # Errors
    ///
    /// Returns a conflict error when the profile digest is already bound to a
    /// different profile identity, or an unavailable error when storage fails.
    fn append_provider_profile_revision(
        &self,
        input: AppendProviderProfileRevisionInputDto,
    ) -> DtoResult<()>;
    /// Loads the durable provider catalog state singleton.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the state cannot be read.
    fn load_provider_catalog_status(&self) -> DtoResult<ProviderCatalogStateDto>;
    /// Loads one bounded page of the active provider catalog projection.
    ///
    /// # Errors
    ///
    /// Returns a validation error for an unusable token or limit, or an
    /// unavailable error when the projection cannot be read.
    fn load_provider_catalog_page(
        &self,
        input: LoadProviderCatalogPageInputDto,
    ) -> DtoResult<ProviderCatalogPageDto>;
    /// Atomically accepts one prepared provider catalog candidate.
    ///
    /// # Errors
    ///
    /// Returns a conflict error when the candidate revision does not match the
    /// prepared state, or an unavailable error when the atomic commit fails.
    fn accept_provider_catalog(&self, input: AcceptProviderCatalogInputDto) -> DtoResult<()>;
    /// Rejects one prepared provider catalog candidate.
    ///
    /// # Errors
    ///
    /// Returns a not-found or conflict error when the candidate is not the
    /// prepared candidate, or an unavailable error when storage fails.
    fn reject_provider_catalog_candidate(
        &self,
        input: RejectProviderCatalogCandidateInputDto,
    ) -> DtoResult<()>;
    /// Expires one prepared provider catalog candidate.
    ///
    /// # Errors
    ///
    /// Returns a not-found or conflict error when the candidate is not the
    /// prepared candidate, or an unavailable error when storage fails.
    fn expire_provider_catalog_candidate(
        &self,
        input: ExpireProviderCatalogCandidateInputDto,
    ) -> DtoResult<()>;
    /// Loads the full accepted provider catalog material for the active
    /// catalog: every kind descriptor revision and profile revision plus the
    /// active default profile id. This is the startup reconstruction surface
    /// for the private registry.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_not_active` when no active catalog is
    /// committed, or an unavailable error when the material cannot be read.
    fn load_provider_catalog_material(&self) -> DtoResult<ProviderCatalogMaterialDto> {
        Err(ErrorDto::unavailable(
            "provider_catalog_material_unavailable",
            "the accepted provider catalog material is unavailable",
        ))
    }
}

/// The full accepted provider catalog material for private-registry
/// reconstruction at startup. Credential-free by construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderCatalogMaterialDto {
    /// The committed active catalog revision.
    pub catalog_revision_id: u64,
    /// The active default profile id, when one exists.
    pub default_profile_id: Option<String>,
    /// Every kind descriptor revision accepted with the active catalog.
    pub kind_descriptors: Vec<ProviderKindDescriptorCandidateDto>,
    /// Every profile revision accepted with the active catalog.
    pub profiles: Vec<ProviderProfileCandidateDto>,
}

/// DTO-only repository contract for atomic configuration reloads.
pub trait ConfigurationReloadRepositoryDto {
    /// Atomically commits one configuration reload: the candidate snapshot,
    /// its audit row, and the active state update in one transaction.
    ///
    /// # Errors
    ///
    /// Returns a validation or conflict error when the snapshot cannot be
    /// persisted, or an unavailable error when the atomic commit fails.
    fn commit_configuration_reload(
        &self,
        input: CommitConfigurationReloadInputDto,
    ) -> DtoResult<()>;
}

/// DTO-only repository contract for session provider defaults.
pub trait SessionProviderDefaultsRepositoryDto {
    /// Loads the durable provider profile default for one session, if any.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the default cannot be read.
    fn get_session_provider_profile(
        &self,
        session_id: SessionId,
    ) -> DtoResult<Option<SessionProviderDefaultDto>>;
    /// Sets the durable provider profile default for one session with
    /// optimistic projection-revision concurrency and idempotent operations.
    ///
    /// # Errors
    ///
    /// Returns a conflict error when the expected projection revision is stale
    /// or the operation already bound a different profile, or an unavailable
    /// error when storage fails.
    fn set_session_provider_profile(
        &self,
        input: SetSessionProviderProfileInputDto,
    ) -> DtoResult<SetSessionProviderProfileOutcomeDto>;
}

/// DTO-only repository contract for resolved run provider selections.
pub trait ProviderSelectionRepositoryDto {
    /// Persists the resolved provider selection of one fresh run.
    ///
    /// # Errors
    ///
    /// Returns a conflict error when the run already bound a different
    /// selection, or an unavailable error when storage fails.
    fn persist_resolved_run_provider_selection(
        &self,
        input: PersistResolvedRunProviderSelectionInputDto,
    ) -> DtoResult<()>;
    /// Loads the persisted resolved provider selection of one run, if any.
    ///
    /// Returns `None` when the run has no persisted selection or the supplied
    /// session does not own the run.
    ///
    /// # Errors
    ///
    /// Returns `storage_decode_failed` when the persisted record is malformed,
    /// or an unavailable error when storage cannot be read.
    fn load_resolved_run_provider_selection(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<Option<ProviderSelectionV1>>;
}

/// DTO-only repository contract for the unavailable-provider queue.
pub trait UnavailableQueueRepositoryDto {
    /// Enqueues one unavailable provider run idempotently.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when storage fails.
    fn enqueue_unavailable_run(&self, input: EnqueueUnavailableRunInputDto) -> DtoResult<()>;
    /// Loads one bounded FIFO page of queued unavailable runs.
    ///
    /// # Errors
    ///
    /// Returns a validation error for an unusable limit, or an unavailable
    /// error when the queue cannot be read.
    fn load_unavailable_queue_page(
        &self,
        input: LoadUnavailableQueuePageInputDto,
    ) -> DtoResult<Vec<UnavailableRunQueueEntryDto>>;
    /// Promotes up to `max` queued unavailable runs in FIFO order.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the promotion cannot be committed.
    fn promote_unavailable_runs(
        &self,
        input: PromoteUnavailableRunsInputDto,
    ) -> DtoResult<PromoteUnavailableRunsOutcomeDto>;
    /// Reconciles up to `max` queued unavailable runs against their durable
    /// run states, marking terminalized entries.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the reconciliation cannot be committed.
    fn reconcile_unavailable_queue(
        &self,
        input: ReconcileUnavailableQueueInputDto,
    ) -> DtoResult<ReconcileUnavailableQueueOutcomeDto>;
    /// Loads the latest unresolved reconciliation marker for one session, if any.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the marker cannot be read.
    fn load_queue_reconciliation_marker(
        &self,
        session_id: SessionId,
    ) -> DtoResult<Option<QueueReconciliationMarkerDto>>;
}

/// DTO-only repository contract for provider usage aggregates and facts.
pub trait ProviderUsageRepositoryDto {
    /// Records a batch of provider usage events with stable event identity and
    /// no double counting, and updates the matching usage aggregates.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the usage cannot be committed.
    fn record_provider_usage(&self, input: RecordProviderUsageInputDto) -> DtoResult<()>;
    /// Loads usage aggregates for one provider profile.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the aggregates cannot be read.
    fn load_provider_usage_by_profile(
        &self,
        profile_id: String,
    ) -> DtoResult<Vec<ProviderUsageAggregateDto>>;
    /// Loads usage aggregates for one provider profile revision and model.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the aggregates cannot be read.
    fn load_provider_usage_by_revision_and_model(
        &self,
        provider_profile_revision_id: String,
        model_id: String,
    ) -> DtoResult<Vec<ProviderUsageAggregateDto>>;
}

/// DTO-only repository contract for the provider catalog removal lifecycle.
pub trait ProviderRemovalRepositoryDto {
    /// Creates one provider catalog removal candidate with a thirty-minute
    /// lifetime and at most one pending candidate.
    ///
    /// # Errors
    ///
    /// Returns a conflict error when a pending candidate already exists, or an
    /// unavailable error when storage fails.
    fn create_provider_catalog_removal_candidate(
        &self,
        input: CreateProviderCatalogRemovalCandidateInputDto,
    ) -> DtoResult<()>;
    /// Accepts one pending provider catalog removal candidate.
    ///
    /// # Errors
    ///
    /// Returns a not-found or conflict error when the candidate is not pending,
    /// or an unavailable error when storage fails.
    fn accept_provider_catalog_removal(
        &self,
        input: AcceptProviderCatalogRemovalInputDto,
    ) -> DtoResult<()>;
    /// Rejects one pending provider catalog removal candidate.
    ///
    /// # Errors
    ///
    /// Returns a not-found or conflict error when the candidate is not pending,
    /// or an unavailable error when storage fails.
    fn reject_provider_catalog_removal(
        &self,
        input: RejectProviderCatalogRemovalInputDto,
    ) -> DtoResult<()>;
    /// Expires pending removal candidates past their lifetime and returns the
    /// number expired.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when storage fails.
    fn expire_provider_catalog_removal_candidate(
        &self,
        input: ExpireProviderCatalogRemovalCandidateInputDto,
    ) -> DtoResult<u64>;
}

/// DTO-only repository contract for held recovered runs.
pub trait HeldRunRepositoryDto {
    /// Marks one recovered run as held pending explicit admission.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the held record cannot be committed.
    fn mark_recovered_run_held(&self, input: MarkRecoveredRunHeldInputDto) -> DtoResult<()>;
    /// Admits one held recovered run idempotently without creating a second task.
    ///
    /// # Errors
    ///
    /// Returns a not-found or conflict error when the run is not held or was
    /// already rejected, or an unavailable error when storage fails.
    fn admit_held_recovered_run(&self, input: AdmitHeldRecoveredRunInputDto) -> DtoResult<()>;
    /// Loads the held recovered run record for one run, if any.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the record cannot be read.
    fn load_held_recovered_run(&self, run_id: RunId) -> DtoResult<Option<HeldRecoveredRunDto>>;
}

/// DTO-only repository contract for legacy M4 selection bindings.
pub trait LegacyBindingRepositoryDto {
    /// Appends one deterministic legacy M4 selection binding idempotently.
    ///
    /// # Errors
    ///
    /// Returns a conflict error when the revision already bound a different
    /// binding, or an unavailable error when storage fails.
    fn append_legacy_m4_selection_binding(
        &self,
        input: AppendLegacyM4SelectionBindingInputDto,
    ) -> DtoResult<()>;
    /// Loads the durable legacy M4 selection binding for one config revision, if any.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the binding cannot be read.
    fn load_legacy_m4_selection_binding(
        &self,
        config_revision_id: String,
    ) -> DtoResult<Option<LegacyM4SelectionBindingRecordDto>>;
}
