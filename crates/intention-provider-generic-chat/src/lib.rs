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
        ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls,
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestAssistantMessageContent,
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
        ChatCompletionRequestSystemMessageContent, ChatCompletionRequestToolMessage,
        ChatCompletionRequestToolMessageContent, ChatCompletionRequestUserMessage,
        ChatCompletionRequestUserMessageContent, ChatCompletionStreamOptions,
        ChatCompletionStreamResponseDelta, CreateChatCompletionRequest,
        CreateChatCompletionRequestArgs, CreateChatCompletionStreamResponse, FinishReason,
        FunctionCall, ReasoningEffort,
    },
};
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

/// Additive descriptor-driven driver options; the default is unchanged.
#[derive(Clone, Debug, Default)]
pub struct GenericChatDriverOptions {
    header_policy: Option<AuthenticationHeaderPolicyV1>,
    reasoning_effort: Option<ReasoningEffortLevel>,
}

impl GenericChatDriverOptions {
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
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Moving the validated policy into the option requires a drop that const fn cannot evaluate."
    )]
    pub fn with_header_policy(mut self, policy: AuthenticationHeaderPolicyV1) -> Self {
        self.header_policy = Some(policy);
        self
    }

    /// Declares the closed reasoning effort request field.
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
    /// safe-header transport or the maximum reasoning effort is declared (the
    /// pinned SDK effort set has no max).
    pub fn build(self) -> DtoResult<Self> {
        if self.header_policy.as_ref().is_some_and(|policy| {
            policy.selected_transport() == CredentialTransportMode::SafeHeader
        }) {
            return Err(ErrorDto::validation(
                "unsupported_safe_header_transport",
                "the generic chat adapter does not yet support safe-header credential transport",
            ));
        }
        if self.reasoning_effort == Some(ReasoningEffortLevel::Max) {
            return Err(ErrorDto::validation(
                "unsupported_reasoning_effort",
                "the generic chat adapter cannot express the maximum reasoning effort",
            ));
        }
        Ok(self)
    }
}

/// Generic Chat Completions driver with private SDK client state.
pub struct GenericChatDriver {
    resolved: ResolvedConfigDto,
    client: Client<OpenAIConfig>,
    options: GenericChatDriverOptions,
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

    /// Creates the driver from opaque startup-only provider material and
    /// validated descriptor-driven options.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the material selects a different provider kind.
    pub fn from_startup_material_with_options(
        material: StartupProviderMaterial,
        options: GenericChatDriverOptions,
    ) -> DtoResult<Self> {
        material.into_parts_for_provider(move |resolved, credential| {
            Self::with_credential_and_options(resolved, credential, options)
        })
    }

    fn with_credential(resolved: ResolvedConfigDto, credential: String) -> DtoResult<Self> {
        Self::with_credential_and_options(resolved, credential, GenericChatDriverOptions::default())
    }

