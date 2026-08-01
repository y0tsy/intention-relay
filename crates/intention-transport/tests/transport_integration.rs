#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Transport integration fixtures use direct failure assertions for diagnostics."
)]

use std::thread;

use intention_protocol::{
    ProtocolCapabilityDto, ProtocolHelloDto, ProtocolMessageDto, ProtocolQueryDto,
    ProtocolQueryResultDto, ProtocolRequestEnvelopeDto, ProtocolRequestPayloadDto,
    ProtocolResponseEnvelopeDto, ProtocolResponsePayloadDto, ProtocolVersionDto,
};
use intention_transport::{
    LocalConnection, LocalEndpoint, LocalListener, local_protocol_version, negotiate_client,
    negotiate_daemon,
};
use intention_types::{CorrelationIdDto, SchemaVersionDto};
use tempfile::TempDir;

fn endpoint(directory: &TempDir) -> LocalEndpoint {
    LocalEndpoint::from_path(directory.path().join("fixture.sock"))
        .expect("temporary endpoint must be absolute")
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

#[test]
fn framed_transport_negotiates_and_preserves_correlated_dtos() {
    let directory = TempDir::new().expect("temporary directory is available");
    let listener = LocalListener::bind(endpoint(&directory)).expect("listener binds");
    let client_endpoint = endpoint(&directory);

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
    let listener = LocalListener::bind(endpoint(&directory)).expect("listener binds");
    let client_endpoint = endpoint(&directory);

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

#[cfg(unix)]
#[test]
fn unix_listener_uses_owner_only_socket_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let directory = TempDir::new().expect("temporary directory is available");
    let endpoint = endpoint(&directory);
    let listener = LocalListener::bind(endpoint.clone()).expect("listener binds");
    let mode = std::fs::metadata(endpoint.as_path())
        .expect("socket metadata is available")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    drop(listener);
}
