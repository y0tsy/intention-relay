#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

use intention_model::{
    FinishReasonDto, ModelCapabilitiesDto, ModelDriver, ModelEventDto, ModelMessageDto,
    ModelRequestDto, ModelRoleDto, ModelStreamLifecycleDto, ProviderErrorDto, ToolCallDto,
    UsageDto,
};
use intention_types::{CorrelationIdDto, RunId, ToolCallId};

fn message(role: ModelRoleDto, content: &str) -> ModelMessageDto {
    ModelMessageDto::new(role, content).expect("fixture message is valid")
}

fn request() -> ModelRequestDto {
    ModelRequestDto::new(
        RunId::new(),
        "fixture-model",
        vec![message(ModelRoleDto::User, "hello")],
        None,
        None,
    )
    .expect("request is valid")
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

    let valid = request();
    assert_eq!(valid.model(), "fixture-model");
    assert!(ModelRequestDto::new(RunId::new(), " ", vec![], None, None).is_err());
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
    assert!(lifecycle.is_terminal());
}

#[test]
fn stream_lifecycle_rejects_invalid_order_and_invalid_payloads() {
    let mut lifecycle = ModelStreamLifecycleDto::new();
    assert_eq!(
        lifecycle
            .accept(&ModelEventDto::text_delta("first").expect("text is valid"))
            .expect_err("content before start must fail")
            .code(),
        "invalid_model_stream_order"
    );
    assert_eq!(
        lifecycle
            .accept(&ModelEventDto::reasoning_delta("first").expect("reasoning is valid"))
            .expect_err("reasoning before start must fail")
            .code(),
        "invalid_model_stream_order"
    );
    lifecycle
        .accept(&ModelEventDto::started())
        .expect("start is valid");
    assert!(ToolCallDto::new(ToolCallId::new(), " ", "{}").is_err());
    assert!(ToolCallDto::new(ToolCallId::new(), "inspect", "not-json").is_err());
    assert!(UsageDto::reported(3, 5, 7).is_err());
    lifecycle
        .accept(&ModelEventDto::finished(FinishReasonDto::Stop))
        .expect("finish is valid");
    assert!(lifecycle.accept(&ModelEventDto::started()).is_err());
}

#[test]
fn provider_errors_remain_safe_and_credential_free() {
    let error = ProviderErrorDto::unavailable("provider_request_failed", true, None)
        .expect("safe error is valid");
    let encoded = serde_json::to_string(&error).expect("error serializes");
    assert!(encoded.contains("provider_request_failed"));
    assert!(!encoded.contains("fixture-credential-not-real-12345"));
    assert!(ProviderErrorDto::unavailable(" ", false, None).is_err());
}

#[test]
fn public_model_contracts_round_trip_and_preserve_validated_accessors() {
    for role in [
        ModelRoleDto::System,
        ModelRoleDto::User,
        ModelRoleDto::Assistant,
    ] {
        let round_trip: ModelMessageDto = serde_json::from_str(
            &serde_json::to_string(&message(role, "content")).expect("message serializes"),
        )
        .expect("message deserializes");
        assert_eq!(round_trip.role(), role);
        assert_eq!(round_trip.content(), "content");
    }

    let request = ModelRequestDto::new(
        RunId::new(),
        "fixture-model",
        vec![
            message(ModelRoleDto::System, "message-system"),
            message(ModelRoleDto::User, "message-user"),
            message(ModelRoleDto::Assistant, "message-assistant"),
        ],
        Some("system-context".to_owned()),
        Some(intention_model::ModelRequestedCapabilitiesDto::new(
            true, true, true, true,
        )),
    )
    .expect("complete request is valid");
    let encoded = serde_json::to_string(&request).expect("request serializes");
    let decoded: ModelRequestDto = serde_json::from_str(&encoded).expect("request deserializes");
    assert_eq!(decoded.run_id(), request.run_id());
    assert_eq!(decoded.system_context(), Some("system-context"));
    assert!(decoded.requested_capabilities().reasoning());
    assert!(decoded.requested_capabilities().multimodal());
    assert!(decoded.requested_capabilities().tool_calls());
    assert!(decoded.requested_capabilities().vendor_extensions());
    assert_eq!(decoded.messages().len(), 3);
    assert!(
        ModelRequestDto::new(
            RunId::new(),
            "model",
            vec![message(ModelRoleDto::User, "message")],
            Some(" ".to_owned()),
            None,
        )
        .is_err()
    );
    assert!(serde_json::from_str::<ModelRequestDto>(
        r#"{"run_id":"00000000-0000-0000-0000-000000000000","model":"model","messages":[],"unexpected":true}"#
    )
    .is_err());
}

