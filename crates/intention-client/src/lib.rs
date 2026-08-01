//! Shared bootstrap, dispatch, subscription, and reconnect client for local adapters.
//!
//! Adapters use this crate instead of direct daemon, runtime, storage, or
//! transport implementation access. It retains only reconnect projection state;
//! daemon authority remains remote.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use intention_protocol::{
    DaemonHealthDto, ProtocolCapabilityDto, ProtocolHelloDto, ProtocolMessageDto, ProtocolQueryDto,
    ProtocolQueryResultDto, ProtocolRequestEnvelopeDto, ProtocolRequestPayloadDto,
    ProtocolResponsePayloadDto, SessionSnapshotDto, SessionSubscriptionResponseDto,
    SubscribeSessionCommandDto,
};
use intention_transport::{
    LocalConnection, LocalEndpoint, local_protocol_version, negotiate_client,
};
use intention_types::{
    CorrelationIdDto, DtoResult, ErrorCategoryDto, ErrorDto, EventEnvelopeDto, EventId,
    SchemaVersionDto, SessionEventSequenceDto, SessionId,
};

const SCHEMA_VERSION: SchemaVersionDto = SchemaVersionDto::new(1, 0);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const STARTUP_RETRY: Duration = Duration::from_millis(25);

/// Launches a daemon process after bootstrap has acquired the startup lock.
pub trait DaemonLauncher: Send + Sync {
    /// Starts one daemon host for `endpoint`.
    ///
    /// # Errors
    ///
    /// Returns only a safe typed launch error. Readiness is verified separately
    /// by `IntentionClient` through protocol negotiation and health query.
    fn launch(&self, endpoint: &LocalEndpoint) -> DtoResult<()>;
}

/// A process launcher for the thin `intention-daemon` binary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessDaemonLauncher {
    program: String,
}

impl ProcessDaemonLauncher {
    /// Configures a non-empty daemon program path or command name.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the configured program is blank.
    pub fn new(program: impl Into<String>) -> DtoResult<Self> {
        let program = program.into();
        if program.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_daemon_program",
                "daemon program must not be empty",
            ));
        }
        Ok(Self { program })
    }
}

impl DaemonLauncher for ProcessDaemonLauncher {
    fn launch(&self, endpoint: &LocalEndpoint) -> DtoResult<()> {
        Command::new(&self.program)
            .arg(endpoint.instance_id())
            .spawn()
            .map(|_| ())
            .map_err(|_| {
                ErrorDto::new(
                    "local_daemon_launch_failed",
                    ErrorCategoryDto::Unavailable,
                    "the local daemon could not be started",
                    intention_types::ErrorRetryDto::Manual,
                    None,
                )
                .unwrap_or_else(|_| unavailable("local_daemon_launch_failed"))
            })
    }
}

/// The connected shared-client facade exposed to presentation adapters.
pub struct IntentionClient {
    endpoint: LocalEndpoint,
    hello: ProtocolHelloDto,
    launcher: Box<dyn DaemonLauncher>,
}

impl IntentionClient {
    /// Creates a typed client with the adapter metadata used in protocol hello.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the adapter metadata is invalid.
    pub fn new(
        endpoint: LocalEndpoint,
        adapter_name: impl Into<String>,
        launcher: Box<dyn DaemonLauncher>,
    ) -> DtoResult<Self> {
        let hello = ProtocolHelloDto::new(
            local_protocol_version(),
            vec![
                ProtocolCapabilityDto::SessionSubscriptions,
                ProtocolCapabilityDto::CorrelatedRequests,
                ProtocolCapabilityDto::DaemonHealth,
            ],
            adapter_name,
        )?;
        Ok(Self {
            endpoint,
            hello,
            launcher,
        })
    }

    /// Connects to a ready daemon or serializes exactly one local process launch.
    ///
    /// The client attempts IPC before spawning, retries after acquiring the
    /// process-wide advisory lock, and treats a successful hello plus health
    /// response as readiness. The client does not stop a shared daemon on drop.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error if bootstrap, launch, negotiation, or readiness
    /// cannot complete before the bounded deadline.
    pub fn connect_or_bootstrap(&self) -> DtoResult<DaemonHealthDto> {
        match self.connect_ready() {
            Ok(health) => return Ok(health),
            Err(error) if !is_daemon_unavailable(&error) => return Err(error),
            Err(_) => {}
        }

        let _lock = StartupLock::acquire(&self.endpoint)?;
        match self.connect_ready() {
            Ok(health) => return Ok(health),
            Err(error) if !is_daemon_unavailable(&error) => return Err(error),
            Err(_) => {}
        }
        self.launcher.launch(&self.endpoint)?;
        self.wait_for_ready()
    }

