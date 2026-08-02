#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Test-first protocol contract and compatibility evidence.

use intention_domain::{
    DomainEventDto, GetSessionSnapshotQueryDto, RunModeDto, RunStatusChangedEventDto, RunStatusDto,
    SendUserTurnCommandDto, StopRunCommandDto,
};
use intention_protocol::{
    DaemonHealthDto, DaemonReadinessDto, ProtocolAcceptedDto, ProtocolCapabilityDto,
    ProtocolCommandDto, ProtocolCommandResultDto, ProtocolHelloDto, ProtocolMessageDto,
    ProtocolQueryDto, ProtocolQueryResultDto, ProtocolRequestEnvelopeDto,
    ProtocolRequestPayloadDto, ProtocolResponseEnvelopeDto, ProtocolResponsePayloadDto,
    ProtocolVersionDto, SessionEventTailBatchDto, SessionResyncDto, SessionResyncReasonDto,
    SessionSnapshotDto, SessionSubscriptionResponseDto, SubscribeSessionCommandDto,
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

#[test]
fn protocol_request_and_response_variants_round_trip_through_wire_envelopes() {
    let schema = SchemaVersionDto::new(1, 0);
    let session_id = SessionId::new();
    let correlation_id = CorrelationIdDto::new();
    let request_variants = [
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, intention_types::TurnId::new(), "hello")
                .expect("fixture turn is valid"),
        )),
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::StopRun(StopRunCommandDto::new(
            session_id,
            intention_types::RunId::new(),
        ))),
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SubscribeSession(
            SubscribeSessionCommandDto::new(schema, session_id, None, RunModeDto::Build),
        )),
        ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
        ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetSessionSnapshot(
            GetSessionSnapshotQueryDto::new(session_id),
        )),
    ];

    for payload in request_variants {
        let envelope = ProtocolRequestEnvelopeDto::new(
            ProtocolVersionDto::new(1, 0),
            correlation_id,
            ProtocolMessageDto::new(schema, payload),
        );
        let encoded = serde_json::to_string(&envelope).expect("request envelope serializes");
        let decoded: ProtocolRequestEnvelopeDto =
            serde_json::from_str(&encoded).expect("request envelope deserializes");
        assert_eq!(decoded, envelope);
    }

    let snapshot = SessionSnapshotDto::new(schema, session_id, SessionEventSequenceDto::new(0));
    let response_variants = [
        ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(
            ProtocolAcceptedDto::new(correlation_id),
        )),
        ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Rejected(
            intention_types::ErrorDto::validation("fixture_rejected", "fixture rejection"),
        )),
        ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(
            DaemonHealthDto::new(
                schema,
                ProtocolVersionDto::new(1, 0),
                DaemonReadinessDto::Draining,
            ),
        )),
        ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::SessionSnapshot(snapshot)),
        ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(
            intention_types::ErrorDto::validation("fixture_query_rejected", "fixture rejection"),
        )),
        ProtocolResponsePayloadDto::Subscription(SessionSubscriptionResponseDto::resync_required(
            SessionResyncDto::new(
                schema,
                session_id,
                SessionResyncReasonDto::HistoryUnavailable,
            ),
        )),
    ];

    for payload in response_variants {
        let envelope = ProtocolResponseEnvelopeDto::new(
            ProtocolVersionDto::new(1, 0),
            correlation_id,
            ProtocolMessageDto::new(schema, payload),
        );
        let encoded = serde_json::to_string(&envelope).expect("response envelope serializes");
        let decoded: ProtocolResponseEnvelopeDto =
            serde_json::from_str(&encoded).expect("response envelope deserializes");
        assert_eq!(decoded, envelope);
    }
}

#[test]
fn event_tails_and_subscription_responses_reject_mismatched_boundaries() {
    let schema = SchemaVersionDto::new(1, 0);
    let session_id = SessionId::new();
    let other_session_id = SessionId::new();
    let overflow = SessionEventTailBatchDto::new(
        schema,
        session_id,
        SessionEventSequenceDto::new(u64::MAX),
        vec![fixture_event(session_id, 0)],
    )
    .expect_err("a tail cannot continue after the maximum sequence");
    assert_eq!(overflow.code(), "invalid_event_tail");

    let session_mismatch = SessionEventTailBatchDto::new(
        schema,
        session_id,
        SessionEventSequenceDto::new(0),
        vec![fixture_event(other_session_id, 1)],
    )
    .expect_err("tail events must belong to the requested session");
    assert_eq!(session_mismatch.code(), "invalid_event_tail");

    let tail = SessionEventTailBatchDto::new(
        schema,
        session_id,
        SessionEventSequenceDto::new(1),
        Vec::new(),
    )
    .expect("empty tail is valid");
    let response = SessionSubscriptionResponseDto::snapshot_and_tail(
        SessionSnapshotDto::new(schema, session_id, SessionEventSequenceDto::new(0)),
        tail,
    )
    .expect_err("snapshot and tail positions must agree");
    assert_eq!(response.code(), "invalid_subscription_response");
}

#[test]
fn malformed_protocol_payload_fields_and_closed_variants_are_rejected() {
    for wire in [
        r#"{"kind":"command","data":{"kind":"send_user_turn","data":{"session_id":"11111111-1111-4111-8111-111111111111","turn_id":"11111111-1111-4111-8111-111111111111","content":" "}}}"#,
        r#"{"kind":"query","data":{"kind":"unknown_query"}}"#,
        r#"{"status":"unknown","data":{}}"#,
        r#"{"kind":"subscription","data":{"kind":"snapshot_and_tail","data":{"snapshot":{"schema_version":{"major":1,"minor":0},"session_id":"11111111-1111-4111-8111-111111111111","at_sequence":0},"tail":{"schema_version":{"major":1,"minor":0},"session_id":"22222222-2222-4222-8222-222222222222","after_sequence":0,"events":[]}}}}"#,
    ] {
        if wire.contains("unknown_query") {
            assert!(serde_json::from_str::<ProtocolRequestPayloadDto>(wire).is_err());
        } else if wire.contains("unknown") {
            assert!(serde_json::from_str::<ProtocolCommandResultDto>(wire).is_err());
        } else if wire.contains("subscription") {
            assert!(serde_json::from_str::<ProtocolResponsePayloadDto>(wire).is_err());
        } else {
            assert!(serde_json::from_str::<ProtocolRequestPayloadDto>(wire).is_err());
        }
    }
}
