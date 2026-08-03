//! Provider-neutral model contracts and validated stream facts.
//!
//! This crate contains no provider SDK, asynchronous runtime, or transport
//! resources. Provider crates translate their native responses into these
//! validated DTOs before crossing the provider boundary.

use std::fmt::{Display, Formatter};

use intention_types::{CorrelationIdDto, DtoResult, ErrorDto, ErrorRetryDto, RunId, ToolCallId};
use serde::{Deserialize, Deserializer, Serialize, de};

/// The sender role of a text-only model-context message.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRoleDto {
    /// Daemon-selected model instruction context.
    System,
    /// User-provided turn content.
    User,
    /// A prior normalized assistant response.
    Assistant,
}

/// A validated text-only model-context message.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelMessageDto {
    role: ModelRoleDto,
    content: String,
}

impl<'de> Deserialize<'de> for ModelMessageDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawModelMessageDto {
            role: ModelRoleDto,
            content: String,
        }

        let raw = RawModelMessageDto::deserialize(deserializer)?;
        Self::new(raw.role, raw.content).map_err(de::Error::custom)
    }
}

impl ModelMessageDto {
    /// Creates a non-blank text-only context message.
    ///
    /// # Errors
    ///
    /// Returns a validation error when content is blank.
    pub fn new(role: ModelRoleDto, content: impl Into<String>) -> DtoResult<Self> {
        let content = content.into();
        if content.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_model_message_content",
                "model message content must not be empty",
            ));
        }
        Ok(Self { role, content })
    }

    /// Returns the message role.
    #[must_use]
    pub const fn role(&self) -> ModelRoleDto {
        self.role
    }

    /// Returns the text-only message content.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
}

/// Requested model-context capabilities that require preflight support.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ModelRequestedCapabilitiesDto {
    reasoning: bool,
    multimodal: bool,
    tool_calls: bool,
    vendor_extensions: bool,
}

impl ModelRequestedCapabilitiesDto {
    /// Creates an explicit request capability set.
    #[must_use]
    pub const fn new(
        reasoning: bool,
        multimodal: bool,
        tool_calls: bool,
        vendor_extensions: bool,
    ) -> Self {
        Self {
            reasoning,
            multimodal,
            tool_calls,
            vendor_extensions,
        }
    }

    /// Returns whether reasoning output was requested.
    #[must_use]
    pub const fn reasoning(self) -> bool {
        self.reasoning
    }

    /// Returns whether multimodal input or output was requested.
    #[must_use]
    pub const fn multimodal(self) -> bool {
        self.multimodal
    }

    /// Returns whether function-style tool calls were requested.
    #[must_use]
    pub const fn tool_calls(self) -> bool {
        self.tool_calls
    }

    /// Returns whether provider-specific extensions were requested.
    #[must_use]
    pub const fn vendor_extensions(self) -> bool {
        self.vendor_extensions
    }
}

/// The explicit capabilities declared by a selected provider driver.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ModelCapabilitiesDto {
    text: bool,
    reasoning: bool,
    tool_calls: bool,
    multimodal: bool,
    vendor_extensions: bool,
    streaming: bool,
}

impl ModelCapabilitiesDto {
    /// Creates an explicit capability declaration.
    #[must_use]
    pub const fn new(
        text: bool,
        reasoning: bool,
        tool_calls: bool,
        multimodal: bool,
        vendor_extensions: bool,
        streaming: bool,
    ) -> Self {
        Self {
            text,
            reasoning,
            tool_calls,
            multimodal,
            vendor_extensions,
            streaming,
        }
    }

    /// Returns whether text input/output is supported.
    #[must_use]
    pub const fn supports_text(self) -> bool {
        self.text
    }

    /// Returns whether reasoning output is supported.
    #[must_use]
    pub const fn supports_reasoning(self) -> bool {
        self.reasoning
    }

    /// Returns whether tool calls are supported.
    #[must_use]
    pub const fn supports_tool_calls(self) -> bool {
        self.tool_calls
    }

    /// Returns whether multimodal input/output is supported.
    #[must_use]
    pub const fn supports_multimodal(self) -> bool {
        self.multimodal
    }

    /// Returns whether provider-specific extensions are supported.
    #[must_use]
    pub const fn supports_vendor_extensions(self) -> bool {
        self.vendor_extensions
    }

    /// Returns whether streaming is supported.
    #[must_use]
    pub const fn supports_streaming(self) -> bool {
        self.streaming
    }

