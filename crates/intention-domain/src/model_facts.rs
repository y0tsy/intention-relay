//! Durable M4 model-fact DTOs, events, projections, and replay values.

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::reasoning_history::ReasoningDeltaCategory;
use intention_types::{
    AssistantTurnId, CorrelationIdDto, DtoResult, ErrorDto, ErrorRetryDto, FinishReasonDto,
    ProviderErrorDto, RunId, SessionEventSequenceDto, SessionId, TimestampDto, ToolCallDto,
    ToolCallId, UsageDto,
};

const MAX_ASSISTANT_CONTENT_BYTES: usize = 4 * 1024;
const MAX_REASONING_FACT_BYTES: usize = 512 * 1024;
const MAX_TAIL_FACTS: usize = 256;

// The canonical reasoning category type is codec-only in `reasoning_history`;
// its serde support lives here because the durable fact DTOs are the only
// serde consumers of the shared category type.
impl Serialize for ReasoningDeltaCategory {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(match self {
            Self::Primary => "primary",
            Self::Detail => "detail",
        })
    }
}

impl<'de> Deserialize<'de> for ReasoningDeltaCategory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match String::deserialize(deserializer)?.as_str() {
            "primary" => Ok(Self::Primary),
            "detail" => Ok(Self::Detail),
            _ => Err(de::Error::custom("invalid reasoning delta category")),
        }
    }
}

fn non_blank(value: String, code: &'static str, message: &'static str) -> DtoResult<String> {
    if value.trim().is_empty() {
        Err(ErrorDto::validation(code, message))
    } else {
        Ok(value)
    }
}

fn positive_attempt(attempt: u16) -> DtoResult<()> {
    if attempt == 0 {
        Err(ErrorDto::validation(
            "invalid_provider_attempt",
            "provider attempt must be positive",
        ))
    } else {
        Ok(())
    }
}

/// Validates one reasoning fact payload against the closed per-fact bound.
///
/// # Errors
///
/// Returns a validation error when content is blank or exceeds 512 KiB.
fn validate_reasoning_fact_content(content: String) -> DtoResult<String> {
    let content = non_blank(
        content,
        "invalid_reasoning_delta",
        "reasoning delta must not be empty",
    )?;
    if content.len() > MAX_REASONING_FACT_BYTES {
        return Err(ErrorDto::validation(
            "invalid_reasoning_delta",
            "reasoning delta must not exceed 512 KiB",
        ));
    }
    Ok(content)
}

/// Validates that appending one reasoning fact keeps the combined per-run
/// reasoning output within the closed 4 MiB bound.
///
/// # Errors
///
/// Returns a validation error when the combined reasoning output would exceed
/// 4 MiB, or when the byte counts overflow.
pub fn validate_reasoning_output_bound(current_bytes: u64, next_bytes: u64) -> DtoResult<()> {
    let exceeds = current_bytes
        .checked_add(next_bytes)
        .is_none_or(|combined| combined > crate::reasoning_history::MAX_REASONING_AGGREGATE_BYTES);
    if exceeds {
        return Err(ErrorDto::validation(
            "reasoning_output_limit_exceeded",
            "combined reasoning output must not exceed 4 MiB per run",
        ));
    }
    Ok(())
}

/// An ordered, zero-based durable model-fact position within one run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RunEventCursorDto(u64);

impl RunEventCursorDto {
    /// Creates an explicit durable model-fact cursor.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the ordered cursor value.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// The closed taxonomy of durable model facts.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRunFactKindDto {
    /// A selected provider attempt started.
    ProviderAttemptStarted,
    /// A selected provider attempt failed safely.
    ProviderAttemptFailed,
    /// The runtime scheduled the next provider attempt.
    RetryScheduled,
    /// Assistant text was appended under one stable assistant turn.
    AssistantContentAppended,
    /// A reasoning delta was recorded only in the event tail.
    ReasoningDeltaRecorded,
    /// A reasoning summary delta was recorded only in the event tail.
    ReasoningSummaryDeltaRecorded,
    /// Provider-normalized usage was recorded.
    UsageRecorded,
    /// A provider-normalized tool call was recorded.
    ToolCallRecorded,
    /// A tool result was durably recorded for one tool call.
    ToolResultRecorded,
    /// The provider reported a terminal finish reason.
    Finished,
    /// The run recorded a safe terminal failure.
    Failed,
}

