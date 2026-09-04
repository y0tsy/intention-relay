//! Provider-neutral model contracts (including tool calls and results) and validated stream facts.
//!
//! This crate contains no provider SDK, asynchronous runtime, or transport
//! resources. Provider crates translate their native responses into these
//! validated DTOs before crossing the provider boundary.

use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};

use futures_core::Stream;
use intention_types::{DtoResult, ErrorDto, ErrorRetryDto, RunId, ToolCallId};
pub use intention_types::{FinishReasonDto, ProviderErrorDto, ToolCallDto, UsageDto};
use serde::{Deserialize, Deserializer, Serialize, de};

/// The sender role of a model-context message, including tool calls and results.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRoleDto {
    /// Daemon-selected model instruction context.
    System,
    /// User-provided turn content.
    User,
    /// A prior normalized assistant response, optionally carrying tool calls.
    Assistant,
    /// A tool-role message carrying the result of one tool call.
    Tool,
}

/// A validated model-context message, text-only or carrying tool calls or a tool result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelMessageDto {
    role: ModelRoleDto,
    content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCallDto>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<ToolCallId>,
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
            #[serde(default)]
            tool_calls: Option<Vec<ToolCallDto>>,
            #[serde(default)]
            tool_call_id: Option<ToolCallId>,
        }

        let raw = RawModelMessageDto::deserialize(deserializer)?;
        let message = match (raw.role, raw.tool_calls, raw.tool_call_id) {
            (ModelRoleDto::Tool, None, Some(tool_call_id)) => {
                Self::tool_result(tool_call_id, raw.content)
            }
            (ModelRoleDto::Assistant, Some(tool_calls), None) => {
                Self::assistant_tool_calls(Some(raw.content), tool_calls)
            }
            (
                role @ (ModelRoleDto::System | ModelRoleDto::User | ModelRoleDto::Assistant),
                None,
                None,
            ) => Self::new(role, raw.content),
            _ => Err(ErrorDto::validation(
                "invalid_model_message_shape",
                "model message role and tool-call fields are inconsistent",
            )),
        };
        message.map_err(de::Error::custom)
    }
}

