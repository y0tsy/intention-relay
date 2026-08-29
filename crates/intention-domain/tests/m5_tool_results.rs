#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "M5 tool-result contract fixtures use expect for precise diagnostics."
)]

//! Typed, credential-free durable tool-result contract evidence.

use intention_domain::{
    DomainEventDto, ToolLifecycleEventDto, ToolLifecycleStatusDto, ToolResultMetadataEntryDto,
    ToolResultRecordedEventDto, ToolResultStatusDto,
};
use intention_types::{
    EventEnvelopeDto, EventId, RunId, SchemaVersionDto, SessionEventSequenceDto, SessionId,
    TimestampDto, ToolCallId,
};

fn time() -> TimestampDto {
    TimestampDto::from_unix_seconds(1_700_000_000).expect("fixture time is valid")
}

fn entry(key: &str, value: &str) -> ToolResultMetadataEntryDto {
    ToolResultMetadataEntryDto::new(key, value).expect("bounded metadata entry is valid")
}

fn record() -> ToolResultRecordedEventDto {
    ToolResultRecordedEventDto::new(
        SessionId::new(),
        RunId::new(),
        ToolCallId::new(),
        "read",
        ToolResultStatusDto::Completed,
        "bounded safe result",
        vec![entry("bytes", "17"), entry("truncated", "false")],
        time(),
    )
    .expect("bounded fixture record is valid")
}

#[test]
fn tool_result_record_round_trips_with_typed_identity_and_accessors() {
    let event = record();
    assert_eq!(event.tool_id(), "read");
    assert_eq!(event.status(), ToolResultStatusDto::Completed);
    assert_eq!(event.normalized_content(), "bounded safe result");
    assert_eq!(event.structured_metadata().len(), 2);
    assert_eq!(event.structured_metadata()[0].key(), "bytes");
    assert_eq!(event.structured_metadata()[0].value(), "17");
    assert_eq!(event.occurred_at(), time());

    let decoded: ToolResultRecordedEventDto =
        serde_json::from_str(&serde_json::to_string(&event).expect("record serializes"))
            .expect("record decodes");
    assert_eq!(decoded, event);

    let session_id = event.session_id();
    let payload = DomainEventDto::ToolResultRecorded(event);
    let envelope = EventEnvelopeDto::new(
        intention_types::EventMetadataDto::new(
            SchemaVersionDto::new(1, 0),
            EventId::new(),
            session_id,
            None,
            None,
            SessionEventSequenceDto::new(1),
            time(),
        ),
        payload,
    );
    let encoded = serde_json::to_string(&envelope).expect("envelope serializes");
    let decoded: EventEnvelopeDto<DomainEventDto> =
        serde_json::from_str(&encoded).expect("envelope decodes");
    assert_eq!(decoded.session_id(), session_id);
    assert_eq!(decoded.payload(), envelope.payload());
    assert!(matches!(
        decoded.payload(),
        DomainEventDto::ToolResultRecorded(_)
    ));
}

#[test]
fn tool_result_record_validates_identity_safety_and_metadata_uniqueness() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let call_id = ToolCallId::new();

    assert!(
        ToolResultRecordedEventDto::new(
            session_id,
            run_id,
            call_id,
            " ",
            ToolResultStatusDto::Completed,
            "content",
            Vec::new(),
            time(),
        )
        .is_err()
    );
    // An empty normalized result is legitimate; content is not size-bounded.
    assert!(
        ToolResultRecordedEventDto::new(
            session_id,
            run_id,
            call_id,
            "read",
            ToolResultStatusDto::Failed,
            "",
            Vec::new(),
            time(),
        )
        .is_ok()
    );
    assert!(
        ToolResultRecordedEventDto::new(
            session_id,
            run_id,
            call_id,
            "read",
            ToolResultStatusDto::Completed,
            "x".repeat(4 * 1024),
            Vec::new(),
            time(),
        )
        .is_ok()
    );
    // Content beyond the former 4 KiB cap is accepted.
    assert!(
        ToolResultRecordedEventDto::new(
            session_id,
            run_id,
            call_id,
            "read",
            ToolResultStatusDto::Completed,
            "x".repeat(4 * 1024 + 1),
            Vec::new(),
            time(),
        )
        .is_ok()
    );
    assert!(
        ToolResultRecordedEventDto::new(
            session_id,
            run_id,
            call_id,
            "read",
            ToolResultStatusDto::Completed,
            "bad\0content",
            Vec::new(),
            time(),
        )
        .is_err()
    );

    let bounded_metadata = || -> Vec<ToolResultMetadataEntryDto> {
        (0..16)
            .map(|index| entry(&format!("key{index}"), "v"))
            .collect()
    };
    assert!(
        ToolResultRecordedEventDto::new(
            session_id,
            run_id,
            call_id,
            "read",
            ToolResultStatusDto::Completed,
            "content",
            bounded_metadata(),
            time(),
        )
        .is_ok()
    );
    let oversized_metadata = || -> Vec<ToolResultMetadataEntryDto> {
        (0..17)
            .map(|index| entry(&format!("key{index}"), "v"))
            .collect()
    };
    // Metadata beyond the former 16-entry cap is accepted.
    assert!(
        ToolResultRecordedEventDto::new(
            session_id,
            run_id,
            call_id,
            "read",
            ToolResultStatusDto::Completed,
            "content",
            oversized_metadata(),
            time(),
        )
        .is_ok()
    );
    assert!(
        ToolResultRecordedEventDto::new(
            session_id,
            run_id,
            call_id,
            "read",
            ToolResultStatusDto::Completed,
            "content",
            vec![entry("key", "first"), entry("key", "second")],
            time(),
        )
        .is_err()
    );

    assert!(ToolResultMetadataEntryDto::new(" ", "v").is_err());
    assert!(ToolResultMetadataEntryDto::new("k", "bad\0value").is_err());
    assert!(ToolResultMetadataEntryDto::new("x".repeat(128), "v").is_ok());
    // Keys and values beyond the former 128-byte and 1 KiB caps are accepted.
    assert!(ToolResultMetadataEntryDto::new("x".repeat(129), "v").is_ok());
    assert!(ToolResultMetadataEntryDto::new("k", "x".repeat(1024)).is_ok());
    assert!(ToolResultMetadataEntryDto::new("k", "x".repeat(1025)).is_ok());
}

