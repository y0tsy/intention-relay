//! OpenRouter provider normalization backed privately by `openrouter-rs`.
//!
//! This adapter owns OpenRouter SDK construction and request translation. It
//! emits only provider-neutral model DTOs and never exposes SDK stream resources.

use crate::mapping;
use crate::mapping::WireRole;
use crate::model::ModelToolDefinitionDto;
use crate::model::{
    FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto, ModelEventDto,
    ModelEventStream, ModelExecutionDriver, ModelMessageDto, ModelRequestDto, ProviderErrorDto,
};
use crate::stream::{EventTranslator, NormalizedEvents, normalized_stream};
use futures_util::{StreamExt, stream};
use intention_config::{ProviderKindDto, ResolvedConfigDto, StartupProviderMaterial};
use intention_proto::{DtoResult, ErrorDto};
use openrouter_rs::{
    OpenRouterClient,
    api::chat::{ChatCompletionRequest, ContentPart, Message},
    error::OpenRouterError,
    types::{FinishReason as OpenRouterFinishReason, Role, stream::StreamEvent},
};

/// OpenRouter driver with private SDK client state.
pub struct OpenRouterDriver {
    resolved: ResolvedConfigDto,
    client: OpenRouterClient,
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
        Ok(Self { resolved, client })
    }
}

impl ModelExecutionDriver for OpenRouterDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    }

    fn execute(
        &self,
        request: ModelRequestDto,
        cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        // An already-cancelled run never starts provider work. Mid-stream
        // interruption belongs to the caller that owns the run: it races the
        // signal and drops this stream, and dropping aborts the SDK response.
        if cancellation.is_cancelled() {
            return Box::pin(stream::empty());
        }
        let native_request = match translate_request(&request) {
            Ok(request) => request,
            Err(_) => {
                return Box::pin(stream::once(async {
                    Err(mapping::fixed_error("openrouter_request_rejected"))
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
                        |native| normalized_stream(native, OpenRouterTranslator),
                    )
            })
            .flatten(),
        )
    }
}

/// Translates the OpenRouter adapter's native events into normalized events.
struct OpenRouterTranslator;

impl EventTranslator for OpenRouterTranslator {
    type Item = StreamEvent;

    fn translate_item(&mut self, event: Self::Item, events: &mut NormalizedEvents<'_>) {
        match event {
            StreamEvent::ContentDelta(content) => match ModelEventDto::text_delta(content) {
                Ok(event) => events.push(event),
                Err(_) => events.fail("openrouter_invalid_text"),
            },
            StreamEvent::ReasoningDelta(content) => match ModelEventDto::reasoning_delta(content) {
                Ok(event) => events.push(event),
                Err(_) => events.fail("openrouter_invalid_reasoning"),
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
                        mapping::complete_tool_call(&call.function.name, &call.function.arguments)
                    })
                    .collect::<DtoResult<Vec<_>>>();
                match calls {
                    Ok(calls) => {
                        for call in calls {
                            events.push(ModelEventDto::tool_call(call));
                        }
                        if let Some(usage) = usage {
                            match mapping::reported_usage(
                                usage.prompt_tokens,
                                usage.completion_tokens,
                                usage.total_tokens,
                            ) {
                                Ok(usage) => events.push(ModelEventDto::usage(usage)),
                                Err(_) => {
                                    events.fail("openrouter_invalid_usage");
                                    return;
                                }
                            }
                        }
                        events.finish(
                            finish_reason
                                .map_or(FinishReasonDto::Unknown, map_native_finish_reason),
                        );
                    }
                    Err(_) => events.fail("openrouter_invalid_tool_call"),
                }
            }
            StreamEvent::Error(error) => events.fail_error(map_openrouter_error(&error)),
            StreamEvent::ReasoningDetailsDelta(_) => {}
            _ => events.fail("openrouter_unsupported_stream_event"),
        }
    }

    fn native_ended(&mut self, events: &mut NormalizedEvents<'_>) {
        events.fail("openrouter_stream_incomplete");
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
    mapping::provider_error(
        "openrouter_provider_unavailable",
        "openrouter_provider_request_rejected",
        retryable,
    )
}

