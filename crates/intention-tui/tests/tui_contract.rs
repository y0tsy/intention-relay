#![allow(
    clippy::expect_used,
    reason = "TUI proof tests use controlled fixture-daemon diagnostics."
)]

use intention_client::{DaemonLauncher, IntentionClient};
use intention_proto::RunModeDto;
use intention_proto::{
    DaemonReadinessDto, SessionSubscriptionResponseDto, SubscribeSessionCommandDto,
};
use intention_proto::{DtoResult, ErrorDto, SchemaVersionDto, SessionId};
use intention_test_support::FixtureHost;
use intention_transport::LocalEndpoint;
use intention_tui::TuiProofClient;

struct UnavailableLauncher;

impl DaemonLauncher for UnavailableLauncher {
    fn launch(&self, _endpoint: &LocalEndpoint) -> DtoResult<()> {
        Err(ErrorDto::unavailable(
            "fixture_daemon_unavailable",
            "fixture daemon is unavailable",
        ))
    }
}

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
    let client = IntentionClient::new(endpoint, "fixture-tui", Box::new(ExistingDaemonLauncher))
        .expect("fixture client is valid");
    let tui = TuiProofClient::new(client);
    let health = tui
        .connect()
        .await
        .expect("TUI reaches ready daemon health");
    assert_eq!(health.readiness(), DaemonReadinessDto::Ready);
    let session = tui
        .subscribe(SubscribeSessionCommandDto::new(
            SchemaVersionDto::new(1, 1),
            session_id,
            RunModeDto::Build,
        ))
        .await
        .expect("TUI receives the daemon fixture subscription");
    assert!(matches!(
        session,
        SessionSubscriptionResponseDto::Snapshot(snapshot)
            if snapshot.session_id() == session_id
    ));
    daemon
        .await
        .expect("fixture daemon task completes")
        .expect("fixture daemon serves the TUI requests");
}

#[tokio::test]
async fn tui_proof_preserves_the_shared_client_error_contract() {
    let endpoint = LocalEndpoint::from_instance_id("tui-proof-unavailable")
        .expect("fixture instance name is valid");
    let client = IntentionClient::new(endpoint, "fixture-tui", Box::new(UnavailableLauncher))
        .expect("fixture client is valid");
    let tui = TuiProofClient::new(client);
    let error = tui
        .connect()
        .await
        .expect_err("fixture launcher must produce typed client failure");
    assert_eq!(error.code(), "fixture_daemon_unavailable");
    assert_ne!(
        error.category(),
        intention_proto::ErrorCategoryDto::Internal
    );
}