impl ModelRunFactKindDto {
    /// Returns the stable wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProviderAttemptStarted => "provider_attempt_started",
            Self::ProviderAttemptFailed => "provider_attempt_failed",
            Self::RetryScheduled => "retry_scheduled",
            Self::AssistantContentAppended => "assistant_content_appended",
            Self::ReasoningDeltaRecorded => "reasoning_delta_recorded",
            Self::ReasoningSummaryDeltaRecorded => "reasoning_summary_delta_recorded",
            Self::UsageRecorded => "usage_recorded",
            Self::ToolCallRecorded => "tool_call_recorded",
            Self::ToolResultRecorded => "tool_result_recorded",
            Self::Finished => "finished",
            Self::Failed => "failed",
        }
    }
}

/// The outcome of one recorded tool result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ToolResultOutcomeDto {
    /// The tool call completed with normalized content.
    Succeeded { content: String },
    /// The tool call failed safely.
    Failed { failure: RunFailureDto },
}

impl ToolResultOutcomeDto {
    /// Creates a successful tool-result outcome.
    ///
    /// # Errors
    ///
    /// Returns a validation error when content is blank.
    pub fn succeeded(content: impl Into<String>) -> DtoResult<Self> {
        let content = non_blank(
            content.into(),
            "invalid_tool_result_content",
            "tool result content must not be empty",
        )?;
        Ok(Self::Succeeded { content })
    }

    /// Creates a safe failed tool-result outcome.
    #[must_use]
    pub const fn failed(failure: RunFailureDto) -> Self {
        Self::Failed { failure }
    }
}

/// A safe durable terminal or attempt failure without native provider text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunFailureDto {
    code: String,
    retry: ErrorRetryDto,
    correlation_id: Option<CorrelationIdDto>,
}

impl<'de> Deserialize<'de> for RunFailureDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawRunFailureDto {
            code: String,
            retry: ErrorRetryDto,
            #[serde(default)]
            correlation_id: Option<CorrelationIdDto>,
        }
        let raw = RawRunFailureDto::deserialize(deserializer)?;
        Self::new(raw.code, raw.retry, raw.correlation_id).map_err(de::Error::custom)
    }
}

impl RunFailureDto {
    /// Creates a safe durable failure.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the stable failure code is blank.
    pub fn new(
        code: impl Into<String>,
        retry: ErrorRetryDto,
        correlation_id: Option<CorrelationIdDto>,
    ) -> DtoResult<Self> {
        Ok(Self {
            code: non_blank(
                code.into(),
                "invalid_run_failure_code",
                "run failure code must not be empty",
            )?,
            retry,
            correlation_id,
        })
    }

    /// Converts a provider-normalized safe error to the durable failure shape.
    #[must_use]
    pub fn from_provider(error: ProviderErrorDto) -> Self {
        Self {
            code: error.code().to_owned(),
            retry: error.retry(),
            correlation_id: error.correlation_id(),
        }
    }

    /// Returns the stable safe failure code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns retry guidance for this durable failure.
    #[must_use]
    pub const fn retry(&self) -> ErrorRetryDto {
        self.retry
    }

    /// Returns an opaque diagnostic correlation identifier, if any.
    #[must_use]
    pub const fn correlation_id(&self) -> Option<CorrelationIdDto> {
        self.correlation_id
    }
}

