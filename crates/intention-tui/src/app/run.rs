//! The run transitions: the live subscription, frames, turns, and interrupts.

use intention_client::{RETAINED_TRANSCRIPT_MESSAGES, SessionStreamState};
use intention_proto::{
    ErrorDto, MessageKindDto, MessageProjectionDto, RunProjectionDto, SendUserTurnOutcomeDto,
    SessionFrameDto, TextDeltaChannelDto, run_status_is_terminal,
};

use super::{AppState, ConnectionStatus, Effect, RunPhase, StreamStatus, short_identifier};

/// How long a tool call may be in flight before the status row names it.
///
/// Half a second: a shorter call leaves the phase the one it found, so a fast
/// tool never flickers the row. The threshold is a subtraction on the elapsed
/// value the front end reported, never a clock read in this clock-free core.
const TOOL_WORKING_AFTER_MILLIS: u64 = 500;

impl AppState {
    /// Mirrors a freshly opened subscription and appends the rows it was missing.
    pub(super) fn apply_session_stream_opened(&mut self, state: SessionStreamState) -> Vec<Effect> {
        // @todo(hack): the app keeps a second committed-transcript projection and answers a fresh
        // subscription by appending the client's set difference at the tail, so the row order the
        // pane shows is the adapter's own and the mirror must happen to be a prefix of the durable
        // order. The client owns reconciliation; the pane should render the rows it reconciled.
        // The subscription snapshot is a fresh read of the run's rows: rows the
        // seeded transcript is missing (they committed after the session read)
        // are appended once, and rows it already carries are not repeated. The
        // client reconciles the two reads by durable row identity.
        let missing = state.missing_rows(&self.transcript);
        self.extend_transcript(missing);
        self.trim_transcript();
        self.active_run = state.run().copied();
        self.session_stream = Some(state);
        self.stream = StreamStatus::Live;
        self.error = None;
        // A fresh subscription carries no streamed text: a live run it opens is
        // waiting for the first token this state will see, and a terminal one
        // carries no phase at all.
        self.rebase_run_phase();
        Vec::new()
    }

    /// Records a subscription that failed to open or stay live.
    ///
    /// The stream is this front end's one live channel, so its loss is not a
    /// dead end: the state clears what the stream justified - the phase and the
    /// tool watch, so [`Self::live_run`] stops claiming a run nobody observes
    /// and the exit and clear arms come back - and asks for one reconnect. The
    /// reconnect re-reads current state through the one resynchronization this
    /// protocol has, and a reconnect that fails reports its own typed error
    /// instead of retrying in a loop.
    pub(super) fn apply_session_stream_failed(&mut self, error: &ErrorDto) -> Vec<Effect> {
        self.session_stream = None;
        self.stream = StreamStatus::Failed;
        // The stream that justified the live phase is gone, so the phase and
        // the tool watch clear with it: the status row never names a working
        // run this front end no longer observes, and [`Self::live_run`] stops
        // claiming one, which is what lets the exit and clear arms through.
        self.phase = None;
        self.reset_tool_watch();
        let mut effects = self.apply_failure(error);
        effects.extend(self.reconnect_once());
        effects
    }

    /// Records a stream the daemon closed.
    ///
    /// A closed stream is the same loss as a failed one - the daemon closed the
    /// connection or the host is leaving - so it clears the same state and asks
    /// for the same one reconnect.
    pub(super) fn apply_session_stream_ended(&mut self) -> Vec<Effect> {
        self.session_stream = None;
        self.stream = StreamStatus::Ended;
        self.phase = None;
        self.reset_tool_watch();
        self.reconnect_once()
    }

    /// Returns the one reconnect this front end asks for after a lost stream.
    ///
    /// The connection reads as connecting while the attempt runs, and the loss
    /// stays visible: a typed failure keeps its error line, and the stream
    /// status keeps naming the loss, so the status row never claims live updates
    /// this front end does not have. A reconnect that fails reports its own
    /// typed error instead of retrying in a loop.
    fn reconnect_once(&mut self) -> Vec<Effect> {
        self.connection = ConnectionStatus::Connecting;
        self.note("reconnecting".to_owned());
        vec![Effect::Connect]
    }