#[test]
fn tool_result_wire_shape_is_closed_safe_and_additive_tolerant() {
    let event = record();
    let encoded: serde_json::Value =
        serde_json::to_value(&event).expect("record serializes to JSON");

    // The durable shape is closed: exactly the documented typed fields persist,
    // so no credential, config path, raw error, or extra payload can appear.
    let expected = serde_json::json!({
        "session_id": event.session_id(),
        "run_id": event.run_id(),
        "call_id": event.call_id(),
        "tool_id": "read",
        "status": "completed",
        "normalized_content": "bounded safe result",
        "structured_metadata": [
            {"key": "bytes", "value": "17"},
            {"key": "truncated", "value": "false"}
        ],
        "occurred_at": 1_700_000_000,
    });
    assert_eq!(encoded, expected);

    let valid = |value: serde_json::Value| {
        serde_json::from_value::<ToolResultRecordedEventDto>(value).is_ok()
    };
    let mut additive = expected.clone();
    additive["future_additive_field"] = serde_json::json!(true);
    assert!(valid(additive.clone()));
    // The metadata list is optional on the wire and defaults to empty.
    let mut without_metadata = expected.clone();
    without_metadata
        .as_object_mut()
        .expect("object")
        .remove("structured_metadata");
    assert!(valid(without_metadata));

    // Validated wire decoding: persisted safety constraints cannot be bypassed.
    let mut oversize = expected.clone();
    oversize["normalized_content"] = serde_json::json!("x".repeat(4 * 1024 + 1));
    assert!(valid(oversize));
    let mut nul = expected.clone();
    nul["normalized_content"] = serde_json::json!("bad\0content");
    assert!(!valid(nul));
    let mut unknown_status = expected.clone();
    unknown_status["status"] = serde_json::json!("started");
    assert!(!valid(unknown_status));
    let mut negative_time = expected;
    negative_time["occurred_at"] = serde_json::json!(-1);
    assert!(!valid(negative_time));
    let mut missing_identity = additive;
    missing_identity
        .as_object_mut()
        .expect("object")
        .remove("call_id");
    assert!(!valid(missing_identity));
}

#[test]
fn additive_tool_result_taxonomy_preserves_prior_event_variants() {
    let session_id = SessionId::new();
    let prior = DomainEventDto::ToolLifecycle(
        ToolLifecycleEventDto::new(
            session_id,
            RunId::new(),
            ToolCallId::new(),
            "read",
            ToolLifecycleStatusDto::Started,
            "bounded detail",
            time(),
        )
        .expect("lifecycle event is valid"),
    );
    let added = DomainEventDto::ToolResultRecorded(record());

    for payload in [prior, added] {
        let envelope = EventEnvelopeDto::new(
            intention_types::EventMetadataDto::new(
                SchemaVersionDto::new(1, 0),
                EventId::new(),
                session_id,
                None,
                None,
                SessionEventSequenceDto::new(1),
                time(),
            ),
            payload,
        );
        let encoded = serde_json::to_string(&envelope).expect("envelope serializes");
        let decoded: EventEnvelopeDto<DomainEventDto> =
            serde_json::from_str(&encoded).expect("envelope decodes");
        assert_eq!(decoded.payload(), envelope.payload());
    }
}