/// A validated model fact input before durable cursor assignment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelRunFactInputDto {
    /// A provider attempt began.
    ProviderAttemptStarted { attempt: u16 },
    /// A provider attempt failed safely.
    ProviderAttemptFailed {
        attempt: u16,
        failure: RunFailureDto,
    },
    /// A retry was scheduled after a failed attempt.
    RetryScheduled {
        failed_attempt: u16,
        next_attempt: u16,
    },
    /// Bounded assistant content was durably appended.
    AssistantContentAppended {
        assistant_turn_id: AssistantTurnId,
        content: String,
    },
    /// A reasoning delta is retained only in tail history.
    ReasoningDeltaRecorded {
        category: ReasoningDeltaCategory,
        content: String,
    },
    /// A reasoning summary delta is retained only in tail history.
    ReasoningSummaryDeltaRecorded { content: String },
    /// Provider-normalized usage was recorded.
    UsageRecorded { usage: UsageDto },
    /// Provider-normalized tool-call evidence was recorded.
    ToolCallRecorded { call: ToolCallDto },
    /// A durable tool result answered one recorded tool call.
    ToolResultRecorded {
        call_id: ToolCallId,
        outcome: ToolResultOutcomeDto,
    },
    /// A terminal provider reason was recorded.
    Finished { reason: FinishReasonDto },
    /// A safe terminal failure was recorded.
    Failed { failure: RunFailureDto },
}

impl<'de> Deserialize<'de> for ModelRunFactInputDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum RawModelRunFactInputDto {
            ProviderAttemptStarted {
                attempt: u16,
            },
            ProviderAttemptFailed {
                attempt: u16,
                failure: RunFailureDto,
            },
            RetryScheduled {
                failed_attempt: u16,
                next_attempt: u16,
            },
            AssistantContentAppended {
                assistant_turn_id: AssistantTurnId,
                content: String,
            },
            ReasoningDeltaRecorded {
                category: ReasoningDeltaCategory,
                content: String,
            },
            ReasoningSummaryDeltaRecorded {
                content: String,
            },
            UsageRecorded {
                usage: UsageDto,
            },
            ToolCallRecorded {
                call: ToolCallDto,
            },
            ToolResultRecorded {
                call_id: ToolCallId,
                outcome: ToolResultOutcomeDto,
            },
            Finished {
                reason: FinishReasonDto,
            },
            Failed {
                failure: RunFailureDto,
            },
        }

        let value = match RawModelRunFactInputDto::deserialize(deserializer)? {
            RawModelRunFactInputDto::ProviderAttemptStarted { attempt } => {
                Self::provider_attempt_started(attempt)
            }
            RawModelRunFactInputDto::ProviderAttemptFailed { attempt, failure } => {
                Self::provider_attempt_failed(attempt, failure)
            }
            RawModelRunFactInputDto::RetryScheduled {
                failed_attempt,
                next_attempt,
            } => Self::retry_scheduled(failed_attempt, next_attempt),
            RawModelRunFactInputDto::AssistantContentAppended {
                assistant_turn_id,
                content,
            } => Self::assistant_content_appended(assistant_turn_id, content),
            RawModelRunFactInputDto::ReasoningDeltaRecorded { category, content } => {
                Self::reasoning_delta_recorded_categorized(category, content)
            }
            RawModelRunFactInputDto::ReasoningSummaryDeltaRecorded { content } => {
                Self::reasoning_summary_delta_recorded(content)
            }
            RawModelRunFactInputDto::UsageRecorded { usage } => Ok(Self::usage_recorded(usage)),
            RawModelRunFactInputDto::ToolCallRecorded { call } => {
                Ok(Self::tool_call_recorded(call))
            }
            RawModelRunFactInputDto::ToolResultRecorded { call_id, outcome } => {
                Self::tool_result_recorded(call_id, outcome)
            }
            RawModelRunFactInputDto::Finished { reason } => Ok(Self::finished(reason)),
            RawModelRunFactInputDto::Failed { failure } => Ok(Self::failed(failure)),
        };
        value.map_err(de::Error::custom)
    }
}

