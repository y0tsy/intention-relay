//! The headless front end: one prompt driven to a terminal run status.
//!
//! The module owns the blocking client driver the line front ends share. The
//! driver owns one [`AppState`] and performs the effects the core returns
//! through the shared [`client_task`] mapper over one client, so no front end
//! repeats session, turn, transcript, or stream logic: those are values of the
//! core. Around the driver the module owns the wait deadline, the two output
//! formats, and the mapping of a driven run to one process status.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use intention_client::{IntentionClient, RunStreamSubscription};
use intention_proto::{
    ErrorDto, MessageKindDto, MessageProjectionDto, RunId, RunModeDto, RunStatusDto,
    RunStreamFrameDto, SessionId, TextDeltaChannelDto, WorkspaceRootDto, run_status_is_terminal,
};
use intention_transport::LocalEndpoint;
use intention_tui::app::{Action, AppState, Effect};
use intention_tui::client_task;

use crate::cli::{ExitStatus, Format, Options};

/// The bounded wait after a deadline interrupt before the process ends with the
/// timeout status, letting the daemon publish the effect of the interrupt.
// @todo(hack): the interrupt is best-effort: nothing on the wire tells the front
// end that the daemon applied it, so the run gets a fixed two-second grace and
// whatever the grace observes is discarded. An acknowledged interrupt would let
// the timeout status name the real outcome.
const INTERRUPT_GRACE: Duration = Duration::from_secs(2);

/// Runs one prompt and returns the process status the command ends with.
pub fn run<W: Write, E: Write>(
    prompt: &str,
    options: &Options,
    client: IntentionClient,
    endpoint: LocalEndpoint,
    out: &mut W,
    err: &mut E,
) -> ExitStatus {
    let runtime = match crate::session_runtime() {
        Ok(runtime) => runtime,
        Err(error) => return crate::startup_failure(&error, err),
    };
    runtime.block_on(drive(prompt, options, client, endpoint, out, err))
}

/// Selects the session, sends the prompt, and streams the run to its end.
async fn drive<W: Write, E: Write>(
    prompt: &str,
    options: &Options,
    client: IntentionClient,
    endpoint: LocalEndpoint,
    out: &mut W,
    err: &mut E,
) -> ExitStatus {
    let workspace_root = match crate::workspace_root(options.workspace()) {
        Ok(root) => root,
        Err(error) => return crate::startup_failure(&error, err),
    };
    let mut driver = Driver::new(client, endpoint, workspace_root, options.mode());
    let mut output = Output::new(options.format(), out);

    if let Err(error) = driver.connect().await {
        return client_failure(&mut output, err, &error);
    }
    match options.session() {
        Some(session) => driver.show(session).await,
        None if options.continue_session() => match driver.most_recent_session().await {
            Ok(Some(session)) => driver.show(session).await,
            Ok(None) => return client_failure(&mut output, err, &no_sessions_error()),
            Err(error) => return client_failure(&mut output, err, &error),
        },
        None => driver.create().await,
    }
    if driver.state().session_id().is_none() {
        return client_failure(&mut output, err, &driver.session_failure());
    }

    let deadline = Deadline::after(options.timeout(), Instant::now());
    driver.send_turn(prompt).await;
    if let Some(error) = driver.last_failure().cloned() {
        return client_failure(&mut output, err, &error);
    }
    match driver.pump(&mut output, deadline).await {
        Ok(outcome) => {
            let session = driver.state().session_id();
            let run = driver.state().active_run().map(|run| run.run_id());
            let _ = output.finish(outcome, session, run);
            exit_status(outcome)
        }
        Err(failure) => pump_failure(&mut output, err, &failure),
    }
}

/// Reports one typed client failure in the selected output format.
fn client_failure<W: Write, E: Write>(
    output: &mut Output<'_, W>,
    err: &mut E,
    error: &ErrorDto,
) -> ExitStatus {
    let _ = output.error(err, error);
    ExitStatus::for_error(error)
}

/// Reports one driven-run failure in the selected output format.
fn pump_failure<W: Write, E: Write>(
    output: &mut Output<'_, W>,
    err: &mut E,
    failure: &PumpFailure,
) -> ExitStatus {
    match failure {
        PumpFailure::Client(error) => client_failure(output, err, error),
        PumpFailure::Output(_error) => {
            let _ = writeln!(err, "error: the run output could not be written");
            ExitStatus::Daemon
        }
    }
}

/// Returns the process status one driven run ends with.
fn exit_status(outcome: RunOutcome) -> ExitStatus {
    match outcome {
        RunOutcome::TimedOut => ExitStatus::Timeout,
        RunOutcome::Terminal(status) => {
            ExitStatus::for_run_status(status).unwrap_or(ExitStatus::Rejected)
        }
    }
}

