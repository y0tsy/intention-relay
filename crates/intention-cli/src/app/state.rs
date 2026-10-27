//! The render-free state machine the line front ends drive.
//!
//! This module owns the state structure, its accessors, and the one `update`
//! dispatch. The transition bodies live one file per feature beside it:
//! `sessions.rs` and `run.rs`.

use intention_client::SessionStreamState;
use intention_proto::{
    DtoResult, ErrorDto, MessageId, MessageProjectionDto, RunProjectionDto, RunStatusDto, SessionId,
};

use super::{Action, Effect, StreamStatus};

/// Every value one line front end holds, with no terminal attached.
///
/// The state owns the current session's committed transcript and the live run
/// mirror; a front end reads it and mutates it only through
/// [`AppState::update`]. The fields are visible to the transition modules beside
/// this one and to nothing else.
pub struct AppState {
    pub(super) initial_session: Option<SessionId>,
    pub(super) session_id: Option<SessionId>,
    /// The shared client's state of the open session, if any.
    ///
    /// The client owns the committed transcript - its rows, its one retention
    /// bound, its run projection, its supersession identities, and its
    /// append-only epoch - so this front end holds no second copy of any of it:
    /// the accessors below delegate. The field is crate-visible because the
    /// headless driver's tests derive a frame through the same client state the
    /// production pump holds.
    pub(crate) session_state: Option<SessionStreamState>,
    /// How many reconnects this front end has asked for the current stream.
    ///
    /// The budget is one reconnect per lost stream: the first loss asks for it
    /// and a second loss inside the same recovery asks for nothing, so the
    /// front end reports its typed failure instead of retrying in a loop. Only
    /// a frame the state accepts restores it: a subscription that opened and
    /// delivered no traffic leaves it spent.
    pub(super) reconnects_asked: u32,
    pub(super) stream: StreamStatus,
    pub(super) error: Option<String>,
}

impl AppState {
    /// Creates the state a front end starts from, pinned to one session to open.
    ///
    /// `initial_session` is the session the caller selected on the command line,
    /// or the session the client resolved for an explicit continuation; `None`
    /// opens nothing. The state needs no terminal, daemon, or clock to exist.
    #[must_use]
    pub const fn new(initial_session: Option<SessionId>) -> Self {
        Self {
            initial_session,
            session_id: None,
            session_state: None,
            reconnects_asked: 0,
            stream: StreamStatus::Inactive,
            error: None,
        }
    }

