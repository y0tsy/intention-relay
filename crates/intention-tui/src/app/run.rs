//! The run transitions: the live subscription, frames, turns, and interrupts.

use intention_client::{RETAINED_TRANSCRIPT_MESSAGES, RunStreamState};
use intention_proto::{
    ErrorDto, MessageKindDto, MessageProjectionDto, RunStreamFrameDto, SendUserTurnOutcomeDto,
    TextDeltaChannelDto, run_status_is_terminal,
};

use super::{AppState, Effect, RunPhase, StreamStatus, short_identifier};

/// How long a tool call may be in flight before the status row names it.
///
/// Half a second: a shorter call leaves the phase the one it found, so a fast
/// tool never flickers the row. The threshold is a subtraction on the elapsed
/// value the front end reported, never a clock read in this clock-free core.
const TOOL_WORKING_AFTER_MILLIS: u64 = 500;

impl AppState {
    /// Mirrors a freshly opened subscription and appends the rows it was missing.
    pub(super) fn apply_run_stream_opened(&mut self, state: RunStreamState) -> Vec<Effect> {
        // The subscription snapshot is a fresh read of the run's rows: rows the
        // seeded transcript is missing (they committed after the session read)
        // are appended once, and rows it already carries are not repeated. The
        // client reconciles the two reads by durable row identity.
        let missing = state.missing_rows(&self.transcript);
        self.extend_transcript(missing);
        self.trim_transcript();
        self.active_run = state.run().copied();
        self.run_stream = Some(state);
        self.stream = StreamStatus::Live;
        self.error = None;
        // A fresh subscription carries no streamed text: a live run it opens is
        // waiting for the first token this state will see, and a terminal one
        // carries no phase at all.
        self.rebase_run_phase();
        Vec::new()
    }

    /// Records a subscription that failed to open or stay live.
    pub(super) fn apply_run_stream_failed(&mut self, error: &ErrorDto) -> Vec<Effect> {
        self.run_stream = None;
        self.stream = StreamStatus::Failed;
        self.apply_failure(error)
    }

    /// Records a stream the daemon closed.
    pub(super) fn apply_run_stream_ended(&mut self) -> Vec<Effect> {
        self.run_stream = None;
        self.stream = StreamStatus::Ended;
        self.note("live updates ended".to_owned());
        Vec::new()
    }

    /// Applies one committed or transient run-stream frame.
    pub(super) fn apply_frame(&mut self, frame: RunStreamFrameDto) -> Vec<Effect> {
        let Some(stream) = self.run_stream.as_mut() else {
            // A frame for a subscription this front end no longer holds is stale;
            // dropping it keeps the seeded transcript authoritative.
            return Vec::new();
        };
        if let Err(error) = stream.apply_frame(frame.clone()) {
            return self.apply_failure(&error);
        }
        match frame {
            RunStreamFrameDto::Content(message) => {
                // A content frame repeating the newest accepted row carries the
                // identity of the row the snapshot already carried, so the two
                // are the same committed row.
                if self.transcript.last().map(MessageProjectionDto::id) != Some(message.id()) {
                    self.push_transcript_row(message);
                    self.trim_transcript();
                }
            }
            RunStreamFrameDto::Status(run) => {
                self.active_run = Some(run);
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
            RunStreamFrameDto::TextDelta(delta) => {
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

    /// Subscribes to a run the daemon accepted a turn for.
    pub(super) fn apply_turn_accepted(&mut self, outcome: SendUserTurnOutcomeDto) -> Vec<Effect> {
        self.error = None;
        let Some(session_id) = self.session_id else {
            return Vec::new();
        };
        match outcome {
            SendUserTurnOutcomeDto::Started { run_id, .. } => {
                self.note(format!("run {} started", short_identifier(run_id)));
                vec![Effect::Subscribe { session_id, run_id }]
            }
            SendUserTurnOutcomeDto::Pending => {
                self.note("turn queued behind the active run".to_owned());
                Vec::new()
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

    /// Requests the interruption of the active run, if one is live.
    pub(super) fn request_interrupt(&mut self) -> Vec<Effect> {
        let Some(session_id) = self.session_id else {
            self.notice = Some("no session is open".to_owned());
            return Vec::new();
        };
        let active_run = self
            .active_run
            .filter(|run| !run_status_is_terminal(run.status()));
        let Some(run) = active_run else {
            self.notice = Some("no active run to interrupt".to_owned());
            return Vec::new();
        };
        self.note(format!(
            "interrupting run {}",
            short_identifier(run.run_id())
        ));
        vec![Effect::Interrupt {
            session_id,
            run_id: run.run_id(),
        }]
    }

    /// Records the accepted interrupt request.
    pub(super) fn apply_interrupt_accepted(&mut self) -> Vec<Effect> {
        self.note("interrupt accepted".to_owned());
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
