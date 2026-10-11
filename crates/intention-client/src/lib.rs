//! Shared bootstrap, dispatch, subscription, and reconnect client for local adapters.
//!
//! Adapters use this crate instead of direct daemon, runtime, storage, or
//! transport implementation access. It exposes typed operations over domain
//! identifiers and retains only the committed run projection, the committed
//! transcript rows, and the transient provisional text the daemon reports;
//! daemon authority remains remote.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use intention_proto::{
    CancelRunCommandDto, ClientRequestDto, CreateSessionAcceptedDto, CreateSessionCommandDto,
    DaemonHealthDto, DtoResult, ErrorCategoryDto, ErrorDto, ErrorRetryDto,
    GetSessionSnapshotQueryDto, IdempotencyKey, InterruptRunAcceptedDto, InterruptRunCommandDto,
    MessageId, MessageKindDto, MessageProjectionDto, ProtocolResultDto, RemoveTurnAcceptedDto,
    RemoveTurnCommandDto, RunCancelledAcceptedDto, RunId, RunProjectionDto, RunStatusDto,
    SendUserTurnCommandDto, SendUserTurnOutcomeDto, SessionFrameDto, SessionId, SessionSnapshotDto,
    SessionSummariesDto, SetTuiThemeCommandDto, SubscribeSessionCommandDto, TextDeltaChannelDto,
    TextDeltaFrameDto, ThemeDto, TuiSettingsDto, TuiThemeAcceptedDto, TurnId, decode_response,
    encode_request, parse_session_frame, run_status_is_terminal,
};
use intention_transport::{
    AsyncLocalClientConnection, AsyncMessageReceiver, AsyncMessageSender, LocalEndpoint,
};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const STARTUP_RETRY: Duration = Duration::from_millis(25);
/// Bounded wait for one correlated command or query reply.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Bounded wait for the correlated first reply to a session-stream subscription.
const STREAM_REPLY_TIMEOUT: Duration = Duration::from_secs(30);
/// The request identity of the single request one connection carries.
const REQUEST_ID: u64 = 1;

/// The environment variable that declares one local endpoint instance.
///
/// The value is a safe endpoint instance identifier; an installation that
/// shares a machine with another daemon addresses its own endpoint with it.
/// The variable is a declared client surface: the terminal binary and the
/// process end-to-end seam both resolve the endpoint through it, and no adapter
/// carries a second spelling.
pub const ENDPOINT_VARIABLE: &str = "INTENTION_ENDPOINT";

/// Resolves the local endpoint the calling process addresses.
///
/// The declared instance identifier in [`ENDPOINT_VARIABLE`], when present and
/// non-blank, wins; with no declared instance the platform-default endpoint is
/// used.
///
/// # Errors
///
/// Returns the typed `invalid_local_endpoint_instance` failure when the
/// declared instance identifier is unsafe, or a typed unavailable failure when
/// the platform runtime directory cannot be determined.
pub fn endpoint_discovery() -> DtoResult<LocalEndpoint> {
    endpoint_from_declared(std::env::var(ENDPOINT_VARIABLE).ok().as_deref())
}

/// Resolves one local endpoint from an optional declared instance identifier.
///
/// The pure half of [`endpoint_discovery`]: a non-blank declared identifier
/// wins and its absence uses the platform-default endpoint.
///
/// # Errors
///
/// Returns the typed `invalid_local_endpoint_instance` failure when the
/// declared instance identifier is unsafe, or a typed unavailable failure when
/// the platform runtime directory cannot be determined.
pub fn endpoint_from_declared(declared: Option<&str>) -> DtoResult<LocalEndpoint> {
    match declared {
        Some(instance_id) if !instance_id.trim().is_empty() => {
            LocalEndpoint::from_instance_id(instance_id)
        }
        _ => LocalEndpoint::platform_default(),
    }
}

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

    /// Resolves the daemon program installed beside this process's binary.
    ///
    /// The daemon binary is the sibling of the running executable in one
    /// installation directory, so a development build and an installed pair
    /// both resolve without configuration.
    ///
    /// # Errors
    ///
    /// Returns the typed `daemon_program_unavailable` failure when the running
    /// executable has no resolvable sibling daemon program.
    pub fn platform_default() -> DtoResult<Self> {
        let executable = std::env::current_exe().map_err(|_| daemon_program_unavailable())?;
        Self::new(daemon_program_beside(&executable)?)
    }
}

