#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Client contract fixtures use direct assertions and controlled fixture launchers."
)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use intention_client::{DaemonLauncher, IntentionClient};
use intention_protocol::{
    DaemonReadinessDto, ProtocolCapabilityDto, ProtocolHelloDto, ProtocolMessageDto,
    ProtocolQueryDto, ProtocolQueryResultDto, ProtocolRequestPayloadDto,
    ProtocolResponseEnvelopeDto, ProtocolResponsePayloadDto, SessionSnapshotDto,
    SessionSubscriptionResponseDto, SubscribeSessionCommandDto,
};
use intention_transport::{LocalEndpoint, LocalListener, local_protocol_version, negotiate_daemon};
use intention_types::{DtoResult, SchemaVersionDto, SessionEventSequenceDto, SessionId};
use tempfile::TempDir;

#[derive(Clone)]
struct FixtureLauncher;

impl DaemonLauncher for FixtureLauncher {
    fn launch(&self, endpoint: &LocalEndpoint) -> DtoResult<()> {
        let endpoint = endpoint.clone();
        let _ = thread::spawn(move || serve_one_fixture_connection(endpoint));
        Ok(())
    }
}

static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(0);

fn endpoint(_directory: &TempDir) -> LocalEndpoint {
    let sequence = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time must be after Unix epoch")
        .as_nanos();
    LocalEndpoint::from_instance_id(format!("client-fixture-{nanos}-{sequence}"))
        .expect("fixture instance name is valid")
}

fn client(endpoint: LocalEndpoint) -> IntentionClient {
    IntentionClient::new(endpoint, "fixture-client", Box::new(FixtureLauncher))
        .expect("fixture client is valid")
}

fn daemon_hello() -> ProtocolHelloDto {
    ProtocolHelloDto::new(
        local_protocol_version(),
        vec![
            ProtocolCapabilityDto::SessionSubscriptions,
            ProtocolCapabilityDto::CorrelatedRequests,
            ProtocolCapabilityDto::DaemonHealth,
        ],
        "fixture-daemon",
    )
    .expect("fixture daemon hello is valid")
}

fn serve_one_fixture_connection(endpoint: LocalEndpoint) {
    let listener = LocalListener::bind(endpoint).expect("fixture listener binds");
    let mut connection = listener.accept().expect("fixture client connects");
    negotiate_daemon(&mut connection, daemon_hello()).expect("fixture hello negotiates");
    let request = connection
        .receive_request()
        .expect("fixture request arrives");
    let payload = match request.message().payload() {
        ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth) => {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(
                intention_protocol::DaemonHealthDto::new(
                    SchemaVersionDto::new(1, 0),
                    local_protocol_version(),
                    DaemonReadinessDto::Ready,
                ),
            ))
        }
        ProtocolRequestPayloadDto::Command(
            intention_protocol::ProtocolCommandDto::SubscribeSession(command),
        ) => {
            let snapshot = SessionSnapshotDto::new(
                SchemaVersionDto::new(1, 0),
                command.session_id(),
                command
                    .after_sequence()
                    .unwrap_or(SessionEventSequenceDto::new(0)),
            );
            let tail = intention_protocol::SessionEventTailBatchDto::new(
                SchemaVersionDto::new(1, 0),
                command.session_id(),
                snapshot.at_sequence(),
                Vec::new(),
            )
            .expect("empty fixture tail is valid");
            ProtocolResponsePayloadDto::Subscription(
                SessionSubscriptionResponseDto::snapshot_and_tail(snapshot, tail)
                    .expect("matching fixture snapshot and tail are valid"),
            )
        }
        _ => ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(
            intention_types::ErrorDto::validation("fixture", "unexpected fixture request"),
        )),
    };
    connection
        .send_response(&ProtocolResponseEnvelopeDto::new(
            local_protocol_version(),
            request.correlation_id(),
            ProtocolMessageDto::new(SchemaVersionDto::new(1, 0), payload),
        ))
        .expect("fixture response sends");
}

#[test]
fn bootstrap_starts_one_fixture_daemon_and_waits_for_ready_health() {
    let directory = TempDir::new().expect("temporary directory is available");
    let health = client(endpoint(&directory))
        .connect_or_bootstrap()
        .expect("bootstrap must yield typed ready health");
    assert_eq!(health.readiness(), DaemonReadinessDto::Ready);
}

#[test]
fn subscription_response_preserves_snapshot_position_after_reconnect() {
    let directory = TempDir::new().expect("temporary directory is available");
    let session_id = SessionId::new();
    let endpoint = endpoint(&directory);
    let client = client(endpoint.clone());
    let first = client
        .connect_or_bootstrap()
        .expect("first bootstrap connects to ready daemon");
    assert_eq!(first.readiness(), DaemonReadinessDto::Ready);

    let server = thread::spawn(move || serve_one_fixture_connection(endpoint));
    thread::sleep(Duration::from_millis(10));
    let response = client
        .subscribe(SubscribeSessionCommandDto::new(
            SchemaVersionDto::new(1, 0),
            session_id,
            Some(SessionEventSequenceDto::new(4)),
            intention_domain::RunModeDto::Build,
        ))
        .expect("reconnected subscription returns a consistent response");
    match response {
        SessionSubscriptionResponseDto::SnapshotAndTail { snapshot, tail } => {
            assert_eq!(snapshot.at_sequence(), SessionEventSequenceDto::new(4));
            assert_eq!(tail.after_sequence(), SessionEventSequenceDto::new(4));
        }
        SessionSubscriptionResponseDto::ResyncRequired(_) => {
            panic!("fixture must provide snapshot and tail")
        }
    }
    server.join().expect("fixture server completes");
}
