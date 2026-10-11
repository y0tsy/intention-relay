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

use intention_client::{
    IntentionClient, SessionStreamClient, SessionStreamSubscription, StreamStep,
};
use intention_proto::{
    ErrorDto, MessageId, MessageKindDto, MessageProjectionDto, RunId, RunModeDto, RunStatusDto,
    SessionFrameDto, SessionId, TextDeltaChannelDto, WorkspaceRootDto, run_status_is_terminal,
};
use intention_transport::LocalEndpoint;
use intention_tui::app::{Action, AppState, Effect};
use intention_tui::client_task;

use crate::cli::{ExitStatus, Format, Options};

/// The bounded wait for the terminal status the deadline's stop commits.
///
/// The stop is the terminal gesture every front end uses, and the committed
/// terminal status frame is its acknowledgement: the grace bounds how long the
/// process waits for that frame before it reports the timeout fallback.
const STOP_GRACE: Duration = Duration::from_secs(2);

/// The SIGINT requests one front end selects on while it waits.
pub type InterruptRequests = tokio::sync::mpsc::UnboundedReceiver<()>;

/// Returns the receiver that yields once per SIGINT the process is sent.
///
/// The signal must not end the process while a front end watches a run: the run
/// would keep working in the daemon for a front end that is gone. Every wait in
/// a front end selects on this receiver instead - a wait that observes a run
/// stops it, and the wait that owns a line loop leaves the loop - so one signal
/// does exactly one thing. The listener lives as long as the last receiver:
/// dropping it ends the listener task.
///
/// # Panics
///
/// Panics outside a Tokio runtime, where the listener cannot be spawned.
#[must_use]
pub fn interrupt_requests() -> InterruptRequests {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while tokio::signal::ctrl_c().await.is_ok() {
            if sender.send(()).is_err() {
                break;
            }
        }
    });
    receiver
}

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
    // The front end resolves the session it opens once, through the client: a
    // selected session is read by its own snapshot, and an explicit continuation
    // resolves the target the daemon's list order names. A launch that asked for
    // neither opens nothing: the prompt creates the session it needs and commits
    // itself as that session's first turn in one command.
    let opened = match options.session() {
        Some(session) => driver.show(session).await,
        None if options.continue_session() => match driver.continue_session().await {
            Ok(session) => driver.show(session).await,
            Err(error) => Err(error),
        },
        None => Ok(()),
    };
    if let Err(error) = opened {
        return client_failure(&mut output, err, &error);
    }

    let deadline = Deadline::after(options.timeout(), Instant::now());
    if let Some(error) = driver.send_turn(prompt).await {
        return client_failure(&mut output, err, &error);
    }
    // The signal waits begin with the run: a SIGINT before the turn reached the
    // daemon keeps its own process default.
    let mut interrupts = interrupt_requests();
    match driver.pump(&mut output, deadline, &mut interrupts).await {
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

/// Returns the outcome of one reported run status, or `None` while it is live.
fn terminal_outcome(status: Option<RunStatusDto>) -> Option<RunOutcome> {
    status
        .filter(|status| run_status_is_terminal(*status))
        .map(RunOutcome::Terminal)
}

/// Returns the outcome one passed deadline ends with.
///
/// The grace waits for the terminal status the stop the deadline issued
/// commits, so the timeout fallback is reported only when the grace observed no
/// status at all.
fn deadline_outcome(observed: Option<RunOutcome>) -> RunOutcome {
    observed.unwrap_or(RunOutcome::TimedOut)
}

/// The terminal end of one driven run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunOutcome {
    /// The run reported one terminal status.
    Terminal(RunStatusDto),
    /// The grace expired with no terminal status observed.
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
#[expect(
    clippy::large_enum_variant,
    reason = "The frame variant carries the wire DTO by value, so the driver hands the decoded frame on without a second allocation; boxing it would add an indirection to the streaming loop the other states never pay for."
)]
#[derive(Debug)]
enum Received {
    /// One committed or transient frame arrived.
    Frame(SessionFrameDto),
    /// The subscription ended or failed; the core holds the reported reason.
    Closed,
    /// No live subscription is open.
    Empty,
    /// The wait deadline passed.
    Expired,
    /// SIGINT reached the wait; the run it observes is stopped by its caller.
    Interrupted,
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

