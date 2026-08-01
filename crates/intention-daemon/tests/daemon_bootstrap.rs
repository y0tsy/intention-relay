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
    program: String,
}

impl DaemonLauncher for BinaryLauncher {
    fn launch(&self, endpoint: &LocalEndpoint) -> DtoResult<()> {
        let child = Command::new(&self.program)
            .arg(endpoint.instance_id())
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
fn concurrent_clients_bootstrap_one_daemon_and_observe_shared_fixture_state() {
    let endpoint = endpoint();
    let child = Arc::new(Mutex::new(None));
    let launcher = BinaryLauncher {
        child: Arc::clone(&child),
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
            client
                .connect_or_bootstrap()
                .expect("concurrent bootstrap returns ready health")
        })
    });
    let first = handles.map(|handle| handle.join().expect("client thread completes"));
    assert!(
        first
            .iter()
            .all(|health| health.readiness() == DaemonReadinessDto::Ready)
    );

    let session_id = intention_types::SessionId::new();
    let client = IntentionClient::new(endpoint, "fixture-observer", Box::new(launcher))
        .expect("observer client is valid");
    let session = client
        .subscribe(SubscribeSessionCommandDto::new(
            SchemaVersionDto::new(1, 0),
            session_id,
            Some(SessionEventSequenceDto::new(0)),
            intention_domain::RunModeDto::Build,
        ))
        .expect("shared daemon responds with typed subscription result");
    assert!(matches!(
        session,
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