impl ModelMessageDto {
    /// Creates a non-blank text-only context message.
    ///
    /// # Errors
    ///
    /// Returns a validation error when content is blank or the role is [`ModelRoleDto::Tool`].
    pub fn new(role: ModelRoleDto, content: impl Into<String>) -> DtoResult<Self> {
        let content = content.into();
        if content.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_model_message_content",
                "model message content must not be empty",
            ));
        }
        if role == ModelRoleDto::Tool {
            return Err(ErrorDto::validation(
                "invalid_model_message_role",
                "tool-role messages require a tool call result",
            ));
        }
        Ok(Self {
            role,
            content,
            tool_calls: None,
            tool_call_id: None,
        })
    }

    /// Creates an assistant tool-call message that may carry empty text.
    ///
    /// # Errors
    ///
    /// Returns a validation error when no tool call is provided.
    pub fn assistant_tool_calls(
        content: Option<String>,
        tool_calls: Vec<ToolCallDto>,
    ) -> DtoResult<Self> {
        if tool_calls.is_empty() {
            return Err(ErrorDto::validation(
                "invalid_model_message_tool_calls",
                "assistant tool-call message must contain at least one tool call",
            ));
        }
        Ok(Self {
            role: ModelRoleDto::Assistant,
            content: content.unwrap_or_default(),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        })
    }

    /// Creates a non-blank tool-role message answering one tool call.
    ///
    /// # Errors
    ///
    /// Returns a validation error when content is blank.
    pub fn tool_result(tool_call_id: ToolCallId, content: impl Into<String>) -> DtoResult<Self> {
        let content = content.into();
        if content.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_model_message_content",
                "model message content must not be empty",
            ));
        }
        Ok(Self {
            role: ModelRoleDto::Tool,
            content,
            tool_calls: None,
            tool_call_id: Some(tool_call_id),
        })
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

    /// Returns the tool calls carried by an assistant tool-call message, if any.
    #[must_use]
    pub fn tool_calls(&self) -> Option<&[ToolCallDto]> {
        self.tool_calls.as_deref()
    }

    /// Returns the tool call identity answered by a tool-role message, if any.
    #[must_use]
    pub const fn tool_call_id(&self) -> Option<ToolCallId> {
        self.tool_call_id
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
    /// Creates a provider-neutral model request.
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

    /// Returns the model context messages.
    #[must_use]
    pub fn messages(&self) -> &[ModelMessageDto] {
        &self.messages
    }

    /// Returns a copy of this request with the model context messages replaced.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the replacement message list is empty.
    pub fn with_messages(&self, messages: Vec<ModelMessageDto>) -> DtoResult<Self> {
        Self::new(
            self.run_id,
            self.model.clone(),
            messages,
            self.system_context.clone(),
            Some(self.requested_capabilities),
        )
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

/// The closed category of one normalized reasoning fragment.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningFragmentCategoryDto {
    /// The main textual reasoning representation.
    Primary,
    /// A separate detailed reasoning representation.
    Detail,
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
    ReasoningDelta {
        category: ReasoningFragmentCategoryDto,
        content: String,
    },
    /// A non-empty reasoning summary delta arrived.
    ReasoningSummaryDelta { content: String },
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
            TextDelta {
                content: String,
            },
            ReasoningDelta {
                category: ReasoningFragmentCategoryDto,
                content: String,
            },
            ReasoningSummaryDelta {
                content: String,
            },
            ToolCall {
                call: ToolCallDto,
            },
            Usage {
                usage: UsageDto,
            },
            Finished {
                reason: FinishReasonDto,
            },
        }

        match RawModelEventDto::deserialize(deserializer)? {
            RawModelEventDto::Started => Ok(Self::started()),
            RawModelEventDto::TextDelta { content } => {
                Self::text_delta(content).map_err(de::Error::custom)
            }
            RawModelEventDto::ReasoningDelta { category, content } => {
                Self::reasoning_delta_categorized(category, content).map_err(de::Error::custom)
            }
            RawModelEventDto::ReasoningSummaryDelta { content } => {
                Self::reasoning_summary_delta(content).map_err(de::Error::custom)
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

    /// Creates a non-empty reasoning delta categorized as primary.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the delta is empty.
    pub fn reasoning_delta(content: impl Into<String>) -> DtoResult<Self> {
        Self::reasoning_delta_categorized(ReasoningFragmentCategoryDto::Primary, content)
    }

    /// Creates a non-empty categorized reasoning delta.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the delta is empty.
    pub fn reasoning_delta_categorized(
        category: ReasoningFragmentCategoryDto,
        content: impl Into<String>,
    ) -> DtoResult<Self> {
        let content = content.into();
        if content.is_empty() {
            Err(ErrorDto::validation(
                "invalid_model_reasoning_delta",
                "model reasoning delta must not be empty",
            ))
        } else {
            Ok(Self::ReasoningDelta { category, content })
        }
    }

    /// Creates a non-empty reasoning summary delta.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the delta is empty.
    pub fn reasoning_summary_delta(content: impl Into<String>) -> DtoResult<Self> {
        let content = content.into();
        if content.is_empty() {
            Err(ErrorDto::validation(
                "invalid_model_reasoning_summary_delta",
                "model reasoning summary delta must not be empty",
            ))
        } else {
            Ok(Self::ReasoningSummaryDelta { content })
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

/// The closed provider-neutral reasoning effort levels.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffortLevel {
    /// Reasoning is explicitly disabled.
    None,
    /// Minimal reasoning effort.
    Minimal,
    /// Low reasoning effort.
    Low,
    /// Balanced reasoning effort.
    Medium,
    /// High reasoning effort.
    High,
    /// Extra-high reasoning effort.
    Xhigh,
    /// Maximum reasoning effort.
    Max,
}

/// The closed provider-neutral reasoning modes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponsesReasoningMode {
    /// The standard reasoning mode.
    Standard,
    /// The higher-capability reasoning mode.
    Pro,
}

/// The closed credential transport modes (names only, never values).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialTransportMode {
    /// Authorization through the standard bearer scheme.
    Bearer,
    /// Authorization through one descriptor-selected safe header name.
    SafeHeader,
}

/// Maximum characters of one safe header name.
const MAX_SAFE_HEADER_NAME_CHARS: usize = 128;

/// A descriptor-declared header policy carrying names only, never credential values.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AuthenticationHeaderPolicyV1 {
    allowed_header_names: Vec<String>,
    selected_transport: CredentialTransportMode,
}

impl<'de> Deserialize<'de> for AuthenticationHeaderPolicyV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawAuthenticationHeaderPolicyV1 {
            allowed_header_names: Vec<String>,
            selected_transport: CredentialTransportMode,
        }
        let raw = RawAuthenticationHeaderPolicyV1::deserialize(deserializer)?;
        Self::new(raw.allowed_header_names, raw.selected_transport).map_err(de::Error::custom)
    }
}

