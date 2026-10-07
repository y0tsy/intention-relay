//! Shared bootstrap, dispatch, subscription, and reconnect client for local adapters.
//!
//! Adapters use this crate instead of direct daemon, runtime, storage, or
//! transport implementation access. It retains only the current run projection
//! and transcript rows the daemon reports; daemon authority remains remote.

use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use intention_proto::{
    DaemonHealthDto, DaemonReadinessDto, ProtocolCommandDto, ProtocolHelloDto, ProtocolMethodDto,
    ProtocolQueryDto, ProtocolQueryResultDto, ProtocolRequestPayloadDto,
    ProtocolResponsePayloadDto, RunStatusFrameDto, RunStreamFrameDto, RunSubscriptionResponseDto,
    SessionSnapshotDto, SessionSubscriptionResponseDto, SubscribeRunCommandDto,
    SubscribeSessionCommandDto, decode_response, encode_request, parse_run_frame_notification,
};
use intention_proto::{DtoResult, ErrorCategoryDto, ErrorDto, RunId, SessionId};
use intention_proto::{
    GetSessionSnapshotQueryDto, MessageProjectionDto, RunProjectionDto, RunStatusDto,
};
use intention_transport::{
    AsyncLocalClientConnection, AsyncMessageReceiver, AsyncRequestSender, LocalEndpoint,
    local_protocol_version,
};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const STARTUP_RETRY: Duration = Duration::from_millis(25);
/// Bounded wait for one correlated command or query reply.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Bounded wait for the correlated first reply to a run-stream subscription.
const STREAM_REPLY_TIMEOUT: Duration = Duration::from_secs(30);

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
                    intention_proto::ErrorRetryDto::Manual,
                    None,
                )
                .unwrap_or_else(|_| unavailable("local_daemon_launch_failed"))
            })
    }
}

