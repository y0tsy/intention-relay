//! OpenRouter provider normalization backed privately by `openrouter-rs`.
//!
//! This adapter owns OpenRouter SDK construction and request translation. It
//! emits only `intention-model` DTOs, never invokes tools, storage, transport,
//! or event publication, and does not start a runtime-owned stream in this
//! package.

use intention_config::{ProviderKindDto, ResolvedConfigDto, StartupProviderMaterial};
use intention_model::{
    FinishReasonDto, ModelCapabilitiesDto, ModelDriver, ModelEventDto, ModelMessageDto,
    ModelRequestDto, ModelRoleDto, ProviderErrorDto, ToolCallDto, UsageDto,
};
use intention_types::{DtoResult, ErrorDto, ToolCallId};
use openrouter_rs::{
    OpenRouterClient,
    api::chat::{ChatCompletionRequest, Message},
    types::Role,
};

/// OpenRouter driver with private SDK client state.
pub struct OpenRouterDriver {
    resolved: ResolvedConfigDto,
    client: OpenRouterClient,
    outbound_calls_for_test: u32,
}

impl std::fmt::Debug for OpenRouterDriver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenRouterDriver")
            .field("provider", &self.resolved.provider().kind())
            .field("model", &self.resolved.provider().model())
            .finish_non_exhaustive()
    }
}

impl OpenRouterDriver {
    /// Creates a driver from opaque startup-only provider material.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the material does not select OpenRouter.
    pub fn from_startup_material(material: StartupProviderMaterial) -> DtoResult<Self> {
        material.into_parts_for_provider(Self::with_credential)
    }

    fn with_credential(resolved: ResolvedConfigDto, credential: String) -> DtoResult<Self> {
        if resolved.provider().kind() != ProviderKindDto::Openrouter {
            return Err(ErrorDto::validation(
                "invalid_openrouter_provider_config",
                "OpenRouter driver requires OpenRouter provider configuration",
            ));
        }
        let client = OpenRouterClient::builder()
            .api_key(credential)
            .build()
            .map_err(|_| {
                ErrorDto::unavailable(
                    "openrouter_client_unavailable",
                    "OpenRouter client could not be configured",
                )
            })?;
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

    /// Prepares a private OpenRouter request after capability validation.
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

    /// Maps text output into the canonical stream contract.
    ///
    /// # Errors
    ///
    /// Returns a validation error for an empty text delta.
    pub fn map_fixture_text(content: &str) -> DtoResult<ModelEventDto> {
        ModelEventDto::text_delta(content)
    }

    /// Maps a reasoning delta into the canonical stream contract.
    ///
    /// # Errors
    ///
    /// Returns a validation error for an empty reasoning delta.
    pub fn map_fixture_reasoning(content: &str) -> DtoResult<ModelEventDto> {
        ModelEventDto::reasoning_delta(content)
    }

    /// Maps reported usage into canonical usage.
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

    /// Maps an OpenRouter finish reason string into the canonical reason.
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

    /// Maps a complete tool call into the canonical tool-call DTO.
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

    /// Maps a native status category into a safe provider error without native text.
    ///
    /// # Errors
    ///
    /// Returns a validation error only if the fixed normalized error is malformed.
    pub fn map_fixture_error(status: u16, _native_message: &str) -> DtoResult<ProviderErrorDto> {
        ProviderErrorDto::unavailable(
            if status == 429 || status >= 500 {
                "openrouter_provider_unavailable"
            } else {
                "openrouter_provider_request_rejected"
            },
            status == 429 || status >= 500,
            None,
        )
    }
}

impl ModelDriver for OpenRouterDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    }
}

fn translate_request(request: &ModelRequestDto) -> DtoResult<ChatCompletionRequest> {
    let mut messages = Vec::new();
    if let Some(context) = request.system_context() {
        messages.push(Message::new(Role::System, context));
    }
    for message in request.messages() {
        messages.push(translate_message(message));
    }
    ChatCompletionRequest::builder()
        .model(request.model())
        .messages(messages)
        .build()
        .map_err(|_| {
            ErrorDto::validation(
                "invalid_openrouter_request",
                "OpenRouter request could not be translated",
            )
        })
}

fn translate_message(message: &ModelMessageDto) -> Message {
    let role = match message.role() {
        ModelRoleDto::System => Role::System,
        ModelRoleDto::User => Role::User,
        ModelRoleDto::Assistant => Role::Assistant,
    };
    Message::new(role, message.content())
}
