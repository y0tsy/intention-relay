//! OpenRouter provider normalization backed privately by `openrouter-rs`.
//!
//! This adapter owns OpenRouter SDK construction and request translation. It
//! emits only provider-neutral model DTOs and never exposes SDK stream resources.

use crate::auth::AuthenticationHeaderPolicyV1;
use crate::descriptor;
use crate::mapping;
use crate::mapping::WireRole;
use crate::model::ModelToolDefinitionDto;
use crate::model::{
    FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto, ModelEventDto,
    ModelEventStream, ModelExecutionDriver, ModelMessageDto, ModelRequestDto, ProviderErrorDto,
    ProviderHealthEvidenceDto, ProviderHealthReasonDto, ProviderHealthStateDto,
    ProviderModelRecordDto, ProviderProbeFuture, ReasoningFragmentCategoryDto,
};
use crate::stream::{EventTranslator, NormalizedEvents, normalized_stream};
use futures_util::{StreamExt, stream};
use intention_proto::provider::{
    CredentialTransportDto, CredentialTransportModeDto, ProviderKindId, ProviderProfileId,
    ProviderProfileRevisionV1,
};
use intention_proto::{DtoResult, ErrorDto};
use openrouter_rs::{
    OpenRouterClient,
    api::chat::{ChatCompletionRequest, ContentPart, Message},
    error::OpenRouterError,
    types::{FinishReason as OpenRouterFinishReason, Role, stream::StreamEvent},
};

/// The pinned SDK's discovered model record.
type OpenRouterModel = openrouter_rs::api::models::Model;

/// The fixed non-retryable failure for a request this adapter cannot translate.
const OPENROUTER_REQUEST_REJECTED: &str = "openrouter_request_rejected";

/// The closed driver options of the OpenRouter adapter.
///
/// The one declared option is the exact profile's credential transport. The
/// adapter applies it at construction: a declaration it cannot apply fails
/// closed instead of being silently defaulted or ignored.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenRouterDriverOptions {
    credential_transport: CredentialTransportDto,
}

impl OpenRouterDriverOptions {
    /// Creates the options declared by one exact profile revision.
    #[must_use]
    pub fn from_profile_revision(revision: &ProviderProfileRevisionV1) -> Self {
        Self {
            credential_transport: revision.credential_transport().clone(),
        }
    }

    /// Returns the declared credential transport.
    #[must_use]
    pub const fn credential_transport(&self) -> &CredentialTransportDto {
        &self.credential_transport
    }

    /// Returns the authentication header policy this adapter applies.
    #[must_use]
    fn authentication_header_policy(&self) -> AuthenticationHeaderPolicyV1 {
        AuthenticationHeaderPolicyV1::from_credential_transport(&self.credential_transport)
    }
}

/// OpenRouter driver with private SDK client state.
pub struct OpenRouterDriver {
    kind_id: ProviderKindId,
    model_id: String,
    /// The exact profile this driver was built for: every construction path is
    /// profile-bound, so a health probe always names its own profile.
    profile_id: ProviderProfileId,
    client: OpenRouterClient,
}

impl std::fmt::Debug for OpenRouterDriver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenRouterDriver")
            .field("kind", &self.kind_id)
            .field("model", &self.model_id)
            .finish_non_exhaustive()
    }
}

impl OpenRouterDriver {
    /// Creates a driver from one exact profile revision and its private credential.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the revision does not select OpenRouter, when
    /// it declares an endpoint this kind cannot override, or when its declared
    /// credential transport cannot be applied.
    pub fn from_profile_revision(
        revision: &ProviderProfileRevisionV1,
        credential: String,
    ) -> DtoResult<Self> {
        let options = OpenRouterDriverOptions::from_profile_revision(revision);
        Self::from_profile_revision_with_options(revision, credential, options)
    }

