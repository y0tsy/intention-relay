//! OpenRouter provider normalization backed privately by `openrouter-rs`.
//!
//! This adapter owns OpenRouter SDK construction and request translation. It
//! emits only `intention-model` DTOs and never exposes SDK stream resources.

use std::collections::VecDeque;

use futures_util::{
    Stream, StreamExt,
    future::{Either, select},
    stream,
};
use intention_config::{ProviderKindDto, ResolvedConfigDto, StartupProviderMaterial};
use intention_model::{
    AuthenticationHeaderPolicyV1, CredentialTransportMode, FinishReasonDto,
    ModelCancellationSignal, ModelCapabilitiesDto, ModelDriver, ModelEventDto, ModelEventStream,
    ModelExecutionDriver, ModelMessageDto, ModelRequestDto, ModelRoleDto, ProviderErrorDto,
    ReasoningEffortLevel, ToolCallDto, UsageDto,
};
use intention_types::{DtoResult, ErrorDto, ToolCallId};
use openrouter_rs::{
    OpenRouterClient,
    api::chat::{ChatCompletionRequest, Message},
    error::OpenRouterError,
    types::{Effort, FinishReason as OpenRouterFinishReason, Role, stream::StreamEvent},
};

/// Additive descriptor-driven driver options; the default is unchanged.
#[derive(Clone, Debug, Default)]
pub struct OpenRouterDriverOptions {
    header_policy: Option<AuthenticationHeaderPolicyV1>,
    reasoning_effort: Option<ReasoningEffortLevel>,
}

impl OpenRouterDriverOptions {
    /// Creates default driver options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares the descriptor header policy (names only, never values).
    ///
    /// The policy is validated by construction and stored privately; it is
    /// never logged, serialized, or made durable.
    #[must_use]
    pub fn with_header_policy(mut self, policy: AuthenticationHeaderPolicyV1) -> Self {
        self.header_policy = Some(policy);
        self
    }

    /// Declares the reasoning effort applied to OpenRouter requests.
    #[must_use]
    pub const fn with_reasoning_effort(mut self, effort: ReasoningEffortLevel) -> Self {
        self.reasoning_effort = Some(effort);
        self
    }

    /// Validates that every declared option is applicable to this adapter.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the header policy selects the
    /// safe-header transport, which this SDK adapter cannot inject without
    /// the `http` crate as a production dependency.
    pub fn build(self) -> DtoResult<Self> {
        if self.header_policy.as_ref().is_some_and(|policy| {
            policy.selected_transport() == CredentialTransportMode::SafeHeader
        }) {
            return Err(ErrorDto::validation(
                "unsupported_safe_header_transport",
                "the OpenRouter adapter does not yet support safe-header credential transport",
            ));
        }
        Ok(self)
    }
}

/// OpenRouter driver with private SDK client state.
pub struct OpenRouterDriver {
    resolved: ResolvedConfigDto,
    client: OpenRouterClient,
    options: OpenRouterDriverOptions,
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

    /// Creates a driver from opaque startup-only provider material and
    /// validated descriptor-driven options.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the material does not select OpenRouter.
    pub fn from_startup_material_with_options(
        material: StartupProviderMaterial,
        options: OpenRouterDriverOptions,
    ) -> DtoResult<Self> {
        material.into_parts_for_provider(move |resolved, credential| {
            Self::with_credential_and_options(resolved, credential, options)
        })
    }

    fn with_credential(resolved: ResolvedConfigDto, credential: String) -> DtoResult<Self> {
        Self::with_credential_and_options(resolved, credential, OpenRouterDriverOptions::default())
    }

