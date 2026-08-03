#![allow(
    clippy::expect_used,
    reason = "Daemon binary integration uses explicit process diagnostics."
)]

use std::env;
use std::process::Command;

fn daemon_program() -> String {
    env::var("CARGO_BIN_EXE_intention-daemon")
        .expect("Cargo supplies the daemon binary for integration tests")
}

#[test]
fn daemon_binary_rejects_unsafe_or_fixture_arguments() {
    let output = Command::new(daemon_program())
        .arg("../unsafe-endpoint")
        .output()
        .expect("daemon binary starts");
    assert!(
        !output.status.success(),
        "unsafe endpoint exits unsuccessfully"
    );
    assert_eq!(
        String::from_utf8(output.stderr)
            .expect("daemon error output is UTF-8")
            .trim(),
        "invalid_local_endpoint_instance"
    );
    let extra_argument = Command::new(daemon_program())
        .args(["fixture-endpoint", "11111111-1111-4111-8111-111111111111"])
        .output()
        .expect("daemon binary starts");
    assert!(!extra_argument.status.success());
    assert_eq!(
        String::from_utf8(extra_argument.stderr)
            .expect("daemon error output is UTF-8")
            .trim(),
        "invalid_daemon_arguments"
    );
}
