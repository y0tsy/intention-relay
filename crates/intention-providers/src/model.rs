//! Provider-neutral model contracts (including tool calls and results) and validated stream facts.
//!
//! This module contains no provider SDK, asynchronous runtime, or transport
//! resources. Provider adapters translate their native responses into these
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
use intention_proto::{
    DtoResult, ErrorDto, ErrorRetryDto, ReasoningHistoryTransferDto, RunId, ToolCallId,
};
pub use intention_proto::{
    FinishReasonDto, ProviderErrorDto, ProviderHealthEvidenceDto, ProviderHealthReasonDto,
    ProviderHealthStateDto, ProviderModelRecordDto, ReasoningFragmentCategoryDto, ToolCallDto,
    UsageDto,
};
use serde::Serialize;

/// The closed failure code for a malformed or out-of-order reasoning value.
pub const PROVIDER_REASONING_STREAM_INVALID: &str = "provider_reasoning_stream_invalid";

/// The closed failure code for one over-bound reasoning fragment.
pub const PROVIDER_REASONING_FRAGMENT_TOO_LARGE: &str = "provider_reasoning_fragment_too_large";

/// The closed failure code for a driver that cannot probe its provider.
pub const PROVIDER_PROBE_UNSUPPORTED: &str = "provider_probe_unsupported";

/// Maximum bytes of one normalized reasoning fragment or summary delta.
///
/// The bound is a representation bound on provider reasoning text: a larger
/// fragment fails normalization with `provider_reasoning_fragment_too_large`
/// and is never truncated. The combined per-run reasoning bound stays the
/// runtime's `reasoning_output_limit_exceeded`.
pub const MAX_MODEL_REASONING_FRAGMENT_BYTES: usize = 512 * 1024;

/// The sender role of a model-context message, including tool calls and results.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
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
    /// A daemon-synthesized notice about prior run state.
    Notice,
}

/// A validated model-context message, text-only or carrying tool calls or a tool result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelMessageDto {
    role: ModelRoleDto,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCallDto>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<ToolCallId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_history: Option<AssistantReasoningHistoryDto>,
    #[serde(skip_serializing_if = "is_false")]
    cache_control: bool,
}

/// Reports whether an optional message flag stays off the wire.
const fn is_false(value: &bool) -> bool {
    !*value
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
            reasoning_history: None,
            cache_control: false,
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
            reasoning_history: None,
            cache_control: false,
        })
    }

    /// Creates an assistant message carrying cross-turn reasoning history.
    ///
    /// The history stays a typed attachment beside the assistant text: message
    /// content carries the response's own text and never prior reasoning.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the content is blank.
    pub fn assistant_with_reasoning_history(
        content: impl Into<String>,
        history: AssistantReasoningHistoryDto,
    ) -> DtoResult<Self> {
        let mut message = Self::new(ModelRoleDto::Assistant, content)?;
        message.reasoning_history = Some(history);
        Ok(message)
    }

    /// Attaches cross-turn reasoning history to this assistant message.
    ///
    /// # Errors
    ///
    /// Returns a validation error when this message is not an assistant message.
    pub fn attach_reasoning_history(
        &mut self,
        history: AssistantReasoningHistoryDto,
    ) -> DtoResult<()> {
        if self.role != ModelRoleDto::Assistant {
            return Err(ErrorDto::validation(
                "invalid_model_message_role",
                "only assistant messages carry reasoning history",
            ));
        }
        self.reasoning_history = Some(history);
        Ok(())
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
            reasoning_history: None,
            cache_control: false,
        })
    }

    /// Replaces this message's content, keeping its role and tool shape.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the replacement content is blank.
    pub fn replace_content(&mut self, content: impl Into<String>) -> DtoResult<()> {
        let content = content.into();
        if content.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_model_message_content",
                "model message content must not be empty",
            ));
        }
        self.content = content;
        Ok(())
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

    /// Returns the cross-turn reasoning history attached to this assistant
    /// message, if any.
    #[must_use]
    pub const fn reasoning_history(&self) -> Option<&AssistantReasoningHistoryDto> {
        self.reasoning_history.as_ref()
    }

    /// Returns whether this message marks the end of one cacheable prompt prefix.
    ///
    /// The flag is a transient request-assembly hint: it never changes message
    /// content, role, or tool shape, and providers translate it into their
    /// prompt-cache marker when they support one.
    #[must_use]
    pub const fn cache_control(&self) -> bool {
        self.cache_control
    }

    /// Marks or clears the prompt-cache breakpoint carried by this message.
    pub const fn set_cache_control(&mut self, cache_control: bool) {
        self.cache_control = cache_control;
    }
}

