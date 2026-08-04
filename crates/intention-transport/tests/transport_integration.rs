#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Transport integration fixtures use direct failure assertions for diagnostics."
)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use intention_protocol::{
    ProtocolCapabilityDto, ProtocolDaemonFrameDto, ProtocolHelloDto, ProtocolMessageDto,
    ProtocolQueryDto, ProtocolQueryResultDto, ProtocolRequestEnvelopeDto,
    ProtocolRequestPayloadDto, ProtocolResponseEnvelopeDto, ProtocolResponsePayloadDto,
    ProtocolVersionDto, RunResyncDto, RunResyncReasonDto, RunStreamFrameDto,
};
use intention_transport::{
    AsyncLocalClientConnection, AsyncLocalListener, LocalConnection, LocalEndpoint, LocalListener,
    local_protocol_version, negotiate_client, negotiate_daemon,
};
use intention_types::{CorrelationIdDto, SchemaVersionDto};
use tempfile::TempDir;

static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(0);

fn endpoint(_directory: &TempDir) -> LocalEndpoint {
    let sequence = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time must be after Unix epoch")
        .as_nanos();
    LocalEndpoint::from_instance_id(format!("transport-fixture-{nanos}-{sequence}"))
        .expect("fixture instance name must be valid")
}

fn hello(name: &str, version: ProtocolVersionDto) -> ProtocolHelloDto {
    ProtocolHelloDto::new(
        version,
        vec![
            ProtocolCapabilityDto::SessionSubscriptions,
            ProtocolCapabilityDto::CorrelatedRequests,
            ProtocolCapabilityDto::DaemonHealth,
        ],
        name,
    )
    .expect("fixture hello must be valid")
}

fn health_request() -> ProtocolRequestEnvelopeDto {
    ProtocolRequestEnvelopeDto::new(
        local_protocol_version(),
        CorrelationIdDto::new(),
        ProtocolMessageDto::new(
            SchemaVersionDto::new(1, 0),
            ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
        ),
    )
}

fn unavailable_response(correlation_id: CorrelationIdDto) -> ProtocolResponseEnvelopeDto {
    ProtocolResponseEnvelopeDto::new(
        local_protocol_version(),
        correlation_id,
        ProtocolMessageDto::new(
            SchemaVersionDto::new(1, 0),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(
                intention_types::ErrorDto::unavailable("fixture", "fixture unavailable"),
            )),
        ),
    )
}

#[tokio::test]
async fn async_daemon_frame_roles_preserve_correlated_replies_then_uncorrelated_stream_frames() {
    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
    let session_id = intention_types::SessionId::new();
    let run_id = intention_types::RunId::new();
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("server accepts client");
        let (_, mut requests, mut frames) = connection
            .negotiate_daemon_frames(hello("daemon-frame-daemon", local_protocol_version()))
            .await
            .expect("daemon hello negotiates");
        let request = requests.receive().await.expect("server receives request");
        frames
            .send(&ProtocolDaemonFrameDto::Response(unavailable_response(
                request.correlation_id(),
            )))
            .await
            .expect("server sends correlated response");
        frames
            .send(&ProtocolDaemonFrameDto::RunStream(
                RunStreamFrameDto::Resync(RunResyncDto::new(
                    session_id,
                    run_id,
                    RunResyncReasonDto::SubscriberTooSlow,
                )),
            ))
            .await
            .expect("server sends stream frame");
    });
    let connection = AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("client connects");
    let (_, mut requests, mut frames) = connection
        .negotiate_daemon_frames(hello("daemon-frame-client", local_protocol_version()))
        .await
        .expect("client hello negotiates");
    let request = health_request();
    requests.send(&request).await.expect("request sends");
    assert!(matches!(
        frames.receive().await.expect("response arrives"),
        ProtocolDaemonFrameDto::Response(response) if response.correlation_id() == request.correlation_id()
    ));
    assert!(matches!(
        frames.receive().await.expect("stream frame arrives"),
        ProtocolDaemonFrameDto::RunStream(RunStreamFrameDto::Resync(resync))
            if resync.session_id() == session_id && resync.run_id() == run_id
    ));
    server.await.expect("server task completes");
}