    /// Queries the daemon-owned health projection after a fresh negotiated connection.
    ///
    /// # Errors
    ///
    /// Returns a safe typed transport or protocol error if the daemon cannot
    /// produce a correlated health response.
    pub fn health(&self) -> DtoResult<DaemonHealthDto> {
        self.connect_ready()
    }

    /// Queries the current M2 session snapshot fixture.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable, rejected, or invalid-response error.
    pub fn session_snapshot(&self, session_id: SessionId) -> DtoResult<SessionSnapshotDto> {
        let response = self.request(ProtocolRequestPayloadDto::Query(
            ProtocolQueryDto::GetSessionSnapshot(
                intention_domain::GetSessionSnapshotQueryDto::new(session_id),
            ),
        ))?;
        match response {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::SessionSnapshot(
                snapshot,
            )) => Ok(snapshot),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(error)) => {
                Err(error)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Obtains a consistent snapshot-and-tail or typed resync response.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable or invalid-response error. A server-directed
    /// resync is returned as data so adapters can discard local projection state.
    pub fn subscribe(
        &self,
        subscription: SubscribeSessionCommandDto,
    ) -> DtoResult<SessionSubscriptionResponseDto> {
        let response = self.request(ProtocolRequestPayloadDto::Command(
            intention_protocol::ProtocolCommandDto::SubscribeSession(subscription),
        ))?;
        match response {
            ProtocolResponsePayloadDto::Subscription(response) => Ok(response),
            _ => Err(invalid_response()),
        }
    }

    fn connect_ready(&self) -> DtoResult<DaemonHealthDto> {
        let mut connection = self.connect()?;
        let health = self.request_on(
            &mut connection,
            ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
        )?;
        match health {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(
                health,
            )) => Ok(health),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(error)) => {
                Err(error)
            }
            _ => Err(invalid_response()),
        }
    }

    fn request(&self, payload: ProtocolRequestPayloadDto) -> DtoResult<ProtocolResponsePayloadDto> {
        let mut connection = self.connect()?;
        self.request_on(&mut connection, payload)
    }

    fn connect(&self) -> DtoResult<LocalConnection> {
        let mut connection = LocalConnection::connect(&self.endpoint)?;
        negotiate_client(&mut connection, self.hello.clone())?;
        Ok(connection)
    }

    fn request_on(
        &self,
        connection: &mut LocalConnection,
        payload: ProtocolRequestPayloadDto,
    ) -> DtoResult<ProtocolResponsePayloadDto> {
        let correlation_id = CorrelationIdDto::new();
        let request = ProtocolRequestEnvelopeDto::new(
            local_protocol_version(),
            correlation_id,
            ProtocolMessageDto::new(SCHEMA_VERSION, payload),
        );
        connection.send_request(&request)?;
        let response = connection.receive_response()?;
        if response.correlation_id() != correlation_id {
            return Err(invalid_response());
        }
        Ok(response.message().payload().clone())
    }

    fn wait_for_ready(&self) -> DtoResult<DaemonHealthDto> {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            match self.connect_ready() {
                Ok(health) => return Ok(health),
                Err(error) if is_daemon_unavailable(&error) && Instant::now() < deadline => {
                    thread::sleep(STARTUP_RETRY);
                }
                Err(error) => return Err(error),
            }
        }
    }
}

/// A sequence-aware local reducer for snapshot-plus-tail subscription recovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionSubscriptionReducer {
    session_id: SessionId,
    snapshot: Option<SessionSnapshotDto>,
    last_sequence: Option<SessionEventSequenceDto>,
    seen_events: BTreeSet<EventId>,
}

impl SessionSubscriptionReducer {
    /// Creates an empty local reducer for one daemon-owned session.
    #[must_use]
    pub const fn new(session_id: SessionId) -> Self {
        Self {
            session_id,
            snapshot: None,
            last_sequence: None,
            seen_events: BTreeSet::new(),
        }
    }

    /// Applies a complete subscription recovery response.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the response belongs to another session or
    /// contains a non-contiguous tail. A resync instruction clears local state and
    /// is returned as `Ok(true)`.
    pub fn apply(&mut self, response: SessionSubscriptionResponseDto) -> DtoResult<bool> {
        match response {
            SessionSubscriptionResponseDto::ResyncRequired(resync) => {
                if resync.session_id() != self.session_id {
                    return Err(ErrorDto::validation(
                        "invalid_subscription_session",
                        "subscription response belongs to another session",
                    ));
                }
                self.snapshot = None;
                self.last_sequence = None;
                self.seen_events.clear();
                Ok(true)
            }
            SessionSubscriptionResponseDto::SnapshotAndTail { snapshot, tail } => {
                if snapshot.session_id() != self.session_id || tail.session_id() != self.session_id
                {
                    return Err(ErrorDto::validation(
                        "invalid_subscription_session",
                        "subscription response belongs to another session",
                    ));
                }
                self.snapshot = Some(snapshot);
                self.last_sequence = Some(snapshot.at_sequence());
                self.seen_events.clear();
                for event in tail.events() {
                    self.apply_event(event)?;
                }
                Ok(false)
            }
        }
    }

