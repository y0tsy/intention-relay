//! Starts the M2 local daemon host from an explicit private endpoint.

#![allow(
    clippy::exit,
    clippy::print_stderr,
    reason = "The thin daemon binary must report a safe startup failure and return a process status."
)]

use std::env;

use intention_transport::LocalEndpoint;

fn main() {
    let endpoint = env::args()
        .nth(1)
        .map(LocalEndpoint::from_instance_id)
        .transpose()
        .and_then(|configured| configured.map_or_else(LocalEndpoint::platform_default, Ok));
    match endpoint.and_then(intention_daemon::run) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("{}", error.code());
            std::process::exit(1);
        }
    }
}
