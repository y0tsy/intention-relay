#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Test-first protocol contract and current-version wire evidence.

use intention_domain::{
    DomainEventDto, GetSessionSnapshotQueryDto, RunModeDto, RunStatusChangedEventDto, RunStatusDto,
    SendUserTurnCommandDto, SessionProjectionDto, StopRunCommandDto,
};
use intention_protocol::{
    CURRENT_PROTOCOL_VERSION, DaemonHealthDto, DaemonReadinessDto, JsonRpcErrorDto,
    JsonRpcRequestDto, JsonRpcResponseDto, PROTOCOL_HELLO_METHOD, ProtocolAcceptedDto,
    ProtocolAcceptedResultDto, ProtocolCommandDto, ProtocolCommandResultDto, ProtocolHelloDto,
    ProtocolMethodDto, ProtocolQueryDto, ProtocolQueryResultDto, ProtocolRequestPayloadDto,
    ProtocolResponsePayloadDto, ProtocolVersionDto, RunResyncDto, RunResyncReasonDto,
    RunStreamFrameDto, SessionEventTailBatchDto, SessionResyncDto, SessionResyncReasonDto,
    SessionSnapshotDto, SessionSubscriptionResponseDto, SubscribeSessionCommandDto,
    decode_hello_request, decode_request_line, decode_response, encode_hello_request,
    encode_request, encode_response, parse_run_frame_notification,
};
use intention_types::{
    CorrelationIdDto, ErrorDto, EventEnvelopeDto, EventId, EventMetadataDto, ProjectId,
    SchemaVersionDto, SessionEventSequenceDto, SessionId, TimestampDto, WorkspaceId,
};

fn fixture_workspace_root() -> intention_domain::WorkspaceRootDto {
    intention_domain::WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-protocol-contracts-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("fixture workspace root is valid")
}

fn fixture_projection(
    session_id: SessionId,
    at_sequence: SessionEventSequenceDto,
) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        fixture_workspace_root(),
        RunModeDto::Build,
        None,
        None,
        Vec::new(),
        at_sequence,
    )
    .expect("fixture projection is valid")
}

fn fixture_event(session_id: SessionId, sequence: u64) -> EventEnvelopeDto<DomainEventDto> {
    let occurred_at = TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid");
    EventEnvelopeDto::new(
        EventMetadataDto::new(
            SchemaVersionDto::new(1, 1),
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
fn protocol_hello_round_trips_with_the_current_version() {
    let hello = ProtocolHelloDto::new(CURRENT_PROTOCOL_VERSION, "fixture-tui")
        .expect("fixture hello is valid");

    let encoded = serde_json::to_string(&hello).expect("test serialization must succeed");
    let decoded: ProtocolHelloDto =
        serde_json::from_str(&encoded).expect("test deserialization must succeed");

    assert_eq!(decoded, hello);
    assert_eq!(decoded.version(), CURRENT_PROTOCOL_VERSION);
    assert_eq!(decoded.adapter_name(), "fixture-tui");
    assert!(
        !encoded.contains("capabilit"),
        "the handshake carries only version and adapter metadata"
    );
}

#[test]
fn only_the_exact_current_protocol_version_passes_negotiation_equality() {
    let current = intention_protocol::CURRENT_PROTOCOL_VERSION;
    assert_eq!(current, ProtocolVersionDto::new(2, 0));
    assert_ne!(ProtocolVersionDto::new(1, 1), current);
    assert_ne!(ProtocolVersionDto::new(2, 1), current);
    // A non-current peer fails the daemon/client negotiation gate with the
    // typed `incompatible_protocol_version` error (covered in
    // intention-transport integration tests).
}

#[test]
fn jsonrpc_method_table_covers_every_request_variant_exactly_once() {
    let schema = SchemaVersionDto::new(1, 1);
    let session_id = SessionId::new();
    let run_id = intention_types::RunId::new();
    let payloads = [
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::CreateSession(
            intention_domain::CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                fixture_workspace_root(),
                RunModeDto::Build,
            ),
        )),
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, intention_types::TurnId::new(), "hello")
                .expect("fixture turn is valid"),
        )),
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::RemoveQueuedTurn(
            intention_domain::RemoveQueuedTurnCommandDto::new(
                session_id,
                intention_types::TurnId::new(),
            ),
        )),
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::StopRun(StopRunCommandDto::new(
            session_id, run_id,
        ))),
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SubscribeSession(
            SubscribeSessionCommandDto::new(schema, session_id, None, RunModeDto::Build),
        )),
        ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
        ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetSessionSnapshot(
            GetSessionSnapshotQueryDto::new(session_id),
        )),
        ProtocolRequestPayloadDto::RunSubscription(
            intention_protocol::SubscribeRunCommandDto::new(schema, session_id, run_id, None),
        ),
    ];

    let mut methods = std::collections::BTreeSet::new();
    for (index, payload) in payloads.into_iter().enumerate() {
        let id = index as u64 + 1;
        let method = ProtocolMethodDto::for_payload(&payload);
        assert!(
            method.accepts_request(&payload),
            "the method must accept its own payload"
        );
        assert!(
            methods.insert(method.as_str()),
            "each request variant owns exactly one method"
        );
        assert!(
            !method.as_str().is_empty(),
            "every method publishes a wire name"
        );

        let request = encode_request(id, payload);
        assert_eq!(request.method(), method.as_str());
        let line = serde_json::to_string(&request).expect("request serializes");
        assert!(
            line.contains(r#""jsonrpc":"2.0""#),
            "every request is a JSON-RPC 2.0 envelope"
        );
        let decoded = decode_request_line(&line).expect("request decodes");
        assert_eq!(decoded.id(), id);
        assert_eq!(decoded.payload(), request.params());
    }
    assert_eq!(
        methods.len(),
        8,
        "the local protocol implements one method per request variant"
    );
}