/// An established connection with its typed protocol directions.
struct NegotiatedConnection {
    requests: AsyncRequestSender,
    messages: AsyncMessageReceiver,
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
        let hello = ProtocolHelloDto::new(local_protocol_version(), adapter_name)?;
        Ok(Self {
            endpoint,
            hello,
            launcher,
        })
    }

    /// Connects to a ready daemon or serializes exactly one local process launch.
    ///
    /// The client attempts IPC before spawning, retries an already-starting
    /// daemon without spawning, then uses a process-wide advisory lock only when
    /// the endpoint is unavailable. Readiness requires compatible hello, a
    /// correlated health query, and `Ready` state.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error if bootstrap, launch, negotiation, or readiness
    /// cannot complete before the bounded deadline.
    pub async fn connect_or_bootstrap(&self) -> DtoResult<DaemonHealthDto> {
        match self.connect_ready().await {
            Ok(health) => return Ok(health),
            Err(error) if is_daemon_starting(&error) => return self.wait_for_ready().await,
            Err(error) if !is_daemon_unavailable(&error) => return Err(error),
            Err(_) => {}
        }

        let _lock = StartupLock::acquire(&self.endpoint)?;
        match self.connect_ready().await {
            Ok(health) => return Ok(health),
            Err(error) if is_daemon_starting(&error) => return self.wait_for_ready().await,
            Err(error) if !is_daemon_unavailable(&error) => return Err(error),
            Err(_) => {}
        }
        self.launcher.launch(&self.endpoint)?;
        self.wait_for_ready().await
    }

    /// Queries the daemon-owned health projection after a fresh negotiated connection.
    ///
    /// # Errors
    ///
    /// Returns a safe typed transport, protocol, or non-ready daemon error.
    pub async fn health(&self) -> DtoResult<DaemonHealthDto> {
        self.connect_ready().await
    }

    /// Queries the current session snapshot.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable, rejected, or invalid-response error.
    pub async fn session_snapshot(&self, session_id: SessionId) -> DtoResult<SessionSnapshotDto> {
        let response = self
            .request(ProtocolRequestPayloadDto::Query(
                ProtocolQueryDto::GetSessionSnapshot(GetSessionSnapshotQueryDto::new(session_id)),
            ))
            .await?;
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

    /// Obtains the current session snapshot or a typed scoped response.
    ///
    /// A session subscription is a one-shot current-state request: there is no
    /// cursor and no resume state. Re-subscribing re-reads current state.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable or invalid-response error. A scoped rejection
    /// is returned as data so adapters can render the daemon's decision.
    pub async fn subscribe(
        &self,
        subscription: SubscribeSessionCommandDto,
    ) -> DtoResult<SessionSubscriptionResponseDto> {
        let response = self
            .request(ProtocolRequestPayloadDto::Command(
                ProtocolCommandDto::SubscribeSession(subscription),
            ))
            .await?;
        match response {
            ProtocolResponsePayloadDto::Subscription(response) => Ok(response),
            _ => Err(invalid_response()),
        }
    }

    async fn connect_ready(&self) -> DtoResult<DaemonHealthDto> {
        let mut connection = self.connect().await?;
        let health = self
            .request_on(
                &mut connection,
                ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
            )
            .await?;
        match health {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(
                health,
            )) => match health.readiness() {
                DaemonReadinessDto::Ready => Ok(health),
                DaemonReadinessDto::Starting => Err(ErrorDto::new(
                    "local_daemon_starting",
                    ErrorCategoryDto::Unavailable,
                    "the local daemon is starting",
                    intention_proto::ErrorRetryDto::Delayed,
                    None,
                )
                .unwrap_or_else(|_| unavailable("local_daemon_starting"))),
                DaemonReadinessDto::Draining | DaemonReadinessDto::Unavailable => {
                    Err(ErrorDto::new(
                        "local_daemon_not_ready",
                        ErrorCategoryDto::Unavailable,
                        "the local daemon is not ready to serve requests",
                        intention_proto::ErrorRetryDto::Delayed,
                        None,
                    )
                    .unwrap_or_else(|_| unavailable("local_daemon_not_ready")))
                }
            },
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(error)) => {
                Err(error)
            }
            _ => Err(invalid_response()),
        }
    }

    async fn request(
        &self,
        payload: ProtocolRequestPayloadDto,
    ) -> DtoResult<ProtocolResponsePayloadDto> {
        let mut connection = self.connect().await?;
        self.request_on(&mut connection, payload).await
    }

    async fn connect(&self) -> DtoResult<NegotiatedConnection> {
        let connection = AsyncLocalClientConnection::connect(&self.endpoint).await?;
        let (_, requests, messages) = connection.negotiate(self.hello.clone()).await?;
        Ok(NegotiatedConnection { requests, messages })
    }

    async fn request_on(
        &self,
        connection: &mut NegotiatedConnection,
        payload: ProtocolRequestPayloadDto,
    ) -> DtoResult<ProtocolResponsePayloadDto> {
        let method = ProtocolMethodDto::for_payload(&payload);
        let request = encode_request(1, payload);
        tokio::time::timeout(REQUEST_TIMEOUT, async {
            connection.requests.send_message(&request).await?;
            let line = connection.messages.receive_line().await?;
            decode_response(&line, method, 1)
        })
        .await
        .map_err(|_| request_timeout())?
    }

    async fn wait_for_ready(&self) -> DtoResult<DaemonHealthDto> {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            match self.connect_ready().await {
                Ok(health) => return Ok(health),
                Err(error)
                    if (is_daemon_unavailable(&error) || is_daemon_starting(&error))
                        && Instant::now() < deadline =>
                {
                    tokio::time::sleep(STARTUP_RETRY).await;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

/// An asynchronous facade for dedicated run-stream subscriptions.
///
/// It shares the one local connection role: requests flow to the daemon, and
/// responses or `run.frame` notifications flow back.
pub struct RunStreamClient {
    endpoint: LocalEndpoint,
    hello: ProtocolHelloDto,
}

impl RunStreamClient {
    /// Creates an async run-stream client with safe adapter metadata.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the adapter metadata is blank.
    pub fn new(endpoint: LocalEndpoint, adapter_name: impl Into<String>) -> DtoResult<Self> {
        Ok(Self {
            endpoint,
            hello: ProtocolHelloDto::new(local_protocol_version(), adapter_name)?,
        })
    }

    /// Connects, subscribes, and applies the authoritative first reply.
    ///
    /// The request carries only the session and run identity; its correlated
    /// reply is the current run state, and later committed state arrives as
    /// live `run.frame` notifications on the same connection.
    ///
    /// # Errors
    ///
    /// Returns a typed protocol, transport, or scoped-response error, or an
    /// unavailable timeout error when the correlated reply does not arrive
    /// before the bounded deadline.
    pub async fn subscribe(
        &self,
        subscription: SubscribeRunCommandDto,
    ) -> DtoResult<RunStreamSubscription> {
        let session_id = subscription.session_id();
        let run_id = subscription.run_id();
        let connection = AsyncLocalClientConnection::connect(&self.endpoint).await?;
        let (_, mut requests, mut messages) = connection.negotiate(self.hello.clone()).await?;
        let request = encode_request(1, ProtocolRequestPayloadDto::RunSubscription(subscription));
        requests.send_message(&request).await?;
        let line = tokio::time::timeout(STREAM_REPLY_TIMEOUT, messages.receive_line())
            .await
            .map_err(|_| stream_reply_timeout())??;
        let response = decode_response(&line, ProtocolMethodDto::RunSubscribe, 1)?;
        let initial = match response {
            ProtocolResponsePayloadDto::RunSubscription(response) => response,
            _ => return Err(invalid_response()),
        };
        let mut reducer = RunSubscriptionReducer::new(session_id, run_id);
        reducer.apply_initial(initial)?;
        Ok(RunStreamSubscription {
            requests,
            messages,
            reducer,
        })
    }
}

/// An established run-stream subscription with opaque transport resources.
pub struct RunStreamSubscription {
    /// The request direction is retained so the connection keeps both halves
    /// open while committed frames arrive on the response direction.
    #[expect(
        dead_code,
        reason = "the request sender is held only to keep the daemon connection open for live frames"
    )]
    requests: AsyncRequestSender,
    messages: AsyncMessageReceiver,
    reducer: RunSubscriptionReducer,
}

impl RunStreamSubscription {
    /// Receives, applies, and returns the next committed run-stream frame.
    ///
    /// Returns `Ok(None)` once the daemon closes the stream; the caller then
    /// re-reads current state by re-subscribing. Content and status frames are
    /// applied to the reducer before they are returned.
    ///
    /// # Errors
    ///
    /// Returns a typed framing, protocol, or scope-validation error without
    /// mutating reducer state. Live frames may be arbitrarily sparse, so this
    /// wait is intentionally unbounded; only the correlated reply waits carry a
    /// deadline.
    pub async fn receive(&mut self) -> DtoResult<Option<RunStreamFrameDto>> {
        let line = match self.messages.receive_line().await {
            Ok(line) => line,
            Err(error) if error.code() == "local_daemon_connection_unavailable" => return Ok(None),
            Err(error) => return Err(error),
        };
        let frame = parse_run_frame_notification(&line)?;
        self.reducer.apply_frame(frame.clone())?;
        Ok(Some(frame))
    }

    /// Returns the state reducer for this fixed run scope.
    #[must_use]
    pub const fn reducer(&self) -> &RunSubscriptionReducer {
        &self.reducer
    }
}

/// A run-scoped reducer holding the current run projection and transcript rows.
///
/// There are no cursors and no positions: the reducer starts from the
/// correlated subscription snapshot and applies committed content and status
/// frames in arrival order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunSubscriptionReducer {
    session_id: SessionId,
    run_id: RunId,
    run: Option<RunProjectionDto>,
    messages: Vec<MessageProjectionDto>,
}

