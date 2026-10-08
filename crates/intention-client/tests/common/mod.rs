#![allow(
    clippy::expect_used,
    reason = "Shared client fixtures use expect for precise test diagnostics."
)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use intention_proto::{
    MessageKindDto, MessageProjectionDto, ProtocolHelloDto, RunId, SchemaVersionDto, SessionId,
};
use intention_transport::{LocalEndpoint, local_protocol_version};

/// The current DTO schema version every fixture DTO carries.
pub const SCHEMA_VERSION: SchemaVersionDto = intention_proto::CURRENT_DTO_SCHEMA_VERSION;

/// Bound that turns a hanging client call into a visible test failure.
pub const TEST_REPLY_BOUND: Duration = Duration::from_secs(5);

static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(0);

/// Returns one unique logical endpoint for a fixture listener.
pub fn endpoint() -> LocalEndpoint {
    let sequence = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time follows epoch")
        .as_nanos();
    LocalEndpoint::from_instance_id(format!(
        "client-fixture-{}-{nanos}-{sequence}",
        std::process::id()
    ))
    .expect("fixture instance name is valid")
}

/// Returns one compatible fixture hello naming `name`.
pub fn hello(name: &str) -> ProtocolHelloDto {
    ProtocolHelloDto::new(local_protocol_version(), name).expect("fixture hello is valid")
}

/// Returns one transcript row with no reasoning and no tool identity.
pub fn message(
    session_id: SessionId,
    run_id: Option<RunId>,
    kind: MessageKindDto,
    text: &str,
) -> MessageProjectionDto {
    MessageProjectionDto::new(session_id, run_id, kind, text, None, None, None)
        .expect("fixture transcript row is valid")
}
