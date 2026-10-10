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
use intention_proto::{
    CreateSessionCommandDto, MessageKindDto, MessageProjectionDto, PendingTurnProjectionDto,
    RemoveTurnCommandDto, RunProjectionDto, RunStatusDto, SessionProjectionDto, SessionSnapshotDto,
    SessionSummariesDto, WorkspaceBindingDto, WorkspaceRootDto,
};
use intention_proto::{
    DtoResult, ErrorCategoryDto, ErrorDto, ErrorRetryDto, FinishReasonDto, IdempotencyKey, RunId,
    SessionId, ThemeDto, TimestampDto, ToolCallId, UsageDto, run_status_is_terminal,
};
use serde::{Deserialize, Deserializer, Serialize, de};

mod sqlite;

pub use sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};

/// The maximum durable tool result content size in bytes.
const MAX_TOOL_RESULT_CONTENT_BYTES: usize = 512 * 1024;

/// The maximum durable user turn content size in bytes.
///
/// A turn is admitted with a bound so the session projection that carries its
/// pending rows can never be permanently larger than one transport envelope.
const MAX_TURN_CONTENT_BYTES: usize = 512 * 1024;

/// The maximum encoded pending-turn projection content kept in one session
/// projection, in bytes.
///
/// It equals the admission bound, so the newest queued turn always fits and a
/// projection always reports at least one pending turn; older turns beyond it
/// are reported through the projection's omitted count instead of being
/// materialised.
const MAX_PENDING_TURN_PROJECTION_BYTES: usize = MAX_TURN_CONTENT_BYTES;

/// The terminal outcome recorded for one local tool result.
///
/// The taxonomy is deliberately closed to terminal outcomes: a call records
/// exactly one result row, and admission, rejection, and start evidence are
/// not persisted.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultStatusDto {
    /// The tool completed and produced a normalized safe result.
    Completed,
    /// The tool reported a safe failure outcome.
    Failed,
    /// The tool stopped before a final outcome; its captured output is partial.
    Partial,
}

impl ToolResultStatusDto {
    /// Returns the canonical durable string representation of this tool result status.
    ///
    /// The representation is persisted verbatim, so it must stay byte-identical
    /// across releases.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Partial => "partial",
        }
    }

    /// Parses the canonical durable string representation of a tool result status.
    ///
    /// # Errors
    ///
    /// Returns a safe internal error when `value` is not a declared durable tool result status.
    pub fn parse(value: &str) -> DtoResult<Self> {
        match value {
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "partial" => Ok(Self::Partial),
            _ => Err(ErrorDto::new(
                "invalid_tool_result_status",
                ErrorCategoryDto::Internal,
                "the durable tool result status is not declared",
                ErrorRetryDto::Never,
                None,
            )?),
        }
    }
}

/// One credential-free structured metadata entry of a tool result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolResultMetadataEntryDto {
    key: String,
    value: String,
}

impl<'de> Deserialize<'de> for ToolResultMetadataEntryDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawToolResultMetadataEntryDto {
            key: String,
            value: String,
        }

        let raw = RawToolResultMetadataEntryDto::deserialize(deserializer)?;
        Self::new(raw.key, raw.value).map_err(de::Error::custom)
    }
}

impl ToolResultMetadataEntryDto {
    /// Creates one metadata entry with a non-blank key.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the key is blank or either field
    /// contains a NUL character.
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> DtoResult<Self> {
        let key = key.into();
        let value = value.into();
        if key.trim().is_empty() || key.contains('\0') || value.contains('\0') {
            return Err(ErrorDto::validation(
                "invalid_tool_result_metadata",
                "tool result metadata must have a non-blank key",
            ));
        }
        Ok(Self { key, value })
    }

