//! Generic OpenAI Chat Completions provider normalization.
//!
//! `async-openai` remains a private implementation dependency. This adapter
//! supports only text context/output, reported usage, finish reasons, and
//! function-style tool calls. Reasoning, multimodal, and vendor extensions fail
//! preflight before an outbound request is prepared.

use async_openai::{
    Client,
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestAssistantMessageContent,
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
        ChatCompletionRequestSystemMessageContent, ChatCompletionRequestUserMessage,
        ChatCompletionRequestUserMessageContent, CreateChatCompletionRequest,
        CreateChatCompletionRequestArgs, FinishReason,
    },
};
use intention_config::{ProviderKindDto, ResolvedConfigDto, StartupProviderMaterial};
use intention_model::{
    FinishReasonDto, ModelCapabilitiesDto, ModelDriver, ModelEventDto, ModelMessageDto,
    ModelRequestDto, ModelRoleDto, ProviderErrorDto, ToolCallDto, UsageDto,
};
use intention_types::{DtoResult, ErrorDto, ToolCallId};

/// Generic Chat Completions driver with private SDK client state.
pub struct GenericChatDriver {
    resolved: ResolvedConfigDto,
    client: Client<OpenAIConfig>,
    outbound_calls_for_test: u32,
}

impl std::fmt::Debug for GenericChatDriver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GenericChatDriver")
            .field("provider", &self.resolved.provider().kind())
            .field("model", &self.resolved.provider().model())
            .finish_non_exhaustive()
    }
}

impl GenericChatDriver {
    /// Creates the driver from opaque startup-only provider material.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the material selects a different provider kind.
    pub fn from_startup_material(material: StartupProviderMaterial) -> DtoResult<Self> {
        material.into_parts_for_provider(Self::with_credential)
    }

    fn with_credential(resolved: ResolvedConfigDto, credential: String) -> DtoResult<Self> {
        if resolved.provider().kind() != ProviderKindDto::GenericChatCompletionApi {
            return Err(ErrorDto::validation(
                "invalid_generic_chat_provider_config",
                "generic chat driver requires generic chat provider configuration",
            ));
        }
        let endpoint = resolved.provider().endpoint().ok_or_else(|| {
            ErrorDto::validation(
                "missing_generic_chat_endpoint",
                "generic chat provider requires a configured endpoint",
            )
        })?;
        let client = Client::with_config(
            OpenAIConfig::new()
                .with_api_base(endpoint)
                .with_api_key(credential),
        );
        Ok(Self {
            resolved,
            client,
            outbound_calls_for_test: 0,
        })
    }

    /// Returns the non-network preflight validation result.
    ///
    /// # Errors
    ///
    /// Returns a policy error for unsupported request capabilities.
    pub fn preflight(&self, request: &ModelRequestDto) -> DtoResult<()> {
        ModelDriver::preflight(self, request)
    }

    /// Prepares the private Chat Completions request after capability validation.
    ///
    /// This method only validates and translates the DTO into the SDK request
    /// shape. It never starts outbound work; the daemon-owned Tokio stream loop
    /// remains a later M4 package responsibility.
    ///
    /// # Errors
    ///
    /// Returns a safe policy or translation error before outbound work.
    pub fn prepare_request(&mut self, request: &ModelRequestDto) -> DtoResult<()> {
        self.preflight(request)?;
        let _native_request = translate_request(request)?;
        let _client = &self.client;
        self.outbound_calls_for_test = self.outbound_calls_for_test.saturating_add(1);
        Ok(())
    }

    /// Returns the number of SDK request preparations completed in this process.
    ///
    /// This is diagnostic state only; preparation is not an outbound provider call.
    #[must_use]
    pub const fn prepared_request_count(&self) -> u32 {
        self.outbound_calls_for_test
    }

    /// Maps a text delta fixture into the canonical stream contract.
    ///
    /// # Errors
    ///
    /// Returns a validation error for an empty text delta.
    pub fn map_fixture_text(content: &str) -> DtoResult<ModelEventDto> {
        ModelEventDto::text_delta(content)
    }