#[test]
fn daemon_health_and_snapshot_tail_contracts_round_trip() {
    let schema = SchemaVersionDto::new(1, 1);
    let health = DaemonHealthDto::new(schema, CURRENT_PROTOCOL_VERSION, DaemonReadinessDto::Ready);
    assert_eq!(
        serde_json::from_str::<DaemonHealthDto>(
            &serde_json::to_string(&health).expect("health serializes")
        )
        .expect("health deserializes"),
        health
    );

    let session_id = SessionId::new();
    let snapshot = SessionSnapshotDto::with_projection(
        schema,
        session_id,
        SessionEventSequenceDto::new(2),
        fixture_projection(session_id, SessionEventSequenceDto::new(2)),
    )
    .expect("fixture snapshot is valid");
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
fn subscribe_command_carries_optional_run_scope_on_the_current_shape() {
    let command = SubscribeSessionCommandDto::with_run_id(
        SchemaVersionDto::new(1, 1),
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
fn protocol_request_and_response_variants_round_trip_through_jsonrpc_envelopes() {
    let schema = SchemaVersionDto::new(1, 1);
    let session_id = SessionId::new();
    let run_id = intention_types::RunId::new();
    let correlation_id = CorrelationIdDto::new();
    let request_variants = [
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, intention_types::TurnId::new(), "hello")
                .expect("fixture turn is valid"),
        )),
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::StopRun(StopRunCommandDto::new(
            session_id, run_id,
        ))),
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SubscribeSession(
            SubscribeSessionCommandDto::new(schema, session_id, None, RunModeDto::Build),
        )),
        ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
        ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetSessionSnapshot(
            GetSessionSnapshotQueryDto::new(session_id),
        )),
    ];

    for (index, payload) in request_variants.into_iter().enumerate() {
        let id = index as u64 + 10;
        let request = encode_request(id, payload);
        let encoded = serde_json::to_string(&request).expect("request envelope serializes");
        let decoded = decode_request_line(&encoded).expect("request envelope deserializes");
        assert_eq!(decoded.id(), id);
        assert_eq!(decoded.payload(), request.params());
    }

    let snapshot = SessionSnapshotDto::with_projection(
        schema,
        session_id,
        SessionEventSequenceDto::new(0),
        fixture_projection(session_id, SessionEventSequenceDto::new(0)),
    )
    .expect("fixture snapshot is valid");
    let responses = [
        (
            ProtocolMethodDto::TurnSend,
            ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(
                ProtocolAcceptedDto::with_result(
                    correlation_id,
                    ProtocolAcceptedResultDto::StopRun(
                        intention_protocol::StopRunAcceptedDto::new(
                            session_id,
                            run_id,
                            SessionEventSequenceDto::new(1),
                        ),
                    ),
                ),
            )),
        ),
        (
            ProtocolMethodDto::TurnSend,
            ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Rejected(
                ErrorDto::validation("fixture_rejected", "fixture rejection"),
            )),
        ),
        (
            ProtocolMethodDto::DaemonHealth,
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(
                DaemonHealthDto::new(
                    schema,
                    CURRENT_PROTOCOL_VERSION,
                    DaemonReadinessDto::Draining,
                ),
            )),
        ),
        (
            ProtocolMethodDto::SessionSnapshot,
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::SessionSnapshot(
                snapshot,
            )),
        ),
        (
            ProtocolMethodDto::SessionSnapshot,
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(
                ErrorDto::validation("fixture_query_rejected", "fixture rejection"),
            )),
        ),
        (
            ProtocolMethodDto::SessionSubscribe,
            ProtocolResponsePayloadDto::Subscription(
                SessionSubscriptionResponseDto::resync_required(SessionResyncDto::new(
                    schema,
                    session_id,
                    SessionResyncReasonDto::HistoryUnavailable,
                )),
            ),
        ),
    ];

    for (index, (method, payload)) in responses.into_iter().enumerate() {
        let id = index as u64 + 20;
        let response = encode_response(id, payload);
        let encoded = serde_json::to_string(&response).expect("response envelope serializes");
        let decoded =
            decode_response(&encoded, method, id).expect("response envelope deserializes");
        assert_eq!(decoded, response.result_value().cloned().expect("result"));
    }
}