impl ModelRunFactInputDto {
    /// Creates a positive provider-attempt start fact.
    ///
    /// # Errors
    ///
    /// Returns a validation error if the attempt is zero.
    pub fn provider_attempt_started(attempt: u16) -> DtoResult<Self> {
        positive_attempt(attempt)?;
        Ok(Self::ProviderAttemptStarted { attempt })
    }

    /// Creates a positive provider-attempt failure fact.
    ///
    /// # Errors
    ///
    /// Returns a validation error if the attempt is zero.
    pub fn provider_attempt_failed(attempt: u16, failure: RunFailureDto) -> DtoResult<Self> {
        positive_attempt(attempt)?;
        Ok(Self::ProviderAttemptFailed { attempt, failure })
    }

    /// Creates a retry fact whose next attempt immediately follows its failed attempt.
    ///
    /// # Errors
    ///
    /// Returns a validation error for a zero attempt, an overflow, or non-consecutive attempts.
    pub fn retry_scheduled(failed_attempt: u16, next_attempt: u16) -> DtoResult<Self> {
        positive_attempt(failed_attempt)?;
        if failed_attempt.checked_add(1) != Some(next_attempt) {
            return Err(ErrorDto::validation(
                "invalid_retry_attempt",
                "retry next attempt must immediately follow the failed attempt",
            ));
        }
        Ok(Self::RetryScheduled {
            failed_attempt,
            next_attempt,
        })
    }

    /// Creates bounded non-blank assistant content.
    ///
    /// # Errors
    ///
    /// Returns a validation error when content is blank or exceeds 4 KiB.
    pub fn assistant_content_appended(
        assistant_turn_id: AssistantTurnId,
        content: impl Into<String>,
    ) -> DtoResult<Self> {
        let content = non_blank(
            content.into(),
            "invalid_assistant_content",
            "assistant content must not be empty",
        )?;
        if content.len() > MAX_ASSISTANT_CONTENT_BYTES {
            return Err(ErrorDto::validation(
                "invalid_assistant_content",
                "assistant content must not exceed 4 KiB",
            ));
        }
        Ok(Self::AssistantContentAppended {
            assistant_turn_id,
            content,
        })
    }

    /// Creates a non-blank tail-only reasoning fact categorized as primary.
    ///
    /// # Errors
    ///
    /// Returns a validation error when content is blank or exceeds 512 KiB.
    pub fn reasoning_delta_recorded(content: impl Into<String>) -> DtoResult<Self> {
        Self::reasoning_delta_recorded_categorized(ReasoningDeltaCategory::Primary, content)
    }

    /// Creates a categorized non-blank tail-only reasoning fact.
    ///
    /// # Errors
    ///
    /// Returns a validation error when content is blank or exceeds 512 KiB.
    pub fn reasoning_delta_recorded_categorized(
        category: ReasoningDeltaCategory,
        content: impl Into<String>,
    ) -> DtoResult<Self> {
        Ok(Self::ReasoningDeltaRecorded {
            category,
            content: validate_reasoning_fact_content(content.into())?,
        })
    }

    /// Creates a non-blank tail-only reasoning summary fact.
    ///
    /// # Errors
    ///
    /// Returns a validation error when content is blank or exceeds 512 KiB.
    pub fn reasoning_summary_delta_recorded(content: impl Into<String>) -> DtoResult<Self> {
        Ok(Self::ReasoningSummaryDeltaRecorded {
            content: validate_reasoning_fact_content(content.into())?,
        })
    }

    /// Creates a usage fact.
    #[must_use]
    pub const fn usage_recorded(usage: UsageDto) -> Self {
        Self::UsageRecorded { usage }
    }

    /// Creates tool-call evidence.
    #[must_use]
    pub const fn tool_call_recorded(call: ToolCallDto) -> Self {
        Self::ToolCallRecorded { call }
    }