impl AuthenticationHeaderPolicyV1 {
    /// Creates a validated header policy (names only, never values).
    ///
    /// # Errors
    ///
    /// Returns a validation error when a header name is not a non-empty HTTP
    /// token of at most 128 characters, names are duplicated, or the selected
    /// transport and the allowed header names are inconsistent.
    pub fn new(
        allowed_header_names: Vec<String>,
        selected_transport: CredentialTransportMode,
    ) -> DtoResult<Self> {
        if !valid_header_names(&allowed_header_names) {
            return Err(ErrorDto::validation(
                "invalid_safe_header_name",
                "header policy names must be unique HTTP tokens of at most 128 characters",
            ));
        }
        let transport_is_consistent = match selected_transport {
            CredentialTransportMode::Bearer => allowed_header_names.is_empty(),
            CredentialTransportMode::SafeHeader => !allowed_header_names.is_empty(),
        };
        if !transport_is_consistent {
            return Err(ErrorDto::validation(
                "invalid_credential_transport",
                "bearer transport rejects header names and safe-header transport requires at least one",
            ));
        }
        Ok(Self {
            allowed_header_names,
            selected_transport,
        })
    }

    /// Returns the allowed header names (names only, never values).
    #[must_use]
    pub fn allowed_header_names(&self) -> &[String] {
        &self.allowed_header_names
    }

    /// Returns the selected credential transport mode.
    #[must_use]
    pub const fn selected_transport(&self) -> CredentialTransportMode {
        self.selected_transport
    }
}

/// Whether every declared header name is a unique non-empty HTTP token of at
/// most [`MAX_SAFE_HEADER_NAME_CHARS`] characters.
fn valid_header_names(names: &[String]) -> bool {
    let mut seen = std::collections::HashSet::with_capacity(names.len());
    names.iter().all(|name| {
        !name.is_empty()
            && name.len() <= MAX_SAFE_HEADER_NAME_CHARS
            && name.bytes().all(is_http_token_byte)
            && seen.insert(name.clone())
    })
}

/// Whether `byte` is one HTTP token character (`tchar`).
const fn is_http_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// Descriptor-declared native reasoning preservation controls.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderNativePreservationControlsV1 {
    preserve_thinking: bool,
    thinking_keep: bool,
}

impl ProviderNativePreservationControlsV1 {
    /// Creates native reasoning preservation controls.
    #[must_use]
    pub const fn new(preserve_thinking: bool, thinking_keep: bool) -> Self {
        Self {
            preserve_thinking,
            thinking_keep,
        }
    }

    /// Returns whether native thinking must be preserved.
    #[must_use]
    pub const fn preserve_thinking(self) -> bool {
        self.preserve_thinking
    }

    /// Returns whether the thinking payload must be kept.
    #[must_use]
    pub const fn thinking_keep(self) -> bool {
        self.thinking_keep
    }
}

/// Closed code-owned server-side parser bounds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ParserLimitsV1 {
    max_bytes: u64,
    max_nesting: u32,
    max_fields: u32,
    max_array_items: u32,
}

/// The closed maximum parser payload bytes (512 KiB).
const MAX_PARSER_MAX_BYTES: u64 = 512 * 1024;
/// The closed maximum parser nesting depth.
const MAX_PARSER_MAX_NESTING: u32 = 128;
/// The closed maximum parser field count.
const MAX_PARSER_MAX_FIELDS: u32 = 4096;
/// The closed maximum parser array-item count.
const MAX_PARSER_MAX_ARRAY_ITEMS: u32 = 65_536;

