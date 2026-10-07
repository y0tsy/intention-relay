#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Test-first protocol contract and current-version wire evidence.

use intention_proto::{
    CURRENT_DTO_SCHEMA_VERSION, CURRENT_PROTOCOL_VERSION, DaemonHealthDto, DaemonReadinessDto,
    InterruptRunAcceptedDto, JsonRpcErrorDto, JsonRpcRequestDto, JsonRpcResponseDto,
    PROTOCOL_HELLO_METHOD, ProtocolAcceptedDto, ProtocolAcceptedResultDto, ProtocolCommandDto,
    ProtocolCommandResultDto, ProtocolHelloDto, ProtocolMethodDto, ProtocolQueryDto,
    ProtocolQueryResultDto, ProtocolRequestPayloadDto, ProtocolResponsePayloadDto,
    ProtocolVersionDto, RunStatusFrameDto, RunStreamFrameDto, RunSubscriptionResponseDto,
    RunSubscriptionSnapshotDto, SessionSnapshotDto, SessionSubscriptionResponseDto,
    SubscribeRunCommandDto, SubscribeSessionCommandDto, decode_hello_request, decode_request_line,
    decode_response, encode_hello_request, encode_request, encode_response, is_notification_line,
    parse_run_frame_notification,
};
use intention_proto::{
    ConfigRevisionId, CorrelationIdDto, ErrorDto, IdempotencyKey, ProjectId, RunId,
    SchemaVersionDto, SessionId, TurnId, WorkspaceId,
};
use intention_proto::{
    GetSessionSnapshotQueryDto, InterruptRunCommandDto, MessageKindDto, MessageProjectionDto,
    RunModeDto, RunProjectionDto, RunStatusDto, SendUserTurnCommandDto, SessionProjectionDto,
};

fn fixture_workspace_root() -> intention_proto::WorkspaceRootDto {
    intention_proto::WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-proto-contracts-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("fixture workspace root is valid")
}

fn fixture_projection(session_id: SessionId) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        fixture_workspace_root(),
        RunModeDto::Build,
        None,
        None,
        Vec::new(),
    )
    .expect("fixture projection is valid")
}

fn fixture_message(session_id: SessionId, run_id: RunId) -> MessageProjectionDto {
    MessageProjectionDto::new(
        session_id,
        Some(run_id),
        MessageKindDto::Notice,
        "fixture notice",
        None,
        None,
        None,
    )
    .expect("fixture message is valid")
}