#[test]
fn jsonrpc_error_responses_map_to_stable_typed_errors() {
    let id = 7_u64;
    let failure = JsonRpcResponseDto::<ProtocolResponsePayloadDto>::error(
        Some(id),
        JsonRpcErrorDto::from_error(
            intention_protocol::jsonrpc::JSONRPC_INTERNAL_ERROR,
            ErrorDto::validation("fixture_daemon_error", "fixture daemon failure"),
        ),
    );
    let line = serde_json::to_string(&failure).expect("error response serializes");
    let error = decode_response(&line, ProtocolMethodDto::DaemonHealth, id)
        .expect_err("error responses surface the typed error");
    assert_eq!(error.code(), "fixture_daemon_error");

    let untyped = JsonRpcResponseDto::<ProtocolResponsePayloadDto>::error(
        Some(id),
        JsonRpcErrorDto::new(
            intention_protocol::jsonrpc::JSONRPC_INTERNAL_ERROR,
            "fixture failure",
            None,
        ),
    );
    let line = serde_json::to_string(&untyped).expect("untyped error response serializes");
    let error = decode_response(&line, ProtocolMethodDto::DaemonHealth, id)
        .expect_err("an error response without typed data still fails closed");
    assert_eq!(error.code(), "local_daemon_jsonrpc_error");

    let response = encode_response(
        id,
        ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(
            DaemonHealthDto::new(
                SchemaVersionDto::new(1, 1),
                CURRENT_PROTOCOL_VERSION,
                DaemonReadinessDto::Ready,
            ),
        )),
    );
    let line = serde_json::to_string(&response).expect("success response serializes");
    let mismatch = decode_response(&line, ProtocolMethodDto::DaemonHealth, id + 1)
        .expect_err("a mismatched response identity fails closed");
    assert_eq!(mismatch.code(), "invalid_local_protocol_response");
    let wrong_family = decode_response(&line, ProtocolMethodDto::SessionSubscribe, id)
        .expect_err("a mismatched response family fails closed");
    assert_eq!(wrong_family.code(), "invalid_local_protocol_response");

    let unknown = r#"{"jsonrpc":"2.0","id":9,"method":"fixture.unknown","params":{"value":1}}"#;
    let failure = decode_request_line(unknown).expect_err("unknown methods fail");
    assert_eq!(
        failure.error().code(),
        intention_protocol::JSONRPC_METHOD_NOT_FOUND
    );
    assert_eq!(failure.id(), Some(9));
}