/// Maximum bytes of one transient assistant-reasoning attachment.
///
/// The bound caps the reasoning text one provider response may attach to its
/// tool calls for the same-run tool-loop continuation.
const MAX_MODEL_ASSISTANT_REASONING_BYTES: usize = 512 * 1024;

/// Transient assistant reasoning attached to the tool calls of one provider
/// response; preserved only for the same-run tool-loop continuation, never
/// durable, never message content.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AssistantReasoningDto {
    tool_call_ids: Vec<ToolCallId>,
    text: String,
}

impl AssistantReasoningDto {
    /// Creates the reasoning attached to one provider round's tool calls.
    ///
    /// The text may be empty: a provider can carry the reasoning channel with
    /// no textual content, and that presence alone must round-trip into the
    /// continuation request.
    ///
    /// # Errors
    ///
    /// Returns a validation error when no tool-call identity is attached, a
    /// tool-call identity repeats, or the text exceeds 512 KiB or carries a
    /// control character other than a line break or tab.
    pub fn new(tool_call_ids: Vec<ToolCallId>, text: impl Into<String>) -> DtoResult<Self> {
        let text = text.into();
        if !valid_assistant_reasoning_tool_call_ids(&tool_call_ids) {
            return Err(ErrorDto::validation(
                "invalid_model_assistant_reasoning",
                "assistant reasoning requires at least one unique tool-call identity",
            ));
        }
        if text.len() > MAX_MODEL_ASSISTANT_REASONING_BYTES
            || text.chars().any(forbidden_reasoning_text_character)
        {
            return Err(ErrorDto::validation(
                "invalid_model_assistant_reasoning_text",
                "assistant reasoning text must be at most 512 KiB without invalid control characters",
            ));
        }
        Ok(Self {
            tool_call_ids,
            text,
        })
    }

    /// Returns the identities of the tool calls this reasoning belongs to.
    #[must_use]
    pub fn tool_call_ids(&self) -> &[ToolCallId] {
        &self.tool_call_ids
    }

    /// Returns the reasoning text; empty means the channel carried no text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Whether the attached tool-call identities are present and unique.
fn valid_assistant_reasoning_tool_call_ids(tool_call_ids: &[ToolCallId]) -> bool {
    let mut seen = std::collections::HashSet::with_capacity(tool_call_ids.len());
    !tool_call_ids.is_empty() && tool_call_ids.iter().all(|id| seen.insert(*id))
}

/// Whether `character` must be rejected in round-tripped reasoning text.
///
/// Line breaks and tabs are ordinary reasoning text; every other control
/// character is transport noise that must never re-enter a provider request.
const fn forbidden_reasoning_text_character(character: char) -> bool {
    character.is_control() && !matches!(character, '\n' | '\r' | '\t')
}

/// Maximum bytes of one attached cross-turn reasoning history.
///
/// The bound is the frozen history aggregate bound: a larger attachment fails
/// validation whole and is never truncated into a partial history.
const MAX_MODEL_REASONING_HISTORY_BYTES: usize = 4 * 1024 * 1024;

/// Validated cross-turn reasoning history attached to one assistant message.
///
/// The attachment is the durable form of one prior response's reasoning under
/// the descriptor's transfer compatibility identity. Fragments keep the order
/// they were recorded in and carry their recorded category; summaries follow at
/// the tail. The history travels beside the assistant text and never inside it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AssistantReasoningHistoryDto {
    compatibility_id: String,
    fragments: Vec<(ReasoningFragmentCategoryDto, String)>,
    summaries: Vec<String>,
}

