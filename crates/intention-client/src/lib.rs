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
    ClientRequestDto, CreateSessionAcceptedDto, CreateSessionCommandDto, DaemonHealthDto,
    DtoResult, ErrorCategoryDto, ErrorDto, GetSessionSnapshotQueryDto, IdempotencyKey,
    InterruptRunAcceptedDto, InterruptRunCommandDto, MessageId, MessageKindDto,
    MessageProjectionDto, ProtocolResultDto, RemoveTurnAcceptedDto, RemoveTurnCommandDto, RunId,
    RunProjectionDto, RunStatusDto, RunStreamFrameDto, RunSubscriptionSnapshotDto,
    SendUserTurnCommandDto, SendUserTurnOutcomeDto, SessionId, SessionSnapshotDto,
    SessionSummariesDto, SetTuiThemeCommandDto, SubscribeRunCommandDto, TextDeltaChannelDto,
    TextDeltaFrameDto, ThemeDto, TuiSettingsDto, TuiThemeAcceptedDto, TurnId, decode_response,
    encode_request, parse_run_frame, run_status_is_terminal,
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
    /// The command carries no session identity to compare: the daemon assigns
    /// the session, project, and workspace, so acceptance evidence is the only
    /// thing the reply can be checked for.
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

    /// Returns the session a continuation reads, if any session exists.
    ///
    /// The daemon's session list is ordered by the recency contract: newest
    /// durable update first, then session identity ascending. The first
    /// summary is therefore the continue target, and a caller never re-derives
    /// the maximum update time from the summaries it reads.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not a session list.
    pub async fn most_recent_session(&self) -> DtoResult<Option<SessionId>> {
        Ok(self
            .list_sessions()
            .await?
            .sessions()
            .first()
            .map(|summary| summary.session_id()))
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

/// The number of newest committed transcript rows one subscription retains.
///
/// Live content frames are unbounded in number, so a long-lived subscription
/// keeps only the newest rows; the durable transcript stays complete, and a
/// re-subscribe re-reads the bounded current-state snapshot.
pub const RETAINED_TRANSCRIPT_MESSAGES: usize = 256;

/// The committed state of one fixed run scope.
///
/// The subscription reply is the current run snapshot, and every later frame
/// carries the committed value of its own scope: a content frame appends its
/// committed transcript row and a status frame replaces the committed run
/// projection. There are no cursors and no positions: a status frame is
/// idempotent because it replaces the whole run, and a content frame is
/// idempotent for the newest accepted row by the durable row identity it
/// carries, so no accepted row is shown twice. The retained transcript keeps
/// the newest [`RETAINED_TRANSCRIPT_MESSAGES`] rows, and
/// [`RunStreamState::missing_rows`] reconciles another read of the same durable
/// transcript with this one. Transient provisional text is not committed
/// state: each of the current model step's two text channels is held in its
/// own buffer for that step only and is dropped as soon as the step's assistant
/// row commits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunStreamState {
    session_id: SessionId,
    run_id: RunId,
    run: Option<RunProjectionDto>,
    messages: Vec<MessageProjectionDto>,
    /// The model step the provisional buffers currently belong to, if any.
    provisional_step: Option<u32>,
    /// The transient answer text of the current model step.
    provisional_text: String,
    /// The transient reasoning text of the current model step.
    provisional_reasoning: String,
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
            provisional_step: None,
            provisional_text: String::new(),
            provisional_reasoning: String::new(),
        }
    }

    /// Applies the correlated first reply for this subscription.
    ///
    /// A snapshot replaces every value held before it, including any transient
    /// provisional text.
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
        next.retain_newest();
        *self = next;
        Ok(())
    }

    /// Returns the rows of this snapshot that `existing` does not already
    /// carry, in order.
    ///
    /// A run subscription and a session read read the same durable transcript,
    /// so one may carry committed rows the other already shows. Rows are
    /// matched by their durable identity, so two rows with equal content but
    /// distinct identities (a user sending the same text twice) are never
    /// conflated.
    #[must_use]
    pub fn missing_rows(&self, existing: &[MessageProjectionDto]) -> Vec<MessageProjectionDto> {
        let known = existing
            .iter()
            .map(MessageProjectionDto::id)
            .collect::<BTreeSet<MessageId>>();
        self.messages
            .iter()
            .filter(|row| !known.contains(&row.id()))
            .cloned()
            .collect()
    }

    /// Applies one uncorrelated run-stream frame.
    ///
    /// A provisional text delta is best-effort advance notice: it mutates only
    /// the transient buffer of the channel it names and never the committed run
    /// or transcript. A terminal status frame ends the run and drops both
    /// buffers with it.
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
            RunStreamFrameDto::TextDelta(delta) => self.apply_text_delta(delta),
        }
    }

    fn apply_content(&mut self, message: MessageProjectionDto) -> DtoResult<()> {
        self.ensure_scope(message.session_id(), message.run_id())?;
        // The committed assistant row of a step supersedes both its channels'
        // provisional text, so neither buffer outlives its own step.
        if message.kind() == MessageKindDto::Assistant {
            self.clear_provisional();
        }
        // A content frame repeating the newest accepted row carries the
        // identity of the row the subscription snapshot already carried:
        // re-appending it would show the same committed row twice.
        if self.messages.last().map(MessageProjectionDto::id) == Some(message.id()) {
            return Ok(());
        }
        self.messages.push(message);
        self.retain_newest();
        Ok(())
    }

    fn apply_text_delta(&mut self, delta: TextDeltaFrameDto) -> DtoResult<()> {
        self.ensure_scope(delta.session_id(), Some(delta.run_id()))?;
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
        Ok(())
    }

    /// Drops the transient provisional text of every channel of the current
    /// model step.
    fn clear_provisional(&mut self) {
        self.provisional_step = None;
        self.provisional_text.clear();
        self.provisional_reasoning.clear();
    }

    /// Drops the oldest retained rows past [`RETAINED_TRANSCRIPT_MESSAGES`].
    fn retain_newest(&mut self) {
        if self.messages.len() > RETAINED_TRANSCRIPT_MESSAGES {
            let excess = self.messages.len() - RETAINED_TRANSCRIPT_MESSAGES;
            self.messages.drain(..excess);
        }
    }

    fn apply_status(&mut self, run: RunProjectionDto) -> DtoResult<()> {
        self.ensure_scope(run.session_id(), Some(run.run_id()))?;
        if self.run.is_none() {
            return Err(invalid_response());
        }
        // A terminal status ends the run: the daemon discards its unpublished
        // delta windows, so no advance notice may outlive the run.
        if run_status_is_terminal(run.status()) {
            self.clear_provisional();
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
