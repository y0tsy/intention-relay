//! The run transitions: the live subscription, frames, turns, and interrupts.

use intention_client::{RETAINED_TRANSCRIPT_MESSAGES, RunStreamState};
use intention_proto::{
    ErrorDto, MessageProjectionDto, RunStreamFrameDto, SendUserTurnOutcomeDto,
    run_status_is_terminal,
};

use super::{AppState, Effect, StreamStatus, short_identifier};

impl AppState {
    /// Mirrors a freshly opened subscription and appends the rows it was missing.
    pub(super) fn apply_run_stream_opened(&mut self, state: RunStreamState) -> Vec<Effect> {
        // The subscription snapshot is a fresh read of the run's rows: rows the
        // seeded transcript is missing (they committed after the session read)
        // are appended once, and rows it already carries are not repeated.
        let missing = missing_rows(&self.transcript, state.messages());
        self.extend_transcript(missing);
        self.trim_transcript();
        self.active_run = state.run().copied();
        self.run_stream = Some(state);
        self.stream = StreamStatus::Live;
        self.error = None;
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
                // A content frame repeating the newest accepted row is the row the
                // snapshot already carried; the wire carries no row identity.
                // @todo(core): reconcile committed transcript rows in the client
                // (a durable row identity is a core fact) instead of comparing
                // whole DTOs in the front end.
                if self.transcript.last() != Some(&message) {
                    self.push_transcript_row(message);
                    self.trim_transcript();
                }
            }
            RunStreamFrameDto::Status(run) => {
                self.active_run = Some(run);
                self.note(format!("run {}", run.status().as_str()));
            }
            RunStreamFrameDto::TextDelta(_) => {}
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
    pub(super) const fn apply_elapsed_reported(&mut self, millis: u64) -> Vec<Effect> {
        self.elapsed_millis = Some(millis);
        Vec::new()
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
    fn trim_transcript(&mut self) {
        if self.transcript.len() > RETAINED_TRANSCRIPT_MESSAGES {
            let excess = self.transcript.len() - RETAINED_TRANSCRIPT_MESSAGES;
            self.transcript.drain(..excess);
            self.note_replacement();
        }
    }
}

/// Returns the rows of `rows` that `existing` does not already carry, in order.
///
/// The run subscription snapshot and the session snapshot read the same durable
/// transcript, so one may carry rows the other already shows; the ordered match
/// keeps repeated-but-distinct rows (a user sending the same text twice) apart.
// @todo(core): merge the session read and the run-subscription read into one
// ordered transcript in the client; the front end must not reconcile two reads
// of the same store by matching rows in order.
fn missing_rows(
    existing: &[MessageProjectionDto],
    rows: &[MessageProjectionDto],
) -> Vec<MessageProjectionDto> {
    let mut cursor = 0;
    let mut missing = Vec::new();
    for row in rows {
        while existing
            .get(cursor)
            .is_some_and(|candidate| candidate != row)
        {
            cursor += 1;
        }
        if existing.get(cursor) == Some(row) {
            cursor += 1;
        } else {
            missing.push(row.clone());
        }
    }
    missing
}