    /// Applies one committed or transient run-stream frame.
    pub(super) fn apply_frame(&mut self, frame: SessionFrameDto) -> Vec<Effect> {
        let Some(stream) = self.session_stream.as_mut() else {
            // A frame for a subscription this front end no longer holds is stale;
            // dropping it keeps the seeded transcript authoritative.
            return Vec::new();
        };
        if let Err(error) = stream.apply_frame(frame.clone()) {
            return self.apply_failure(&error);
        }
        match frame {
            SessionFrameDto::Content(message) => {
                // A content frame repeating the newest accepted row carries the
                // identity of the row the snapshot already carried, so the two
                // are the same committed row.
                // @todo(hack): the newest-accepted-row idempotency rule is re-implemented here on
                // the second projection, although the client applies the same rule to its own rows.
                // Two copies mean a change to the rule reaches only one of them - and the copy the
                // user reads is this one.
                if self.transcript.last().map(MessageProjectionDto::id) != Some(message.id()) {
                    self.push_transcript_row(message);
                    self.trim_transcript();
                }
            }
            SessionFrameDto::Status(run) => {
                // A status of a run this state does not project yet is a run
                // change - it came from another window's turn or from the daemon
                // itself - and the phase belongs to the run, so the previous
                // run's phase and its in-flight tool call never carry over.
                let changed = self
                    .active_run
                    .is_none_or(|current| current.run_id() != run.run_id());
                self.active_run = Some(run);
                if changed {
                    self.rebase_run_phase();
                }
                // A live status keeps the phase the stream already named, and
                // the first status of a run this state has not streamed yet
                // starts as waiting. A terminal status ends the run's phase:
                // the row shows the projection's own terminal word, and no
                // tool call stays in flight past the run it belonged to.
                if run_status_is_terminal(run.status()) {
                    self.phase = None;
                    self.reset_tool_watch();
                } else {
                    self.phase = self.phase.or(Some(RunPhase::Waiting));
                }
                self.note(format!("run {}", run.status().as_str()));
            }
            SessionFrameDto::TextDelta(delta) => {
                // The channel carries every run of the session, so a delta of a
                // run this state does not project is another window's text: it
                // never colours the projected run's phase.
                if self
                    .active_run
                    .is_none_or(|current| current.run_id() != delta.run_id())
                {
                    return Vec::new();
                }
                // The channel names the phase: the reasoning stream thinks and
                // the answer stream answers, and the later channel of a step
                // is the phase the step is in.
                self.phase = Some(match delta.channel() {
                    TextDeltaChannelDto::Reasoning => RunPhase::Thinking,
                    TextDeltaChannelDto::Answer => RunPhase::Answering,
                });
            }
        }
        Vec::new()
    }

    /// Records the turn the daemon accepted and keeps the session stream live.
    ///
    /// The run a started turn opens reaches this state through the session's own
    /// stream, so nothing new is subscribed here: a stream that is already live
    /// carries the run's status, rows, and text, and only a lost one is
    /// re-established.
    pub(super) fn apply_turn_accepted(&mut self, outcome: SendUserTurnOutcomeDto) -> Vec<Effect> {
        self.error = None;
        let Some(_session_id) = self.session_id else {
            return Vec::new();
        };
        match outcome {
            SendUserTurnOutcomeDto::Started { run_id, .. } => {
                self.note(format!("run {} started", short_identifier(run_id)));
                self.ensure_session_stream()
            }
            SendUserTurnOutcomeDto::Pending => {
                self.note("turn queued behind the active run".to_owned());
                self.ensure_session_stream()
            }
        }
    }

    /// Records the elapsed time one front end measured for the current turn.
    ///
    /// The value is the front end's (the core has no clock); the core only keeps
    /// it so the status line stays a pure function of state. A new turn clears
    /// it before the daemon accepts it.
    ///
    /// The same value is what the working threshold measures against: an
    /// in-flight tool call the reported elapsed puts more than half a second
    /// past its committed row names the working phase.
    pub(super) const fn apply_elapsed_reported(&mut self, millis: u64) -> Vec<Effect> {
        self.elapsed_millis = Some(millis);
        if let Some(from) = self.tool_wait_from
            && millis.saturating_sub(from) > TOOL_WORKING_AFTER_MILLIS
        {
            self.tool_working = true;
        }
        Vec::new()
    }

    /// Starts the phase of the run a dispatched turn request begins.
    ///
    /// The front end produced `Effect::SendTurn`: the run the status row now
    /// describes is the one that request starts, so the previous run's phase
    /// and tool watch never describe it.
    pub(super) const fn apply_turn_dispatched(&mut self) {
        self.phase = Some(RunPhase::Waiting);
        self.reset_tool_watch();
    }

    /// Records a turn request that never reached a run.
    ///
    /// The waiting phase belongs to the run the request would have started, so
    /// a failed send ends it; the failure itself becomes the error line.
    pub(super) fn apply_turn_send_failed(&mut self, error: &ErrorDto) -> Vec<Effect> {
        self.phase = None;
        self.apply_failure(error)
    }

    /// Re-bases the live phase on the run the state now mirrors.
    ///
    /// A session snapshot and a fresh subscription both re-read committed
    /// state, so the previous run's phase does not describe what they carry: a
    /// live run starts as waiting for the first token this state will see, and
    /// a terminal one carries no phase at all. The tool watch is re-read from
    /// the rows the state now shows.
    pub(super) fn rebase_run_phase(&mut self) {
        self.reset_tool_watch();
        self.recount_tool_watch();
        let live = self
            .run_status()
            .is_some_and(|status| !run_status_is_terminal(status));
        self.phase = live.then_some(RunPhase::Waiting);
    }

