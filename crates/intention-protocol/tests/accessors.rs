#![allow(
    clippy::expect_used,
    reason = "Protocol accessor coverage uses expect for fixture diagnostics."
)]

use intention_domain::RunModeDto;
use intention_protocol::{
    DaemonHealthDto, DaemonReadinessDto, ProtocolMessageDto, ProtocolRequestEnvelopeDto,
    ProtocolRequestPayloadDto, ProtocolVersionDto, SessionEventTailBatchDto, SessionResyncDto,
    SessionResyncReasonDto, SessionSnapshotDto, SubscribeSessionCommandDto,
};
use intention_types::{CorrelationIdDto, SchemaVersionDto, SessionEventSequenceDto, SessionId};

#[test]
fn public_protocol_accessors_preserve_typed_values() {
    let schema = SchemaVersionDto::new(1, 0);
    let version = ProtocolVersionDto::new(1, 3);
    let health = DaemonHealthDto::new(schema, version, DaemonReadinessDto::Starting);
    assert_eq!(health.schema_version(), schema);
    assert_eq!(health.protocol_version(), version);
    assert_eq!(health.readiness(), DaemonReadinessDto::Starting);

    let session_id = SessionId::new();
    let snapshot = SessionSnapshotDto::new(schema, session_id, SessionEventSequenceDto::new(4));
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
    let message = ProtocolMessageDto::new(
        schema,
        ProtocolRequestPayloadDto::Command(
            intention_protocol::ProtocolCommandDto::SubscribeSession(subscription),
        ),
    );
    assert_eq!(message.schema_version(), schema);
    let envelope = ProtocolRequestEnvelopeDto::new(version, CorrelationIdDto::new(), message);
    assert_eq!(envelope.protocol_version(), version);
    assert!(matches!(
        envelope.message().payload(),
        ProtocolRequestPayloadDto::Command(_)
    ));
    let payload = envelope.message().clone().into_payload();
    assert!(matches!(payload, ProtocolRequestPayloadDto::Command(_)));
}