    /// Maps reported token counts into canonical usage.
    ///
    /// # Errors
    ///
    /// Returns a validation error for inconsistent token totals.
    pub fn map_fixture_usage(
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
    ) -> DtoResult<UsageDto> {
        UsageDto::reported(input_tokens, output_tokens, total_tokens)
    }

    /// Maps a Chat Completions finish reason string without exposing SDK types.
    #[must_use]
    pub fn map_fixture_finish(reason: &str) -> FinishReasonDto {
        match reason {
            "stop" => FinishReasonDto::Stop,
            "length" => FinishReasonDto::Length,
            "tool_calls" | "function_call" => FinishReasonDto::ToolCalls,
            "content_filter" => FinishReasonDto::ContentFilter,
            "error" => FinishReasonDto::Error,
            _ => FinishReasonDto::Unknown,
        }
    }

    /// Maps a complete function call fixture into the canonical tool-call contract.
    ///
    /// # Errors
    ///
    /// Returns a validation error for an invalid function-call shape.
    pub fn map_fixture_tool_call(
        _provider_call_id: &str,
        name: &str,
        arguments_json: &str,
    ) -> DtoResult<ToolCallDto> {
        ToolCallDto::new(ToolCallId::new(), name, arguments_json)
    }

    /// Maps a native status category to a safe provider error without native text.
    ///
    /// # Errors
    ///
    /// Returns a validation error only if the fixed normalized error is malformed.
    pub fn map_fixture_error(status: u16, _native_message: &str) -> DtoResult<ProviderErrorDto> {
        ProviderErrorDto::unavailable(
            if status == 429 || status >= 500 {
                "generic_chat_provider_unavailable"
            } else {
                "generic_chat_provider_request_rejected"
            },
            status == 429 || status >= 500,
            None,
        )
    }
}

impl ModelDriver for GenericChatDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, false, true, false, false, true)
    }
}

fn translate_request(request: &ModelRequestDto) -> DtoResult<CreateChatCompletionRequest> {
    let mut messages = Vec::new();
    if let Some(context) = request.system_context() {
        messages.push(ChatCompletionRequestMessage::System(
            ChatCompletionRequestSystemMessage {
                content: ChatCompletionRequestSystemMessageContent::Text(context.to_owned()),
                name: None,
            },
        ));
    }
    for message in request.messages() {
        messages.push(translate_message(message)?);
    }
    CreateChatCompletionRequestArgs::default()
        .model(request.model())
        .messages(messages)
        .build()
        .map_err(|_| {
            ErrorDto::validation(
                "invalid_generic_chat_request",
                "generic chat request could not be translated",
            )
        })
}

fn translate_message(message: &ModelMessageDto) -> DtoResult<ChatCompletionRequestMessage> {
    match message.role() {
        ModelRoleDto::System => Ok(ChatCompletionRequestMessage::System(
            ChatCompletionRequestSystemMessage {
                content: ChatCompletionRequestSystemMessageContent::Text(
                    message.content().to_owned(),
                ),
                name: None,
            },
        )),
        ModelRoleDto::User => Ok(ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessage {
                content: ChatCompletionRequestUserMessageContent::Text(
                    message.content().to_owned(),
                ),
                name: None,
            },
        )),
        ModelRoleDto::Assistant => ChatCompletionRequestAssistantMessageArgs::default()
            .content(ChatCompletionRequestAssistantMessageContent::Text(
                message.content().to_owned(),
            ))
            .build()
            .map(ChatCompletionRequestMessage::Assistant)
            .map_err(|_| {
                ErrorDto::validation(
                    "invalid_generic_chat_request",
                    "generic chat request could not be translated",
                )
            }),
    }
}

#[allow(
    dead_code,
    reason = "Native conversion remains private until the daemon-owned runtime starts provider streams in a later M4 package."
)]
const fn map_native_finish(reason: FinishReason) -> FinishReasonDto {
    match reason {
        FinishReason::Stop => FinishReasonDto::Stop,
        FinishReason::Length => FinishReasonDto::Length,
        FinishReason::ToolCalls | FinishReason::FunctionCall => FinishReasonDto::ToolCalls,
        FinishReason::ContentFilter => FinishReasonDto::ContentFilter,
    }
}