/// Returns the failure of a continuation with no session to continue.
fn no_sessions_error() -> ErrorDto {
    ErrorDto::validation(
        "no_sessions_to_continue",
        "the daemon reports no session to continue",
    )
}

/// Returns the outcome of one reported run status, or `None` while it is live.
fn terminal_outcome(status: Option<RunStatusDto>) -> Option<RunOutcome> {
    status
        .filter(|status| run_status_is_terminal(*status))
        .map(RunOutcome::Terminal)
}

/// The terminal end of one driven run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunOutcome {
    /// The run reported one terminal status.
    Terminal(RunStatusDto),
    /// The wait deadline passed after a best-effort interrupt.
    TimedOut,
}

impl RunOutcome {
    /// Returns the stable record representation of this outcome.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Terminal(status) => status.as_str(),
            Self::TimedOut => "timed_out",
        }
    }

    /// Returns the human description of this outcome.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Terminal(status) => status.as_str(),
            Self::TimedOut => "timed out",
        }
    }
}

/// One failure a driven run reports to its front end.
#[derive(Debug)]
pub enum PumpFailure {
    /// A shared-client operation failed.
    Client(ErrorDto),
    /// Writing the front end's output failed.
    Output(io::Error),
}

/// The absolute wait deadline of one run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Deadline {
    at: Instant,
}

impl Deadline {
    /// Returns the deadline `timeout` after `now`, or `None` for no bound.
    ///
    /// A timeout the monotonic clock cannot represent means no bounded wait.
    // @todo(hack): an unrepresentable `--timeout` silently degrades into an
    // unbounded wait, which weakens the user's own bound instead of reporting
    // it; the caller should hear that the deadline could not be armed.
    #[must_use]
    pub fn after(timeout: Option<Duration>, now: Instant) -> Option<Self> {
        timeout
            .and_then(|timeout| now.checked_add(timeout))
            .map(|at| Self { at })
    }

    /// Returns the wait left before the deadline, or `None` once it passed.
    #[must_use]
    pub fn remaining(self, now: Instant) -> Option<Duration> {
        self.at
            .checked_duration_since(now)
            .filter(|remaining| !remaining.is_zero())
    }
}

/// The next observation of a live run subscription.
#[derive(Debug)]
enum Received {
    /// One committed or transient frame arrived.
    Frame(RunStreamFrameDto),
    /// The subscription ended or failed; the core holds the reported reason.
    Closed,
    /// No live subscription is open.
    Empty,
    /// The wait deadline passed.
    Expired,
}

/// One output sink for the observable events of a driven run.
pub trait Report {
    /// Reports one transient provisional text chunk of one model step and
    /// channel.
    ///
    /// Every chunk carries the channel it belongs to, so a sink decides for
    /// itself what to do with the reasoning a step streams before its answer.
    ///
    /// # Errors
    ///
    /// Returns the output failure.
    fn delta(&mut self, step: u32, channel: TextDeltaChannelDto, text: &str) -> io::Result<()>;

    /// Reports one committed transcript row.
    ///
    /// # Errors
    ///
    /// Returns the output failure.
    fn row(&mut self, row: &MessageProjectionDto) -> io::Result<()>;

    /// Reports one observed run lifecycle status.
    ///
    /// # Errors
    ///
    /// Returns the output failure.
    fn status(&mut self, status: RunStatusDto) -> io::Result<()>;
}

/// The blocking client driver of the line front ends.
pub struct Driver {
    client: IntentionClient,
    endpoint: LocalEndpoint,
    workspace_root: WorkspaceRootDto,
    mode: RunModeDto,
    state: AppState,
    subscription: Option<RunStreamSubscription>,
    last_failure: Option<ErrorDto>,
    reported_rows: usize,
    reported_status: Option<RunStatusDto>,
}

impl Driver {
    /// Creates a driver over one client with an empty application state.
    #[must_use]
    pub fn new(
        client: IntentionClient,
        endpoint: LocalEndpoint,
        workspace_root: WorkspaceRootDto,
        mode: RunModeDto,
    ) -> Self {
        Self {
            client,
            endpoint,
            workspace_root: workspace_root.clone(),
            mode,
            state: AppState::new(None).with_workspace_root(workspace_root),
            subscription: None,
            last_failure: None,
            reported_rows: 0,
            reported_status: None,
        }
    }

    /// Returns the application state the driver keeps current.
    #[must_use]
    pub const fn state(&self) -> &AppState {
        &self.state
    }

    /// Returns the last typed failure a performed effect reported, if any.
    #[must_use]
    pub const fn last_failure(&self) -> Option<&ErrorDto> {
        self.last_failure.as_ref()
    }

