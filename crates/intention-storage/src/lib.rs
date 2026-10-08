//! DTO-only semantic storage contracts for durable sessions and transcript state.
//!
//! The SQLite backend lives behind this same boundary as the private `sqlite`
//! module, surfaced only through the DTO-only repository contract below.
//!
//! Implementations own transactions and backend resources. This crate exposes no
//! connection, filesystem, SQL, path, or closure-based API across its boundary.
//!
//! Every state-changing method commits its change in exactly one transaction or
//! changes nothing, and returns the committed values. There is no event log, no
//! snapshot, no cursor, and no replay: a later read observes only committed
//! state, and a caller publishes from the values a commit returned.
//!
//! Every field is a typed domain or storage value. No opaque JSON string,
//! untyped map, or caller-supplied encoding crosses this boundary: a backend
//! that persists a record as text owns both directions of its canonical codec
//! behind the typed field, and only the typed value is visible to callers.

use intention_config::ConfigSnapshotDto;
use intention_domain::{ToolResultMetadataEntryDto, ToolResultStatusDto, run_status_is_terminal};
use intention_proto::{
    ConfigRevisionId, DtoResult, ErrorDto, FinishReasonDto, IdempotencyKey, RunId, SessionId,
    TimestampDto, ToolCallId, UsageDto,
};
use intention_proto::{
    CreateSessionCommandDto, MessageKindDto, MessageProjectionDto, PendingTurnProjectionDto,
    RemoveTurnCommandDto, RunProjectionDto, RunStatusDto, SessionProjectionDto,
};

mod sqlite;

pub use sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};

/// The maximum durable tool result content size in bytes.
const MAX_TOOL_RESULT_CONTENT_BYTES: usize = 512 * 1024;

/// Typed durable evidence of one committed local tool result.
///
/// `content` carries the bounded canonical projection of the typed result as
/// selected by the caller; it never carries credentials, absolute workspace
/// roots, or backend resources. `metadata` carries approved credential-free
/// structured entries such as truncation or process-exit evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolResultEvidenceDto {
    session_id: SessionId,
    run_id: RunId,
    call_id: ToolCallId,
    tool_id: String,
    status: ToolResultStatusDto,
    content: String,
    metadata: Vec<ToolResultMetadataEntryDto>,
    occurred_at: TimestampDto,
}