    fn with_credential_and_options(
        resolved: ResolvedConfigDto,
        credential: String,
        options: GenericChatDriverOptions,
    ) -> DtoResult<Self> {
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

    /// Prepares the private Chat Completions request after capability validation.
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
        let native_request = match translate_request(&request, &self.options) {
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
    terminal: bool,
    terminal_reason: Option<FinishReasonDto>,
    usage_reported: bool,
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
            terminal: false,
            terminal_reason: None,
            usage_reported: false,
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
                Either::Left((Some(Ok(chunk)), _)) => self.accept_chunk(chunk),
                Either::Left((Some(Err(error)), _)) => self.fail_error(map_openai_error(&error)),
                Either::Left((None, _)) => self.native_ended(),
                Either::Right(((), _)) => return None,
            }
        }
    }

    fn accept_chunk(&mut self, chunk: CreateChatCompletionStreamResponse) {
        let post_finish = self.terminal_reason.is_some();
        if post_finish
            && chunk.choices.iter().any(|choice| {
                choice.finish_reason.is_some()
                    || choice
                        .delta
                        .content
                        .as_deref()
                        .is_some_and(|content| !content.is_empty())
                    || choice.delta.tool_calls.is_some()
            })
        {
            // Chunks after the finish reason may carry at most the final
            // usage summary; any further content, tool fragment, or finish is
            // malformed and fails the stream.
            if chunk
                .choices
                .iter()
                .any(|choice| choice.finish_reason.is_some())
            {
                self.fail("generic_chat_duplicate_finish");
            } else {
                self.fail("generic_chat_post_finish_content");
            }
            return;
        }
        if let Some(usage) = chunk.usage {
            if self.usage_reported {
                self.fail("generic_chat_duplicate_usage");
                return;
            }
            match UsageDto::reported(
                u64::from(usage.prompt_tokens),
                u64::from(usage.completion_tokens),
                u64::from(usage.total_tokens),
            ) {
                Ok(usage) => {
                    self.usage_reported = true;
                    self.pending.push_back(Ok(ModelEventDto::usage(usage)));
                }
                Err(_) => self.fail("generic_chat_invalid_usage"),
            }
        }
        if post_finish {
            return;
        }
        for choice in chunk.choices {
            if self.accept_delta(choice.index, choice.delta).is_err() {
                if !self.terminal {
                    self.fail("generic_chat_invalid_tool_call");
                }
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

    /// Handles the native end-of-stream.
    ///
    /// A recorded finish reason is terminal only once the native stream ends:
    /// standard streams deliver a trailing usage-only chunk after the
    /// finish-reason chunk, and the driver requested usage inclusion.
    /// Recording the reason and continuing to poll collects that usage
    /// exactly once (PR24-009). An absent reason fails the incomplete stream.
    fn native_ended(&mut self) {
        if let Some(reason) = self.terminal_reason.take() {
            self.finish(reason);
        } else {
            self.fail("generic_chat_stream_incomplete");
        }
    }

    fn finish(&mut self, reason: FinishReasonDto) {
        match std::mem::take(&mut self.tools)
            .into_values()
            .map(FunctionToolFragments::finish)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(calls) => {
                self.pending.extend(
                    calls
                        .into_iter()
                        .map(|call| Ok(ModelEventDto::tool_call(call))),
                );
                self.pending.push_back(Ok(ModelEventDto::finished(reason)));
                self.terminal = true;
            }
            Err(()) => self.fail("generic_chat_invalid_tool_call"),
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
        "tool_calls" => FinishReasonDto::ToolCalls,
        "content_filter" => FinishReasonDto::ContentFilter,
        "error" => FinishReasonDto::Error,
        _ => FinishReasonDto::Unknown,
    }
}

const fn map_native_finish(reason: FinishReason) -> FinishReasonDto {
    match reason {
        FinishReason::Stop => FinishReasonDto::Stop,
        FinishReason::Length => FinishReasonDto::Length,
        FinishReason::ToolCalls => FinishReasonDto::ToolCalls,
        FinishReason::ContentFilter => FinishReasonDto::ContentFilter,
        FinishReason::FunctionCall => FinishReasonDto::Unknown,
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

fn translate_request(
    request: &ModelRequestDto,
    options: &GenericChatDriverOptions,
) -> DtoResult<CreateChatCompletionRequest> {
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
    let mut args = CreateChatCompletionRequestArgs::default();
    args.model(request.model());
    args.messages(messages);
    args.stream_options(ChatCompletionStreamOptions {
        include_usage: Some(true),
        include_obfuscation: None,
    });
    if let Some(effort) = options.reasoning_effort {
        args.reasoning_effort(map_reasoning_effort(effort)?);
    }
    args.build().map_err(|_| {
        ErrorDto::validation(
            "invalid_generic_chat_request",
            "generic chat request could not be translated",
        )
    })
}

/// Maps the closed effort level onto the pinned SDK effort set.
///
/// # Errors
///
/// Returns a validation error for the maximum effort, which the pinned SDK
/// effort set cannot express.
fn map_reasoning_effort(effort: ReasoningEffortLevel) -> DtoResult<ReasoningEffort> {
    match effort {
        ReasoningEffortLevel::None => Ok(ReasoningEffort::None),
        ReasoningEffortLevel::Minimal => Ok(ReasoningEffort::Minimal),
        ReasoningEffortLevel::Low => Ok(ReasoningEffort::Low),
        ReasoningEffortLevel::Medium => Ok(ReasoningEffort::Medium),
        ReasoningEffortLevel::High => Ok(ReasoningEffort::High),
        ReasoningEffortLevel::Xhigh => Ok(ReasoningEffort::Xhigh),
        ReasoningEffortLevel::Max => Err(ErrorDto::validation(
            "unsupported_reasoning_effort",
            "the generic chat adapter cannot express the maximum reasoning effort",
        )),
    }
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
        ModelRoleDto::Assistant => translate_assistant_message(message),
        ModelRoleDto::Tool => {
            let tool_call_id = message.tool_call_id().ok_or_else(|| {
                ErrorDto::validation(
                    "invalid_generic_chat_request",
                    "tool-role messages must carry one tool call identity",
                )
            })?;
            Ok(ChatCompletionRequestMessage::Tool(
                ChatCompletionRequestToolMessage {
                    content: ChatCompletionRequestToolMessageContent::Text(
                        message.content().to_owned(),
                    ),
                    tool_call_id: tool_call_id.to_string(),
                },
            ))
        }
    }
}

fn translate_assistant_message(
    message: &ModelMessageDto,
) -> DtoResult<ChatCompletionRequestMessage> {
    let content = message.content();
    message.tool_calls().map_or_else(
        || {
            ChatCompletionRequestAssistantMessageArgs::default()
                .content(ChatCompletionRequestAssistantMessageContent::Text(
                    content.to_owned(),
                ))
                .build()
                .map(ChatCompletionRequestMessage::Assistant)
                .map_err(|_| {
                    ErrorDto::validation(
                        "invalid_generic_chat_request",
                        "generic chat request could not be translated",
                    )
                })
        },
        |tool_calls| {
            // The assistant content is optional when tool calls are present, so
            // an empty DTO content stays omitted on the wire.
            let mut args = ChatCompletionRequestAssistantMessageArgs::default();
            if !content.is_empty() {
                args.content(ChatCompletionRequestAssistantMessageContent::Text(
                    content.to_owned(),
                ));
            }
            args.tool_calls(
                tool_calls
                    .iter()
                    .map(|call| {
                        ChatCompletionMessageToolCalls::from(ChatCompletionMessageToolCall {
                            id: call.call_id().to_string(),
                            function: FunctionCall {
                                name: call.name().to_owned(),
                                arguments: call.arguments_json().to_owned(),
                            },
                        })
                    })
                    .collect::<Vec<_>>(),
            )
            .build()
            .map(ChatCompletionRequestMessage::Assistant)
            .map_err(|_| {
                ErrorDto::validation(
                    "invalid_generic_chat_request",
                    "generic chat request could not be translated",
                )
            })
        },
    )
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "Private tool-fragment fixtures use expect to provide precise test failure messages."
)]
mod tests {
    use super::*;
    use intention_types::RunId;

    #[test]
    fn generic_chat_translates_assistant_tool_calls_and_tool_results() {
        let call = ToolCallDto::new(ToolCallId::new(), "read", r#"{"path":"hello.txt"}"#)
            .expect("fixture call is valid");
        let request = ModelRequestDto::new(
            RunId::new(),
            "fixture",
            vec![
                ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid"),
                ModelMessageDto::assistant_tool_calls(
                    Some("before".to_owned()),
                    vec![call.clone()],
                )
                .expect("message is valid"),
                ModelMessageDto::tool_result(call.call_id(), "hello world")
                    .expect("message is valid"),
            ],
            None,
            None,
        )
        .expect("request is valid");

        let wire = serde_json::to_value(
            translate_request(&request, &GenericChatDriverOptions::default())
                .expect("request translates"),
        )
        .expect("request serializes");
        assert_eq!(
            wire,
            serde_json::json!({
                "model": "fixture",
                "messages": [
                    {"role": "user", "content": "hello"},
                    {
                        "role": "assistant",
                        "content": "before",
                        "tool_calls": [{
                            "id": call.call_id().to_string(),
                            "type": "function",
                            "function": {
                                "name": "read",
                                "arguments": r#"{"path":"hello.txt"}"#,
                            },
                        }],
                    },
                    {
                        "role": "tool",
                        "content": "hello world",
                        "tool_call_id": call.call_id().to_string(),
                    },
                ],
                "stream_options": {"include_usage": true},
            })
        );

        // The model-tool loop's follow-up round carries an assistant tool-call
        // message without text; its empty content stays omitted on the wire.
        let follow_up = ModelRequestDto::new(
            RunId::new(),
            "fixture",
            vec![
                ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid"),
                ModelMessageDto::assistant_tool_calls(None, vec![call.clone()])
                    .expect("message is valid"),
            ],
            None,
            None,
        )
        .expect("request is valid");
        let wire = serde_json::to_value(
            translate_request(&follow_up, &GenericChatDriverOptions::default())
                .expect("request translates"),
        )
        .expect("request serializes");
        let assistant = &wire["messages"][1];
        assert!(assistant.get("content").is_none());
        assert_eq!(
            assistant["tool_calls"][0],
            serde_json::json!({
                "id": call.call_id().to_string(),
                "type": "function",
                "function": {
                    "name": "read",
                    "arguments": r#"{"path":"hello.txt"}"#,
                },
            })
        );
    }

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
    fn trailing_usage_after_finish_reason_is_drained_before_finished() {
        // Standard stream order: finish-reason chunk, trailing usage-only
        // chunk, then end. The finish reason must not terminalize the stream
        // before the usage chunk is polled (PR24-009).
        let mut state = GenericStreamState::new(
            futures_util::stream::empty::<Result<CreateChatCompletionStreamResponse, OpenAIError>>(
            ),
            ModelCancellationSignal::new(),
        );
        state.accept_chunk(chunk(
            vec![delta(None, None, Some(FinishReason::Stop))],
            None,
        ));
        assert!(
            state.terminal_reason.is_some(),
            "the finish reason is recorded when its chunk arrives"
        );
        assert!(
            !state.terminal,
            "a recorded finish reason must not terminalize the stream"
        );
        state.accept_chunk(chunk(
            Vec::new(),
            Some(async_openai::types::chat::CompletionUsage {
                prompt_tokens: 2,
                completion_tokens: 3,
                total_tokens: 5,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }),
        ));
        // The native stream ends: the recorded reason becomes terminal and
        // the collected usage is emitted before Finished.
        state.native_ended();
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
        assert_eq!(state.pending.pop_front(), None);
        assert!(state.terminal);
    }

    #[test]
    #[allow(
        deprecated,
        reason = "The fixture constructs the SDK stream-chunk shape with its deprecated fingerprint field."
    )]
    fn usage_is_emitted_at_most_once_and_post_finish_content_fails() {
        let mut duplicated =
            GenericStreamState::new(stream::empty(), ModelCancellationSignal::new());
        let usage = || async_openai::types::chat::CompletionUsage {
            prompt_tokens: 2,
            completion_tokens: 3,
            total_tokens: 5,
            prompt_tokens_details: None,
            completion_tokens_details: None,
        };
        duplicated.accept_chunk(chunk(Vec::new(), Some(usage())));
        duplicated.accept_chunk(chunk(Vec::new(), Some(usage())));
        assert!(matches!(
            duplicated.pending.back(),
            Some(Err(error)) if error.code() == "generic_chat_duplicate_usage"
        ));

        let mut post_finish =
            GenericStreamState::new(stream::empty(), ModelCancellationSignal::new());
        post_finish.accept_chunk(chunk(
            vec![delta(None, None, Some(FinishReason::Stop))],
            None,
        ));
        post_finish.accept_chunk(chunk(vec![delta(Some("late"), None, None)], None));
        assert!(matches!(
            post_finish.pending.back(),
            Some(Err(error)) if error.code() == "generic_chat_post_finish_content"
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
        reason = "The pinned SDK delta struct requires its deprecated function-call field in initializers."
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

    #[test]
    fn descriptor_options_reject_before_any_request_and_never_disclose_policy() {
        let bearer = AuthenticationHeaderPolicyV1::new(
            Vec::new(),
            intention_model::CredentialTransportMode::Bearer,
        )
        .expect("bearer policy is valid");
        let options = GenericChatDriverOptions::new()
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
            GenericChatDriverOptions::new()
                .with_header_policy(safe_header)
                .build()
                .expect_err("safe-header transport is rejected before any request")
                .code(),
            "unsupported_safe_header_transport"
        );

        let material = startup_material();
        let driver = GenericChatDriver::from_startup_material_with_options(material, options)
            .expect("driver builds with validated options");
        let debug = format!("{driver:?}");
        assert!(!debug.contains("fixture-credential-not-real-12345"));
        assert!(!debug.contains("X-Custom-Auth"));
        assert!(!debug.contains("reasoning"));
        assert_eq!(driver.prepared_request_count(), 0);
    }

    #[test]
    fn unapplicable_effort_declarations_reject_before_any_request() {
        assert_eq!(
            GenericChatDriverOptions::new()
                .with_reasoning_effort(ReasoningEffortLevel::Max)
                .build()
                .expect_err("maximum effort is rejected")
                .code(),
            "unsupported_reasoning_effort"
        );
    }

    #[test]
    fn declared_reasoning_effort_is_applied_to_the_native_request() {
        let options = GenericChatDriverOptions::new()
            .with_reasoning_effort(ReasoningEffortLevel::Low)
            .build()
            .expect("descriptor options build");
        let request = ModelRequestDto::new(
            RunId::new(),
            "fixture",
            vec![ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid")],
            None,
            None,
        )
        .expect("request is valid");
        let wire = serde_json::to_value(
            translate_request(&request, &options).expect("request translates"),
        )
        .expect("request serializes");
        assert_eq!(wire["reasoning_effort"], "low");

        let default_wire = serde_json::to_value(
            translate_request(&request, &GenericChatDriverOptions::default())
                .expect("request translates"),
        )
        .expect("request serializes");
        assert!(default_wire.get("reasoning_effort").is_none());
    }

    #[test]
    fn normalized_reasoning_failures_never_carry_raw_provider_text() {
        let mut state = GenericStreamState::new(stream::empty(), ModelCancellationSignal::new());
        state.fail("provider_reasoning_stream_invalid");
        let error = state
            .pending
            .back()
            .expect("failure is pending")
            .as_ref()
            .expect_err("failure is an error");
        assert_eq!(error.code(), "provider_reasoning_stream_invalid");
        let encoded = serde_json::to_string(error).expect("error serializes");
        assert!(!encoded.contains("secret provider text"));
        assert!(!encoded.contains("fixture-credential-not-real-12345"));
    }

    fn startup_material() -> StartupProviderMaterial {
        ResolvedConfigDto::parse_startup_material(intention_config::RawConfigInputDto::new(
            "schema_version = 1\n[provider]\nkind = \"generic-chat-completion-api\"\nmodel = \"fixture\"\nendpoint = \"https://example.invalid/v1\"\ncredential = \"fixture-credential-not-real-12345\"",
            intention_config::ConfigSourceDto::Explicit(
                intention_config::ConfigPathDto::parse(
                    std::env::temp_dir()
                        .join("intention-relay-generic-chat-options.toml")
                        .to_string_lossy()
                        .into_owned(),
                )
                .expect("fixture path is absolute"),
            ),
        ))
        .expect("generic chat config resolves")
    }
}