fn fixture_run(session_id: SessionId, run_id: RunId) -> RunProjectionDto {
    RunProjectionDto::new(
        session_id,
        run_id,
        TurnId::new(),
        RunStatusDto::Running,
        ConfigRevisionId::new(),
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
    // Compared against a literal rather than the constant the fixture was
    // built from, so this assertion can fail: the wire is pinned to 2.0.
    assert_eq!(decoded.version(), ProtocolVersionDto::new(1, 0));
    assert_eq!(decoded.adapter_name(), "fixture-tui");
    assert!(
        !encoded.contains("capabilit"),
        "the handshake carries only version and adapter metadata"
    );
}

#[test]
fn only_the_exact_current_protocol_version_passes_negotiation_equality() {
    let current = intention_proto::CURRENT_PROTOCOL_VERSION;
    assert_eq!(current, ProtocolVersionDto::new(1, 0));
    assert_ne!(ProtocolVersionDto::new(1, 1), current);
    assert_ne!(ProtocolVersionDto::new(2, 1), current);
    // A non-current peer fails the daemon/client negotiation gate with the
    // typed `incompatible_protocol_version` error (covered in
    // intention-transport integration tests).
}

/// Returns the position of one method in the protocol method table.
///
/// This match is deliberately wildcard-free: adding a variant to
/// `ProtocolMethodDto` fails this test to compile until the new method is
/// named here, which is what makes [`EVERY_METHOD`] provably complete.
const fn method_position(method: ProtocolMethodDto) -> usize {
    match method {
        ProtocolMethodDto::SessionCreate => 0,
        ProtocolMethodDto::TurnSend => 1,
        ProtocolMethodDto::TurnRemove => 2,
        ProtocolMethodDto::RunInterrupt => 3,
        ProtocolMethodDto::SessionSubscribe => 4,
        ProtocolMethodDto::RunSubscribe => 5,
        ProtocolMethodDto::DaemonHealth => 6,
        ProtocolMethodDto::SessionSnapshot => 7,
    }
}

/// Every implemented protocol method, positionally aligned with
/// [`method_position`]; that wildcard-free match is the completeness proof.
const EVERY_METHOD: [ProtocolMethodDto; 8] = [
    ProtocolMethodDto::SessionCreate,
    ProtocolMethodDto::TurnSend,
    ProtocolMethodDto::TurnRemove,
    ProtocolMethodDto::RunInterrupt,
    ProtocolMethodDto::SessionSubscribe,
    ProtocolMethodDto::RunSubscribe,
    ProtocolMethodDto::DaemonHealth,
    ProtocolMethodDto::SessionSnapshot,
];

/// Returns the request payload one method carries.
///
/// Wildcard-free for the same reason as [`method_position`].
fn method_payload(method: ProtocolMethodDto) -> ProtocolRequestPayloadDto {
    let schema = SchemaVersionDto::new(1, 1);
    let session_id = SessionId::new();
    let run_id = RunId::new();
    match method {
        ProtocolMethodDto::SessionCreate => ProtocolRequestPayloadDto::Command(
            ProtocolCommandDto::CreateSession(intention_proto::CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                fixture_workspace_root(),
                RunModeDto::Build,
            )),
        ),
        ProtocolMethodDto::TurnSend => {
            ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(
                SendUserTurnCommandDto::new(session_id, IdempotencyKey::new(), "hello")
                    .expect("fixture turn is valid"),
            ))
        }
        ProtocolMethodDto::TurnRemove => {
            ProtocolRequestPayloadDto::Command(ProtocolCommandDto::RemoveTurn(
                intention_proto::RemoveTurnCommandDto::new(session_id, TurnId::new()),
            ))
        }
        ProtocolMethodDto::RunInterrupt => ProtocolRequestPayloadDto::Command(
            ProtocolCommandDto::InterruptRun(InterruptRunCommandDto::new(session_id, run_id)),
        ),
        ProtocolMethodDto::SessionSubscribe => {
            ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SubscribeSession(
                SubscribeSessionCommandDto::new(schema, session_id, RunModeDto::Build),
            ))
        }
        ProtocolMethodDto::RunSubscribe => ProtocolRequestPayloadDto::RunSubscription(
            SubscribeRunCommandDto::new(schema, session_id, run_id),
        ),
        ProtocolMethodDto::DaemonHealth => {
            ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth)
        }
        ProtocolMethodDto::SessionSnapshot => ProtocolRequestPayloadDto::Query(
            ProtocolQueryDto::GetSessionSnapshot(GetSessionSnapshotQueryDto::new(session_id)),
        ),
    }
}

/// Returns a payload the given method must reject as invalid params.
///
/// `daemon.health` accepts exactly one payload, so that query is foreign to
/// every other method; the one method that accepts it gets a stop-run command.
fn foreign_payload(method: ProtocolMethodDto) -> ProtocolRequestPayloadDto {
    match method {
        ProtocolMethodDto::DaemonHealth => {
            ProtocolRequestPayloadDto::Command(ProtocolCommandDto::InterruptRun(
                InterruptRunCommandDto::new(SessionId::new(), RunId::new()),
            ))
        }
        _ => ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
    }
}

