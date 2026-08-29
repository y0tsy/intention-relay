//! Generic OpenAI Chat Completions provider normalization.
//!
//! `async-openai` remains a private implementation dependency. This adapter
//! retains SDK-owned SSE parsing and translates only its parsed stream values
//! into provider-neutral model events.

use std::collections::{BTreeMap, VecDeque};

use async_openai::{
    Client,
    config::OpenAIConfig,
    error::OpenAIError,
    types::chat::{
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestAssistantMessageContent,
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
        ChatCompletionRequestSystemMessageContent, ChatCompletionRequestUserMessage,
        ChatCompletionRequestUserMessageContent, ChatCompletionStreamOptions,
        ChatCompletionStreamResponseDelta, CreateChatCompletionRequest,
        CreateChatCompletionRequestArgs, CreateChatCompletionStreamResponse, FinishReason,
    },
};
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
        map_finish_reason(reason)
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
        provider_error(status == 429 || status >= 500)
    }
}

impl ModelDriver for GenericChatDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, false, true, false, false, true)
    }
}

impl ModelExecutionDriver for GenericChatDriver {
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
                Err(non_retryable_error("generic_chat_request_rejected"))
            }));
        }
        let native_request = match translate_request(&request) {
            Ok(request) => request,
            Err(_) => {
                return Box::pin(stream::once(async {
                    Err(non_retryable_error("generic_chat_request_rejected"))
                }));
            }
        };
        let client = self.client.clone();
        Box::pin(
            stream::once(async move {
                client
                    .chat()
                    .create_stream(native_request)
                    .await
                    .map_or_else(
                        |error| {
                            Box::pin(stream::once(async move { Err(map_openai_error(&error)) }))
                                as ModelEventStream
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
    S: Stream<Item = Result<CreateChatCompletionStreamResponse, OpenAIError>> + Send + 'static,
{
    Box::pin(stream::unfold(
        GenericStreamState::new(native, cancellation),
        |mut state| async move { state.next().await.map(|event| (event, state)) },
    ))
}

struct GenericStreamState<S> {
    native: std::pin::Pin<Box<S>>,
    cancellation: ModelCancellationSignal,
    pending: VecDeque<Result<ModelEventDto, ProviderErrorDto>>,
    tools: BTreeMap<(u32, u32), FunctionToolFragments>,
    legacy_tools: BTreeMap<u32, FunctionToolFragments>,
    terminal: bool,
    terminal_reason: Option<FinishReasonDto>,
}

impl<S> GenericStreamState<S>
where
    S: Stream<Item = Result<CreateChatCompletionStreamResponse, OpenAIError>>,
{
    fn new(native: S, cancellation: ModelCancellationSignal) -> Self {
        let mut pending = VecDeque::new();
        pending.push_back(Ok(ModelEventDto::started()));
        Self {
            native: Box::pin(native),
            cancellation,
            pending,
            tools: BTreeMap::new(),
            legacy_tools: BTreeMap::new(),
            terminal: false,
            terminal_reason: None,
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
            if let Some(reason) = self.terminal_reason.take() {
                self.finish(reason);
                continue;
            }
            match select(self.native.next(), self.cancellation.cancelled()).await {
                Either::Left((Some(Ok(chunk)), _)) => self.accept_chunk(chunk),
                Either::Left((Some(Err(error)), _)) => self.fail_error(map_openai_error(&error)),
                Either::Left((None, _)) => self.fail("generic_chat_stream_incomplete"),
                Either::Right(((), _)) => return None,
            }
        }
    }

    fn accept_chunk(&mut self, chunk: CreateChatCompletionStreamResponse) {
        if let Some(usage) = chunk.usage {
            match UsageDto::reported(
                u64::from(usage.prompt_tokens),
                u64::from(usage.completion_tokens),
                u64::from(usage.total_tokens),
            ) {
                Ok(usage) => self.pending.push_back(Ok(ModelEventDto::usage(usage))),
                Err(_) => self.fail("generic_chat_invalid_usage"),
            }
        }
        for choice in chunk.choices {
            if self.accept_delta(choice.index, choice.delta).is_err() {
                self.fail("generic_chat_invalid_tool_call");
                return;
            }
            if let Some(reason) = choice.finish_reason
                && self
                    .terminal_reason
                    .replace(map_native_finish(reason))
                    .is_some()
            {
                self.fail("generic_chat_duplicate_finish");
                return;
            }
        }
    }

    fn accept_delta(
        &mut self,
        choice_index: u32,
        delta: ChatCompletionStreamResponseDelta,
    ) -> Result<(), ()> {
        if let Some(content) = delta.content.filter(|content| !content.is_empty()) {
            self.pending
                .push_back(Ok(ModelEventDto::text_delta(content).map_err(|_| ())?));
        }
        #[allow(
            deprecated,
            reason = "Chat Completions exposes legacy function-call stream fragments."
        )]
        if let Some(function) = delta.function_call {
            let fragments = self.legacy_tools.entry(choice_index).or_default();
            fragments.merge_legacy(function)?;
        }
        if let Some(calls) = delta.tool_calls {
            for call in calls {
                let fragments = self.tools.entry((choice_index, call.index)).or_default();
                fragments.merge(
                    call.id,
                    call.r#type.map(|_| "function".to_owned()),
                    call.function,
                )?;
            }
        }
        Ok(())
    }

    fn finish(&mut self, reason: FinishReasonDto) {
        let modern = std::mem::take(&mut self.tools)
            .into_values()
            .map(FunctionToolFragments::finish)
            .collect::<Result<Vec<_>, _>>();
        let legacy = std::mem::take(&mut self.legacy_tools)
            .into_values()
            .map(FunctionToolFragments::finish_legacy)
            .collect::<Result<Vec<_>, _>>();
        match (modern, legacy) {
            (Ok(modern), Ok(legacy)) if modern.is_empty() || legacy.is_empty() => {
                self.pending.extend(
                    modern
                        .into_iter()
                        .chain(legacy)
                        .map(|call| Ok(ModelEventDto::tool_call(call))),
                );
                self.pending.push_back(Ok(ModelEventDto::finished(reason)));
                self.terminal = true;
            }
            (Ok(_), Ok(_)) => self.fail("generic_chat_conflicting_tool_call"),
            (Err(()), _) | (_, Err(())) => self.fail("generic_chat_invalid_tool_call"),
        }
    }

    fn fail_error(&mut self, error: ProviderErrorDto) {
        self.pending.push_back(Err(error));
        self.terminal = true;
    }

    fn fail(&mut self, code: &'static str) {
        self.fail_error(non_retryable_error(code));
    }
}

#[derive(Default)]
struct FunctionToolFragments {
    id: Option<String>,
    kind: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl FunctionToolFragments {
    fn merge(
        &mut self,
        id: Option<String>,
        kind: Option<String>,
        function: Option<async_openai::types::chat::FunctionCallStream>,
    ) -> Result<(), ()> {
        merge_constant(&mut self.id, id)?;
        merge_constant(&mut self.kind, kind)?;
        if self.kind.as_deref().is_some_and(|kind| kind != "function") {
            return Err(());
        }
        if let Some(function) = function {
            merge_constant(&mut self.name, function.name)?;
            if let Some(arguments) = function.arguments {
                self.arguments.push_str(&arguments);
            }
        }
        Ok(())
    }

    fn finish_legacy(self) -> Result<ToolCallDto, ()> {
        let name = self.name.ok_or(())?;
        ToolCallDto::new(ToolCallId::new(), name, self.arguments).map_err(|_| ())
    }

    fn merge_legacy(
        &mut self,
        function: async_openai::types::chat::FunctionCallStream,
    ) -> Result<(), ()> {
        merge_constant(&mut self.name, function.name)?;
        if let Some(arguments) = function.arguments {
            self.arguments.push_str(&arguments);
        }
        Ok(())
    }

    fn finish(self) -> Result<ToolCallDto, ()> {
        let _id = self.id.ok_or(())?;
        let name = self.name.ok_or(())?;
        ToolCallDto::new(ToolCallId::new(), name, self.arguments).map_err(|_| ())
    }
}

fn merge_constant(slot: &mut Option<String>, next: Option<String>) -> Result<(), ()> {
    if let Some(next) = next {
        if slot.as_ref().is_some_and(|current| current != &next) {
            return Err(());
        }
        *slot = Some(next);
    }
    Ok(())
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

const fn map_native_finish(reason: FinishReason) -> FinishReasonDto {
    match reason {
        FinishReason::Stop => FinishReasonDto::Stop,
        FinishReason::Length => FinishReasonDto::Length,
        FinishReason::ToolCalls | FinishReason::FunctionCall => FinishReasonDto::ToolCalls,
        FinishReason::ContentFilter => FinishReasonDto::ContentFilter,
    }
}

fn provider_error(retryable: bool) -> DtoResult<ProviderErrorDto> {
    ProviderErrorDto::unavailable(
        if retryable {
            "generic_chat_provider_unavailable"
        } else {
            "generic_chat_provider_request_rejected"
        },
        retryable,
        None,
    )
}

fn map_openai_error(error: &OpenAIError) -> ProviderErrorDto {
    let retryable = match error {
        OpenAIError::Reqwest(_) | OpenAIError::StreamError(_) => true,
        OpenAIError::ApiError(error) => matches!(
            error.api_error.r#type.as_deref(),
            None | Some("rate_limit_exceeded" | "server_error")
        ),
        OpenAIError::JSONDeserialize(..)
        | OpenAIError::FileSaveError(_)
        | OpenAIError::FileReadError(_)
        | OpenAIError::InvalidArgument(_) => false,
    };
    provider_error(retryable)
        .unwrap_or_else(|_| non_retryable_error("generic_chat_provider_failure"))
}

#[allow(
    clippy::expect_used,
    reason = "The fixed non-blank normalized error code is validated by ProviderErrorDto."
)]
fn non_retryable_error(code: &'static str) -> ProviderErrorDto {
    ProviderErrorDto::unavailable(code, false, None)
        .expect("fixed normalized provider error code is valid")
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
        .stream_options(ChatCompletionStreamOptions {
            include_usage: Some(true),
            include_obfuscation: None,
        })
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

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "Private tool-fragment fixtures use expect to provide precise test failure messages."
)]
mod tests {
    use super::*;

    #[test]
    fn tool_fragments_accept_split_and_interleaved_calls_only_at_terminal() {
        let mut first = FunctionToolFragments::default();
        let mut second = FunctionToolFragments::default();
        first
            .merge(
                Some("first".to_owned()),
                Some("function".to_owned()),
                Some(async_openai::types::chat::FunctionCallStream {
                    name: Some("inspect".to_owned()),
                    arguments: Some("{\"path\"".to_owned()),
                }),
            )
            .expect("first fragment is valid");
        second
            .merge(
                Some("second".to_owned()),
                Some("function".to_owned()),
                Some(async_openai::types::chat::FunctionCallStream {
                    name: Some("search".to_owned()),
                    arguments: Some("{\"query\"".to_owned()),
                }),
            )
            .expect("second fragment is valid");
        first
            .merge(
                None,
                None,
                Some(async_openai::types::chat::FunctionCallStream {
                    name: None,
                    arguments: Some(":\"src\"}".to_owned()),
                }),
            )
            .expect("first continuation is valid");
        second
            .merge(
                None,
                None,
                Some(async_openai::types::chat::FunctionCallStream {
                    name: None,
                    arguments: Some(":\"model\"}".to_owned()),
                }),
            )
            .expect("second continuation is valid");
        assert_eq!(
            first
                .finish()
                .expect("first call completes")
                .arguments_json(),
            "{\"path\":\"src\"}"
        );
        assert_eq!(
            second
                .finish()
                .expect("second call completes")
                .arguments_json(),
            "{\"query\":\"model\"}"
        );
    }

    #[test]
    fn tool_fragments_reject_conflicting_or_incomplete_values() {
        let mut conflicting = FunctionToolFragments::default();
        conflicting
            .merge(
                Some("call".to_owned()),
                Some("function".to_owned()),
                Some(async_openai::types::chat::FunctionCallStream {
                    name: Some("inspect".to_owned()),
                    arguments: Some("{}".to_owned()),
                }),
            )
            .expect("initial fragment is valid");
        assert!(
            conflicting
                .merge(Some("other".to_owned()), None, None)
                .is_err()
        );
        assert!(FunctionToolFragments::default().finish().is_err());
        let mut malformed = FunctionToolFragments::default();
        malformed
            .merge(
                Some("call".to_owned()),
                Some("function".to_owned()),
                Some(async_openai::types::chat::FunctionCallStream {
                    name: Some("inspect".to_owned()),
                    arguments: Some("not-json".to_owned()),
                }),
            )
            .expect("fragment shape is valid");
        assert!(malformed.finish().is_err());
    }

    #[test]
    fn legacy_function_fragments_complete_only_at_terminal() {
        let mut fragments = FunctionToolFragments::default();
        fragments
            .merge_legacy(async_openai::types::chat::FunctionCallStream {
                name: Some("inspect".to_owned()),
                arguments: Some("{\"path\"".to_owned()),
            })
            .expect("legacy initial fragment is valid");
        fragments
            .merge_legacy(async_openai::types::chat::FunctionCallStream {
                name: None,
                arguments: Some(":\"src\"}".to_owned()),
            })
            .expect("legacy continuation is valid");
        let call = fragments.finish_legacy().expect("legacy call completes");
        assert_eq!(call.name(), "inspect");
        assert_eq!(call.arguments_json(), "{\"path\":\"src\"}");
        assert!(FunctionToolFragments::default().finish_legacy().is_err());
        let mut malformed = FunctionToolFragments::default();
        malformed
            .merge_legacy(async_openai::types::chat::FunctionCallStream {
                name: Some("inspect".to_owned()),
                arguments: Some("not-json".to_owned()),
            })
            .expect("legacy fragment shape is valid");
        assert!(malformed.finish_legacy().is_err());
        let mut conflicting = FunctionToolFragments::default();
        conflicting
            .merge_legacy(async_openai::types::chat::FunctionCallStream {
                name: Some("inspect".to_owned()),
                arguments: Some("{}".to_owned()),
            })
            .expect("legacy initial fragment is valid");
        assert!(
            conflicting
                .merge_legacy(async_openai::types::chat::FunctionCallStream {
                    name: Some("other".to_owned()),
                    arguments: None,
                })
                .is_err()
        );
    }

    #[test]
    fn native_errors_preserve_safe_retry_guidance() {
        let rate_limited = OpenAIError::ApiError(async_openai::error::ApiErrorResponse {
            status_code: http::StatusCode::TOO_MANY_REQUESTS,
            api_error: async_openai::error::ApiError {
                message: "secret provider text".to_owned(),
                r#type: Some("rate_limit_exceeded".to_owned()),
                param: None,
                code: None,
            },
        });
        let permanent = OpenAIError::ApiError(async_openai::error::ApiErrorResponse {
            status_code: http::StatusCode::BAD_REQUEST,
            api_error: async_openai::error::ApiError {
                message: "secret provider text".to_owned(),
                r#type: Some("invalid_request_error".to_owned()),
                param: None,
                code: None,
            },
        });
        assert_eq!(
            map_openai_error(&rate_limited).retry(),
            intention_types::ErrorRetryDto::Delayed
        );
        assert_eq!(
            map_openai_error(&permanent).retry(),
            intention_types::ErrorRetryDto::Never
        );
        assert_eq!(
            map_openai_error(&OpenAIError::StreamError(Box::new(
                async_openai::error::StreamError::EventStream("secret provider text".to_owned())
            )))
            .retry(),
            intention_types::ErrorRetryDto::Delayed
        );
    }

    #[test]
    #[allow(
        deprecated,
        reason = "The fixture constructs the SDK stream-chunk shape with its deprecated fingerprint field."
    )]
    fn usage_only_final_chunk_maps_before_finish() {
        let mut state = GenericStreamState::new(
            futures_util::stream::empty::<Result<CreateChatCompletionStreamResponse, OpenAIError>>(
            ),
            ModelCancellationSignal::new(),
        );
        state.accept_chunk(CreateChatCompletionStreamResponse {
            id: "fixture".to_owned(),
            choices: Vec::new(),
            created: 0,
            model: "fixture".to_owned(),
            service_tier: None,
            system_fingerprint: None,
            object: "chat.completion.chunk".to_owned(),
            usage: Some(async_openai::types::chat::CompletionUsage {
                prompt_tokens: 2,
                completion_tokens: 3,
                total_tokens: 5,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }),
        });
        state.finish(FinishReasonDto::Stop);
        assert_eq!(
            state.pending.pop_front(),
            Some(Ok(ModelEventDto::started()))
        );
        assert_eq!(
            state.pending.pop_front(),
            Some(Ok(ModelEventDto::usage(
                UsageDto::reported(2, 3, 5).expect("usage is valid")
            )))
        );
        assert_eq!(
            state.pending.pop_front(),
            Some(Ok(ModelEventDto::finished(FinishReasonDto::Stop)))
        );
    }

    #[test]
    fn conflicting_modern_and_legacy_calls_fail_without_duplicate_output() {
        let mut state = GenericStreamState::new(
            futures_util::stream::empty::<Result<CreateChatCompletionStreamResponse, OpenAIError>>(
            ),
            ModelCancellationSignal::new(),
        );
        let mut modern = FunctionToolFragments::default();
        modern
            .merge(
                Some("modern".to_owned()),
                Some("function".to_owned()),
                Some(async_openai::types::chat::FunctionCallStream {
                    name: Some("inspect".to_owned()),
                    arguments: Some("{}".to_owned()),
                }),
            )
            .expect("modern fixture is valid");
        let mut legacy = FunctionToolFragments::default();
        legacy
            .merge_legacy(async_openai::types::chat::FunctionCallStream {
                name: Some("inspect".to_owned()),
                arguments: Some("{}".to_owned()),
            })
            .expect("legacy fixture is valid");
        state.tools.insert((0, 0), modern);
        state.legacy_tools.insert(0, legacy);
        state.finish(FinishReasonDto::ToolCalls);
        assert_eq!(
            state.pending.pop_front(),
            Some(Ok(ModelEventDto::started()))
        );
        assert!(matches!(
            state.pending.pop_front(),
            Some(Err(error)) if error.code() == "generic_chat_conflicting_tool_call"
        ));
    }

    #[allow(
        deprecated,
        reason = "The private fixture constructs the SDK stream-chunk shape with its deprecated fingerprint field."
    )]
    fn chunk(
        choices: Vec<async_openai::types::chat::ChatChoiceStream>,
        usage: Option<async_openai::types::chat::CompletionUsage>,
    ) -> CreateChatCompletionStreamResponse {
        CreateChatCompletionStreamResponse {
            id: "fixture".to_owned(),
            choices,
            created: 0,
            model: "fixture".to_owned(),
            service_tier: None,
            system_fingerprint: None,
            object: "chat.completion.chunk".to_owned(),
            usage,
        }
    }

    #[allow(
        deprecated,
        reason = "The private fixture constructs the SDK legacy-field shape accepted by normalization."
    )]
    fn delta(
        content: Option<&str>,
        tool_calls: Option<Vec<async_openai::types::chat::ChatCompletionMessageToolCallChunk>>,
        finish_reason: Option<FinishReason>,
    ) -> async_openai::types::chat::ChatChoiceStream {
        async_openai::types::chat::ChatChoiceStream {
            index: 0,
            delta: ChatCompletionStreamResponseDelta {
                content: content.map(str::to_owned),
                function_call: None,
                tool_calls,
                role: None,
                refusal: None,
            },
            finish_reason,
            logprobs: None,
        }
    }

    #[test]
    fn native_chunks_normalize_content_tools_and_terminal_finish() {
        let mut state = GenericStreamState::new(stream::empty(), ModelCancellationSignal::new());
        state.accept_chunk(chunk(
            vec![delta(
                Some("answer"),
                Some(vec![
                    async_openai::types::chat::ChatCompletionMessageToolCallChunk {
                        index: 0,
                        id: Some("call".to_owned()),
                        r#type: Some(async_openai::types::chat::FunctionType::Function),
                        function: Some(async_openai::types::chat::FunctionCallStream {
                            name: Some("inspect".to_owned()),
                            arguments: Some("{}".to_owned()),
                        }),
                    },
                ]),
                Some(FinishReason::ToolCalls),
            )],
            None,
        ));
        let reason = state.terminal_reason.take().expect("finish is present");
        state.finish(reason);

        assert_eq!(
            state.pending.pop_front(),
            Some(Ok(ModelEventDto::started()))
        );
        assert_eq!(
            state.pending.pop_front(),
            Some(Ok(ModelEventDto::text_delta("answer").expect("valid text")))
        );
        assert!(matches!(
            state.pending.pop_front(),
            Some(Ok(ModelEventDto::ToolCall { call })) if call.name() == "inspect"
        ));
        assert_eq!(
            state.pending.pop_front(),
            Some(Ok(ModelEventDto::finished(FinishReasonDto::ToolCalls)))
        );
    }

    #[test]
    fn native_chunks_reject_duplicate_finish_invalid_usage_and_incomplete_tools() {
        let mut duplicate =
            GenericStreamState::new(stream::empty(), ModelCancellationSignal::new());
        duplicate.accept_chunk(chunk(
            vec![delta(None, None, Some(FinishReason::Stop))],
            None,
        ));
        duplicate.accept_chunk(chunk(
            vec![delta(None, None, Some(FinishReason::Length))],
            None,
        ));
        assert!(matches!(
            duplicate.pending.back(),
            Some(Err(error)) if error.code() == "generic_chat_duplicate_finish"
        ));

        let mut invalid_usage =
            GenericStreamState::new(stream::empty(), ModelCancellationSignal::new());
        invalid_usage.accept_chunk(chunk(
            Vec::new(),
            Some(async_openai::types::chat::CompletionUsage {
                prompt_tokens: 1,
                completion_tokens: 1,
                total_tokens: 1,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }),
        ));
        assert!(matches!(
            invalid_usage.pending.back(),
            Some(Err(error)) if error.code() == "generic_chat_invalid_usage"
        ));

        let mut incomplete =
            GenericStreamState::new(stream::empty(), ModelCancellationSignal::new());
        incomplete.accept_chunk(chunk(
            vec![delta(
                None,
                Some(vec![
                    async_openai::types::chat::ChatCompletionMessageToolCallChunk {
                        index: 0,
                        id: Some("call".to_owned()),
                        r#type: Some(async_openai::types::chat::FunctionType::Function),
                        function: Some(async_openai::types::chat::FunctionCallStream {
                            name: None,
                            arguments: Some("{}".to_owned()),
                        }),
                    },
                ]),
                Some(FinishReason::ToolCalls),
            )],
            None,
        ));
        let reason = incomplete
            .terminal_reason
            .take()
            .expect("finish is present");
        incomplete.finish(reason);
        assert!(matches!(
            incomplete.pending.back(),
            Some(Err(error)) if error.code() == "generic_chat_invalid_tool_call"
        ));
    }
}