    fn with_credential_and_options(
        resolved: ResolvedConfigDto,
        credential: String,
        options: OpenRouterDriverOptions,
    ) -> DtoResult<Self> {
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
            options,
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
    /// # Errors
    ///
    /// Returns a safe policy or translation error before outbound work.
    pub fn prepare_request(&mut self, request: &ModelRequestDto) -> DtoResult<()> {
        self.preflight(request)?;
        let _native_request = translate_request(request, &self.options)?;
        let _client = &self.client;
        self.outbound_calls_for_test = self.outbound_calls_for_test.saturating_add(1);
        Ok(())
    }

    /// Returns the number of SDK request preparations completed in this process.
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
        map_finish_reason(reason)
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

impl ModelExecutionDriver for OpenRouterDriver {
    fn execute(
        &self,
        request: ModelRequestDto,
        cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        if cancellation.is_cancelled() {
            return Box::pin(stream::empty());
        }
        if self.preflight(&request).is_err() {
            return Box::pin(stream::once(async {
                Err(safe_error("openrouter_request_rejected"))
            }));
        }
        let native_request = match translate_request(&request, &self.options) {
            Ok(request) => request,
            Err(_) => {
                return Box::pin(stream::once(async {
                    Err(safe_error("openrouter_request_rejected"))
                }));
            }
        };
        let client = self.client.clone();
        Box::pin(
            stream::once(async move {
                client
                    .chat()
                    .stream_tool_aware(&native_request)
                    .await
                    .map_or_else(
                        |error| {
                            Box::pin(stream::once(
                                async move { Err(map_openrouter_error(&error)) },
                            )) as ModelEventStream
                        },
                        |native| normalize_stream(native, cancellation),
                    )
            })
            .flatten(),
        )
    }
}

fn normalize_stream<S>(native: S, cancellation: ModelCancellationSignal) -> ModelEventStream
where
    S: Stream<Item = StreamEvent> + Send + 'static,
{
    Box::pin(stream::unfold(
        OpenRouterStreamState::new(native, cancellation),
        |mut state| async move { state.next().await.map(|event| (event, state)) },
    ))
}

struct OpenRouterStreamState<S> {
    native: std::pin::Pin<Box<S>>,
    cancellation: ModelCancellationSignal,
    pending: VecDeque<Result<ModelEventDto, ProviderErrorDto>>,
    terminal: bool,
}

impl<S> OpenRouterStreamState<S>
where
    S: Stream<Item = StreamEvent>,
{
    fn new(native: S, cancellation: ModelCancellationSignal) -> Self {
        let mut pending = VecDeque::new();
        pending.push_back(Ok(ModelEventDto::started()));
        Self {
            native: Box::pin(native),
            cancellation,
            pending,
            terminal: false,
        }
    }

    async fn next(&mut self) -> Option<Result<ModelEventDto, ProviderErrorDto>> {
        loop {
            if self.cancellation.is_cancelled() {
                return None;
            }
            if let Some(event) = self.pending.pop_front() {
                return Some(event);
            }
            if self.terminal {
                return None;
            }
            match select(self.native.next(), self.cancellation.cancelled()).await {
                Either::Left((Some(event), _)) => self.accept(event),
                Either::Left((None, _)) => self.fail("openrouter_stream_incomplete"),
                Either::Right(((), _)) => return None,
            }
        }
    }

    fn accept(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::ContentDelta(content) => match ModelEventDto::text_delta(content) {
                Ok(event) => self.pending.push_back(Ok(event)),
                Err(_) => self.fail("openrouter_invalid_text"),
            },
            StreamEvent::ReasoningDelta(content) => match ModelEventDto::reasoning_delta(content) {
                Ok(event) => self.pending.push_back(Ok(event)),
                Err(_) => self.fail("provider_reasoning_stream_invalid"),
            },
            StreamEvent::ReasoningDetailsDelta(details) => {
                for detail in details {
                    if !self.accept_reasoning_detail(&detail) {
                        return;
                    }
                }
            }
            StreamEvent::Done {
                tool_calls,
                finish_reason,
                usage,
                ..
            } => {
                let calls = tool_calls
                    .into_iter()
                    .map(|call| {
                        ToolCallDto::new(
                            ToolCallId::new(),
                            call.function.name,
                            call.function.arguments,
                        )
                    })
                    .collect::<DtoResult<Vec<_>>>();
                match calls {
                    Ok(calls) => {
                        self.pending.extend(
                            calls
                                .into_iter()
                                .map(|call| Ok(ModelEventDto::tool_call(call))),
                        );
                        if let Some(usage) = usage {
                            match UsageDto::reported(
                                u64::from(usage.prompt_tokens),
                                u64::from(usage.completion_tokens),
                                u64::from(usage.total_tokens),
                            ) {
                                Ok(usage) => {
                                    self.pending.push_back(Ok(ModelEventDto::usage(usage)))
                                }
                                Err(_) => {
                                    self.fail("openrouter_invalid_usage");
                                    return;
                                }
                            }
                        }
                        self.pending.push_back(Ok(ModelEventDto::finished(
                            finish_reason
                                .map_or(FinishReasonDto::Unknown, map_native_finish_reason),
                        )));
                        self.terminal = true;
                    }
                    Err(_) => self.fail("openrouter_invalid_tool_call"),
                }
            }
            StreamEvent::Error(error) => self.fail_error(map_openrouter_error(&error)),
            _ => self.fail("openrouter_unsupported_stream_event"),
        }
    }

    fn fail_error(&mut self, error: ProviderErrorDto) {
        self.pending.push_back(Err(error));
        self.terminal = true;
    }

    /// Classifies one structured reasoning-detail block.
    ///
    /// Textual blocks (`reasoning.text`) publish their text or data content as
    /// a Detail delta. Recognized opaque blocks (encrypted payloads and
    /// server-tool activity) are suppressed: they cannot be normalized into
    /// publishable reasoning and must never leak their raw payloads. Any
    /// other shape — an empty or unknown block type, or a textual block
    /// without content — is malformed and fails the stream.
    fn accept_reasoning_detail(&mut self, detail: &openrouter_rs::types::ReasoningDetail) -> bool {
        let local_type = detail
            .block_type
            .strip_prefix("reasoning.")
            .unwrap_or(&detail.block_type);
        if local_type == "text" {
            let Some(text) = detail.content().filter(|text| !text.is_empty()) else {
                self.fail("provider_reasoning_stream_invalid");
                return false;
            };
            match ModelEventDto::reasoning_delta_categorized(
                intention_model::ReasoningFragmentCategoryDto::Detail,
                text,
            ) {
                Ok(event) => self.pending.push_back(Ok(event)),
                Err(_) => {
                    self.fail("provider_reasoning_stream_invalid");
                    return false;
                }
            }
            return true;
        }
        if detail.block_type.is_empty() {
            self.fail("provider_reasoning_stream_invalid");
            return false;
        }
        let opaque = local_type.contains("encrypted") || local_type.starts_with("server_tool");
        if !opaque {
            self.fail("provider_reasoning_stream_invalid");
            return false;
        }
        true
    }

    fn fail(&mut self, code: &'static str) {
        self.fail_error(safe_error(code));
    }
}

fn map_finish_reason(reason: &str) -> FinishReasonDto {
    match reason {
        "stop" => FinishReasonDto::Stop,
        "length" => FinishReasonDto::Length,
        "tool_calls" => FinishReasonDto::ToolCalls,
        "content_filter" => FinishReasonDto::ContentFilter,
        "error" => FinishReasonDto::Error,
        _ => FinishReasonDto::Unknown,
    }
}

fn map_native_finish_reason(reason: OpenRouterFinishReason) -> FinishReasonDto {
    match reason {
        OpenRouterFinishReason::Stop => FinishReasonDto::Stop,
        OpenRouterFinishReason::Length => FinishReasonDto::Length,
        OpenRouterFinishReason::ToolCalls => FinishReasonDto::ToolCalls,
        OpenRouterFinishReason::ContentFilter => FinishReasonDto::ContentFilter,
        OpenRouterFinishReason::Error => FinishReasonDto::Error,
        OpenRouterFinishReason::Other(_) => FinishReasonDto::Unknown,
        _ => FinishReasonDto::Unknown,
    }
}

fn map_openrouter_error(error: &OpenRouterError) -> ProviderErrorDto {
    let retryable = match error {
        OpenRouterError::Api(context) => context.is_retryable(),
        OpenRouterError::HttpRequest(_) | OpenRouterError::Io(_) | OpenRouterError::Unknown(_) => {
            true
        }
        OpenRouterError::ConfigError(_)
        | OpenRouterError::KeyNotConfigured
        | OpenRouterError::UninitializedFieldError(_)
        | OpenRouterError::Serialization(_) => false,
    };
    ProviderErrorDto::unavailable(
        if retryable {
            "openrouter_provider_unavailable"
        } else {
            "openrouter_provider_request_rejected"
        },
        retryable,
        None,
    )
    .unwrap_or_else(|_| safe_error("openrouter_provider_failure"))
}

#[allow(
    clippy::expect_used,
    reason = "The fixed non-blank normalized error code is validated by ProviderErrorDto."
)]
fn safe_error(code: &'static str) -> ProviderErrorDto {
    ProviderErrorDto::unavailable(code, false, None).unwrap_or_else(|_| {
        ProviderErrorDto::unavailable("openrouter_provider_failure", false, None)
            .expect("fixed normalized provider error is valid")
    })
}

