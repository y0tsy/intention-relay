//! Generic OpenAI Chat Completions provider normalization.
//!
//! `async-openai` remains a private implementation dependency. This adapter
//! retains SDK-owned SSE parsing and translates only its parsed stream values
//! into provider-neutral model events.

use std::collections::BTreeMap;

use crate::auth::AuthenticationHeaderPolicyV1;
use crate::descriptor;
use crate::mapping;
use crate::mapping::WireRole;
use crate::model::{
    AssistantReasoningHistoryDto, FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto,
    ModelEventDto, ModelEventStream, ModelExecutionDriver, ModelMessageDto, ModelRequestDto,
    ProviderErrorDto, ProviderHealthEvidenceDto, ProviderHealthReasonDto, ProviderHealthStateDto,
    ProviderModelRecordDto, ProviderProbeFuture, ReasoningFragmentCategoryDto, ToolCallDto,
};
use crate::stream::{EventTranslator, NormalizedEvents, normalized_stream};
use async_openai::{
    Client,
    config::OpenAIConfig,
    error::OpenAIError,
    types::{
        chat::{
            ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls,
            ChatCompletionStreamOptions, ChatCompletionTool, ChatCompletionTools, FunctionCall,
            FunctionObject,
        },
        models::ListModelResponse,
    },
};
use futures_util::{StreamExt, stream};
use intention_proto::provider::{
    CredentialTransportDto, CredentialTransportModeDto, ProviderKindId, ProviderProfileId,
    ProviderProfileRevisionV1,
};
use intention_proto::{DtoResult, ErrorDto, ToolCallId};

mod wire;

use wire::{WireCacheControl, WireChunk, WireDelta, WireMessage, WireRequest};

/// The fixed non-retryable failure for a request this adapter cannot translate.
const GENERIC_CHAT_REQUEST_REJECTED: &str = "generic_chat_request_rejected";

/// The closed driver options of the generic Chat Completions adapter.
///
/// The one declared option is the exact profile's credential transport. The
/// adapter applies it at construction: a declaration it cannot apply fails
/// closed instead of being silently defaulted or ignored.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenericChatDriverOptions {
    credential_transport: CredentialTransportDto,
}

impl GenericChatDriverOptions {
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

/// Generic Chat Completions driver with private SDK client state.
pub struct GenericChatDriver {
    kind_id: ProviderKindId,
    model_id: String,
    /// The exact profile this driver was built for: every construction path is
    /// profile-bound, so a health probe always names its own profile.
    profile_id: ProviderProfileId,
    client: Client<OpenAIConfig>,
}

impl std::fmt::Debug for GenericChatDriver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GenericChatDriver")
            .field("kind", &self.kind_id)
            .field("model", &self.model_id)
            .finish_non_exhaustive()
    }
}

impl GenericChatDriver {
    /// Creates a driver from one exact profile revision and its private credential.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the revision does not select generic Chat
    /// Completions, when it declares no endpoint, or when its declared
    /// credential transport cannot be applied.
    pub fn from_profile_revision(
        revision: &ProviderProfileRevisionV1,
        credential: String,
    ) -> DtoResult<Self> {
        let options = GenericChatDriverOptions::from_profile_revision(revision);
        Self::from_profile_revision_with_options(revision, credential, options)
    }

    /// Creates a driver from one exact profile revision and explicit options.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the revision does not select generic Chat
    /// Completions, when it declares no endpoint, when the options do not
    /// declare the revision's own credential transport, or when that transport
    /// cannot be applied.
    pub fn from_profile_revision_with_options(
        revision: &ProviderProfileRevisionV1,
        credential: String,
        options: GenericChatDriverOptions,
    ) -> DtoResult<Self> {
        if revision.kind_id().as_str() != descriptor::GENERIC_CHAT_KIND_ID {
            return Err(invalid_provider_config());
        }
        let endpoint = revision
            .normalized_effective_endpoint()
            .ok_or_else(missing_endpoint_error)?;
        if options.credential_transport() != revision.credential_transport() {
            return Err(ErrorDto::validation(
                "generic_chat_credential_transport_mismatch",
                "the applied options must declare the exact revision credential transport",
            ));
        }
        let policy = options.authentication_header_policy();
        let client = build_client(endpoint, credential, &policy)?;
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
        "invalid_generic_chat_provider_config",
        "generic chat driver requires generic chat provider configuration",
    )
}

/// The validation failure for a profile that declares no endpoint.
fn missing_endpoint_error() -> ErrorDto {
    ErrorDto::validation(
        "missing_generic_chat_endpoint",
        "generic chat provider requires a configured endpoint",
    )
}

/// Builds the private SDK client for one applied header policy.
///
/// The pinned `async-openai` 0.42 config merges `with_header` custom headers
/// into every request yet always emits its own bearer authorization over them,
/// so nothing it exposes can replace the bearer header. A safe-header transport
/// therefore cannot be applied and fails closed here.
fn build_client(
    endpoint: &str,
    credential: String,
    policy: &AuthenticationHeaderPolicyV1,
) -> DtoResult<Client<OpenAIConfig>> {
    match policy.transport() {
        CredentialTransportModeDto::Bearer => {}
        CredentialTransportModeDto::SafeHeader => {
            return Err(ErrorDto::validation(
                "generic_chat_credential_transport_unsupported",
                "the generic chat adapter cannot apply a safe-header credential transport",
            ));
        }
    }
    Ok(Client::with_config(
        OpenAIConfig::new()
            .with_api_base(endpoint)
            .with_api_key(credential),
    ))
}

/// The fixed failure for one model listing request that did not complete.
fn model_listing_failed() -> ErrorDto {
    ErrorDto::unavailable(
        "generic_chat_model_listing_failed",
        "the generic chat model listing request did not complete",
    )
}

/// Maps one SDK failure onto the closed provider health reason vocabulary.
///
/// A transport failure is a repeated-request problem only when the transport
/// itself reported a timeout; every answered-but-unusable response is a provider
/// rejection. No native error text crosses this mapping.
fn health_reason(error: &OpenAIError) -> ProviderHealthReasonDto {
    match error {
        OpenAIError::Reqwest(error) if error.is_timeout() => ProviderHealthReasonDto::TimedOut,
        OpenAIError::Reqwest(_) => ProviderHealthReasonDto::EndpointUnreachable,
        _ => ProviderHealthReasonDto::ProviderRejected,
    }
}

/// Maps one SDK model listing response onto validated discovered records.
///
/// # Errors
///
/// Returns a validation error when a discovered model identity or name is not a
/// representable record.
fn model_records(response: &ListModelResponse) -> DtoResult<Vec<ProviderModelRecordDto>> {
    response
        .data
        .iter()
        .map(|model| ProviderModelRecordDto::new(model.id.clone(), None))
        .collect()
}