impl AssistantReasoningHistoryDto {
    /// Creates one validated cross-turn reasoning history attachment.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the compatibility identity is not a safe
    /// token, the attachment carries neither fragment nor summary, one fragment
    /// or summary exceeds 512 KiB or carries an invalid control character, or
    /// the aggregate exceeds the frozen 4 MiB history bound.
    pub fn new(
        compatibility_id: impl Into<String>,
        fragments: Vec<(ReasoningFragmentCategoryDto, String)>,
        summaries: Vec<String>,
    ) -> DtoResult<Self> {
        let compatibility_id = compatibility_id.into();
        // The compatibility identity is the transfer contract's own token, so
        // the frozen transfer validator owns that rule instead of a duplicate.
        ReasoningHistoryTransferDto::textual_history_v1(compatibility_id.clone())?;
        if fragments.is_empty() && summaries.is_empty() {
            return Err(ErrorDto::validation(
                "invalid_model_assistant_reasoning_history",
                "attached reasoning history requires at least one fragment or summary",
            ));
        }
        let mut aggregate = 0_usize;
        for (_, content) in &fragments {
            check_reasoning_history_text(content, &mut aggregate)?;
        }
        for summary in &summaries {
            check_reasoning_history_text(summary, &mut aggregate)?;
        }
        Ok(Self {
            compatibility_id,
            fragments,
            summaries,
        })
    }

    /// Returns the descriptor transfer compatibility identity of this history.
    #[must_use]
    pub fn compatibility_id(&self) -> &str {
        &self.compatibility_id
    }

    /// Returns the ordered reasoning fragments with their recorded categories.
    #[must_use]
    pub fn fragments(&self) -> &[(ReasoningFragmentCategoryDto, String)] {
        &self.fragments
    }

    /// Returns the ordered reasoning summaries that follow the fragments.
    #[must_use]
    pub fn summaries(&self) -> &[String] {
        &self.summaries
    }
}

/// Validates one history text against the per-fragment and aggregate bounds.
///
/// # Errors
///
/// Returns a validation error when one text exceeds its per-fragment bound or
/// carries an invalid control character, or when the aggregate exceeds the
/// frozen history bound.
fn check_reasoning_history_text(text: &str, aggregate: &mut usize) -> DtoResult<()> {
    if text.len() > MAX_MODEL_REASONING_FRAGMENT_BYTES
        || text.chars().any(forbidden_reasoning_text_character)
    {
        return Err(ErrorDto::validation(
            "invalid_model_assistant_reasoning_history_text",
            "reasoning history text must be at most 512 KiB without invalid control characters",
        ));
    }
    *aggregate += text.len();
    if *aggregate > MAX_MODEL_REASONING_HISTORY_BYTES {
        return Err(ErrorDto::validation(
            "invalid_model_assistant_reasoning_history_size",
            "attached reasoning history must not exceed the 4 MiB history bound",
        ));
    }
    Ok(())
}

/// The explicit capabilities declared by a selected provider driver.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ModelCapabilitiesDto {
    text: bool,
    reasoning: bool,
    reasoning_summary: bool,
    tool_calls: bool,
    multimodal: bool,
    vendor_extensions: bool,
    streaming: bool,
}

impl ModelCapabilitiesDto {
    /// Creates an explicit capability declaration.
    ///
    /// The declaration grants no reasoning-summary support: summary output is
    /// opt-in through [`Self::with_reasoning_summary`], so a driver that does
    /// not name it never claims it.
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
            reasoning_summary: false,
            tool_calls,
            multimodal,
            vendor_extensions,
            streaming,
        }
    }

    /// Declares or denies reasoning-summary output support.
    #[must_use]
    pub const fn with_reasoning_summary(mut self, reasoning_summary: bool) -> Self {
        self.reasoning_summary = reasoning_summary;
        self
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

    /// Returns whether reasoning-summary output is supported.
    #[must_use]
    pub const fn supports_reasoning_summary(self) -> bool {
        self.reasoning_summary
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

    /// Verifies this declaration can serve every request the runtime builds.
    ///
    /// The runtime streams text with tool calls, so a driver that cannot
    /// declare both cannot serve any run. The check runs once when the
    /// configuration-selected provider is composed. A test-support facade that
    /// injects its own driver is exempt: it carries the injected driver's own
    /// declaration instead of a configuration-selected one.
    ///
    /// # Errors
    ///
    /// Returns a policy error when text or tool-call support is not declared.
    pub fn ensure_runtime_requirements(self) -> DtoResult<()> {
        if self.text && self.tool_calls {
            Ok(())
        } else {
            Err(ErrorDto::new(
                "unsupported_model_capability",
                intention_proto::ErrorCategoryDto::Policy,
                "the selected provider cannot serve streamed text with tool calls",
                ErrorRetryDto::Never,
                None,
            )?)
        }
    }
}

/// Maximum characters of one tool-definition name (provider function-name constraint).
const MAX_TOOL_DEFINITION_NAME_CHARS: usize = 64;

/// Maximum bytes of one tool-definition JSON parameter document.
const MAX_TOOL_DEFINITION_PARAMETERS_BYTES: usize = 65_536;

/// A validated model tool definition advertised to a provider.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelToolDefinitionDto {
    name: String,
    description: String,
    parameters_json: String,
}