    /// Reports one committed transcript row and the model step it superseded.
    ///
    /// The identity is the client's: `superseded_step` is the step whose
    /// streamed text this row became, so a sink that already wrote that step's
    /// provisional text asks for the identity instead of comparing text, and a
    /// row the client marks as no step at all is never suppressed.
    ///
    /// # Errors
    ///
    /// Returns the output failure.
    fn row(&mut self, row: &MessageProjectionDto, superseded_step: Option<u32>) -> io::Result<()>;

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
    mode: Option<RunModeDto>,
    state: AppState,
    subscription: Option<SessionStreamSubscription>,
    last_failure: Option<ErrorDto>,
    /// The newest committed row this driver has reported, if any.
    ///
    /// Rows are walked by durable identity, never by position: the client's
    /// retention bound can drop a row the driver already reported without
    /// shifting the rows it has not, so an identity is what tells the next
    /// report where the previous one ended.
    reported_row: Option<MessageId>,
    reported_status: Option<RunStatusDto>,
}

impl Driver {
    /// Creates a driver over one client with an empty application state.
    #[must_use]
    pub fn new(
        client: IntentionClient,
        endpoint: LocalEndpoint,
        workspace_root: WorkspaceRootDto,
        mode: Option<RunModeDto>,
    ) -> Self {
        Self {
            client,
            endpoint,
            workspace_root: workspace_root.clone(),
            mode,
            state: AppState::new(None).with_workspace_root(workspace_root),
            subscription: None,
            last_failure: None,
            reported_row: None,
            reported_status: None,
        }
    }

    /// Returns the application state the driver keeps current.
    #[must_use]
    pub const fn state(&self) -> &AppState {
        &self.state
    }

    /// Connects or bootstraps the shared client.
    ///
    /// # Errors
    ///
    /// Returns the typed client failure when the daemon cannot be reached.
    pub async fn connect(&self) -> Result<(), ErrorDto> {
        self.client.connect_or_bootstrap().await.map(|_health| ())
    }

    /// Connects and applies the bootstrap state of a launch that opens no session.
    ///
    /// The daemon's readiness, its session list, and its settings are read, and
    /// no session is opened: the front end shows its welcome state until the
    /// user asks for a session.
    ///
    /// # Errors
    ///
    /// Returns the typed client failure when the daemon cannot be reached.
    pub async fn bootstrap(&mut self) -> Result<(), ErrorDto> {
        let health = self.client.connect_or_bootstrap().await?;
        self.reset(None);
        let _failure = self.apply(Action::Bootstrapped(health)).await;
        Ok(())
    }

    /// Resolves the session a continuation opens.
    ///
    /// The resolution is the client's: the daemon's list order names the target,
    /// and a list with no session is the client's own typed failure, which this
    /// driver reports unchanged.
    ///
    /// # Errors
    ///
    /// Returns `no_sessions_to_continue` when the daemon lists no session, or
    /// the typed failure the session list read reported.
    pub async fn continue_session(&self) -> Result<SessionId, ErrorDto> {
        self.client.continue_session().await
    }

    /// Reads one known session's current snapshot into the core.
    ///
    /// The session is open exactly when this returns `Ok`; its identity is the
    /// state's own, so a caller that needs it reads the state it holds.
    ///
    /// # Errors
    ///
    /// Returns the typed failure the session read reported.
    pub async fn show(&mut self, session: SessionId) -> Result<(), ErrorDto> {
        self.reset(Some(session));
        let outcome = self.perform(Effect::OpenSession(session)).await;
        let failure = match outcome {
            Some((action, failure)) => {
                let (next_failure, _reconnected) = self.apply(action).await;
                failure.or(next_failure)
            }
            None => None,
        };
        self.finish_open(failure)
    }