impl ModelExecutionDriver for GenericChatDriver {
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
                    Err(mapping::fixed_error(GENERIC_CHAT_REQUEST_REJECTED))
                }));
            }
        };
        let client = self.client.clone();
        Box::pin(
            stream::once(async move {
                client
                    .chat()
                    .create_stream_byot::<WireRequest, WireChunk>(native_request)
                    .await
                    .map_or_else(
                        |error| {
                            Box::pin(stream::once(async move { Err(map_openai_error(&error)) }))
                                as ModelEventStream
                        },
                        |native| normalized_stream(native, GenericTranslator::default()),
                    )
            })
            .flatten(),
        )
    }

    /// Probes the provider through one `GET {base}/models` request.
    ///
    /// The listing is the generic adapter's cheapest authenticated reachability
    /// check: it names no model and changes no state, and one answered request
    /// yields `Available`. Health evidence is reported, never raised, so a
    /// provider that rejects or never answers the request keeps its exact closed
    /// reason.
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

    /// Lists the provider's models through one `GET {base}/models` request.
    fn list_models(&self) -> ProviderProbeFuture<Vec<ProviderModelRecordDto>> {
        let client = self.client.clone();
        Box::pin(async move {
            let response = client
                .models()
                .list()
                .await
                .map_err(|_| model_listing_failed())?;
            model_records(&response)
        })
    }
}

/// Translates the generic adapter's native chunks into normalized events.
#[derive(Default)]
struct GenericTranslator {
    tools: BTreeMap<(u32, u32), FunctionToolFragments>,
    terminal_reason: Option<FinishReasonDto>,
    usage_reported: bool,
    reasoning_presence_reported: bool,
}

impl EventTranslator for GenericTranslator {
    type Item = Result<WireChunk, OpenAIError>;

    fn translate_item(&mut self, item: Self::Item, events: &mut NormalizedEvents<'_>) {
        match item {
            Ok(chunk) => self.accept_chunk(chunk, events),
            Err(error) => events.fail_error(map_openai_error(&error)),
        }
    }

    /// A recorded finish reason is terminal only once the native stream ends:
    /// standard streams deliver a trailing usage-only chunk after the
    /// finish-reason chunk, and the driver requested usage inclusion.
    /// Recording the reason and continuing to poll collects that usage
    /// exactly once (PR24-009). An absent reason fails the incomplete stream.
    fn native_ended(&mut self, events: &mut NormalizedEvents<'_>) {
        if let Some(reason) = self.terminal_reason.take() {
            self.finish(reason, events);
        } else {
            events.fail("generic_chat_stream_incomplete");
        }
    }
}

impl GenericTranslator {
    fn accept_chunk(&mut self, chunk: WireChunk, events: &mut NormalizedEvents<'_>) {
        let post_finish = self.terminal_reason.is_some();
        if post_finish
            && chunk.choices.iter().any(|choice| {
                choice.finish_reason.is_some()
                    || choice
                        .delta
                        .content
                        .as_deref()
                        .is_some_and(|content| !content.is_empty())
                    || choice
                        .delta
                        .reasoning_content
                        .as_deref()
                        .is_some_and(|reasoning| !reasoning.is_empty())
                    || choice.delta.tool_calls.is_some()
            })
        {
            // Chunks after the finish reason may carry at most the final
            // usage summary; any further content, reasoning fragment, tool
            // fragment, or finish is malformed and fails the stream.
            if chunk
                .choices
                .iter()
                .any(|choice| choice.finish_reason.is_some())
            {
                events.fail("generic_chat_duplicate_finish");
            } else {
                events.fail("generic_chat_post_finish_content");
            }
            return;
        }
        // A gateway may repeat the usage summary across chunks. The first
        // summary is authoritative and a repeat is dropped instead of failing
        // the stream, so a live provider that reports usage more than once on
        // one response still completes.
        if let Some(usage) = chunk.usage.filter(|_| !self.usage_reported) {
            match mapping::reported_usage(
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.total_tokens,
            ) {
                Ok(usage) => {
                    self.usage_reported = true;
                    events.push(ModelEventDto::usage(usage));
                }
                // A rejected usage total is terminal, so the shared sink drops
                // everything the rest of this chunk would queue.
                Err(_) => events.fail("generic_chat_invalid_usage"),
            }
        }
        if post_finish {
            return;
        }
        for choice in chunk.choices {
            if self
                .accept_delta(choice.index, choice.delta, events)
                .is_err()
            {
                events.fail("generic_chat_invalid_tool_call");
                return;
            }
            if let Some(reason) = choice.finish_reason
                && self
                    .terminal_reason
                    .replace(finish_reason(&reason))
                    .is_some()
            {
                events.fail("generic_chat_duplicate_finish");
                return;
            }
        }
    }

    fn accept_delta(
        &mut self,
        choice_index: u32,
        delta: WireDelta,
        events: &mut NormalizedEvents<'_>,
    ) -> Result<(), ()> {
        // The provider streams the thinking channel before the answer it
        // informs, so a delta carrying both keeps that order.
        if let Some(reasoning) = delta.reasoning_content {
            self.accept_reasoning(reasoning, events);
        }
        if let Some(content) = delta.content.filter(|content| !content.is_empty()) {
            events.push(ModelEventDto::text_delta(content).map_err(|_| ())?);
        }
        if let Some(calls) = delta.tool_calls {
            for call in calls {
                let fragments = self.tools.entry((choice_index, call.index)).or_default();
                fragments.merge(call.id, call.function)?;
            }
        }
        Ok(())
    }

    /// Normalizes one reasoning fragment of a provider delta.
    ///
    /// An empty value marks that the provider carried the reasoning channel
    /// with no text. Providers repeat that empty value on nearly every chunk,
    /// so it becomes at most one textless presence event per stream and
    /// creates no fact; the continuation request must still send the channel
    /// back beside the assistant tool calls, because a provider in thinking
    /// mode rejects a request whose assistant message omits it. A
    /// non-empty fragment is normalized as the closed primary category and
    /// stays a transient reasoning fact: it is never appended to assistant
    /// text, never becomes message content, and never enters an error payload.
    /// A value the closed normalized surface cannot represent, or one that
    /// exceeds the per-fragment representation bound, fails the stream with
    /// the closed reasoning failure through the normalization sink instead of
    /// publishing raw, partial, or oversized reasoning.
    fn accept_reasoning(&mut self, reasoning: String, events: &mut NormalizedEvents<'_>) {
        if reasoning.is_empty() {
            if !self.reasoning_presence_reported {
                self.reasoning_presence_reported = true;
                events.push_reasoning(Ok(ModelEventDto::reasoning_presence()));
            }
            return;
        }
        events.push_reasoning(ModelEventDto::reasoning_delta(
            ReasoningFragmentCategoryDto::Primary,
            reasoning,
        ));
    }

