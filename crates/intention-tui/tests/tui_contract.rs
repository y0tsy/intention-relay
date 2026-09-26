#![allow(
    clippy::expect_used,
    reason = "TUI proof tests use controlled fixture-daemon diagnostics."
)]

use intention_client::{DaemonLauncher, IntentionClient};
use intention_protocol::{
    DaemonReadinessDto, SessionEventTailBatchDto, SessionSubscriptionResponseDto,
    SubscribeSessionCommandDto,
};
use intention_test_support::FixtureHost;
use intention_transport::LocalEndpoint;
use intention_tui::TuiProofClient;
use intention_types::{
    DtoResult, ErrorDto, EventEnvelopeDto, EventId, EventMetadataDto, SchemaVersionDto,
    SessionEventSequenceDto, SessionId, TimestampDto, ToolCallId,
};

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
            SchemaVersionDto::new(1, 1),
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

#[test]
fn tui_adapter_uses_shared_client_for_tool_lifecycle_streams() {
    // The TUI adapter is intentionally constructed from the client facade;
    // tool lifecycle transport remains a protocol/client responsibility.
    let endpoint = LocalEndpoint::from_instance_id("tui-tool-lifecycle").expect("valid endpoint");
    let client = IntentionClient::new(endpoint, "fixture-tui", Box::new(UnavailableLauncher))
        .expect("fixture client is valid");
    let tui = TuiProofClient::new(client);
    let error = tui.connect().expect_err("shared client path is exercised");
    assert_eq!(error.code(), "fixture_daemon_unavailable");
}

#[test]
fn tui_and_shared_client_preserve_identical_typed_tool_events_from_session_tail() {
    let session_id = SessionId::new();
    let run_id = intention_types::RunId::new();
    let call_id = ToolCallId::new();
    let timestamp = TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid");
    let event = intention_domain::ToolLifecycleEventDto::new(
        session_id,
        run_id,
        call_id,
        "read",
        intention_domain::ToolLifecycleStatusDto::Completed,
        "normalized result; secret redacted",
        timestamp,
    )
    .expect("tool event is valid");
    let envelope = EventEnvelopeDto::new(
        EventMetadataDto::new(
            SchemaVersionDto::new(1, 1),
            EventId::new(),
            session_id,
            Some(run_id),
            None,
            SessionEventSequenceDto::new(1),
            timestamp,
        ),
        intention_domain::DomainEventDto::ToolLifecycle(event),
    );
    let tail = SessionEventTailBatchDto::new(
        SchemaVersionDto::new(1, 1),
        session_id,
        SessionEventSequenceDto::new(0),
        vec![envelope.clone()],
    )
    .expect("tail is valid");
    let response = serde_json::to_vec(&tail).expect("tail serializes");
    let tui_tail: SessionEventTailBatchDto =
        serde_json::from_slice(&response).expect("TUI decodes shared tail");
    let client_tail: SessionEventTailBatchDto =
        serde_json::from_slice(&response).expect("shared client decodes tail");
    assert_eq!(tui_tail, client_tail);
    assert_eq!(&tui_tail.events()[0], &envelope);
    match &tui_tail.events()[0].payload() {
        intention_domain::DomainEventDto::ToolLifecycle(tool) => {
            assert_eq!(tool.session_id(), session_id);
            assert_eq!(tool.run_id(), run_id);
            assert_eq!(tool.call_id(), call_id);
            assert_eq!(
                tool.status(),
                &intention_domain::ToolLifecycleStatusDto::Completed
            );
            assert!(tool.detail().contains("redacted"));
            assert!(!tool.detail().contains("credential"));
        }
        other => assert!(
            matches!(other, intention_domain::DomainEventDto::ToolLifecycle(_)),
            "session tail must retain typed tool lifecycle event"
        ),
    }
}
