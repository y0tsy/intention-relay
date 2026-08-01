#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Versioned persisted-event fixture compatibility evidence.

use intention_domain::DomainEventDto;
use intention_types::EventEnvelopeDto;

#[test]
fn persisted_event_fixture_decodes_and_round_trips() {
    let fixture = include_str!("fixtures/event-envelope-v1.json");
    let envelope: EventEnvelopeDto<DomainEventDto> =
        serde_json::from_str(fixture).expect("persisted event fixture must decode");

    assert_eq!(envelope.schema_version().major(), 1);
    assert_eq!(envelope.sequence().value(), 1);
    assert!(matches!(
        envelope.payload(),
        DomainEventDto::SessionCreated(_)
    ));

    let encoded = serde_json::to_string(&envelope).expect("test serialization must succeed");
    let decoded: EventEnvelopeDto<DomainEventDto> =
        serde_json::from_str(&encoded).expect("test deserialization must succeed");
    assert_eq!(decoded, envelope);
}

#[test]
fn domain_event_wire_contract_rejects_invalid_shapes_and_accepts_additive_fields() {
    for wire in [
        r#"{"schema_version":{"major":1,"minor":0},"event_id":"not-an-id"}"#,
        r#"{"schema_version":{"major":1,"minor":0},"event_id":"11111111-1111-4111-8111-111111111111","session_id":"22222222-2222-4222-8222-222222222222","sequence":1,"occurred_at":1700000000}"#,
        r#"{"schema_version":{"major":1,"minor":0},"event_id":"11111111-1111-4111-8111-111111111111","session_id":"22222222-2222-4222-8222-222222222222","run_id":null,"turn_id":null,"sequence":1,"occurred_at":1700000000,"payload":{"kind":"unknown_event","data":{}}}"#,
    ] {
        assert!(serde_json::from_str::<EventEnvelopeDto<DomainEventDto>>(wire).is_err());
    }

    let compatible = include_str!("fixtures/event-envelope-v1.json").replacen(
        "\n}",
        ",\n  \"future_additive\": true\n}",
        1,
    );
    assert!(serde_json::from_str::<EventEnvelopeDto<DomainEventDto>>(&compatible).is_ok());
}
