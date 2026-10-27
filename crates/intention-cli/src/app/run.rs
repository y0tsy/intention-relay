//! The run transitions: the live subscription, frames, turns, and stops.

use intention_client::SessionStreamState;
use intention_proto::{ErrorDto, RunProjectionDto, SendUserTurnOutcomeDto, run_status_is_terminal};

use super::{AppState, Effect, StreamStatus};

impl AppState {
    /// Takes the client's committed state of a freshly opened subscription.
    ///
    /// The subscription reply is the client's own read of the session, and the
    /// state beside it is that read: this front end prints those rows and
    /// nothing else, so it holds the client's value instead of merging a copy of
    /// its own. Every later frame replaces it the same way, which is what keeps
    /// one derivation and one transcript version across the two.
    pub(super) fn apply_session_stream_opened(&mut self, state: SessionStreamState) -> Vec<Effect> {
        self.session_state = Some(state);
        self.stream = StreamStatus::Live;
        // An accepted subscription proves only that a stream exists: the
        // reconnect budget a loss spent stays spent until a frame this state
        // accepts shows the stream is carrying the session's traffic.
        self.error = None;
        Vec::new()
    }

    /// Records a subscription that failed to open or stay live.
    ///
    /// The stream is this front end's one live channel, so its loss is not a
    /// dead end: the state ends the observation the stream justified and asks
    /// for the one reconnect this loss is owed. The reconnect re-reads current
    /// state through the one resynchronization this protocol has, and a
    /// reconnect that fails reports its own typed error instead of retrying in
    /// a loop.
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
    /// The transient provisional text belongs to the stream that streamed it, so
    /// [`Self::live_run`] stops claiming a run nobody observes.
    fn end_stream_observation(&mut self) {
        if let Some(state) = self.session_state.as_mut() {
            state.end_stream();
        }
    }

    /// Returns the one reconnect this front end asks for after a lost stream.
    ///
    /// The budget is one reconnect per lost stream: the first loss asks for it
    /// and a second loss inside the same recovery asks for nothing, so the front
    /// end reports its typed failure instead of retrying in a loop. Only a frame
    /// the state accepts restores the budget: a subscription that opened and
    /// delivered nothing leaves the next loss asking for nothing. The loss stays
    /// visible: a typed failure keeps its error line, and the stream status keeps
    /// naming the loss.
    pub(super) fn reconnect_once(&mut self) -> Vec<Effect> {
        if self.reconnects_asked > 0 {
            return Vec::new();
        }
        self.reconnects_asked += 1;
        vec![Effect::Connect]
    }

    /// Applies one run-stream frame the client already derived.
    ///
    /// The client applies every frame to the one session state it exposes, so
    /// the state carried beside the frame is the derivation's own result: this
    /// front end holds it instead of replaying the frame into a copy of its own.
    pub(super) fn apply_frame(&mut self, state: SessionStreamState) -> Vec<Effect> {
        if self.stream != StreamStatus::Live {
            // A frame for a subscription this front end no longer holds is
            // stale; dropping it keeps the committed rows authoritative.
            return Vec::new();
        }
        self.session_state = Some(state);
        // A frame the client accepted is the evidence that this stream carries
        // the session's traffic: the one reconnect a later loss is owed is
        // restored here, and never by a subscription that only opened.
        self.reconnects_asked = 0;
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
            SendUserTurnOutcomeDto::Started { .. } => self.ensure_session_stream(),
            SendUserTurnOutcomeDto::Pending => self.ensure_session_stream(),
        }
    }

    /// Records a turn request that never reached a run.
    ///
    /// The failure itself becomes the error line.
    pub(super) fn apply_turn_send_failed(&mut self, error: &ErrorDto) -> Vec<Effect> {
        self.apply_failure(error)
    }

    /// Returns the run this front end currently observes as live.
    ///
    /// A run counts as live only while a live subscription observes it: the
    /// projection alone outlives the stream when the daemon dies, and a front
    /// end that trusted it would keep offering to stop a run nobody runs any
    /// more. A terminal status or a lost subscription both end this run's
    /// liveness.
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
            return Vec::new();
        };
        let Some(session_id) = self.session_id else {
            return Vec::new();
        };
        vec![Effect::CancelRun {
            session_id,
            run_id: run.run_id(),
        }]
    }

    /// Records the accepted stop request.
    pub(super) fn apply_run_cancel_accepted(&mut self) -> Vec<Effect> {
        Vec::new()
    }
}