impl RunSubscriptionReducer {
    /// Creates empty local state fixed to one session and run.
    #[must_use]
    pub const fn new(session_id: SessionId, run_id: RunId) -> Self {
        Self {
            session_id,
            run_id,
            run: None,
            messages: Vec::new(),
        }
    }

    /// Applies the correlated first reply for this subscription.
    ///
    /// A snapshot replaces every projection value held before it.
    ///
    /// # Errors
    ///
    /// Returns a typed scoped-response error without mutation when the snapshot
    /// belongs to another session or run, and otherwise returns the carried
    /// subscription error unchanged.
    pub fn apply_initial(&mut self, response: RunSubscriptionResponseDto) -> DtoResult<()> {
        match response {
            RunSubscriptionResponseDto::Snapshot(snapshot) => {
                self.ensure_scope(snapshot.run().session_id(), Some(snapshot.run().run_id()))?;
                let mut next = Self::new(self.session_id, self.run_id);
                next.run = Some(*snapshot.run());
                next.messages = snapshot.messages().to_vec();
                *self = next;
                Ok(())
            }
            RunSubscriptionResponseDto::Error(error) => Err(error),
        }
    }

    /// Applies one uncorrelated current-state frame.
    ///
    /// # Errors
    ///
    /// Returns a typed scoped-response error without mutation for a frame that
    /// belongs to another session or run scope, and an invalid-response error
    /// for a status frame received before the subscription snapshot.
    pub fn apply_frame(&mut self, frame: RunStreamFrameDto) -> DtoResult<()> {
        match frame {
            RunStreamFrameDto::Content(message) => self.apply_content(message),
            RunStreamFrameDto::Status(status) => self.apply_status(status),
        }
    }

