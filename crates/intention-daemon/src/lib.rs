//! Thin daemon process host for the M2 local protocol fixture.
//!
//! The daemon owns the local listener and typed connection hosting. It delegates
//! health, query, command, and subscription meaning to the composition facade;
//! the facade remains intentionally non-durable until M3.

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
/// # Errors
///
/// Returns a typed error if the endpoint cannot bind. Per-connection failures are
/// isolated to that connection so a malformed or disconnected client cannot stop
/// the shared daemon host.
pub fn run(endpoint: LocalEndpoint) -> DtoResult<()> {
    let listener = LocalListener::bind(endpoint)?;
    let facade = DaemonApplicationFacade::new_fixture();
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