    /// Creates a driver from one exact profile revision and explicit options.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the revision does not select OpenRouter, when
    /// it declares an endpoint this kind cannot override, when it declares a
    /// textual reasoning history transfer this kind cannot echo, when the
    /// options do not declare the revision's own credential transport, or when
    /// that transport cannot be applied.
    pub fn from_profile_revision_with_options(
        revision: &ProviderProfileRevisionV1,
        credential: String,
        options: OpenRouterDriverOptions,
    ) -> DtoResult<Self> {
        if revision.kind_id().as_str() != descriptor::OPENROUTER_KIND_ID {
            return Err(invalid_provider_config());
        }
        if revision.normalized_effective_endpoint().is_some() {
            return Err(ErrorDto::validation(
                "invalid_openrouter_provider_config",
                "OpenRouter profiles do not permit an endpoint override",
            ));
        }
        // The kind descriptor declares `Disabled` reasoning history transfer:
        // the pinned SDK has no assistant-message reasoning echo field, so a
        // profile that selects textual history is rejected here rather than
        // silently sending a request without the declared history.
        if !revision
            .declared_model_capability_subset()
            .reasoning_input_contract()
            .is_disabled()
        {
            return Err(ErrorDto::validation(
                "openrouter_reasoning_history_unsupported",
                "the OpenRouter adapter cannot transfer textual reasoning history",
            ));
        }
        if options.credential_transport() != revision.credential_transport() {
            return Err(ErrorDto::validation(
                "openrouter_credential_transport_mismatch",
                "the applied options must declare the exact revision credential transport",
            ));
        }
        let policy = options.authentication_header_policy();
        let client = build_client(credential, &policy)?;
        Ok(Self {
            kind_id: revision.kind_id().clone(),
            model_id: revision.model_id().to_owned(),
            profile_id: revision.profile_id().clone(),
            client,
        })
    }
}

/// The validation failure for material that does not select this adapter's kind.
fn invalid_provider_config() -> ErrorDto {
    ErrorDto::validation(
        "invalid_openrouter_provider_config",
        "OpenRouter driver requires OpenRouter provider configuration",
    )
}

/// Builds the private SDK client for one applied header policy.
///
/// The pinned `openrouter-rs` 0.18 client owns request authentication: it
/// always applies bearer authorization and refuses to build without an API key,
/// and the only caller-supplied client seam, a custom `reqwest::Client`, can
/// add default headers but cannot replace that header. A safe-header transport
/// therefore cannot be applied and fails closed here.
fn build_client(
    credential: String,
    policy: &AuthenticationHeaderPolicyV1,
) -> DtoResult<OpenRouterClient> {
    match policy.transport() {
        CredentialTransportModeDto::Bearer => {}
        CredentialTransportModeDto::SafeHeader => {
            return Err(ErrorDto::validation(
                "openrouter_credential_transport_unsupported",
                "the OpenRouter adapter cannot apply a safe-header credential transport",
            ));
        }
    }
    OpenRouterClient::builder()
        .api_key(credential)
        .build()
        .map_err(|_| {
            ErrorDto::unavailable(
                "openrouter_client_unavailable",
                "OpenRouter client could not be configured",
            )
        })
}

/// The fixed failure for one model listing request that did not complete.
fn model_listing_failed() -> ErrorDto {
    ErrorDto::unavailable(
        "openrouter_model_listing_failed",
        "the OpenRouter model listing request did not complete",
    )
}

/// Maps one SDK failure onto the closed provider health reason vocabulary.
///
/// The pinned SDK's request error carries no structured timeout flag, so any
/// transport failure reports the endpoint as unreachable; a missing key is the
/// one credential reason, and every answered-but-unusable response is a
/// provider rejection. No native error text crosses this mapping.
const fn health_reason(error: &OpenRouterError) -> ProviderHealthReasonDto {
    match error {
        OpenRouterError::KeyNotConfigured => ProviderHealthReasonDto::CredentialNotConfigured,
        OpenRouterError::HttpRequest(_) => ProviderHealthReasonDto::EndpointUnreachable,
        _ => ProviderHealthReasonDto::ProviderRejected,
    }
}

