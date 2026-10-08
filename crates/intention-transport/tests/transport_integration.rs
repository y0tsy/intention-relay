#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Transport integration fixtures use direct failure assertions for diagnostics."
)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use intention_proto::ErrorDto;
use intention_proto::{
    JsonRpcRequestDto, JsonRpcResponseDto, ProtocolDaemonMessageDto, ProtocolHelloDto,
    ProtocolMethodDto, ProtocolQueryDto, ProtocolQueryResultDto, ProtocolRequestPayloadDto,
    ProtocolResponsePayloadDto, ProtocolVersionDto, RunStreamFrameDto, decode_request_line,
    decode_response, encode_request, encode_response, parse_run_frame_notification,
};
use intention_transport::{
    AsyncLocalClientConnection, AsyncLocalListener, LocalEndpoint, local_protocol_version,
};
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

fn hello(name: &str) -> ProtocolHelloDto {
    ProtocolHelloDto::new(local_protocol_version(), name).expect("fixture hello must be valid")
}

fn hello_at(version: ProtocolVersionDto, name: &str) -> ProtocolHelloDto {
    ProtocolHelloDto::new(version, name).expect("fixture hello must be valid")
}

fn health_request(id: u64) -> JsonRpcRequestDto<ProtocolRequestPayloadDto> {
    encode_request(
        id,
        ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
    )
}

fn rejected_payload() -> ProtocolResponsePayloadDto {
    ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(
        ErrorDto::unavailable("fixture", "fixture unavailable"),
    ))
}

fn unavailable_response(id: u64) -> JsonRpcResponseDto<ProtocolResponsePayloadDto> {
    encode_response(id, rejected_payload())
}

/// Builds one current-state status frame from its wire shape so this crate
/// keeps exercising framing without depending on the domain vocabulary.
fn running_status_frame(
    session_id: intention_proto::SessionId,
    run_id: intention_proto::RunId,
) -> RunStreamFrameDto {
    serde_json::from_value(serde_json::json!({
        "kind": "status",
        "data": { "session_id": session_id, "run_id": run_id, "status": "running" },
    }))
    .expect("current-state status frame decodes")
}

#[tokio::test]
async fn async_connection_preserves_correlated_replies_then_uncorrelated_stream_frames() {
    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
    let session_id = intention_proto::SessionId::new();
    let run_id = intention_proto::RunId::new();
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("server accepts client");
        let (remote, mut requests, mut messages) = connection
            .negotiate(hello("daemon-frame-daemon"))
            .await
            .expect("daemon hello negotiates");
        assert_eq!(remote.adapter_name(), "daemon-frame-client");
        let line = requests
            .receive_line()
            .await
            .expect("server receives request");
        let request = decode_request_line(&line).expect("request decodes");
        assert!(matches!(
            request.payload(),
            ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth)
        ));
        messages
            .send_message(&unavailable_response(request.id()))
            .await
            .expect("server sends correlated response");
        messages
            .send_message(&ProtocolDaemonMessageDto::run_frame(running_status_frame(
                session_id, run_id,
            )))
            .await
            .expect("server sends stream frame");
    });
    let connection = AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("client connects");
    let (remote, mut requests, mut messages) = connection
        .negotiate(hello("daemon-frame-client"))
        .await
        .expect("client hello negotiates");
    assert_eq!(remote.adapter_name(), "daemon-frame-daemon");
    requests
        .send_message(&health_request(1))
        .await
        .expect("request sends");
    let line = messages.receive_line().await.expect("response arrives");
    let response = decode_response(&line, ProtocolMethodDto::DaemonHealth, 1)
        .expect("correlated response decodes");
    assert_eq!(response, rejected_payload());
    let line = messages.receive_line().await.expect("stream frame arrives");
    assert!(matches!(
        parse_run_frame_notification(&line).expect("stream frame parses"),
        RunStreamFrameDto::Status(status)
            if status.session_id() == session_id && status.run_id() == run_id
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
        let (remote, mut requests, mut messages) = connection
            .negotiate(hello("async-fixture-daemon"))
            .await
            .expect("daemon hello negotiates");
        assert_eq!(remote.adapter_name(), "async-fixture-client");
        for _ in 0..3 {
            let line = requests
                .receive_line()
                .await
                .expect("server receives request");
            let request = decode_request_line(&line).expect("request decodes");
            messages
                .send_message(&unavailable_response(request.id()))
                .await
                .expect("server sends response");
        }
    });

    let connection = AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("client connects");
    let (remote, mut requests, mut messages) = connection
        .negotiate(hello("async-fixture-client"))
        .await
        .expect("client hello negotiates");
    assert_eq!(remote.adapter_name(), "async-fixture-daemon");
    for id in 1..=3 {
        requests
            .send_message(&health_request(id))
            .await
            .expect("client sends request");
    }
    for id in 1..=3 {
        let line = messages
            .receive_line()
            .await
            .expect("client receives response");
        assert_eq!(
            decode_response(&line, ProtocolMethodDto::DaemonHealth, id).expect("response decodes"),
            rejected_payload()
        );
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
        let (_, mut requests, mut messages) = connection
            .negotiate(hello("async-fixture-daemon"))
            .await
            .expect("daemon hello negotiates");
        for _ in 0..32 {
            let line = requests
                .receive_line()
                .await
                .expect("server receives request");
            let request = decode_request_line(&line).expect("request decodes");
            messages
                .send_message(&unavailable_response(request.id()))
                .await
                .expect("server sends response");
        }
    });

    let connection = AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("client connects");
    let (_, mut requests, mut messages) = connection
        .negotiate(hello("async-fixture-client"))
        .await
        .expect("client hello negotiates");
    let (_, received) = tokio::join!(
        async {
            for id in 1..=32 {
                requests
                    .send_message(&health_request(id))
                    .await
                    .expect("client sends request");
            }
        },
        async {
            let mut ids = Vec::new();
            for _ in 0..32 {
                let line = messages.receive_line().await.expect("client receives line");
                let response: JsonRpcResponseDto<ProtocolResponsePayloadDto> =
                    JsonRpcResponseDto::parse(&line).expect("response parses");
                ids.push(response.id().expect("responses echo an identity"));
            }
            ids
        }
    );
    assert_eq!(received, (1..=32).collect::<Vec<_>>());
    server.await.expect("server task completes");
}