    /// Returns the failure a session that did not open reports.
    #[must_use]
    pub fn session_failure(&self) -> ErrorDto {
        // @todo(hack): the front end composes protocol errors for states the
        // core never reports - `session_not_open` here, `no_sessions_to_continue`
        // in `no_sessions_error`, and `run_stream_closed` in `pump_failure`;
        // the client should carry these conditions as typed failures.
        self.last_failure
            .clone()
            .unwrap_or_else(|| ErrorDto::validation("session_not_open", "no session is open"))
    }

    /// Connects or bootstraps the shared client.
    ///
    /// # Errors
    ///
    /// Returns the typed client failure when the daemon cannot be reached.
    pub async fn connect(&self) -> Result<(), ErrorDto> {
        self.client.connect_or_bootstrap().await.map(|_health| ())
    }

    /// Opens the session a front end starts from.
    ///
    /// A selected session is read by its own snapshot. With no selection, no
    /// session is opened unless the caller explicitly asked to continue: the
    /// core's bootstrap path then reads the session list and opens the newest
    /// session the daemon reports, and a launch that asked for neither opens
    /// nothing.
    ///
    /// # Errors
    ///
    /// Returns the typed client failure when the daemon cannot be reached.
    pub async fn select(
        &mut self,
        session: Option<SessionId>,
        continue_session: bool,
    ) -> Result<(), ErrorDto> {
        let health = self.client.connect_or_bootstrap().await?;
        match session {
            Some(session) => self.show(session).await,
            None => {
                self.reset(None, continue_session);
                self.apply(Action::Bootstrapped(health)).await;
            }
        }
        self.reported_rows = self.state.transcript().len();
        Ok(())
    }

    /// Reads one known session's current snapshot into the core.
    pub async fn show(&mut self, session: SessionId) {
        self.reset(Some(session), false);
        if let Some(action) = self.perform(Effect::OpenSession(session)).await {
            self.apply(action).await;
        }
        self.reported_rows = self.state.transcript().len();
    }

    /// Creates a new session from the driver's workspace and mode through the core.
    pub async fn create(&mut self) {
        self.reset(None, false);
        self.apply(Action::NewSessionRequested).await;
        self.reported_rows = self.state.transcript().len();
    }

    /// Returns the most recently updated session the daemon reports, if any.
    ///
    /// # Errors
    ///
    /// Returns the typed failure when the session list cannot be read.
    pub async fn most_recent_session(&self) -> Result<Option<SessionId>, ErrorDto> {
        self.client.most_recent_session().await
    }

    /// Sends one prompt as a user turn through the core.
    // @todo(hack): the prompt is replayed as input characters so this driver can
    // reuse the input path; the core should expose one typed turn submission so
    // a scripted prompt is not simulated typing.
    pub async fn send_turn(&mut self, content: &str) {
        self.last_failure = None;
        for character in content.chars() {
            self.state.update(Action::InputChar(character));
        }
        self.apply(Action::InputSubmitted).await;
    }

    /// Streams the live run until it ends or the deadline passes.
    ///
    /// A passed deadline interrupts the active run, waits the bounded grace, and
    /// reports the timeout.
    ///
    /// # Errors
    ///
    /// Returns the typed failure of a run whose stream cannot be recovered and
    /// the output failure of the reporting sink.
    pub async fn pump(
        &mut self,
        events: &mut impl Report,
        deadline: Option<Deadline>,
    ) -> Result<RunOutcome, PumpFailure> {
        if let Some(outcome) = self.pump_until(events, deadline).await? {
            return Ok(outcome);
        }
        self.apply(Action::InterruptRequested).await;
        let grace = Deadline::after(Some(INTERRUPT_GRACE), Instant::now());
        let _observed = self.pump_until(events, grace).await;
        Ok(RunOutcome::TimedOut)
    }

    /// Reads run frames until the run ends or the wait deadline passes.
    async fn pump_until(
        &mut self,
        events: &mut impl Report,
        deadline: Option<Deadline>,
    ) -> Result<Option<RunOutcome>, PumpFailure> {
        // @todo(hack): a closed or empty stream is recoverable exactly once; the
        // budget and the decision to fail after it live here because the run
        // stream has no resume of its own. Kept as is by owner decision.
        let mut reconnects = 0;
        loop {
            self.report_pending(events)?;
            if let Some(outcome) = terminal_outcome(self.state.run_status()) {
                return Ok(Some(outcome));
            }
            match self.next(deadline).await {
                Received::Frame(frame) => {
                    if let RunStreamFrameDto::TextDelta(delta) = &frame {
                        events
                            .delta(delta.step(), delta.channel(), delta.text())
                            .map_err(PumpFailure::Output)?;
                    }
                    self.apply(Action::FrameReceived(frame)).await;
                }
                Received::Expired => return Ok(None),
                Received::Empty | Received::Closed => {
                    if reconnects > 0 {
                        return Err(self.pump_failure());
                    }
                    reconnects += 1;
                    self.apply(Action::ReconnectRequested).await;
                }
            }
        }
    }

