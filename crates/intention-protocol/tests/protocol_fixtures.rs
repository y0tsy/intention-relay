#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Versioned local-protocol fixture compatibility evidence.

use intention_domain::RunModeDto;
use intention_protocol::{
    DaemonHealthDto, DaemonReadinessDto, ProtocolAcceptedDto, ProtocolHelloDto,
    ProtocolRequestEnvelopeDto, ProtocolVersionDto, SessionSnapshotDto, SubscribeSessionCommandDto,
};
use intention_types::{CorrelationIdDto, SchemaVersionDto};

#[test]
fn protocol_fixtures_decode_and_round_trip() {
    let hello: ProtocolHelloDto =
        serde_json::from_str(include_str!("fixtures/protocol-hello-v1.json"))
            .expect("protocol hello fixture must decode");
    assert_eq!(hello.version(), ProtocolVersionDto::new(1, 0));
    assert_eq!(hello.adapter_name(), "fixture-tui");

    // This v1 fixture intentionally has no `run_id`; that additive M2 field
    // must remain absent when older clients serialize a subscription command.
    let subscription: SubscribeSessionCommandDto =
        serde_json::from_str(include_str!("fixtures/protocol-subscribe-session-v1.json"))
            .expect("legacy subscription fixture must decode");
    assert_eq!(subscription.schema_version(), SchemaVersionDto::new(1, 0));
    assert_eq!(subscription.run_id(), None);
    assert_eq!(subscription.requested_mode(), RunModeDto::Build);

    let run_scoped_subscription: SubscribeSessionCommandDto = serde_json::from_str(include_str!(
        "fixtures/protocol-subscribe-session-run-v1.json"
    ))
    .expect("run-scoped subscription fixture must decode");
    assert!(run_scoped_subscription.run_id().is_some());

    let accepted: ProtocolAcceptedDto =
        serde_json::from_str(include_str!("fixtures/protocol-accepted-v1.json"))
            .expect("accepted fixture must decode");
    assert_eq!(
        accepted.correlation_id(),
        CorrelationIdDto::parse("11111111-1111-4111-8111-111111111111")
            .expect("fixture correlation is canonical")
    );

    let request: ProtocolRequestEnvelopeDto =
        serde_json::from_str(include_str!("fixtures/protocol-request-envelope-v1.json"))
            .expect("request envelope fixture must decode");
    assert_eq!(request.protocol_version(), ProtocolVersionDto::new(1, 0));
    assert_eq!(
        request.message().schema_version(),
        SchemaVersionDto::new(1, 0)
    );

    let snapshot: SessionSnapshotDto =
        serde_json::from_str(include_str!("fixtures/protocol-session-snapshot-v1.json"))
            .expect("session snapshot fixture must decode");
    assert_eq!(snapshot.schema_version(), SchemaVersionDto::new(1, 0));
    assert_eq!(snapshot.at_sequence().value(), 4);

    let health: DaemonHealthDto =
        serde_json::from_str(include_str!("fixtures/protocol-daemon-health-v1.json"))
            .expect("daemon health fixture must decode");
    assert_eq!(health.readiness(), DaemonReadinessDto::Ready);
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
        r#"{"correlation_id":"not-a-canonical-uuid"}"#,
        r#"{"protocol_version":{"major":1,"minor":0},"correlation_id":"not-a-canonical-uuid","message":{"schema_version":{"major":1,"minor":0},"payload":{"kind":"query","data":{"kind":"get_daemon_health"}}}}"#,
    ] {
        if wire.contains("correlation_id") && wire.contains("protocol_version") {
            assert!(serde_json::from_str::<ProtocolRequestEnvelopeDto>(wire).is_err());
        } else if wire.contains("correlation_id") {
            assert!(serde_json::from_str::<ProtocolAcceptedDto>(wire).is_err());
        } else {
            assert!(serde_json::from_str::<SubscribeSessionCommandDto>(wire).is_err());
        }
    }
}

#[test]
fn additive_protocol_fields_remain_compatible() {
    let hello: ProtocolHelloDto = serde_json::from_str(
        r#"{"version":{"major":1,"minor":0},"capabilities":[],"adapter_name":"fixture","future_additive":true}"#,
    )
    .expect("unknown additive protocol fields are ignored for compatibility");
    assert_eq!(hello.adapter_name(), "fixture");
}