    fn finish(&mut self, reason: FinishReasonDto, events: &mut NormalizedEvents<'_>) {
        match std::mem::take(&mut self.tools)
            .into_values()
            .map(FunctionToolFragments::finish)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(calls) => {
                for call in calls {
                    events.push(ModelEventDto::tool_call(call));
                }
                events.finish(reason);
            }
            Err(()) => events.fail("generic_chat_invalid_tool_call"),
        }
    }
}

#[derive(Default)]
struct FunctionToolFragments {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl FunctionToolFragments {
    fn merge(
        &mut self,
        id: Option<String>,
        function: Option<async_openai::types::chat::FunctionCallStream>,
    ) -> Result<(), ()> {
        merge_constant(&mut self.id, id)?;
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
        mapping::complete_tool_call(&name, &self.arguments).map_err(|_| ())
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

/// Maps one provider finish-reason string onto the closed canonical reason.
///
/// The provider value stays an open string, so every unlisted reason (a
/// vendor-specific value or an extension such as `stop_sequence`) degrades to
/// [`FinishReasonDto::Unknown`] instead of aborting the response.
fn finish_reason(reason: &str) -> FinishReasonDto {
    match reason {
        "stop" => FinishReasonDto::Stop,
        "length" => FinishReasonDto::Length,
        "tool_calls" => FinishReasonDto::ToolCalls,
        "content_filter" => FinishReasonDto::ContentFilter,
        "error" => FinishReasonDto::Error,
        _ => FinishReasonDto::Unknown,
    }
}

fn map_openai_error(error: &OpenAIError) -> ProviderErrorDto {
    let retryable = match error {
        OpenAIError::Reqwest(_) | OpenAIError::StreamError(_) => true,
        OpenAIError::ApiError(error) => api_error_retryable(error),
        OpenAIError::JSONDeserialize(..)
        | OpenAIError::FileSaveError(_)
        | OpenAIError::FileReadError(_)
        | OpenAIError::InvalidArgument(_) => false,
    };
    mapping::provider_error(
        "generic_chat_provider_unavailable",
        "generic_chat_provider_request_rejected",
        retryable,
    )
}

/// Classifies one SDK API error by its authoritative HTTP status.
///
/// Throttling (429) and server faults (5xx) are transient; every other client
/// rejection (4xx) is permanent, so a gateway that returns a permanent status
/// without a `type` field is never retried to the attempt maximum. The
/// provider's optional `type` string stays a secondary signal: it decides only
/// a status this taxonomy does not classify by itself, and an unclassified
/// status without a transient type stays permanent.
fn api_error_retryable(error: &async_openai::error::ApiErrorResponse) -> bool {
    match error.status_code.as_u16() {
        429 | 500..=599 => true,
        400..=499 => false,
        _ => matches!(
            error.api_error.r#type.as_deref(),
            Some("rate_limit_exceeded" | "server_error")
        ),
    }
}

/// Translates one provider-neutral request into the exact wire request.
///
/// The daemon-owned system context always closes with a prompt-cache marker:
/// it is the stable instruction block that every round of the attempt repeats.
/// A provider-neutral message carries its own marker when the runtime's context
/// window pass closed the stable window prefix on it.
///
/// # Errors
///
/// Returns a validation error for an undecodable tool-parameter schema or a
/// malformed tool-role message.
///
/// The BYOT request path has no SDK builder left to fail on, so the former
/// `build` failure branch is gone rather than reproduced.
fn translate_request(request: &ModelRequestDto) -> DtoResult<WireRequest> {
    let attachments = reasoning_attachments(request);
    let mut messages = Vec::new();
    if let Some(context) = request.system_context() {
        messages.push(WireMessage::System {
            content: context.to_owned(),
            cache_control: Some(WireCacheControl::ephemeral()),
        });
    }
    for message in request.messages() {
        messages.push(translate_message(message, &attachments)?);
    }
    let tools: Vec<ChatCompletionTools> = request
        .tools()
        .iter()
        .map(|tool| -> DtoResult<ChatCompletionTools> {
            Ok(ChatCompletionTools::Function(ChatCompletionTool {
                function: FunctionObject {
                    name: tool.name().to_owned(),
                    description: Some(tool.description().to_owned()),
                    parameters: Some(mapping::decode_parameters(
                        tool.parameters_json(),
                        "invalid_generic_chat_request",
                        "generic chat tool parameters could not be decoded",
                    )?),
                    strict: None,
                },
            }))
        })
        .collect::<DtoResult<_>>()?;
    Ok(WireRequest {
        model: request.model().to_owned(),
        messages,
        stream: true,
        stream_options: ChatCompletionStreamOptions {
            include_usage: Some(true),
            include_obfuscation: None,
        },
        tools,
    })
}

/// Maps every attached tool-call identity to the reasoning text that must
/// round-trip with the assistant tool-call message carrying that identity.
///
/// Attachments are transient same-run material: they are matched by identity,
/// never merged into message content, and never consulted for other roles.
fn reasoning_attachments(request: &ModelRequestDto) -> BTreeMap<ToolCallId, &str> {
    let mut attachments = BTreeMap::new();
    for attachment in request.assistant_reasoning() {
        for call_id in attachment.tool_call_ids() {
            attachments.insert(*call_id, attachment.text());
        }
    }
    attachments
}

fn translate_message(
    message: &ModelMessageDto,
    attachments: &BTreeMap<ToolCallId, &str>,
) -> DtoResult<WireMessage> {
    let cache_control = message.cache_control().then(WireCacheControl::ephemeral);
    match mapping::wire_role(message.role()) {
        WireRole::System => Ok(WireMessage::System {
            content: message.content().to_owned(),
            cache_control,
        }),
        WireRole::User => Ok(WireMessage::User {
            content: message.content().to_owned(),
            cache_control,
        }),
        WireRole::Assistant => translate_assistant_message(message, attachments),
        WireRole::Tool => {
            let tool_call_id =
                mapping::tool_result_identity(message, "invalid_generic_chat_request")?;
            Ok(WireMessage::Tool {
                content: message.content().to_owned(),
                tool_call_id: tool_call_id.to_string(),
                cache_control,
            })
        }
    }
}

fn translate_assistant_message(
    message: &ModelMessageDto,
    attachments: &BTreeMap<ToolCallId, &str>,
) -> DtoResult<WireMessage> {
    // The assistant content is optional when tool calls are present, so an
    // empty DTO content stays omitted on the wire.
    let content = (!message.content().is_empty()).then(|| message.content().to_owned());
    let cache_control = message.cache_control().then(WireCacheControl::ephemeral);
    let tool_calls = message.tool_calls();
    // Cross-turn history and the same-run attachment both feed the one native
    // reasoning field beside the assistant text. Attached history is the
    // durable form of the same channel, so it wins when both are present.
    let reasoning_content = message.reasoning_history().map_or_else(
        || {
            tool_calls
                .and_then(|calls| {
                    calls
                        .iter()
                        .find_map(|call| attachments.get(&call.call_id()).copied())
                })
                .map(str::to_owned)
        },
        |history| Some(reasoning_history_text(history)),
    );
    let Some(tool_calls) = tool_calls else {
        return Ok(WireMessage::Assistant {
            content,
            tool_calls: None,
            reasoning_content,
            cache_control,
        });
    };
    Ok(WireMessage::Assistant {
        content,
        tool_calls: Some(
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
                .collect(),
        ),
        reasoning_content,
        cache_control,
    })
}

/// Renders one attached cross-turn reasoning history as the native field text.
///
/// Fragments keep their recorded stream order and the summaries follow at the
/// tail, so the concatenation reconstructs the response's whole reasoning text
/// under the descriptor's compatibility identity. The text is a provider field
/// value only: message content never absorbs it.
fn reasoning_history_text(history: &AssistantReasoningHistoryDto) -> String {
    let mut text = String::new();
    for (_, content) in history.fragments() {
        text.push_str(content);
    }
    for summary in history.summaries() {
        text.push_str(summary);
    }
    text
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "Private tool-fragment fixtures use expect to provide precise test failure messages."
)]
mod tests {
    use super::*;
    use crate::model::{ModelRoleDto, UsageDto};
    use intention_proto::{ErrorRetryDto, RunId};