fn translate_request(
    request: &ModelRequestDto,
    options: &OpenRouterDriverOptions,
) -> DtoResult<ChatCompletionRequest> {
    let mut messages = Vec::new();
    if let Some(context) = request.system_context() {
        messages.push(Message::new(Role::System, context));
    }
    for message in request.messages() {
        messages.push(translate_message(message)?);
    }
    let mut builder = ChatCompletionRequest::builder();
    builder.model(request.model());
    builder.messages(messages);
    if let Some(effort) = options.reasoning_effort {
        builder.reasoning_effort(map_effort(effort));
    }
    builder.build().map_err(|_| {
        ErrorDto::validation(
            "invalid_openrouter_request",
            "OpenRouter request could not be translated",
        )
    })
}

const fn map_effort(effort: ReasoningEffortLevel) -> Effort {
    match effort {
        ReasoningEffortLevel::None => Effort::None,
        ReasoningEffortLevel::Minimal => Effort::Minimal,
        ReasoningEffortLevel::Low => Effort::Low,
        ReasoningEffortLevel::Medium => Effort::Medium,
        ReasoningEffortLevel::High => Effort::High,
        ReasoningEffortLevel::Xhigh => Effort::Xhigh,
        ReasoningEffortLevel::Max => Effort::Max,
    }
}

