//! Shared bootstrap, dispatch, subscription, and reconnect client for local adapters.
//!
//! Adapters use this crate instead of direct daemon, runtime, storage, or
//! transport implementation access. It exposes typed operations over domain
//! identifiers and retains only the committed run projection and transcript
//! rows the daemon reports; daemon authority remains remote.

use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use intention_proto::{
    ClientRequestDto, CreateSessionAcceptedDto, CreateSessionCommandDto, DaemonHealthDto,
    DtoResult, ErrorCategoryDto, ErrorDto, GetSessionSnapshotQueryDto, IdempotencyKey,
    InterruptRunAcceptedDto, InterruptRunCommandDto, MessageProjectionDto, ProtocolResultDto,
    RemoveTurnAcceptedDto, RemoveTurnCommandDto, RunId, RunProjectionDto, RunStatusDto,
    RunStreamFrameDto, RunSubscriptionSnapshotDto, SendUserTurnCommandDto, SendUserTurnOutcomeDto,
    SessionId, SessionSnapshotDto, SubscribeRunCommandDto, TurnId, decode_response, encode_request,
    parse_run_frame,
};
use intention_transport::{
    AsyncLocalClientConnection, AsyncMessageReceiver, AsyncMessageSender, LocalEndpoint,
};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const STARTUP_RETRY: Duration = Duration::from_millis(25);
/// Bounded wait for one correlated command or query reply.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Bounded wait for the correlated first reply to a run-stream subscription.
const STREAM_REPLY_TIMEOUT: Duration = Duration::from_secs(30);
/// The request identity of the single request one connection carries.
const REQUEST_ID: u64 = 1;

/// Launches a daemon process after bootstrap has acquired the startup lock.
pub trait DaemonLauncher: Send + Sync {
    /// Starts one daemon host for `endpoint`.
    ///
    /// # Errors
    ///
    /// Returns only a safe typed launch error. Readiness is verified separately
    /// by `IntentionClient` through a health request on a fresh connection.
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
                ErrorDto::unavailable(
                    "local_daemon_launch_failed",
                    "the local daemon could not be started",
                )
            })
    }
}

/// One established connection with its typed directions.
struct LocalLink {
    sender: AsyncMessageSender,
    receiver: AsyncMessageReceiver,
}

/// The connected shared-client facade exposed to presentation adapters.
pub struct IntentionClient {
    endpoint: LocalEndpoint,
    launcher: Box<dyn DaemonLauncher>,
}

impl IntentionClient {
    /// Creates a typed client for one local endpoint and its launcher seam.
    #[must_use]
    pub fn new(endpoint: LocalEndpoint, launcher: Box<dyn DaemonLauncher>) -> Self {
        Self { endpoint, launcher }
    }

    /// Connects to a ready daemon or serializes exactly one local process launch.
    ///
    /// The client attempts the connection before spawning, then uses a
    /// process-wide advisory lock only when the endpoint is unavailable.
    /// Readiness is a healthy projection on a fresh connection.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error if bootstrap, launch, connection, or readiness
    /// cannot complete before the bounded deadline.
    pub async fn connect_or_bootstrap(&self) -> DtoResult<DaemonHealthDto> {
        match self.connect_ready().await {
            Ok(health) => return Ok(health),
            Err(error) if !is_unavailable(&error) => return Err(error),
            Err(_) => {}
        }

        let _lock = StartupLock::acquire(&self.endpoint)?;
        match self.connect_ready().await {
            Ok(health) => return Ok(health),
            Err(error) if !is_unavailable(&error) => return Err(error),
            Err(_) => {}
        }
        self.launcher.launch(&self.endpoint)?;
        self.wait_for_ready().await
    }

    /// Waits for the daemon to report ready health within the bootstrap budget.
    ///
    /// The wait retries an unavailable daemon with the same bounded budget and
    /// backoff used by [`IntentionClient::connect_or_bootstrap`]; it never
    /// launches a process.
    ///
    /// # Errors
    ///
    /// Returns the typed error that ended the wait: the last unavailable error
    /// when the budget expires first, or the daemon's typed rejection.
    pub async fn await_ready(&self) -> DtoResult<DaemonHealthDto> {
        self.wait_for_ready().await
    }

    /// Creates a durable session and returns the daemon's acceptance evidence.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not a session creation.
    pub async fn create_session(
        &self,
        command: CreateSessionCommandDto,
    ) -> DtoResult<CreateSessionAcceptedDto> {
        match self
            .request(ClientRequestDto::CreateSession(command))
            .await?
        {
            ProtocolResultDto::SessionCreated(created) => Ok(created),
            _ => Err(invalid_response()),
        }
    }

