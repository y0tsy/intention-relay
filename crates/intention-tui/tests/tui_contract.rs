#![allow(
    clippy::expect_used,
    reason = "TUI proof tests use controlled fixture-daemon diagnostics."
)]

use intention_client::{DaemonLauncher, IntentionClient};
use intention_proto::{DaemonReadinessDto, DtoResult, SessionId};
use intention_test_support::FixtureHost;
use intention_transport::LocalEndpoint;
use intention_tui::TuiProofClient;

struct ExistingDaemonLauncher;

impl DaemonLauncher for ExistingDaemonLauncher {
    fn launch(&self, _endpoint: &LocalEndpoint) -> DtoResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn tui_proof_reaches_the_shared_fixture_daemon_only_through_the_client() {
    let endpoint = LocalEndpoint::from_instance_id(format!("tui-proof-{}", std::process::id()))
        .expect("fixture instance name is valid");
    let session_id = SessionId::new();
    let daemon_endpoint = endpoint.clone();
    let fixture = FixtureHost::open(session_id).expect("fixture host opens");
    let daemon = tokio::spawn(fixture.serve(daemon_endpoint, 2));
    let client = IntentionClient::new(endpoint, Box::new(ExistingDaemonLauncher));
    let tui = TuiProofClient::new(client);
    let health = tui
        .connect()
        .await
        .expect("TUI reaches ready daemon health");
    assert_eq!(health.readiness(), DaemonReadinessDto::Ready);
    let snapshot = tui
        .session_snapshot(session_id)
        .await
        .expect("TUI receives the daemon fixture session snapshot");
    assert_eq!(
        snapshot.session_id(),
        session_id,
        "the TUI reads current fixture session state"
    );
    daemon
        .await
        .expect("fixture daemon task completes")
        .expect("fixture daemon serves the TUI requests");
}