    /// Creates a new session from the driver's workspace and mode through the core.
    ///
    /// The session is open exactly when this returns `Ok`; its identity is the
    /// state's own, so a caller that needs it reads the state it holds.
    ///
    /// # Errors
    ///
    /// Returns the typed failure the creation or the open of the created session
    /// reported.
    pub async fn create(&mut self) -> Result<(), ErrorDto> {
        self.reset(None);
        let (failure, _reconnected) = self.apply(Action::NewSessionRequested).await;
        self.finish_open(failure)
    }

    /// Records the rows a session read already carries and reports the open outcome.
    ///
    /// A session read re-reads the whole transcript, so the rows it carries are
    /// not new events for a reporter that has not run yet: the walk starts after
    /// the newest row the read carried.
    fn finish_open(&mut self, failure: Option<ErrorDto>) -> Result<(), ErrorDto> {
        self.reported_row = self.state.transcript().last().map(MessageProjectionDto::id);
        failure.map_or(Ok(()), Err)
    }

    /// Sends one prompt as one typed user turn through the core.
    ///
    /// The prompt is data rather than simulated keystrokes, so the interactive
    /// command registry never decides whether it is sent and a trailing
    /// backslash is never a line break. Returns the typed failure the turn
    /// reported, if any.
    pub async fn send_turn(&mut self, content: &str) -> Option<ErrorDto> {
        self.last_failure = None;
        self.apply(Action::TurnSubmitted {
            content: content.to_owned(),
        })
        .await;
        self.last_failure.clone()
    }

    /// Stops the run this driver observes as live.
    ///
    /// Returns whether a run was live to stop. A signal that reaches a driver
    /// with no live run asks for nothing and changes nothing, so the waited-for
    /// turn keeps streaming.
    pub async fn request_stop(&mut self) -> bool {
        if self.state.live_run().is_none() {
            return false;
        }
        self.apply(Action::CancelRunRequested).await;
        true
    }

    /// Streams the live run until it ends or the deadline passes.
    ///
    /// A passed deadline stops the run the driver observes with the same
    /// command every stop gesture uses, waits the bounded grace for the
    /// terminal status that stop commits, and reports the status it observed.
    /// SIGINT stops the run the driver observes instead of ending the process:
    /// the loop keeps reading until the daemon reports the stop it accepted, so
    /// no front end leaves a run working unwatched.
    ///
    /// # Errors
    ///
    /// Returns the typed failure of a run whose stream cannot be recovered and
    /// the output failure of the reporting sink.
    pub async fn pump(
        &mut self,
        events: &mut impl Report,
        deadline: Option<Deadline>,
        interrupts: &mut InterruptRequests,
    ) -> Result<RunOutcome, PumpFailure> {
        if let Some(outcome) = self.pump_until(events, deadline, interrupts).await? {
            return Ok(outcome);
        }
        self.apply(Action::CancelRunRequested).await;
        let grace = Deadline::after(Some(STOP_GRACE), Instant::now());
        let observed = self.pump_until(events, grace, interrupts).await?;
        Ok(deadline_outcome(observed))
    }

    /// Reads run frames until the run ends or the wait deadline passes.
    async fn pump_until(
        &mut self,
        events: &mut impl Report,
        deadline: Option<Deadline>,
        interrupts: &mut InterruptRequests,
    ) -> Result<Option<RunOutcome>, PumpFailure> {
        loop {
            self.report_pending(events)?;
            if let Some(outcome) = terminal_outcome(self.state.run_status()) {
                return Ok(Some(outcome));
            }
            match self.next(deadline, interrupts).await {
                Received::Frame(frame) => {
                    if let SessionFrameDto::TextDelta(delta) = &frame {
                        events
                            .delta(delta.step(), delta.channel(), delta.text())
                            .map_err(PumpFailure::Output)?;
                    }
                    self.apply(Action::FrameReceived(frame)).await;
                }
                Received::Interrupted => {
                    // The signal reaches the loop that owns the state, which
                    // stops the run it observes and keeps reading until the run
                    // reports its own terminal status.
                    let _stopped = self.request_stop().await;
                }
                Received::Expired => return Ok(None),
                Received::Empty | Received::Closed => {
                    // The front end owns the one reconnect a lost stream is
                    // owed: it asks for it while its budget lasts, and reports
                    // its typed failure once the budget is spent.
                    let (_failure, reconnected) = self.apply(Action::ReconnectRequested).await;
                    if !reconnected {
                        return Err(self.pump_failure());
                    }
                }
            }
        }
    }