/// Maps one SDK model listing response onto validated discovered records.
///
/// The SDK's model name is presentation text, not identity: a blank name is
/// omitted instead of failing the whole listing, and the exact model slug is
/// the discovered identity.
///
/// # Errors
///
/// Returns a validation error when a discovered model identity is not a
/// representable record.
fn model_records(models: &[OpenRouterModel]) -> DtoResult<Vec<ProviderModelRecordDto>> {
    models
        .iter()
        .map(|model| {
            let display_name = (!model.name.trim().is_empty()).then(|| model.name.clone());
            ProviderModelRecordDto::new(model.id.clone(), display_name)
        })
        .collect()
}

impl ModelExecutionDriver for OpenRouterDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        descriptor::driver_capabilities(&self.kind_id)
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
                    Err(mapping::fixed_error(OPENROUTER_REQUEST_REJECTED))
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

    /// Probes the provider through one SDK model listing request.
    ///
    /// The listing is the adapter's cheapest authenticated reachability check:
    /// it names no model and changes no state, and one answered request yields
    /// `Available`. Health evidence is reported, never raised, so a provider
    /// that rejects or never answers the request keeps its exact closed reason.
    fn health_probe(&self) -> ProviderProbeFuture<ProviderHealthEvidenceDto> {
        let client = self.client.clone();
        let profile_id = self.profile_id.clone();
        Box::pin(async move {
            match client.models().list().await {
                Ok(_) => ProviderHealthEvidenceDto::new(
                    profile_id,
                    ProviderHealthStateDto::Available,
                    None,
                ),
                Err(error) => ProviderHealthEvidenceDto::new(
                    profile_id,
                    ProviderHealthStateDto::Unavailable,
                    Some(health_reason(&error)),
                ),
            }
        })
    }

    /// Lists the provider's models through the SDK model listing request.
    fn list_models(&self) -> ProviderProbeFuture<Vec<ProviderModelRecordDto>> {
        let client = self.client.clone();
        Box::pin(async move {
            let models = client
                .models()
                .list()
                .await
                .map_err(|_| model_listing_failed())?;
            model_records(&models)
        })
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
            StreamEvent::ReasoningDelta(content) => {
                events.push_reasoning(ModelEventDto::reasoning_delta(
                    ReasoningFragmentCategoryDto::Primary,
                    content,
                ));
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
            // The pinned SDK's reasoning-details entries carry a typed
            // representation union: textual blocks that duplicate the primary
            // reasoning channel above, summaries, and encrypted or opaque
            // blocks the closed normalized categories cannot represent. This
            // adapter therefore keeps the event unconsumed rather than
            // publishing a raw or synthesized category; a future descriptor
            // that declares `reasoning_details[].text` as its own closed
            // representation owns that mapping.
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
/// intentional no-op rationale for the provider-neutral attachment. An attached
/// cross-turn reasoning history is therefore rejected instead of dropped: the
/// kind descriptor declares `Disabled` transfer, so a request carrying history
/// contradicts the descriptor rather than exercising an unreachable path.
///
/// # Errors
///
/// Returns a validation error when the message carries attached reasoning
/// history or when a tool-call message cannot be mapped.
fn translate_assistant_message(message: &ModelMessageDto) -> DtoResult<Message> {
    if message.reasoning_history().is_some() {
        return Err(ErrorDto::validation(
            "openrouter_reasoning_history_unsupported",
            "the OpenRouter adapter cannot transfer textual reasoning history",
        ));
    }
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

    /// Builds one structured reasoning-detail block through the SDK decoder.
    fn reasoning_detail(
        block_type: &str,
        text: Option<&str>,
    ) -> openrouter_rs::types::ReasoningDetail {
        serde_json::from_value(serde_json::json!({
            "type": block_type,
            "text": text,
        }))
        .expect("private SDK reasoning-detail fixture decodes")
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
                Ok(ModelEventDto::reasoning_delta(
                    ReasoningFragmentCategoryDto::Primary,
                    "because"
                )
                .expect("valid reasoning")),
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
            Some(Err(error)) if error.code() == "provider_reasoning_stream_invalid"
        ));

        let oversized_reasoning = collect_events(vec![StreamEvent::ReasoningDelta(
            "a".repeat(crate::model::MAX_MODEL_REASONING_FRAGMENT_BYTES + 1),
        )]);
        assert!(matches!(
            oversized_reasoning.last(),
            Some(Err(error)) if error.code() == "provider_reasoning_fragment_too_large"
        ));
        assert!(
            !oversized_reasoning.iter().any(|event| matches!(
                event,
                Ok(ModelEventDto::ReasoningDelta { .. })
                    | Ok(ModelEventDto::ReasoningSummaryDelta { .. })
            )),
            "an over-bound native fragment is rejected whole, never truncated"
        );

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

    /// Pins the documented decision that reasoning-details blocks stay unconsumed.
    ///
    /// A detail block carries an open `type` string plus text, data, summary,
    /// signature, and server-tool fields; textual blocks duplicate the primary
    /// reasoning channel this adapter already normalizes, and encrypted or
    /// opaque blocks have no closed category. Publishing either raw or under a
    /// synthesized category would violate the closed normalized surface, so the
    /// adapter emits no fact for the event.
    #[test]
    fn reasoning_details_blocks_publish_no_fact_and_no_raw_content() {
        let details = collect_events(vec![StreamEvent::ReasoningDetailsDelta(vec![
            reasoning_detail("reasoning.text", Some("detailed reasoning")),
            reasoning_detail("reasoning.encrypted", None),
        ])]);
        assert_eq!(details.len(), 2);
        assert_eq!(details[0], Ok(ModelEventDto::started()));
        assert!(matches!(
            details.last(),
            Some(Err(error)) if error.code() == "openrouter_stream_incomplete"
        ));
        assert!(
            !serde_json::to_string(&details)
                .expect("normalized events serialize")
                .contains("detailed reasoning"),
            "no raw reasoning-detail payload reaches the normalized surface"
        );
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

    /// Pins the reachability argument for the retained request-rejection arm.
    ///
    /// The arm is taken only when `translate_request` fails. Every fallible step
    /// of that translation is guarded by a DTO constructor or by the descriptor
    /// contract: the widest request the adapter's `Disabled` transfer admits
    /// translates, and the one remaining failure, an attached cross-turn
    /// reasoning history, contradicts that descriptor and is rejected twice —
    /// here and at profile construction.
    #[test]
    fn every_admissible_request_translates_so_the_rejection_arm_stays_unreachable() {
        let message = ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid");
        let call = ToolCallDto::new(ToolCallId::new(), "read", r#"{"path":"hello.txt"}"#)
            .expect("fixture call is valid");
        let definition = ModelToolDefinitionDto::new(
            "read",
            "Read a workspace file",
            r#"{"type":"object","properties":{"path":{"type":"string"}}}"#,
        )
        .expect("tool definition is valid");
        let widest = ModelRequestDto::new(
            RunId::new(),
            "fixture-model",
            vec![
                ModelMessageDto::new(ModelRoleDto::System, "instructions")
                    .expect("message is valid"),
                message.clone(),
                ModelMessageDto::assistant_tool_calls(
                    Some("before".to_owned()),
                    vec![call.clone()],
                )
                .expect("message is valid"),
                ModelMessageDto::tool_result(call.call_id(), "hello world")
                    .expect("message is valid"),
            ],
            Some("instructions".to_owned()),
        )
        .expect("request is valid")
        .with_tools(vec![definition])
        .expect("tool advertisement is valid");

        assert!(
            translate_request(&widest).is_ok(),
            "every admissible request must translate"
        );

        // A tool-role message always carries its identity, so the tool-result
        // step cannot fail; tool parameters and the request identity are
        // validated where they are built, so the remaining steps cannot fail.
        assert!(ModelMessageDto::new(ModelRoleDto::Tool, "result").is_err());
        assert!(ModelToolDefinitionDto::new("read", "Read a file", "not-json").is_err());
        assert!(ModelRequestDto::new(RunId::new(), " ", vec![message], None).is_err());
        assert!(ModelRequestDto::new(RunId::new(), "fixture-model", Vec::new(), None).is_err());

        let error = crate::mapping::fixed_error(OPENROUTER_REQUEST_REJECTED);
        assert_eq!(error.code(), "openrouter_request_rejected");
        assert_eq!(error.retry(), intention_proto::ErrorRetryDto::Never);
    }

    #[test]
    fn cross_turn_history_is_rejected_instead_of_dropped() {
        let history = crate::model::AssistantReasoningHistoryDto::new(
            "generic-chat-reasoning-content-v1",
            vec![(ReasoningFragmentCategoryDto::Primary, "prior".to_owned())],
            Vec::new(),
        )
        .expect("fixture history is valid");
        let message = ModelMessageDto::assistant_with_reasoning_history("answer", history)
            .expect("assistant history message is valid");
        let request = ModelRequestDto::new(RunId::new(), "fixture-model", vec![message], None)
            .expect("request is valid");
        assert_eq!(
            translate_request(&request)
                .expect_err("the pinned SDK cannot echo prior reasoning")
                .code(),
            "openrouter_reasoning_history_unsupported"
        );
    }

    /// Builds one discovered model record through the SDK decoder.
    fn discovered_model(id: &str, name: &str) -> OpenRouterModel {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": name,
            "created": 1.0,
            "architecture": {
                "modality": "text->text",
                "tokenizer": "fixture",
                "instruct_type": "chat",
                "input_modalities": ["text"],
                "output_modalities": ["text"],
            },
            "top_provider": {
                "context_length": 4096.0,
                "max_completion_tokens": 1024.0,
                "is_moderated": false,
            },
            "pricing": {
                "prompt": "0",
                "completion": "0",
                "image": "0",
                "request": "0",
                "input_cache_read": "0",
                "input_cache_write": "0",
                "web_search": "0",
                "internal_reasoning": "0",
            },
        }))
        .expect("private SDK model fixture decodes")
    }

    #[test]
    fn model_listing_records_keep_the_slug_and_omit_a_blank_name() {
        let models = vec![
            discovered_model("openrouter/fixture", "Fixture Model"),
            discovered_model("openrouter/blank-name", "   "),
            discovered_model("openrouter/empty-name", ""),
        ];
        let records = model_records(&models).expect("fixture listing maps");
        assert_eq!(records[0].model_id(), "openrouter/fixture");
        assert_eq!(records[0].display_name(), Some("Fixture Model"));
        assert!(
            records[1].display_name().is_none() && records[2].display_name().is_none(),
            "a blank SDK name is presentation noise, not a discovered display name"
        );
        assert_eq!(records[2].model_id(), "openrouter/empty-name");
    }

    #[test]
    fn probe_failures_map_onto_the_closed_health_reasons() {
        assert_eq!(
            health_reason(&OpenRouterError::KeyNotConfigured),
            ProviderHealthReasonDto::CredentialNotConfigured
        );
        assert_eq!(
            health_reason(&api_error(http::StatusCode::UNAUTHORIZED)),
            ProviderHealthReasonDto::ProviderRejected
        );
        assert_eq!(
            health_reason(&OpenRouterError::HttpRequest(
                openrouter_rs::error::HttpRequestError::new("fixture transport failure")
            )),
            ProviderHealthReasonDto::EndpointUnreachable
        );
    }
}
