#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Test-first protocol contract and compatibility evidence.

use intention_domain::{DomainEventDto, RunStatusChangedEventDto, RunStatusDto};
use intention_protocol::{
    DaemonHealthDto, DaemonReadinessDto, ProtocolCapabilityDto, ProtocolHelloDto,
    ProtocolMessageDto, ProtocolQueryDto, ProtocolRequestEnvelopeDto, ProtocolRequestPayloadDto,
    ProtocolVersionDto, SessionEventTailBatchDto, SessionSnapshotDto,
    SessionSubscriptionResponseDto, SubscribeSessionCommandDto,
};
use intention_types::{
    CorrelationIdDto, EventEnvelopeDto, EventId, EventMetadataDto, SchemaVersionDto,
    SessionEventSequenceDto, SessionId, TimestampDto,
};

fn fixture_event(session_id: SessionId, sequence: u64) -> EventEnvelopeDto<DomainEventDto> {
    let occurred_at = TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid");
    EventEnvelopeDto::new(
        EventMetadataDto::new(
            SchemaVersionDto::new(1, 0),
            EventId::new(),
            session_id,
            None,
            None,
            SessionEventSequenceDto::new(sequence),
            occurred_at,
        ),
        DomainEventDto::RunStatusChanged(RunStatusChangedEventDto::new(
            session_id,
            intention_types::RunId::new(),
            RunStatusDto::Running,
            occurred_at,
        )),
    )
}

#[test]
fn protocol_hello_round_trips_with_versioned_capabilities() {
    let hello = ProtocolHelloDto::new(
        ProtocolVersionDto::new(1, 0),
        vec![
            ProtocolCapabilityDto::SessionSubscriptions,
            ProtocolCapabilityDto::CorrelatedRequests,
            ProtocolCapabilityDto::DaemonHealth,
        ],
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
fn correlated_versioned_requests_round_trip_with_typed_query_payloads() {
    let correlation = CorrelationIdDto::parse("11111111-1111-4111-8111-111111111111")
        .expect("fixture correlation is canonical");
    let request = ProtocolRequestEnvelopeDto::new(
        ProtocolVersionDto::new(1, 0),
        correlation,
        ProtocolMessageDto::new(
            SchemaVersionDto::new(1, 0),
            ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
        ),
    );

    let encoded = serde_json::to_string(&request).expect("test serialization must succeed");
    let decoded: ProtocolRequestEnvelopeDto =
        serde_json::from_str(&encoded).expect("test deserialization must succeed");

    assert_eq!(decoded, request);
    assert_eq!(decoded.correlation_id(), correlation);
}

#[test]
fn daemon_health_and_snapshot_tail_contracts_round_trip() {
    let schema = SchemaVersionDto::new(1, 0);
    let health = DaemonHealthDto::new(
        schema,
        ProtocolVersionDto::new(1, 0),
        DaemonReadinessDto::Ready,
    );
    assert_eq!(
        serde_json::from_str::<DaemonHealthDto>(
            &serde_json::to_string(&health).expect("health serializes")
        )
        .expect("health deserializes"),
        health
    );

    let session_id = SessionId::new();
    let snapshot = SessionSnapshotDto::new(schema, session_id, SessionEventSequenceDto::new(2));
    let tail = SessionEventTailBatchDto::new(
        schema,
        session_id,
        snapshot.at_sequence(),
        vec![fixture_event(session_id, 3)],
    )
    .expect("contiguous tail is valid");
    let response = SessionSubscriptionResponseDto::snapshot_and_tail(snapshot, tail)
        .expect("matching snapshot and tail are valid");

    let encoded = serde_json::to_string(&response).expect("response serializes");
    let decoded: SessionSubscriptionResponseDto =
        serde_json::from_str(&encoded).expect("response deserializes");
    assert_eq!(decoded, response);
}

#[test]
fn subscribe_command_keeps_legacy_shape_and_adds_optional_run_scope() {
    let command = SubscribeSessionCommandDto::with_run_id(
        SchemaVersionDto::new(1, 0),
        SessionId::new(),
        Some(intention_types::RunId::new()),
        Some(SessionEventSequenceDto::new(3)),
        intention_domain::RunModeDto::Build,
    );

    let encoded = serde_json::to_string(&command).expect("test serialization must succeed");
    let decoded: SubscribeSessionCommandDto =
        serde_json::from_str(&encoded).expect("test deserialization must succeed");

    assert_eq!(decoded, command);
    assert_eq!(
        decoded.requested_mode(),
        intention_domain::RunModeDto::Build
    );
}