/// Translates one provider-neutral request into the private native SDK shape.
///
/// The daemon-owned system context always closes with the SDK's ephemeral
/// prompt-cache marker: it is the stable instruction block every round of the
/// attempt repeats. A provider-neutral message carries its own marker when the
/// runtime's context window pass closed the stable window prefix on it.
///
/// The provider-neutral assistant reasoning attachment
/// (`ModelRequestDto::assistant_reasoning()`) is intentionally not consumed by
/// this adapter: the pinned OpenRouter chat-completions SDK has no
/// assistant-message reasoning echo field, and OpenRouter expresses reasoning
/// through request-scoped options (`reasoning`, `include_reasoning`) plus
/// response-side reasoning content. The attachment stays runtime-owned
/// transient same-run state and never changes this native request shape.
fn translate_request(request: &ModelRequestDto) -> DtoResult<ChatCompletionRequest> {
    let mut messages = Vec::new();
    if let Some(context) = request.system_context() {
        messages.push(Message::with_parts(
            Role::System,
            vec![ContentPart::cacheable_text(context)],
        ));
    }
    for message in request.messages() {
        messages.push(translate_message(message)?);
    }
    let mut builder = ChatCompletionRequest::builder();
    builder.model(request.model());
    builder.messages(messages);
    if !request.tools().is_empty() {
        let tools = request
            .tools()
            .iter()
            .map(translate_tool)
            .collect::<DtoResult<Vec<_>>>()?;
        builder.tools(tools);
    }
    builder.build().map_err(|_| {
        ErrorDto::validation(
            "invalid_openrouter_request",
            "OpenRouter request could not be translated",
        )
    })
}

/// Translates one advertised tool definition into the native SDK tool shape.
///
/// The definition's validated parameter text is decoded into the exact value
/// type the native SDK constructor declares. No tool preference is declared:
/// the model chooses whether and which advertised tool to call.
///
/// # Errors
///
/// Returns a safe translation error when the parameter text does not decode.
fn translate_tool(definition: &ModelToolDefinitionDto) -> DtoResult<openrouter_rs::types::Tool> {
    Ok(openrouter_rs::types::Tool::new(
        definition.name(),
        definition.description(),
        mapping::decode_parameters(
            definition.parameters_json(),
            "invalid_openrouter_request",
            "OpenRouter request could not be translated",
        )?,
    ))
}

fn translate_message(message: &ModelMessageDto) -> DtoResult<Message> {
    match mapping::wire_role(message.role()) {
        WireRole::System => Ok(translate_text_message(Role::System, message)),
        WireRole::User => Ok(translate_text_message(Role::User, message)),
        WireRole::Assistant => translate_assistant_message(message),
        WireRole::Tool => {
            let tool_call_id =
                mapping::tool_result_identity(message, "invalid_openrouter_request")?;
            Ok(translate_tool_message(&tool_call_id.to_string(), message))
        }
    }
}

/// Translates one plain text message, marking its content as cacheable when the
/// runtime's context window pass closed a prompt-cache prefix on it.
fn translate_text_message(role: Role, message: &ModelMessageDto) -> Message {
    if message.cache_control() {
        Message::with_parts(role, vec![ContentPart::cacheable_text(message.content())])
    } else {
        Message::new(role, message.content())
    }
}

/// Translates one tool response, keeping the prompt-cache marker of a message
/// that closes a cacheable prefix.
fn translate_tool_message(tool_call_id: &str, message: &ModelMessageDto) -> Message {
    if message.cache_control() {
        Message::tool_response(
            tool_call_id,
            vec![ContentPart::cacheable_text(message.content())],
        )
    } else {
        Message::tool_response(tool_call_id, message.content())
    }
}