impl ToolResultEvidenceDto {
    /// Creates typed tool result evidence with bounded canonical content.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the tool identity is blank, the content
    /// is blank, oversized, or contains interior NUL bytes, or the metadata keys
    /// are duplicated.
    #[expect(
        clippy::too_many_arguments,
        reason = "The flat durable tool-result payload keeps one validating constructor."
    )]
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        call_id: ToolCallId,
        tool_id: impl Into<String>,
        status: ToolResultStatusDto,
        content: impl Into<String>,
        metadata: Vec<ToolResultMetadataEntryDto>,
        occurred_at: TimestampDto,
    ) -> DtoResult<Self> {
        let tool_id = tool_id.into();
        let content = content.into();
        if tool_id.trim().is_empty() || content.trim().is_empty() || content.contains('\0') {
            return Err(ErrorDto::validation(
                "invalid_tool_result",
                "tool result needs a tool identity and non-empty safe content",
            ));
        }
        if content.len() > MAX_TOOL_RESULT_CONTENT_BYTES {
            return Err(ErrorDto::validation(
                "invalid_tool_result",
                "tool result content exceeds the durable canonical size limit",
            ));
        }
        let mut seen_keys = std::collections::HashSet::with_capacity(metadata.len());
        if metadata.iter().any(|entry| !seen_keys.insert(entry.key())) {
            return Err(ErrorDto::validation(
                "invalid_tool_result_metadata",
                "tool result metadata keys must be unique",
            ));
        }
        Ok(Self {
            session_id,
            run_id,
            call_id,
            tool_id,
            status,
            content,
            metadata,
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
    /// Returns the bounded canonical durable result content.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
    /// Returns the approved structured metadata entries.
    #[must_use]
    pub fn metadata(&self) -> &[ToolResultMetadataEntryDto] {
        &self.metadata
    }
    /// Returns the durable evidence event time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Inputs required to create one durable session.
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
    /// Returns the externally selected creation time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Inputs required to durably accept a turn, including its possible first-run identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptUserTurnInputDto {
    session_id: SessionId,
    idempotency_key: IdempotencyKey,
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
    /// Repeating an accepted `idempotency_key` replays the current durable
    /// outcome instead of recording a second turn.
    ///
    /// # Errors
    ///
    /// Returns a validation error when content is blank or the safe snapshot is
    /// not suitable for persistence.
    pub fn new(
        session_id: SessionId,
        idempotency_key: IdempotencyKey,
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
            idempotency_key,
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
    /// Returns the caller-supplied repeatable-operation identity.
    #[must_use]
    pub const fn idempotency_key(&self) -> IdempotencyKey {
        self.idempotency_key
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
    /// Returns the externally selected acceptance time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Inputs required to remove a pending turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoveTurnInputDto {
    command: RemoveTurnCommandDto,
    occurred_at: TimestampDto,
}

impl RemoveTurnInputDto {
    /// Creates typed pending-turn removal storage input.
    #[must_use]
    pub const fn new(command: RemoveTurnCommandDto, occurred_at: TimestampDto) -> Self {
        Self {
            command,
            occurred_at,
        }
    }
    /// Returns the typed removal command.
    #[must_use]
    pub const fn command(self) -> RemoveTurnCommandDto {
        self.command
    }
    /// Returns the externally selected removal time.
    #[must_use]
    pub const fn occurred_at(self) -> TimestampDto {
        self.occurred_at
    }
}

/// Inputs required to append every pending user turn to one active run context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsumePendingUserTurnsInputDto {
    session_id: SessionId,
    run_id: RunId,
    occurred_at: TimestampDto,
}

impl ConsumePendingUserTurnsInputDto {
    /// Creates a pending-turn join request for one active run.
    #[must_use]
    pub const fn new(session_id: SessionId, run_id: RunId, occurred_at: TimestampDto) -> Self {
        Self {
            session_id,
            run_id,
            occurred_at,
        }
    }
    /// Returns the owning session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }
    /// Returns the target active run identity.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }
    /// Returns the selected commit time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Immutable outcome for an accepted user turn after committing it durably.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcceptedTurnOutcomeDto {
    /// The turn began a newly created run and committed its user message.
    Started {
        /// The committed run projection.
        run: RunProjectionDto,
        /// The committed user message that starts the run context.
        message: MessageProjectionDto,
    },
    /// The turn is a pending message that joins the active run context later.
    Pending(PendingTurnProjectionDto),
}

impl AcceptedTurnOutcomeDto {
    /// Returns the committed run projection when the turn started a run.
    #[must_use]
    pub const fn started_run(&self) -> Option<RunProjectionDto> {
        match self {
            Self::Started { run, .. } => Some(*run),
            Self::Pending(_) => None,
        }
    }
    /// Returns the committed user message when the turn started a run.
    #[must_use]
    pub const fn started_message(&self) -> Option<&MessageProjectionDto> {
        match self {
            Self::Started { message, .. } => Some(message),
            Self::Pending(_) => None,
        }
    }
    /// Returns the committed pending turn when the turn was queued.
    #[must_use]
    pub const fn pending_turn(&self) -> Option<&PendingTurnProjectionDto> {
        match self {
            Self::Started { .. } => None,
            Self::Pending(turn) => Some(turn),
        }
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
    /// Returns the externally selected transition time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Inputs required to commit one terminal run outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinishRunInputDto {
    session_id: SessionId,
    run_id: RunId,
    status: RunStatusDto,
    usage: Option<UsageDto>,
    finish_reason: Option<FinishReasonDto>,
    error_code: Option<String>,
    error_message: Option<String>,
    occurred_at: TimestampDto,
}

impl FinishRunInputDto {
    /// Creates a terminal run-outcome commit request.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the status is not terminal, when the
    /// error pair is incomplete or blank, or when a carried value contains a NUL
    /// byte.
    #[expect(
        clippy::too_many_arguments,
        reason = "The flat terminal-outcome payload keeps one validating constructor."
    )]
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        status: RunStatusDto,
        usage: Option<UsageDto>,
        finish_reason: Option<FinishReasonDto>,
        error_code: Option<String>,
        error_message: Option<String>,
        occurred_at: TimestampDto,
    ) -> DtoResult<Self> {
        if !run_status_is_terminal(status) {
            return Err(ErrorDto::validation(
                "invalid_run_outcome",
                "a terminal run outcome needs a terminal status",
            ));
        }
        let error_valid = match (&error_code, &error_message) {
            (None, None) => true,
            (Some(code), Some(message)) => {
                !code.trim().is_empty()
                    && !message.trim().is_empty()
                    && !code.contains('\0')
                    && !message.contains('\0')
            }
            _ => false,
        };
        if !error_valid {
            return Err(ErrorDto::validation(
                "invalid_run_outcome",
                "a terminal run outcome carries either both error fields or neither",
            ));
        }
        Ok(Self {
            session_id,
            run_id,
            status,
            usage,
            finish_reason,
            error_code,
            error_message,
            occurred_at,
        })
    }
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
    /// Returns the externally selected completion time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Inputs required to commit one transcript row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendMessageInputDto {
    message: MessageProjectionDto,
    occurred_at: TimestampDto,
}

impl AppendMessageInputDto {
    /// Creates one transcript-append input.
    #[must_use]
    pub const fn new(message: MessageProjectionDto, occurred_at: TimestampDto) -> Self {
        Self {
            message,
            occurred_at,
        }
    }
    /// Returns the validated transcript row to commit.
    #[must_use]
    pub const fn message(&self) -> &MessageProjectionDto {
        &self.message
    }
    /// Returns the externally selected commit time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Inputs required to commit one tool result with its answering transcript row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteToolResultInputDto {
    evidence: ToolResultEvidenceDto,
    message: MessageProjectionDto,
}

impl WriteToolResultInputDto {
    /// Creates one tool-result commit request.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the answering message is not a
    /// `tool_result` row for the same session, run, call, and tool identity.
    pub fn new(evidence: ToolResultEvidenceDto, message: MessageProjectionDto) -> DtoResult<Self> {
        if message.kind() != MessageKindDto::ToolResult
            || message.session_id() != evidence.session_id()
            || message.run_id() != Some(evidence.run_id())
            || message.tool_call_id() != Some(evidence.call_id())
            || message.tool_id() != Some(evidence.tool_id())
        {
            return Err(ErrorDto::validation(
                "invalid_tool_result",
                "a tool result commits with its own answering transcript row",
            ));
        }
        Ok(Self { evidence, message })
    }
    /// Returns the typed durable evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ToolResultEvidenceDto {
        &self.evidence
    }
    /// Returns the answering transcript row committed in the same transaction.
    #[must_use]
    pub const fn message(&self) -> &MessageProjectionDto {
        &self.message
    }
}

