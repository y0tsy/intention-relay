//! Shared bootstrap, dispatch, subscription, and reconnect client for local adapters.
//!
//! Adapters use this crate instead of direct daemon, runtime, storage, or
//! transport implementation access. It retains only reconnect projection state;
//! daemon authority remains remote.

use std::collections::{BTreeSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use intention_domain::{ModelRunFactInputDto, RunEventCursorDto, RunSnapshotDto};
use intention_protocol::{
    DaemonHealthDto, DaemonReadinessDto, ProtocolHelloDto, ProtocolMethodDto, ProtocolQueryDto,
    ProtocolQueryResultDto, ProtocolRequestPayloadDto, ProtocolResponsePayloadDto, RunLiveBatchDto,
    RunResyncDto, RunResyncReasonDto, RunStreamFrameDto, RunSubscriptionResponseDto,
    SessionSnapshotDto, SessionSubscriptionResponseDto, SubscribeRunCommandDto,
    SubscribeSessionCommandDto, decode_response, encode_request, parse_run_frame_notification,
};
use intention_transport::{
    AsyncLocalClientConnection, AsyncMessageReceiver, AsyncRequestSender, LocalConnection,
    LocalEndpoint, local_protocol_version, negotiate_client,
};
use intention_types::{DtoResult, ErrorCategoryDto, ErrorDto, SchemaVersionDto, SessionId};

const SCHEMA_VERSION: SchemaVersionDto = intention_protocol::CURRENT_DTO_SCHEMA_VERSION;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const STARTUP_RETRY: Duration = Duration::from_millis(25);
/// Bounded wait for one correlated run-stream reply or notification.
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
                    intention_types::ErrorRetryDto::Manual,
                    None,
                )
                .unwrap_or_else(|_| unavailable("local_daemon_launch_failed"))
            })
    }
}

