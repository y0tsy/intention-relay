mod support;

use intention_proto::{RunId, ToolCallId};
use intention_providers::{
    AssistantReasoningDto, ModelCapabilitiesDto, ModelEventDto, ModelMessageDto, ModelRequestDto,
    ModelRoleDto, ModelToolDefinitionDto, ProviderErrorDto, ToolCallDto, UsageDto,
};
use support::plain_request;

fn message(role: ModelRoleDto, content: &str) -> ModelMessageDto {
    ModelMessageDto::new(role, content).expect("fixture message is valid")
}

fn tool_definition(name: &str) -> ModelToolDefinitionDto {
    ModelToolDefinitionDto::new(name, "fixture description", r#"{"type":"object"}"#)
}

#[test]
fn model_request_and_capabilities_validate_provider_neutral_contracts() {
    assert!(
        ModelCapabilitiesDto::new(true)
            .ensure_runtime_requirements()
            .is_ok()
    );

    let valid = plain_request();
    assert_eq!(valid.model(), "fixture-model");
    assert!(ModelRequestDto::new(RunId::new(), " ", vec![], None).is_err());
    assert!(ModelMessageDto::new(ModelRoleDto::User, " ").is_err());
}

#[test]
fn capabilities_cover_the_runtime_requirement_and_safe_provider_codes() {
    let complete = ModelCapabilitiesDto::new(true);
    complete
        .ensure_runtime_requirements()
        .expect("a declaration that serves streamed text with tool calls passes");
    assert_eq!(
        ModelCapabilitiesDto::new(false)
            .ensure_runtime_requirements()
            .expect_err("a declaration without streamed text and tool calls fails")
            .code(),
        "unsupported_model_capability"
    );

    // The closed constructors are the validation boundary for these in-process
    // model DTOs; there is no decoder for them (ARCH-5-5, DEC-6).
    assert!(ModelEventDto::text_delta("").is_err());
    assert!(ModelEventDto::reasoning_delta("").is_err());
    assert!(
        ModelRequestDto::new(
            RunId::new(),
            "model",
            vec![message(ModelRoleDto::User, "message")],
            Some(" ".to_owned()),
        )
        .is_err()
    );

    assert!(serde_json::from_str::<ProviderErrorDto>(r#"{"code":"","retry":"never"}"#).is_err());
    assert!(ProviderErrorDto::unavailable(" ", false).is_err());
}

#[test]
fn model_stream_payload_constructors_reject_invalid_values() {
    // The normalized stream enforces the order contract while it builds, so
    // these are the payload rules the facts it carries keep closed.
    assert!(ToolCallDto::new(ToolCallId::new(), " ", "{}").is_err());
    assert!(ToolCallDto::new(ToolCallId::new(), "inspect", "not-json").is_err());
    assert!(UsageDto::reported(3, 5, 7).is_err());
}

#[test]
fn model_tool_messages_validate_roles_fields_and_content() {
    let call = ToolCallDto::new(ToolCallId::new(), "inspect", "{}").expect("tool call is valid");

    let with_content =
        ModelMessageDto::assistant_tool_calls(Some("thinking".to_owned()), vec![call.clone()])
            .expect("assistant tool-call message with content is valid");
    assert_eq!(with_content.role(), ModelRoleDto::Assistant);
    assert_eq!(with_content.content(), "thinking");
    assert_eq!(with_content.tool_calls(), Some(&[call.clone()][..]));
    assert_eq!(with_content.tool_call_id(), None);

    let without_content = ModelMessageDto::assistant_tool_calls(None, vec![call.clone()])
        .expect("tool-call message is valid");
    assert_eq!(without_content.content(), "");
    assert_eq!(without_content.tool_calls(), Some(&[call][..]));

    let tool_call_id = ToolCallId::new();
    let result = ModelMessageDto::tool_result(tool_call_id, "result content")
        .expect("tool result message is valid");
    assert_eq!(result.role(), ModelRoleDto::Tool);
    assert_eq!(result.content(), "result content");
    assert_eq!(result.tool_call_id(), Some(tool_call_id));
    assert_eq!(result.tool_calls(), None);

    // The role and field rules the decode half used to enforce are constructor
    // rules now: a tool-role message needs its call identity, an assistant
    // tool-call message needs at least one call, and content stays non-blank.
    assert_eq!(
        ModelMessageDto::new(ModelRoleDto::Tool, "result")
            .expect_err("a tool-role message needs its call identity")
            .code(),
        "invalid_model_message_role"
    );
    assert_eq!(
        ModelMessageDto::assistant_tool_calls(None, Vec::new())
            .expect_err("empty tool-call list must fail")
            .code(),
        "invalid_model_message_tool_calls"
    );
    assert!(ModelMessageDto::tool_result(tool_call_id, " ").is_err());
}

#[test]
fn message_cache_markers_and_replaced_content_validate() {
    let unmarked = message(ModelRoleDto::User, "first");
    assert!(!unmarked.cache_control());

    let mut marked = ModelMessageDto::tool_result(ToolCallId::new(), "tool result")
        .expect("tool result message is valid");
    marked.set_cache_control(true);
    assert!(marked.cache_control());

    let tool_call_id = ToolCallId::new();
    let mut result = ModelMessageDto::tool_result(tool_call_id, "x".repeat(64))
        .expect("tool result message is valid");
    result
        .replace_content("short [compressed]")
        .expect("replacement content is valid");
    assert_eq!(result.content(), "short [compressed]");
    assert_eq!(result.role(), ModelRoleDto::Tool);
    assert_eq!(result.tool_call_id(), Some(tool_call_id));
    assert_eq!(
        result
            .replace_content(" ")
            .expect_err("blank replacement must fail")
            .code(),
        "invalid_model_message_content"
    );
    assert_eq!(result.content(), "short [compressed]");
}

#[test]
fn model_request_with_messages_preserves_fields() {
    let request = ModelRequestDto::new(
        RunId::new(),
        "fixture-model",
        vec![message(ModelRoleDto::User, "first")],
        Some("system-context".to_owned()),
    )
    .expect("request is valid")
    .with_tools(vec![tool_definition("inspect_path")])
    .expect("request with tools is valid");
    let call = ToolCallDto::new(ToolCallId::new(), "inspect", "{}").expect("tool call is valid");
    let updated = request
        .with_messages(vec![
            message(ModelRoleDto::User, "next"),
            ModelMessageDto::assistant_tool_calls(None, vec![call])
                .expect("tool-call message is valid"),
        ])
        .expect("updated request is valid");
    assert_eq!(updated.run_id(), request.run_id());
    assert_eq!(updated.model(), request.model());
    assert_eq!(updated.system_context(), request.system_context());
    assert_eq!(updated.tools(), request.tools());
    assert_eq!(updated.messages().len(), 2);
    assert_eq!(updated.messages()[0].role(), ModelRoleDto::User);
    assert!(updated.messages()[1].tool_calls().is_some());
    assert!(request.with_messages(Vec::new()).is_err());
}

#[test]
fn assistant_reasoning_carries_its_text_and_bounded_text_rules() {
    let call_id = ToolCallId::new();
    let reasoning = AssistantReasoningDto::new(vec![call_id], "considering context")
        .expect("reasoning is valid");
    assert_eq!(reasoning.tool_call_ids(), &[call_id]);
    assert_eq!(reasoning.text(), "considering context");

    // An empty text is the presence-only form and must stay valid.
    let presence =
        AssistantReasoningDto::new(vec![call_id], "").expect("presence-only reasoning is valid");
    assert!(presence.text().is_empty());
    assert_eq!(presence.tool_call_ids(), &[call_id]);

    assert!(
        AssistantReasoningDto::new(vec![call_id], "line one\nline two\tindented").is_ok(),
        "the text keeps every whitespace character it carries"
    );
    // The identities are the round's own tool calls, so the attachment binds to
    // whatever it names: an attachment naming none binds to no message and
    // never reaches the wire.
    let unbound = AssistantReasoningDto::new(Vec::new(), "thinking")
        .expect("an attachment that binds to no call is still a value");
    assert!(unbound.tool_call_ids().is_empty());
    let bounded = "a".repeat(512 * 1024);
    assert!(AssistantReasoningDto::new(vec![call_id], bounded).is_ok());
    let oversized = "a".repeat(512 * 1024 + 1);
    assert_eq!(
        AssistantReasoningDto::new(vec![call_id], oversized)
            .expect_err("oversized reasoning must fail")
            .code(),
        "invalid_model_assistant_reasoning"
    );
    for text in ["bad\u{0}text", "bad\u{7}text", "bad\u{1b}text"] {
        assert_eq!(
            AssistantReasoningDto::new(vec![call_id], text)
                .expect_err("invalid control characters must fail")
                .code(),
            "invalid_model_assistant_reasoning"
        );
    }
}

#[test]
fn reasoning_presence_marks_a_textless_provider_channel() {
    let presence = ModelEventDto::reasoning_presence();
    assert_eq!(
        presence,
        ModelEventDto::ReasoningDelta {
            content: String::new(),
        }
    );
    assert!(ModelEventDto::reasoning_delta("").is_err());
}

#[test]
fn model_request_assistant_reasoning_survives_rebuilds() {
    let without_reasoning = plain_request();
    assert!(without_reasoning.assistant_reasoning().is_empty());
    let encoded = serde_json::to_string(&without_reasoning).expect("request serializes");
    assert!(!encoded.contains("\"assistant_reasoning\""));

    let call = ToolCallDto::new(ToolCallId::new(), "inspect", "{}").expect("tool call is valid");
    let reasoning =
        AssistantReasoningDto::new(vec![call.call_id()], "considering context").expect("valid");
    let with_reasoning = without_reasoning
        .with_assistant_reasoning(vec![reasoning.clone()])
        .expect("request with reasoning is valid");
    assert_eq!(
        with_reasoning.assistant_reasoning(),
        std::slice::from_ref(&reasoning)
    );
    let encoded = serde_json::to_string(&with_reasoning).expect("request serializes");
    assert!(encoded.contains("\"assistant_reasoning\""));

    let rebuilt_messages = with_reasoning
        .with_messages(vec![message(ModelRoleDto::User, "next")])
        .expect("updated request is valid");
    assert_eq!(
        rebuilt_messages.assistant_reasoning(),
        with_reasoning.assistant_reasoning()
    );
    let rebuilt_tools = with_reasoning
        .with_tools(vec![tool_definition("inspect_path")])
        .expect("tool request is valid");
    assert_eq!(
        rebuilt_tools.assistant_reasoning(),
        with_reasoning.assistant_reasoning()
    );
    let cleared = rebuilt_tools
        .with_assistant_reasoning(Vec::new())
        .expect("cleared request is valid");
    assert!(cleared.assistant_reasoning().is_empty());
}

#[test]
fn model_tool_definitions_expose_their_code_owned_fields() {
    let definition = tool_definition("inspect_path");
    assert_eq!(definition.name(), "inspect_path");
    assert_eq!(definition.description(), "fixture description");
    assert_eq!(definition.parameters_json(), r#"{"type":"object"}"#);
}