#[tokio::test]
async fn async_transport_negotiates_once_and_exchanges_ordered_correlated_frames() {
    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("server accepts client");
        let (remote, mut requests, mut responses) = connection
            .negotiate(hello("async-fixture-daemon", local_protocol_version()))
            .await
            .expect("daemon hello negotiates");
        assert_eq!(remote.adapter_name(), "async-fixture-client");
        for _ in 0..3 {
            let request = requests.receive().await.expect("server receives request");
            responses
                .send(&unavailable_response(request.correlation_id()))
                .await
                .expect("server sends response");
        }
    });

    let connection = AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("client connects");
    let (remote, mut requests, mut responses) = connection
        .negotiate(hello("async-fixture-client", local_protocol_version()))
        .await
        .expect("client hello negotiates");
    assert_eq!(remote.adapter_name(), "async-fixture-daemon");
    let correlations = (0..3)
        .map(|_| {
            let request = health_request();
            let correlation_id = request.correlation_id();
            (request, correlation_id)
        })
        .collect::<Vec<_>>();
    for (request, _) in &correlations {
        requests.send(request).await.expect("client sends request");
    }
    for (_, correlation_id) in correlations {
        let response = responses.receive().await.expect("client receives response");
        assert_eq!(response.correlation_id(), correlation_id);
    }
    server.await.expect("server task completes");
}

#[tokio::test]
async fn async_transport_split_roles_exchange_concurrent_multiple_frames_without_corruption() {
    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("server accepts client");
        let (_, mut requests, mut responses) = connection
            .negotiate(hello("async-fixture-daemon", local_protocol_version()))
            .await
            .expect("daemon hello negotiates");
        for _ in 0..32 {
            let request = requests.receive().await.expect("server receives request");
            responses
                .send(&unavailable_response(request.correlation_id()))
                .await
                .expect("server sends response");
        }
    });

    let connection = AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("client connects");
    let (_, mut requests, mut responses) = connection
        .negotiate(hello("async-fixture-client", local_protocol_version()))
        .await
        .expect("client hello negotiates");
    let (sent, received) = tokio::join!(
        async {
            let mut correlations = Vec::new();
            for _ in 0..32 {
                let request = health_request();
                let correlation_id = request.correlation_id();
                requests.send(&request).await.expect("client sends request");
                correlations.push(correlation_id);
            }
            correlations
        },
        async {
            let mut correlations = Vec::new();
            for _ in 0..32 {
                correlations.push(
                    responses
                        .receive()
                        .await
                        .expect("client receives response")
                        .correlation_id(),
                );
            }
            correlations
        }
    );
    assert_eq!(received, sent);
    server.await.expect("server task completes");
}

#[test]
fn framed_transport_negotiates_and_preserves_correlated_dtos() {
    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
    let listener = LocalListener::bind(endpoint.clone()).expect("listener binds");
    let client_endpoint = endpoint;

    let server = thread::spawn(move || {
        let mut connection = listener.accept().expect("server accepts client");
        let remote = negotiate_daemon(
            &mut connection,
            hello("fixture-daemon", local_protocol_version()),
        )
        .expect("compatible hello negotiates");
        assert_eq!(remote.adapter_name(), "fixture-client");
        let request = connection
            .receive_request()
            .expect("server receives request");
        let response = ProtocolResponseEnvelopeDto::new(
            local_protocol_version(),
            request.correlation_id(),
            ProtocolMessageDto::new(
                SchemaVersionDto::new(1, 0),
                ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(
                    intention_types::ErrorDto::unavailable("fixture", "fixture unavailable"),
                )),
            ),
        );
        connection
            .send_response(&response)
            .expect("server sends response");
    });

    let mut client = LocalConnection::connect(&client_endpoint).expect("client connects");
    let remote = negotiate_client(
        &mut client,
        hello("fixture-client", local_protocol_version()),
    )
    .expect("compatible hello negotiates");
    assert_eq!(remote.adapter_name(), "fixture-daemon");
    let request = health_request();
    client.send_request(&request).expect("request sends");
    let response = client.receive_response().expect("response arrives");
    assert_eq!(response.correlation_id(), request.correlation_id());

    server.join().expect("server thread completes");
}

#[test]
fn incompatible_protocol_major_fails_closed_without_hanging() {
    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
    let listener = LocalListener::bind(endpoint.clone()).expect("listener binds");
    let client_endpoint = endpoint;

    let server = thread::spawn(move || {
        let mut connection = listener.accept().expect("server accepts client");
        let error = negotiate_daemon(
            &mut connection,
            hello("fixture-daemon", local_protocol_version()),
        )
        .expect_err("mismatched major must fail");
        assert_eq!(error.code(), "incompatible_protocol_version");
    });

    let mut client = LocalConnection::connect(&client_endpoint).expect("client connects");
    client
        .send_hello(&hello("fixture-client", ProtocolVersionDto::new(2, 0)))
        .expect("client hello sends");

    server.join().expect("server thread completes");
}

