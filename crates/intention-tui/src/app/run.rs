//! The run transitions: the live subscription, frames, turns, and stops.

use intention_client::{FrameUpdate, SessionStreamState};
use intention_proto::{
    ErrorDto, RunProjectionDto, SendUserTurnOutcomeDto, SessionFrameDto, TextDeltaChannelDto,
    run_status_is_terminal,
};

use super::{AppState, ConnectionStatus, Effect, RunPhase, StreamStatus, short_identifier};

/// How long a tool call may be in flight before the status row names it.
///
/// Half a second: a shorter call leaves the phase the one it found, so a fast
/// tool never flickers the row. The threshold is a subtraction on the elapsed
/// value the front end reported, never a clock read in this clock-free core.
const TOOL_WORKING_AFTER_MILLIS: u64 = 500;

impl AppState {
    /// Takes the client's committed state of a freshly opened subscription.
    ///
    /// The subscription reply is the client's own read of the session, and the
    /// state beside it is that read: this front end renders those rows and
    /// nothing else, so it holds the client's value instead of merging a copy
    /// of its own. Every later frame replaces it the same way, which is what
    /// keeps one derivation and one transcript version across the two.
    pub(super) fn apply_session_stream_opened(&mut self, state: SessionStreamState) -> Vec<Effect> {
        self.session_state = Some(state);
        self.note_replacement();
        self.stream = StreamStatus::Live;
        // An accepted subscription proves only that a stream exists: the
        // reconnect budget a loss spent stays spent until a frame this state
        // accepts shows the stream is carrying the session's traffic.
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
    /// dead end: the state ends the observation the stream justified - the
    /// transient text, the phase, and the tool watch, so [`Self::live_run`]
    /// stops claiming a run nobody observes and the exit and clear arms come
    /// back - and asks for the one reconnect this loss is owed. The reconnect
    /// re-reads current state through the one resynchronization this protocol
    /// has, and a reconnect that fails reports its own typed error instead of
    /// retrying in a loop.
    ///
    /// The committed rows and the run projection stay: they are the session's
    /// own durable state, and a front end that lost a stream still knows what
    /// the session committed.
    pub(super) fn apply_session_stream_failed(&mut self, error: &ErrorDto) -> Vec<Effect> {
        self.stream = StreamStatus::Failed;
        self.end_stream_observation();
        let mut effects = self.apply_failure(error);
        effects.extend(self.reconnect_once());
        effects
    }

    /// Records a stream the daemon closed.
    ///
    /// A closed stream is the same loss as a failed one - the daemon closed the
    /// connection or the host is leaving - so it ends the same observation and
    /// asks for the same one reconnect.
    pub(super) fn apply_session_stream_ended(&mut self) -> Vec<Effect> {
        self.stream = StreamStatus::Ended;
        self.end_stream_observation();
        self.reconnect_once()
    }

    /// Ends the observation the lost stream justified.
    ///
    /// The transient provisional text belongs to the stream that streamed it,
    /// and the phase and the tool watch belong to a run this front end no longer
    /// observes: the status row never names a working run nobody watches, and
    /// [`Self::live_run`] stops claiming one, which is what lets the exit and
    /// clear arms through.
    fn end_stream_observation(&mut self) {
        if let Some(state) = self.session_state.as_mut() {
            state.end_stream();
        }
        self.phase = None;
        self.reset_tool_watch();
    }

    /// Returns the one reconnect this front end asks for after a lost stream.
    ///
    /// The budget is one reconnect per lost stream: the first loss asks for it
    /// and a second loss inside the same recovery asks for nothing, so the
    /// front end reports its typed failure instead of retrying in a loop. Only
    /// a frame the state accepts restores the budget: a subscription that
    /// opened and delivered nothing leaves the next loss asking for nothing.
    /// The connection reads as connecting while the attempt runs, and the loss
    /// stays visible: a typed failure keeps its error line, and the stream
    /// status keeps naming the loss, so the status row never claims live
    /// updates this front end does not have.
    pub(super) fn reconnect_once(&mut self) -> Vec<Effect> {
        if self.reconnects_asked > 0 {
            return Vec::new();
        }
        self.reconnects_asked += 1;
        self.connection = ConnectionStatus::Connecting;
        self.note("reconnecting".to_owned());
        vec![Effect::Connect]
    }

    /// Applies one run-stream frame the client already derived.
    ///
    /// The client applies every frame to the one session state it exposes, so
    /// the state beside the frame is the derivation's own result: this front
    /// end takes it and decides only what the frame means for its own display
    /// state - the phase, the notice, and whether a replacement moved the rows.
    pub(super) fn apply_frame(
        &mut self,
        frame: SessionFrameDto,
        update: FrameUpdate,
        state: SessionStreamState,
    ) -> Vec<Effect> {
        if self.stream != StreamStatus::Live {
            // A frame for a subscription this front end no longer holds is
            // stale; dropping it keeps the committed rows authoritative.
            return Vec::new();
        }
        let epoch = self.transcript_epoch();
        let previous_run = self.active_run().map(|run| run.run_id());
        self.session_state = Some(state);
        // A frame the client accepted is the evidence that this stream carries
        // the session's traffic: the one reconnect a later loss is owed is
        // restored here, and never by a subscription that only opened.
        self.reconnects_asked = 0;
        match frame {
            SessionFrameDto::Content(_) => {
                // A row the client's retention bound dropped in front of the
                // transcript is a replacement, never an append: every display
                // row moved, so the selection and the expansions go with it.
                // The in-flight tool watch does not move with a row: the
                // projection the daemon republishes after a committed tool row
                // is the fact that carries the run's open calls.
                if self.transcript_appended_since(epoch).is_none() {
                    self.note_replacement();
                }
            }
            SessionFrameDto::Status(run) => {
                // A status of a run this state does not project yet is a run
                // change - it came from another window's turn or from the daemon
                // itself - and the phase belongs to the run, so the previous
                // run's phase and its in-flight tool call never carry over. A
                // status of the projected run keeps the phase the stream named.
                if previous_run != Some(run.run_id()) {
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
                    // The committed projection is the one fact that says how
                    // many tool calls the run has open, so the watch follows
                    // every live status the daemon publishes.
                    self.refresh_tool_watch();
                }
                self.note(format!("run {}", run.status().as_str()));
            }
            SessionFrameDto::TextDelta(delta) => {
                // The channel carries every run of the session, so a delta of a
                // run the client did not advance is another window's text: it
                // never colours the projected run's phase.
                if !matches!(update, FrameUpdate::Delta { .. }) {
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
    /// the projection the read carries.
    pub(super) fn rebase_run_phase(&mut self) {
        self.reset_tool_watch();
        self.refresh_tool_watch();
        let live = self
            .run_status()
            .is_some_and(|status| !run_status_is_terminal(status));
        self.phase = live.then_some(RunPhase::Waiting);
    }

    /// Re-reads the in-flight tool watch from the run projection's open calls.
    ///
    /// The count is the committed run's own fact - storage pairs its committed
    /// call rows with their committed results by call identity - so this state
    /// decides only the word. The elapsed value the front end last reported is
    /// the baseline the working threshold measures from, so a call the
    /// projection reports before any report has no baseline and never gets one
    /// invented for it.
    pub(super) fn refresh_tool_watch(&mut self) {
        let open = self.active_run().map_or(0, |run| run.open_tool_calls());
        if open == 0 {
            self.reset_tool_watch();
            return;
        }
        if self.tools_in_flight == 0 {
            self.tool_wait_from = self.elapsed_millis;
        }
        self.tools_in_flight = open;
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
        self.active_run()
            .filter(|run| !run_status_is_terminal(run.status()))
    }

    /// Requests that the live run stop for good.
    ///
    /// The stop is the terminal gesture: an accepted request ends the run, so
    /// the notice names the stop.
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

    /// Records the accepted stop request.
    pub(super) fn apply_run_cancel_accepted(&mut self) -> Vec<Effect> {
        self.note("stop accepted".to_owned());
        Vec::new()
    }
}