    fn apply_content(&mut self, message: MessageProjectionDto) -> DtoResult<()> {
        self.ensure_scope(message.session_id(), message.run_id())?;
        self.messages.push(message);
        Ok(())
    }

    fn apply_status(&mut self, frame: RunStatusFrameDto) -> DtoResult<()> {
        self.ensure_scope(frame.session_id(), Some(frame.run_id()))?;
        let Some(run) = self.run else {
            return Err(invalid_response());
        };
        self.run = Some(RunProjectionDto::new(
            run.session_id(),
            run.run_id(),
            run.turn_id(),
            frame.status(),
            run.config_revision_id(),
        ));
        Ok(())
    }

    fn ensure_scope(&self, session_id: SessionId, run_id: Option<RunId>) -> DtoResult<()> {
        if session_id == self.session_id && run_id.is_none_or(|run_id| run_id == self.run_id) {
            Ok(())
        } else {
            Err(scope_error())
        }
    }

    /// Returns the fixed session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }
    /// Returns the fixed run identity.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }
    /// Returns the current run projection, once a snapshot has been accepted.
    #[must_use]
    pub const fn run(&self) -> Option<&RunProjectionDto> {
        self.run.as_ref()
    }
    /// Returns the current run lifecycle status, once a snapshot has been accepted.
    #[must_use]
    pub const fn status(&self) -> Option<RunStatusDto> {
        match self.run {
            Some(run) => Some(run.status()),
            None => None,
        }
    }
    /// Returns the committed transcript rows accepted so far.
    #[must_use]
    pub fn messages(&self) -> &[MessageProjectionDto] {
        &self.messages
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
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|candidate| candidate.is_absolute())
            .map(|candidate| candidate.join("intention-relay"))
            .ok_or_else(|| unavailable("startup_lock_unavailable"))
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

fn is_daemon_starting(error: &ErrorDto) -> bool {
    error.code() == "local_daemon_starting"
}

fn invalid_response() -> ErrorDto {
    ErrorDto::validation(
        "invalid_local_protocol_response",
        "the local daemon returned an unexpected protocol response",
    )
}

fn scope_error() -> ErrorDto {
    ErrorDto::validation(
        "invalid_run_subscription",
        "run subscription data belongs to another run scope",
    )
}

fn request_timeout() -> ErrorDto {
    ErrorDto::unavailable(
        "local_daemon_reply_timeout",
        "the local daemon did not answer the request in time",
    )
}

fn stream_reply_timeout() -> ErrorDto {
    ErrorDto::unavailable(
        "run_stream_reply_timeout",
        "the local daemon did not answer the run-stream request in time",
    )
}

fn unavailable(code: &'static str) -> ErrorDto {
    ErrorDto::unavailable(code, "the local daemon connection is unavailable")
}