fn translate_message(message: &ModelMessageDto) -> DtoResult<Message> {
    match message.role() {
        ModelRoleDto::System => Ok(Message::new(Role::System, message.content())),
        ModelRoleDto::User => Ok(Message::new(Role::User, message.content())),
        ModelRoleDto::Assistant => translate_assistant_message(message),
        ModelRoleDto::Tool => {
            let tool_call_id = message.tool_call_id().ok_or_else(|| {
                ErrorDto::validation(
                    "invalid_openrouter_request",
                    "tool-role messages must carry one tool call identity",
                )
            })?;
            Ok(Message::tool_response(
                &tool_call_id.to_string(),
                message.content(),
            ))
        }
    }
}

/// Translates an assistant message, mapping locally executed tool calls back
/// onto the OpenRouter assistant shape so the tool-result round can continue.
///
/// # Errors
///
/// Returns a validation error when a tool-call message cannot be mapped.
fn translate_assistant_message(message: &ModelMessageDto) -> DtoResult<Message> {
    let content = message.content();
    let Some(tool_calls) = message.tool_calls() else {
        return Ok(Message::new(Role::Assistant, content));
    };
    if tool_calls.is_empty() {
        return Ok(Message::new(Role::Assistant, content));
    }
    let native_calls = tool_calls
        .iter()
        .map(|call| {
            openrouter_rs::types::ToolCall::new(
                call.call_id().to_string(),
                call.name(),
                call.arguments_json(),
            )
        })
        .collect::<Vec<_>>();
    Ok(Message::assistant_with_tool_calls(
        content.to_owned(),
        native_calls,
    ))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "Private SDK error fixtures construct exact native variants."
)]
mod tests {
    use super::*;
    use futures_util::FutureExt;
    use intention_types::RunId;