    /// Rejects a request requiring an undeclared capability before any outbound call.
    ///
    /// # Errors
    ///
    /// Returns a policy error when the requested model behavior is unsupported.
    pub fn ensure_supports(self, requested: ModelRequestedCapabilitiesDto) -> DtoResult<()> {
        let unsupported = (requested.reasoning && !self.reasoning)
            || (requested.multimodal && !self.multimodal)
            || (requested.tool_calls && !self.tool_calls)
            || (requested.vendor_extensions && !self.vendor_extensions);
        if unsupported {
            Err(ErrorDto::new(
                "unsupported_model_capability",
                intention_types::ErrorCategoryDto::Policy,
                "the selected provider does not support the requested model capability",
                ErrorRetryDto::Never,
                None,
            )?)
        } else {
            Ok(())
        }
    }
}

/// A validated provider-neutral model request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelRequestDto {
    run_id: RunId,
    model: String,
    messages: Vec<ModelMessageDto>,
    system_context: Option<String>,
    requested_capabilities: ModelRequestedCapabilitiesDto,
}

impl<'de> Deserialize<'de> for ModelRequestDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawModelRequestDto {
            run_id: RunId,
            model: String,
            messages: Vec<ModelMessageDto>,
            #[serde(default)]
            system_context: Option<String>,
            #[serde(default)]
            requested_capabilities: ModelRequestedCapabilitiesDto,
        }

        let raw = RawModelRequestDto::deserialize(deserializer)?;
        Self::new(
            raw.run_id,
            raw.model,
            raw.messages,
            raw.system_context,
            Some(raw.requested_capabilities),
        )
        .map_err(de::Error::custom)
    }
}

impl ModelRequestDto {
    /// Creates a text-only provider-neutral model request.
    ///
    /// # Errors
    ///
    /// Returns a validation error for a blank model, an empty message list, or a blank system context.
    pub fn new(
        run_id: RunId,
        model: impl Into<String>,
        messages: Vec<ModelMessageDto>,
        system_context: Option<String>,
        requested_capabilities: Option<ModelRequestedCapabilitiesDto>,
    ) -> DtoResult<Self> {
        let model = model.into();
        if model.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_model_identifier",
                "model identifier must not be empty",
            ));
        }
        if messages.is_empty() {
            return Err(ErrorDto::validation(
                "missing_model_messages",
                "model request must contain at least one message",
            ));
        }
        if system_context
            .as_ref()
            .is_some_and(|context| context.trim().is_empty())
        {
            return Err(ErrorDto::validation(
                "invalid_model_system_context",
                "model system context must not be empty when provided",
            ));
        }
        Ok(Self {
            run_id,
            model,
            messages,
            system_context,
            requested_capabilities: requested_capabilities.unwrap_or_default(),
        })
    }

    /// Returns the daemon-owned run identity.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    /// Returns the selected model identifier.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Returns the text-only model context messages.
    #[must_use]
    pub fn messages(&self) -> &[ModelMessageDto] {
        &self.messages
    }

    /// Returns optional daemon-selected system context.
    #[must_use]
    pub fn system_context(&self) -> Option<&str> {
        self.system_context.as_deref()
    }

    /// Returns capability requirements for preflight.
    #[must_use]
    pub const fn requested_capabilities(&self) -> ModelRequestedCapabilitiesDto {
        self.requested_capabilities
    }
}

/// A validated model-generated function call with JSON object input.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolCallDto {
    call_id: ToolCallId,
    name: String,
    arguments_json: String,
}

impl<'de> Deserialize<'de> for ToolCallDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawToolCallDto {
            call_id: ToolCallId,
            name: String,
            arguments_json: String,
        }

        let raw = RawToolCallDto::deserialize(deserializer)?;
        Self::new(raw.call_id, raw.name, raw.arguments_json).map_err(de::Error::custom)
    }
}

impl ToolCallDto {
    /// Creates a function call with a non-empty name and object-shaped JSON arguments.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the name is blank or arguments are not a JSON object.
    pub fn new(
        call_id: ToolCallId,
        name: impl Into<String>,
        arguments_json: impl Into<String>,
    ) -> DtoResult<Self> {
        let name = name.into();
        let arguments_json = arguments_json.into();
        if name.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_tool_call_name",
                "tool call name must not be empty",
            ));
        }
        let _: std::collections::BTreeMap<String, serde::de::IgnoredAny> =
            serde_json::from_str(&arguments_json).map_err(|_| {
                ErrorDto::validation(
                    "invalid_tool_call_arguments",
                    "tool call arguments must be a JSON object",
                )
            })?;
        Ok(Self {
            call_id,
            name,
            arguments_json,
        })
    }

    /// Returns the daemon-assigned tool-call identity.
    #[must_use]
    pub const fn call_id(&self) -> ToolCallId {
        self.call_id
    }

    /// Returns the function name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the validated JSON object text for later typed tool decoding.
    #[must_use]
    pub fn arguments_json(&self) -> &str {
        &self.arguments_json
    }
}