    /// Sends one user turn and returns the daemon's durable turn outcome.
    ///
    /// The daemon proposes the run identity: a turn that starts a run reports
    /// `Started` with that run, and a turn that joins the active run context
    /// reports `Pending`.
    ///
    /// # Errors
    ///
    /// Returns a typed validation error for blank content, the daemon's typed
    /// rejection, a typed transport or timeout error, or an invalid-response
    /// error when the reply is not an accepted turn.
    pub async fn send_user_turn(
        &self,
        session_id: SessionId,
        idempotency_key: IdempotencyKey,
        content: String,
    ) -> DtoResult<SendUserTurnOutcomeDto> {
        let command = SendUserTurnCommandDto::new(session_id, idempotency_key, content)?;
        match self
            .request(ClientRequestDto::SendUserTurn(command))
            .await?
        {
            ProtocolResultDto::TurnAccepted(turn) => Ok(turn.outcome()),
            _ => Err(invalid_response()),
        }
    }

    /// Removes one not-yet-seen pending user turn.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not a pending-turn removal.
    pub async fn remove_turn(
        &self,
        session_id: SessionId,
        turn_id: TurnId,
    ) -> DtoResult<RemoveTurnAcceptedDto> {
        match self
            .request(ClientRequestDto::RemoveTurn(RemoveTurnCommandDto::new(
                session_id, turn_id,
            )))
            .await?
        {
            ProtocolResultDto::TurnRemoved(removed) => Ok(removed),
            _ => Err(invalid_response()),
        }
    }

    /// Requests interruption of one exact active run's current operation.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not an accepted interrupt.
    pub async fn interrupt_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<InterruptRunAcceptedDto> {
        match self
            .request(ClientRequestDto::InterruptRun(InterruptRunCommandDto::new(
                session_id, run_id,
            )))
            .await?
        {
            ProtocolResultDto::RunInterrupted(interrupted) => Ok(interrupted),
            _ => Err(invalid_response()),
        }
    }

    /// Queries the current durable session snapshot.
    ///
    /// This is the single session read: it returns the current durable
    /// projection snapshot, is not a retained connection, and has no cursor or
    /// resume state. Re-reading re-reads current state.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not a session snapshot.
    pub async fn session_snapshot(&self, session_id: SessionId) -> DtoResult<SessionSnapshotDto> {
        match self
            .request(ClientRequestDto::GetSessionSnapshot(
                GetSessionSnapshotQueryDto::new(session_id),
            ))
            .await?
        {
            ProtocolResultDto::SessionSnapshot(snapshot) => Ok(snapshot),
            _ => Err(invalid_response()),
        }
    }

    async fn connect_ready(&self) -> DtoResult<DaemonHealthDto> {
        let mut link = self.connect().await?;
        match self
            .request_on(&mut link, ClientRequestDto::GetDaemonHealth)
            .await?
        {
            ProtocolResultDto::DaemonHealth(health) => Ok(health),
            _ => Err(invalid_response()),
        }
    }

    async fn request(&self, request: ClientRequestDto) -> DtoResult<ProtocolResultDto> {
        let mut link = self.connect().await?;
        self.request_on(&mut link, request).await
    }

    async fn connect(&self) -> DtoResult<LocalLink> {
        let connection = AsyncLocalClientConnection::connect(&self.endpoint).await?;
        let (sender, receiver) = connection.split();
        Ok(LocalLink { sender, receiver })
    }

    async fn request_on(
        &self,
        link: &mut LocalLink,
        request: ClientRequestDto,
    ) -> DtoResult<ProtocolResultDto> {
        let message = encode_request(REQUEST_ID, request);
        tokio::time::timeout(REQUEST_TIMEOUT, async {
            link.sender.send_message(&message).await?;
            let line = link.receiver.receive_line().await?;
            decode_response(&line, REQUEST_ID)
        })
        .await
        .map_err(|_| request_timeout())?
    }

