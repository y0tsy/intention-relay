//! Shared normalization rules for the concrete provider adapters.
//!
//! Both adapters translate their native SDK values into the same
//! provider-neutral DTOs. A rule that is identical for every adapter lives
//! here, so it has one owner; anything that depends on a native SDK type (a
//! native finish-reason enum, an SDK retryability classifier, or a native wire
//! message shape) stays in the adapter that owns that SDK.

use std::str::FromStr;

use crate::model::{ModelMessageDto, ModelRoleDto, ProviderErrorDto, ToolCallDto, UsageDto};
use intention_proto::{DtoResult, ErrorDto, ToolCallId};

/// The wire-role class every adapter emits for one provider-neutral role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WireRole {
    /// One system instruction block.
    System,
    /// One user-role message, including a daemon-synthesized notice.
    User,
    /// One prior assistant response.
    Assistant,
    /// One tool-role result.
    Tool,
}

/// Maps one provider-neutral role onto the shared wire-role class.
///
/// A notice is daemon-synthesized context for the model: the wire carries it as
/// a user-role message with its text unchanged.
pub const fn wire_role(role: ModelRoleDto) -> WireRole {
    match role {
        ModelRoleDto::System => WireRole::System,
        ModelRoleDto::User | ModelRoleDto::Notice => WireRole::User,
        ModelRoleDto::Assistant => WireRole::Assistant,
        ModelRoleDto::Tool => WireRole::Tool,
    }
}

/// Maps native token counters onto validated canonical usage.
///
/// # Errors
///
/// Returns a validation error when the total does not equal input plus output.
pub fn reported_usage(
    input_tokens: u32,
    output_tokens: u32,
    total_tokens: u32,
) -> DtoResult<UsageDto> {
    UsageDto::reported(
        u64::from(input_tokens),
        u64::from(output_tokens),
        u64::from(total_tokens),
    )
}

/// Assembles one complete provider tool call with a fresh canonical identity.
///
/// The provider's own call identity never crosses the boundary: the canonical
/// DTO always carries a locally allocated identity, and only the function name
/// and argument text are provider-derived.
///
/// # Errors
///
/// Returns a validation error for an invalid function-call shape.
pub fn complete_tool_call(name: &str, arguments_json: &str) -> DtoResult<ToolCallDto> {
    ToolCallDto::new(ToolCallId::new(), name, arguments_json)
}

/// Returns the tool-call identity a tool-role message must carry.
///
/// # Errors
///
/// Returns the adapter's validation error when the message carries no identity.
pub fn tool_result_identity(
    message: &ModelMessageDto,
    invalid_code: &'static str,
) -> DtoResult<ToolCallId> {
    message.tool_call_id().ok_or_else(|| {
        ErrorDto::validation(
            invalid_code,
            "tool-role messages must carry one tool call identity",
        )
    })
}

/// Decodes validated tool-parameter text into the adapter's target type.
///
/// The decoded type is inferred at the call site from the native constructor
/// that consumes it, so an adapter never names a JSON value type.
///
/// # Errors
///
/// Returns the adapter's validation error when the text does not decode.
pub fn decode_parameters<T: FromStr>(
    raw: &str,
    invalid_code: &'static str,
    invalid_message: &'static str,
) -> DtoResult<T> {
    raw.parse()
        .map_err(|_| ErrorDto::validation(invalid_code, invalid_message))
}

/// Builds the normalized provider failure selected by one retry decision.
///
/// The adapter supplies both of its fixed codes; the retry decision selects
/// which one the normalized error carries.
pub fn provider_error(
    unavailable_code: &'static str,
    rejected_code: &'static str,
    retryable: bool,
) -> ProviderErrorDto {
    let code = if retryable {
        unavailable_code
    } else {
        rejected_code
    };
    error_dto(code, retryable)
}

/// Builds one fixed non-retryable provider failure that carries no native text.
pub fn fixed_error(code: &'static str) -> ProviderErrorDto {
    error_dto(code, false)
}

#[allow(
    clippy::expect_used,
    reason = "Normalized provider error codes are compile-time adapter constants, so validation cannot fail."
)]
fn error_dto(code: &'static str, retryable: bool) -> ProviderErrorDto {
    ProviderErrorDto::unavailable(code, retryable, None)
        .expect("normalized provider error codes are non-blank adapter constants")
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "Mapping-rule fixtures use expect to name impossible local failures."
)]
mod tests {
    use super::*;
    use crate::model::ModelEventDto;
    use intention_proto::ErrorRetryDto;