    /// Applies one action and returns the client effects it asks for.
    ///
    /// The transition is total: every action leaves a coherent state, and a
    /// failure carried by an action becomes the error line instead of an error
    /// return, so a front end never has to recover from a state transition.
    pub fn update(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::Bootstrapped(_health) => self.apply_bootstrapped(),
            Action::BootstrapFailed(error) => self.apply_bootstrap_failed(&error),
            Action::SessionSnapshotLoaded(snapshot) => self.apply_session_snapshot(snapshot),
            Action::SessionSnapshotFailed(error) => self.apply_failure(&error),
            Action::SessionCreated(session_id) => self.apply_session_created(session_id),
            Action::SessionCreateFailed {
                carried_first_turn: _,
                error,
            } => self.apply_failure(&error),
            Action::SessionStreamOpened(state) => self.apply_session_stream_opened(state),
            Action::SessionStreamFailed(error) => self.apply_session_stream_failed(&error),
            Action::SessionStreamEnded => self.apply_session_stream_ended(),
            Action::FrameReceived(state) => self.apply_frame(*state),
            Action::TurnAccepted(outcome) => self.apply_turn_accepted(outcome),
            Action::TurnFailed(error) => self.apply_turn_send_failed(&error),
            Action::RunCancelAccepted => self.apply_run_cancel_accepted(),
            Action::RunCancelFailed(error) => self.apply_failure(&error),
            Action::TurnSubmitted { content } => self.submit_turn(content),
            Action::NewSessionRequested => self.request_new_session(),
            Action::CancelRunRequested => self.request_cancel(),
        }
    }

    /// Returns the session the front end currently shows.
    #[must_use]
    pub const fn session_id(&self) -> Option<SessionId> {
        self.session_id
    }

    /// Returns the committed transcript rows of the current session.
    ///
    /// The rows are the client's own reconciled list: this front end holds no
    /// second projection, so what a caller prints is exactly what the client
    /// kept.
    #[must_use]
    pub fn transcript(&self) -> &[MessageProjectionDto] {
        self.session_state
            .as_ref()
            .map_or(&[], SessionStreamState::messages)
    }

    /// Returns the retained rows appended after the identity a caller last
    /// reported, in durable order.
    ///
    /// The walk is the client's, by durable identity: a reported row the
    /// retention bound has since dropped never blocks the next report.
    #[must_use]
    pub fn transcript_after(&self, after: Option<MessageId>) -> &[MessageProjectionDto] {
        self.session_state
            .as_ref()
            .map_or(&[], |state| state.rows_after(after))
    }

    /// Returns the model step whose provisional text a committed row
    /// superseded, once that row arrived.
    ///
    /// The identity is the client's, so a sink that already wrote a step's
    /// streamed text never compares text to decide whether to suppress a row.
    #[must_use]
    pub fn superseded_step(&self, row: MessageId) -> Option<u32> {
        self.session_state
            .as_ref()
            .and_then(|state| state.superseded_step(row))
    }

    /// Returns the last known run projection.
    ///
    /// The projection is the client's: a status frame replaces it, so a finished
    /// run keeps reporting its terminal status until another run starts, and a
    /// lost stream leaves the last projection the session reported in place.
    #[must_use]
    pub fn active_run(&self) -> Option<RunProjectionDto> {
        self.session_state
            .as_ref()
            .and_then(SessionStreamState::run)
            .copied()
    }

    /// Returns the last known run lifecycle status.
    #[must_use]
    pub fn run_status(&self) -> Option<RunStatusDto> {
        self.session_state
            .as_ref()
            .and_then(SessionStreamState::status)
    }

    /// Returns the single-line failure the last transition reported.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Merges one read of a session's durable transcript into the client state.
    ///
    /// The client matches rows by durable identity, so a read of the session
    /// this front end already holds joins the rows it has; a read of another
    /// session seeds the state, because a front end holds exactly one session's
    /// rows.
    ///
    /// # Errors
    ///
    /// Returns the client's scoped-response error for a read of another session.
    pub(super) fn merge_transcript(&mut self, read: SessionStreamState) -> DtoResult<()> {
        if self.session_id != Some(read.session_id()) {
            self.session_state = Some(read);
            return Ok(());
        }
        let state = self
            .session_state
            .get_or_insert_with(|| SessionStreamState::new(read.session_id()));
        state.reconcile(read)
    }

    /// Sends one prompt the caller supplied as data.
    ///
    /// A front end hands the turn over directly, so the slash grammar never
    /// decides what the prompt means. With no session open the prompt creates
    /// the session it needs as that session's first turn, in one command.
    pub(super) fn submit_turn(&mut self, content: String) -> Vec<Effect> {
        self.error = None;
        let Some(session_id) = self.session_id else {
            return vec![Effect::CreateSession {
                first_turn: Some(content),
            }];
        };
        vec![Effect::SendTurn {
            session_id,
            content,
        }]
    }

    /// Records one typed failure as the error line.
    ///
    /// Shared by every transition module: a failed client call keeps the
    /// failure's typed code and safe message in the state instead of returning
    /// an error, and the line is the error type's own one-line form.
    pub(super) fn apply_failure(&mut self, error: &ErrorDto) -> Vec<Effect> {
        self.error = Some(error.to_string());
        Vec::new()
    }
}