impl ParserLimitsV1 {
    /// Creates validated parser limits within the closed code-owned bounds.
    ///
    /// # Errors
    ///
    /// Returns a validation error when a limit is zero or exceeds its closed bound.
    pub fn new(
        max_bytes: u64,
        max_nesting: u32,
        max_fields: u32,
        max_array_items: u32,
    ) -> DtoResult<Self> {
        if max_bytes == 0
            || max_bytes > MAX_PARSER_MAX_BYTES
            || max_nesting == 0
            || max_nesting > MAX_PARSER_MAX_NESTING
            || max_fields == 0
            || max_fields > MAX_PARSER_MAX_FIELDS
            || max_array_items == 0
            || max_array_items > MAX_PARSER_MAX_ARRAY_ITEMS
        {
            return Err(ErrorDto::validation(
                "invalid_parser_limits",
                "parser limits must be positive and within the closed code-owned bounds",
            ));
        }
        Ok(Self {
            max_bytes,
            max_nesting,
            max_fields,
            max_array_items,
        })
    }

    /// Returns the closed maximum parser payload bytes.
    #[must_use]
    pub const fn max_bytes(self) -> u64 {
        self.max_bytes
    }

    /// Returns the closed maximum parser nesting depth.
    #[must_use]
    pub const fn max_nesting(self) -> u32 {
        self.max_nesting
    }

    /// Returns the closed maximum parser field count.
    #[must_use]
    pub const fn max_fields(self) -> u32 {
        self.max_fields
    }

    /// Returns the closed maximum parser array-item count.
    #[must_use]
    pub const fn max_array_items(self) -> u32 {
        self.max_array_items
    }
}

/// Closed server-side parser configuration; never raw JSON or templates.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ServerSideParserConfigV1 {
    /// No server-side parser is configured.
    None,
    /// A vLLM parser identified by its stable parser id.
    Vllm {
        parser_id: String,
        bounded_limits: ParserLimitsV1,
    },
    /// An SGLang parser identified by its stable parser id.
    Sglang {
        parser_id: String,
        bounded_limits: ParserLimitsV1,
    },
}

impl<'de> Deserialize<'de> for ServerSideParserConfigV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "mode", rename_all = "snake_case")]
        enum RawServerSideParserConfigV1 {
            None,
            Vllm {
                parser_id: String,
                bounded_limits: ParserLimitsV1,
            },
            Sglang {
                parser_id: String,
                bounded_limits: ParserLimitsV1,
            },
        }
        match RawServerSideParserConfigV1::deserialize(deserializer)? {
            RawServerSideParserConfigV1::None => Ok(Self::none()),
            RawServerSideParserConfigV1::Vllm {
                parser_id,
                bounded_limits,
            } => Self::vllm(parser_id, bounded_limits).map_err(de::Error::custom),
            RawServerSideParserConfigV1::Sglang {
                parser_id,
                bounded_limits,
            } => Self::sglang(parser_id, bounded_limits).map_err(de::Error::custom),
        }
    }
}

impl ServerSideParserConfigV1 {
    /// Creates the disabled parser configuration.
    #[must_use]
    pub const fn none() -> Self {
        Self::None
    }

    /// Creates a validated vLLM parser configuration.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the parser id is blank or carries control characters.
    pub fn vllm(parser_id: impl Into<String>, bounded_limits: ParserLimitsV1) -> DtoResult<Self> {
        Ok(Self::Vllm {
            parser_id: validate_parser_id(parser_id.into())?,
            bounded_limits,
        })
    }

    /// Creates a validated SGLang parser configuration.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the parser id is blank or carries control characters.
    pub fn sglang(parser_id: impl Into<String>, bounded_limits: ParserLimitsV1) -> DtoResult<Self> {
        Ok(Self::Sglang {
            parser_id: validate_parser_id(parser_id.into())?,
            bounded_limits,
        })
    }
}

/// Validates one stable parser identifier.
///
/// # Errors
///
/// Returns a validation error when the parser id is blank or carries control characters.
fn validate_parser_id(parser_id: String) -> DtoResult<String> {
    if parser_id.trim().is_empty() || parser_id.chars().any(char::is_control) {
        Err(ErrorDto::validation(
            "invalid_parser_id",
            "parser id must be a non-blank value without control characters",
        ))
    } else {
        Ok(parser_id)
    }
}

/// The closed model-capability taxonomy revision of the flattened envelope.
pub const MODEL_CAPABILITY_TAXONOMY_V1: &str = "model-capability-taxonomy-v1";

