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
use intention_types::{DtoResult, ErrorDto, SchemaVersionDto};

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

#[cfg(feature = "test-support")]
#[doc(hidden)]
pub fn serve_test_connection(connection: LocalConnection, facade: DaemonApplicationFacade) {
    serve_connection(connection, facade);
}

fn serve_listener(listener: LocalListener, facade: DaemonApplicationFacade) -> DtoResult<()> {
    loop {
        serve_next_connection(&listener, &facade)?;
    }
}

fn serve_next_connection(
    listener: &LocalListener,
    facade: &DaemonApplicationFacade,
) -> DtoResult<()> {
    listener.accept().map_or_else(
        |_| {
            Err(ErrorDto::unavailable(
                "local_daemon_listener_unavailable",
                "the local daemon listener is unavailable",
            ))
        },
        |connection| {
            let connection_facade = facade.clone();
            let _ = thread::Builder::new()
                .name("intention-daemon-connection".to_owned())
                .spawn(move || serve_connection(connection, connection_facade));
            Ok(())
        },
    )
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
        reason = "Daemon host unit tests use controlled native fixtures for direct protocol diagnostics."
    )]

    use super::*;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use intention_config::{
        ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
    };
    use intention_protocol::{
        ProtocolHelloDto, ProtocolQueryDto, ProtocolQueryResultDto, ProtocolRequestEnvelopeDto,
        ProtocolRequestPayloadDto,
    };
    use intention_transport::negotiate_client;
    use intention_types::{ConfigRevisionId, CorrelationIdDto, TimestampDto};
    use tempfile::TempDir;

    fn endpoint() -> LocalEndpoint {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after Unix epoch")
            .as_nanos();
        LocalEndpoint::from_instance_id(format!("daemon-library-{nanos}"))
            .expect("fixture endpoint is valid")
    }

    fn fixture_facade() -> (TempDir, DaemonApplicationFacade) {
        let source = ConfigSourceDto::Explicit(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("intention-daemon-unit.toml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("fixture configuration path is absolute"),
        );
        let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential\"",
            source,
        ))
        .expect("fixture configuration resolves");
        let snapshot = ConfigSnapshotDto::new(
            SchemaVersionDto::new(1, 0),
            ConfigRevisionId::new(),
            TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid"),
            resolved,
        )
        .expect("fixture snapshot is credential-free");
        let directory = TempDir::new().expect("temporary fixture directory exists");
        let facade = DaemonApplicationFacade::open_for_test_support(
            directory.path().join("daemon.sqlite"),
            snapshot,
        )
        .expect("fixture facade opens");
        (directory, facade)
    }

    #[test]
    fn run_rejects_an_endpoint_already_owned_by_another_host() {
        let endpoint = endpoint();
        let _listener = LocalListener::bind(endpoint.clone()).expect("fixture listener binds");
        assert_eq!(
            run(endpoint)
                .expect_err("daemon must not reclaim an owned endpoint")
                .code(),
            "local_daemon_endpoint_in_use"
        );
    }

    #[test]
    fn listener_accepts_and_dispatches_one_typed_health_query() {
        let endpoint = endpoint();
        let listener = LocalListener::bind(endpoint.clone()).expect("fixture listener binds");
        let mut client = LocalConnection::connect(&endpoint).expect("fixture client connects");
        let server = std::thread::spawn(move || {
            let (directory, facade) = fixture_facade();
            let _directory = directory;
            serve_next_connection(&listener, &facade)
        });
        server
            .join()
            .expect("single accept thread completes")
            .expect("single accept succeeds");
        negotiate_client(
            &mut client,
            ProtocolHelloDto::new(
                local_protocol_version(),
                vec![
                    ProtocolCapabilityDto::SessionSubscriptions,
                    ProtocolCapabilityDto::CorrelatedRequests,
                    ProtocolCapabilityDto::DaemonHealth,
                ],
                "daemon-library-test",
            )
            .expect("fixture hello is valid"),
        )
        .expect("fixture hello negotiates");
        client
            .send_request(&ProtocolRequestEnvelopeDto::new(
                local_protocol_version(),
                CorrelationIdDto::new(),
                ProtocolMessageDto::new(
                    SchemaVersionDto::new(1, 0),
                    ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
                ),
            ))
            .expect("health request sends");
        assert!(matches!(
            client
                .receive_response()
                .expect("health response arrives")
                .message()
                .payload(),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(health))
                if health.readiness() == intention_protocol::DaemonReadinessDto::Ready
        ));
        std::thread::sleep(Duration::from_millis(1));
    }
}
