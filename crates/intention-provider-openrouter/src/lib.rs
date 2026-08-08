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
    FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto, ModelDriver, ModelEventDto,
    ModelEventStream, ModelExecutionDriver, ModelMessageDto, ModelRequestDto, ModelRoleDto,
    ProviderErrorDto, ToolCallDto, UsageDto,
};
use intention_types::{DtoResult, ErrorDto, ToolCallId};
use openrouter_rs::{
    OpenRouterClient,
    api::chat::{ChatCompletionRequest, Message},
    error::OpenRouterError,
    types::{FinishReason as OpenRouterFinishReason, Role, stream::StreamEvent},
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
        let native_request = match translate_request(&request) {
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
                Err(_) => self.fail("openrouter_invalid_reasoning"),
            },
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
            StreamEvent::ReasoningDetailsDelta(_) => {}
            _ => self.fail("openrouter_unsupported_stream_event"),
        }
    }

    fn fail_error(&mut self, error: ProviderErrorDto) {
        self.pending.push_back(Err(error));
        self.terminal = true;
    }

    fn fail(&mut self, code: &'static str) {
        self.fail_error(safe_error(code));
    }
}

fn map_finish_reason(reason: &str) -> FinishReasonDto {
    match reason {
        "stop" => FinishReasonDto::Stop,
        "length" => FinishReasonDto::Length,
        "tool_calls" | "function_call" => FinishReasonDto::ToolCalls,
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

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "Private SDK error fixtures construct exact native variants."
)]
mod tests {
    use super::*;
    use futures_util::FutureExt;

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
            Some(Err(error)) if error.code() == "openrouter_invalid_reasoning"
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

        let mut unsupported = state();
        unsupported.accept(StreamEvent::ReasoningDetailsDelta(Vec::new()));
        assert_eq!(unsupported.pending.len(), 1);
        unsupported.fail("openrouter_stream_incomplete");
        assert!(matches!(
            unsupported.pending.back(),
            Some(Err(error)) if error.code() == "openrouter_stream_incomplete"
        ));
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
}