    /// Creates a durable tool-result fact. The call identity is a canonical
    /// UUID by construction, so no further validation is required.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the call identity is not a canonical UUID.
    pub const fn tool_result_recorded(
        call_id: ToolCallId,
        outcome: ToolResultOutcomeDto,
    ) -> DtoResult<Self> {
        Ok(Self::ToolResultRecorded { call_id, outcome })
    }

    /// Creates a terminal finish fact.
    #[must_use]
    pub const fn finished(reason: FinishReasonDto) -> Self {
        Self::Finished { reason }
    }

    /// Creates a safe terminal failure fact.
    #[must_use]
    pub const fn failed(failure: RunFailureDto) -> Self {
        Self::Failed { failure }
    }

    /// Returns the closed durable fact kind.
    #[must_use]
    pub const fn kind(&self) -> ModelRunFactKindDto {
        match self {
            Self::ProviderAttemptStarted { .. } => ModelRunFactKindDto::ProviderAttemptStarted,
            Self::ProviderAttemptFailed { .. } => ModelRunFactKindDto::ProviderAttemptFailed,
            Self::RetryScheduled { .. } => ModelRunFactKindDto::RetryScheduled,
            Self::AssistantContentAppended { .. } => ModelRunFactKindDto::AssistantContentAppended,
            Self::ReasoningDeltaRecorded { .. } => ModelRunFactKindDto::ReasoningDeltaRecorded,
            Self::ReasoningSummaryDeltaRecorded { .. } => {
                ModelRunFactKindDto::ReasoningSummaryDeltaRecorded
            }
            Self::UsageRecorded { .. } => ModelRunFactKindDto::UsageRecorded,
            Self::ToolCallRecorded { .. } => ModelRunFactKindDto::ToolCallRecorded,
            Self::ToolResultRecorded { .. } => ModelRunFactKindDto::ToolResultRecorded,
            Self::Finished { .. } => ModelRunFactKindDto::Finished,
            Self::Failed { .. } => ModelRunFactKindDto::Failed,
        }
    }
}

/// A durable typed model fact with its assigned per-run cursor.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelRunFactDto {
    cursor: RunEventCursorDto,
    #[serde(flatten)]
    input: ModelRunFactInputDto,
}

impl<'de> Deserialize<'de> for ModelRunFactDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawModelRunFactDto {
            cursor: RunEventCursorDto,
            #[serde(flatten)]
            input: ModelRunFactInputDto,
        }
        let raw = RawModelRunFactDto::deserialize(deserializer)?;
        Self::new(raw.cursor, raw.input).map_err(de::Error::custom)
    }
}

impl ModelRunFactDto {
    /// Creates a cursor-assigned durable fact.
    ///
    /// # Errors
    ///
    /// Returns a validation error for cursor zero, which is the pre-first-fact position.
    pub fn new(cursor: RunEventCursorDto, input: ModelRunFactInputDto) -> DtoResult<Self> {
        if cursor.value() == 0 {
            return Err(ErrorDto::validation(
                "invalid_run_event_cursor",
                "durable model fact cursor must be positive",
            ));
        }
        Ok(Self { cursor, input })
    }

    /// Returns this fact's assigned run cursor.
    #[must_use]
    pub const fn cursor(&self) -> RunEventCursorDto {
        self.cursor
    }

    /// Returns the typed input payload.
    #[must_use]
    pub const fn input(&self) -> &ModelRunFactInputDto {
        &self.input
    }

    /// Returns this fact's closed taxonomy kind.
    #[must_use]
    pub const fn kind(&self) -> ModelRunFactKindDto {
        self.input.kind()
    }
}

/// A typed durable event payload whose order is the model-fact cursor itself.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelRunFactEventDto {
    session_id: SessionId,
    run_id: RunId,
    fact: ModelRunFactDto,
    occurred_at: TimestampDto,
}