/// A flattened provider-neutral capability envelope.
///
/// The domain canonical `ModelCapabilitySetV1` (model-capability-taxonomy-v1)
/// remains the authoritative form; this DTO maps 1:1 onto that taxonomy and
/// exists so provider-neutral configuration can flow without a domain import.
/// The `intention-model` crate cannot import `intention-domain`, so the
/// flattened boolean fields are the provider-neutral projection of the
/// domain's closed capability set.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelCapabilityEnvelopeV1 {
    taxonomy_version: String,
    input_text_only: bool,
    text_streaming: bool,
    structured_output_unsupported: bool,
    reasoning: bool,
    tool_exchange: bool,
    context_preservation_local_durable_history: bool,
}

impl<'de> Deserialize<'de> for ModelCapabilityEnvelopeV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawModelCapabilityEnvelopeV1 {
            taxonomy_version: String,
            input_text_only: bool,
            text_streaming: bool,
            structured_output_unsupported: bool,
            reasoning: bool,
            tool_exchange: bool,
            context_preservation_local_durable_history: bool,
        }
        let raw = RawModelCapabilityEnvelopeV1::deserialize(deserializer)?;
        Self::new(
            raw.taxonomy_version,
            raw.input_text_only,
            raw.text_streaming,
            raw.structured_output_unsupported,
            raw.reasoning,
            raw.tool_exchange,
            raw.context_preservation_local_durable_history,
        )
        .map_err(de::Error::custom)
    }
}

impl ModelCapabilityEnvelopeV1 {
    /// Creates a validated flattened capability envelope.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the taxonomy version is not the closed
    /// `model-capability-taxonomy-v1` value.
    pub fn new(
        taxonomy_version: impl Into<String>,
        input_text_only: bool,
        text_streaming: bool,
        structured_output_unsupported: bool,
        reasoning: bool,
        tool_exchange: bool,
        context_preservation_local_durable_history: bool,
    ) -> DtoResult<Self> {
        let taxonomy_version = taxonomy_version.into();
        if taxonomy_version != MODEL_CAPABILITY_TAXONOMY_V1 {
            return Err(ErrorDto::validation(
                "invalid_model_capability_taxonomy",
                "model capability envelope requires the closed model-capability-taxonomy-v1 version",
            ));
        }
        Ok(Self {
            taxonomy_version,
            input_text_only,
            text_streaming,
            structured_output_unsupported,
            reasoning,
            tool_exchange,
            context_preservation_local_durable_history,
        })
    }

    /// Returns the closed taxonomy revision.
    #[must_use]
    pub fn taxonomy_version(&self) -> &str {
        &self.taxonomy_version
    }

    /// Returns whether the closed taxonomy declares text-only input.
    #[must_use]
    pub const fn input_text_only(&self) -> bool {
        self.input_text_only
    }

    /// Returns whether the closed taxonomy declares text streaming.
    #[must_use]
    pub const fn text_streaming(&self) -> bool {
        self.text_streaming
    }

    /// Returns whether the closed taxonomy declares structured output unsupported.
    #[must_use]
    pub const fn structured_output_unsupported(&self) -> bool {
        self.structured_output_unsupported
    }

    /// Returns whether the closed taxonomy declares reasoning support.
    #[must_use]
    pub const fn reasoning(&self) -> bool {
        self.reasoning
    }

    /// Returns whether the closed taxonomy declares tool exchange.
    #[must_use]
    pub const fn tool_exchange(&self) -> bool {
        self.tool_exchange
    }

    /// Returns whether the closed taxonomy declares local durable history context preservation.
    #[must_use]
    pub const fn context_preservation_local_durable_history(&self) -> bool {
        self.context_preservation_local_durable_history
    }
}

/// Optional typed reasoning usage.
///
/// This is the typed reasoning-usage component of a reported usage record:
/// a missing component means not reported (never zero), and no price or
/// currency is ever carried. The `UsageDto::Reported` attachment itself lives
/// in `intention-types`; this crate defines the component so provider-neutral
/// reasoning usage stays typed and credential-free.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ReasoningUsageDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_tokens: Option<u64>,
}

impl<'de> Deserialize<'de> for ReasoningUsageDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawReasoningUsageDto {
            #[serde(default)]
            input_tokens: Option<u64>,
            #[serde(default)]
            output_tokens: Option<u64>,
        }
        let raw = RawReasoningUsageDto::deserialize(deserializer)?;
        Self::new(raw.input_tokens, raw.output_tokens).map_err(de::Error::custom)
    }
}