    /// Returns the stable metadata key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Returns the metadata value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

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

/// One validated terminal run outcome, committed by a single durable transaction.
///
/// The status is always terminal. `usage` and `finish_reason` may be absent
/// when the provider reported neither, and the optional error pair carries the
/// safe code and message of a failed run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunOutcomeDto {
    status: RunStatusDto,
    usage: Option<UsageDto>,
    finish_reason: Option<FinishReasonDto>,
    error_code: Option<String>,
    error_message: Option<String>,
}

impl RunOutcomeDto {
    /// Creates one validated terminal run outcome.
    ///
    /// # Errors
    ///
    /// Returns a validation error when `status` is not terminal, or when the
    /// error pair is incomplete, blank, or contains a NUL byte.
    pub fn new(
        status: RunStatusDto,
        usage: Option<UsageDto>,
        finish_reason: Option<FinishReasonDto>,
        error_code: Option<String>,
        error_message: Option<String>,
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
            status,
            usage,
            finish_reason,
            error_code,
            error_message,
        })
    }

    /// Returns the terminal run status.
    #[must_use]
    pub const fn status(&self) -> RunStatusDto {
        self.status
    }

    /// Returns the reported model usage, when the provider reported one.
    #[must_use]
    pub const fn usage(&self) -> Option<&UsageDto> {
        self.usage.as_ref()
    }

    /// Returns the provider finish reason, when the provider reported one.
    #[must_use]
    pub const fn finish_reason(&self) -> Option<FinishReasonDto> {
        self.finish_reason
    }

    /// Returns the safe error code of a failed run, when one was recorded.
    #[must_use]
    pub fn error_code(&self) -> Option<&str> {
        self.error_code.as_deref()
    }

    /// Returns the safe error message of a failed run, when one was recorded.
    #[must_use]
    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
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
    /// Returns a validation or conflict error when the supplied creation
    /// command cannot be committed, or an unavailable error when durable
    /// storage fails.
    fn create_session(
        &self,
        command: CreateSessionCommandDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<SessionProjectionDto>;

    /// Accepts a turn and atomically records whether it starts a run or becomes a pending message.
    ///
    /// `proposed_run_id` and `config_snapshot` are committed if this turn starts
    /// immediately, or retained as the immutable future-run selection if queued.
    /// Repeating an accepted `idempotency_key` replays the current durable
    /// outcome instead of recording a second turn.
    ///
    /// # Errors
    ///
    /// Returns a validation error when content is blank or exceeds the durable
    /// turn-content bound (512 KiB), a validation, not-found, or conflict error
    /// when the turn cannot be accepted for its session, or an unavailable error
    /// when storage fails.
    fn accept_user_turn(
        &self,
        session_id: SessionId,
        idempotency_key: IdempotencyKey,
        content: &str,
        proposed_run_id: RunId,
        config_snapshot: ConfigSnapshotDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<AcceptedTurnOutcomeDto>;

    /// Removes a not-yet-seen pending turn and returns its committed projection.
    ///
    /// `occurred_at` is intentionally unrecorded: a removed pending turn keeps
    /// no durable removal time, and the caller-supplied value never enters a
    /// stored column.
    ///
    /// # Errors
    ///
    /// Returns a not-found or conflict error when the pending turn cannot be
    /// removed, or an unavailable error when durable storage fails.
    fn remove_turn(
        &self,
        command: RemoveTurnCommandDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<PendingTurnProjectionDto>;

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
        session_id: SessionId,
        run_id: RunId,
        occurred_at: TimestampDto,
    ) -> DtoResult<Vec<MessageProjectionDto>>;

    /// Transitions a run to one declared successor status.
    ///
    /// # Errors
    ///
    /// Returns a validation, not-found, or conflict error when the transition
    /// is invalid, or an unavailable error when storage fails.
    fn transition_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
        status: RunStatusDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<RunProjectionDto>;

    /// Commits one validated terminal run outcome with its usage, finish, and
    /// error evidence.
    ///
    /// The outcome is written once: repeating the exact committed outcome is
    /// idempotent, while a different terminal outcome for the same run is a
    /// typed conflict instead of a silent overwrite of the recorded evidence.
    ///
    /// # Errors
    ///
    /// Returns a not-found, conflict, or classified storage error when the
    /// outcome cannot be committed for the run. The outcome's terminal shape
    /// is validated by [`RunOutcomeDto::new`] before this call.
    fn finish_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
        outcome: RunOutcomeDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<RunProjectionDto>;

    /// Appends one transcript row and returns it as committed.
    ///
    /// # Errors
    ///
    /// Returns a validation, not-found, or conflict error when the row cannot be
    /// committed for its session and run, or an unavailable error when storage
    /// fails.
    fn append_message(
        &self,
        message: MessageProjectionDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<MessageProjectionDto>;

    /// Commits one tool result with its answering transcript row in one transaction.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the answering row is not a `tool_result`
    /// row for the same session, run, call, and tool identity, a not-found or
    /// conflict error when the scoped call cannot record its result, or an
    /// unavailable error when storage fails.
    fn write_tool_result(
        &self,
        evidence: ToolResultEvidenceDto,
        message: MessageProjectionDto,
    ) -> DtoResult<ToolResultEvidenceDto>;

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

    /// Lists the current durable sessions newest-first in one bounded window.
    ///
    /// Returns at most `limit` sessions ordered by their last durable update,
    /// newest first, with the durable session identity breaking ties, and
    /// reports how many sessions exist beyond the returned window instead of
    /// silently truncating the list.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when durable storage cannot be read.
    fn list_sessions(&self, limit: u32) -> DtoResult<SessionSummariesDto>;

    /// Returns the durable project and workspace identity bound to one root.
    ///
    /// `None` means the root carries no binding yet: the next creation for it
    /// establishes one. A bound root resolves to exactly the pair an earlier
    /// creation committed, so a caller joins the durable association instead of
    /// proposing a second identity the association would reject.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when durable storage cannot be read.
    fn workspace_binding(&self, root: &WorkspaceRootDto) -> DtoResult<Option<WorkspaceBindingDto>>;

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
    /// Every unfinished run is marked in one transaction, and the supplied time
    /// becomes each recovered run's finish time.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when recovery changes cannot be durably
    /// committed.
    fn recover_unfinished_runs(
        &self,
        recovered_at: TimestampDto,
    ) -> DtoResult<Vec<RunProjectionDto>>;

    /// Loads the current durable projection of one session together with its
    /// newest committed transcript rows in one read.
    ///
    /// The default body composes [`Self::load_session_projection`] and
    /// [`Self::load_recent_messages`]; a backend that can serve both from one
    /// committed read overrides it. Either way the returned projection and
    /// transcript rows are the committed state observed by that read.
    ///
    /// # Errors
    ///
    /// Returns the not-found, validation, or unavailable error of the
    /// underlying projection and transcript reads.
    fn load_session_snapshot(
        &self,
        session_id: SessionId,
        message_limit: u32,
    ) -> DtoResult<SessionSnapshotDto> {
        let projection = self.load_session_projection(session_id)?;
        let messages = self.load_recent_messages(session_id, message_limit)?;
        SessionSnapshotDto::with_projection(session_id, projection, messages)
    }

    /// Loads the durable terminal theme override.
    ///
    /// `None` means no override was ever stored: the caller answers from the
    /// resolved configuration instead.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when durable storage cannot be read.
    fn load_tui_theme(&self) -> DtoResult<Option<ThemeDto>>;

    /// Commits the durable terminal theme override in exactly one transaction.
    ///
    /// The settings row is a singleton, so a later set replaces the earlier
    /// value instead of recording a second row.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the override cannot be committed.
    fn save_tui_theme(&self, theme: ThemeDto) -> DtoResult<()>;

    /// Records an already credential-free configuration revision snapshot.
    ///
    /// # Errors
    ///
    /// Returns a validation or conflict error when the revision cannot be
    /// recorded, or an unavailable error when durable storage fails.
    fn accept_configuration_revision(&self, snapshot: ConfigSnapshotDto) -> DtoResult<()>;
}