    /// Records that one committed tool call joined the transcript.
    ///
    /// A tool call is in flight from its committed row until the result row
    /// that answers it. The elapsed value the front end last reported is the
    /// baseline the working threshold measures from, so a call that commits
    /// before any report has no baseline and never gets one invented for it.
    // @todo(hack): the watch is incremented and decremented by row kind, while the transcript pane
    // pairs a call with its answer by the call's identity - so an orphan or an out-of-order result
    // cancels a call that is genuinely in flight, and the count spans every run in the transcript.
    // Until the projection carries the open calls, pair the rows by their call id and filter to the
    // live run.
    pub(super) const fn note_tool_call(&mut self) {
        if self.tools_in_flight == 0 {
            self.tool_wait_from = self.elapsed_millis;
        }
        self.tools_in_flight += 1;
    }

    /// Records that one committed tool result answered a call.
    pub(super) const fn note_tool_result(&mut self) {
        self.tools_in_flight = self.tools_in_flight.saturating_sub(1);
        if self.tools_in_flight == 0 {
            self.reset_tool_watch();
        }
    }

    /// Re-reads the in-flight tool calls from the committed transcript.
    pub(super) fn recount_tool_watch(&mut self) {
        let calls = self
            .transcript
            .iter()
            .filter(|row| row.kind() == MessageKindDto::ToolCall)
            .count();
        let results = self
            .transcript
            .iter()
            .filter(|row| row.kind() == MessageKindDto::ToolResult)
            .count();
        // @todo(core): the terminal still decides which tool calls are in flight by subtracting the
        // committed result rows from the committed call rows over the whole transcript. The run's
        // open calls are a run fact the daemon knows while it executes them, so the projection should
        // carry them and the front end should derive the word, not the truth.
        self.tools_in_flight = calls.saturating_sub(results);
        if self.tools_in_flight == 0 {
            self.reset_tool_watch();
            return;
        }
        if self.tool_wait_from.is_none() {
            self.tool_wait_from = self.elapsed_millis;
        }
    }

    /// Ends the tool watch: no call is in flight.
    pub(super) const fn reset_tool_watch(&mut self) {
        self.tools_in_flight = 0;
        self.tool_wait_from = None;
        self.tool_working = false;
    }

    /// Returns the run this front end currently observes as live.
    ///
    /// A run counts as live only while a live subscription observes it: the
    /// projection alone outlives the stream when the daemon dies, and a front
    /// end that trusted it would keep offering to stop a run nobody runs any
    /// more - and never reach its own exit and clear arms. A terminal status or
    /// a lost subscription both end this run's liveness, and the status row
    /// stops naming a working phase.
    #[must_use]
    pub fn live_run(&self) -> Option<RunProjectionDto> {
        if self.stream != StreamStatus::Live {
            return None;
        }
        self.active_run
            .filter(|run| !run_status_is_terminal(run.status()))
    }

    /// Requests the interruption of the active run, if one is live.
    pub(super) fn request_interrupt(&mut self) -> Vec<Effect> {
        let Some(run) = self.live_run() else {
            self.notice = Some("no active run to interrupt".to_owned());
            return Vec::new();
        };
        if let Some(session_id) = self.session_id {
            return vec![Effect::Interrupt {
                session_id,
                run_id: run.run_id(),
            }];
        }
        Vec::new()
    }

    /// Requests that the live run stop for good.
    ///
    /// The stop is the terminal gesture: an accepted request ends the run, so
    /// the notice names the stop instead of an interruption the model would
    /// answer with another step.
    pub(super) fn request_cancel(&mut self) -> Vec<Effect> {
        let Some(run) = self.live_run() else {
            self.notice = Some("no active run to stop".to_owned());
            return Vec::new();
        };
        let Some(session_id) = self.session_id else {
            return Vec::new();
        };
        self.note(format!("stopping run {}", short_identifier(run.run_id())));
        vec![Effect::CancelRun {
            session_id,
            run_id: run.run_id(),
        }]
    }

    /// Records the accepted interrupt request.
    pub(super) fn apply_interrupt_accepted(&mut self) -> Vec<Effect> {
        self.note("interrupt accepted".to_owned());
        Vec::new()
    }

    /// Records the accepted stop request.
    pub(super) fn apply_run_cancel_accepted(&mut self) -> Vec<Effect> {
        self.note("stop accepted".to_owned());
        Vec::new()
    }

    /// Drops the oldest transcript rows past the client's own retention bound.
    ///
    /// A drain drops rows in front of the transcript, so it is a replacement
    /// and never an append: the epoch moves and the append-only run ends here.
    // @todo(hack): the client's retention bound is re-applied here because the
    // front end's mirror can hold more rows than the client keeps; the bound
    // should be enforced in one layer.
    fn trim_transcript(&mut self) {
        if self.transcript.len() > RETAINED_TRANSCRIPT_MESSAGES {
            let excess = self.transcript.len() - RETAINED_TRANSCRIPT_MESSAGES;
            self.transcript.drain(..excess);
            self.note_replacement();
        }
    }
}
