#![allow(
    clippy::expect_used,
    reason = "Protocol accessor coverage uses expect for fixture diagnostics."
)]

use intention_domain::{RunModeDto, SessionProjectionDto};
use intention_protocol::{
    CURRENT_PROTOCOL_VERSION, DaemonHealthDto, DaemonReadinessDto, ProtocolCommandDto,
    ProtocolMethodDto, ProtocolQueryResultDto, ProtocolRequestPayloadDto,
    ProtocolResponsePayloadDto, ProtocolVersionDto, SessionEventTailBatchDto, SessionResyncDto,
    SessionResyncReasonDto, SessionSnapshotDto, SubscribeSessionCommandDto, decode_request_line,
    decode_response, encode_request, encode_response,
};
use intention_types::{
    ProjectId, SchemaVersionDto, SessionEventSequenceDto, SessionId, WorkspaceId,
};

fn fixture_projection(
    session_id: SessionId,
    at_sequence: SessionEventSequenceDto,
) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        intention_domain::WorkspaceRootDto::parse(
            std::env::temp_dir()
                .join("intention-protocol-accessors-workspace")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture workspace root is valid"),
        RunModeDto::Build,
        None,
        None,
        Vec::new(),
        at_sequence,
    )
    .expect("fixture projection is valid")
}

#[test]
fn public_protocol_accessors_preserve_typed_values() {
    let schema = SchemaVersionDto::new(1, 1);
    let version = CURRENT_PROTOCOL_VERSION;
    let health = DaemonHealthDto::new(schema, version, DaemonReadinessDto::Starting);
    assert_eq!(health.schema_version(), schema);
    assert_eq!(health.protocol_version(), version);
    assert_eq!(health.readiness(), DaemonReadinessDto::Starting);

    let session_id = SessionId::new();
    let snapshot = SessionSnapshotDto::with_projection(
        schema,
        session_id,
        SessionEventSequenceDto::new(4),
        fixture_projection(session_id, SessionEventSequenceDto::new(4)),
    )
    .expect("fixture snapshot is valid");
    let tail =
        SessionEventTailBatchDto::new(schema, session_id, snapshot.at_sequence(), Vec::new())
            .expect("empty tail is valid");
    assert_eq!(tail.schema_version(), schema);
    assert_eq!(tail.session_id(), session_id);
    assert_eq!(tail.after_sequence(), snapshot.at_sequence());
    assert_eq!(tail.next_after_sequence(), snapshot.at_sequence());

    let resync = SessionResyncDto::new(schema, session_id, SessionResyncReasonDto::InvalidPosition);
    assert_eq!(resync.schema_version(), schema);
    assert_eq!(resync.session_id(), session_id);
    assert_eq!(resync.reason(), SessionResyncReasonDto::InvalidPosition);

    let subscription = SubscribeSessionCommandDto::new(schema, session_id, None, RunModeDto::Build);
    assert_eq!(subscription.session_id(), session_id);
    assert_eq!(subscription.requested_mode(), RunModeDto::Build);
    assert_eq!(subscription.schema_version(), schema);
}

#[test]
fn jsonrpc_envelope_accessors_preserve_typed_values() {
    let schema = SchemaVersionDto::new(1, 1);
    let session_id = SessionId::new();
    let subscription = SubscribeSessionCommandDto::new(schema, session_id, None, RunModeDto::Build);
    let payload =
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SubscribeSession(subscription));

    let request = encode_request(9, payload.clone());
    assert_eq!(request.id(), 9);
    assert_eq!(
        request.method(),
        ProtocolMethodDto::SessionSubscribe.as_str()
    );
    assert_eq!(request.params(), &payload);

    let line = serde_json::to_string(&request).expect("request serializes");
    let decoded = decode_request_line(&line).expect("request decodes");
    assert_eq!(decoded.id(), 9);
    assert_eq!(decoded.payload(), &payload);
    let decoded_payload = decoded.into_payload();
    assert!(matches!(
        &decoded_payload,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SubscribeSession(command))
            if command == &subscription
    ));

    let health = DaemonHealthDto::new(
        schema,
        ProtocolVersionDto::new(2, 0),
        DaemonReadinessDto::Ready,
    );
    let response = encode_response(
        9,
        ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(health)),
    );
    assert_eq!(response.id(), Some(9));
    assert_eq!(
        response.result_value(),
        Some(&ProtocolResponsePayloadDto::QueryResult(
            ProtocolQueryResultDto::DaemonHealth(health)
        ))
    );
    assert!(response.error_value().is_none());
    let encoded = encode_response(
        9,
        ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(health)),
    );
    assert!(matches!(
        decode_response(
            &serde_json::to_string(&encoded).expect("response serializes"),
            ProtocolMethodDto::DaemonHealth,
            9,
        )
        .expect("response decodes"),
        ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(_))
    ));
}