impl<'de> Deserialize<'de> for ModelRunFactEventDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawModelRunFactEventDto {
            session_id: SessionId,
            run_id: RunId,
            fact: ModelRunFactDto,
            occurred_at: TimestampDto,
        }
        let raw = RawModelRunFactEventDto::deserialize(deserializer)?;
        Ok(Self::new(
            raw.session_id,
            raw.run_id,
            raw.fact,
            raw.occurred_at,
        ))
    }
}

impl ModelRunFactEventDto {
    /// Creates a typed durable model-fact event payload.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        run_id: RunId,
        fact: ModelRunFactDto,
        occurred_at: TimestampDto,
    ) -> Self {
        Self {
            session_id,
            run_id,
            fact,
            occurred_at,
        }
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

    /// Returns the typed cursor-assigned fact.
    #[must_use]
    pub const fn fact(&self) -> &ModelRunFactDto {
        &self.fact
    }

    /// Returns the fact occurrence time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// The M4 model augmentation of a compatible M3 run projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelRunProjectionDto {
    run_projection: super::RunProjectionDto,
    cursor: RunEventCursorDto,
    assistant_turn_id: Option<AssistantTurnId>,
    assistant_content: String,
    usage: Option<UsageDto>,
    finish_reason: Option<FinishReasonDto>,
    failure: Option<RunFailureDto>,
}

impl<'de> Deserialize<'de> for ModelRunProjectionDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawModelRunProjectionDto {
            run_projection: super::RunProjectionDto,
            cursor: RunEventCursorDto,
            #[serde(default)]
            assistant_turn_id: Option<AssistantTurnId>,
            #[serde(default)]
            assistant_content: String,
            #[serde(default)]
            usage: Option<UsageDto>,
            #[serde(default)]
            finish_reason: Option<FinishReasonDto>,
            #[serde(default)]
            failure: Option<RunFailureDto>,
        }
        let raw = RawModelRunProjectionDto::deserialize(deserializer)?;
        Self::new(
            raw.run_projection,
            raw.cursor,
            raw.assistant_turn_id,
            raw.assistant_content,
            raw.usage,
            raw.finish_reason,
            raw.failure,
        )
        .map_err(de::Error::custom)
    }
}

impl ModelRunProjectionDto {
    /// Creates a safe M4 model projection around the stable M3 projection.
    ///
    /// # Errors
    ///
    /// Returns a validation error when assistant identity/content or terminal facts conflict.
    pub fn new(
        run_projection: super::RunProjectionDto,
        cursor: RunEventCursorDto,
        assistant_turn_id: Option<AssistantTurnId>,
        assistant_content: impl Into<String>,
        usage: Option<UsageDto>,
        finish_reason: Option<FinishReasonDto>,
        failure: Option<RunFailureDto>,
    ) -> DtoResult<Self> {
        let assistant_content = assistant_content.into();
        let assistant_identity_is_consistent =
            assistant_turn_id.is_some() || assistant_content.is_empty();
        let terminal_outcome_is_consistent = finish_reason.is_none() || failure.is_none();
        if !assistant_identity_is_consistent || !terminal_outcome_is_consistent {
            return Err(ErrorDto::validation(
                "invalid_model_run_projection",
                "model run projection has inconsistent safe model fields",
            ));
        }
        Ok(Self {
            run_projection,
            cursor,
            assistant_turn_id,
            assistant_content,
            usage,
            finish_reason,
            failure,
        })
    }

    /// Returns the compatible M3 run projection.
    #[must_use]
    pub const fn run_projection(&self) -> super::RunProjectionDto {
        self.run_projection
    }

    /// Returns the current durable model-fact cursor.
    #[must_use]
    pub const fn cursor(&self) -> RunEventCursorDto {
        self.cursor
    }

    /// Returns the current assistant turn identity, when text exists.
    #[must_use]
    pub const fn assistant_turn_id(&self) -> Option<AssistantTurnId> {
        self.assistant_turn_id
    }

