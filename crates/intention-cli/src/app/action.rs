//! Typed inputs and client effects of the render-free application state.

use intention_client::SessionStreamState;
use intention_proto::{
    DaemonHealthDto, ErrorDto, RunId, SendUserTurnOutcomeDto, SessionId, SessionSnapshotDto,
};

/// The state of the live run-stream subscription.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamStatus {
    /// No subscription is open.
    Inactive,
    /// Committed frames and provisional deltas are arriving.
    Live,
    /// The daemon closed the stream; re-reading current state needs a new subscription.
    Ended,
    /// The subscription failed; the error line carries the reason.
    Failed,
}

/// One typed input a front end feeds into [`super::AppState::update`].
///
/// Every variant carries a client or wire value; none of them performs work, so
/// the same action sequence drives the state identically in a test and in a
/// live front end.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    /// The client reached a ready daemon.
    Bootstrapped(DaemonHealthDto),
    /// The client could not reach a ready daemon.
    BootstrapFailed(ErrorDto),
    /// The daemon answered the current state of one session.
    SessionSnapshotLoaded(SessionSnapshotDto),
    /// Reading a session snapshot failed.
    SessionSnapshotFailed(ErrorDto),
    /// The daemon accepted a newly created session.
    SessionCreated(SessionId),
    /// Creating a session failed.
    ///
    /// The failure carries the fact the waiting phase needs: the create command
    /// that carried the prompt started the run the phase describes, so its
    /// rejection ends that phase, while a create without a first turn started
    /// no run and leaves a live run's phase standing.
    SessionCreateFailed {
        /// Whether the failed command carried a first user turn.
        carried_first_turn: bool,
        /// The typed rejection.
        error: ErrorDto,
    },
    /// A run subscription opened with the daemon's current run state.
    SessionStreamOpened(SessionStreamState),
    /// Opening a run subscription failed.
    SessionStreamFailed(ErrorDto),
    /// The daemon closed the run stream.
    SessionStreamEnded,
    /// One run-stream frame arrived, already derived by the client.
    ///
    /// The client applies every frame to the one session state it exposes and
    /// hands the result here: the front end takes that state and never replays
    /// the frame into a copy of its own. Boxed because the state is the widest
    /// value an action carries, and the action crosses a channel once per frame.
    FrameReceived(Box<SessionStreamState>),
    /// The daemon accepted one user turn.
    TurnAccepted(SendUserTurnOutcomeDto),
    /// Sending a user turn failed.
    TurnFailed(ErrorDto),
    /// The daemon accepted one stop request.
    RunCancelAccepted,
    /// Stopping the active run failed.
    RunCancelFailed(ErrorDto),
    /// One prompt arrived as data instead of as typed input.
    ///
    /// A scripted caller - the headless command, the line REPL - hands the turn
    /// over directly, so the slash grammar never decides what the prompt means:
    /// a `/…` prompt is a turn and a trailing backslash is not a line break.
    TurnSubmitted {
        /// The prompt content, exactly as the caller supplied it.
        content: String,
    },
    /// The user asked for a new session.
    NewSessionRequested,
    /// The user asked to stop the active run for good.
    CancelRunRequested,
}

/// One client operation the application core asks the front end to perform.
///
/// Effects carry no client handle: a front end decides how to run them, so the
/// application core stays constructible without a daemon.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    /// Bootstrap or re-bootstrap the shared local client.
    Connect,
    /// Read the current snapshot of one session.
    OpenSession(SessionId),
    /// Create a session from the front end's workspace and mode.
    ///
    /// `first_turn` is the prompt the creation commits as the new session's
    /// first turn, or `None` to create an empty session. The prompt never parks
    /// in the state: the one command either commits it or rejects it.
    CreateSession {
        /// The first user turn's content, when a prompt asked for the session.
        first_turn: Option<String>,
    },
    /// Send one user turn with this content.
    SendTurn {
        /// The owning session.
        session_id: SessionId,
        /// The non-blank turn content.
        content: String,
    },
    /// Subscribe to the live changes of one session.
    Subscribe {
        /// The owning session.
        session_id: SessionId,
    },
    /// Request that one run stop for good.
    ///
    /// The daemon signals the run's in-flight operation and the run ends
    /// `interrupted`.
    CancelRun {
        /// The owning session.
        session_id: SessionId,
        /// The stopped run.
        run_id: RunId,
    },
}