struct NegotiatedConnection {
    connection: LocalConnection,
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
    pub fn connect_or_bootstrap(&self) -> DtoResult<DaemonHealthDto> {
        match self.connect_ready() {
            Ok(health) => return Ok(health),
            Err(error) if is_daemon_starting(&error) => return self.wait_for_ready(),
            Err(error) if !is_daemon_unavailable(&error) => return Err(error),
            Err(_) => {}
        }

        let _lock = StartupLock::acquire(&self.endpoint)?;
        match self.connect_ready() {
            Ok(health) => return Ok(health),
            Err(error) if is_daemon_starting(&error) => return self.wait_for_ready(),
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
    /// Returns a safe typed transport, protocol, or non-ready daemon error.
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

    /// Obtains a consistent snapshot or typed resync response.
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
            )) => match health.readiness() {
                DaemonReadinessDto::Ready => Ok(health),
                DaemonReadinessDto::Starting => Err(ErrorDto::new(
                    "local_daemon_starting",
                    ErrorCategoryDto::Unavailable,
                    "the local daemon is starting",
                    intention_types::ErrorRetryDto::Delayed,
                    None,
                )
                .unwrap_or_else(|_| unavailable("local_daemon_starting"))),
                DaemonReadinessDto::Draining | DaemonReadinessDto::Unavailable => {
                    Err(ErrorDto::new(
                        "local_daemon_not_ready",
                        ErrorCategoryDto::Unavailable,
                        "the local daemon is not ready to serve requests",
                        intention_types::ErrorRetryDto::Delayed,
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

    fn request(&self, payload: ProtocolRequestPayloadDto) -> DtoResult<ProtocolResponsePayloadDto> {
        let mut connection = self.connect()?;
        self.request_on(&mut connection, payload)
    }

    fn connect(&self) -> DtoResult<NegotiatedConnection> {
        let mut connection = LocalConnection::connect(&self.endpoint)?;
        negotiate_client(&mut connection, self.hello.clone())?;
        Ok(NegotiatedConnection { connection })
    }

    fn request_on(
        &self,
        connection: &mut NegotiatedConnection,
        payload: ProtocolRequestPayloadDto,
    ) -> DtoResult<ProtocolResponsePayloadDto> {
        let method = ProtocolMethodDto::for_payload(&payload);
        let request = encode_request(1, payload);
        connection.connection.send_message(&request)?;
        let line = connection.connection.receive_line()?;
        decode_response(&line, method, 1)
    }

    fn wait_for_ready(&self) -> DtoResult<DaemonHealthDto> {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            match self.connect_ready() {
                Ok(health) => return Ok(health),
                Err(error)
                    if (is_daemon_unavailable(&error) || is_daemon_starting(&error))
                        && Instant::now() < deadline =>
                {
                    thread::sleep(STARTUP_RETRY);
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
            next_request_id: 2,
            pending_frames: VecDeque::new(),
            reducer,
        })
    }
}

/// An established run-stream subscription with opaque transport resources.
pub struct RunStreamSubscription {
    requests: AsyncRequestSender,
    messages: AsyncMessageReceiver,
    next_request_id: u64,
    pending_frames: VecDeque<RunStreamFrameDto>,
    reducer: RunSubscriptionReducer,
}

impl RunStreamSubscription {
    /// Receives and applies one daemon run-frame notification.
    ///
    /// Frames buffered while an earlier replay reply was pending are applied
    /// first. A returned resync is locally generated for a detected cursor gap.
    /// A daemon-originated matching resync clears the reducer and returns
    /// `None`.
    ///
    /// # Errors
    ///
    /// Returns a typed framing, protocol, or scope-validation error. Live
    /// frames may be arbitrarily sparse, so this wait is intentionally
    /// unbounded; only the correlated reply waits carry a deadline.
    pub async fn receive(&mut self) -> DtoResult<Option<RunResyncDto>> {
        if let Some(frame) = self.pending_frames.pop_front() {
            return self.reducer.apply_frame(frame);
        }
        let line = self.messages.receive_line().await?;
        let frame = parse_run_frame_notification(&line)?;
        self.reducer.apply_frame(frame)
    }

    /// Sends a new subscription request from the last valid cursor and applies
    /// its immediate replay, resync, or error response.
    ///
    /// Frames the daemon queued ahead of the correlated reply are buffered and
    /// applied in arrival order after the reply, so the reply is never mistaken
    /// for a frame.
    ///
    /// # Errors
    ///
    /// Returns a typed transport, protocol, or scoped-response error, or an
    /// unavailable timeout error when the correlated reply does not arrive
    /// before the bounded deadline. The current reducer state is retained if
    /// the reply is invalid or rejected.
    pub async fn request_replay(&mut self) -> DtoResult<()> {
        let subscription = SubscribeRunCommandDto::new(
            SCHEMA_VERSION,
            self.reducer.session_id(),
            self.reducer.run_id(),
            self.reducer.last_cursor(),
        );
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        let request = encode_request(
            request_id,
            ProtocolRequestPayloadDto::RunSubscription(subscription),
        );
        self.requests.send_message(&request).await?;
        let response = self.await_replay_response(request_id).await?;
        match response {
            ProtocolResponsePayloadDto::RunSubscription(response) => {
                self.reducer.apply_initial(response)?;
            }
            _ => return Err(invalid_response()),
        }
        self.apply_buffered_frames()
    }

    /// Reads until the correlated replay reply, buffering interleaved frames.
    ///
    /// Only a `run.frame` notification is buffered; any other line, including a
    /// response with another correlation id, is rejected by `decode_response`.
    async fn await_replay_response(
        &mut self,
        request_id: u64,
    ) -> DtoResult<ProtocolResponsePayloadDto> {
        tokio::time::timeout(STREAM_REPLY_TIMEOUT, async {
            loop {
                let line = self.messages.receive_line().await?;
                match parse_run_frame_notification(&line) {
                    Ok(frame) => self.pending_frames.push_back(frame),
                    Err(_) => {
                        return decode_response(&line, ProtocolMethodDto::RunSubscribe, request_id);
                    }
                }
            }
        })
        .await
        .map_err(|_| stream_reply_timeout())?
    }

    /// Applies buffered frames in order, stopping at the first local resync.
    ///
    /// A frame that requires a resync is left buffered so the next `receive`
    /// reports that resync; the reducer keeps its state for both paths.
    fn apply_buffered_frames(&mut self) -> DtoResult<()> {
        while let Some(frame) = self.pending_frames.pop_front() {
            if self.reducer.apply_frame(frame.clone())?.is_some() {
                self.pending_frames.push_front(frame);
                return Ok(());
            }
        }
        Ok(())
    }

    /// Returns the state reducer for this fixed run scope.
    #[must_use]
    pub const fn reducer(&self) -> &RunSubscriptionReducer {
        &self.reducer
    }

    /// Returns mutable reducer state for adapter-directed recovery.
    #[must_use]
    pub const fn reducer_mut(&mut self) -> &mut RunSubscriptionReducer {
        &mut self.reducer
    }
}

/// A run-scoped reducer preserving daemon-authoritative snapshots and cursor order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunSubscriptionReducer {
    session_id: intention_types::SessionId,
    run_id: intention_types::RunId,
    snapshot: Option<RunSnapshotDto>,
    last_cursor: Option<RunEventCursorDto>,
    reasoning_content: String,
    historical_reasoning_cursors: BTreeSet<RunEventCursorDto>,
    history_unavailable: bool,
}

impl RunSubscriptionReducer {
    /// Creates empty local state fixed to one session and run.
    #[must_use]
    pub const fn new(
        session_id: intention_types::SessionId,
        run_id: intention_types::RunId,
    ) -> Self {
        Self {
            session_id,
            run_id,
            snapshot: None,
            last_cursor: None,
            reasoning_content: String::new(),
            historical_reasoning_cursors: BTreeSet::new(),
            history_unavailable: false,
        }
    }

    /// Applies the correlated first reply for this subscription.
    ///
    /// # Errors
    ///
    /// Returns a typed scoped-response error without mutation for a replay that
    /// belongs to another session or run.
    pub fn apply_initial(&mut self, response: RunSubscriptionResponseDto) -> DtoResult<()> {
        match response {
            RunSubscriptionResponseDto::Replay(snapshot) => {
                self.ensure_scope(snapshot.session_id(), snapshot.run_id())?;
                let cursor = snapshot.cursor();
                let mut next = Self::new(self.session_id, self.run_id);
                next.snapshot = Some(snapshot);
                next.last_cursor = Some(cursor);
                *self = next;
                Ok(())
            }
            RunSubscriptionResponseDto::Resync(resync) => self.apply_resync(resync),
            RunSubscriptionResponseDto::Error(error) => Err(error),
        }
    }

    /// Applies one uncorrelated run-stream frame.
    ///
    /// # Errors
    ///
    /// Returns a typed scoped-response error without mutation for wrong scope or
    /// frames sent after an initial unavailable-history response.
    pub fn apply_frame(&mut self, frame: RunStreamFrameDto) -> DtoResult<Option<RunResyncDto>> {
        if self.history_unavailable {
            return Err(invalid_response());
        }
        match frame {
            RunStreamFrameDto::LiveBatch(batch) => self.apply_live_batch(batch),
            RunStreamFrameDto::Snapshot(frame) => {
                self.ensure_scope(frame.session_id(), frame.run_id())?;
                self.snapshot = Some(frame.snapshot().clone());
                self.last_cursor = Some(frame.snapshot().cursor());
                self.historical_reasoning_cursors.clear();
                Ok(None)
            }
            RunStreamFrameDto::Resync(resync) => {
                self.apply_resync(resync)?;
                Ok(None)
            }
        }
    }

    /// Applies a live batch, returning a local cursor-gap resync without mutation.
    ///
    /// # Errors
    ///
    /// Returns a typed scope-validation error without mutation for another run.
    pub fn apply_live_batch(&mut self, batch: RunLiveBatchDto) -> DtoResult<Option<RunResyncDto>> {
        self.ensure_scope(batch.session_id(), batch.run_id())?;
        let last = self.last_cursor.map_or(0, RunEventCursorDto::value);
        if let Some(first_new) = batch
            .facts()
            .iter()
            .find(|fact| fact.cursor().value() > last)
            && first_new.cursor().value() != last.saturating_add(1)
        {
            return Ok(Some(RunResyncDto::new(
                self.session_id,
                self.run_id,
                RunResyncReasonDto::CursorGap,
            )));
        }
        let snapshot_cursor = self
            .snapshot
            .as_ref()
            .map_or(RunEventCursorDto::new(0), RunSnapshotDto::cursor);
        let mut next_cursor = self.last_cursor;
        let mut appended_reasoning = String::new();
        let mut historical_cursors = self.historical_reasoning_cursors.clone();
        for fact in batch.facts() {
            if fact.cursor().value() <= snapshot_cursor.value() {
                if let ModelRunFactInputDto::ReasoningDeltaRecorded { content, .. } = fact.input()
                    && historical_cursors.insert(fact.cursor())
                {
                    appended_reasoning.push_str(content);
                }
                continue;
            }
            if fact.cursor().value() <= last {
                continue;
            }
            next_cursor = Some(fact.cursor());
        }
        self.reasoning_content.push_str(&appended_reasoning);
        self.historical_reasoning_cursors = historical_cursors;
        self.last_cursor = next_cursor;
        Ok(None)
    }

    fn apply_resync(&mut self, resync: RunResyncDto) -> DtoResult<()> {
        self.ensure_scope(resync.session_id(), resync.run_id())?;
        self.snapshot = None;
        self.last_cursor = None;
        self.reasoning_content.clear();
        self.historical_reasoning_cursors.clear();
        self.history_unavailable = resync.reason() == RunResyncReasonDto::HistoryUnavailable;
        Ok(())
    }

    fn ensure_scope(
        &self,
        session_id: intention_types::SessionId,
        run_id: intention_types::RunId,
    ) -> DtoResult<()> {
        if session_id == self.session_id && run_id == self.run_id {
            Ok(())
        } else {
            Err(ErrorDto::validation(
                "invalid_run_subscription",
                "run subscription data belongs to another run scope",
            ))
        }
    }

    /// Returns the fixed session identity.
    #[must_use]
    pub const fn session_id(&self) -> intention_types::SessionId {
        self.session_id
    }
    /// Returns the fixed run identity.
    #[must_use]
    pub const fn run_id(&self) -> intention_types::RunId {
        self.run_id
    }
    /// Returns the authoritative daemon snapshot, if one has been accepted.
    #[must_use]
    pub fn snapshot(&self) -> Option<RunSnapshotDto> {
        self.snapshot.clone()
    }
    /// Returns the last accepted durable run cursor.
    #[must_use]
    pub const fn last_cursor(&self) -> Option<RunEventCursorDto> {
        self.last_cursor
    }
    /// Returns tail-only historical reasoning accepted after an authoritative snapshot.
    #[must_use]
    pub fn reasoning_content(&self) -> &str {
        &self.reasoning_content
    }
    /// Reports whether the initial reply failed closed for unavailable history.
    #[must_use]
    pub const fn history_unavailable(&self) -> bool {
        self.history_unavailable
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

fn stream_reply_timeout() -> ErrorDto {
    ErrorDto::unavailable(
        "run_stream_reply_timeout",
        "the local daemon did not answer the run-stream request in time",
    )
}

fn unavailable(code: &'static str) -> ErrorDto {
    ErrorDto::unavailable(code, "the local daemon connection is unavailable")
}