/// Provider-normalized token usage with a reported or not-reported state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UsageDto {
    /// The provider did not report usage for this stream.
    NotReported,
    /// The provider reported internally consistent token counts.
    Reported {
        /// Tokens attributed to input/context.
        input_tokens: u64,
        /// Tokens attributed to model output.
        output_tokens: u64,
        /// Total tokens attributed to the request.
        total_tokens: u64,
    },
}

impl<'de> Deserialize<'de> for UsageDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "state", rename_all = "snake_case")]
        enum RawUsageDto {
            NotReported,
            Reported {
                input_tokens: u64,
                output_tokens: u64,
                total_tokens: u64,
            },
        }

        match RawUsageDto::deserialize(deserializer)? {
            RawUsageDto::NotReported => Ok(Self::NotReported),
            RawUsageDto::Reported {
                input_tokens,
                output_tokens,
                total_tokens,
            } => {
                Self::reported(input_tokens, output_tokens, total_tokens).map_err(de::Error::custom)
            }
        }
    }
}

impl UsageDto {
    /// Creates validated reported usage.
    ///
    /// # Errors
    ///
    /// Returns a validation error when total tokens do not equal input plus output.
    pub fn reported(input_tokens: u64, output_tokens: u64, total_tokens: u64) -> DtoResult<Self> {
        if input_tokens.saturating_add(output_tokens) != total_tokens {
            return Err(ErrorDto::validation(
                "invalid_model_usage",
                "model usage total must equal input plus output tokens",
            ));
        }
        Ok(Self::Reported {
            input_tokens,
            output_tokens,
            total_tokens,
        })
    }
}

/// Provider-normalized terminal reason.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReasonDto {
    /// The model completed naturally.
    Stop,
    /// The requested or provider output limit was reached.
    Length,
    /// The model emitted one or more tool calls.
    ToolCalls,
    /// Provider content policy prevented normal completion.
    ContentFilter,
    /// The provider returned an explicit terminal error reason.
    Error,
    /// The provider terminated without a documented terminal reason.
    Unknown,
}

/// A provider-neutral normalized stream fact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelEventDto {
    /// The provider stream started successfully.
    Started,
    /// A non-empty text content delta arrived.
    TextDelta { content: String },
    /// A non-empty reasoning delta arrived.
    ReasoningDelta { content: String },
    /// A complete provider-normalized tool call arrived.
    ToolCall { call: ToolCallDto },
    /// Final usage became available.
    Usage { usage: UsageDto },
    /// The provider stream reached a terminal reason.
    Finished { reason: FinishReasonDto },
}

impl<'de> Deserialize<'de> for ModelEventDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum RawModelEventDto {
            Started,
            TextDelta { content: String },
            ReasoningDelta { content: String },
            ToolCall { call: ToolCallDto },
            Usage { usage: UsageDto },
            Finished { reason: FinishReasonDto },
        }

        match RawModelEventDto::deserialize(deserializer)? {
            RawModelEventDto::Started => Ok(Self::started()),
            RawModelEventDto::TextDelta { content } => {
                Self::text_delta(content).map_err(de::Error::custom)
            }
            RawModelEventDto::ReasoningDelta { content } => {
                Self::reasoning_delta(content).map_err(de::Error::custom)
            }
            RawModelEventDto::ToolCall { call } => Ok(Self::tool_call(call)),
            RawModelEventDto::Usage { usage } => Ok(Self::usage(usage)),
            RawModelEventDto::Finished { reason } => Ok(Self::finished(reason)),
        }
    }
}

impl ModelEventDto {
    /// Creates a stream-start fact.
    #[must_use]
    pub const fn started() -> Self {
        Self::Started
    }

    /// Creates a non-empty text content delta.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the delta is empty.
    pub fn text_delta(content: impl Into<String>) -> DtoResult<Self> {
        let content = content.into();
        if content.is_empty() {
            Err(ErrorDto::validation(
                "invalid_model_text_delta",
                "model text delta must not be empty",
            ))
        } else {
            Ok(Self::TextDelta { content })
        }
    }

    /// Creates a non-empty reasoning delta.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the delta is empty.
    pub fn reasoning_delta(content: impl Into<String>) -> DtoResult<Self> {
        let content = content.into();
        if content.is_empty() {
            Err(ErrorDto::validation(
                "invalid_model_reasoning_delta",
                "model reasoning delta must not be empty",
            ))
        } else {
            Ok(Self::ReasoningDelta { content })
        }
    }

    /// Creates a complete normalized tool-call fact.
    #[must_use]
    pub const fn tool_call(call: ToolCallDto) -> Self {
        Self::ToolCall { call }
    }