#[test]
fn run_frame_notifications_round_trip_and_reject_other_methods() {
    let session_id = SessionId::new();
    let run_id = intention_types::RunId::new();
    let frame = RunStreamFrameDto::Resync(RunResyncDto::new(
        session_id,
        run_id,
        RunResyncReasonDto::SubscriberTooSlow,
    ));
    let notification = intention_protocol::ProtocolDaemonMessageDto::run_frame(frame.clone());
    let line = serde_json::to_string(&notification).expect("notification serializes");
    assert!(line.contains(r#""method":"run.frame""#));
    assert_eq!(
        parse_run_frame_notification(&line).expect("run frame parses"),
        frame
    );

    let foreign = JsonRpcResponseDto::<ProtocolResponsePayloadDto>::error(
        Some(1),
        JsonRpcErrorDto::invalid_request(),
    );
    let line = serde_json::to_string(&foreign).expect("foreign line serializes");
    assert!(
        parse_run_frame_notification(&line).is_err(),
        "a response is never accepted as a run frame"
    );
}

#[test]
fn hello_version_mismatch_answers_with_the_typed_32001_error() {
    let current = ProtocolHelloDto::new(CURRENT_PROTOCOL_VERSION, "fixture-client")
        .expect("fixture hello is valid");
    let request = encode_hello_request(1, current.clone());
    let line = serde_json::to_string(&request).expect("hello request serializes");
    let decoded: JsonRpcRequestDto<ProtocolHelloDto> =
        JsonRpcRequestDto::parse(&line).expect("hello request parses");
    assert_eq!(decoded.method(), PROTOCOL_HELLO_METHOD);
    assert_eq!(
        decode_hello_request(&decoded).expect("current hello is accepted"),
        current
    );

    let stale = ProtocolHelloDto::new(ProtocolVersionDto::new(1, 1), "stale-client")
        .expect("stale hello is well-formed");
    let request = encode_hello_request(1, stale);
    let line = serde_json::to_string(&request).expect("stale hello serializes");
    let decoded: JsonRpcRequestDto<ProtocolHelloDto> =
        JsonRpcRequestDto::parse(&line).expect("stale hello parses");
    let mismatch = decode_hello_request(&decoded).expect_err("version mismatch is typed");
    assert_eq!(
        mismatch.code(),
        intention_protocol::JSONRPC_VERSION_MISMATCH
    );
    assert_eq!(
        mismatch.data().map(ErrorDto::code),
        Some("incompatible_protocol_version")
    );

    let skipped_hello = JsonRpcRequestDto::new(1, "daemon.health", current);
    let required = decode_hello_request(&skipped_hello)
        .expect_err("the first request on a connection must be hello");
    assert_eq!(required.code(), intention_protocol::JSONRPC_INVALID_REQUEST);
}

#[test]
fn event_tails_and_subscription_responses_reject_mismatched_boundaries() {
    let schema = SchemaVersionDto::new(1, 1);
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
        SessionSnapshotDto::with_projection(
            schema,
            session_id,
            SessionEventSequenceDto::new(0),
            fixture_projection(session_id, SessionEventSequenceDto::new(0)),
        )
        .expect("fixture snapshot is valid"),
        tail,
    )
    .expect_err("snapshot and tail positions must agree");
    assert_eq!(response.code(), "invalid_subscription_response");
}

#[test]
fn unknown_additive_hello_fields_remain_tolerated() {
    let hello: ProtocolHelloDto = serde_json::from_str(
        r#"{"version":{"major":2,"minor":0},"adapter_name":"fixture","future_additive":true}"#,
    )
    .expect("unknown additive protocol fields are ignored for compatibility");
    assert_eq!(hello.adapter_name(), "fixture");
    assert_eq!(hello.version(), CURRENT_PROTOCOL_VERSION);
}

#[test]
fn malformed_protocol_payload_fields_and_closed_variants_are_rejected() {
    for wire in [
        r#"{"kind":"command","data":{"kind":"send_user_turn","data":{"session_id":"11111111-1111-4111-8111-111111111111","turn_id":"11111111-1111-4111-8111-111111111111","content":" "}}}"#,
        r#"{"kind":"query","data":{"kind":"unknown_query"}}"#,
        r#"{"status":"unknown","data":{}}"#,
        r#"{"kind":"subscription","data":{"kind":"snapshot_and_tail","data":{"snapshot":{"schema_version":{"major":1,"minor":1},"session_id":"11111111-1111-4111-8111-111111111111","at_sequence":0},"tail":{"schema_version":{"major":1,"minor":1},"session_id":"22222222-2222-4222-8222-222222222222","after_sequence":0,"events":[]}}}}"#,
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