/// Inputs required to mark all persisted unfinished runs interrupted at recovery time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoverUnfinishedRunsInputDto {
    recovered_at: TimestampDto,
}

impl RecoverUnfinishedRunsInputDto {
    /// Creates recovery input with an explicit commit time.
    #[must_use]
    pub const fn new(recovered_at: TimestampDto) -> Self {
        Self { recovered_at }
    }
    /// Returns the recovery time.
    #[must_use]
    pub const fn recovered_at(self) -> TimestampDto {
        self.recovered_at
    }
}

/// DTO-only full session model context for one current starting run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartingRunModelContextDto {
    session_id: SessionId,
    run_id: RunId,
    safe_config: ConfigSnapshotDto,
    messages: Vec<MessageProjectionDto>,
}

impl StartingRunModelContextDto {
    /// Creates context whose final message is the current starting user turn.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the configuration is not persistable,
    /// the context is empty, a message belongs to another session, or the final
    /// message is not the starting run's user turn.
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        safe_config: ConfigSnapshotDto,
        messages: Vec<MessageProjectionDto>,
    ) -> DtoResult<Self> {
        safe_config.validate_for_persistence()?;
        let ends_with_user = messages.last().is_some_and(|message| {
            message.kind() == MessageKindDto::User && message.run_id() == Some(run_id)
        });
        if messages
            .iter()
            .any(|message| message.session_id() != session_id)
            || !ends_with_user
        {
            return Err(ErrorDto::validation(
                "invalid_model_context",
                "starting run model context must end with its session's starting user turn",
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

    /// Returns ordered committed transcript rows in insertion order.
    #[must_use]
    pub fn messages(&self) -> &[MessageProjectionDto] {
        &self.messages
    }
}

/// The DTO-only current-state storage contract implemented by durable backends.
pub trait StorageRepositoryDto {
    /// Creates a session and returns its committed current-state projection.
    ///
    /// # Errors
    ///
    /// Returns a validation or conflict error when the supplied creation input
    /// cannot be committed, or an unavailable error when durable storage fails.
    fn create_session(&self, input: CreateSessionInputDto) -> DtoResult<SessionProjectionDto>;

    /// Accepts a turn and atomically records whether it starts a run or becomes a pending message.
    ///
    /// # Errors
    ///
    /// Returns a validation, not-found, or conflict error when the turn cannot
    /// be accepted for its session, or an unavailable error when storage fails.
    fn accept_user_turn(&self, input: AcceptUserTurnInputDto) -> DtoResult<AcceptedTurnOutcomeDto>;

    /// Removes a not-yet-seen pending turn and returns its committed projection.
    ///
    /// # Errors
    ///
    /// Returns a not-found or conflict error when the pending turn cannot be
    /// removed, or an unavailable error when durable storage fails.
    fn remove_turn(&self, input: RemoveTurnInputDto) -> DtoResult<PendingTurnProjectionDto>;

    /// Appends every pending user turn to one active run context in insertion order.
    ///
    /// Every consumed turn becomes `appended` in the same transaction that
    /// commits its user message, so a turn joins exactly one run context.
    ///
    /// # Errors
    ///
    /// Returns a conflict, not-found, or unavailable error when the scoped run
    /// cannot join its pending messages atomically.
    fn consume_pending_user_turns(
        &self,
        input: ConsumePendingUserTurnsInputDto,
    ) -> DtoResult<Vec<MessageProjectionDto>>;

    /// Transitions a run to one declared successor status.
    ///
    /// # Errors
    ///
    /// Returns a validation, not-found, or conflict error when the transition
    /// is invalid, or an unavailable error when storage fails.
    fn transition_run(&self, input: TransitionRunInputDto) -> DtoResult<RunProjectionDto>;

    /// Commits one terminal run outcome with its usage, finish, and error evidence.
    ///
    /// # Errors
    ///
    /// Returns a validation, not-found, or conflict error when the outcome is
    /// invalid for the run, or an unavailable error when storage fails.
    fn finish_run(&self, input: FinishRunInputDto) -> DtoResult<RunProjectionDto>;

    /// Appends one transcript row and returns it as committed.
    ///
    /// # Errors
    ///
    /// Returns a validation, not-found, or conflict error when the row cannot be
    /// committed for its session and run, or an unavailable error when storage
    /// fails.
    fn append_message(&self, input: AppendMessageInputDto) -> DtoResult<MessageProjectionDto>;

    /// Commits one tool result with its answering transcript row in one transaction.
    ///
    /// # Errors
    ///
    /// Returns a validation, not-found, or conflict error when the scoped call
    /// cannot record its result, or an unavailable error when storage fails.
    fn write_tool_result(&self, input: WriteToolResultInputDto)
    -> DtoResult<ToolResultEvidenceDto>;

    /// Loads typed evidence durably recorded for one tool call.
    ///
    /// # Errors
    ///
    /// Returns `tool_result_not_found` when no durable evidence exists for the
    /// supplied identity, or `tool_result_unavailable` when durable evidence
    /// cannot be read. No partial evidence, credentials, or backend resources
    /// cross this DTO-only boundary on failure.
    fn load_tool_result(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call_id: ToolCallId,
    ) -> DtoResult<ToolResultEvidenceDto>;

    /// Loads the persisted credential-free configuration selected for a matching run.
    ///
    /// # Errors
    ///
    /// Returns `run_configuration_not_found` only when the durable run row is
    /// genuinely absent for the supplied identity, `storage_unavailable` when the
    /// durable backend cannot be read at all, `run_configuration_unavailable`
    /// when the persisted safe selection row is absent, and
    /// `storage_decode_failed` when the selection is present but cannot be
    /// decoded. Credentials, raw TOML, configuration paths, and backend
    /// resources never cross this boundary.
    fn load_run_config_snapshot(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<ConfigSnapshotDto>;

    /// Loads the full ordered model context for one current starting run.
    ///
    /// The returned safe immutable configuration belongs only to the target run.
    /// Messages are the committed transcript rows of the session in insertion
    /// order and end with the target run's user turn.
    ///
    /// # Errors
    ///
    /// Returns `run_model_context_unavailable` when the run is unknown,
    /// cross-session, no longer starting, or durable context cannot be read.
    /// No partial context, credentials, raw TOML, configuration paths, or backend
    /// resources cross this DTO-only boundary on failure.
    fn load_starting_run_model_context(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<StartingRunModelContextDto>;

    /// Loads the current durable projection of one run.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for an unknown or cross-session run, or an
    /// unavailable error when storage cannot be read.
    fn load_run_projection(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunProjectionDto>;

    /// Loads the current durable projection of one session.
    ///
    /// # Errors
    ///
    /// Returns a not-found error when the session has no durable projection, or
    /// an unavailable error when storage cannot be read.
    fn load_session_projection(&self, session_id: SessionId) -> DtoResult<SessionProjectionDto>;

    /// Loads the most recent committed transcript rows of one session.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for an unknown session, or an unavailable error
    /// when storage cannot be read.
    fn load_recent_messages(
        &self,
        session_id: SessionId,
        limit: u32,
    ) -> DtoResult<Vec<MessageProjectionDto>>;

    /// Loads the committed transcript rows of one run in insertion order.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for an unknown or cross-session run, or an
    /// unavailable error when storage cannot be read.
    fn load_run_messages(
        &self,
        session_id: SessionId,
        run_id: RunId,
        limit: u32,
    ) -> DtoResult<Vec<MessageProjectionDto>>;

    /// Marks persisted unfinished runs interrupted at the supplied time.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when recovery changes cannot be durably
    /// committed.
    fn recover_unfinished_runs(
        &self,
        input: RecoverUnfinishedRunsInputDto,
    ) -> DtoResult<Vec<RunProjectionDto>>;

    /// Records an already credential-free configuration revision snapshot.
    ///
    /// # Errors
    ///
    /// Returns a validation or conflict error when the revision cannot be
    /// recorded, or an unavailable error when durable storage fails.
    fn accept_configuration_revision(&self, snapshot: ConfigSnapshotDto) -> DtoResult<()>;
}