    // Text and reasoning reach the stream contract through these constructors
    // alone, so their canonical shape is part of the shared mapping evidence.
    #[test]
    fn text_and_reasoning_events_keep_the_canonical_wire_shape() {
        let text = ModelEventDto::text_delta("hello").expect("text maps");
        assert_eq!(
            serde_json::to_string(&text).expect("text serializes"),
            r#"{"kind":"text_delta","content":"hello"}"#
        );
        let reasoning =
            ModelEventDto::reasoning_delta("considering context").expect("reasoning maps");
        assert_eq!(
            serde_json::to_string(&reasoning).expect("reasoning serializes"),
            r#"{"kind":"reasoning_delta","content":"considering context"}"#
        );
    }

    #[test]
    fn complete_tool_calls_keep_the_native_function_and_allocate_the_canonical_identity() {
        let tool = complete_tool_call("inspect", "{}").expect("tool maps");
        assert_eq!(tool.name(), "inspect");
        assert_eq!(tool.arguments_json(), "{}");
        let second = complete_tool_call("inspect", "{}").expect("tool maps");
        assert_ne!(
            tool.call_id(),
            second.call_id(),
            "the canonical identity is allocated locally, never copied from the provider"
        );
        assert!(complete_tool_call("", "{}").is_err());
        assert!(complete_tool_call("inspect", "not-json").is_err());
    }

    #[test]
    fn tool_result_messages_require_exactly_one_identity() {
        let call = complete_tool_call("inspect", "{}").expect("tool maps");
        let result =
            ModelMessageDto::tool_result(call.call_id(), "result").expect("message is valid");
        assert_eq!(
            tool_result_identity(&result, "invalid_fixture_request").expect("identity is present"),
            call.call_id()
        );
        let text = ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid");
        assert_eq!(
            tool_result_identity(&text, "invalid_fixture_request")
                .expect_err("a text message carries no tool-result identity")
                .code(),
            "invalid_fixture_request"
        );
    }

    #[test]
    fn every_adapter_role_shares_one_wire_mapping_and_notices_are_user_context() {
        for (role, expected) in [
            (ModelRoleDto::System, WireRole::System),
            (ModelRoleDto::User, WireRole::User),
            (ModelRoleDto::Notice, WireRole::User),
            (ModelRoleDto::Assistant, WireRole::Assistant),
            (ModelRoleDto::Tool, WireRole::Tool),
        ] {
            assert_eq!(wire_role(role), expected, "role {role:?}");
        }
    }

    #[test]
    fn parameter_decoding_reports_the_adapter_error_without_native_text() {
        let decoded: u32 = decode_parameters(
            "42",
            "invalid_fixture_request",
            "fixture parameters could not be decoded",
        )
        .expect("typed parameter text decodes");
        assert_eq!(decoded, 42);
        let error = decode_parameters::<u32>(
            "secret provider text",
            "invalid_fixture_request",
            "fixture parameters could not be decoded",
        )
        .expect_err("non-numeric text fails the decode");
        assert_eq!(error.code(), "invalid_fixture_request");
        assert!(!format!("{error:?}").contains("secret provider text"));
    }

    #[test]
    fn provider_errors_keep_the_adapter_codes_retry_guidance_and_no_native_text() {
        for (retryable, expected_code, expected_retry) in [
            (
                true,
                "openrouter_provider_unavailable",
                ErrorRetryDto::Delayed,
            ),
            (
                false,
                "openrouter_provider_request_rejected",
                ErrorRetryDto::Never,
            ),
        ] {
            let error = provider_error(
                "openrouter_provider_unavailable",
                "openrouter_provider_request_rejected",
                retryable,
            );
            assert_eq!(error.code(), expected_code);
            assert_eq!(error.retry(), expected_retry);
            assert!(
                !serde_json::to_string(&error)
                    .expect("error serializes")
                    .contains("provider text must not leak")
            );
        }
        let error = provider_error(
            "generic_chat_provider_unavailable",
            "generic_chat_provider_request_rejected",
            true,
        );
        assert_eq!(error.code(), "generic_chat_provider_unavailable");
        assert_eq!(error.retry(), ErrorRetryDto::Delayed);
    }

    #[test]
    fn fixed_errors_are_non_retryable_and_never_carry_native_text() {
        let error = fixed_error("openrouter_provider_failure");
        assert_eq!(error.code(), "openrouter_provider_failure");
        assert_eq!(error.retry(), ErrorRetryDto::Never);
        let error = fixed_error("generic_chat_provider_failure");
        assert_eq!(error.code(), "generic_chat_provider_failure");
        assert_eq!(error.retry(), ErrorRetryDto::Never);
    }
}
