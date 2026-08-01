#![allow(
    clippy::expect_used,
    reason = "TUI proof tests use fixture client construction diagnostics."
)]

use intention_client::{DaemonLauncher, IntentionClient};
use intention_protocol::DaemonReadinessDto;
use intention_transport::LocalEndpoint;
use intention_tui::TuiProofClient;
use intention_types::{DtoResult, ErrorDto};

struct UnavailableLauncher;

impl DaemonLauncher for UnavailableLauncher {
    fn launch(&self, _endpoint: &LocalEndpoint) -> DtoResult<()> {
        Err(ErrorDto::unavailable(
            "fixture_daemon_unavailable",
            "fixture daemon is unavailable",
        ))
    }
}

#[test]
fn tui_proof_uses_the_shared_client_error_contract() {
    let endpoint = LocalEndpoint::from_instance_id("tui-proof-unavailable")
        .expect("fixture instance name is valid");
    let client = IntentionClient::new(endpoint, "fixture-tui", Box::new(UnavailableLauncher))
        .expect("fixture client is valid");
    let tui = TuiProofClient::new(client);
    let error = tui
        .connect()
        .expect_err("fixture launcher must produce typed client failure");
    assert_eq!(error.code(), "fixture_daemon_unavailable");
    assert_ne!(
        error.category(),
        intention_types::ErrorCategoryDto::Internal
    );
    assert_ne!(DaemonReadinessDto::Ready, DaemonReadinessDto::Unavailable);
}
