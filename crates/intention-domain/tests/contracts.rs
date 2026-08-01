#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Test-first domain DTO and invariant evidence.

use intention_domain::{
    DomainEventDto, RunModeDto, SendUserTurnCommandDto, SessionCreatedEventDto, WorkspaceRootDto,
};
use intention_types::{ProjectId, SessionId, TimestampDto, TurnId};

#[test]
fn send_turn_requires_a_non_empty_message_and_typed_session_identity() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let command = SendUserTurnCommandDto::new(session_id, turn_id, "Explain M1")
        .expect("non-empty fixture message is valid");

    assert_eq!(command.session_id(), session_id);
    assert_eq!(command.turn_id(), turn_id);
    assert_eq!(command.content(), "Explain M1");
    assert!(SendUserTurnCommandDto::new(SessionId::new(), TurnId::new(), "   ").is_err());
}

#[test]
fn workspace_root_and_turn_wire_values_enforce_validation() {
    assert!(WorkspaceRootDto::parse("/workspace/project").is_ok());
    assert!(WorkspaceRootDto::parse("").is_err());
    assert!(WorkspaceRootDto::parse("relative/project").is_err());
    assert!(serde_json::from_str::<WorkspaceRootDto>(r#""relative/project""#).is_err());

    assert!(serde_json::from_str::<SendUserTurnCommandDto>(
        r#"{"session_id":"11111111-1111-4111-8111-111111111111","turn_id":"22222222-2222-4222-8222-222222222222","content":" "}"#
    )
    .is_err());
}

#[test]
fn domain_events_round_trip_with_typed_identity_and_mode() {
    let event = DomainEventDto::SessionCreated(SessionCreatedEventDto::new(
        ProjectId::new(),
        SessionId::new(),
        WorkspaceRootDto::parse("/workspace/project").expect("fixture root is absolute"),
        RunModeDto::Plan,
        TimestampDto::from_unix_seconds(1_700_000_000).expect("fixture timestamp is valid"),
    ));

    let encoded = serde_json::to_string(&event).expect("test serialization must succeed");
    let decoded: DomainEventDto =
        serde_json::from_str(&encoded).expect("test deserialization must succeed");

    assert_eq!(decoded, event);
}
