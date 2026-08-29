#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Versioned persisted-event fixture compatibility evidence.

use intention_domain::{DomainEventDto, ToolLifecycleEventDto, ToolLifecycleStatusDto};
use intention_types::{EventEnvelopeDto, RunId, SessionId, TimestampDto, ToolCallId};

fn native_event_fixture() -> String {
    let workspace_root = serde_json::to_string(
        &std::env::temp_dir()
            .join("intention-domain-event-fixture-workspace")
            .to_string_lossy(),
    )
    .expect("native fixture workspace serializes");
    include_str!("fixtures/event-envelope-v1.json").replace(
        "\"workspace_root\": \"/workspace/project\"",
        &format!("\"workspace_root\": {workspace_root}"),
    )
}

#[test]
fn persisted_event_fixture_decodes_and_round_trips() {
    let fixture = native_event_fixture();
    let envelope: EventEnvelopeDto<DomainEventDto> =
        serde_json::from_str(&fixture).expect("persisted event fixture must decode");

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

    let compatible = native_event_fixture().replacen("\n}", ",\n  \"future_additive\": true\n}", 1);
    assert!(serde_json::from_str::<EventEnvelopeDto<DomainEventDto>>(&compatible).is_ok());
}

#[test]
fn tool_lifecycle_event_round_trips_and_rejects_unsafe_detail() {
    let event = ToolLifecycleEventDto::new(
        SessionId::new(),
        RunId::new(),
        ToolCallId::new(),
        "read",
        ToolLifecycleStatusDto::Completed,
        "bounded result",
        TimestampDto::from_unix_seconds(1_700_000_000).expect("timestamp"),
    )
    .expect("event");
    let payload = DomainEventDto::ToolLifecycle(event.clone());
    let encoded = serde_json::to_string(&payload).expect("serialize event");
    assert_eq!(
        serde_json::from_str::<DomainEventDto>(&encoded).expect("decode event"),
        payload
    );
    assert!(
        ToolLifecycleEventDto::new(
            SessionId::new(),
            RunId::new(),
            ToolCallId::new(),
            "read",
            ToolLifecycleStatusDto::Failed,
            "bad\0detail",
            TimestampDto::from_unix_seconds(1_700_000_000).expect("timestamp")
        )
        .is_err()
    );
    assert!(
        ToolLifecycleEventDto::new(
            SessionId::new(),
            RunId::new(),
            ToolCallId::new(),
            " ",
            ToolLifecycleStatusDto::Admitted,
            "detail",
            TimestampDto::from_unix_seconds(1_700_000_000).expect("timestamp"),
        )
        .is_err()
    );
    // Detail beyond the former 4 KiB cap is accepted; NUL content is not.
    assert!(
        ToolLifecycleEventDto::new(
            SessionId::new(),
            RunId::new(),
            ToolCallId::new(),
            "read",
            ToolLifecycleStatusDto::Completed,
            "x".repeat(4097),
            TimestampDto::from_unix_seconds(1_700_000_000).expect("timestamp"),
        )
        .is_ok()
    );
    assert_eq!(event.session_id(), event.session_id());
    assert_eq!(event.run_id(), event.run_id());
    assert_eq!(event.call_id(), event.call_id());
    assert_eq!(event.tool_id(), "read");
    assert_eq!(event.status(), &ToolLifecycleStatusDto::Completed);
    assert_eq!(event.detail(), "bounded result");
    assert_eq!(event.occurred_at().unix_seconds(), 1_700_000_000);
}