    /// Returns accumulated assistant content.
    #[must_use]
    pub fn assistant_content(&self) -> &str {
        &self.assistant_content
    }

    /// Reasoning is tail-only and is never included in a snapshot projection.
    #[must_use]
    pub const fn reasoning_content(&self) -> Option<&str> {
        None
    }

    /// Returns final normalized usage, when recorded.
    #[must_use]
    pub const fn usage(&self) -> Option<UsageDto> {
        self.usage
    }

    /// Returns terminal finish reason, when recorded.
    #[must_use]
    pub const fn finish_reason(&self) -> Option<FinishReasonDto> {
        self.finish_reason
    }

    /// Returns a safe terminal failure, when recorded.
    #[must_use]
    pub const fn failure(&self) -> Option<&RunFailureDto> {
        self.failure.as_ref()
    }
}

/// A dedicated safe M4 run snapshot at one session sequence and run cursor.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunSnapshotDto {
    session_id: SessionId,
    run_id: RunId,
    at_sequence: SessionEventSequenceDto,
    projection: ModelRunProjectionDto,
}

impl<'de> Deserialize<'de> for RunSnapshotDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawRunSnapshotDto {
            session_id: SessionId,
            run_id: RunId,
            at_sequence: SessionEventSequenceDto,
            projection: ModelRunProjectionDto,
        }
        let raw = RawRunSnapshotDto::deserialize(deserializer)?;
        Self::new(raw.session_id, raw.run_id, raw.at_sequence, raw.projection)
            .map_err(de::Error::custom)
    }
}

impl RunSnapshotDto {
    /// Creates a dedicated run-scoped snapshot.
    ///
    /// # Errors
    ///
    /// Returns a validation error if embedded M3 run identity differs from the snapshot identity.
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        at_sequence: SessionEventSequenceDto,
        projection: ModelRunProjectionDto,
    ) -> DtoResult<Self> {
        if projection.run_projection().session_id() != session_id
            || projection.run_projection().run_id() != run_id
        {
            return Err(ErrorDto::validation(
                "invalid_run_snapshot",
                "run snapshot identity must match its compatible run projection",
            ));
        }
        Ok(Self {
            session_id,
            run_id,
            at_sequence,
            projection,
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

    /// Returns the durable session sequence captured with this run snapshot.
    #[must_use]
    pub const fn at_sequence(&self) -> SessionEventSequenceDto {
        self.at_sequence
    }

    /// Returns the model-fact cursor included by this snapshot.
    #[must_use]
    pub const fn cursor(&self) -> RunEventCursorDto {
        self.projection.cursor()
    }

    /// Returns the safe model-augmented projection.
    #[must_use]
    pub const fn projection(&self) -> &ModelRunProjectionDto {
        &self.projection
    }

    /// Returns the compatible M3 run projection.
    #[must_use]
    pub const fn run_projection(&self) -> super::RunProjectionDto {
        self.projection.run_projection()
    }
}

/// A bounded strict-after page of run-scoped durable facts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunEventTailPageDto {
    session_id: SessionId,
    run_id: RunId,
    after_cursor: RunEventCursorDto,
    facts: Vec<ModelRunFactDto>,
    next_after_cursor: RunEventCursorDto,
    has_more: bool,
}

impl<'de> Deserialize<'de> for RunEventTailPageDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawRunEventTailPageDto {
            session_id: SessionId,
            run_id: RunId,
            after_cursor: RunEventCursorDto,
            facts: Vec<ModelRunFactDto>,
            next_after_cursor: RunEventCursorDto,
            has_more: bool,
        }
        let raw = RawRunEventTailPageDto::deserialize(deserializer)?;
        Self::new(
            raw.session_id,
            raw.run_id,
            raw.after_cursor,
            raw.facts,
            raw.next_after_cursor,
            raw.has_more,
        )
        .map_err(de::Error::custom)
    }
}

