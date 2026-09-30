#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Fixture-host integration uses explicit protocol diagnostics."
)]

use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use intention_protocol::{
    ProtocolCommandDto, ProtocolHelloDto, ProtocolMethodDto, ProtocolQueryDto,
    ProtocolQueryResultDto, ProtocolRequestPayloadDto, ProtocolResponsePayloadDto,
    SessionResyncReasonDto, SessionSubscriptionResponseDto, SubscribeSessionCommandDto,
    decode_response, encode_request,
};
use intention_test_support::FixtureHost;
use intention_transport::{
    LocalConnection, LocalEndpoint, local_protocol_version, negotiate_client,
};
use intention_types::{RunId, SchemaVersionDto, SessionEventSequenceDto, SessionId};

fn endpoint() -> LocalEndpoint {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time is after Unix epoch")
        .as_nanos();
    LocalEndpoint::from_instance_id(format!("test-support-host-{nanos}"))
        .expect("fixture endpoint is valid")
}

fn connect(endpoint: &LocalEndpoint) -> LocalConnection {
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    loop {
        match LocalConnection::connect(endpoint) {
            Ok(connection) => return connection,
            Err(_) if std::time::Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(5))
            }
            Err(error) => panic!("fixture client connects: {error}"),
        }
    }
}

fn negotiate(connection: &mut LocalConnection) {
    negotiate_client(
        connection,
        ProtocolHelloDto::new(local_protocol_version(), "test-support-client")
            .expect("fixture hello is valid"),
    )
    .expect("fixture hello negotiates");
}

#[test]
fn fixture_host_serves_durable_snapshot_and_scoped_resync() {
    let session_id = SessionId::new();
    let fixture = FixtureHost::open(session_id).expect("fixture host opens");
    let endpoint = endpoint();
    let host = fixture.spawn(endpoint.clone(), 2);

    let mut snapshot_connection = connect(&endpoint);
    negotiate(&mut snapshot_connection);
    snapshot_connection
        .send_message(&encode_request(
            1,
            ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetSessionSnapshot(
                intention_domain::GetSessionSnapshotQueryDto::new(session_id),
            )),
        ))
        .expect("snapshot request sends");
    let line = snapshot_connection
        .receive_line()
        .expect("snapshot response arrives");
    let response = decode_response(&line, ProtocolMethodDto::SessionSnapshot, 1)
        .expect("snapshot response decodes");
    assert!(matches!(
        response,
        ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::SessionSnapshot(snapshot))
            if snapshot.session_id() == session_id
    ));

    let mut scoped_connection = connect(&endpoint);
    negotiate(&mut scoped_connection);
    scoped_connection
        .send_message(&encode_request(
            1,
            ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SubscribeSession(
                SubscribeSessionCommandDto::with_run_id(
                    SchemaVersionDto::new(1, 1),
                    session_id,
                    Some(RunId::new()),
                    Some(SessionEventSequenceDto::new(u64::MAX)),
                    intention_domain::RunModeDto::Build,
                ),
            )),
        ))
        .expect("scoped request sends");
    let line = scoped_connection
        .receive_line()
        .expect("scoped response arrives");
    let response = decode_response(&line, ProtocolMethodDto::SessionSubscribe, 1)
        .expect("scoped response decodes");
    assert!(matches!(
        response,
        ProtocolResponsePayloadDto::Subscription(SessionSubscriptionResponseDto::ResyncRequired(
            resync
        )) if resync.reason() == SessionResyncReasonDto::HistoryUnavailable
    ));
    host.join()
        .expect("fixture host thread completes")
        .expect("fixture host serves both connections");
}