#[tokio::test]
async fn unavailable_endpoint_is_a_typed_error() {
    let directory = TempDir::new().expect("temporary directory is available");
    let error = match AsyncLocalClientConnection::connect(&endpoint(&directory)).await {
        Ok(_) => panic!("absent endpoint must not connect"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "local_daemon_unavailable");
}

#[tokio::test]
async fn endpoint_identifiers_validate_and_listener_binding_does_not_reclaim_active_names() {
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
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("initial listener binds");
    let second_listener = AsyncLocalListener::bind(endpoint);
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
        let (_, mut requests, mut messages) = connection
            .negotiate(hello("windows-async-daemon"))
            .await
            .expect("named-pipe daemon hello negotiates");
        for _ in 0..2 {
            let line = requests
                .receive_line()
                .await
                .expect("named-pipe request arrives");
            let request = decode_request_line(&line).expect("named-pipe request decodes");
            messages
                .send_message(&unavailable_response(request.id()))
                .await
                .expect("named-pipe response sends");
        }
    });
    let connection = AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("named-pipe client connects");
    let (_, mut requests, mut messages) = connection
        .negotiate(hello("windows-async-client"))
        .await
        .expect("named-pipe client hello negotiates");
    for id in 1..=2 {
        requests
            .send_message(&health_request(id))
            .await
            .expect("named-pipe request sends");
        let line = messages
            .receive_line()
            .await
            .expect("named-pipe response arrives");
        assert_eq!(
            decode_response(&line, ProtocolMethodDto::DaemonHealth, id)
                .expect("named-pipe response decodes"),
            rejected_payload()
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

// Exact-version negotiation must reject a same-major minor mismatch exactly
// like a major mismatch, on every path: the wire carries one protocol version.

#[tokio::test]
async fn async_negotiation_rejects_minor_mismatch_on_the_daemon_side() {
    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("server accepts client");
        let error = connection
            .negotiate(hello("async-fixture-daemon"))
            .await
            .err()
            .expect("async daemon must reject a minor mismatch");
        assert_eq!(error.code(), "incompatible_protocol_version");
    });
    let connection = AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("client connects");
    let client_result = connection
        .negotiate(hello_at(
            ProtocolVersionDto::new(2, 1),
            "async-fixture-client",
        ))
        .await;
    assert!(
        client_result.is_err(),
        "the mismatched client must fail closed without hanging"
    );
    let error = match client_result {
        Ok(_) => panic!("a minor mismatch must not negotiate"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "incompatible_protocol_version");
    server.await.expect("server task completes");
}

#[tokio::test]
async fn async_negotiation_rejects_minor_mismatch_on_the_client_side() {
    // The daemon gate checks the client's exact version before replying, so the
    // fixture daemon offers a future minor version in its own handshake to
    // exercise the client-side exact-version gate.
    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("scripted daemon accepts");
        connection
            .negotiate(hello_at(
                ProtocolVersionDto::new(2, 1),
                "scripted-future-daemon",
            ))
            .await
            .expect("scripted future hello sends");
    });
    let connection = AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("client connects");
    let error = match connection.negotiate(hello("async-fixture-client")).await {
        Ok(_) => panic!("a client of the current version must reject a 2.1 daemon"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "incompatible_protocol_version");
    server.await.expect("scripted daemon completes");
}