#[test]
fn jsonrpc_method_table_covers_every_request_variant_exactly_once() {
    let mut names = std::collections::BTreeSet::new();
    for (position, method) in EVERY_METHOD.into_iter().enumerate() {
        assert_eq!(
            method_position(method),
            position,
            "the enumerated table and the wildcard-free match must agree"
        );
        assert!(
            names.insert(method.as_str()),
            "{} owns a wire name no other method claims",
            method.as_str()
        );
        assert!(
            !method.as_str().is_empty(),
            "every method publishes a wire name"
        );

        let payload = method_payload(method);
        assert!(
            method.accepts_request(&payload),
            "{} must accept its own payload",
            method.as_str()
        );
        let id = position as u64 + 1;
        let request = encode_request(id, payload);
        assert_eq!(
            request.method(),
            method.as_str(),
            "the method owns its wire name"
        );
        assert_eq!(
            ProtocolMethodDto::for_payload(request.params()),
            method,
            "each request payload maps back to exactly one method"
        );
        let line = serde_json::to_string(&request).expect("request serializes");
        assert!(
            line.contains(r#""jsonrpc":"2.0""#),
            "every request is a JSON-RPC 2.0 envelope"
        );
        let decoded = decode_request_line(&line).expect("request decodes");
        assert_eq!(decoded.id(), id);
        assert_eq!(decoded.payload(), request.params());

        // The reverse direction: the name table recognizes this wire name
        // independently of the payload table, because a known name with a
        // foreign payload fails as invalid params (-32602) and never as an
        // unknown method (-32601).
        let mismatched = JsonRpcRequestDto::new(id, method.as_str(), foreign_payload(method));
        let line = serde_json::to_string(&mismatched).expect("mismatched request serializes");
        let failure = decode_request_line(&line).expect_err("a foreign payload is rejected");
        assert_eq!(
            failure.error().code(),
            intention_proto::JSONRPC_INVALID_PARAMS,
            "{} is a known method, so only its payload pairing may fail",
            method.as_str()
        );
        assert_eq!(failure.id(), Some(id));
    }
    assert_eq!(
        names.len(),
        8,
        "the local protocol implements one uniquely named method per variant"
    );
}

#[test]
fn daemon_health_and_session_snapshot_contracts_round_trip() {
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
    let run_id = RunId::new();
    let snapshot = SessionSnapshotDto::with_projection(
        schema,
        session_id,
        fixture_projection(session_id),
        vec![fixture_message(session_id, run_id)],
    )
    .expect("fixture snapshot is valid");
    assert_eq!(snapshot.session_id(), session_id);
    assert_eq!(snapshot.messages().len(), 1);
    let response = SessionSubscriptionResponseDto::snapshot(snapshot);

    let encoded = serde_json::to_string(&response).expect("response serializes");
    let decoded: SessionSubscriptionResponseDto =
        serde_json::from_str(&encoded).expect("response deserializes");
    assert_eq!(decoded, response);
}

#[test]
fn subscribe_command_carries_optional_run_scope_on_the_current_shape() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let command = SubscribeSessionCommandDto::with_run_id(
        SchemaVersionDto::new(1, 1),
        session_id,
        Some(run_id),
        RunModeDto::Build,
    );
    assert_eq!(command.run_id(), Some(run_id));
    assert_eq!(
        SubscribeSessionCommandDto::new(
            SchemaVersionDto::new(1, 1),
            session_id,
            RunModeDto::Build,
        )
        .run_id(),
        None,
        "the session-wide constructor requests no run scope"
    );

    let encoded = serde_json::to_string(&command).expect("test serialization must succeed");
    assert!(
        !encoded.contains("cursor") && !encoded.contains("sequence"),
        "the subscription carries no cursor or sequence position"
    );
    let decoded: SubscribeSessionCommandDto =
        serde_json::from_str(&encoded).expect("test deserialization must succeed");

    assert_eq!(decoded, command);
    assert_eq!(decoded.requested_mode(), intention_proto::RunModeDto::Build);
}

#[test]
fn run_subscription_command_carries_only_the_run_scope() {
    let command =
        SubscribeRunCommandDto::new(SchemaVersionDto::new(1, 1), SessionId::new(), RunId::new());

    let encoded = serde_json::to_string(&command).expect("test serialization must succeed");
    assert!(
        !encoded.contains("cursor"),
        "the run subscription carries no cursor"
    );
    let decoded: SubscribeRunCommandDto =
        serde_json::from_str(&encoded).expect("test deserialization must succeed");
    assert_eq!(decoded, command);
}

