#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Fixture-host integration uses explicit protocol diagnostics."
)]

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use intention_proto::{ClientRequestDto, GetSessionSnapshotQueryDto, ProtocolResultDto, RunId};
use intention_proto::{SessionId, SubscribeRunCommandDto, decode_response, encode_request};
use intention_test_support::FixtureHost;
use intention_transport::{
    AsyncLocalClientConnection, AsyncMessageReceiver, AsyncMessageSender, LocalEndpoint,
};

fn endpoint() -> LocalEndpoint {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time is after Unix epoch")
        .as_nanos();
    LocalEndpoint::from_instance_id(format!("test-support-host-{nanos}"))
        .expect("fixture endpoint is valid")
}

async fn connect(endpoint: &LocalEndpoint) -> (AsyncMessageSender, AsyncMessageReceiver) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match AsyncLocalClientConnection::connect(endpoint).await {
            Ok(connection) => return connection.split(),
            Err(error) => {
                assert!(
                    Instant::now() < deadline,
                    "fixture client connects before the deadline: {error}"
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
}

#[tokio::test]
async fn fixture_host_serves_current_session_state_and_typed_scoped_refusals() {
    let session_id = SessionId::new();
    let fixture = FixtureHost::open(session_id).expect("fixture host opens");
    let endpoint = endpoint();
    let host = tokio::spawn(fixture.serve(endpoint.clone(), 2));

    let (mut requests, mut messages) = connect(&endpoint).await;
    requests
        .send_message(&encode_request(
            1,
            ClientRequestDto::GetSessionSnapshot(GetSessionSnapshotQueryDto::new(session_id)),
        ))
        .await
        .expect("snapshot request sends");
    let line = messages
        .receive_line()
        .await
        .expect("snapshot response arrives");
    let response = decode_response(&line, 1).expect("snapshot response decodes");
    assert!(matches!(
        response,
        ProtocolResultDto::SessionSnapshot(snapshot) if snapshot.session_id() == session_id
    ));

    let (mut requests, mut messages) = connect(&endpoint).await;
    requests
        .send_message(&encode_request(
            1,
            ClientRequestDto::SubscribeRun(SubscribeRunCommandDto::new(session_id, RunId::new())),
        ))
        .await
        .expect("scoped request sends");
    let line = messages
        .receive_line()
        .await
        .expect("scoped response arrives");
    let error = decode_response(&line, 1).expect_err("an unknown run scope is refused");
    assert_eq!(
        error.code(),
        "storage_record_not_found",
        "an unknown run scope is refused with a typed error instead of a resync"
    );

    host.await
        .expect("fixture host task completes")
        .expect("fixture host serves both connections");
}