    /// Waits for the next observation of the live run subscription.
    async fn next(&mut self, deadline: Option<Deadline>) -> Received {
        let Some(subscription) = self.subscription.as_mut() else {
            return Received::Empty;
        };
        let received = match deadline.map(|deadline| deadline.remaining(Instant::now())) {
            Some(Some(wait)) => match tokio::time::timeout(wait, subscription.receive()).await {
                Ok(received) => received,
                Err(_expired) => return Received::Expired,
            },
            Some(None) => return Received::Expired,
            None => subscription.receive().await,
        };
        match received {
            Ok(Some(frame)) => Received::Frame(frame),
            Ok(None) => {
                self.subscription = None;
                self.apply(Action::RunStreamEnded).await;
                Received::Closed
            }
            Err(error) => {
                self.subscription = None;
                self.record_failure(&error);
                self.apply(Action::RunStreamFailed(error)).await;
                Received::Closed
            }
        }
    }

    /// Reports the rows and the status the core accepted since the last report.
    fn report_pending(&mut self, events: &mut impl Report) -> Result<(), PumpFailure> {
        let rows = self.state.transcript();
        let start = self.reported_rows.min(rows.len());
        for row in &rows[start..] {
            events.row(row).map_err(PumpFailure::Output)?;
        }
        self.reported_rows = rows.len();
        let status = self.state.run_status();
        if status != self.reported_status {
            self.reported_status = status;
            if let Some(status) = status {
                events.status(status).map_err(PumpFailure::Output)?;
            }
        }
        Ok(())
    }

    /// Applies one action and performs the effects it produces, in order.
    async fn apply(&mut self, action: Action) {
        let mut pending = VecDeque::from([action]);
        while let Some(action) = pending.pop_front() {
            for effect in self.state.update(action) {
                if let Some(reported) = self.perform(effect).await {
                    pending.push_back(reported);
                }
            }
        }
    }

    /// Performs one effect and returns the action it reports, if any.
    ///
    /// Every request effect goes through the shared [`client_task`] mapper. A
    /// failure never escapes: the driver keeps the typed error for its process
    /// status and reports the core's own failure action, so every state
    /// transition stays total.
    async fn perform(&mut self, effect: Effect) -> Option<Action> {
        match effect {
            Effect::Subscribe { session_id, run_id } => {
                Some(self.open_stream_action(session_id, run_id).await)
            }
            Effect::CloseStream => {
                // This driver owns its one live subscription, so the close
                // effect drops it here instead of reaching the client.
                self.subscription = None;
                None
            }
            effect => {
                match client_task::perform(&self.client, &self.workspace_root, self.mode, effect)
                    .await?
                {
                    Ok(action) => Some(action),
                    Err((action, error)) => {
                        self.record_failure(&error);
                        Some(action)
                    }
                }
            }
        }
    }

    /// Opens one run subscription and returns the core's action for its outcome.
    async fn open_stream_action(&mut self, session_id: SessionId, run_id: RunId) -> Action {
        match client_task::open_subscription(&self.endpoint, session_id, run_id).await {
            Ok(subscription) => {
                let state = subscription.state().clone();
                self.subscription = Some(subscription);
                Action::RunStreamOpened(state)
            }
            Err(error) => {
                self.record_failure(&error);
                Action::RunStreamFailed(error)
            }
        }
    }

    /// Returns the typed failure a run that cannot be observed reports.
    fn pump_failure(&self) -> PumpFailure {
        PumpFailure::Client(self.last_failure.clone().unwrap_or_else(|| {
            ErrorDto::unavailable(
                "run_stream_closed",
                "the run stream closed before the run reported a terminal status",
            )
        }))
    }

    /// Keeps one typed failure for the caller's process status.
    fn record_failure(&mut self, error: &ErrorDto) {
        self.last_failure = Some(error.clone());
    }

    /// Drops every session-scoped value a new selection replaces.
    fn reset(&mut self, session: Option<SessionId>, continue_session: bool) {
        self.state = AppState::new(session)
            .continuing(continue_session)
            .with_workspace_root(self.workspace_root.clone());
        self.subscription = None;
        self.last_failure = None;
        self.reported_rows = 0;
        self.reported_status = None;
    }
}

