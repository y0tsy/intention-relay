#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Daemon bootstrap integration uses controlled process lifecycle diagnostics."
)]

use std::env;
use std::process::{Child, Command};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use intention_client::{DaemonLauncher, IntentionClient};
use intention_protocol::{
    DaemonReadinessDto, SessionSubscriptionResponseDto, SubscribeSessionCommandDto,
};
use intention_transport::LocalEndpoint;
use intention_types::{DtoResult, ErrorDto, SchemaVersionDto, SessionEventSequenceDto};

#[derive(Clone)]
struct BinaryLauncher {
    child: Arc<Mutex<Option<Child>>>,
    fixture_session_id: intention_types::SessionId,
    program: String,
}

impl DaemonLauncher for BinaryLauncher {
    fn launch(&self, endpoint: &LocalEndpoint) -> DtoResult<()> {
        let child = Command::new(&self.program)
            .arg(endpoint.instance_id())
            .arg(self.fixture_session_id.to_string())
            .spawn()
            .map_err(|_| {
                ErrorDto::unavailable(
                    "fixture_daemon_launch_failed",
                    "fixture daemon launch failed",
                )
            })?;
        let mut owned_child = self.child.lock().map_err(|_| {
            ErrorDto::unavailable(
                "fixture_daemon_launch_failed",
                "fixture daemon launch failed",
            )
        })?;
        *owned_child = Some(child);
        drop(owned_child);
        Ok(())
    }
}

fn endpoint() -> LocalEndpoint {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time must be after Unix epoch")
        .as_nanos();
    LocalEndpoint::from_instance_id(format!("daemon-bootstrap-{nanos}"))
        .expect("fixture instance identifier is valid")
}

fn daemon_program() -> String {
    env::var("CARGO_BIN_EXE_intention-daemon")
        .expect("Cargo supplies the daemon binary for integration tests")
}

#[test]
fn daemon_binary_rejects_an_unsafe_endpoint_identifier() {
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
}

#[test]
fn concurrent_clients_bootstrap_one_daemon_and_observe_shared_fixture_state() {
    let endpoint = endpoint();
    let fixture_session_id = intention_types::SessionId::new();
    let child = Arc::new(Mutex::new(None));
    let launcher = BinaryLauncher {
        child: Arc::clone(&child),
        fixture_session_id,
        program: daemon_program(),
    };
    let barrier = Arc::new(Barrier::new(2));
    let handles = [0, 1].map(|index| {
        let endpoint = endpoint.clone();
        let launcher = launcher.clone();
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            let client = IntentionClient::new(
                endpoint,
                format!("fixture-client-{index}"),
                Box::new(launcher),
            )
            .expect("fixture client is valid");
            barrier.wait();
            let health = client
                .connect_or_bootstrap()
                .expect("concurrent bootstrap returns ready health");
            (client, health)
        })
    });
    let [(first_client, first_health), (second_client, second_health)] =
        handles.map(|handle| handle.join().expect("client thread completes"));
    assert_eq!(first_health.readiness(), DaemonReadinessDto::Ready);
    assert_eq!(second_health.readiness(), DaemonReadinessDto::Ready);
    assert_eq!(
        first_client.health().expect("first client rechecks health"),
        second_client
            .health()
            .expect("second client rechecks health")
    );

    let first_snapshot = first_client
        .session_snapshot(fixture_session_id)
        .expect("first client observes the fixture snapshot");
    let second_snapshot = second_client
        .session_snapshot(fixture_session_id)
        .expect("second client observes the fixture snapshot");
    assert_eq!(first_snapshot, second_snapshot);
    assert_eq!(first_snapshot.session_id(), fixture_session_id);

    let subscription = SubscribeSessionCommandDto::new(
        SchemaVersionDto::new(1, 0),
        fixture_session_id,
        Some(SessionEventSequenceDto::new(0)),
        intention_domain::RunModeDto::Build,
    );
    let first_session = first_client
        .subscribe(subscription)
        .expect("first client receives the fixture subscription");
    let second_session = second_client
        .subscribe(subscription)
        .expect("second client receives the fixture subscription");
    assert_eq!(first_session, second_session);
    assert!(matches!(
        first_session,
        SessionSubscriptionResponseDto::SnapshotAndTail { snapshot, tail }
            if snapshot.session_id() == fixture_session_id && tail.session_id() == fixture_session_id
    ));

    let unknown_session = second_client
        .subscribe(SubscribeSessionCommandDto::new(
            SchemaVersionDto::new(1, 0),
            intention_types::SessionId::new(),
            Some(SessionEventSequenceDto::new(0)),
            intention_domain::RunModeDto::Build,
        ))
        .expect("unknown session returns a typed subscription result");
    assert!(matches!(
        unknown_session,
        SessionSubscriptionResponseDto::ResyncRequired(_)
    ));
    let spawned_child = child
        .lock()
        .expect("fixture child mutex remains available")
        .take();
    if let Some(mut child) = spawned_child {
        child.kill().expect("fixture daemon can be terminated");
        child.wait().expect("fixture daemon can be reaped");
    }
}
