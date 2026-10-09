#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

mod support;

use intention_proto::{RunId, ToolCallId};
use intention_providers::{
    AssistantReasoningDto, FinishReasonDto, ModelCapabilitiesDto, ModelEventDto, ModelMessageDto,
    ModelRequestDto, ModelRoleDto, ModelStreamLifecycleDto, ModelToolDefinitionDto,
    ProviderErrorDto, ToolCallDto, UsageDto,
};
use support::plain_request;

fn message(role: ModelRoleDto, content: &str) -> ModelMessageDto {
    ModelMessageDto::new(role, content).expect("fixture message is valid")
}

fn tool_definition(name: &str) -> ModelToolDefinitionDto {
    ModelToolDefinitionDto::new(name, "fixture description", r#"{"type":"object"}"#)
        .expect("fixture tool definition is valid")
}

#[test]
fn model_request_and_capabilities_validate_provider_neutral_contracts() {
    let capabilities = ModelCapabilitiesDto::new(true, false, true, false, false, true);
    assert!(capabilities.supports_text());
    assert!(!capabilities.supports_reasoning());
    assert!(capabilities.supports_tool_calls());
    assert!(!capabilities.supports_multimodal());
    assert!(!capabilities.supports_vendor_extensions());
    assert!(capabilities.supports_streaming());

    let valid = plain_request();
    assert_eq!(valid.model(), "fixture-model");
    assert!(ModelRequestDto::new(RunId::new(), " ", vec![], None).is_err());
    assert!(ModelMessageDto::new(ModelRoleDto::User, " ").is_err());
}

#[test]
fn stream_lifecycle_accepts_ordered_normalized_events() {
    let mut lifecycle = ModelStreamLifecycleDto::new();
    let tool = ToolCallDto::new(ToolCallId::new(), "inspect", "{\"path\":\"src\"}")
        .expect("tool call is valid");
    let usage = UsageDto::reported(3, 5, 8).expect("usage is internally consistent");
    for event in [
        ModelEventDto::started(),
        ModelEventDto::text_delta("hello").expect("text is valid"),
        ModelEventDto::reasoning_delta("considering context").expect("reasoning is valid"),
        ModelEventDto::tool_call(tool),
        ModelEventDto::usage(usage),
        ModelEventDto::finished(FinishReasonDto::Stop),
    ] {
        lifecycle.accept(&event).expect("ordered event is valid");
    }
}

#[test]
fn capabilities_cover_the_runtime_requirement_and_safe_provider_codes() {
    let complete = ModelCapabilitiesDto::new(true, true, true, true, true, true);
    complete
        .ensure_runtime_requirements()
        .expect("complete capability declaration serves the runtime");
    for unsupported in [
        ModelCapabilitiesDto::new(false, true, true, true, true, true),
        ModelCapabilitiesDto::new(true, true, false, true, true, true),
    ] {
        assert_eq!(
            unsupported
                .ensure_runtime_requirements()
                .expect_err("a declaration without text or tool calls fails")
                .code(),
            "unsupported_model_capability"
        );
    }

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
    assert!(ProviderErrorDto::unavailable(" ", false, None).is_err());
}

#[test]
fn stream_lifecycle_rejects_invalid_order_payloads_duplicate_usage_and_terminal_preconditions() {
    // Facts before `Started` are rejected whichever kind follows the order
    // violation, and the closed payload constructors keep rejecting invalid
    // values.
    let mut before_start = ModelStreamLifecycleDto::new();
    assert_eq!(
        before_start
            .accept(&ModelEventDto::text_delta("first").expect("text is valid"))
            .expect_err("content before start must fail")
            .code(),
        "invalid_model_stream_order"
    );
    assert_eq!(
        before_start
            .accept(&ModelEventDto::reasoning_delta("first").expect("reasoning is valid"))
            .expect_err("reasoning before start must fail")
            .code(),
        "invalid_model_stream_order"
    );
    assert!(
        before_start
            .accept(&ModelEventDto::usage(UsageDto::NotReported))
            .is_err()
    );
    assert!(
        before_start
            .accept(&ModelEventDto::finished(FinishReasonDto::Length))
            .is_err()
    );
    assert!(ToolCallDto::new(ToolCallId::new(), " ", "{}").is_err());
    assert!(ToolCallDto::new(ToolCallId::new(), "inspect", "not-json").is_err());
    assert!(UsageDto::reported(3, 5, 7).is_err());

    // One accepted sequence: start, a single usage summary, one terminal finish.
    let mut lifecycle = ModelStreamLifecycleDto::new();
    lifecycle
        .accept(&ModelEventDto::started())
        .expect("start is valid");
    lifecycle
        .accept(&ModelEventDto::usage(UsageDto::NotReported))
        .expect("first usage is valid");
    assert!(
        lifecycle
            .accept(&ModelEventDto::usage(UsageDto::NotReported))
            .is_err()
    );
    lifecycle
        .accept(&ModelEventDto::finished(FinishReasonDto::ToolCalls))
        .expect("finish is valid");
    for event in [
        ModelEventDto::started(),
        ModelEventDto::text_delta("after").expect("text is valid"),
        ModelEventDto::reasoning_delta("after").expect("reasoning is valid"),
        ModelEventDto::finished(FinishReasonDto::Unknown),
    ] {
        assert!(
            lifecycle.accept(&event).is_err(),
            "no fact follows the terminal finish"
        );
    }
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
fn assistant_reasoning_validates_tool_call_identities_and_bounded_text() {
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

    for text in ["", "line one\nline two\tindented"] {
        assert!(AssistantReasoningDto::new(vec![call_id], text).is_ok());
    }
    assert_eq!(
        AssistantReasoningDto::new(Vec::new(), "thinking")
            .expect_err("reasoning without tool calls must fail")
            .code(),
        "invalid_model_assistant_reasoning"
    );
    assert_eq!(
        AssistantReasoningDto::new(vec![call_id, call_id], "thinking")
            .expect_err("repeated tool-call identities must fail")
            .code(),
        "invalid_model_assistant_reasoning"
    );
    let bounded = "a".repeat(512 * 1024);
    assert!(AssistantReasoningDto::new(vec![call_id], bounded).is_ok());
    let oversized = "a".repeat(512 * 1024 + 1);
    assert_eq!(
        AssistantReasoningDto::new(vec![call_id], oversized)
            .expect_err("oversized reasoning must fail")
            .code(),
        "invalid_model_assistant_reasoning_text"
    );
    for text in ["bad\u{0}text", "bad\u{7}text", "bad\u{1b}text"] {
        assert_eq!(
            AssistantReasoningDto::new(vec![call_id], text)
                .expect_err("invalid control characters must fail")
                .code(),
            "invalid_model_assistant_reasoning_text"
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
fn model_tool_definitions_validate_names_descriptions_and_parameters() {
    let long_name = "a".repeat(65);
    for name in [
        "",
        " ",
        "inspect.path",
        "inspect space",
        "inspecté",
        long_name.as_str(),
    ] {
        assert_eq!(
            ModelToolDefinitionDto::new(name, "description", r#"{"type":"object"}"#)
                .expect_err("invalid tool definition name must fail")
                .code(),
            "invalid_tool_definition_name"
        );
    }
    assert_eq!(
        ModelToolDefinitionDto::new("inspect", " ", r#"{"type":"object"}"#)
            .expect_err("blank tool definition description must fail")
            .code(),
        "invalid_tool_definition_description"
    );
    let oversized_parameters = format!(r#"{{"padding":"{}"}}"#, "a".repeat(70_000));
    for parameters in [
        "",
        "[]",
        r#""text""#,
        "not-json",
        oversized_parameters.as_str(),
    ] {
        assert_eq!(
            ModelToolDefinitionDto::new("inspect", "description", parameters)
                .expect_err("invalid tool definition parameters must fail")
                .code(),
            "invalid_tool_definition_parameters"
        );
    }

    let definition = tool_definition("inspect_path");
    assert_eq!(definition.name(), "inspect_path");
    assert_eq!(definition.description(), "fixture description");
    assert_eq!(definition.parameters_json(), r#"{"type":"object"}"#);
}