impl ReasoningUsageDto {
    /// Creates optional typed reasoning usage.
    ///
    /// # Errors
    ///
    /// Returns a validation error when a reported component is zero.
    pub fn new(input_tokens: Option<u64>, output_tokens: Option<u64>) -> DtoResult<Self> {
        if input_tokens == Some(0) || output_tokens == Some(0) {
            return Err(ErrorDto::validation(
                "invalid_model_reasoning_usage",
                "reasoning usage components must be positive when reported",
            ));
        }
        Ok(Self {
            input_tokens,
            output_tokens,
        })
    }

    /// Returns reported reasoning input tokens, if reported.
    #[must_use]
    pub const fn input_tokens(self) -> Option<u64> {
        self.input_tokens
    }

    /// Returns reported reasoning output tokens, if reported.
    #[must_use]
    pub const fn output_tokens(self) -> Option<u64> {
        self.output_tokens
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
            | ModelEventDto::ReasoningSummaryDelta { .. }
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
            | ModelEventDto::ReasoningSummaryDelta { .. }
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

/// Provider-neutral cancellation state shared with a model execution stream.
#[derive(Clone, Default)]
pub struct ModelCancellationSignal {
    state: Arc<CancellationState>,
}

#[derive(Default)]
struct CancellationState {
    cancelled: AtomicBool,
    next_waiter_id: AtomicUsize,
    waiters: Mutex<Vec<(usize, Waker)>>,
}

impl ModelCancellationSignal {
    /// Creates a cancellation signal in its active state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    /// Requests cancellation and wakes every current waiter.
    pub fn cancel(&self) {
        if !self.state.cancelled.swap(true, Ordering::AcqRel) {
            let waiters = match self.state.waiters.lock() {
                Ok(mut waiters) => std::mem::take(&mut *waiters),
                Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
            };
            for (_, waiter) in waiters {
                waiter.wake();
            }
        }
    }

    /// Returns a fresh independently awaitable future that completes on cancellation.
    #[must_use]
    pub fn cancelled(&self) -> ModelCancelledFuture {
        ModelCancelledFuture {
            state: self.state.clone(),
            waiter_id: None,
        }
    }
}

/// Provider-neutral future returned by [`ModelCancellationSignal::cancelled`].
pub struct ModelCancelledFuture {
    state: Arc<CancellationState>,
    waiter_id: Option<usize>,
}

impl Future for ModelCancelledFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if self.state.cancelled.load(Ordering::Acquire) {
            return Poll::Ready(());
        }
        let waiter_id = self.waiter_id.unwrap_or_else(|| {
            let waiter_id = self.state.next_waiter_id.fetch_add(1, Ordering::Relaxed);
            self.waiter_id = Some(waiter_id);
            waiter_id
        });
        match self.state.waiters.lock() {
            Ok(mut waiters) => replace_waiter(&mut waiters, waiter_id, context.waker()),
            Err(poisoned) => {
                let mut waiters = poisoned.into_inner();
                replace_waiter(&mut waiters, waiter_id, context.waker());
            }
        }
        if self.state.cancelled.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl Drop for ModelCancelledFuture {
    fn drop(&mut self) {
        let Some(waiter_id) = self.waiter_id else {
            return;
        };
        match self.state.waiters.lock() {
            Ok(mut waiters) => remove_waiter(&mut waiters, waiter_id),
            Err(poisoned) => remove_waiter(&mut poisoned.into_inner(), waiter_id),
        }
    }
}

fn replace_waiter(waiters: &mut Vec<(usize, Waker)>, waiter_id: usize, waker: &Waker) {
    if let Some((_, current)) = waiters.iter_mut().find(|(id, _)| *id == waiter_id) {
        if !current.will_wake(waker) {
            *current = waker.clone();
        }
    } else {
        waiters.push((waiter_id, waker.clone()));
    }
}

fn remove_waiter(waiters: &mut Vec<(usize, Waker)>, waiter_id: usize) {
    waiters.retain(|(id, _)| *id != waiter_id);
}

/// Ordered provider-neutral execution stream.
pub type ModelEventStream =
    Pin<Box<dyn Stream<Item = Result<ModelEventDto, ProviderErrorDto>> + Send>>;

/// Provider-neutral asynchronous execution boundary.
pub trait ModelExecutionDriver: ModelDriver {
    /// Starts a validated model request and returns ordered normalized provider events.
    fn execute(
        &self,
        request: ModelRequestDto,
        cancellation: ModelCancellationSignal,
    ) -> ModelEventStream;
}
