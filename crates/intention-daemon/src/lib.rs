//! Thin daemon process host for the local protocol facade.
//!
//! The daemon owns the local listener and typed connection hosting. It delegates
//! health, query, command, and replay-only subscription meaning to the durable
//! composition facade.

use std::thread;

use intention::DaemonApplicationFacade;
use intention_protocol::{
    ProtocolCapabilityDto, ProtocolHelloDto, ProtocolMessageDto, ProtocolRequestPayloadDto,
    ProtocolResponseEnvelopeDto, ProtocolResponsePayloadDto,
};
use intention_transport::{
    LocalConnection, LocalEndpoint, LocalListener, local_protocol_version, negotiate_daemon,
};
use intention_types::{DtoResult, ErrorDto, SchemaVersionDto, SessionId};

/// Runs the local daemon host until its process is terminated.
///
/// Production startup loads and validates the platform-standard TOML configuration,
/// creates a new credential-free snapshot for this daemon epoch, opens AppData
/// SQLite storage, and completes recovery before the host begins accepting peers.
///
/// # Errors
///
/// Returns a safe typed error if configuration, durable startup, or endpoint
/// binding cannot complete. Per-connection failures are isolated to that
/// connection so a malformed or disconnected client cannot stop the host.
pub fn run(endpoint: LocalEndpoint) -> DtoResult<()> {
    let listener = LocalListener::bind(endpoint)?;
    let facade = DaemonApplicationFacade::open_platform()?;
    serve_listener(listener, facade)
}

/// Runs a daemon host with an explicit deterministic fixture session identity.
///
/// This narrow entry point is used by integration fixtures that must verify that
/// multiple adapters observe one known daemon-owned session. Normal daemon
/// startup must use [`run`] instead.
///
/// # Errors
///
/// Returns a typed error if the endpoint cannot bind or continue accepting peers.
pub fn run_fixture(endpoint: LocalEndpoint, session_id: SessionId) -> DtoResult<()> {
    run_with_facade(
        endpoint,
        DaemonApplicationFacade::new_fixture_with_session_id(session_id),
    )
}

/// Hosts a bounded number of connections for an integration fixture.
///
/// # Errors
///
/// Returns a typed error if the endpoint cannot bind or accept every requested
/// connection. It is not a production lifecycle API.
pub fn serve_fixture_connections(
    endpoint: LocalEndpoint,
    session_id: SessionId,
    connection_count: usize,
) -> DtoResult<()> {
    let listener = LocalListener::bind(endpoint)?;
    let facade = DaemonApplicationFacade::new_fixture_with_session_id(session_id);
    for _ in 0..connection_count {
        let connection = listener.accept()?;
        serve_connection(connection, facade.clone());
    }
    Ok(())
}

fn run_with_facade(endpoint: LocalEndpoint, facade: DaemonApplicationFacade) -> DtoResult<()> {
    serve_listener(LocalListener::bind(endpoint)?, facade)
}

fn serve_listener(listener: LocalListener, facade: DaemonApplicationFacade) -> DtoResult<()> {
    loop {
        match listener.accept() {
            Ok(connection) => {
                let connection_facade = facade.clone();
                let _ = thread::Builder::new()
                    .name("intention-daemon-connection".to_owned())
                    .spawn(move || serve_connection(connection, connection_facade));
            }
            Err(_) => {
                return Err(ErrorDto::unavailable(
                    "local_daemon_listener_unavailable",
                    "the local daemon listener is unavailable",
                ));
            }
        }
    }
}

