//! Slice 2 per-turn provider override contract tests.
//!
//! The override fields are additive: the three-argument constructor still
//! works, legacy wire JSON without the new fields still deserializes, and the
//! bounded override validation rejects invalid or credential-shaped values.

#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect for precise failure diagnostics."
)]

use intention_domain::SendUserTurnCommandDto;
use intention_types::{SessionId, TurnId};

fn fixture_turn() -> SendUserTurnCommandDto {
    SendUserTurnCommandDto::new(SessionId::new(), TurnId::new(), "fixture turn")
        .expect("fixture turn is valid")
}

#[test]
fn three_argument_constructor_keeps_working_without_overrides() {
    let turn = fixture_turn();
    assert!(turn.profile_override().is_none());
    assert!(turn.expected_profile_revision().is_none());
}

#[test]
fn override_builder_binds_both_values() {
    let turn = fixture_turn()
        .with_profile_override("default", Some("rev-1".to_owned()))
        .expect("override binding is valid");
    assert_eq!(turn.profile_override(), Some("default"));
    assert_eq!(turn.expected_profile_revision(), Some("rev-1"));
}

#[test]
fn override_builder_accepts_an_override_without_an_expected_revision() {
    let turn = fixture_turn()
        .with_profile_override("default", None)
        .expect("override without revision is valid");
    assert_eq!(turn.profile_override(), Some("default"));
    assert!(turn.expected_profile_revision().is_none());
}

#[test]
fn expected_revision_without_override_is_rejected() {
    let error = fixture_turn()
        .with_profile_override("", Some("rev-1".to_owned()))
        .expect_err("expected revision without override must fail");
    assert_eq!(error.code(), "provider_profile_override_invalid");
}

#[test]
fn overlong_override_values_are_rejected() {
    let overlong = "p".repeat(64);
    let error = fixture_turn()
        .with_profile_override(overlong.clone(), None)
        .expect_err("overlong override must fail");
    assert_eq!(error.code(), "provider_profile_override_invalid");
    let error = fixture_turn()
        .with_profile_override("default", Some(overlong))
        .expect_err("overlong expected revision must fail");
    assert_eq!(error.code(), "provider_profile_override_invalid");
}

#[test]
fn blank_override_is_rejected() {
    let error = fixture_turn()
        .with_profile_override("   ", None)
        .expect_err("blank override must fail");
    assert_eq!(error.code(), "provider_profile_override_invalid");
}

#[test]
fn control_characters_in_override_values_are_rejected() {
    let error = fixture_turn()
        .with_profile_override("default\n", None)
        .expect_err("control-bearing override must fail");
    assert_eq!(error.code(), "provider_profile_override_invalid");
}

#[test]
fn credential_shaped_override_values_are_rejected() {
    for credential in [
        "profile-api_key",
        "sk-secret-profile",
        "token=abc",
        "password=def",
        "apikey-profile",
    ] {
        let error = fixture_turn()
            .with_profile_override(credential, None)
            .expect_err("credential-shaped override must fail");
        assert_eq!(error.code(), "credentials_forbidden");
    }
    let error = fixture_turn()
        .with_profile_override("default", Some("api_key-rev".to_owned()))
        .expect_err("credential-shaped expected revision must fail");
    assert_eq!(error.code(), "credentials_forbidden");
}

#[test]
fn legacy_wire_json_without_override_fields_deserializes() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let wire =
        format!(r#"{{"session_id":"{session_id}","turn_id":"{turn_id}","content":"hello"}}"#);
    let decoded: SendUserTurnCommandDto =
        serde_json::from_str(&wire).expect("legacy wire JSON deserializes");
    assert_eq!(decoded.session_id(), session_id);
    assert_eq!(decoded.turn_id(), turn_id);
    assert_eq!(decoded.content(), "hello");
    assert!(decoded.profile_override().is_none());
    assert!(decoded.expected_profile_revision().is_none());
}

#[test]
fn wire_json_with_override_fields_round_trips() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let wire = format!(
        r#"{{"session_id":"{session_id}","turn_id":"{turn_id}","content":"hello","profile_override":"default","expected_profile_revision":"rev-1"}}"#
    );
    let decoded: SendUserTurnCommandDto =
        serde_json::from_str(&wire).expect("override wire JSON deserializes");
    assert_eq!(decoded.profile_override(), Some("default"));
    assert_eq!(decoded.expected_profile_revision(), Some("rev-1"));
    let encoded = serde_json::to_string(&decoded).expect("override command serializes");
    let round: SendUserTurnCommandDto =
        serde_json::from_str(&encoded).expect("override command round-trips");
    assert_eq!(round, decoded);
}

#[test]
fn wire_json_with_expected_revision_but_no_override_is_rejected() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let wire = format!(
        r#"{{"session_id":"{session_id}","turn_id":"{turn_id}","content":"hello","expected_profile_revision":"rev-1"}}"#
    );
    let error: Result<SendUserTurnCommandDto, _> = serde_json::from_str(&wire);
    assert!(
        error.is_err(),
        "expected revision without override must fail wire decode"
    );
}
