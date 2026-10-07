#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Fixture-host integration uses explicit protocol diagnostics."
)]

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use intention_proto::{GetSessionSnapshotQueryDto, RunModeDto};
use intention_proto::{
    ProtocolHelloDto, ProtocolMethodDto, ProtocolQueryDto, ProtocolQueryResultDto,
    ProtocolRequestPayloadDto, ProtocolResponsePayloadDto, SessionSubscriptionResponseDto,
    SubscribeSessionCommandDto, decode_response, encode_request,
};
use intention_proto::{RunId, SchemaVersionDto, SessionId};
use intention_test_support::FixtureHost;
use intention_transport::{
    AsyncLocalClientConnection, AsyncMessageReceiver, AsyncRequestSender, LocalEndpoint,
    local_protocol_version,
};

fn endpoint() -> LocalEndpoint {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time is after Unix epoch")
        .as_nanos();
    LocalEndpoint::from_instance_id(format!("test-support-host-{nanos}"))
        .expect("fixture endpoint is valid")
}

async fn connect(endpoint: &LocalEndpoint) -> (AsyncRequestSender, AsyncMessageReceiver) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let connection = match AsyncLocalClientConnection::connect(endpoint).await {
            Ok(connection) => connection,
            Err(error) => {
                assert!(
                    Instant::now() < deadline,
                    "fixture client connects before the deadline: {error}"
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
                continue;
            }
        };
        let (_, requests, messages) = connection
            .negotiate(
                ProtocolHelloDto::new(local_protocol_version(), "test-support-client")
                    .expect("fixture hello is valid"),
            )
            .await
            .expect("fixture hello negotiates");
        return (requests, messages);
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
            ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetSessionSnapshot(
                GetSessionSnapshotQueryDto::new(session_id),
            )),
        ))
        .await
        .expect("snapshot request sends");
    let line = messages
        .receive_line()
        .await
        .expect("snapshot response arrives");
    let response = decode_response(&line, ProtocolMethodDto::SessionSnapshot, 1)
        .expect("snapshot response decodes");
    assert!(matches!(
        response,
        ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::SessionSnapshot(snapshot))
            if snapshot.session_id() == session_id
    ));

    let (mut requests, mut messages) = connect(&endpoint).await;
    requests
        .send_message(&encode_request(
            1,
            ProtocolRequestPayloadDto::Command(
                intention_proto::ProtocolCommandDto::SubscribeSession(
                    SubscribeSessionCommandDto::with_run_id(
                        SchemaVersionDto::new(1, 1),
                        session_id,
                        Some(RunId::new()),
                        RunModeDto::Build,
                    ),
                ),
            ),
        ))
        .await
        .expect("scoped request sends");
    let line = messages
        .receive_line()
        .await
        .expect("scoped response arrives");
    let response = decode_response(&line, ProtocolMethodDto::SessionSubscribe, 1)
        .expect("scoped response decodes");
    assert!(
        matches!(
            response,
            ProtocolResponsePayloadDto::Subscription(SessionSubscriptionResponseDto::Error(error))
                if error.code() == "storage_record_not_found"
        ),
        "an unknown run scope is refused with a typed error instead of a resync"
    );

    host.await
        .expect("fixture host task completes")
        .expect("fixture host serves both connections");
}
