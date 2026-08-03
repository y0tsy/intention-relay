#![allow(
    clippy::expect_used,
    reason = "TUI proof tests use controlled fixture-daemon diagnostics."
)]

use intention_client::{DaemonLauncher, IntentionClient};
use intention_protocol::{
    DaemonReadinessDto, SessionSubscriptionResponseDto, SubscribeSessionCommandDto,
};
use intention_test_support::FixtureHost;
use intention_transport::LocalEndpoint;
use intention_tui::TuiProofClient;
use intention_types::{DtoResult, ErrorDto, SchemaVersionDto, SessionEventSequenceDto, SessionId};

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

#[test]
fn tui_proof_reaches_the_shared_fixture_daemon_only_through_the_client() {
    let endpoint = LocalEndpoint::from_instance_id(format!("tui-proof-{}", std::process::id()))
        .expect("fixture instance name is valid");
    let session_id = SessionId::new();
    let daemon_endpoint = endpoint.clone();
    let fixture = FixtureHost::open(session_id).expect("fixture host opens");
    let daemon = fixture.spawn(daemon_endpoint, 2);
    let client = IntentionClient::new(endpoint, "fixture-tui", Box::new(ExistingDaemonLauncher))
        .expect("fixture client is valid");
    let tui = TuiProofClient::new(client);
    let health = tui.connect().expect("TUI reaches ready daemon health");
    assert_eq!(health.readiness(), DaemonReadinessDto::Ready);
    let session = tui
        .subscribe(SubscribeSessionCommandDto::new(
            SchemaVersionDto::new(1, 0),
            session_id,
            Some(SessionEventSequenceDto::new(0)),
            intention_domain::RunModeDto::Build,
        ))
        .expect("TUI receives the daemon fixture subscription");
    assert!(matches!(
        session,
        SessionSubscriptionResponseDto::SnapshotAndTail { snapshot, tail }
            if snapshot.session_id() == session_id && tail.session_id() == session_id
    ));
    daemon
        .join()
        .expect("fixture daemon thread completes")
        .expect("fixture daemon serves the TUI requests");
}

#[test]
fn tui_proof_preserves_the_shared_client_error_contract() {
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
}