impl ModelToolDefinitionDto {
    /// Creates a validated tool definition with object-shaped JSON parameters.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the name is not an ASCII `[A-Za-z0-9_-]`
    /// token of at most 64 characters, the description is blank, or the
    /// parameters are empty, larger than 64 KiB, or not a JSON object.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters_json: impl Into<String>,
    ) -> DtoResult<Self> {
        let name = name.into();
        let description = description.into();
        let parameters_json = parameters_json.into();
        if name.trim().is_empty()
            || name.len() > MAX_TOOL_DEFINITION_NAME_CHARS
            || !name.bytes().all(is_tool_definition_name_byte)
        {
            return Err(ErrorDto::validation(
                "invalid_tool_definition_name",
                "tool definition name must be an ASCII [A-Za-z0-9_-] token of at most 64 characters",
            ));
        }
        if description.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_tool_definition_description",
                "tool definition description must not be empty",
            ));
        }
        if parameters_json.is_empty()
            || parameters_json.len() > MAX_TOOL_DEFINITION_PARAMETERS_BYTES
        {
            return Err(invalid_tool_definition_parameters());
        }
        let _: std::collections::BTreeMap<String, serde::de::IgnoredAny> =
            serde_json::from_str(&parameters_json)
                .map_err(|_| invalid_tool_definition_parameters())?;
        Ok(Self {
            name,
            description,
            parameters_json,
        })
    }

    /// Returns the advertised function name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the model-facing tool description.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns the validated JSON object text describing the tool parameters.
    #[must_use]
    pub fn parameters_json(&self) -> &str {
        &self.parameters_json
    }
}

/// Whether `byte` is one provider function-name character (`[A-Za-z0-9_-]`).
const fn is_tool_definition_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

/// The validation error for invalid tool-definition JSON parameters.
fn invalid_tool_definition_parameters() -> ErrorDto {
    ErrorDto::validation(
        "invalid_tool_definition_parameters",
        "tool definition parameters must be a non-empty JSON object of at most 64 KiB",
    )
}

/// A validated provider-neutral model request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelRequestDto {
    run_id: RunId,
    model: String,
    messages: Vec<ModelMessageDto>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ModelToolDefinitionDto>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    assistant_reasoning: Vec<AssistantReasoningDto>,
    system_context: Option<String>,
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
            tools: Vec::new(),
            assistant_reasoning: Vec::new(),
            system_context,
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

    /// Returns the tool definitions advertised with this request.
    #[must_use]
    pub fn tools(&self) -> &[ModelToolDefinitionDto] {
        &self.tools
    }

    /// Returns the transient reasoning attachments carried into the same-run
    /// tool-loop continuation.
    #[must_use]
    pub fn assistant_reasoning(&self) -> &[AssistantReasoningDto] {
        &self.assistant_reasoning
    }

    /// Returns a copy of this request with the model context messages replaced.
    ///
    /// Advertised tools and transient reasoning attachments are preserved.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the replacement message list is empty.
    pub fn with_messages(&self, messages: Vec<ModelMessageDto>) -> DtoResult<Self> {
        let mut request = Self::new(
            self.run_id,
            self.model.clone(),
            messages,
            self.system_context.clone(),
        )?;
        request.tools = self.tools.clone();
        request.assistant_reasoning = self.assistant_reasoning.clone();
        Ok(request)
    }

    /// Returns a copy of this request with the advertised tool definitions replaced.
    ///
    /// Transient reasoning attachments are preserved.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the retained request fields no longer
    /// satisfy request validation.
    pub fn with_tools(&self, tools: Vec<ModelToolDefinitionDto>) -> DtoResult<Self> {
        let mut request = Self::new(
            self.run_id,
            self.model.clone(),
            self.messages.clone(),
            self.system_context.clone(),
        )?;
        request.tools = tools;
        request.assistant_reasoning = self.assistant_reasoning.clone();
        Ok(request)
    }

    /// Returns a copy of this request with the full ordered transient reasoning
    /// attachments replaced.
    ///
    /// One attachment belongs to each assistant tool-call message of the
    /// same-run tool loop; attachments are neither durable nor message content,
    /// and later rebuilds through [`Self::with_messages`] or [`Self::with_tools`]
    /// preserve them.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the retained request fields no longer
    /// satisfy request validation.
    pub fn with_assistant_reasoning(
        &self,
        reasoning: Vec<AssistantReasoningDto>,
    ) -> DtoResult<Self> {
        let mut request = Self::new(
            self.run_id,
            self.model.clone(),
            self.messages.clone(),
            self.system_context.clone(),
        )?;
        request.tools = self.tools.clone();
        request.assistant_reasoning = reasoning;
        Ok(request)
    }

    /// Returns optional daemon-selected system context.
    #[must_use]
    pub fn system_context(&self) -> Option<&str> {
        self.system_context.as_deref()
    }
}