    /// Collects the shared stream's events for one fixed native chunk sequence.
    fn collect_chunks(
        chunks: Vec<Result<WireChunk, OpenAIError>>,
    ) -> Vec<Result<ModelEventDto, ProviderErrorDto>> {
        futures_executor::block_on(
            normalized_stream(stream::iter(chunks), GenericTranslator::default())
                .collect::<Vec<_>>(),
        )
    }

    fn usage() -> async_openai::types::chat::CompletionUsage {
        async_openai::types::chat::CompletionUsage {
            prompt_tokens: 2,
            completion_tokens: 3,
            total_tokens: 5,
            prompt_tokens_details: None,
            completion_tokens_details: None,
        }
    }

    #[test]
    fn notice_role_translates_as_a_user_message_with_its_text_unchanged() {
        let request = ModelRequestDto::new(
            RunId::new(),
            "fixture",
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
            wire["messages"][0],
            serde_json::json!({
                "role": "user",
                "content": "[The tool call \"read\" did not receive a final result.]",
            })
        );
    }

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
        )
        .expect("request is valid");

        let wire = serde_json::to_value(translate_request(&request).expect("request translates"))
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
                "stream": true,
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
        )
        .expect("request is valid");
        let wire = serde_json::to_value(translate_request(&follow_up).expect("request translates"))
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
    fn cache_markers_close_the_system_block_and_the_stable_prefix() {
        let call = ToolCallDto::new(ToolCallId::new(), "read", r#"{"path":"hello.txt"}"#)
            .expect("fixture call is valid");
        let mut result =
            ModelMessageDto::tool_result(call.call_id(), "hello world").expect("message is valid");
        result.set_cache_control(true);
        let request = ModelRequestDto::new(
            RunId::new(),
            "fixture",
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
            wire["messages"][0],
            serde_json::json!({
                "role": "system",
                "content": "instructions",
                "cache_control": {"type": "ephemeral"},
            })
        );
        assert!(wire["messages"][1].get("cache_control").is_none());
        assert!(wire["messages"][2].get("cache_control").is_none());
        assert_eq!(
            wire["messages"][3]["cache_control"],
            serde_json::json!({"type": "ephemeral"}),
            "the flagged message closes the stable window prefix"
        );
    }

    #[test]
    fn generic_chat_advertises_tool_definitions_without_tool_choice() {
        let request = ModelRequestDto::new(
            RunId::new(),
            "fixture",
            vec![ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid")],
            None,
        )
        .expect("request is valid")
        .with_tools(vec![
            crate::model::ModelToolDefinitionDto::new(
                "read_file",
                "Reads one file",
                r#"{"type":"object","properties":{"path":{"type":"string"}}}"#,
            )
            .expect("tool is valid"),
            crate::model::ModelToolDefinitionDto::new(
                "search",
                "Searches files",
                r#"{"type":"object","required":["query"]}"#,
            )
            .expect("tool is valid"),
        ])
        .expect("tools are valid");

        let wire = serde_json::to_value(translate_request(&request).expect("request translates"))
            .expect("request serializes");
        assert_eq!(
            wire,
            serde_json::json!({
                "model": "fixture",
                "messages": [{"role": "user", "content": "hello"}],
                "stream": true,
                "stream_options": {"include_usage": true},
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "read_file",
                            "description": "Reads one file",
                            "parameters": {
                                "type": "object",
                                "properties": {"path": {"type": "string"}},
                            },
                        },
                    },
                    {
                        "type": "function",
                        "function": {
                            "name": "search",
                            "description": "Searches files",
                            "parameters": {
                                "type": "object",
                                "required": ["query"],
                            },
                        },
                    },
                ],
            })
        );
        assert!(wire.get("tool_choice").is_none());
    }

    #[test]
    fn generic_chat_omits_tools_when_no_definitions_are_advertised() {
        let request = ModelRequestDto::new(
            RunId::new(),
            "fixture",
            vec![ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid")],
            None,
        )
        .expect("request is valid");

        let wire = serde_json::to_value(translate_request(&request).expect("request translates"))
            .expect("request serializes");
        assert_eq!(
            wire,
            serde_json::json!({
                "model": "fixture",
                "messages": [{"role": "user", "content": "hello"}],
                "stream": true,
                "stream_options": {"include_usage": true},
            })
        );
        assert!(wire.get("tools").is_none());
        assert!(wire.get("tool_choice").is_none());
    }

    #[test]
    fn assistant_reasoning_attachments_round_trip_onto_the_wire() {
        let first = ToolCallDto::new(ToolCallId::new(), "read", r#"{"path":"hello.txt"}"#)
            .expect("fixture call is valid");
        let second = ToolCallDto::new(ToolCallId::new(), "search", r#"{"query":"hello"}"#)
            .expect("fixture call is valid");

        // Without an attachment the assistant message carries no reasoning key.
        let wire = serde_json::to_value(
            translate_request(&assistant_tool_request(&first, &second))
                .expect("request translates"),
        )
        .expect("request serializes");
        assert!(wire["messages"][0].get("reasoning_content").is_none());

        // An attachment matching any of the message's call identities carries
        // its text beside the tool calls.
        let attached = assistant_tool_request(&first, &second)
            .with_assistant_reasoning(vec![
                crate::model::AssistantReasoningDto::new(
                    vec![second.call_id()],
                    "weighing the options",
                )
                .expect("attachment is valid"),
            ])
            .expect("attachment is accepted");
        let wire = serde_json::to_value(translate_request(&attached).expect("request translates"))
            .expect("request serializes");
        assert_eq!(
            wire["messages"][0]["reasoning_content"],
            "weighing the options"
        );
        assert!(wire["messages"][0].get("content").is_none());

        // An empty attachment text is a presence marker the provider needs, so
        // the key stays on the wire.
        let presence = assistant_tool_request(&first, &second)
            .with_assistant_reasoning(vec![
                crate::model::AssistantReasoningDto::new(vec![second.call_id()], "")
                    .expect("presence attachment is valid"),
            ])
            .expect("attachment is accepted");
        let wire = serde_json::to_value(translate_request(&presence).expect("request translates"))
            .expect("request serializes");
        assert_eq!(wire["messages"][0]["reasoning_content"], "");

        // An attachment for an unrelated call never leaks onto this message.
        let unrelated = assistant_tool_request(&first, &second)
            .with_assistant_reasoning(vec![
                crate::model::AssistantReasoningDto::new(vec![ToolCallId::new()], "other call")
                    .expect("attachment is valid"),
            ])
            .expect("attachment is accepted");
        let wire = serde_json::to_value(translate_request(&unrelated).expect("request translates"))
            .expect("request serializes");
        assert!(wire["messages"][0].get("reasoning_content").is_none());
        assert!(
            wire["messages"][0]["tool_calls"][0]
                .get("reasoning_content")
                .is_none()
        );
    }

    #[test]
    fn attached_cross_turn_history_lands_beside_the_assistant_text() {
        let history = crate::model::AssistantReasoningHistoryDto::new(
            "generic-chat-reasoning-content-v1",
            vec![
                (ReasoningFragmentCategoryDto::Primary, "first ".to_owned()),
                (ReasoningFragmentCategoryDto::Detail, "second".to_owned()),
            ],
            vec!["summary".to_owned()],
        )
        .expect("fixture history is valid");
        let message =
            ModelMessageDto::assistant_with_reasoning_history("the answer", history.clone())
                .expect("assistant history message is valid");
        let request = ModelRequestDto::new(
            RunId::new(),
            "fixture",
            vec![
                message.clone(),
                ModelMessageDto::new(ModelRoleDto::User, "next").expect("message is valid"),
            ],
            None,
        )
        .expect("request is valid");

        let wire = serde_json::to_value(translate_request(&request).expect("request translates"))
            .expect("request serializes");
        assert_eq!(wire["messages"][0]["role"], "assistant");
        assert_eq!(wire["messages"][0]["content"], "the answer");
        assert_eq!(
            wire["messages"][0]["reasoning_content"], "first secondsummary",
            "fragments keep their stream order and summaries follow at the tail"
        );
        assert_eq!(
            serde_json::to_value(&message).expect("message serializes")["content"],
            "the answer",
            "prior reasoning never enters the ordinary message text"
        );

        // A tool-call round carrying the same durable history keeps both the
        // history text and the tool calls on one native assistant message.
        let call = ToolCallDto::new(ToolCallId::new(), "read", r#"{"path":"hello.txt"}"#)
            .expect("fixture call is valid");
        let mut tool_message =
            ModelMessageDto::assistant_tool_calls(Some("working".to_owned()), vec![call])
                .expect("tool-call message is valid");
        tool_message
            .attach_reasoning_history(history)
            .expect("assistant messages accept history");
        let request = ModelRequestDto::new(RunId::new(), "fixture", vec![tool_message], None)
            .expect("request is valid");
        let wire = serde_json::to_value(translate_request(&request).expect("request translates"))
            .expect("request serializes");
        assert_eq!(
            wire["messages"][0]["reasoning_content"],
            "first secondsummary"
        );
        assert_eq!(wire["messages"][0]["content"], "working");
        assert!(wire["messages"][0]["tool_calls"].is_array());
    }

    #[test]
    fn model_listing_records_keep_the_exact_discovered_identity() {
        let response = ListModelResponse {
            object: "list".to_owned(),
            data: vec![
                async_openai::types::models::Model {
                    id: "fixture-model".to_owned(),
                    object: "model".to_owned(),
                    created: 1,
                    owned_by: "fixture-owner".to_owned(),
                    shutdown_date: None,
                },
                async_openai::types::models::Model {
                    id: "second-model".to_owned(),
                    object: "model".to_owned(),
                    created: 2,
                    owned_by: "fixture-owner".to_owned(),
                    shutdown_date: None,
                },
            ],
        };
        let records = model_records(&response).expect("fixture listing maps");
        assert_eq!(
            records
                .iter()
                .map(|record| record.model_id().to_owned())
                .collect::<Vec<_>>(),
            vec!["fixture-model".to_owned(), "second-model".to_owned()]
        );
        assert!(
            records.iter().all(|record| record.display_name().is_none()),
            "the SDK exposes no display name for a chat-completions model"
        );

        let invalid = ListModelResponse {
            object: "list".to_owned(),
            data: vec![async_openai::types::models::Model {
                id: "  ".to_owned(),
                object: "model".to_owned(),
                created: 3,
                owned_by: "fixture-owner".to_owned(),
                shutdown_date: None,
            }],
        };
        assert_eq!(
            model_records(&invalid)
                .expect_err("a blank discovered identity is not a record")
                .code(),
            "invalid_provider_model_record"
        );
    }

    #[test]
    fn probe_failures_map_onto_the_closed_health_reasons() {
        assert_eq!(
            health_reason(&api_error(http::StatusCode::UNAUTHORIZED, None)),
            ProviderHealthReasonDto::ProviderRejected
        );
        assert_eq!(
            health_reason(&OpenAIError::InvalidArgument("fixture".to_owned())),
            ProviderHealthReasonDto::ProviderRejected
        );
    }

    /// Pins the reachability argument for the retained request-rejection arm.
    ///
    /// The arm is taken only when `translate_request` fails, and every fallible
    /// step of that translation is guarded by a DTO constructor: the widest
    /// request the public API can build translates, and the states the two
    /// fallible steps would need are rejected where they are built. The arm's
    /// own fixed code and retry guidance are pinned at the constant it uses.
    #[test]
    fn every_constructible_request_translates_so_the_rejection_arm_stays_unreachable() {
        let message = ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid");
        let call = ToolCallDto::new(ToolCallId::new(), "read", r#"{"path":"hello.txt"}"#)
            .expect("fixture call is valid");
        let definition = crate::model::ModelToolDefinitionDto::new(
            "read",
            "Read a workspace file",
            r#"{"type":"object","properties":{"path":{"type":"string"}}}"#,
        )
        .expect("tool definition is valid");
        let attachment =
            crate::model::AssistantReasoningDto::new(vec![call.call_id()], "chain of thought")
                .expect("reasoning attachment is valid");
        let widest = ModelRequestDto::new(
            RunId::new(),
            "fixture",
            vec![
                ModelMessageDto::new(ModelRoleDto::System, "instructions")
                    .expect("message is valid"),
                message.clone(),
                ModelMessageDto::new(ModelRoleDto::Notice, "[The tool call did not finish.]")
                    .expect("notice is valid"),
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
        .expect("tool advertisement is valid")
        .with_assistant_reasoning(vec![attachment])
        .expect("attachment is accepted");

        assert!(
            translate_request(&widest).is_ok(),
            "every request the public API can build must translate"
        );

        // A tool-role message always carries its identity, so the tool-result
        // step cannot fail; tool parameters and the request identity are
        // validated where they are built, so the remaining steps cannot fail.
        assert!(ModelMessageDto::new(ModelRoleDto::Tool, "result").is_err());
        assert!(
            crate::model::ModelToolDefinitionDto::new("read", "Read a file", "not-json").is_err()
        );
        assert!(ModelRequestDto::new(RunId::new(), " ", vec![message], None).is_err());
        assert!(ModelRequestDto::new(RunId::new(), "fixture", Vec::new(), None).is_err());

        let error = crate::mapping::fixed_error(GENERIC_CHAT_REQUEST_REJECTED);
        assert_eq!(error.code(), "generic_chat_request_rejected");
        assert_eq!(error.retry(), ErrorRetryDto::Never);
    }

    fn assistant_tool_request(first: &ToolCallDto, second: &ToolCallDto) -> ModelRequestDto {
        ModelRequestDto::new(
            RunId::new(),
            "fixture",
            vec![
                ModelMessageDto::assistant_tool_calls(None, vec![first.clone(), second.clone()])
                    .expect("message is valid"),
            ],
            None,
        )
        .expect("request is valid")
    }

    #[test]
    fn tool_fragments_accept_split_and_interleaved_calls_only_at_terminal() {
        let mut first = FunctionToolFragments::default();
        let mut second = FunctionToolFragments::default();
        first
            .merge(
                Some("first".to_owned()),
                Some(async_openai::types::chat::FunctionCallStream {
                    name: Some("inspect".to_owned()),
                    arguments: Some("{\"path\"".to_owned()),
                }),
            )
            .expect("first fragment is valid");
        second
            .merge(
                Some("second".to_owned()),
                Some(async_openai::types::chat::FunctionCallStream {
                    name: Some("search".to_owned()),
                    arguments: Some("{\"query\"".to_owned()),
                }),
            )
            .expect("second fragment is valid");
        first
            .merge(
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
                Some(async_openai::types::chat::FunctionCallStream {
                    name: Some("inspect".to_owned()),
                    arguments: Some("{}".to_owned()),
                }),
            )
            .expect("initial fragment is valid");
        assert!(conflicting.merge(Some("other".to_owned()), None).is_err());
        assert!(FunctionToolFragments::default().finish().is_err());
        let mut malformed = FunctionToolFragments::default();
        malformed
            .merge(
                Some("call".to_owned()),
                Some(async_openai::types::chat::FunctionCallStream {
                    name: Some("inspect".to_owned()),
                    arguments: Some("not-json".to_owned()),
                }),
            )
            .expect("fragment shape is valid");
        assert!(malformed.finish().is_err());
    }

    #[test]
    fn native_api_errors_follow_the_http_status_not_the_optional_type() {
        // The gateway status is the authoritative retryability signal:
        // throttling and server faults are transient, every other client
        // rejection is permanent, whatever the provider's optional `type`
        // string says. An untyped 400 must therefore never be retried to the
        // attempt maximum.
        for (status, kind, expected) in [
            (http::StatusCode::BAD_REQUEST, None, ErrorRetryDto::Never),
            (
                http::StatusCode::BAD_REQUEST,
                Some("rate_limit_exceeded"),
                ErrorRetryDto::Never,
            ),
            (
                http::StatusCode::INTERNAL_SERVER_ERROR,
                None,
                ErrorRetryDto::Delayed,
            ),
            (
                http::StatusCode::SERVICE_UNAVAILABLE,
                Some("invalid_request_error"),
                ErrorRetryDto::Delayed,
            ),
            (
                http::StatusCode::TOO_MANY_REQUESTS,
                Some("rate_limit_exceeded"),
                ErrorRetryDto::Delayed,
            ),
            // A status outside the client/server taxonomy keeps the `type`
            // string as the secondary signal, and stays permanent without it.
            (
                http::StatusCode::FOUND,
                Some("server_error"),
                ErrorRetryDto::Delayed,
            ),
            (http::StatusCode::FOUND, None, ErrorRetryDto::Never),
        ] {
            assert_eq!(
                map_openai_error(&api_error(status, kind)).retry(),
                expected,
                "status {status} with type {kind:?}"
            );
        }
    }

    #[test]
    fn native_transport_errors_and_normalized_codes_never_leak_provider_text() {
        assert_eq!(
            map_openai_error(&OpenAIError::StreamError(Box::new(
                async_openai::error::StreamError::EventStream("secret provider text".to_owned())
            )))
            .retry(),
            ErrorRetryDto::Delayed
        );
        let error = map_openai_error(&api_error(http::StatusCode::BAD_REQUEST, None));
        assert_eq!(error.code(), "generic_chat_provider_request_rejected");
        assert!(
            !serde_json::to_string(&error)
                .expect("error serializes")
                .contains("secret provider text")
        );
    }

    fn api_error(status: http::StatusCode, kind: Option<&str>) -> OpenAIError {
        OpenAIError::ApiError(async_openai::error::ApiErrorResponse {
            status_code: status,
            api_error: async_openai::error::ApiError {
                message: "secret provider text".to_owned(),
                r#type: kind.map(str::to_owned),
                param: None,
                code: None,
                misalignment: None,
            },
        })
    }

    #[test]
    fn trailing_usage_after_finish_reason_is_drained_before_finished() {
        // Standard stream order: finish-reason chunk, trailing usage-only
        // chunk, then end. The finish reason must not terminalize the stream
        // before the usage chunk is polled (PR24-009).
        let events = collect_chunks(vec![
            Ok(chunk(vec![choice(None, None, None, Some("stop"))], None)),
            Ok(chunk(Vec::new(), Some(usage()))),
        ]);
        assert_eq!(
            events,
            vec![
                Ok(ModelEventDto::started()),
                Ok(ModelEventDto::usage(
                    UsageDto::reported(2, 3, 5).expect("usage is valid")
                )),
                Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
            ]
        );
    }

    #[test]
    fn usage_is_emitted_at_most_once_and_a_repeat_is_dropped() {
        // A gateway that repeats the usage summary is not a stream defect: the
        // first summary wins and the repeat is dropped, so the stream still
        // ends through its own terminal outcome instead of a usage failure.
        let duplicated = collect_chunks(vec![
            Ok(chunk(Vec::new(), Some(usage()))),
            Ok(chunk(Vec::new(), Some(usage()))),
        ]);
        assert_eq!(
            duplicated[..2],
            [
                Ok(ModelEventDto::started()),
                Ok(ModelEventDto::usage(
                    UsageDto::reported(2, 3, 5).expect("usage is valid")
                )),
            ],
            "the first usage summary is emitted once and the repeat is dropped"
        );
        assert!(
            matches!(
                duplicated.last(),
                Some(Err(error)) if error.code() == "generic_chat_stream_incomplete"
            ),
            "the dropped repeat leaves the stream to end through its own terminal outcome"
        );

        let post_finish = collect_chunks(vec![
            Ok(chunk(vec![choice(None, None, None, Some("stop"))], None)),
            Ok(chunk(vec![choice(Some("late"), None, None, None)], None)),
        ]);
        assert!(matches!(
            post_finish.last(),
            Some(Err(error)) if error.code() == "generic_chat_post_finish_content"
        ));
    }

    #[test]
    fn a_rejected_usage_total_ends_the_chunk_before_any_delta() {
        // A rejected usage total is terminal, so the text delta and the finish
        // reason carried beside it are dropped instead of being queued behind
        // the failure: nothing may follow a terminal fact.
        let events = collect_chunks(vec![
            Ok(chunk(
                vec![choice(Some("late"), None, None, Some("stop"))],
                Some(async_openai::types::chat::CompletionUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 1,
                    prompt_tokens_details: None,
                    completion_tokens_details: None,
                }),
            )),
            Ok(chunk(vec![choice(Some("later"), None, None, None)], None)),
        ]);

        assert_eq!(events.len(), 2, "no fact may follow the terminal failure");
        assert_eq!(events[0], Ok(ModelEventDto::started()));
        assert!(matches!(
            events[1],
            Err(ref error) if error.code() == "generic_chat_invalid_usage"
        ));
    }

    #[test]
    fn reasoning_deltas_keep_their_order_among_text_usage_and_finish() {
        let events = collect_chunks(vec![
            Ok(chunk(
                vec![choice(Some("answer"), Some("weighing"), None, None)],
                None,
            )),
            Ok(chunk(vec![choice(None, Some(" harder"), None, None)], None)),
            Ok(chunk(vec![choice(None, None, None, Some("stop"))], None)),
            Ok(chunk(Vec::new(), Some(usage()))),
        ]);

        assert_eq!(
            events,
            vec![
                Ok(ModelEventDto::started()),
                Ok(ModelEventDto::reasoning_delta(
                    ReasoningFragmentCategoryDto::Primary,
                    "weighing"
                )
                .expect("reasoning fragment is valid")),
                Ok(ModelEventDto::text_delta("answer").expect("text delta is valid")),
                Ok(ModelEventDto::reasoning_delta(
                    ReasoningFragmentCategoryDto::Primary,
                    " harder"
                )
                .expect("reasoning fragment is valid")),
                Ok(ModelEventDto::usage(
                    UsageDto::reported(2, 3, 5).expect("usage is valid")
                )),
                Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
            ]
        );
    }

    #[test]
    fn empty_reasoning_values_mark_presence_at_most_once() {
        // The provider repeats an empty reasoning value on nearly every chunk;
        // it stays a single textless presence event instead of one event per
        // chunk, and it creates no reasoning fact.
        let events = collect_chunks(vec![
            Ok(chunk(vec![choice(None, Some(""), None, None)], None)),
            Ok(chunk(vec![choice(None, Some(""), None, None)], None)),
            Ok(chunk(vec![choice(None, Some(""), None, None)], None)),
            Ok(chunk(vec![choice(None, None, None, Some("stop"))], None)),
        ]);
        assert_eq!(
            events,
            vec![
                Ok(ModelEventDto::started()),
                Ok(ModelEventDto::reasoning_presence()),
                Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
            ]
        );
    }

    #[test]
    fn reasoning_after_finish_reason_fails_the_stream() {
        let events = collect_chunks(vec![
            Ok(chunk(vec![choice(None, None, None, Some("stop"))], None)),
            Ok(chunk(vec![choice(None, Some("late"), None, None)], None)),
        ]);
        assert!(matches!(
            events.last(),
            Some(Err(error)) if error.code() == "generic_chat_post_finish_content"
        ));
    }

    #[test]
    fn oversized_reasoning_fragments_fail_the_stream_whole() {
        let oversized = "a".repeat(crate::model::MAX_MODEL_REASONING_FRAGMENT_BYTES + 1);
        let events = collect_chunks(vec![
            Ok(chunk(
                vec![choice(None, Some(&oversized), None, None)],
                None,
            )),
            Ok(chunk(vec![choice(None, None, None, Some("stop"))], None)),
        ]);
        assert!(matches!(
            events.last(),
            Some(Err(error)) if error.code() == "provider_reasoning_fragment_too_large"
        ));
        assert!(
            !events.iter().any(|event| matches!(
                event,
                Ok(ModelEventDto::ReasoningDelta { .. })
                    | Ok(ModelEventDto::ReasoningSummaryDelta { .. })
            )),
            "an over-bound native fragment is rejected whole, never truncated"
        );
    }

    #[test]
    fn tool_fragments_merge_across_interleaved_reasoning_deltas() {
        let events = collect_chunks(vec![
            Ok(chunk(
                vec![choice(
                    None,
                    Some("planning the call"),
                    Some(vec![
                        async_openai::types::chat::ChatCompletionMessageToolCallChunk {
                            index: 0,
                            id: Some("call".to_owned()),
                            r#type: Some(async_openai::types::chat::FunctionType::Function),
                            function: Some(async_openai::types::chat::FunctionCallStream {
                                name: Some("inspect".to_owned()),
                                arguments: Some("{\"path\"".to_owned()),
                            }),
                        },
                    ]),
                    None,
                )],
                None,
            )),
            Ok(chunk(
                vec![choice(
                    None,
                    Some(""),
                    Some(vec![
                        async_openai::types::chat::ChatCompletionMessageToolCallChunk {
                            index: 0,
                            id: None,
                            r#type: None,
                            function: Some(async_openai::types::chat::FunctionCallStream {
                                name: None,
                                arguments: Some(":\"src\"}".to_owned()),
                            }),
                        },
                    ]),
                    None,
                )],
                None,
            )),
            Ok(chunk(
                vec![choice(None, None, None, Some("tool_calls"))],
                None,
            )),
        ]);

        assert_eq!(
            events[..3],
            [
                Ok(ModelEventDto::started()),
                Ok(ModelEventDto::reasoning_delta(
                    ReasoningFragmentCategoryDto::Primary,
                    "planning the call"
                )
                .expect("reasoning fragment is valid")),
                // The interleaved empty value repeats the channel without
                // text; it stays one presence marker before the call it
                // belongs to.
                Ok(ModelEventDto::reasoning_presence()),
            ]
        );
        assert!(matches!(
            events.get(3),
            Some(Ok(ModelEventDto::ToolCall { call }))
                if call.name() == "inspect" && call.arguments_json() == r#"{"path":"src"}"#
        ));
        assert_eq!(
            events.get(4),
            Some(&Ok(ModelEventDto::finished(FinishReasonDto::ToolCalls)))
        );
        assert_eq!(events.len(), 5);
    }

    fn chunk(
        choices: Vec<wire::WireChoice>,
        usage: Option<async_openai::types::chat::CompletionUsage>,
    ) -> WireChunk {
        WireChunk { choices, usage }
    }

    fn choice(
        content: Option<&str>,
        reasoning_content: Option<&str>,
        tool_calls: Option<Vec<async_openai::types::chat::ChatCompletionMessageToolCallChunk>>,
        finish_reason: Option<&str>,
    ) -> wire::WireChoice {
        wire::WireChoice {
            index: 0,
            delta: WireDelta {
                content: content.map(str::to_owned),
                reasoning_content: reasoning_content.map(str::to_owned),
                tool_calls,
            },
            finish_reason: finish_reason.map(str::to_owned),
        }
    }

    #[test]
    fn native_chunks_normalize_content_tools_and_terminal_finish() {
        let events = collect_chunks(vec![Ok(chunk(
            vec![choice(
                Some("answer"),
                None,
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
                Some("tool_calls"),
            )],
            None,
        ))]);

        assert_eq!(
            events[..2],
            [
                Ok(ModelEventDto::started()),
                Ok(ModelEventDto::text_delta("answer").expect("valid text")),
            ]
        );
        assert!(matches!(
            events.get(2),
            Some(Ok(ModelEventDto::ToolCall { call })) if call.name() == "inspect"
        ));
        assert_eq!(
            events.get(3),
            Some(&Ok(ModelEventDto::finished(FinishReasonDto::ToolCalls)))
        );
        assert_eq!(events.len(), 4);
    }

    #[test]
    fn native_chunks_reject_duplicate_finish_invalid_usage_and_incomplete_tools() {
        let duplicate = collect_chunks(vec![
            Ok(chunk(vec![choice(None, None, None, Some("stop"))], None)),
            Ok(chunk(vec![choice(None, None, None, Some("length"))], None)),
        ]);
        assert!(matches!(
            duplicate.last(),
            Some(Err(error)) if error.code() == "generic_chat_duplicate_finish"
        ));

        let invalid_usage = collect_chunks(vec![Ok(chunk(
            Vec::new(),
            Some(async_openai::types::chat::CompletionUsage {
                prompt_tokens: 1,
                completion_tokens: 1,
                total_tokens: 1,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }),
        ))]);
        assert!(matches!(
            invalid_usage.last(),
            Some(Err(error)) if error.code() == "generic_chat_invalid_usage"
        ));

        let incomplete = collect_chunks(vec![Ok(chunk(
            vec![choice(
                None,
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
                Some("tool_calls"),
            )],
            None,
        ))]);
        assert!(matches!(
            incomplete.last(),
            Some(Err(error)) if error.code() == "generic_chat_invalid_tool_call"
        ));
    }

    #[test]
    fn unlisted_wire_finish_reasons_degrade_to_unknown_instead_of_aborting() {
        // A gateway may answer with a reason this adapter does not know (for
        // example `stop_sequence`, `max_tokens`, or a vendor-specific value).
        // The chunk must decode and the response must complete with the closed
        // `Unknown` reason instead of failing the whole response.
        for reason in ["stop_sequence", "max_tokens", "vendor_specific_reason"] {
            let events = collect_chunks(vec![Ok(decode_chunk(&format!(
                r#"{{"choices":[{{"index":0,"delta":{{"content":"answer"}},"finish_reason":"{reason}"}}]}}"#
            )))]);

            assert_eq!(
                events,
                vec![
                    Ok(ModelEventDto::started()),
                    Ok(ModelEventDto::text_delta("answer").expect("valid text")),
                    Ok(ModelEventDto::finished(FinishReasonDto::Unknown)),
                ],
                "reason {reason}"
            );
        }
    }

    #[test]
    fn known_wire_finish_reasons_keep_their_closed_mapping() {
        for (reason, expected) in [
            ("stop", FinishReasonDto::Stop),
            ("length", FinishReasonDto::Length),
            ("tool_calls", FinishReasonDto::ToolCalls),
            ("content_filter", FinishReasonDto::ContentFilter),
            ("error", FinishReasonDto::Error),
            // The closed reason set has no function-call category, so the
            // provider value keeps degrading to `Unknown` as before.
            ("function_call", FinishReasonDto::Unknown),
        ] {
            let events = collect_chunks(vec![Ok(decode_chunk(&format!(
                r#"{{"choices":[{{"index":0,"delta":{{}},"finish_reason":"{reason}"}}]}}"#
            )))]);
            assert_eq!(
                events,
                vec![
                    Ok(ModelEventDto::started()),
                    Ok(ModelEventDto::finished(expected)),
                ],
                "reason {reason}"
            );
        }
    }

    fn decode_chunk(raw: &str) -> WireChunk {
        serde_json::from_str(raw).expect("fixture chunk decodes through the private wire type")
    }
}
