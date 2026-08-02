//! Starts the M2 local daemon host from an explicit private endpoint.

#![allow(
    clippy::exit,
    clippy::print_stderr,
    reason = "The thin daemon binary must report a safe startup failure and return a process status."
)]

use std::env;

use intention_transport::LocalEndpoint;
use intention_types::SessionId;

fn main() {
    let mut arguments = env::args().skip(1);
    let endpoint = arguments
        .next()
        .map(LocalEndpoint::from_instance_id)
        .transpose()
        .and_then(|configured| configured.map_or_else(LocalEndpoint::platform_default, Ok));
    let fixture_session = arguments
        .next()
        .map(|value| SessionId::parse(&value))
        .transpose();
    let result = match (endpoint, fixture_session) {
        (Ok(endpoint), Ok(Some(session_id))) => intention_daemon::run_fixture(endpoint, session_id),
        (Ok(endpoint), Ok(None)) => intention_daemon::run(endpoint),
        (Err(error), _) | (_, Err(error)) => Err(error),
    };
    match result {
        Ok(()) => {}
        Err(error) => {
            eprintln!("{}", error.code());
            std::process::exit(1);
        }
    }
}