fn serve_connection(mut connection: LocalConnection, facade: DaemonApplicationFacade) {
    // @todo(m4-streaming)
    let hello = match ProtocolHelloDto::new(
        local_protocol_version(),
        vec![
            ProtocolCapabilityDto::SessionSubscriptions,
            ProtocolCapabilityDto::CorrelatedRequests,
            ProtocolCapabilityDto::DaemonHealth,
        ],
        "intention-daemon",
    ) {
        Ok(hello) => hello,
        Err(_) => return,
    };
    if negotiate_daemon(&mut connection, hello).is_err() {
        return;
    }
    let request = match connection.receive_request() {
        Ok(request) => request,
        Err(_) => return,
    };
    let payload = match request.message().payload() {
        ProtocolRequestPayloadDto::Command(command) => match command {
            intention_protocol::ProtocolCommandDto::SubscribeSession(subscription) => {
                ProtocolResponsePayloadDto::Subscription(facade.subscribe(*subscription))
            }
            _ => ProtocolResponsePayloadDto::CommandResult(facade.command(command.clone())),
        },
        ProtocolRequestPayloadDto::Query(query) => {
            ProtocolResponsePayloadDto::QueryResult(facade.query(*query))
        }
    };
    let response = ProtocolResponseEnvelopeDto::new(
        local_protocol_version(),
        request.correlation_id(),
        ProtocolMessageDto::new(SchemaVersionDto::new(1, 0), payload),
    );
    let _ = connection.send_response(&response);
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "Daemon host fixtures use expect for direct diagnostics."
    )]

    use super::*;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use intention_protocol::{
        ProtocolHelloDto, ProtocolQueryDto, ProtocolQueryResultDto, ProtocolRequestEnvelopeDto,
        ProtocolRequestPayloadDto, SessionResyncReasonDto, SessionSubscriptionResponseDto,
        SubscribeSessionCommandDto,
    };
    use intention_transport::{LocalConnection, local_protocol_version, negotiate_client};
    use intention_types::{CorrelationIdDto, RunId, SessionEventSequenceDto};

    fn endpoint() -> LocalEndpoint {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after Unix epoch")
            .as_nanos();
        LocalEndpoint::from_instance_id(format!("daemon-unit-{nanos}"))
            .expect("fixture endpoint is valid")
    }

    #[test]
    fn daemon_host_serves_health_and_all_protocol_dispatch_paths() {
        let endpoint = endpoint();
        let listener =
            LocalListener::bind(endpoint.clone()).expect("daemon fixture listener binds");
        let _host = thread::spawn(move || {
            let connection = listener
                .accept()
                .expect("daemon fixture accepts connection");
            serve_connection(connection, DaemonApplicationFacade::new_fixture());
        });
        let hello = ProtocolHelloDto::new(
            local_protocol_version(),
            vec![
                ProtocolCapabilityDto::SessionSubscriptions,
                ProtocolCapabilityDto::CorrelatedRequests,
                ProtocolCapabilityDto::DaemonHealth,
            ],
            "daemon-unit-client",
        )
        .expect("fixture hello is valid");
        let mut connection =
            LocalConnection::connect(&endpoint).expect("daemon fixture accepts a local connection");
        negotiate_client(&mut connection, hello).expect("fixture hello negotiates");
        let correlation_id = CorrelationIdDto::new();
        connection
            .send_request(&ProtocolRequestEnvelopeDto::new(
                local_protocol_version(),
                correlation_id,
                ProtocolMessageDto::new(
                    SchemaVersionDto::new(1, 0),
                    ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
                ),
            ))
            .expect("health request sends");
        let response = connection
            .receive_response()
            .expect("health response arrives");
        assert_eq!(response.correlation_id(), correlation_id);
        assert!(matches!(
            response.message().payload(),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(_))
        ));
    }

    #[test]
    fn daemon_rejects_an_endpoint_already_owned_by_another_host() {
        let endpoint = endpoint();
        let _listener = LocalListener::bind(endpoint.clone()).expect("fixture listener binds");
        let error = run(endpoint).expect_err("daemon host cannot reclaim a live endpoint");
        assert_eq!(error.code(), "local_daemon_endpoint_in_use");
    }

    #[test]
    fn deterministic_fixture_host_serves_the_requested_session() {
        let endpoint = endpoint();
        let session_id = SessionId::new();
        let server_endpoint = endpoint.clone();
        let host = thread::spawn(move || {
            serve_fixture_connections(server_endpoint, session_id, 1)
                .expect("fixture host serves one connection");
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let mut connection = loop {
            match LocalConnection::connect(&endpoint) {
                Ok(connection) => break connection,
                Err(_) if std::time::Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("fixture client connects: {error}"),
            }
        };
        let hello = ProtocolHelloDto::new(
            local_protocol_version(),
            vec![
                ProtocolCapabilityDto::SessionSubscriptions,
                ProtocolCapabilityDto::CorrelatedRequests,
                ProtocolCapabilityDto::DaemonHealth,
            ],
            "daemon-unit-client",
        )
        .expect("fixture hello is valid");
        negotiate_client(&mut connection, hello).expect("fixture hello negotiates");
        connection
            .send_request(&ProtocolRequestEnvelopeDto::new(
                local_protocol_version(),
                CorrelationIdDto::new(),
                ProtocolMessageDto::new(
                    SchemaVersionDto::new(1, 0),
                    ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetSessionSnapshot(
                        intention_domain::GetSessionSnapshotQueryDto::new(session_id),
                    )),
                ),
            ))
            .expect("fixture snapshot request sends");
        assert!(matches!(
            connection
                .receive_response()
                .expect("fixture snapshot response arrives")
                .message()
                .payload(),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::SessionSnapshot(snapshot))
                if snapshot.session_id() == session_id
        ));
        host.join().expect("fixture host completes");
    }

    #[test]
    fn daemon_dispatches_run_scoped_subscription_as_typed_resync() {
        let endpoint = endpoint();
        let session_id = SessionId::new();
        let server_endpoint = endpoint.clone();
        let host = thread::spawn(move || {
            serve_fixture_connections(server_endpoint, session_id, 1)
                .expect("fixture host serves one connection");
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let mut connection = loop {
            match LocalConnection::connect(&endpoint) {
                Ok(connection) => break connection,
                Err(_) if std::time::Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("fixture client connects: {error}"),
            }
        };
        negotiate_client(
            &mut connection,
            ProtocolHelloDto::new(
                local_protocol_version(),
                vec![
                    ProtocolCapabilityDto::SessionSubscriptions,
                    ProtocolCapabilityDto::CorrelatedRequests,
                    ProtocolCapabilityDto::DaemonHealth,
                ],
                "daemon-unit-client",
            )
            .expect("fixture hello is valid"),
        )
        .expect("fixture hello negotiates");
        connection
            .send_request(&ProtocolRequestEnvelopeDto::new(
                local_protocol_version(),
                CorrelationIdDto::new(),
                ProtocolMessageDto::new(
                    SchemaVersionDto::new(1, 0),
                    ProtocolRequestPayloadDto::Command(
                        intention_protocol::ProtocolCommandDto::SubscribeSession(
                            SubscribeSessionCommandDto::with_run_id(
                                SchemaVersionDto::new(1, 0),
                                session_id,
                                Some(RunId::new()),
                                Some(SessionEventSequenceDto::new(0)),
                                intention_domain::RunModeDto::Build,
                            ),
                        ),
                    ),
                ),
            ))
            .expect("scoped subscription sends");
        assert!(matches!(
            connection
                .receive_response()
                .expect("scoped subscription response arrives")
                .message()
                .payload(),
            ProtocolResponsePayloadDto::Subscription(
                SessionSubscriptionResponseDto::ResyncRequired(resync)
            ) if resync.session_id() == session_id
                && resync.reason() == SessionResyncReasonDto::HistoryUnavailable
        ));
        host.join().expect("fixture host completes");
    }

    #[test]
    fn malformed_hello_and_request_are_isolated_to_the_connection() {
        let endpoint = endpoint();
        let listener = LocalListener::bind(endpoint.clone()).expect("fixture listener binds");
        let host = thread::spawn(move || {
            for _ in 0..2 {
                let connection = listener.accept().expect("fixture listener accepts");
                serve_connection(connection, DaemonApplicationFacade::new_fixture());
            }
        });

        let mut malformed_hello =
            LocalConnection::connect(&endpoint).expect("malformed hello client connects");
        malformed_hello
            .send_hello(
                &ProtocolHelloDto::new(
                    intention_protocol::ProtocolVersionDto::new(2, 0),
                    Vec::new(),
                    "incompatible-client",
                )
                .expect("fixture hello is valid"),
            )
            .expect("fixture hello sends");
        drop(malformed_hello);

        let mut malformed_request =
            LocalConnection::connect(&endpoint).expect("malformed request client connects");
        negotiate_client(
            &mut malformed_request,
            ProtocolHelloDto::new(
                local_protocol_version(),
                vec![
                    ProtocolCapabilityDto::SessionSubscriptions,
                    ProtocolCapabilityDto::CorrelatedRequests,
                    ProtocolCapabilityDto::DaemonHealth,
                ],
                "daemon-unit-client",
            )
            .expect("fixture hello is valid"),
        )
        .expect("compatible hello negotiates");
        drop(malformed_request);
        host.join().expect("host finishes after isolated failures");
    }
}