/// The output sink of the headless command: text or NDJSON records.
enum Output<'a, W: Write> {
    /// Progressive human output on standard output.
    Text(TextReport<&'a mut W>),
    /// Newline-delimited typed JSON records on standard output.
    Json(JsonReport<&'a mut W>),
}

impl<'a, W: Write> Output<'a, W> {
    /// Creates the sink one output format writes to.
    const fn new(format: Format, out: &'a mut W) -> Self {
        match format {
            Format::Text => Self::Text(TextReport::new(out)),
            Format::Json => Self::Json(JsonReport::new(out)),
        }
    }

    /// Writes one safe typed failure in the selected format.
    fn error(&mut self, err: &mut impl Write, error: &ErrorDto) -> io::Result<()> {
        match self {
            Self::Text(_report) => {
                writeln!(err, "error: {} ({})", error.message(), error.code())
            }
            Self::Json(report) => report.error(error),
        }
    }

    /// Writes the final status of one driven run in the selected format.
    fn finish(
        &mut self,
        outcome: RunOutcome,
        session: Option<SessionId>,
        run: Option<RunId>,
    ) -> io::Result<()> {
        match self {
            Self::Text(report) => report.finish(outcome),
            Self::Json(report) => report.finish(outcome, session, run),
        }
    }
}

impl<W: Write> Report for Output<'_, W> {
    fn delta(&mut self, step: u32, channel: TextDeltaChannelDto, text: &str) -> io::Result<()> {
        match self {
            Self::Text(report) => report.delta(step, channel, text),
            Self::Json(report) => report.delta(step, channel, text),
        }
    }

    fn row(&mut self, row: &MessageProjectionDto) -> io::Result<()> {
        match self {
            Self::Text(report) => report.row(row),
            Self::Json(report) => report.row(row),
        }
    }

    fn status(&mut self, status: RunStatusDto) -> io::Result<()> {
        match self {
            Self::Text(report) => report.status(status),
            Self::Json(report) => report.status(status),
        }
    }
}

/// The progressive human output of one run.
///
/// The streamed provisional answer text is written as it arrives. A committed
/// assistant row that repeats the streamed text of its step supersedes that
/// text instead of printing it twice, which is the committed-row rule of the
/// terminal model applied to a pipe.
///
/// The reasoning channel is not written: this format reports the transcript's
/// prose, and a committed row's reasoning is never printed either, so a step's
/// pipe output stays exactly the answer it commits. The machine-readable format
/// carries both channels for a consumer that wants them.
pub struct TextReport<W> {
    writer: W,
    step: Option<u32>,
    streamed: String,
    at_line_start: bool,
}

impl<W: Write> TextReport<W> {
    /// Creates a report writing to `writer`.
    pub const fn new(writer: W) -> Self {
        Self {
            writer,
            step: None,
            streamed: String::new(),
            at_line_start: true,
        }
    }

    /// Writes the final status line of one run.
    ///
    /// # Errors
    ///
    /// Returns the output failure.
    pub fn finish(&mut self, outcome: RunOutcome) -> io::Result<()> {
        self.end_line()?;
        writeln!(self.writer, "run {}", outcome.describe())
    }

    /// Ends the current line when the streamed text left one open.
    fn end_line(&mut self) -> io::Result<()> {
        if self.at_line_start {
            return Ok(());
        }
        writeln!(self.writer)?;
        self.at_line_start = true;
        Ok(())
    }
}

impl<W: Write> Report for TextReport<W> {
    fn delta(&mut self, step: u32, channel: TextDeltaChannelDto, text: &str) -> io::Result<()> {
        if channel == TextDeltaChannelDto::Reasoning {
            return Ok(());
        }
        if self.step != Some(step) {
            self.step = Some(step);
            self.streamed.clear();
        }
        self.streamed.push_str(text);
        self.writer.write_all(text.as_bytes())?;
        self.writer.flush()?;
        self.at_line_start = text.ends_with('\n');
        Ok(())
    }

    fn row(&mut self, row: &MessageProjectionDto) -> io::Result<()> {
        if row.kind() == MessageKindDto::User {
            // The caller supplied the prompt, so repeating it as a transcript
            // row would echo the input instead of reporting the run.
            return Ok(());
        }
        // @todo(hack): streamed text is matched to its committed row by string
        // equality because a delta names only its model step, never the
        // committed row it will become, so a committed row that repeats the
        // streamed text for another reason is suppressed too; the client should
        // mark the row a stream supersedes.
        let supersedes = row.kind() == MessageKindDto::Assistant
            && !self.streamed.is_empty()
            && row.text() == self.streamed;
        self.streamed.clear();
        self.end_line()?;
        if supersedes {
            return Ok(());
        }
        writeln!(self.writer, "{}: {}", row.kind().as_str(), row.text())
    }

    fn status(&mut self, _status: RunStatusDto) -> io::Result<()> {
        // Text output carries the final status after the run; intermediate
        // transitions are status-bar information, not transcript prose.
        Ok(())
    }
}