#[test]
fn protocol_request_and_response_variants_round_trip_through_jsonrpc_envelopes() {
    let schema = SchemaVersionDto::new(1, 1);
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let correlation_id = CorrelationIdDto::new();
    let request_variants = [
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, IdempotencyKey::new(), "hello")
                .expect("fixture turn is valid"),
        )),
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::InterruptRun(
            InterruptRunCommandDto::new(session_id, run_id),
        )),
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SubscribeSession(
            SubscribeSessionCommandDto::new(schema, session_id, RunModeDto::Build),
        )),
        ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
        ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetSessionSnapshot(
            GetSessionSnapshotQueryDto::new(session_id),
        )),
        ProtocolRequestPayloadDto::RunSubscription(SubscribeRunCommandDto::new(
            schema, session_id, run_id,
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
        fixture_projection(session_id),
        vec![fixture_message(session_id, run_id)],
    )
    .expect("fixture snapshot is valid");
    let run_snapshot = RunSubscriptionSnapshotDto::new(
        fixture_run(session_id, run_id),
        vec![fixture_message(session_id, run_id)],
    )
    .expect("fixture run snapshot is valid");
    let responses = [
        (
            ProtocolMethodDto::TurnSend,
            ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(
                ProtocolAcceptedDto::with_result(
                    correlation_id,
                    ProtocolAcceptedResultDto::InterruptRun(InterruptRunAcceptedDto::new(
                        session_id, run_id,
                    )),
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
                snapshot.clone(),
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
            ProtocolResponsePayloadDto::Subscription(SessionSubscriptionResponseDto::snapshot(
                snapshot,
            )),
        ),
        (
            ProtocolMethodDto::RunSubscribe,
            ProtocolResponsePayloadDto::RunSubscription(RunSubscriptionResponseDto::snapshot(
                run_snapshot,
            )),
        ),
        (
            ProtocolMethodDto::RunSubscribe,
            ProtocolResponsePayloadDto::RunSubscription(RunSubscriptionResponseDto::error(
                ErrorDto::validation("run_not_found", "fixture run rejection"),
            )),
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
            intention_proto::JSONRPC_METHOD_NOT_FOUND,
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
            intention_proto::JSONRPC_METHOD_NOT_FOUND,
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
        intention_proto::JSONRPC_METHOD_NOT_FOUND
    );
    assert_eq!(failure.id(), Some(9));
}

#[test]
fn run_frame_notifications_tag_content_and_status_kinds() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let content = RunStreamFrameDto::Content(fixture_message(session_id, run_id));
    let status = RunStreamFrameDto::Status(RunStatusFrameDto::new(
        session_id,
        run_id,
        RunStatusDto::Running,
    ));

    for (frame, kind) in [(&content, "content"), (&status, "status")] {
        let notification = intention_proto::ProtocolDaemonMessageDto::run_frame(frame.clone());
        let line = serde_json::to_string(&notification).expect("notification serializes");
        assert!(line.contains(r#""method":"run.frame""#));
        let value: serde_json::Value =
            serde_json::from_str(&line).expect("notification line is JSON");
        assert_eq!(value["params"]["kind"], kind);
        assert!(value["params"]["data"].is_object());
        assert!(
            !line.contains("cursor") && !line.contains("sequence"),
            "a run frame carries no position"
        );
        assert_eq!(
            parse_run_frame_notification(&line).expect("run frame parses"),
            *frame
        );
    }

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
fn run_subscription_snapshots_round_trip_and_validate_scope() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let run = fixture_run(session_id, run_id);
    let snapshot = RunSubscriptionSnapshotDto::new(run, vec![fixture_message(session_id, run_id)])
        .expect("coherent run snapshot is valid");
    assert_eq!(snapshot.run().run_id(), run_id);
    assert_eq!(snapshot.messages().len(), 1);

    let response = RunSubscriptionResponseDto::snapshot(snapshot);
    let encoded = serde_json::to_string(&response).expect("response serializes");
    let decoded: RunSubscriptionResponseDto =
        serde_json::from_str(&encoded).expect("response deserializes");
    assert_eq!(decoded, response);

    assert_eq!(
        RunSubscriptionSnapshotDto::new(run, vec![fixture_message(SessionId::new(), run_id)])
            .expect_err("a message from another session fails closed")
            .code(),
        "invalid_run_subscription_snapshot"
    );
    assert_eq!(
        RunSubscriptionSnapshotDto::new(run, vec![fixture_message(session_id, RunId::new())])
            .expect_err("a message from another run fails closed")
            .code(),
        "invalid_run_subscription_snapshot"
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
    assert_eq!(mismatch.code(), intention_proto::JSONRPC_VERSION_MISMATCH);
    assert_eq!(
        mismatch.data().map(ErrorDto::code),
        Some("incompatible_protocol_version")
    );

    let skipped_hello = JsonRpcRequestDto::new(1, "daemon.health", current);
    let required = decode_hello_request(&skipped_hello)
        .expect_err("the first request on a connection must be hello");
    assert_eq!(required.code(), intention_proto::JSONRPC_INVALID_REQUEST);
}

#[test]
fn unknown_additive_hello_fields_remain_tolerated() {
    let hello: ProtocolHelloDto = serde_json::from_str(
        r#"{"version":{"major":1,"minor":0},"adapter_name":"fixture","future_additive":true}"#,
    )
    .expect("unknown additive protocol fields are ignored for compatibility");
    assert_eq!(hello.adapter_name(), "fixture");
    assert_eq!(hello.version(), CURRENT_PROTOCOL_VERSION);
}

#[test]
fn malformed_protocol_payload_fields_and_closed_variants_are_rejected() {
    assert!(
        serde_json::from_str::<ProtocolRequestPayloadDto>(
            r#"{"kind":"command","data":{"kind":"send_user_turn","data":{"session_id":"11111111-1111-4111-8111-111111111111","idempotency_key":"11111111-1111-4111-8111-111111111111","content":" "}}}"#
        )
        .is_err(),
        "a blank turn command fails closed"
    );
    assert!(
        serde_json::from_str::<ProtocolRequestPayloadDto>(
            r#"{"kind":"query","data":{"kind":"unknown_query"}}"#
        )
        .is_err(),
        "a closed query table rejects unknown variants"
    );
    assert!(
        serde_json::from_str::<ProtocolCommandResultDto>(r#"{"status":"unknown","data":{}}"#)
            .is_err(),
        "a closed command result table rejects unknown variants"
    );
    assert!(
        serde_json::from_str::<ProtocolResponsePayloadDto>(
            r#"{"kind":"subscription","data":{"kind":"snapshot","data":{"schema_version":{"major":1,"minor":1},"session_id":"11111111-1111-4111-8111-111111111111"}}}"#
        )
        .is_err(),
        "a session snapshot without its projection and messages fails closed"
    );
    assert!(
        serde_json::from_str::<RunStreamFrameDto>(r#"{"kind":"unknown","data":{}}"#).is_err(),
        "a closed run-frame table rejects unknown kinds"
    );
}

#[test]
fn parameterless_daemon_health_decodes_without_a_params_member() {
    // JSON-RPC 2.0 makes `params` optional, and `daemon.health` is the one
    // genuinely parameterless method.
    let line = r#"{"jsonrpc":"2.0","id":1,"method":"daemon.health"}"#;
    let request = decode_request_line(line).expect("the parameterless form is spec-legal");
    assert_eq!(request.id(), 1);
    assert_eq!(
        request.payload(),
        &ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth)
    );

    // A present `params` member is still decoded, so an explicit null fails.
    let explicit_null =
        decode_request_line(r#"{"jsonrpc":"2.0","id":1,"method":"daemon.health","params":null}"#)
            .expect_err("an explicit null params member is not parameterless");
    assert_eq!(
        explicit_null.error().code(),
        intention_proto::JSONRPC_INVALID_PARAMS
    );

    // Every other method requires its typed payload.
    let missing = decode_request_line(r#"{"jsonrpc":"2.0","id":2,"method":"session.snapshot"}"#)
        .expect_err("a method with a required payload rejects an absent params member");
    assert_eq!(
        missing.error().code(),
        intention_proto::JSONRPC_INVALID_PARAMS
    );
    assert_eq!(missing.id(), Some(2));
}

#[test]
fn a_request_without_an_id_member_is_a_notification() {
    let notification = r#"{"jsonrpc":"2.0","method":"daemon.health"}"#;
    assert!(
        is_notification_line(notification),
        "a request envelope without an id member is a notification"
    );

    // An explicit null id is a request, not a notification, so a server answers
    // it instead of staying silent.
    let null_id = r#"{"jsonrpc":"2.0","id":null,"method":"daemon.health"}"#;
    assert!(
        !is_notification_line(null_id),
        "an explicit null id is a request, not a notification"
    );
    let failure = decode_request_line(null_id)
        .expect_err("the numeric-id profile rejects a null identity with a reply");
    assert_eq!(
        failure.error().code(),
        intention_proto::JSONRPC_INVALID_REQUEST
    );
    assert_eq!(failure.id(), None);

    for not_a_notification in [
        r#"{"jsonrpc":"1.0","method":"daemon.health"}"#,
        r#"{"jsonrpc":"2.0","method":""}"#,
        r#"{"jsonrpc":"2.0","id":7,"method":"daemon.health"}"#,
        "{",
    ] {
        assert!(
            !is_notification_line(not_a_notification),
            "{not_a_notification} is not a notification"
        );
    }
}

/// Asserts that one payload DTO rejects a wire value carrying `stale_version`.
fn assert_schema_version_is_rejected<T: serde::de::DeserializeOwned + std::fmt::Debug>(
    wire: serde_json::Value,
    stale_version: &serde_json::Value,
) {
    let error = serde_json::from_value::<T>(wire)
        .expect_err("a non-current payload schema version fails closed");
    assert!(
        error
            .to_string()
            .contains("incompatible_dto_schema_version"),
        "the rejection is typed for {stale_version}: {error}"
    );
}

#[test]
fn a_payload_schema_version_other_than_the_current_one_is_rejected_on_decode() {
    assert_eq!(CURRENT_DTO_SCHEMA_VERSION, SchemaVersionDto::new(1, 1));
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let snapshot = SessionSnapshotDto::with_projection(
        CURRENT_DTO_SCHEMA_VERSION,
        session_id,
        fixture_projection(session_id),
        vec![fixture_message(session_id, run_id)],
    )
    .expect("fixture snapshot is valid");
    let current = serde_json::to_value(&snapshot).expect("snapshot serializes");
    assert!(
        serde_json::from_value::<SessionSnapshotDto>(current.clone()).is_ok(),
        "the current schema version decodes"
    );

    // Every payload DTO that carries a schema version shares the exact-equality
    // gate, not only the snapshot: a stale or future version is always a typed
    // decode rejection.
    let health = serde_json::to_value(DaemonHealthDto::new(
        CURRENT_DTO_SCHEMA_VERSION,
        CURRENT_PROTOCOL_VERSION,
        DaemonReadinessDto::Ready,
    ))
    .expect("health serializes");
    let subscription = serde_json::to_value(SubscribeSessionCommandDto::new(
        CURRENT_DTO_SCHEMA_VERSION,
        session_id,
        RunModeDto::Build,
    ))
    .expect("subscription serializes");
    let run_subscription = serde_json::to_value(SubscribeRunCommandDto::new(
        CURRENT_DTO_SCHEMA_VERSION,
        session_id,
        run_id,
    ))
    .expect("run subscription serializes");

    for stale in [
        serde_json::json!({"major": 1, "minor": 0}),
        serde_json::json!({"major": 1, "minor": 2}),
        serde_json::json!({"major": 2, "minor": 1}),
    ] {
        let mut wire = current.clone();
        wire["schema_version"] = stale.clone();
        assert_schema_version_is_rejected::<SessionSnapshotDto>(wire, &stale);

        let mut wire = health.clone();
        wire["schema_version"] = stale.clone();
        assert_schema_version_is_rejected::<DaemonHealthDto>(wire, &stale);

        let mut wire = subscription.clone();
        wire["schema_version"] = stale.clone();
        assert_schema_version_is_rejected::<SubscribeSessionCommandDto>(wire, &stale);

        let mut wire = run_subscription.clone();
        wire["schema_version"] = stale.clone();
        assert_schema_version_is_rejected::<SubscribeRunCommandDto>(wire, &stale);
    }
}