/// Translates an assistant message, mapping locally executed tool calls back
/// onto the OpenRouter assistant shape so the tool-result round can continue.
///
/// No reasoning echo is mapped onto the native assistant message: the pinned
/// SDK message has no such field, and [`translate_request`] carries the full
/// intentional no-op rationale for the provider-neutral attachment.
///
/// # Errors
///
/// Returns a validation error when a tool-call message cannot be mapped.
fn translate_assistant_message(message: &ModelMessageDto) -> DtoResult<Message> {
    let content = message.content();
    let Some(tool_calls) = message.tool_calls() else {
        return Ok(translate_text_message(Role::Assistant, message));
    };
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
    if message.cache_control() {
        Ok(Message::assistant_with_tool_calls(
            vec![ContentPart::cacheable_text(content)],
            native_calls,
        ))
    } else {
        Ok(Message::assistant_with_tool_calls(
            content.to_owned(),
            native_calls,
        ))
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "Private SDK error fixtures construct exact native variants."
)]
mod tests {
    use super::*;
    use crate::model::{ModelRoleDto, ToolCallDto, UsageDto};
    use futures_util::FutureExt;
    use intention_proto::{RunId, ToolCallId};

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

    fn request() -> ModelRequestDto {
        ModelRequestDto::new(
            RunId::new(),
            "fixture-model",
            vec![ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid")],
            None,
        )
        .expect("request is valid")
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

    /// Collects the shared stream's events for one fixed native event sequence.
    fn collect_events(native: Vec<StreamEvent>) -> Vec<Result<ModelEventDto, ProviderErrorDto>> {
        futures_executor::block_on(
            normalized_stream(stream::iter(native), OpenRouterTranslator).collect::<Vec<_>>(),
        )
    }

    #[test]
    fn native_stream_normalizes_content_reasoning_tools_usage_and_finish() {
        let events = collect_events(vec![
            StreamEvent::ContentDelta("answer".to_owned()),
            StreamEvent::ReasoningDelta("because".to_owned()),
            done(
                vec![openrouter_rs::types::ToolCall::new("call", "inspect", "{}")],
                Some(OpenRouterFinishReason::ToolCalls),
                Some(usage(2, 3, 5)),
            ),
        ]);

        assert_eq!(
            events[..3],
            [
                Ok(ModelEventDto::started()),
                Ok(ModelEventDto::text_delta("answer").expect("valid text")),
                Ok(ModelEventDto::reasoning_delta("because").expect("valid reasoning")),
            ]
        );
        assert!(matches!(
            events.get(3),
            Some(Ok(ModelEventDto::ToolCall { call })) if call.name() == "inspect"
        ));
        assert_eq!(
            events.get(4),
            Some(&Ok(ModelEventDto::usage(
                UsageDto::reported(2, 3, 5).expect("valid usage")
            )))
        );
        assert_eq!(
            events.get(5),
            Some(&Ok(ModelEventDto::finished(FinishReasonDto::ToolCalls)))
        );
        assert_eq!(events.len(), 6);
    }

    #[test]
    fn native_stream_rejects_invalid_incomplete_and_unsupported_events_safely() {
        let invalid_text = collect_events(vec![StreamEvent::ContentDelta(String::new())]);
        assert!(matches!(
            invalid_text.last(),
            Some(Err(error)) if error.code() == "openrouter_invalid_text"
        ));

        let invalid_reasoning = collect_events(vec![StreamEvent::ReasoningDelta(String::new())]);
        assert!(matches!(
            invalid_reasoning.last(),
            Some(Err(error)) if error.code() == "openrouter_invalid_reasoning"
        ));

        let invalid_usage = collect_events(vec![done(Vec::new(), None, Some(usage(1, 1, 1)))]);
        assert!(matches!(
            invalid_usage.last(),
            Some(Err(error)) if error.code() == "openrouter_invalid_usage"
        ));

        let invalid_tool = collect_events(vec![done(
            vec![openrouter_rs::types::ToolCall::new("call", "", "{}")],
            None,
            None,
        )]);
        assert!(matches!(
            invalid_tool.last(),
            Some(Err(error)) if error.code() == "openrouter_invalid_tool_call"
        ));

        // An empty reasoning-details event carries no reasoning: it emits no
        // fact, so the stream reports only the missing terminal event.
        let empty_details = collect_events(vec![StreamEvent::ReasoningDetailsDelta(Vec::new())]);
        assert_eq!(empty_details.len(), 2);
        assert_eq!(empty_details[0], Ok(ModelEventDto::started()));
        assert!(matches!(
            empty_details.last(),
            Some(Err(error)) if error.code() == "openrouter_stream_incomplete"
        ));
    }

    #[test]
    fn declared_tools_are_translated_without_tool_choice() {
        let definition = ModelToolDefinitionDto::new(
            "read",
            "Read a workspace file",
            r#"{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}"#,
        )
        .expect("tool definition is valid");
        let request = request()
            .with_tools(vec![definition])
            .expect("tool advertisement is valid");

        let wire = serde_json::to_value(translate_request(&request).expect("request translates"))
            .expect("request serializes");

        assert_eq!(wire["tools"][0]["type"], "function");
        assert_eq!(wire["tools"][0]["function"]["name"], "read");
        assert_eq!(
            wire["tools"][0]["function"]["description"],
            "Read a workspace file"
        );
        assert_eq!(
            wire["tools"][0]["function"]["parameters"],
            serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
            })
        );
        assert!(wire.get("tool_choice").is_none());
    }