    /// Waits for the next observation of the live run subscription.
    ///
    /// A SIGINT resolves as [`Received::Interrupted`] instead of ending the
    /// process. A frame the subscription already holds is always taken first,
    /// so a signal never discards state the run reported.
    async fn next(
        &mut self,
        deadline: Option<Deadline>,
        interrupts: &mut InterruptRequests,
    ) -> Received {
        let Some(subscription) = self.subscription.as_mut() else {
            return Received::Empty;
        };
        let received = match deadline.map(|deadline| deadline.remaining(Instant::now())) {
            Some(None) => return Received::Expired,
            Some(Some(wait)) => tokio::select! {
                biased;
                received = subscription.receive() => received,
                _ = interrupts.recv() => return Received::Interrupted,
                () = tokio::time::sleep(wait) => return Received::Expired,
            },
            None => tokio::select! {
                biased;
                received = subscription.receive() => received,
                _ = interrupts.recv() => return Received::Interrupted,
            },
        };
        match received {
            Ok(StreamStep::Frame(frame)) => Received::Frame(frame),
            Ok(StreamStep::Closed(error)) => {
                // The client classified the closed connection: the front end
                // ends the observation, and the classification is the failure a
                // run that never reached a terminal status reports.
                self.subscription = None;
                self.record_failure(&error);
                self.apply(Action::SessionStreamEnded).await;
                Received::Closed
            }
            Err(error) => {
                self.subscription = None;
                self.record_failure(&error);
                self.apply(Action::SessionStreamFailed(error)).await;
                Received::Closed
            }
        }
    }

