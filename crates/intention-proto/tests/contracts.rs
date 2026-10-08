#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Test-first contract evidence for the planned `intention-proto` public API.

use intention_proto::{ErrorCategoryDto, SessionId, TimestampDto};

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
