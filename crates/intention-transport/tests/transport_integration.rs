#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Transport integration fixtures use direct failure assertions for diagnostics."
)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use intention_proto::{
    ClientRequestDto, ErrorDto, ProtocolDaemonMessageDto, RunStreamFrameDto, decode_request_line,
    decode_response, encode_request, parse_run_frame,
};
use intention_transport::{AsyncLocalClientConnection, AsyncLocalListener, LocalEndpoint};
use tempfile::TempDir;

static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(0);

fn endpoint(directory: &TempDir) -> LocalEndpoint {
    let sequence = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time must be after Unix epoch")
        .as_nanos();
    let instance_id = format!("transport-fixture-{nanos}-{sequence}");
    #[cfg(unix)]
    {
        // The fixture root is its own temporary directory, so no test creates or
        // chmods an endpoint inside the live platform runtime directory.
        LocalEndpoint::from_instance_id_in(directory.path(), instance_id)
            .expect("fixture instance name must be valid")
    }
    #[cfg(windows)]
    {
        let _ = directory;
        LocalEndpoint::from_instance_id(instance_id).expect("fixture instance name must be valid")
    }
}

const fn health_request(id: u64) -> intention_proto::ProtocolRequestDto {
    encode_request(id, ClientRequestDto::GetDaemonHealth)
}

fn unavailable_message(id: u64) -> ProtocolDaemonMessageDto {
    ProtocolDaemonMessageDto::rejection(
        Some(id),
        ErrorDto::unavailable("fixture", "fixture unavailable"),
    )
}

/// Builds one committed status frame carrying a full run projection.
fn status_frame(
    session_id: intention_proto::SessionId,
    run_id: intention_proto::RunId,
) -> RunStreamFrameDto {
    RunStreamFrameDto::Status(intention_proto::RunProjectionDto::new(
        session_id,
        run_id,
        intention_proto::TurnId::new(),
        intention_proto::RunStatusDto::Running,
        intention_proto::ConfigRevisionId::new(),
    ))
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
        let (mut requests, mut messages) = connection.split();
        let line = requests
            .receive_line()
            .await
            .expect("server receives request");
        let request = decode_request_line(&line).expect("request decodes");
        assert_eq!(request.request(), &ClientRequestDto::GetDaemonHealth);
        messages
            .send_message(&unavailable_message(request.id()))
            .await
            .expect("server sends correlated rejection");
        messages
            .send_message(&ProtocolDaemonMessageDto::frame(status_frame(
                session_id, run_id,
            )))
            .await
            .expect("server sends stream frame");
    });
    let connection = AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("client connects");
    let (mut requests, mut messages) = connection.split();
    requests
        .send_message(&health_request(1))
        .await
        .expect("request sends");
    let line = messages.receive_line().await.expect("rejection arrives");
    assert_eq!(
        decode_response(&line, 1)
            .expect_err("the correlated rejection surfaces its error")
            .code(),
        "fixture"
    );
    let line = messages.receive_line().await.expect("stream frame arrives");
    assert!(matches!(
        parse_run_frame(&line).expect("stream frame parses"),
        RunStreamFrameDto::Status(run)
            if run.session_id() == session_id && run.run_id() == run_id
    ));
    server.await.expect("server task completes");
}

#[tokio::test]
async fn async_link_exchanges_concurrent_multiple_messages_without_corruption() {
    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("server accepts client");
        let (mut requests, mut messages) = connection.split();
        for _ in 0..32 {
            let line = requests
                .receive_line()
                .await
                .expect("server receives request");
            let request = decode_request_line(&line).expect("request decodes");
            messages
                .send_message(&unavailable_message(request.id()))
                .await
                .expect("server sends rejection");
        }
    });

    let connection = AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("client connects");
    let (mut requests, mut messages) = connection.split();
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
                let message = intention_proto::parse_daemon_message(&line)
                    .expect("daemon message is current");
                let ProtocolDaemonMessageDto::Rejection(rejection) = message else {
                    panic!("the fixture daemon answers with a rejection")
                };
                ids.push(rejection.id().expect("rejections echo an identity"));
            }
            ids
        }
    );
    assert_eq!(received, (1..=32).collect::<Vec<_>>());
    server.await.expect("server task completes");
}

#[test]
fn endpoint_identifiers_reject_unsafe_instance_ids() {
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
}

#[cfg(windows)]
#[tokio::test]
async fn windows_async_named_pipe_fixture_exchanges_multiple_messages_and_cleans_up() {
    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("named-pipe listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("named-pipe server accepts");
        let (mut requests, mut messages) = connection.split();
        for _ in 0..2 {
            let line = requests
                .receive_line()
                .await
                .expect("named-pipe request arrives");
            let request = decode_request_line(&line).expect("named-pipe request decodes");
            messages
                .send_message(&unavailable_message(request.id()))
                .await
                .expect("named-pipe rejection sends");
        }
    });
    let connection = AsyncLocalClientConnection::connect(&endpoint)
        .await
        .expect("named-pipe client connects");
    let (mut requests, mut messages) = connection.split();
    for id in 1..=2 {
        requests
            .send_message(&health_request(id))
            .await
            .expect("named-pipe request sends");
        let line = messages
            .receive_line()
            .await
            .expect("named-pipe rejection arrives");
        assert_eq!(
            decode_response(&line, id)
                .expect_err("named-pipe rejection decodes")
                .code(),
            "fixture"
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