impl RunEventTailPageDto {
    /// Creates a bounded contiguous strict-after run-fact page.
    ///
    /// # Errors
    ///
    /// Returns a validation error when facts exceed the bound, are non-contiguous, or continuation is inconsistent.
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        after_cursor: RunEventCursorDto,
        facts: Vec<ModelRunFactDto>,
        next_after_cursor: RunEventCursorDto,
        has_more: bool,
    ) -> DtoResult<Self> {
        if facts.len() > MAX_TAIL_FACTS {
            return Err(ErrorDto::validation(
                "invalid_run_event_tail",
                "run event tail must contain at most 256 facts",
            ));
        }
        let mut expected = after_cursor.value();
        for fact in &facts {
            expected = expected.checked_add(1).ok_or_else(|| {
                ErrorDto::validation("invalid_run_event_tail", "run event cursor overflow")
            })?;
            if fact.cursor().value() != expected {
                return Err(ErrorDto::validation(
                    "invalid_run_event_tail",
                    "run event tail facts must be contiguous after the requested cursor",
                ));
            }
        }
        if next_after_cursor.value() != expected {
            return Err(ErrorDto::validation(
                "invalid_run_event_tail",
                "run event tail continuation must equal its final fact cursor",
            ));
        }
        Ok(Self {
            session_id,
            run_id,
            after_cursor,
            facts,
            next_after_cursor,
            has_more,
        })
    }

    /// Creates the empty page at a current run cursor.
    #[must_use]
    pub const fn empty(session_id: SessionId, run_id: RunId, cursor: RunEventCursorDto) -> Self {
        Self {
            session_id,
            run_id,
            after_cursor: cursor,
            facts: Vec::new(),
            next_after_cursor: cursor,
            has_more: false,
        }
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

    /// Returns the exclusive cursor boundary.
    #[must_use]
    pub const fn after_cursor(&self) -> RunEventCursorDto {
        self.after_cursor
    }

    /// Returns ordered durable facts strictly after the requested cursor.
    #[must_use]
    pub fn facts(&self) -> &[ModelRunFactDto] {
        &self.facts
    }

    /// Returns the continuation cursor.
    #[must_use]
    pub const fn next_after_cursor(&self) -> RunEventCursorDto {
        self.next_after_cursor
    }

    /// Returns whether more facts remain beyond this bounded page.
    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }
}

/// A matching run-scoped durable snapshot and its ordered tail.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunReplayDto {
    snapshot: RunSnapshotDto,
    tail: RunEventTailPageDto,
}

impl<'de> Deserialize<'de> for RunReplayDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawRunReplayDto {
            snapshot: RunSnapshotDto,
            tail: RunEventTailPageDto,
        }
        let raw = RawRunReplayDto::deserialize(deserializer)?;
        Self::new(raw.snapshot, raw.tail).map_err(de::Error::custom)
    }
}

impl RunReplayDto {
    /// Creates matching dedicated run replay components.
    ///
    /// # Errors
    ///
    /// Returns a validation error if the tail differs from the snapshot identity or cursor.
    pub fn new(snapshot: RunSnapshotDto, tail: RunEventTailPageDto) -> DtoResult<Self> {
        let matching_cursor = tail.after_cursor() == snapshot.cursor();
        if tail.session_id() != snapshot.session_id()
            || tail.run_id() != snapshot.run_id()
            || !matching_cursor
        {
            return Err(ErrorDto::validation(
                "invalid_run_replay",
                "run replay tail must begin at its matching run snapshot cursor",
            ));
        }
        Ok(Self { snapshot, tail })
    }

    /// Returns the current safe run snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> &RunSnapshotDto {
        &self.snapshot
    }

    /// Returns the bounded strict-after fact tail.
    #[must_use]
    pub const fn tail(&self) -> &RunEventTailPageDto {
        &self.tail
    }
}