/// One NDJSON record of the headless command.
///
/// The record kinds are the command's closed script interface: `delta`,
/// `message`, `status`, `error`, and `result`. A delta record marks itself
/// transient and names its channel, so a consumer can tell provisional text of
/// either channel from a committed row.
enum Record<'a> {
    /// One transient provisional model text chunk.
    Delta {
        /// The zero-based model step the chunk belongs to.
        step: u32,
        /// The step's text channel the chunk belongs to.
        channel: &'a str,
        /// The chunk text.
        text: &'a str,
    },
    /// One committed transcript row.
    Message {
        /// The canonical message kind.
        kind: &'a str,
        /// The committed row text.
        text: &'a str,
    },
    /// One observed run lifecycle status.
    Status {
        /// The canonical run status.
        status: &'a str,
    },
    /// One safe typed failure.
    Error {
        /// The stable error code.
        code: &'a str,
        /// The stable error category.
        category: &'a str,
        /// The safe human message.
        message: &'a str,
    },
    /// The final result of the run.
    Result {
        /// The stable outcome representation.
        outcome: &'a str,
        /// The session the run belongs to, when one is open.
        session_id: Option<SessionId>,
        /// The run that was observed, when one was known.
        run_id: Option<RunId>,
    },
}

impl Record<'_> {
    /// Writes this record as one JSON line.
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        let value = match self {
            Self::Delta {
                step,
                channel,
                text,
            } => serde_json::json!({
                "record": "delta",
                "transient": true,
                "step": step,
                "channel": channel,
                "text": text,
            }),
            Self::Message { kind, text } => serde_json::json!({
                "record": "message",
                "kind": kind,
                "text": text,
            }),
            Self::Status { status } => serde_json::json!({
                "record": "status",
                "status": status,
            }),
            Self::Error {
                code,
                category,
                message,
            } => serde_json::json!({
                "record": "error",
                "code": code,
                "category": category,
                "message": message,
            }),
            Self::Result {
                outcome,
                session_id,
                run_id,
            } => serde_json::json!({
                "record": "result",
                "outcome": outcome,
                "session_id": session_id.map(|session| session.to_string()),
                "run_id": run_id.map(|run| run.to_string()),
            }),
        };
        writeln!(writer, "{value}")
    }
}

/// The NDJSON record output of one run.
pub struct JsonReport<W> {
    writer: W,
}

impl<W: Write> JsonReport<W> {
    /// Creates a report writing to `writer`.
    pub const fn new(writer: W) -> Self {
        Self { writer }
    }

    /// Writes one safe typed failure record.
    fn error(&mut self, error: &ErrorDto) -> io::Result<()> {
        Record::Error {
            code: error.code(),
            category: error.category().as_str(),
            message: error.message(),
        }
        .write(&mut self.writer)
    }

    /// Writes the final result record of one run.
    fn finish(
        &mut self,
        outcome: RunOutcome,
        session: Option<SessionId>,
        run: Option<RunId>,
    ) -> io::Result<()> {
        Record::Result {
            outcome: outcome.as_str(),
            session_id: session,
            run_id: run,
        }
        .write(&mut self.writer)
    }
}

impl<W: Write> Report for JsonReport<W> {
    fn delta(&mut self, step: u32, channel: TextDeltaChannelDto, text: &str) -> io::Result<()> {
        Record::Delta {
            step,
            channel: channel.as_str(),
            text,
        }
        .write(&mut self.writer)
    }

    fn row(&mut self, row: &MessageProjectionDto) -> io::Result<()> {
        Record::Message {
            kind: row.kind().as_str(),
            text: row.text(),
        }
        .write(&mut self.writer)
    }