#[test]
fn capabilities_tool_usage_events_and_errors_cover_safe_wire_variants() {
    let complete = ModelCapabilitiesDto::new(true, true, true, true, true, true);
    complete
        .ensure_supports(intention_model::ModelRequestedCapabilitiesDto::new(
            true, true, true, true,
        ))
        .expect("complete capability declaration supports request");
    for requested in [
        intention_model::ModelRequestedCapabilitiesDto::new(true, false, false, false),
        intention_model::ModelRequestedCapabilitiesDto::new(false, true, false, false),
        intention_model::ModelRequestedCapabilitiesDto::new(false, false, true, false),
        intention_model::ModelRequestedCapabilitiesDto::new(false, false, false, true),
    ] {
        assert_eq!(
            ModelCapabilitiesDto::new(true, false, false, false, false, true)
                .ensure_supports(requested)
                .expect_err("unsupported capability fails")
                .code(),
            "unsupported_model_capability"
        );
    }

    let call = ToolCallDto::new(ToolCallId::new(), "inspect", "{}")
        .expect("tool object arguments are valid");
    let decoded: ToolCallDto =
        serde_json::from_str(&serde_json::to_string(&call).expect("tool serializes"))
            .expect("tool deserializes");
    assert_eq!(decoded.call_id(), call.call_id());
    assert_eq!(decoded.name(), "inspect");
    assert_eq!(decoded.arguments_json(), "{}");
    assert!(ToolCallDto::new(ToolCallId::new(), "inspect", "[]").is_err());
    assert!(serde_json::from_str::<ToolCallDto>(
        r#"{"call_id":"00000000-0000-0000-0000-000000000000","name":"inspect","arguments_json":"{}","unexpected":true}"#
    )
    .is_err());

    for usage in [
        UsageDto::NotReported,
        UsageDto::reported(1, 2, 3).expect("usage is valid"),
    ] {
        let decoded: UsageDto =
            serde_json::from_str(&serde_json::to_string(&usage).expect("usage serializes"))
                .expect("usage deserializes");
        assert_eq!(decoded, usage);
    }
    assert!(
        serde_json::from_str::<UsageDto>(
            r#"{"state":"reported","input_tokens":1,"output_tokens":2,"total_tokens":2}"#
        )
        .is_err()
    );

    for reason in [
        FinishReasonDto::Stop,
        FinishReasonDto::Length,
        FinishReasonDto::ToolCalls,
        FinishReasonDto::ContentFilter,
        FinishReasonDto::Error,
        FinishReasonDto::Unknown,
    ] {
        let event = ModelEventDto::finished(reason);
        let decoded: ModelEventDto =
            serde_json::from_str(&serde_json::to_string(&event).expect("event serializes"))
                .expect("event deserializes");
        assert_eq!(decoded, event);
    }
    for event in [
        ModelEventDto::started(),
        ModelEventDto::text_delta("delta").expect("delta is valid"),
        ModelEventDto::reasoning_delta("reasoning").expect("reasoning is valid"),
        ModelEventDto::tool_call(call),
        ModelEventDto::usage(UsageDto::NotReported),
    ] {
        let decoded: ModelEventDto =
            serde_json::from_str(&serde_json::to_string(&event).expect("event serializes"))
                .expect("event deserializes");
        assert_eq!(decoded, event);
    }
    assert!(ModelEventDto::text_delta("").is_err());
    assert!(ModelEventDto::reasoning_delta("").is_err());
    assert!(
        serde_json::from_str::<ModelEventDto>(r#"{"kind":"text_delta","content":""}"#).is_err()
    );
    assert!(
        serde_json::from_str::<ModelEventDto>(r#"{"kind":"reasoning_delta","content":""}"#)
            .is_err()
    );

    let correlation = CorrelationIdDto::new();
    let error = ProviderErrorDto::unavailable("provider_unavailable", false, Some(correlation))
        .expect("provider error is valid");
    let decoded: ProviderErrorDto =
        serde_json::from_str(&serde_json::to_string(&error).expect("error serializes"))
            .expect("error deserializes");
    assert_eq!(decoded.code(), "provider_unavailable");
    assert_eq!(decoded.retry(), intention_types::ErrorRetryDto::Never);
    assert_eq!(decoded.correlation_id(), Some(correlation));
    assert_eq!(decoded.to_string(), "provider_unavailable");
    assert!(serde_json::from_str::<ProviderErrorDto>(r#"{"code":"","retry":"never"}"#).is_err());
}

#[test]
fn lifecycle_rejects_duplicate_usage_and_all_terminal_preconditions() {
    let mut before_start = ModelStreamLifecycleDto::new();
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
    assert!(
        lifecycle
            .accept(&ModelEventDto::text_delta("after").expect("text is valid"))
            .is_err()
    );
    assert!(
        lifecycle
            .accept(&ModelEventDto::reasoning_delta("after").expect("reasoning is valid"))
            .is_err()
    );
    assert!(
        lifecycle
            .accept(&ModelEventDto::finished(FinishReasonDto::Unknown))
            .is_err()
    );
}

#[test]
fn model_tool_messages_round_trip_and_validate() {
    let call = ToolCallDto::new(ToolCallId::new(), "inspect", "{}").expect("tool call is valid");

    let with_content =
        ModelMessageDto::assistant_tool_calls(Some("thinking".to_owned()), vec![call.clone()])
            .expect("assistant tool-call message with content is valid");
    assert_eq!(with_content.role(), ModelRoleDto::Assistant);
    assert_eq!(with_content.content(), "thinking");
    assert_eq!(with_content.tool_calls(), Some(&[call.clone()][..]));
    assert_eq!(with_content.tool_call_id(), None);
    let decoded: ModelMessageDto = serde_json::from_str(
        &serde_json::to_string(&with_content).expect("assistant tool-call message serializes"),
    )
    .expect("assistant tool-call message deserializes");
    assert_eq!(decoded, with_content);

    let without_content = ModelMessageDto::assistant_tool_calls(None, vec![call.clone()])
        .expect("tool-call message is valid");
    assert_eq!(without_content.content(), "");
    assert_eq!(without_content.tool_calls(), Some(&[call][..]));
    let encoded = serde_json::to_string(&without_content).expect("tool-call message serializes");
    assert!(encoded.contains("\"tool_calls\""));
    let decoded: ModelMessageDto =
        serde_json::from_str(&encoded).expect("tool-call message deserializes");
    assert_eq!(decoded, without_content);

    let tool_call_id = ToolCallId::new();
    let result = ModelMessageDto::tool_result(tool_call_id, "result content")
        .expect("tool result message is valid");
    assert_eq!(result.role(), ModelRoleDto::Tool);
    assert_eq!(result.content(), "result content");
    assert_eq!(result.tool_call_id(), Some(tool_call_id));
    assert_eq!(result.tool_calls(), None);
    let decoded: ModelMessageDto = serde_json::from_str(
        &serde_json::to_string(&result).expect("tool result message serializes"),
    )
    .expect("tool result message deserializes");
    assert_eq!(decoded, result);

    assert_eq!(
        ModelMessageDto::assistant_tool_calls(None, Vec::new())
            .expect_err("empty tool-call list must fail")
            .code(),
        "invalid_model_message_tool_calls"
    );
    assert!(ModelMessageDto::tool_result(tool_call_id, " ").is_err());
    assert!(
        serde_json::from_str::<ModelMessageDto>(r#"{"role":"tool","content":"result"}"#).is_err()
    );
    assert!(
        serde_json::from_str::<ModelMessageDto>(
            r#"{"role":"user","content":"hi","tool_call_id":"00000000-0000-0000-0000-000000000000"}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<ModelMessageDto>(
            r#"{"role":"assistant","content":"answer","tool_call_id":"00000000-0000-0000-0000-000000000000"}"#
        )
        .is_err()
    );
}

#[test]
fn model_message_legacy_wire_still_decodes() {
    let legacy: ModelMessageDto = serde_json::from_str(r#"{"role":"user","content":"hi"}"#)
        .expect("legacy user message decodes");
    assert_eq!(legacy.role(), ModelRoleDto::User);
    assert_eq!(legacy.content(), "hi");
    assert_eq!(legacy.tool_calls(), None);
    assert_eq!(legacy.tool_call_id(), None);
    assert_eq!(
        serde_json::to_string(&legacy).expect("legacy message serializes"),
        r#"{"role":"user","content":"hi"}"#
    );
    let assistant: ModelMessageDto =
        serde_json::from_str(r#"{"role":"assistant","content":"answer"}"#)
            .expect("legacy assistant message decodes");
    assert_eq!(assistant.role(), ModelRoleDto::Assistant);
    assert_eq!(assistant.content(), "answer");
    let system: ModelMessageDto = serde_json::from_str(r#"{"role":"system","content":"context"}"#)
        .expect("legacy system message decodes");
    assert_eq!(system.role(), ModelRoleDto::System);
}

#[test]
fn model_request_with_messages_preserves_fields() {
    let request = ModelRequestDto::new(
        RunId::new(),
        "fixture-model",
        vec![message(ModelRoleDto::User, "first")],
        Some("system-context".to_owned()),
        Some(intention_model::ModelRequestedCapabilitiesDto::new(
            true, true, true, true,
        )),
    )
    .expect("request is valid");
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
    assert_eq!(
        updated.requested_capabilities(),
        request.requested_capabilities()
    );
    assert_eq!(updated.messages().len(), 2);
    assert_eq!(updated.messages()[0].role(), ModelRoleDto::User);
    assert!(updated.messages()[1].tool_calls().is_some());
    assert!(request.with_messages(Vec::new()).is_err());
}

struct FixtureDriver(ModelCapabilitiesDto);

impl ModelDriver for FixtureDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        self.0
    }
}

#[test]
fn model_driver_default_preflight_uses_declared_capabilities() {
    let driver = FixtureDriver(ModelCapabilitiesDto::new(
        true, false, false, false, false, true,
    ));
    assert!(driver.preflight(&request()).is_ok());
    let tool_request = ModelRequestDto::new(
        RunId::new(),
        "fixture-model",
        vec![message(ModelRoleDto::User, "hello")],
        None,
        Some(intention_model::ModelRequestedCapabilitiesDto::new(
            false, false, true, false,
        )),
    )
    .expect("tool request validates");
    assert!(driver.preflight(&tool_request).is_err());
}
