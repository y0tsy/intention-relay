#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! DTO contract evidence for the `intention-proto` public API: identity, error, and
//! shared model-value wire contracts.

use intention_proto::{
    CorrelationIdDto, ErrorCategoryDto, ErrorDto, ErrorRetryDto, ProviderErrorDto, SessionId,
    TimestampDto, ToolCallDto, ToolCallId, UsageDto, WorkspaceRelativePathDto,
};

#[test]
fn ids_round_trip_as_canonical_uuid_strings() {
    let session_id = SessionId::new();
    let serialized = serde_json::to_string(&session_id).expect("test serialization must succeed");
    let decoded: SessionId =
        serde_json::from_str(&serialized).expect("test deserialization must succeed");

    assert_eq!(decoded, session_id);
    assert_eq!(decoded.to_string(), serialized.trim_matches('"'));
}

#[test]
fn malformed_ids_return_a_typed_validation_error() {
    for invalid in [
        "not-an-id",
        "aaaaaaaaaaaaaaaa4aaa8aaaaaaaaaaaaaaa",
        "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
    ] {
        let error = SessionId::parse(invalid).expect_err("invalid ID must fail");
        assert_eq!(error.category(), ErrorCategoryDto::Validation);
        assert_eq!(error.code(), "invalid_id");
    }
}

#[test]
fn validated_value_dtos_reject_invalid_wire_values() {
    assert!(serde_json::from_str::<TimestampDto>("-1").is_err());
}

#[test]
fn current_shape_minimal_error_decodes_absent_optional_fields_as_none() {
    // The current ErrorDto shape keeps `correlation_id` optional on the wire: a
    // minimal current error without it must decode as None and re-encode
    // without inventing a value.
    let error: ErrorDto = serde_json::from_str(
        r#"{"code":"fixture","category":"not_found","message":"safe","retry":"manual"}"#,
    )
    .expect("minimal current-shape error decodes");
    assert_eq!(error.code(), "fixture");
    assert_eq!(error.category(), ErrorCategoryDto::NotFound);
    assert_eq!(error.retry(), ErrorRetryDto::Manual);
    assert!(
        error.correlation_id().is_none(),
        "absent correlation is None"
    );
    let encoded = serde_json::to_string(&error).expect("test serialization must succeed");
    let decoded: ErrorDto =
        serde_json::from_str(&encoded).expect("round trip must decode identically");
    assert_eq!(decoded, error);
}

#[test]
fn malformed_error_wire_data_is_rejected() {
    for invalid_path in [
        "",
        "/etc/passwd",
        "../escape",
        "src/../escape",
        "src/\u{0000}bad",
    ] {
        assert!(WorkspaceRelativePathDto::parse(invalid_path).is_err());
    }
    let correlation = CorrelationIdDto::new();
    let encoded = serde_json::to_string(&correlation).expect("correlation serializes");
    assert_eq!(
        serde_json::from_str::<CorrelationIdDto>(&encoded).expect("correlation decodes"),
        correlation
    );
    let parsed_path =
        WorkspaceRelativePathDto::parse("src/parsed.rs").expect("fixture path is valid");
    assert_eq!(parsed_path.as_str(), "src/parsed.rs");
    for non_canonical in [
        "aaaaaaaaaaaaaaaa4aaa8aaaaaaaaaaaaaaa",
        "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
    ] {
        assert!(CorrelationIdDto::parse(non_canonical).is_err());
    }

    for wire in [
        r#"{"code":"","category":"not_found","message":"safe","retry":"manual"}"#,
        r#"{"code":"fixture","category":"not_found","message":" ","retry":"manual"}"#,
        r#"{"code":"fixture","category":"not_found","message":"safe","retry":"manual","correlation_id":"not-a-uuid"}"#,
    ] {
        assert!(serde_json::from_str::<ErrorDto>(wire).is_err());
    }
}

#[test]
fn model_values_are_owned_by_types_and_preserve_the_contract() {
    let call = ToolCallDto::new(ToolCallId::new(), "inspect", r#"{"path":"src"}"#)
        .expect("object arguments are valid");
    assert_eq!(call.name(), "inspect");
    assert!(ToolCallDto::new(ToolCallId::new(), "inspect", "[]").is_err());

    let usage = UsageDto::reported(2, 3, 5).expect("consistent usage is valid");
    assert_eq!(
        serde_json::to_string(&usage).expect("usage serializes"),
        r#"{"state":"reported","input_tokens":2,"output_tokens":3,"total_tokens":5}"#
    );
    assert!(UsageDto::reported(u64::MAX, 1, 0).is_err());
    assert!(UsageDto::reported(u64::MAX, 1, u64::MAX).is_err());

    let correlation = CorrelationIdDto::new();
    let error = ProviderErrorDto::unavailable("provider_unavailable", true, Some(correlation))
        .expect("safe provider failure is valid");
    assert_eq!(error.retry(), ErrorRetryDto::Delayed);
    assert_eq!(error.correlation_id(), Some(correlation));
    assert_eq!(error.to_string(), "provider_unavailable");
}

#[test]
fn model_values_reject_invalid_and_decode_valid_wire_forms() {
    assert!(ToolCallDto::new(ToolCallId::new(), " ", "{}").is_err());
    assert!(ToolCallDto::new(ToolCallId::new(), "inspect", "not-json").is_err());
    let call = ToolCallDto::new(ToolCallId::new(), "inspect", "{}").expect("valid call");
    let decoded: ToolCallDto =
        serde_json::from_str(&serde_json::to_string(&call).expect("call serializes"))
            .expect("call decodes");
    assert_eq!(decoded.arguments_json(), "{}");
    assert!(
        serde_json::from_str::<ToolCallDto>(
            r#"{"call_id":"bad","name":"x","arguments_json":"{}"}"#
        )
        .is_err()
    );

    assert_eq!(
        serde_json::from_str::<UsageDto>(r#"{"state":"not_reported"}"#).expect("state decodes"),
        UsageDto::NotReported
    );
    assert!(
        serde_json::from_str::<UsageDto>(
            r#"{"state":"reported","input_tokens":2,"output_tokens":3,"total_tokens":4}"#
        )
        .is_err()
    );

    let error = ProviderErrorDto::unavailable("failed", false, None).expect("valid error");
    assert_eq!(error.retry(), ErrorRetryDto::Never);
    assert!(serde_json::from_str::<ProviderErrorDto>(r#"{"code":"","retry":"never"}"#).is_err());
}