    /// Creates a normalized usage fact.
    #[must_use]
    pub const fn usage(usage: UsageDto) -> Self {
        Self::Usage { usage }
    }

    /// Creates a terminal stream fact.
    #[must_use]
    pub const fn finished(reason: FinishReasonDto) -> Self {
        Self::Finished { reason }
    }
}

/// Validates normalized model-stream ordering without owning runtime delivery.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ModelStreamLifecycleDto {
    started: bool,
    terminal: bool,
    usage_seen: bool,
}

impl ModelStreamLifecycleDto {
    /// Creates a stream validator before the provider emits a start fact.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            started: false,
            terminal: false,
            usage_seen: false,
        }
    }

    /// Accepts one ordered normalized event.
    ///
    /// # Errors
    ///
    /// Returns a validation error if the event cannot occur at this stream position.
    pub fn accept(&mut self, event: &ModelEventDto) -> DtoResult<()> {
        match event {
            ModelEventDto::Started if !self.started && !self.terminal => {
                self.started = true;
                Ok(())
            }
            ModelEventDto::Started => Err(stream_order_error()),
            ModelEventDto::Finished { .. } if self.started && !self.terminal => {
                self.terminal = true;
                Ok(())
            }
            ModelEventDto::Usage { .. } if self.started && !self.terminal && !self.usage_seen => {
                self.usage_seen = true;
                Ok(())
            }
            ModelEventDto::TextDelta { .. }
            | ModelEventDto::ReasoningDelta { .. }
            | ModelEventDto::ToolCall { .. }
                if self.started && !self.terminal =>
            {
                Ok(())
            }
            ModelEventDto::Usage { .. } | ModelEventDto::Finished { .. } => {
                Err(stream_order_error())
            }
            ModelEventDto::TextDelta { .. }
            | ModelEventDto::ReasoningDelta { .. }
            | ModelEventDto::ToolCall { .. } => Err(stream_order_error()),
        }
    }

    /// Returns whether a terminal fact was accepted.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        self.terminal
    }
}

fn stream_order_error() -> ErrorDto {
    ErrorDto::validation(
        "invalid_model_stream_order",
        "model stream event order is invalid",
    )
}

/// A safe provider-normalized failure that never includes native error text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderErrorDto {
    code: String,
    retry: ErrorRetryDto,
    correlation_id: Option<CorrelationIdDto>,
}

impl<'de> Deserialize<'de> for ProviderErrorDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawProviderErrorDto {
            code: String,
            retry: ErrorRetryDto,
            #[serde(default)]
            correlation_id: Option<CorrelationIdDto>,
        }

        let raw = RawProviderErrorDto::deserialize(deserializer)?;
        Self::new(raw.code, raw.retry, raw.correlation_id).map_err(de::Error::custom)
    }
}

impl ProviderErrorDto {
    /// Creates a safe normalized unavailable provider error.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the stable code is blank.
    pub fn unavailable(
        code: impl Into<String>,
        retryable: bool,
        correlation_id: Option<CorrelationIdDto>,
    ) -> DtoResult<Self> {
        Self::new(
            code,
            if retryable {
                ErrorRetryDto::Delayed
            } else {
                ErrorRetryDto::Never
            },
            correlation_id,
        )
    }

    fn new(
        code: impl Into<String>,
        retry: ErrorRetryDto,
        correlation_id: Option<CorrelationIdDto>,
    ) -> DtoResult<Self> {
        let code = code.into();
        if code.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_provider_error_code",
                "provider error code must not be empty",
            ));
        }
        Ok(Self {
            code,
            retry,
            correlation_id,
        })
    }

    /// Returns the stable safe provider failure code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns the retry guidance selected by provider normalization.
    #[must_use]
    pub const fn retry(&self) -> ErrorRetryDto {
        self.retry
    }

    /// Returns an opaque correlation identifier when available.
    #[must_use]
    pub const fn correlation_id(&self) -> Option<CorrelationIdDto> {
        self.correlation_id
    }
}

impl Display for ProviderErrorDto {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.code)
    }
}

/// Provider-neutral stream driver boundary. SDK and runtime resources stay private to providers/runtime.
pub trait ModelDriver {
    /// Returns the static capability declaration for this configured driver.
    fn capabilities(&self) -> ModelCapabilitiesDto;

    /// Validates whether this driver can accept the request before outbound work begins.
    ///
    /// # Errors
    ///
    /// Returns a policy error when a request requires unsupported capability.
    fn preflight(&self, request: &ModelRequestDto) -> DtoResult<()> {
        self.capabilities()
            .ensure_supports(request.requested_capabilities())
    }
}