    #[test]
    fn cache_markers_close_the_system_block_and_the_stable_prefix() {
        let call = ToolCallDto::new(ToolCallId::new(), "read", r#"{"path":"hello.txt"}"#)
            .expect("fixture call is valid");
        let mut result =
            ModelMessageDto::tool_result(call.call_id(), "hello world").expect("message is valid");
        result.set_cache_control(true);
        let request = ModelRequestDto::new(
            RunId::new(),
            "fixture-model",
            vec![
                ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid"),
                ModelMessageDto::assistant_tool_calls(None, vec![call]).expect("message is valid"),
                result,
            ],
            Some("instructions".to_owned()),
        )
        .expect("request is valid");

        let wire = serde_json::to_value(translate_request(&request).expect("request translates"))
            .expect("request serializes");

        assert_eq!(
            wire["messages"][0]["content"][0],
            serde_json::json!({
                "type": "text",
                "text": "instructions",
                "cache_control": {"type": "ephemeral"},
            })
        );
        assert_eq!(wire["messages"][1]["content"], "hello");
        assert!(wire["messages"][2].get("cache_control").is_none());
        assert_eq!(
            wire["messages"][3]["content"][0],
            serde_json::json!({
                "type": "text",
                "text": "hello world",
                "cache_control": {"type": "ephemeral"},
            }),
            "the flagged tool result closes the stable window prefix"
        );
    }

    #[test]
    fn requests_without_declared_tools_carry_no_tools_on_the_wire() {
        let wire = serde_json::to_value(translate_request(&request()).expect("request translates"))
            .expect("request serializes");

        assert!(wire.get("tools").is_none());
        assert!(wire.get("tool_choice").is_none());
    }

    #[test]
    fn normalized_stream_emits_started_and_incomplete_error() {
        let mut stream = normalized_stream(stream::empty::<StreamEvent>(), OpenRouterTranslator);
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
    fn native_stream_errors_map_retryability_without_native_text() {
        let retryable = collect_events(vec![StreamEvent::Error(api_error(
            http::StatusCode::SERVICE_UNAVAILABLE,
        ))]);
        assert!(matches!(
            retryable.last(),
            Some(Err(error)) if error.code() == "openrouter_provider_unavailable"
                && error.retry() == intention_proto::ErrorRetryDto::Delayed
        ));
        assert!(
            !serde_json::to_string(&retryable)
                .expect("normalized errors serialize")
                .contains("secret")
        );

        let permanent = collect_events(vec![StreamEvent::Error(api_error(
            http::StatusCode::BAD_REQUEST,
        ))]);
        assert!(matches!(
            permanent.last(),
            Some(Err(error)) if error.code() == "openrouter_provider_request_rejected"
                && error.retry() == intention_proto::ErrorRetryDto::Never
        ));
    }

    #[test]
    fn notice_role_translates_as_a_user_message_with_its_text_unchanged() {
        let request = ModelRequestDto::new(
            RunId::new(),
            "fixture-model",
            vec![
                ModelMessageDto::new(
                    ModelRoleDto::Notice,
                    "[The tool call \"read\" did not receive a final result.]",
                )
                .expect("notice is valid"),
            ],
            None,
        )
        .expect("request is valid");
        let wire = serde_json::to_value(translate_request(&request).expect("request translates"))
            .expect("request serializes");
        assert_eq!(
            wire["messages"],
            serde_json::json!([
                {
                    "role": "user",
                    "content": "[The tool call \"read\" did not receive a final result.]",
                },
            ])
        );
    }

    #[test]
    fn tool_round_two_translates_assistant_calls_and_tool_results() {
        let call = ToolCallDto::new(ToolCallId::new(), "read", r#"{"path":"hello.txt"}"#)
            .expect("fixture call is valid");
        let definition = ModelToolDefinitionDto::new(
            "read",
            "Read a workspace file",
            r#"{"type":"object","properties":{"path":{"type":"string"}}}"#,
        )
        .expect("tool definition is valid");
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
        )
        .expect("request is valid")
        .with_tools(vec![definition])
        .expect("tool advertisement is valid");

        let wire = serde_json::to_value(translate_request(&request).expect("request translates"))
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
        assert_eq!(wire["tools"][0]["function"]["name"], "read");
        assert!(wire.get("tool_choice").is_none());
    }

    #[test]
    fn assistant_reasoning_attachment_never_changes_the_native_request() {
        let call = ToolCallDto::new(ToolCallId::new(), "read", r#"{"path":"hello.txt"}"#)
            .expect("fixture call is valid");
        let run_id = RunId::new();
        let messages = vec![
            ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid"),
            ModelMessageDto::assistant_tool_calls(None, vec![call.clone()])
                .expect("message is valid"),
            ModelMessageDto::tool_result(call.call_id(), "hello world").expect("message is valid"),
        ];
        let plain = ModelRequestDto::new(run_id, "fixture-model", messages.clone(), None)
            .expect("request is valid");
        assert!(plain.assistant_reasoning().is_empty());
        let attachment =
            crate::model::AssistantReasoningDto::new(vec![call.call_id()], "chain of thought")
                .expect("reasoning attachment is valid");
        let attached = ModelRequestDto::new(run_id, "fixture-model", messages, None)
            .expect("request is valid")
            .with_assistant_reasoning(vec![attachment])
            .expect("attachment is retained on the request");
        assert_eq!(
            attached.assistant_reasoning().len(),
            1,
            "the fixture must carry the attachment, otherwise the comparison is vacuous"
        );

        let plain_wire =
            serde_json::to_value(translate_request(&plain).expect("request translates"))
                .expect("request serializes");
        let attached_wire =
            serde_json::to_value(translate_request(&attached).expect("request translates"))
                .expect("request serializes");

        assert_eq!(attached_wire, plain_wire);
        assert!(
            !serde_json::to_string(&attached_wire)
                .expect("wire serializes")
                .contains("chain of thought"),
            "transient reasoning text must never enter the native request"
        );
    }
}