    fn status(&mut self, status: RunStatusDto) -> io::Result<()> {
        Record::Status {
            status: status.as_str(),
        }
        .write(&mut self.writer)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "Format and deadline tests assert written bytes and instants directly."
    )]

    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};

    use intention_proto::MessageId;

    /// Returns one fresh durable row identity for a committed fixture row.
    fn row_id() -> MessageId {
        static NEXT: AtomicI64 = AtomicI64::new(0);
        MessageId::new(NEXT.fetch_add(1, Ordering::Relaxed) + 1)
            .expect("the fixture row identity is positive")
    }

    /// Returns one coherent committed transcript row fixture.
    fn row(kind: MessageKindDto, text: &str) -> MessageProjectionDto {
        MessageProjectionDto::new(
            row_id(),
            SessionId::new(),
            Some(RunId::new()),
            kind,
            text,
            None,
            None,
            None,
        )
        .expect("the fixture row is coherent")
    }

    /// Writes one record and returns the written line.
    fn written(record: Record<'_>) -> String {
        let mut buffer = Vec::new();
        record.write(&mut buffer).expect("the record writes");
        String::from_utf8(buffer).expect("the record is UTF-8 text")
    }

    /// Returns the text one report wrote into `buffer`.
    fn text_of(buffer: &[u8]) -> &str {
        std::str::from_utf8(buffer).expect("the report wrote UTF-8 text")
    }

    #[test]
    fn a_deadline_reports_the_wait_left_before_it_passes() {
        let now = Instant::now();
        let deadline =
            Deadline::after(Some(Duration::from_secs(5)), now).expect("the deadline is bounded");
        assert_eq!(deadline.remaining(now), Some(Duration::from_secs(5)));
        assert_eq!(
            deadline.remaining(now + Duration::from_secs(4)),
            Some(Duration::from_secs(1))
        );
    }

    #[test]
    fn a_passed_deadline_has_no_wait_left() {
        let now = Instant::now();
        let deadline =
            Deadline::after(Some(Duration::from_secs(5)), now).expect("the deadline is bounded");
        assert_eq!(deadline.remaining(now + Duration::from_secs(5)), None);
        assert_eq!(deadline.remaining(now + Duration::from_secs(30)), None);
    }

    #[test]
    fn a_timeout_beyond_the_clock_is_no_bound() {
        let now = Instant::now();
        assert_eq!(Deadline::after(None, now), None);
        assert_eq!(
            Deadline::after(Some(Duration::from_secs(u64::MAX)), now),
            None
        );
    }

    #[test]
    fn only_terminal_run_statuses_are_run_outcomes() {
        assert_eq!(
            terminal_outcome(Some(RunStatusDto::Completed)),
            Some(RunOutcome::Terminal(RunStatusDto::Completed))
        );
        assert_eq!(
            terminal_outcome(Some(RunStatusDto::Failed)),
            Some(RunOutcome::Terminal(RunStatusDto::Failed))
        );
        assert_eq!(
            terminal_outcome(Some(RunStatusDto::Interrupted)),
            Some(RunOutcome::Terminal(RunStatusDto::Interrupted))
        );
        assert_eq!(terminal_outcome(Some(RunStatusDto::Running)), None);
        assert_eq!(terminal_outcome(Some(RunStatusDto::Starting)), None);
        assert_eq!(terminal_outcome(None), None);
    }

    #[test]
    fn run_outcomes_map_to_their_process_status() {
        assert_eq!(
            exit_status(RunOutcome::Terminal(RunStatusDto::Completed)),
            ExitStatus::Completed
        );
        assert_eq!(
            exit_status(RunOutcome::Terminal(RunStatusDto::Failed)),
            ExitStatus::Rejected
        );
        assert_eq!(
            exit_status(RunOutcome::Terminal(RunStatusDto::Interrupted)),
            ExitStatus::Interrupted
        );
        assert_eq!(exit_status(RunOutcome::TimedOut), ExitStatus::Timeout);
        assert_eq!(RunOutcome::TimedOut.as_str(), "timed_out");
        assert_eq!(RunOutcome::TimedOut.describe(), "timed out");
        assert_eq!(
            RunOutcome::Terminal(RunStatusDto::Completed).as_str(),
            "completed"
        );
    }

    #[test]
    fn json_records_carry_the_closed_record_set() {
        assert_eq!(
            written(Record::Delta {
                step: 2,
                channel: "reasoning",
                text: "hi"
            }),
            "{\"channel\":\"reasoning\",\"record\":\"delta\",\"step\":2,\"text\":\"hi\",\"transient\":true}\n"
        );
        assert_eq!(
            written(Record::Message {
                kind: "assistant",
                text: "hi"
            }),
            "{\"kind\":\"assistant\",\"record\":\"message\",\"text\":\"hi\"}\n"
        );
        assert_eq!(
            written(Record::Status { status: "running" }),
            "{\"record\":\"status\",\"status\":\"running\"}\n"
        );
        assert_eq!(
            written(Record::Error {
                code: "invalid_request",
                category: "validation",
                message: "no",
            }),
            "{\"category\":\"validation\",\"code\":\"invalid_request\",\"message\":\"no\",\"record\":\"error\"}\n"
        );
        assert_eq!(
            written(Record::Result {
                outcome: "completed",
                session_id: None,
                run_id: None,
            }),
            "{\"outcome\":\"completed\",\"record\":\"result\",\"run_id\":null,\"session_id\":null}\n"
        );
    }

    #[test]
    fn json_record_text_is_escaped() {
        let line = written(Record::Delta {
            step: 0,
            channel: "answer",
            text: "a \"quote\"\n",
        });
        assert_eq!(
            line,
            "{\"channel\":\"answer\",\"record\":\"delta\",\"step\":0,\"text\":\"a \\\"quote\\\"\\n\",\"transient\":true}\n"
        );
    }

    #[test]
    fn a_json_sink_reports_deltas_messages_status_and_the_result() {
        let session = SessionId::new();
        let run = RunId::new();
        let mut buffer = Vec::new();
        let mut sink = Output::new(Format::Json, &mut buffer);
        sink.delta(0, TextDeltaChannelDto::Reasoning, "weighing")
            .expect("the reasoning delta writes");
        sink.delta(0, TextDeltaChannelDto::Answer, "hi")
            .expect("the answer delta writes");
        sink.row(&row(MessageKindDto::Assistant, "hi"))
            .expect("the row writes");
        sink.status(RunStatusDto::Running)
            .expect("the status writes");
        sink.finish(
            RunOutcome::Terminal(RunStatusDto::Completed),
            Some(session),
            Some(run),
        )
        .expect("the result writes");
        let text = text_of(&buffer);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 5);
        assert!(lines[0].contains("\"record\":\"delta\""));
        assert!(lines[0].contains("\"transient\":true"));
        assert!(lines[0].contains("\"channel\":\"reasoning\""));
        assert!(lines[1].contains("\"channel\":\"answer\""));
        assert!(lines[2].contains("\"record\":\"message\""));
        assert!(lines[3].contains("\"record\":\"status\""));
        assert!(lines[4].contains("\"outcome\":\"completed\""));
        assert!(lines[4].contains(&run.to_string()));
        assert!(lines[4].contains(&session.to_string()));
    }

    #[test]
    fn a_text_sink_streams_deltas_and_lets_the_committed_row_supersede_them() {
        let mut buffer = Vec::new();
        let mut report = TextReport::new(&mut buffer);
        report
            .delta(0, TextDeltaChannelDto::Answer, "hello ")
            .expect("the delta writes");
        report
            .delta(0, TextDeltaChannelDto::Answer, "world")
            .expect("the delta writes");
        report
            .row(&row(MessageKindDto::Assistant, "hello world"))
            .expect("the row writes");
        report
            .finish(RunOutcome::Terminal(RunStatusDto::Completed))
            .expect("the status writes");
        assert_eq!(text_of(&buffer), "hello world\nrun completed\n");
    }

    #[test]
    fn a_text_sink_writes_the_answer_channel_and_leaves_the_reasoning_out() {
        let mut buffer = Vec::new();
        let mut report = TextReport::new(&mut buffer);
        report
            .delta(0, TextDeltaChannelDto::Reasoning, "weighing the options")
            .expect("the reasoning delta writes");
        report
            .delta(0, TextDeltaChannelDto::Answer, "the answer")
            .expect("the answer delta writes");
        report
            .row(&row(MessageKindDto::Assistant, "the answer"))
            .expect("the row writes");
        assert_eq!(
            text_of(&buffer),
            "the answer\n",
            "the pipe reports the answer the row carries, never the reasoning channel"
        );
    }

    #[test]
    fn a_text_sink_prints_a_committed_row_the_stream_did_not_carry() {
        let mut buffer = Vec::new();
        let mut report = TextReport::new(&mut buffer);
        report
            .delta(0, TextDeltaChannelDto::Answer, "partial")
            .expect("the delta writes");
        report
            .row(&row(MessageKindDto::Assistant, "partial and more"))
            .expect("the row writes");
        report
            .row(&row(MessageKindDto::Notice, "step finished"))
            .expect("the row writes");
        assert_eq!(
            text_of(&buffer),
            "partial\nassistant: partial and more\nnotice: step finished\n"
        );
    }

    #[test]
    fn a_text_sink_skips_the_prompt_the_caller_supplied() {
        let mut buffer = Vec::new();
        let mut report = TextReport::new(&mut buffer);
        report
            .row(&row(MessageKindDto::User, "hello"))
            .expect("the row writes");
        report
            .delta(0, TextDeltaChannelDto::Answer, "answer")
            .expect("the delta writes");
        report
            .row(&row(MessageKindDto::Assistant, "answer"))
            .expect("the row writes");
        report
            .finish(RunOutcome::TimedOut)
            .expect("the status writes");
        assert_eq!(text_of(&buffer), "answer\nrun timed out\n");
    }

    #[test]
    fn a_text_sink_starts_a_new_step_with_an_empty_stream() {
        let mut buffer = Vec::new();
        let mut report = TextReport::new(&mut buffer);
        report
            .delta(0, TextDeltaChannelDto::Answer, "first")
            .expect("the delta writes");
        report
            .delta(1, TextDeltaChannelDto::Answer, "second")
            .expect("the delta writes");
        report
            .row(&row(MessageKindDto::Assistant, "second"))
            .expect("the row writes");
        assert_eq!(text_of(&buffer), "firstsecond\n");
    }
}
