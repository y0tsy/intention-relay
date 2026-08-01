//! Starts the M2 local daemon host from an explicit private endpoint.

#![allow(
    clippy::exit,
    clippy::print_stderr,
    reason = "The thin daemon binary must report a safe startup failure and return a process status."
)]

use std::env;
use std::path::PathBuf;

use intention_transport::LocalEndpoint;

fn main() {
    let endpoint = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .map(LocalEndpoint::from_path)
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