/// Returns the daemon program path beside one executable path.
///
/// The pure sibling rule behind [`ProcessDaemonLauncher::platform_default`]: the
/// program is the `intention-daemon` binary in the executable's own directory,
/// carrying the platform executable suffix.
///
/// # Errors
///
/// Returns the typed `daemon_program_unavailable` failure when the executable
/// path has no parent directory or the resolved program is not valid UTF-8
/// text.
pub fn daemon_program_beside(executable: &Path) -> DtoResult<String> {
    let directory = executable.parent().ok_or_else(daemon_program_unavailable)?;
    let program = directory.join(format!("intention-daemon{}", std::env::consts::EXE_SUFFIX));
    program
        .to_str()
        .map(str::to_owned)
        .ok_or_else(daemon_program_unavailable)
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

        let _lock = StartupLock::acquire(&self.endpoint).await?;
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
    /// The command carries the identities the caller proposes and, optionally,
    /// the first user turn the creation commits as the session's first turn.
    /// The acceptance echoes the created session identity and the accepted
    /// first turn, when one was carried.
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
    /// error when the reply is not an accepted turn of this session.
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
            ProtocolResultDto::TurnAccepted(turn) if turn.session_id() == session_id => {
                Ok(turn.outcome())
            }
            _ => Err(invalid_response()),
        }
    }

    /// Removes one not-yet-seen pending user turn.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not the removal of this
    /// session's turn.
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
            ProtocolResultDto::TurnRemoved(removed)
                if removed.session_id() == session_id && removed.turn_id() == turn_id =>
            {
                Ok(removed)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Requests interruption of one exact active run's current operation.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not an accepted interrupt
    /// of this session's run.
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
            ProtocolResultDto::RunInterrupted(interrupted)
                if interrupted.session_id() == session_id && interrupted.run_id() == run_id =>
            {
                Ok(interrupted)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Requests that one exact active run stop for good.
    ///
    /// The stop is terminal where an interruption is not: the daemon signals
    /// the run's in-flight operation and the run ends `interrupted`.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not an accepted stop of
    /// this session's run.
    pub async fn cancel_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunCancelledAcceptedDto> {
        match self
            .request(ClientRequestDto::CancelRun(CancelRunCommandDto::new(
                session_id, run_id,
            )))
            .await?
        {
            ProtocolResultDto::RunCancelled(cancelled)
                if cancelled.session_id() == session_id && cancelled.run_id() == run_id =>
            {
                Ok(cancelled)
            }
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
    /// or an invalid-response error when the reply is not the requested
    /// session's snapshot.
    pub async fn session_snapshot(&self, session_id: SessionId) -> DtoResult<SessionSnapshotDto> {
        match self
            .request(ClientRequestDto::GetSessionSnapshot(
                GetSessionSnapshotQueryDto::new(session_id),
            ))
            .await?
        {
            ProtocolResultDto::SessionSnapshot(snapshot) if snapshot.session_id() == session_id => {
                Ok(snapshot)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Lists the current durable sessions in their durable order.
    ///
    /// This is a bounded read: the reply carries an explicit omitted count, so
    /// a caller renders the window as partial instead of reading it as the
    /// complete list. Like the session snapshot, this is not a retained
    /// connection and re-listing re-reads current state.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not a session list.
    pub async fn list_sessions(&self) -> DtoResult<SessionSummariesDto> {
        match self.request(ClientRequestDto::ListSessions).await? {
            ProtocolResultDto::SessionsListed(summaries) => Ok(summaries),
            _ => Err(invalid_response()),
        }
    }

    /// Resolves the session a continuation opens.
    ///
    /// The daemon's list order is the recency contract: the first summary is the
    /// continue target, so a caller never re-derives it from the summaries it
    /// reads. A list with no session is the client's typed
    /// `no_sessions_to_continue` failure - one resolution, one empty-list
    /// outcome for every front end.
    ///
    /// # Errors
    ///
    /// Returns `no_sessions_to_continue`, the daemon's typed rejection, a typed
    /// transport or timeout error, or an invalid-response error when the reply
    /// is not a session list.
    pub async fn continue_session(&self) -> DtoResult<SessionId> {
        let sessions = self.list_sessions().await?;
        match sessions.sessions().first() {
            Some(summary) => Ok(summary.session_id()),
            None => Err(no_sessions_to_continue()?),
        }
    }

    /// Reads the effective terminal settings.
    ///
    /// The daemon answers its stored theme override or, before any set, the
    /// theme the resolved configuration selected.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not terminal settings.
    pub async fn tui_settings(&self) -> DtoResult<TuiSettingsDto> {
        match self.request(ClientRequestDto::GetTuiSettings).await? {
            ProtocolResultDto::TuiSettings(settings) => Ok(settings),
            _ => Err(invalid_response()),
        }
    }

    /// Selects and stores the terminal theme.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not the acceptance of the
    /// requested theme.
    pub async fn set_tui_theme(&self, theme: ThemeDto) -> DtoResult<TuiThemeAcceptedDto> {
        match self
            .request(ClientRequestDto::SetTuiTheme(SetTuiThemeCommandDto::new(
                theme,
            )))
            .await?
        {
            ProtocolResultDto::TuiThemeSet(accepted) if accepted.theme() == theme => Ok(accepted),
            _ => Err(invalid_response()),
        }
    }

    async fn connect_ready(&self) -> DtoResult<DaemonHealthDto> {
        let link = self.connect().await?;
        match Self::request_on(link, ClientRequestDto::GetDaemonHealth).await? {
            ProtocolResultDto::DaemonHealth(health) => Ok(health),
            _ => Err(invalid_response()),
        }
    }

    async fn request(&self, request: ClientRequestDto) -> DtoResult<ProtocolResultDto> {
        let link = self.connect().await?;
        Self::request_on(link, request).await
    }

    async fn connect(&self) -> DtoResult<LocalLink> {
        let connection = AsyncLocalClientConnection::connect(&self.endpoint).await?;
        let (sender, receiver) = connection.split();
        Ok(LocalLink { sender, receiver })
    }

    /// Sends one request and decodes its correlated answer.
    ///
    /// The link is consumed, so one connection carries exactly one in-flight
    /// request; that convention is what makes the daemon's identity-less
    /// rejection of an undecodable request line unambiguous.
    async fn request_on(
        mut link: LocalLink,
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

/// An asynchronous facade for dedicated session-stream subscriptions.
///
/// It shares the one local link: requests flow to the daemon, and committed
/// session frames flow back.
pub struct SessionStreamClient {
    endpoint: LocalEndpoint,
}

impl SessionStreamClient {
    /// Creates a session-stream client for one local endpoint.
    #[must_use]
    pub const fn new(endpoint: LocalEndpoint) -> Self {
        Self { endpoint }
    }

    /// Returns the client's classification of a stream the daemon closed.
    ///
    /// A caller that cannot observe the work it followed reports this value
    /// instead of composing an error of its own: the condition is a local
    /// transport one, and the client is what classifies it.
    #[must_use]
    pub fn closed_stream_failure() -> ErrorDto {
        unavailable(CLOSED_STREAM_CODE)
    }

    /// Connects, subscribes, and applies the authoritative first reply.
    ///
    /// The request carries the session identity alone; its correlated reply is
    /// the current session snapshot, and every later committed row, active-run
    /// status, and transient delta arrives as a live frame on the same
    /// connection - whoever committed it in the session.
    ///
    /// # Errors
    ///
    /// Returns a typed protocol, transport, or scoped-response error, or an
    /// unavailable timeout error when the correlated reply does not arrive
    /// before the bounded deadline.
    pub async fn subscribe(
        &self,
        subscription: SubscribeSessionCommandDto,
    ) -> DtoResult<SessionStreamSubscription> {
        let session_id = subscription.session_id();
        let connection = AsyncLocalClientConnection::connect(&self.endpoint).await?;
        let (mut sender, mut receiver) = connection.split();
        let request = encode_request(REQUEST_ID, ClientRequestDto::SubscribeSession(subscription));
        sender.send_message(&request).await?;
        let line = tokio::time::timeout(STREAM_REPLY_TIMEOUT, receiver.receive_line())
            .await
            .map_err(|_| stream_reply_timeout())??;
        let ProtocolResultDto::SessionSubscribed(snapshot) = decode_response(&line, REQUEST_ID)?
        else {
            return Err(invalid_response());
        };
        if snapshot.session_id() != session_id {
            // The correlated reply belongs to the session this subscription
            // asked for; another session's snapshot is a scoped-response error.
            return Err(scope_error());
        }
        Ok(SessionStreamSubscription {
            sender,
            receiver,
            state: SessionStreamState::from_snapshot(snapshot),
        })
    }
}

/// An established session-stream subscription with opaque transport resources.
pub struct SessionStreamSubscription {
    /// The send direction is retained so the connection keeps both halves
    /// open while committed frames arrive on the receive direction.
    #[expect(
        dead_code,
        reason = "the send direction is held only to keep the daemon connection open for live frames"
    )]
    sender: AsyncMessageSender,
    receiver: AsyncMessageReceiver,
    state: SessionStreamState,
}

/// One observation of a live session stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamStep {
    /// One committed or transient frame.
    Frame(SessionFrameDto),
    /// The daemon closed the connection.
    ///
    /// The failure is the client's own local transport classification; a
    /// re-subscribe re-reads current state, and a caller that cannot observe the
    /// work it followed reports this value instead of composing one of its own.
    Closed(ErrorDto),
}

impl SessionStreamSubscription {
    /// Receives, applies, and returns the next observation of this stream.
    ///
    /// Content and status frames are applied to the committed state before they
    /// are returned, and a connection the daemon closed is the client's own
    /// classification rather than an error.
    ///
    /// # Errors
    ///
    /// Returns a typed framing, protocol, or scope-validation error without
    /// mutating committed state. Live frames may be arbitrarily sparse, so this
    /// wait is intentionally unbounded; only the correlated reply waits carry a
    /// deadline.
    pub async fn receive(&mut self) -> DtoResult<StreamStep> {
        let line = match self.receiver.receive_line().await {
            Ok(line) => line,
            Err(error) if error.code() == CLOSED_STREAM_CODE => {
                return Ok(StreamStep::Closed(
                    SessionStreamClient::closed_stream_failure(),
                ));
            }
            Err(error) => return Err(error),
        };
        let frame = parse_session_frame(&line)?;
        let _update = self.state.apply_frame(frame.clone())?;
        Ok(StreamStep::Frame(frame))
    }

    /// Returns the committed state of this fixed session scope.
    #[must_use]
    pub const fn state(&self) -> &SessionStreamState {
        &self.state
    }
}

/// The code the client classifies a daemon-closed stream with.
const CLOSED_STREAM_CODE: &str = "local_daemon_connection_unavailable";

/// The number of newest committed transcript rows one subscription retains.
///
/// Live content frames are unbounded in number, so a long-lived subscription
/// keeps only the newest rows; the durable transcript stays complete, and a
/// re-subscribe re-reads the bounded current-state snapshot.
pub const RETAINED_TRANSCRIPT_MESSAGES: usize = 256;

/// What one applied session-stream frame changed in the committed state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameUpdate {
    /// The frame advanced nothing: a content frame repeated the newest accepted
    /// row, or a status frame repeated the projected run.
    Unchanged,
    /// A content frame appended this committed row identity.
    Row(MessageId),
    /// A status frame replaced the session's run projection.
    Run(RunId),
    /// A transient delta advanced the provisional buffer of the run and step it
    /// names.
    Delta {
        /// The run whose step streamed the text.
        run_id: RunId,
        /// The zero-based model step the text belongs to.
        step: u32,
    },
}

/// The committed state of one fixed session scope.
///
/// The subscription reply is the current session snapshot, and every later
/// frame carries the committed value of its own scope: a content frame appends
/// its committed transcript row - of any run the session committed - and a
/// status frame replaces the session's active-run projection. There are no
/// cursors and no positions: a status frame is idempotent because it replaces
/// the whole projection, and a content frame is idempotent for the newest
/// accepted row by the durable row identity it carries, so no accepted row is
/// shown twice. The retained transcript keeps the newest
/// [`RETAINED_TRANSCRIPT_MESSAGES`] rows, and
/// [`SessionStreamState::reconcile`] is the one entry that merges another read
/// of the same durable transcript into them. Transient provisional text is not
/// committed state: each of the active run's current model step's two text
/// channels is held in its own buffer for that step only and is dropped as soon
/// as the step's assistant row commits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionStreamState {
    session_id: SessionId,
    run: Option<RunProjectionDto>,
    messages: Vec<MessageProjectionDto>,
    /// The committed rows that superseded a model step's streamed text, oldest
    /// first: one durable row identity to the step it became.
    supersessions: Vec<(MessageId, u32)>,
    /// The retained transcript's version: every mutation moves it forward.
    epoch: u64,
    /// The epoch a replacement or a front drain left the transcript at.
    ///
    /// Only rows appended at or after this epoch keep the transcript a prefix
    /// of what it was, so a query before it can never be answered append-only.
    append_base: u64,
    /// The epoch of every row appended since [`Self::append_base`], in append
    /// order, so the count of rows appended after any epoch in the run is a
    /// partition point rather than a scan.
    appends: Vec<u64>,
    /// The model step the provisional buffers currently belong to, if any.
    provisional_step: Option<u32>,
    /// The transient answer text of the current model step.
    provisional_text: String,
    /// The transient reasoning text of the current model step.
    provisional_reasoning: String,
}

impl SessionStreamState {
    /// Creates empty local state fixed to one session.
    #[must_use]
    pub const fn new(session_id: SessionId) -> Self {
        Self {
            session_id,
            run: None,
            messages: Vec::new(),
            supersessions: Vec::new(),
            epoch: 0,
            append_base: 0,
            appends: Vec::new(),
            provisional_step: None,
            provisional_text: String::new(),
            provisional_reasoning: String::new(),
        }
    }

    /// Reads one durable session snapshot into a state of its own.
    ///
    /// A snapshot is a read of the session's durable transcript: the state it
    /// returns carries the snapshot's rows - the newest
    /// [`RETAINED_TRANSCRIPT_MESSAGES`] of them - and the run projection the
    /// snapshot carried. A caller that already holds this session's state
    /// merges the read with [`SessionStreamState::reconcile`] instead, so the
    /// two reads of one transcript meet by durable row identity.
    #[must_use]
    pub fn from_snapshot(snapshot: SessionSnapshotDto) -> Self {
        let mut state = Self::new(snapshot.session_id());
        state.run = snapshot.projection().active_run();
        state.messages = snapshot.messages().to_vec();
        state.retain_newest();
        // A read is never an append: the rows it carries replace what the state
        // held, so no caller can answer an epoch query append-only across it.
        state.note_replacement();
        state
    }

    /// Reconciles a newer read of this session's durable transcript.
    ///
    /// Rows are matched by durable identity: every row the read carries joins
    /// this state's rows in identity order, and the newest
    /// [`RETAINED_TRANSCRIPT_MESSAGES`] rows are kept. The read's run projection
    /// replaces this state's - a newer read is the current one - and the
    /// supersession identity of every retained row stays.
    ///
    /// # Errors
    ///
    /// Returns the typed scoped-response error for a read of another session.
    pub fn reconcile(&mut self, read: Self) -> DtoResult<()> {
        self.ensure_scope(read.session_id)?;
        let known = self
            .messages
            .iter()
            .map(MessageProjectionDto::id)
            .collect::<BTreeSet<MessageId>>();
        self.messages.extend(
            read.messages
                .into_iter()
                .filter(|row| !known.contains(&row.id())),
        );
        self.messages.sort_by_key(MessageProjectionDto::id);
        self.run = read.run;
        // The read is a fresh observation of committed state: the transient
        // advance notice of an earlier stream never outlives it.
        self.clear_provisional();
        self.note_replacement();
        self.retain_newest();
        Ok(())
    }

    /// Returns the retained rows appended after the identity a caller last
    /// reported, in durable order.
    ///
    /// `after` is the durable identity of the newest row the caller already
    /// reported, or `None` for a caller that has reported none. The retained
    /// rows are bounded, so a caller that lets more rows commit than the bound
    /// retains before it reads cannot be shown the rows the bound dropped; a
    /// caller that reports each frame before it waits again sees every one, and
    /// a reported row the bound has since dropped never blocks the next report.
    #[must_use]
    pub fn rows_after(&self, after: Option<MessageId>) -> &[MessageProjectionDto] {
        match after {
            None => &self.messages,
            Some(after) => {
                let start = self.messages.partition_point(|row| row.id() <= after);
                &self.messages[start..]
            }
        }
    }

    /// Returns the model step whose provisional text a committed row
    /// superseded, once that row arrived.
    ///
    /// The identity is kept for as long as its row is retained, so a sink that
    /// already wrote the step's streamed answer asks for this identity instead
    /// of comparing text, and a row whose identity the window dropped has no
    /// supersession left to report.
    #[must_use]
    pub fn superseded_step(&self, row: MessageId) -> Option<u32> {
        self.supersessions
            .iter()
            .find(|(identity, _step)| *identity == row)
            .map(|(_identity, step)| *step)
    }

    /// Returns the version of the retained transcript.
    ///
    /// Every mutation moves it forward: an appended row, a reconciliation, a
    /// read, and a front trim each leave a new value.
    #[must_use]
    pub const fn transcript_epoch(&self) -> u64 {
        self.epoch
    }

    /// Returns how many rows were appended since `epoch`, when the rows current
    /// at `epoch` remain a prefix of the retained transcript; `None` means a
    /// reconciliation, a read, or a front trim ended the append-only run.
    #[must_use]
    pub fn transcript_appended_since(&self, epoch: u64) -> Option<usize> {
        if epoch > self.epoch || epoch < self.append_base {
            return None;
        }
        let before = self.appends.partition_point(|mark| *mark <= epoch);
        Some(self.appends.len() - before)
    }

    /// Ends this state's stream observation.
    ///
    /// The transient provisional text belongs to the stream that streamed it,
    /// so a stream that ended drops both of its channels' buffers. The
    /// committed rows and the run projection stay: they are the session's own
    /// durable state, and a front end that lost one stream still knows what the
    /// session committed and which run it last observed.
    pub fn end_stream(&mut self) {
        self.clear_provisional();
    }

    /// Applies one uncorrelated session-stream frame and reports what it
    /// changed.
    ///
    /// A provisional text delta is best-effort advance notice: it mutates only
    /// the transient buffer of the channel it names and never the committed
    /// projection or transcript. A terminal status frame ends the run it names
    /// and drops both buffers with it.
    ///
    /// # Errors
    ///
    /// Returns a typed scoped-response error without mutation for a frame that
    /// belongs to another session.
    pub fn apply_frame(&mut self, frame: SessionFrameDto) -> DtoResult<FrameUpdate> {
        match frame {
            SessionFrameDto::Content(message) => self.apply_content(message),
            SessionFrameDto::Status(run) => self.apply_status(run),
            SessionFrameDto::TextDelta(delta) => self.apply_text_delta(delta),
        }
    }

    fn apply_content(&mut self, message: MessageProjectionDto) -> DtoResult<FrameUpdate> {
        self.ensure_scope(message.session_id())?;
        // A content frame repeating the newest accepted row carries the
        // identity of the row the subscription snapshot already carried:
        // re-appending it would show the same committed row twice.
        let repeated = self.messages.last().map(MessageProjectionDto::id) == Some(message.id());
        if message.kind() == MessageKindDto::Assistant {
            // The committed assistant row of a step supersedes both its
            // channels' provisional text, and it carries the identity of the
            // step it became: a sink that already wrote that step's streamed
            // text asks for the identity instead of comparing text.
            if !repeated && let Some(step) = self.provisional_step {
                self.supersessions.push((message.id(), step));
            }
            self.clear_provisional();
        }
        if repeated {
            return Ok(FrameUpdate::Unchanged);
        }
        let id = message.id();
        self.messages.push(message);
        self.note_append();
        self.retain_newest();
        Ok(FrameUpdate::Row(id))
    }

    fn apply_text_delta(&mut self, delta: TextDeltaFrameDto) -> DtoResult<FrameUpdate> {
        self.ensure_scope(delta.session_id())?;
        // Transient text belongs to the session's current run: a delta of a run
        // this state no longer projects is a cadence frame that crossed its
        // run's own terminal commit, so it changes nothing.
        if self
            .run
            .as_ref()
            .is_none_or(|run| run.run_id() != delta.run_id())
        {
            return Ok(FrameUpdate::Unchanged);
        }
        // A delta for a new step replaces both buffers: the previous step's
        // advance notice is stale once the run moves on.
        if self.provisional_step != Some(delta.step()) {
            self.provisional_step = Some(delta.step());
            self.provisional_text.clear();
            self.provisional_reasoning.clear();
        }
        // The channel is what keeps the reasoning a step thinks through apart
        // from the answer it commits: each buffer grows on its own, so no
        // reasoning chunk ever becomes answer text.
        match delta.channel() {
            TextDeltaChannelDto::Answer => self.provisional_text.push_str(delta.text()),
            TextDeltaChannelDto::Reasoning => self.provisional_reasoning.push_str(delta.text()),
        }
        Ok(FrameUpdate::Delta {
            run_id: delta.run_id(),
            step: delta.step(),
        })
    }

    /// Drops the transient provisional text of every channel of the current
    /// model step.
    fn clear_provisional(&mut self) {
        self.provisional_step = None;
        self.provisional_text.clear();
        self.provisional_reasoning.clear();
    }

    /// Records one appended row in the append-only run.
    fn note_append(&mut self) {
        self.epoch += 1;
        self.appends.push(self.epoch);
    }

    /// Moves the epoch past a mutation that is not an append.
    ///
    /// A reconciliation, a read, and a front drain all move every retained row,
    /// so the append-only run ends here and a stale caller lays the whole
    /// transcript out again.
    fn note_replacement(&mut self) {
        self.epoch += 1;
        self.appends.clear();
        self.append_base = self.epoch;
    }

    /// Drops the oldest retained rows past [`RETAINED_TRANSCRIPT_MESSAGES`].
    ///
    /// A front drain moves every retained row, so it is a replacement and never
    /// an append.
    fn retain_newest(&mut self) {
        if self.messages.len() > RETAINED_TRANSCRIPT_MESSAGES {
            let excess = self.messages.len() - RETAINED_TRANSCRIPT_MESSAGES;
            self.messages.drain(..excess);
            self.note_replacement();
        }
        self.prune_supersessions();
    }

    /// Drops the supersession identity of every row the window no longer holds.
    ///
    /// The identity lives exactly as long as its row: a sink that kept its own
    /// copy of the step it streamed can never be told about a row the window
    /// dropped, and it prints that row in full instead.
    fn prune_supersessions(&mut self) {
        let retained = self
            .messages
            .iter()
            .map(MessageProjectionDto::id)
            .collect::<BTreeSet<MessageId>>();
        self.supersessions
            .retain(|(row, _step)| retained.contains(row));
    }

    fn apply_status(&mut self, run: RunProjectionDto) -> DtoResult<FrameUpdate> {
        self.ensure_scope(run.session_id())?;
        // The first status of a run the session did not carry yet is exactly
        // how a window learns that a run started elsewhere, so a status frame
        // is accepted whenever it belongs to the session.
        // A terminal status ends the run: the daemon discards its unpublished
        // delta windows, so no advance notice may outlive the run.
        if run_status_is_terminal(run.status()) {
            self.clear_provisional();
        }
        let changed = self.run.as_ref() != Some(&run);
        let run_id = run.run_id();
        self.run = Some(run);
        Ok(if changed {
            FrameUpdate::Run(run_id)
        } else {
            FrameUpdate::Unchanged
        })
    }

    fn ensure_scope(&self, session_id: SessionId) -> DtoResult<()> {
        if session_id == self.session_id {
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

    /// Returns the transient provisional answer text of the current model step.
    ///
    /// The text is best-effort advance notice, never committed state: it is
    /// empty until the first delta of a step arrives and is cleared once that
    /// step's committed assistant row arrives or the run reaches a terminal
    /// status. Only the answer channel grows this buffer; the reasoning the
    /// step precedes it with is buffered separately.
    #[must_use]
    pub fn provisional_text(&self) -> &str {
        &self.provisional_text
    }

    /// Returns the transient provisional reasoning text of the current model
    /// step.
    ///
    /// The reasoning channel is advance notice exactly like the answer text,
    /// with the same lifetime: it is empty until the first reasoning delta of a
    /// step arrives and is cleared with the answer once that step's committed
    /// assistant row arrives or the run reaches a terminal status.
    #[must_use]
    pub fn provisional_reasoning(&self) -> &str {
        &self.provisional_reasoning
    }
}

struct StartupLock {
    file: std::fs::File,
}

impl StartupLock {
    /// Takes the per-endpoint bootstrap lock within the bootstrap budget.
    ///
    /// # Errors
    ///
    /// Returns the typed startup-lock failure when the lock cannot be taken
    /// before the budget expires.
    async fn acquire(endpoint: &LocalEndpoint) -> DtoResult<Self> {
        Self::acquire_path(&startup_lock_path(endpoint)?, STARTUP_TIMEOUT).await
    }

    /// Takes the lock at `path`, retrying a contended lock until `budget` ends.
    ///
    /// The wait is bounded and yields to the runtime between attempts, so a
    /// bootstrap never blocks a task on another process without a deadline.
    ///
    /// # Errors
    ///
    /// Returns the typed startup-lock failure when the lock cannot be taken
    /// before `budget` expires.
    async fn acquire_path(path: &Path, budget: Duration) -> DtoResult<Self> {
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
            .open(path)
            .map_err(|_| unavailable("startup_lock_unavailable"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|_| unavailable("startup_lock_unavailable"))?;
        }
        let deadline = tokio::time::Instant::now() + budget;
        loop {
            match fs4::FileExt::try_lock(&file) {
                Ok(()) => return Ok(Self { file }),
                Err(fs4::TryLockError::WouldBlock) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(STARTUP_RETRY).await;
                }
                Err(fs4::TryLockError::WouldBlock) => return Err(startup_lock_timeout()),
                Err(fs4::TryLockError::Error(_)) => {
                    return Err(unavailable("startup_lock_unavailable"));
                }
            }
        }
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

/// Returns the client's typed failure for a continuation with no session.
///
/// # Errors
///
/// Returns the constructor's own validation error for a blank code or message;
/// the constants here are non-blank, so the failure is the typed
/// `no_sessions_to_continue`.
fn no_sessions_to_continue() -> DtoResult<ErrorDto> {
    ErrorDto::new(
        "no_sessions_to_continue",
        ErrorCategoryDto::NotFound,
        "the daemon reports no session to continue",
        ErrorRetryDto::Never,
        None,
    )
}

fn scope_error() -> ErrorDto {
    ErrorDto::validation(
        "invalid_session_subscription",
        "session subscription data belongs to another session scope",
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
        "session_stream_reply_timeout",
        "the local daemon did not answer the session-stream request in time",
    )
}

fn unavailable(code: &'static str) -> ErrorDto {
    ErrorDto::unavailable(code, "the local daemon connection is unavailable")
}

/// Returns the typed failure for a daemon program the client cannot resolve.
fn daemon_program_unavailable() -> ErrorDto {
    ErrorDto::unavailable(
        "daemon_program_unavailable",
        "the daemon program could not be resolved",
    )
}

fn startup_lock_timeout() -> ErrorDto {
    ErrorDto::unavailable(
        "local_daemon_startup_timeout",
        "the local daemon bootstrap lock was not released in time",
    )
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "Unit fixtures use direct assertions for precise diagnostics."
    )]

    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn startup_lock_wait_is_bounded_by_its_budget() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after the Unix epoch")
            .as_nanos();
        // The fixture root is its own directory under the temporary directory:
        // the lock path owns its parent, so the fixture never chmods a shared
        // directory such as the platform runtime directory.
        let base = std::env::temp_dir().join(format!(
            "intention-client-lock-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&base).expect("the fixture directory is created");
        let path = base.join("bootstrap.lock");
        let budget = Duration::from_millis(50);
        let held = StartupLock::acquire_path(&path, budget)
            .await
            .expect("an uncontended bootstrap lock is taken");

        let error = match StartupLock::acquire_path(&path, budget).await {
            Ok(_) => panic!("a contended bootstrap lock must end the bounded wait"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "local_daemon_startup_timeout");
        assert_eq!(error.category(), ErrorCategoryDto::Unavailable);

        drop(held);
        let _ = fs::remove_dir_all(&base);
    }
}
