#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Versioned local-protocol fixture compatibility evidence.

use intention_domain::RunModeDto;
use intention_protocol::{
    ProtocolAcceptedDto, ProtocolHelloDto, ProtocolVersionDto, SubscribeSessionCommandDto,
};
use intention_types::SchemaVersionDto;

#[test]
fn protocol_fixtures_decode_and_round_trip() {
    let hello: ProtocolHelloDto =
        serde_json::from_str(include_str!("fixtures/protocol-hello-v1.json"))
            .expect("protocol hello fixture must decode");
    assert_eq!(hello.version(), ProtocolVersionDto::new(1, 0));
    assert_eq!(hello.adapter_name(), "fixture-tui");

    let subscription: SubscribeSessionCommandDto =
        serde_json::from_str(include_str!("fixtures/protocol-subscribe-session-v1.json"))
            .expect("subscription fixture must decode");
    assert_eq!(subscription.schema_version(), SchemaVersionDto::new(1, 0));
    assert_eq!(subscription.requested_mode(), RunModeDto::Build);

    let encoded =
        serde_json::to_string(&subscription).expect("subscription serialization must succeed");
    let decoded: SubscribeSessionCommandDto =
        serde_json::from_str(&encoded).expect("subscription deserialization must succeed");
    assert_eq!(decoded, subscription);
}

#[test]
fn malformed_protocol_wire_shapes_are_rejected() {
    for wire in [
        r#"{"version":{"major":1,"minor":0},"capabilities":[],"adapter_name":""}"#,
        r#"{"version":{"major":"one","minor":0},"capabilities":[],"adapter_name":"fixture"}"#,
        r#"{"version":{"major":1,"minor":0},"capabilities":["unknown"],"adapter_name":"fixture"}"#,
    ] {
        assert!(serde_json::from_str::<ProtocolHelloDto>(wire).is_err());
    }
    for wire in [
        r#"{"schema_version":{"major":1,"minor":0},"session_id":"not-an-id","after_sequence":3,"requested_mode":"build"}"#,
        r#"{"schema_version":{"major":1,"minor":0},"session_id":"11111111-1111-4111-8111-111111111111","after_sequence":3}"#,
    ] {
        assert!(serde_json::from_str::<SubscribeSessionCommandDto>(wire).is_err());
    }
    assert!(serde_json::from_str::<ProtocolAcceptedDto>(r#"{"correlation_id":" "}"#).is_err());
}

#[test]
fn additive_protocol_fields_remain_compatible() {
    let hello: ProtocolHelloDto = serde_json::from_str(
        r#"{"version":{"major":1,"minor":0},"capabilities":[],"adapter_name":"fixture","future_additive":true}"#,
    )
    .expect("unknown additive protocol fields are ignored for compatibility");
    assert_eq!(hello.adapter_name(), "fixture");
}