    async fn wait_for_ready(&self) -> DtoResult<DaemonHealthDto> {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            match self.connect_ready().await {
                Ok(health) => return Ok(health),
                Err(error) if is_unavailable(&error) && Instant::now() < deadline => {
                    tokio::time::sleep(STARTUP_RETRY).await;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

/// An asynchronous facade for dedicated run-stream subscriptions.
///
/// It shares the one local link: requests flow to the daemon, and committed
/// `run.frame` values flow back.
pub struct RunStreamClient {
    endpoint: LocalEndpoint,
}

impl RunStreamClient {
    /// Creates a run-stream client for one local endpoint.
    #[must_use]
    pub const fn new(endpoint: LocalEndpoint) -> Self {
        Self { endpoint }
    }

    /// Connects, subscribes, and applies the authoritative first reply.
    ///
    /// The request carries only the session and run identity; its correlated
    /// reply is the current run state, and later committed state arrives as
    /// live `run.frame` messages on the same connection.
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
        let (mut sender, mut receiver) = connection.split();
        let request = encode_request(REQUEST_ID, ClientRequestDto::SubscribeRun(subscription));
        sender.send_message(&request).await?;
        let line = tokio::time::timeout(STREAM_REPLY_TIMEOUT, receiver.receive_line())
            .await
            .map_err(|_| stream_reply_timeout())??;
        let ProtocolResultDto::RunSubscribed(snapshot) = decode_response(&line, REQUEST_ID)? else {
            return Err(invalid_response());
        };
        let mut state = RunStreamState::new(session_id, run_id);
        state.apply_initial(snapshot)?;
        Ok(RunStreamSubscription {
            sender,
            receiver,
            state,
        })
    }
}

/// An established run-stream subscription with opaque transport resources.
pub struct RunStreamSubscription {
    /// The send direction is retained so the connection keeps both halves
    /// open while committed frames arrive on the receive direction.
    #[expect(
        dead_code,
        reason = "the send direction is held only to keep the daemon connection open for live frames"
    )]
    sender: AsyncMessageSender,
    receiver: AsyncMessageReceiver,
    state: RunStreamState,
}

impl RunStreamSubscription {
    /// Receives, applies, and returns the next committed run-stream frame.
    ///
    /// Returns `Ok(None)` once the daemon closes the stream; the caller then
    /// re-reads current state by re-subscribing. Content and status frames are
    /// applied to the committed state before they are returned.
    ///
    /// # Errors
    ///
    /// Returns a typed framing, protocol, or scope-validation error without
    /// mutating committed state. Live frames may be arbitrarily sparse, so this
    /// wait is intentionally unbounded; only the correlated reply waits carry a
    /// deadline.
    pub async fn receive(&mut self) -> DtoResult<Option<RunStreamFrameDto>> {
        let line = match self.receiver.receive_line().await {
            Ok(line) => line,
            Err(error) if error.code() == "local_daemon_connection_unavailable" => return Ok(None),
            Err(error) => return Err(error),
        };
        let frame = parse_run_frame(&line)?;
        self.state.apply_frame(frame.clone())?;
        Ok(Some(frame))
    }

    /// Returns the committed state of this fixed run scope.
    #[must_use]
    pub const fn state(&self) -> &RunStreamState {
        &self.state
    }
}

/// The committed state of one fixed run scope.
///
/// The subscription reply is the current run snapshot, and every later frame
/// carries the committed value of its own scope: a content frame appends its
/// committed transcript row and a status frame replaces the committed run
/// projection. There are no cursors and no positions, so no merge machine is
/// needed; duplicate or stale frames simply re-apply an older committed value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunStreamState {
    session_id: SessionId,
    run_id: RunId,
    run: Option<RunProjectionDto>,
    messages: Vec<MessageProjectionDto>,
}

impl RunStreamState {
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
    /// A snapshot replaces every value held before it.
    ///
    /// # Errors
    ///
    /// Returns a typed scoped-response error without mutation when the snapshot
    /// belongs to another session or run.
    pub fn apply_initial(&mut self, snapshot: RunSubscriptionSnapshotDto) -> DtoResult<()> {
        self.ensure_scope(snapshot.run().session_id(), Some(snapshot.run().run_id()))?;
        let mut next = Self::new(self.session_id, self.run_id);
        next.run = Some(*snapshot.run());
        next.messages = snapshot.messages().to_vec();
        *self = next;
        Ok(())
    }

    /// Applies one uncorrelated committed frame.
    ///
    /// # Errors
    ///
    /// Returns a typed scoped-response error without mutation for a frame that
    /// belongs to another session or run scope, and an invalid-response error
    /// for a status frame received before the subscription snapshot.
    pub fn apply_frame(&mut self, frame: RunStreamFrameDto) -> DtoResult<()> {
        match frame {
            RunStreamFrameDto::Content(message) => self.apply_content(message),
            RunStreamFrameDto::Status(run) => self.apply_status(run),
        }
    }

    fn apply_content(&mut self, message: MessageProjectionDto) -> DtoResult<()> {
        self.ensure_scope(message.session_id(), message.run_id())?;
        self.messages.push(message);
        Ok(())
    }

    fn apply_status(&mut self, run: RunProjectionDto) -> DtoResult<()> {
        self.ensure_scope(run.session_id(), Some(run.run_id()))?;
        if self.run.is_none() {
            return Err(invalid_response());
        }
        self.run = Some(run);
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

/// Reports whether one failure means the daemon endpoint is not yet reachable.
///
/// The decision is the typed error category, not a matched code string: every
/// unavailable failure is retried within the bounded bootstrap budget.
fn is_unavailable(error: &ErrorDto) -> bool {
    error.category() == ErrorCategoryDto::Unavailable
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