#[test]
fn unavailable_endpoint_is_a_typed_error() {
    let directory = TempDir::new().expect("temporary directory is available");
    let error = match LocalConnection::connect(&endpoint(&directory)) {
        Ok(_) => panic!("absent endpoint must not connect"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "local_daemon_unavailable");
}

#[test]
fn endpoint_identifiers_validate_and_listener_binding_does_not_reclaim_active_names() {
    let long_instance_id = "x".repeat(101);
    for instance_id in [
        "",
        "has space",
        "../escape",
        "slash/name",
        long_instance_id.as_str(),
    ] {
        let error = LocalEndpoint::from_instance_id(instance_id)
            .expect_err("unsafe logical endpoint identifier must be rejected");
        assert_eq!(error.code(), "invalid_local_endpoint_instance");
    }

    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
    assert!(endpoint.instance_id().starts_with("transport-fixture-"));
    let listener = LocalListener::bind(endpoint.clone()).expect("initial listener binds");
    let second_listener = LocalListener::bind(endpoint);
    assert!(
        second_listener.is_err(),
        "active endpoint must not be reclaimed"
    );
    let error = match second_listener {
        Ok(_) => panic!("active endpoint must not be reclaimed"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "local_daemon_endpoint_in_use");
    drop(listener);
}

#[cfg(windows)]
#[tokio::test]
async fn windows_async_named_pipe_fixture_negotiates_multiple_frames_and_cleans_up() {
    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("named-pipe listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("named-pipe server accepts");
        let (_, mut requests, mut responses) = connection
            .negotiate(hello("windows-async-daemon", local_protocol_version()))
            .await
            .expect("named-pipe daemon hello negotiates");
        for _ in 0..2 {
            let request = requests
                .receive()
                .await
                .expect("named-pipe request arrives");
            responses
                .send(&unavailable_response(request.correlation_id()))
                .await
                .expect("named-pipe response sends");
        }
    });
    let connection = AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("named-pipe client connects");
    let (_, mut requests, mut responses) = connection
        .negotiate(hello("windows-async-client", local_protocol_version()))
        .await
        .expect("named-pipe client hello negotiates");
    for _ in 0..2 {
        let request = health_request();
        requests
            .send(&request)
            .await
            .expect("named-pipe request sends");
        assert_eq!(
            responses
                .receive()
                .await
                .expect("named-pipe response arrives")
                .correlation_id(),
            request.correlation_id()
        );
    }
    server.await.expect("named-pipe server completes");
    assert!(
        AsyncLocalClientConnection::connect(&endpoint)
            .await
            .is_err(),
        "dropping the named-pipe listener removes its endpoint"
    );
}

#[cfg(windows)]
#[test]
fn windows_named_pipe_fixture_binds_negotiates_frames_and_cleans_up() {
    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
    let listener = LocalListener::bind(endpoint.clone()).expect("named-pipe listener binds");
    let client_endpoint = endpoint;
    let server = thread::spawn(move || {
        let mut connection = listener
            .accept()
            .expect("named-pipe listener accepts client");
        negotiate_daemon(
            &mut connection,
            hello("windows-fixture-daemon", local_protocol_version()),
        )
        .expect("named-pipe hello negotiates");
        let request = connection
            .receive_request()
            .expect("named-pipe request arrives");
        connection
            .send_response(&ProtocolResponseEnvelopeDto::new(
                local_protocol_version(),
                request.correlation_id(),
                ProtocolMessageDto::new(
                    SchemaVersionDto::new(1, 0),
                    ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(
                        intention_types::ErrorDto::unavailable("fixture", "fixture unavailable"),
                    )),
                ),
            ))
            .expect("named-pipe response sends");
    });
    let mut client =
        LocalConnection::connect(&client_endpoint).expect("named-pipe client connects");
    negotiate_client(
        &mut client,
        hello("windows-fixture-client", local_protocol_version()),
    )
    .expect("named-pipe hello negotiates");
    let request = health_request();
    client
        .send_request(&request)
        .expect("named-pipe request sends");
    assert_eq!(
        client
            .receive_response()
            .expect("named-pipe response arrives")
            .correlation_id(),
        request.correlation_id()
    );
    server.join().expect("named-pipe server completes");
}

#[test]
fn client_negotiation_rejects_an_incompatible_daemon_response() {
    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
    let listener = LocalListener::bind(endpoint.clone()).expect("listener binds");
    let client_endpoint = endpoint;

    let server = thread::spawn(move || {
        let mut connection = listener.accept().expect("server accepts client");
        let remote = connection.receive_hello().expect("server receives hello");
        assert_eq!(remote.adapter_name(), "fixture-client");
        connection
            .send_hello(&hello("fixture-daemon", ProtocolVersionDto::new(2, 0)))
            .expect("server sends incompatible hello");
    });

    let mut client = LocalConnection::connect(&client_endpoint).expect("client connects");
    let error = negotiate_client(
        &mut client,
        hello("fixture-client", local_protocol_version()),
    )
    .expect_err("client must reject incompatible daemon version");
    assert_eq!(error.code(), "incompatible_protocol_version");

    server.join().expect("server thread completes");
}