/// A provider-neutral normalized stream fact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelEventDto {
    /// The provider stream started successfully.
    Started,
    /// A non-empty text content delta arrived.
    TextDelta { content: String },
    /// A non-empty categorized reasoning fragment arrived.
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

    /// Creates a non-empty categorized reasoning fragment.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the fragment is empty, and the closed
    /// `provider_reasoning_fragment_too_large` failure when it exceeds
    /// [`MAX_MODEL_REASONING_FRAGMENT_BYTES`].
    pub fn reasoning_delta(
        category: ReasoningFragmentCategoryDto,
        content: impl Into<String>,
    ) -> DtoResult<Self> {
        let content = content.into();
        if content.is_empty() {
            return Err(ErrorDto::validation(
                "invalid_model_reasoning_delta",
                "model reasoning delta must not be empty",
            ));
        }
        ensure_reasoning_fragment_bound(&content)?;
        Ok(Self::ReasoningDelta { category, content })
    }

    /// Creates a non-empty reasoning summary delta.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the summary is empty, and the closed
    /// `provider_reasoning_fragment_too_large` failure when it exceeds
    /// [`MAX_MODEL_REASONING_FRAGMENT_BYTES`].
    pub fn reasoning_summary_delta(content: impl Into<String>) -> DtoResult<Self> {
        let content = content.into();
        if content.is_empty() {
            return Err(ErrorDto::validation(
                "invalid_model_reasoning_summary_delta",
                "model reasoning summary delta must not be empty",
            ));
        }
        ensure_reasoning_fragment_bound(&content)?;
        Ok(Self::ReasoningSummaryDelta { content })
    }

    /// Creates the textless primary reasoning-channel presence marker.
    ///
    /// A provider can carry the reasoning channel with no textual content, and
    /// that presence alone must round-trip into the continuation request: the
    /// assistant tool-call message keeps the channel beside its tool calls even
    /// when the channel held no text. The marker never becomes a
    /// durable reasoning fact and never reaches assistant content; the
    /// non-empty constructor stays the only source of reasoning text.
    #[must_use]
    pub const fn reasoning_presence() -> Self {
        Self::ReasoningDelta {
            category: ReasoningFragmentCategoryDto::Primary,
            content: String::new(),
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

/// Validates the per-fragment reasoning representation bound.
///
/// # Errors
///
/// Returns the closed `provider_reasoning_fragment_too_large` failure when the
/// fragment exceeds [`MAX_MODEL_REASONING_FRAGMENT_BYTES`].
fn ensure_reasoning_fragment_bound(content: &str) -> DtoResult<()> {
    if content.len() > MAX_MODEL_REASONING_FRAGMENT_BYTES {
        return Err(ErrorDto::validation(
            PROVIDER_REASONING_FRAGMENT_TOO_LARGE,
            "one normalized reasoning fragment must not exceed 512 KiB",
        ));
    }
    Ok(())
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
    /// Category-bearing reasoning fragments and reasoning summaries are live
    /// facts between the start fact and the terminal fact; their textless
    /// primary presence marker is one of those fragments. A reasoning fact
    /// outside that window fails with the closed
    /// `provider_reasoning_stream_invalid` failure, and any other out-of-order
    /// fact fails with the ordinary stream-order failure.
    ///
    /// # Errors
    ///
    /// Returns a validation error if the event cannot occur at this stream position.
    pub fn accept(&mut self, event: &ModelEventDto) -> DtoResult<()> {
        match event {
            ModelEventDto::Started => {
                if self.started || self.terminal {
                    return Err(stream_order_error());
                }
                self.started = true;
                Ok(())
            }
            ModelEventDto::Finished { .. } => {
                if !self.started || self.terminal {
                    return Err(stream_order_error());
                }
                self.terminal = true;
                Ok(())
            }
            ModelEventDto::Usage { .. } => {
                if !self.started || self.terminal || self.usage_seen {
                    return Err(stream_order_error());
                }
                self.usage_seen = true;
                Ok(())
            }
            ModelEventDto::TextDelta { .. } | ModelEventDto::ToolCall { .. } => {
                if self.started && !self.terminal {
                    Ok(())
                } else {
                    Err(stream_order_error())
                }
            }
            ModelEventDto::ReasoningDelta { .. } | ModelEventDto::ReasoningSummaryDelta { .. } => {
                if self.started && !self.terminal {
                    Ok(())
                } else {
                    Err(reasoning_stream_error())
                }
            }
        }
    }
}

fn stream_order_error() -> ErrorDto {
    ErrorDto::validation(
        "invalid_model_stream_order",
        "model stream event order is invalid",
    )
}

fn reasoning_stream_error() -> ErrorDto {
    ErrorDto::validation(
        PROVIDER_REASONING_STREAM_INVALID,
        "a reasoning value arrived outside the live reasoning window",
    )
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

    /// Clears a cancellation request so the same run can observe the next one.
    ///
    /// An interrupt ends one in-flight operation and the run continues; the
    /// executor that handled the interruption resets the signal, so a later
    /// interrupt reaches the same shared signal again.
    pub fn reset(&self) {
        self.state.cancelled.store(false, Ordering::Release);
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

/// One provider-neutral probe future returned by the driver probe boundary.
pub type ProviderProbeFuture<T> = Pin<Box<dyn Future<Output = DtoResult<T>> + Send + 'static>>;

/// Provider-neutral stream driver boundary. SDK and runtime resources stay private to providers/runtime.
pub trait ModelExecutionDriver {
    /// Returns the static capability declaration for this configured driver.
    fn capabilities(&self) -> ModelCapabilitiesDto;

    /// Starts a validated model request and returns ordered normalized provider events.
    fn execute(
        &self,
        request: ModelRequestDto,
        cancellation: ModelCancellationSignal,
    ) -> ModelEventStream;

    /// Probes the configured provider for non-authorizing health evidence.
    ///
    /// A probe is one request with no retry and no automatic continuation, and
    /// the evidence it returns never creates a run, retry counter, or quota.
    /// The default fails closed: a driver that declares no probe path never
    /// invents evidence about its provider.
    ///
    /// # Errors
    ///
    /// Returns the closed `provider_probe_unsupported` failure when this driver
    /// declares no health probe path.
    fn health_probe(&self) -> ProviderProbeFuture<ProviderHealthEvidenceDto> {
        Box::pin(async { Err(probe_unsupported()) })
    }

    /// Lists the provider's discovered model records.
    ///
    /// Discovery is additive evidence: it never authorizes a selection, and a
    /// repeated model identity stays a provider fact rather than a local
    /// decision. No retry and no automatic continuation follows. The default
    /// fails closed with a typed error instead of inventing records.
    ///
    /// # Errors
    ///
    /// Returns the closed `provider_probe_unsupported` failure when this driver
    /// declares no model listing path.
    fn list_models(&self) -> ProviderProbeFuture<Vec<ProviderModelRecordDto>> {
        Box::pin(async { Err(probe_unsupported()) })
    }
}

/// The closed failure for a driver that cannot probe its provider.
fn probe_unsupported() -> ErrorDto {
    ErrorDto::unavailable(
        PROVIDER_PROBE_UNSUPPORTED,
        "the configured driver has no provider probe path",
    )
}