    /// Reports the rows and the status the core accepted since the last report.
    ///
    /// The row walk is by durable identity: the client's retention bound can
    /// drop a row this reporter already reported without moving the rows it has
    /// not, so every row committed past the bound is still reported exactly
    /// once. Every row carries the supersession identity the client holds for
    /// it, so a sink decides what to print from that identity and never from
    /// text.
    fn report_pending(&mut self, events: &mut impl Report) -> Result<(), PumpFailure> {
        for row in self.state.transcript_after(self.reported_row) {
            events
                .row(row, self.state.superseded_step(row.id()))
                .map_err(PumpFailure::Output)?;
            self.reported_row = Some(row.id());
        }
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
    ///
    /// Returns the typed failure the chain reported and whether the chain asked
    /// the client to re-read current state, so a caller that owns a run's wait
    /// can tell a reconnect request from a budget that is already spent.
    async fn apply(&mut self, action: Action) -> (Option<ErrorDto>, bool) {
        let mut failure = None;
        let mut reconnected = false;
        let mut pending = VecDeque::from([action]);
        while let Some(action) = pending.pop_front() {
            for effect in self.state.update(action) {
                if matches!(effect, Effect::Connect) {
                    reconnected = true;
                }
                if let Some((reported, error)) = self.perform(effect).await {
                    if error.is_some() {
                        failure = error;
                    }
                    pending.push_back(reported);
                }
            }
        }
        (failure, reconnected)
    }

    /// Performs one effect and returns the action it reports with the typed
    /// failure the client call observed, if any.
    ///
    /// Every request effect goes through the shared [`client_task`] mapper. A
    /// failure never escapes: the driver keeps the typed error for its process
    /// status and reports the core's own failure action, so every state
    /// transition stays total.
    async fn perform(&mut self, effect: Effect) -> Option<(Action, Option<ErrorDto>)> {
        match effect {
            Effect::Subscribe { session_id } => {
                Some((self.open_stream_action(session_id).await, None))
            }
            effect => {
                match client_task::perform(&self.client, &self.workspace_root, self.mode, effect)
                    .await?
                {
                    Ok(action) => Some((action, None)),
                    Err((action, error)) => {
                        self.record_failure(&error);
                        Some((action, Some(error)))
                    }
                }
            }
        }
    }

    /// Opens one session subscription and returns the core's action for it.
    async fn open_stream_action(&mut self, session_id: SessionId) -> Action {
        match client_task::open_session_subscription(&self.endpoint, session_id).await {
            Ok(subscription) => {
                let state = subscription.state().clone();
                self.subscription = Some(subscription);
                Action::SessionStreamOpened(state)
            }
            Err(error) => {
                self.record_failure(&error);
                Action::SessionStreamFailed(error)
            }
        }
    }

    /// Returns the typed failure a run that cannot be observed reports.
    ///
    /// The failure is the one a stream loss or a failed recovery reported; a
    /// recovery that spent its budget with no reported failure is the client's
    /// own classification of a stream that is gone, so the adapter reports that
    /// value instead of inventing a code of its own.
    fn pump_failure(&self) -> PumpFailure {
        PumpFailure::Client(
            self.last_failure
                .clone()
                .unwrap_or_else(SessionStreamClient::closed_stream_failure),
        )
    }

    /// Keeps one typed failure for the caller's process status.
    fn record_failure(&mut self, error: &ErrorDto) {
        self.last_failure = Some(error.clone());
    }

    /// Drops every session-scoped value a new selection replaces.
    fn reset(&mut self, session: Option<SessionId>) {
        self.state = AppState::new(session).with_workspace_root(self.workspace_root.clone());
        self.subscription = None;
        self.last_failure = None;
        self.reported_row = None;
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
            Self::Text(_report) => writeln!(err, "error: {error}"),
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

    fn row(&mut self, row: &MessageProjectionDto, superseded_step: Option<u32>) -> io::Result<()> {
        match self {
            Self::Text(report) => report.row(row, superseded_step),
            Self::Json(report) => report.row(row, superseded_step),
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
/// assistant row the client marks as the text of the step this sink streamed
/// supersedes that text instead of printing it twice, which is the
/// committed-row rule of the terminal model applied to a pipe: the sink keeps
/// exactly one entry - the last step it streamed - and never matches text.
///
/// The reasoning channel is not written: this format reports the transcript's
/// prose, and a committed row's reasoning is never printed either, so a step's
/// pipe output stays exactly the answer it commits. The machine-readable format
/// carries both channels for a consumer that wants them.
pub struct TextReport<W> {
    writer: W,
    /// The last model step this sink streamed provisional answer text for.
    ///
    /// It is the one entry the supersession rule needs: a step's committed row
    /// precedes the next step's deltas, so no other step can be ahead of the
    /// sink's streamed text.
    streamed_step: Option<u32>,
    at_line_start: bool,
}

impl<W: Write> TextReport<W> {
    /// Creates a report writing to `writer`.
    pub const fn new(writer: W) -> Self {
        Self {
            writer,
            streamed_step: None,
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
        // The sink keeps one entry: the step whose text it last streamed, which
        // is the only step a committed row can still supersede.
        self.streamed_step = Some(step);
        self.writer.write_all(text.as_bytes())?;
        self.writer.flush()?;
        self.at_line_start = text.ends_with('\n');
        Ok(())
    }

    fn row(&mut self, row: &MessageProjectionDto, superseded_step: Option<u32>) -> io::Result<()> {
        if row.kind() == MessageKindDto::User {
            // The caller supplied the prompt, so repeating it as a transcript
            // row would echo the input instead of reporting the run.
            return Ok(());
        }
        // Only the step this sink streamed is suppressed, and only when the
        // client marks this row as that step's committed text: a row the client
        // does not mark - because it became another step, or because the
        // client's window dropped the identity - prints in full, whatever its
        // text happens to repeat.
        let supersedes = superseded_step.is_some_and(|step| Some(step) == self.streamed_step);
        self.end_line()?;
        if supersedes {
            return Ok(());
        }
        // A reasoning-only assistant row carries no text output: the report
        // never writes the reasoning channel, so the row prints nothing
        // instead of an empty assistant line.
        if row.kind() == MessageKindDto::Assistant && row.text().trim().is_empty() {
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

    fn row(&mut self, row: &MessageProjectionDto, _superseded_step: Option<u32>) -> io::Result<()> {
        // The machine format reports every committed row and every transient
        // delta by design, so a supersession suppresses nothing here.
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

    use intention_client::{
        ProcessDaemonLauncher, RETAINED_TRANSCRIPT_MESSAGES, SessionStreamState,
    };
    use intention_proto::MessageId;
    use intention_test_support::daemon_process;

    /// Returns one fresh durable row identity for a committed fixture row.
    fn row_id() -> MessageId {
        static NEXT: AtomicI64 = AtomicI64::new(0);
        MessageId::new(NEXT.fetch_add(1, Ordering::Relaxed) + 1)
            .expect("the fixture row identity is positive")
    }

    /// Returns one committed fixture row of `session_id` and `run_id`.
    fn committed_row(session_id: SessionId, run_id: RunId, text: &str) -> MessageProjectionDto {
        MessageProjectionDto::new(
            row_id(),
            session_id,
            Some(run_id),
            MessageKindDto::Notice,
            text,
            None,
            None,
        )
        .expect("the fixture row is coherent")
    }

    /// Returns one application state whose client holds `count` committed rows
    /// of one session, with that session's identity.
    fn committed_state(count: usize) -> (AppState, SessionId) {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let mut state = AppState::new(None);
        state.update(Action::SessionStreamOpened(SessionStreamState::new(
            session_id,
        )));
        for index in 0..count {
            state.update(Action::FrameReceived(SessionFrameDto::Content(
                committed_row(session_id, run_id, &format!("row {index}")),
            )));
        }
        (state, session_id)
    }

    #[test]
    fn every_row_newer_than_the_last_reported_id_is_reported_once() {
        // The reporter walks the client's rows by durable identity, so a run
        // that commits more rows than the client retains still reports every
        // one of them: the row committed past the bound is the next reported
        // row.
        let (state, _session) = committed_state(RETAINED_TRANSCRIPT_MESSAGES + 2);
        let rows = state.transcript();
        assert_eq!(
            rows.len(),
            RETAINED_TRANSCRIPT_MESSAGES,
            "the client's window is bounded"
        );
        let reported = rows
            .first()
            .expect("the bound drained the oldest rows")
            .id();
        let pending = state.transcript_after(Some(reported));
        assert_eq!(
            pending.len(),
            rows.len() - 1,
            "every retained row newer than the reported identity is pending"
        );
        assert!(pending.iter().all(|row| row.id() > reported));
        assert_eq!(
            pending.last().expect("a row is pending"),
            rows.last().expect("a row is retained"),
            "the row committed past the bound is reported"
        );
    }

    #[test]
    fn a_reported_row_that_was_drained_never_blocks_the_next_report() {
        // The reporter's memory is a durable identity, never a position: a
        // reported row the retention bound has since dropped still lets every
        // later row through.
        let (mut state, session_id) = committed_state(RETAINED_TRANSCRIPT_MESSAGES);
        let reported = state.transcript().first().expect("rows remain").id();
        let run_id = RunId::new();
        let newest = committed_row(session_id, run_id, "the newest row");
        state.update(Action::FrameReceived(SessionFrameDto::Content(
            newest.clone(),
        )));
        assert!(
            state.transcript().iter().all(|row| row.id() != reported),
            "the bound drained the reported row"
        );
        let pending = state.transcript_after(Some(reported));
        assert_eq!(
            pending.len(),
            RETAINED_TRANSCRIPT_MESSAGES,
            "every retained row is newer than the drained reported identity"
        );
        assert_eq!(
            pending.last().expect("a row is pending"),
            &newest,
            "the row committed past the bound is reported"
        );
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
        )
        .expect("the fixture row is coherent")
    }

    /// Returns one coherent committed assistant row fixture carrying
    /// `reasoning` above `text`.
    fn reasoning_row(text: &str, reasoning: &str) -> MessageProjectionDto {
        MessageProjectionDto::new(
            row_id(),
            SessionId::new(),
            Some(RunId::new()),
            MessageKindDto::Assistant,
            text,
            Some(reasoning.to_owned()),
            None,
        )
        .expect("the fixture reasoning row is coherent")
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
    fn a_composed_protocol_error_is_never_reported() {
        // A stream that is gone with nothing reported is the client's own
        // classification, and a recorded failure is reported exactly as the
        // client returned it: the driver invents no code of its own for either.
        let mut driver = offline_driver();
        let PumpFailure::Client(closed) = driver.pump_failure() else {
            unreachable!("a stream loss is a typed client failure")
        };
        assert_eq!(closed, SessionStreamClient::closed_stream_failure());
        assert_eq!(closed.code(), "local_daemon_connection_unavailable");

        let reported = ErrorDto::validation("storage_record_not_found", "no such session");
        driver.record_failure(&reported);
        let PumpFailure::Client(failure) = driver.pump_failure() else {
            unreachable!("a recorded failure is a typed client failure")
        };
        assert_eq!(
            failure, reported,
            "the recorded failure is reported as it was"
        );
    }

    /// Returns a driver that holds no live subscription.
    fn offline_driver() -> Driver {
        let endpoint = daemon_process::unique_endpoint("w1b-composed");
        let launcher = ProcessDaemonLauncher::new("/nonexistent/intention-daemon")
            .expect("the fixture program is a valid launcher program");
        let client = IntentionClient::new(endpoint.clone(), Box::new(launcher));
        Driver::new(
            client,
            endpoint,
            WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy().into_owned())
                .expect("the temporary directory is an absolute workspace root"),
            Some(RunModeDto::Build),
        )
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
    fn a_deadline_without_a_terminal_status_reports_timed_out() {
        assert_eq!(
            deadline_outcome(None),
            RunOutcome::TimedOut,
            "the grace waits for the terminal status the deadline's stop commits, so a grace that observes none is the fallback"
        );
        assert_eq!(
            exit_status(deadline_outcome(None)),
            ExitStatus::Timeout,
            "the fallback is the process's own timeout status"
        );
        assert_eq!(
            deadline_outcome(Some(RunOutcome::Terminal(RunStatusDto::Interrupted))),
            RunOutcome::Terminal(RunStatusDto::Interrupted),
            "the terminal status the grace observed is reported instead of the timeout"
        );
    }

    #[test]
    fn the_one_line_error_form_is_the_types_own_display() {
        let error = ErrorDto::validation("fixture_failure", "the fixture step failed");
        let line = error.to_string();

        // The application's error line is the type's own one-line form.
        let mut state = intention_tui::app::AppState::new(None);
        state.update(intention_tui::app::Action::SessionsListFailed(
            error.clone(),
        ));
        assert_eq!(state.error(), Some(line.as_str()));

        // The process surface writes that form after its own label, in the text
        // sink and at startup alike.
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut output = Output::new(Format::Text, &mut out);
        output
            .error(&mut err, &error)
            .expect("the text sink writes the error line");
        assert_eq!(
            String::from_utf8(err).expect("the error line is UTF-8"),
            format!("error: {line}\n")
        );

        let mut startup = Vec::new();
        let _status = crate::startup_failure(&error, &mut startup);
        assert_eq!(
            String::from_utf8(startup).expect("the startup line is UTF-8"),
            format!("error: {line}\n")
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
        sink.row(&row(MessageKindDto::Assistant, "hi"), Some(0))
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
            .row(&row(MessageKindDto::Assistant, "hello world"), Some(0))
            .expect("the row writes");
        report
            .finish(RunOutcome::Terminal(RunStatusDto::Completed))
            .expect("the status writes");
        assert_eq!(text_of(&buffer), "hello world\nrun completed\n");
    }

    #[test]
    fn a_step_whose_committed_row_differs_by_one_character_prints_once() {
        // The identity decides, never the text: the client marked this row as
        // the step this sink streamed, so the answer is already on the pipe.
        let mut buffer = Vec::new();
        let mut report = TextReport::new(&mut buffer);
        report
            .delta(0, TextDeltaChannelDto::Answer, "the answer.")
            .expect("the delta writes");
        report
            .row(&row(MessageKindDto::Assistant, "the answer"), Some(0))
            .expect("the row writes");
        assert_eq!(text_of(&buffer), "the answer.\n");
    }

    #[test]
    fn a_text_sink_suppresses_only_the_step_it_streamed() {
        // The one-entry bound, on the equal-text defect: the row repeats the
        // text this sink streamed but the client marks it as another step, so
        // it is a row of its own and prints.
        let mut buffer = Vec::new();
        let mut report = TextReport::new(&mut buffer);
        report
            .delta(0, TextDeltaChannelDto::Answer, "done")
            .expect("the delta writes");
        report
            .row(&row(MessageKindDto::Assistant, "done"), Some(1))
            .expect("the row writes");
        assert_eq!(text_of(&buffer), "done\nassistant: done\n");
    }

    #[test]
    fn a_committed_row_beyond_the_retained_supersession_window_prints_once() {
        // The client's window dropped the identity with the row it belonged to,
        // so the row arrives unmarked: the sink prints it once, in full, and
        // never guesses from text.
        let mut buffer = Vec::new();
        let mut report = TextReport::new(&mut buffer);
        report
            .delta(0, TextDeltaChannelDto::Answer, "a long streamed answer")
            .expect("the delta writes");
        report
            .row(
                &row(MessageKindDto::Assistant, "a long streamed answer"),
                None,
            )
            .expect("the row writes");
        assert_eq!(
            text_of(&buffer),
            "a long streamed answer\nassistant: a long streamed answer\n"
        );
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
            .row(&row(MessageKindDto::Assistant, "the answer"), Some(0))
            .expect("the row writes");
        assert_eq!(
            text_of(&buffer),
            "the answer\n",
            "the pipe reports the answer the row carries, never the reasoning channel"
        );
    }

    #[test]
    fn a_text_sink_writes_nothing_for_a_reasoning_only_row() {
        let mut buffer = Vec::new();
        let mut report = TextReport::new(&mut buffer);
        report
            .delta(0, TextDeltaChannelDto::Reasoning, "weighing the call")
            .expect("the reasoning delta writes");
        report
            .row(&reasoning_row("", "weighing the call"), Some(0))
            .expect("the row writes");
        report
            .row(&row(MessageKindDto::Notice, "step finished"), None)
            .expect("the row writes");
        assert_eq!(
            text_of(&buffer),
            "notice: step finished\n",
            "a step without an answer prints no assistant line of its own"
        );
    }

    #[test]
    fn a_text_sink_prints_a_row_the_client_did_not_mark_superseded() {
        // The row the client marks as no step at all is a row of its own: the
        // pipe prints it in full, whatever its text happens to repeat.
        let mut buffer = Vec::new();
        let mut report = TextReport::new(&mut buffer);
        report
            .delta(0, TextDeltaChannelDto::Answer, "partial")
            .expect("the delta writes");
        report
            .row(&row(MessageKindDto::Assistant, "partial and more"), None)
            .expect("the row writes");
        report
            .row(&row(MessageKindDto::Notice, "step finished"), None)
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
            .row(&row(MessageKindDto::User, "hello"), None)
            .expect("the row writes");
        report
            .delta(0, TextDeltaChannelDto::Answer, "answer")
            .expect("the delta writes");
        report
            .row(&row(MessageKindDto::Assistant, "answer"), Some(0))
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
            .row(&row(MessageKindDto::Assistant, "second"), Some(1))
            .expect("the row writes");
        assert_eq!(text_of(&buffer), "firstsecond\n");
    }
}
