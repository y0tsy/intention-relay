#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Test-first contract evidence for the planned `intention-types` public API.

use intention_types::{
    ErrorCategoryDto, ErrorDto, ErrorRetryDto, EventEnvelopeDto, EventId, EventMetadataDto,
    PageCursorDto, PageRequestDto, RunId, SchemaVersionDto, SessionEventSequenceDto, SessionId,
    TimestampDto,
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
fn event_envelopes_round_trip_with_causal_identity() {
    let envelope = EventEnvelopeDto::new(
        EventMetadataDto::new(
            SchemaVersionDto::new(1, 0),
            EventId::new(),
            SessionId::new(),
            Some(RunId::new()),
            None,
            SessionEventSequenceDto::new(7),
            TimestampDto::from_unix_seconds(1_700_000_000).expect("fixture timestamp is valid"),
        ),
        "fixture-event".to_owned(),
    );

    let encoded = serde_json::to_string(&envelope).expect("test serialization must succeed");
    let decoded: EventEnvelopeDto<String> =
        serde_json::from_str(&encoded).expect("test deserialization must succeed");

    assert_eq!(decoded, envelope);
}

#[test]
fn validated_value_dtos_reject_invalid_wire_values() {
    assert!(serde_json::from_str::<TimestampDto>("-1").is_err());
    assert!(serde_json::from_str::<PageCursorDto>(r#"" ""#).is_err());
    assert!(serde_json::from_str::<PageRequestDto>(r#"{"limit":0}"#).is_err());
}

#[test]
fn safe_errors_round_trip_without_internal_implementation_details() {
    let error = ErrorDto::new(
        "invalid_config",
        ErrorCategoryDto::Validation,
        "configuration is invalid",
        ErrorRetryDto::Manual,
        None,
    )
    .expect("fixture error is valid");

    let encoded = serde_json::to_string(&error).expect("test serialization must succeed");
    let decoded: ErrorDto =
        serde_json::from_str(&encoded).expect("test deserialization must succeed");

    assert_eq!(decoded, error);
    assert!(!encoded.contains("backtrace"));
}
