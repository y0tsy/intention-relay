#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Test-first protocol contract and compatibility evidence.

use intention_domain::RunModeDto;
use intention_protocol::{
    ProtocolCapabilityDto, ProtocolHelloDto, ProtocolVersionDto, SubscribeSessionCommandDto,
};
use intention_types::{SchemaVersionDto, SessionEventSequenceDto, SessionId};

#[test]
fn protocol_hello_round_trips_with_versioned_capabilities() {
    let hello = ProtocolHelloDto::new(
        ProtocolVersionDto::new(1, 0),
        vec![ProtocolCapabilityDto::SessionSubscriptions],
        "fixture-tui",
    )
    .expect("fixture hello is valid");

    let encoded = serde_json::to_string(&hello).expect("test serialization must succeed");
    let decoded: ProtocolHelloDto =
        serde_json::from_str(&encoded).expect("test deserialization must succeed");

    assert_eq!(decoded, hello);
}

#[test]
fn incompatible_protocol_major_returns_a_typed_unavailable_error() {
    let server = ProtocolVersionDto::new(1, 0);
    let client = ProtocolVersionDto::new(2, 0);

    let error = server
        .ensure_compatible_with(client)
        .expect_err("different major versions must be rejected");

    assert_eq!(error.code(), "incompatible_protocol_version");
}

#[test]
fn subscribe_command_uses_typed_session_identity_and_schema_version() {
    let command = SubscribeSessionCommandDto::new(
        SchemaVersionDto::new(1, 0),
        SessionId::new(),
        Some(SessionEventSequenceDto::new(3)),
        RunModeDto::Build,
    );

    let encoded = serde_json::to_string(&command).expect("test serialization must succeed");
    let decoded: SubscribeSessionCommandDto =
        serde_json::from_str(&encoded).expect("test deserialization must succeed");

    assert_eq!(decoded, command);
}