    /// Applies one ordered live event, ignoring a duplicate or stale sequence.
    ///
    /// # Errors
    ///
    /// Returns a validation error for another session or a non-contiguous future
    /// sequence, which tells the adapter to request snapshot recovery.
    pub fn apply_event(
        &mut self,
        event: &EventEnvelopeDto<intention_domain::DomainEventDto>,
    ) -> DtoResult<()> {
        if event.session_id() != self.session_id {
            return Err(ErrorDto::validation(
                "invalid_subscription_session",
                "subscription event belongs to another session",
            ));
        }
        if self.seen_events.contains(&event.event_id()) {
            return Ok(());
        }
        let expected = self.last_sequence.map_or(0, SessionEventSequenceDto::value);
        if event.sequence().value() <= expected {
            return Ok(());
        }
        if event.sequence().value() != expected.saturating_add(1) {
            return Err(ErrorDto::validation(
                "subscription_sequence_gap",
                "subscription event sequence requires snapshot recovery",
            ));
        }
        self.seen_events.insert(event.event_id());
        self.last_sequence = Some(event.sequence());
        Ok(())
    }

    /// Returns the last applied daemon sequence, if a snapshot has been accepted.
    #[must_use]
    pub const fn last_sequence(&self) -> Option<SessionEventSequenceDto> {
        self.last_sequence
    }

    /// Returns the current accepted snapshot checkpoint.
    #[must_use]
    pub const fn snapshot(&self) -> Option<SessionSnapshotDto> {
        self.snapshot
    }
}

struct StartupLock {
    file: std::fs::File,
}

impl StartupLock {
    fn acquire(endpoint: &LocalEndpoint) -> DtoResult<Self> {
        let path = startup_lock_path(endpoint)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| unavailable("startup_lock_unavailable"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                    .map_err(|_| unavailable("startup_lock_unavailable"))?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|_| unavailable("startup_lock_unavailable"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|_| unavailable("startup_lock_unavailable"))?;
        }
        fs4::FileExt::lock(&file).map_err(|_| unavailable("startup_lock_unavailable"))?;
        Ok(Self { file })
    }
}

impl Drop for StartupLock {
    fn drop(&mut self) {
        let _ = fs4::FileExt::unlock(&self.file);
    }
}

fn startup_lock_path(endpoint: &LocalEndpoint) -> DtoResult<PathBuf> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    endpoint.instance_id().hash(&mut hasher);
    let endpoint_hash = hasher.finish();
    let base = platform_state_directory()?;
    Ok(base.join(format!("bootstrap-{endpoint_hash:016x}.lock")))
}

fn platform_state_directory() -> DtoResult<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|candidate| candidate.is_absolute())
            .or_else(|| {
                std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .filter(|candidate| candidate.is_absolute())
                    .map(|candidate| candidate.join("intention-relay"))
            })
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|candidate| candidate.is_absolute())
                    .map(|candidate| candidate.join(".config/intention-relay"))
            })
            .ok_or_else(|| unavailable("startup_lock_unavailable"))
    }
    #[cfg(target_os = "macos")]
    {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|candidate| candidate.is_absolute())
            .map(|candidate| candidate.join("Library/Application Support/intention-relay"))
            .ok_or_else(|| unavailable("startup_lock_unavailable"));
    }
    #[cfg(windows)]
    {
        return std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|candidate| candidate.is_absolute())
            .map(|candidate| candidate.join("intention-relay"))
            .ok_or_else(|| unavailable("startup_lock_unavailable"));
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err(unavailable("startup_lock_unavailable"))
    }
}

fn is_daemon_unavailable(error: &ErrorDto) -> bool {
    matches!(
        error.code(),
        "local_daemon_unavailable" | "local_daemon_connection_unavailable"
    )
}

fn invalid_response() -> ErrorDto {
    ErrorDto::validation(
        "invalid_local_protocol_response",
        "the local daemon returned an unexpected protocol response",
    )
}

fn unavailable(code: &'static str) -> ErrorDto {
    ErrorDto::unavailable(code, "the local daemon connection is unavailable")
}