    fn api_error(status: http::StatusCode) -> OpenRouterError {
        OpenRouterError::Api(Box::new(openrouter_rs::error::ApiErrorContext {
            status,
            api_code: None,
            message: "secret provider text".to_owned(),
            request_id: Some("secret request id".to_owned()),
            metadata: None,
            kind: openrouter_rs::error::ApiErrorKind::Generic,
        }))
    }

    fn reasoning_detail(
        block_type: &str,
        text: Option<&str>,
    ) -> openrouter_rs::types::ReasoningDetail {
        serde_json::from_value(serde_json::json!({
            "type": block_type,
            "text": text,
        }))
        .expect("private SDK reasoning detail fixture decodes")
    }

    fn opaque_reasoning_detail(
        block_type: &str,
        data: &str,
    ) -> openrouter_rs::types::ReasoningDetail {
        serde_json::from_value(serde_json::json!({
            "type": block_type,
            "data": data,
        }))
        .expect("private SDK opaque reasoning detail fixture decodes")
    }

    fn startup_material(credential: &str) -> StartupProviderMaterial {
        let source = intention_config::ConfigSourceDto::Explicit(
            intention_config::ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("openrouter-test-config.toml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("fixture path is absolute"),
        );
        let toml = format!(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture-model\"\ncredential = \"{credential}\"\n"
        );
        intention_config::ResolvedConfigDto::parse_startup_material(
            intention_config::RawConfigInputDto::new(toml, source),
        )
        .expect("fixture startup material")
    }

    fn request() -> ModelRequestDto {
        ModelRequestDto::new(
            RunId::new(),
            "fixture-model",
            vec![ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid")],
            None,
            None,
        )
        .expect("request is valid")
    }

    #[test]
    fn native_errors_preserve_safe_retry_guidance() {
        let retryable = map_openrouter_error(&api_error(http::StatusCode::TOO_MANY_REQUESTS));
        let permanent = map_openrouter_error(&api_error(http::StatusCode::BAD_REQUEST));
        assert_eq!(retryable.retry(), intention_types::ErrorRetryDto::Delayed);
        assert_eq!(permanent.retry(), intention_types::ErrorRetryDto::Never);
        assert!(
            !serde_json::to_string(&retryable)
                .expect("error serializes")
                .contains("secret")
        );
    }

    fn done(
        tool_calls: Vec<openrouter_rs::types::ToolCall>,
        finish_reason: Option<OpenRouterFinishReason>,
        usage: Option<openrouter_rs::types::ResponseUsage>,
    ) -> StreamEvent {
        StreamEvent::Done {
            tool_calls,
            finish_reason,
            usage,
            id: "fixture".to_owned(),
            model: "fixture".to_owned(),
        }
    }

    fn usage(
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
    ) -> openrouter_rs::types::ResponseUsage {
        serde_json::from_value(serde_json::json!({
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": total_tokens,
        }))
        .expect("private SDK usage fixture decodes")
    }

    fn state() -> OpenRouterStreamState<impl Stream<Item = StreamEvent>> {
        OpenRouterStreamState::new(stream::empty(), ModelCancellationSignal::new())
    }

    #[test]
    fn native_stream_normalizes_content_reasoning_tools_usage_and_finish() {
        let mut state = state();
        state.accept(StreamEvent::ContentDelta("answer".to_owned()));
        state.accept(StreamEvent::ReasoningDelta("because".to_owned()));
        state.accept(done(
            vec![openrouter_rs::types::ToolCall::new("call", "inspect", "{}")],
            Some(OpenRouterFinishReason::ToolCalls),
            Some(usage(2, 3, 5)),
        ));

        assert_eq!(
            state.pending.pop_front(),
            Some(Ok(ModelEventDto::started()))
        );
        assert_eq!(
            state.pending.pop_front(),
            Some(Ok(ModelEventDto::text_delta("answer").expect("valid text")))
        );
        assert_eq!(
            state.pending.pop_front(),
            Some(Ok(
                ModelEventDto::reasoning_delta("because").expect("valid reasoning")
            ))
        );
        assert!(matches!(
            state.pending.pop_front(),
            Some(Ok(ModelEventDto::ToolCall { call })) if call.name() == "inspect"
        ));
        assert_eq!(
            state.pending.pop_front(),
            Some(Ok(ModelEventDto::usage(
                UsageDto::reported(2, 3, 5).expect("valid usage")
            )))
        );
        assert_eq!(
            state.pending.pop_front(),
            Some(Ok(ModelEventDto::finished(FinishReasonDto::ToolCalls)))
        );
        assert!(state.terminal);
    }

    #[test]
    fn native_stream_rejects_invalid_incomplete_and_unsupported_events_safely() {
        let mut invalid_text = state();
        invalid_text.accept(StreamEvent::ContentDelta(String::new()));
        assert!(matches!(
            invalid_text.pending.back(),
            Some(Err(error)) if error.code() == "openrouter_invalid_text"
        ));

        let mut invalid_reasoning = state();
        invalid_reasoning.accept(StreamEvent::ReasoningDelta(String::new()));
        assert!(matches!(
            invalid_reasoning.pending.back(),
            Some(Err(error)) if error.code() == "provider_reasoning_stream_invalid"
        ));

        let mut invalid_usage = state();
        invalid_usage.accept(done(Vec::new(), None, Some(usage(1, 1, 1))));
        assert!(matches!(
            invalid_usage.pending.back(),
            Some(Err(error)) if error.code() == "openrouter_invalid_usage"
        ));

        let mut invalid_tool = state();
        invalid_tool.accept(done(
            vec![openrouter_rs::types::ToolCall::new("call", "", "{}")],
            None,
            None,
        ));
        assert!(matches!(
            invalid_tool.pending.back(),
            Some(Err(error)) if error.code() == "openrouter_invalid_tool_call"
        ));

        // An empty reasoning-details event carries no reasoning and is a no-op.
        let mut empty_details = state();
        empty_details.accept(StreamEvent::ReasoningDetailsDelta(Vec::new()));
        assert_eq!(empty_details.pending.len(), 1);
        assert!(!empty_details.terminal);

        // A reasoning.text detail without decodable content is malformed
        // reasoning.
        let mut encrypted_detail = state();
        encrypted_detail.accept(StreamEvent::ReasoningDetailsDelta(vec![reasoning_detail(
            "reasoning.text",
            None,
        )]));
        assert!(matches!(
            encrypted_detail.pending.back(),
            Some(Err(error)) if error.code() == "provider_reasoning_stream_invalid"
        ));

        let mut unsupported = state();
        unsupported.fail("openrouter_stream_incomplete");
        assert!(matches!(
            unsupported.pending.back(),
            Some(Err(error)) if error.code() == "openrouter_stream_incomplete"
        ));
    }

    #[test]
    fn reasoning_details_normalize_to_detail_deltas_in_array_order() {
        let mut state = state();
        state.accept(StreamEvent::ReasoningDetailsDelta(vec![
            reasoning_detail("reasoning.text", Some("detail one")),
            reasoning_detail("reasoning.text", Some("detail two")),
        ]));
        assert_eq!(
            state.pending.pop_front(),
            Some(Ok(ModelEventDto::started()))
        );
        assert_eq!(
            state.pending.pop_front(),
            Some(Ok(ModelEventDto::reasoning_delta_categorized(
                intention_model::ReasoningFragmentCategoryDto::Detail,
                "detail one",
            )
            .expect("valid detail delta")))
        );
        assert_eq!(
            state.pending.pop_front(),
            Some(Ok(ModelEventDto::reasoning_delta_categorized(
                intention_model::ReasoningFragmentCategoryDto::Detail,
                "detail two",
            )
            .expect("valid detail delta")))
        );
        assert!(state.pending.is_empty());
    }

    #[test]
    fn descriptor_options_reject_before_any_request_and_never_disclose_policy() {
        let bearer = AuthenticationHeaderPolicyV1::new(
            Vec::new(),
            intention_model::CredentialTransportMode::Bearer,
        )
        .expect("bearer policy is valid");
        let options = OpenRouterDriverOptions::new()
            .with_header_policy(bearer)
            .with_reasoning_effort(ReasoningEffortLevel::High)
            .build()
            .expect("descriptor options build");
        assert!(!format!("{options:?}").contains("X-Custom-Auth"));

        let safe_header = AuthenticationHeaderPolicyV1::new(
            vec!["X-Custom-Auth".to_owned()],
            intention_model::CredentialTransportMode::SafeHeader,
        )
        .expect("safe-header policy is valid");
        assert_eq!(
            OpenRouterDriverOptions::new()
                .with_header_policy(safe_header)
                .build()
                .expect_err("safe-header transport is rejected before any request")
                .code(),
            "unsupported_safe_header_transport"
        );

        let driver = OpenRouterDriver::from_startup_material_with_options(
            startup_material("sk-fake-secret-12345"),
            options,
        )
        .expect("driver builds with validated options");
        let debug = format!("{driver:?}");
        assert!(!debug.contains("sk-fake-secret-12345"));
        assert!(!debug.contains("X-Custom-Auth"));
        assert!(!debug.contains("reasoning"));
        assert_eq!(driver.prepared_request_count(), 0);
    }

    #[test]
    fn declared_reasoning_effort_is_applied_to_the_native_request() {
        let options = OpenRouterDriverOptions::new()
            .with_reasoning_effort(ReasoningEffortLevel::High)
            .build()
            .expect("descriptor options build");
        let wire = serde_json::to_value(
            translate_request(&request(), &options).expect("request translates"),
        )
        .expect("request serializes");
        assert_eq!(wire["reasoning"]["effort"], "high");

        let default_wire = serde_json::to_value(
            translate_request(&request(), &OpenRouterDriverOptions::default())
                .expect("request translates"),
        )
        .expect("request serializes");
        assert!(default_wire.get("reasoning").is_none());
    }

    #[test]
    fn normalized_stream_emits_started_and_incomplete_error() {
        let mut stream = normalize_stream(stream::empty(), ModelCancellationSignal::new());
        assert_eq!(
            stream.next().now_or_never(),
            Some(Some(Ok(ModelEventDto::started())))
        );
        assert!(matches!(
            stream.next().now_or_never(),
            Some(Some(Err(error))) if error.code() == "openrouter_stream_incomplete"
        ));
        assert_eq!(stream.next().now_or_never(), Some(None));
    }

    #[test]
    fn native_finish_reasons_preserve_all_known_values() {
        for (native, expected) in [
            (OpenRouterFinishReason::Stop, FinishReasonDto::Stop),
            (OpenRouterFinishReason::Length, FinishReasonDto::Length),
            (
                OpenRouterFinishReason::ToolCalls,
                FinishReasonDto::ToolCalls,
            ),
            (
                OpenRouterFinishReason::ContentFilter,
                FinishReasonDto::ContentFilter,
            ),
            (OpenRouterFinishReason::Error, FinishReasonDto::Error),
            (
                OpenRouterFinishReason::Other("fixture".to_owned()),
                FinishReasonDto::Unknown,
            ),
        ] {
            assert_eq!(map_native_finish_reason(native), expected);
        }
    }

    #[test]
    fn native_stream_error_maps_retryability_and_cancellation_stops_delivery() {
        let mut retryable = state();
        retryable.accept(StreamEvent::Error(api_error(
            http::StatusCode::SERVICE_UNAVAILABLE,
        )));
        assert!(matches!(
            retryable.pending.back(),
            Some(Err(error)) if error.code() == "openrouter_provider_unavailable"
                && error.retry() == intention_types::ErrorRetryDto::Delayed
        ));

        let mut permanent = state();
        permanent.accept(StreamEvent::Error(api_error(http::StatusCode::BAD_REQUEST)));
        assert!(matches!(
            permanent.pending.back(),
            Some(Err(error)) if error.code() == "openrouter_provider_request_rejected"
                && error.retry() == intention_types::ErrorRetryDto::Never
        ));

        let cancellation = ModelCancellationSignal::new();
        cancellation.cancel();
        let mut cancelled = OpenRouterStreamState::new(stream::empty(), cancellation);
        assert!(cancelled.cancellation.is_cancelled());
        assert_eq!(cancelled.next().now_or_never(), Some(None));
    }

    #[test]
    fn opaque_reasoning_blocks_are_suppressed_while_malformed_shapes_fail() {
        // Recognized opaque blocks (encrypted payloads, server-tool activity)
        // are suppressed: the stream survives and their raw payloads are
        // never published (PR24-021).
        let mut mixed = state();
        mixed.accept(StreamEvent::ReasoningDetailsDelta(vec![
            opaque_reasoning_detail("reasoning.encrypted", "cipher-secret"),
            opaque_reasoning_detail("encrypted", "cipher-secret"),
            opaque_reasoning_detail("reasoning.server_tool_call", r#"{"tool":"search"}"#),
        ]));
        assert_eq!(
            mixed.pending.pop_front(),
            Some(Ok(ModelEventDto::started()))
        );
        assert!(
            mixed.pending.is_empty() && !mixed.terminal,
            "suppressed opaque blocks emit nothing and do not fail the stream"
        );
        mixed.accept(StreamEvent::ReasoningDetailsDelta(vec![reasoning_detail(
            "reasoning.text",
            Some("visible"),
        )]));
        mixed.accept(StreamEvent::ContentDelta("answer".to_owned()));
        let visible = ModelEventDto::reasoning_delta_categorized(
            intention_model::ReasoningFragmentCategoryDto::Detail,
            "visible",
        )
        .expect("valid detail delta");
        assert_eq!(mixed.pending.pop_front(), Some(Ok(visible)));
        assert_eq!(
            mixed.pending.pop_front(),
            Some(Ok(ModelEventDto::text_delta("answer").expect("valid text")))
        );
        let leaked = format!("{:?}", mixed.pending);
        assert!(!leaked.contains("cipher-secret"));

        // Unknown block families are malformed, not silently suppressible.
        let mut unknown = state();
        unknown.accept(StreamEvent::ReasoningDetailsDelta(vec![reasoning_detail(
            "reasoning.custom_opaque_kind",
            Some("visible but unknown"),
        )]));
        assert!(matches!(
            unknown.pending.back(),
            Some(Err(error)) if error.code() == "provider_reasoning_stream_invalid"
        ));

        // A block without any type is malformed.
        let mut typeless = state();
        typeless.accept(StreamEvent::ReasoningDetailsDelta(vec![
            serde_json::from_value(serde_json::json!({"type": ""}))
                .expect("typeless SDK fixture decodes"),
        ]));
        assert!(matches!(
            typeless.pending.back(),
            Some(Err(error)) if error.code() == "provider_reasoning_stream_invalid"
        ));
    }

    #[test]
    fn tool_round_two_translates_assistant_calls_and_tool_results() {
        let call = ToolCallDto::new(ToolCallId::new(), "read", r#"{"path":"hello.txt"}"#)
            .expect("fixture call is valid");
        let request = ModelRequestDto::new(
            RunId::new(),
            "fixture-model",
            vec![
                ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid"),
                ModelMessageDto::assistant_tool_calls(None, vec![call.clone()])
                    .expect("message is valid"),
                ModelMessageDto::tool_result(call.call_id(), "hello world")
                    .expect("message is valid"),
            ],
            None,
            None,
        )
        .expect("request is valid");

        let wire = serde_json::to_value(
            translate_request(&request, &OpenRouterDriverOptions::default())
                .expect("request translates"),
        )
        .expect("request serializes");
        assert_eq!(
            wire["messages"],
            serde_json::json!([
                {"role": "user", "content": "hello"},
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": call.call_id().to_string(),
                        "type": "function",
                        "function": {
                            "name": "read",
                            "arguments": r#"{"path":"hello.txt"}"#,
                        },
                        "index": null,
                    }],
                },
                {
                    "role": "tool",
                    "content": "hello world",
                    "tool_call_id": call.call_id().to_string(),
                },
            ])
        );
    }
}
