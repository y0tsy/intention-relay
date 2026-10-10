//! Typed inputs and client effects of the render-free application state.

use intention_client::RunStreamState;
use intention_proto::{
    DaemonHealthDto, DaemonReadinessDto, ErrorDto, RunId, RunStreamFrameDto,
    SendUserTurnOutcomeDto, SessionId, SessionSnapshotDto, SessionSummariesDto, ThemeDto,
};

use super::{MenuMove, Theme};

/// The readiness the client last reported for the local daemon.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionStatus {
    /// Bootstrap is in flight.
    Connecting,
    /// The daemon reported this readiness.
    Ready(DaemonReadinessDto),
    /// The last attempt failed; the error line carries the reason.
    Failed,
}

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

/// One transcript scroll request, in display rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TranscriptScroll {
    /// Move the transcript window towards older rows.
    Older,
    /// Move the transcript window towards newer rows.
    Newer,
}

/// One cursor move inside the input buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputCursorMove {
    /// Move the cursor one character towards the buffer's start.
    Left,
    /// Move the cursor one character towards the buffer's end.
    Right,
    /// Move the cursor to the first character of the line it is on.
    Home,
    /// Move the cursor to the end of the line it is on.
    End,
}

/// One step through the history of submitted and abandoned input lines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputHistoryMove {
    /// Recall the next older entry, saving the in-progress line first.
    Previous,
    /// Walk towards the newest entry, then back to the in-progress line.
    Next,
}

/// One cursor move inside the sessions browser's row window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserCursorMove {
    /// Move the cursor one row towards the newest session.
    Up,
    /// Move the cursor one row towards the oldest session.
    Down,
}

/// One typed input the front end feeds into [`super::AppState::update`].
///
/// Every variant carries a client, wire, or keymap value; none of them performs
/// work, so the same action sequence drives the state identically in a test, a
/// renderer, or a live terminal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    /// The client reached a ready daemon.
    Bootstrapped(DaemonHealthDto),
    /// The client could not reach a ready daemon.
    BootstrapFailed(ErrorDto),
    /// The daemon answered the bounded current session list.
    SessionsListed(SessionSummariesDto),
    /// Reading the session list failed.
    SessionsListFailed(ErrorDto),
    /// The daemon answered the current state of one session.
    SessionSnapshotLoaded(SessionSnapshotDto),
    /// Reading a session snapshot failed.
    SessionSnapshotFailed(ErrorDto),
    /// The daemon accepted a newly created session.
    SessionCreated(SessionId),
    /// Creating a session failed.
    SessionCreateFailed(ErrorDto),
    /// The daemon answered its effective terminal settings.
    ///
    /// The same action answers the startup read and a theme selection: the
    /// daemon is the authority for the committed theme, so the accepted value
    /// is what moves it, and a selection that is still in flight has not
    /// committed anything.
    SettingsReceived(ThemeDto),
    /// Reading the terminal settings or persisting a selected theme failed.
    SettingsFailed(ErrorDto),
    /// The user selected a theme: the daemon is asked to persist it.
    ThemeSelected(Theme),
    /// The user highlighted a theme in the picker: it previews until the
    /// selection settles.
    ThemePreviewed(Theme),
    /// The user left the theme picker: the local preview is dropped.
    ThemePreviewCleared,
    /// A run subscription opened with the daemon's current run state.
    RunStreamOpened(RunStreamState),
    /// Opening a run subscription failed.
    RunStreamFailed(ErrorDto),
    /// The daemon closed the run stream.
    RunStreamEnded,
    /// One committed or transient run-stream frame arrived.
    FrameReceived(RunStreamFrameDto),
    /// The daemon accepted one user turn.
    TurnAccepted(SendUserTurnOutcomeDto),
    /// The front end measured how long a dispatched turn has been in flight.
    ///
    /// The application core carries no clock: the terminal front end measures
    /// from the turn it dispatches to the run's terminal status and reports the
    /// result here, so the status line stays a pure function of state.
    ElapsedReported {
        /// The measured time, in whole milliseconds.
        millis: u64,
    },
    /// Sending a user turn failed.
    TurnFailed(ErrorDto),
    /// The daemon accepted one interruption request.
    InterruptAccepted,
    /// Interrupting the active run failed.
    InterruptFailed(ErrorDto),
    /// The user typed one character into the input line.
    InputChar(char),
    /// The user deleted the character before the input cursor.
    InputBackspace,
    /// The user moved the input cursor.
    MoveInputCursor(InputCursorMove),
    /// The user walked the input line history.
    NavigateInputHistory(InputHistoryMove),
    /// The user moved the input's command hint band highlight.
    ///
    /// The band is a state machine value the core owns, so the same arrows move
    /// it in a test, a renderer, and a live terminal. Its rows are the commands
    /// the command word selects, or the values of the argument word the caret
    /// sits in.
    MenuMove(MenuMove),
    /// The user committed the highlighted row of the input's hint band.
    ///
    /// The row is a command or one value of the argument the word fills, and
    /// completing it writes its label and one trailing space into the line.
    MenuAccept,
    /// The user submitted the input line.
    InputSubmitted,
    /// The user cleared the input line, keeping its text recallable.
    ClearInput,
    /// The user asked for the sessions browser.
    SessionsBrowserRequested,
    /// The user closed the sessions browser.
    SessionsBrowserClosed,
    /// The user switched the sessions browser to its next tab.
    BrowserTabNext,
    /// The user switched the sessions browser to its previous tab.
    BrowserTabPrevious,
    /// The user typed one character into the sessions browser filter.
    BrowserFilterChar(char),
    /// The user deleted the character before the sessions browser filter.
    BrowserFilterBackspace,
    /// The user moved the sessions browser cursor.
    BrowserCursorMove(BrowserCursorMove),
    /// The user opened the session the sessions browser cursor points at.
    BrowserSelectionRequested,
    /// The user asked to rename the selected session.
    BrowserRenameRequested,
    /// The user asked to archive the selected session.
    BrowserArchiveRequested,
    /// The user asked for the selected session's fork tree.
    BrowserTreeRequested,
    /// The user asked for a new session.
    NewSessionRequested,
    /// The user pressed Ctrl+C: one key press with layered effects.
    ///
    /// The core decides what the press means from the state it finds and from
    /// the arm the previous press left: it arms or fires the interrupt of a
    /// live run, abandons a typed line into the history, and arms or fires the
    /// exit. See `super::ctrl_c`.
    CtrlCPressed,
    /// The user pressed Esc on the chat screen: cancel, clear, or exit.
    ///
    /// The core decides what the press means from the state it finds and from
    /// the arm the previous press left: a live run is interrupted immediately,
    /// a typed line needs two consecutive presses to be abandoned into the
    /// history, and an empty line leaves the front end. The sessions browser
    /// maps Esc to its own close action, so the two meanings never meet. See
    /// `super::escape`.
    EscapePressed,
    /// The user asked to interrupt the active run.
    InterruptRequested,
    /// The user asked to re-read current state from the daemon.
    ReconnectRequested,
    /// The view reported the transcript display rows one selection reaches.
    ///
    /// The range is measured in display rows: the view hit-tests its cells to
    /// rows and reports the range the pointer pressed and dragged to, while
    /// the core owns the selection. A press and release without a drag reports
    /// the pressed row alone.
    SelectTranscriptRows {
        /// The display row the press anchored the selection at.
        anchor: u16,
        /// The display row the pointer last reached.
        extent: u16,
    },
    /// The user activated a collapsed reasoning block's expand affordance.
    ExpandReasoning {
        /// The committed transcript row the clicked marker belongs to, or
        /// `None` for the key binding, which expands the newest collapsed
        /// block.
        row: Option<usize>,
    },
    /// The user dragged the transcript past its edge: one fast scroll step.
    ///
    /// The step is the documented multiple of a wheel notch that a held
    /// pointer uses while it slides past the transcript's edge, and that a
    /// wheel notch arriving while the button is held adds.
    ScrollTranscriptFast(TranscriptScroll),
    /// The user scrolled the transcript window.
    ScrollTranscript(TranscriptScroll),
    /// The user asked to leave the front end.
    Quit,
}

impl Action {
    /// Returns whether this action reports the client's own work.
    ///
    /// A client report answers an effect the front end dispatched, carries a
    /// stream frame, or carries the elapsed time the front end measured. None
    /// of them is something the user asked for, and the layered Ctrl+C arm is
    /// consecutive over the user's own actions only: a live run's frames and a
    /// live turn's clock reports arrive between two presses of the key that
    /// arms its interrupt, so they leave the arm standing.
    #[must_use]
    pub(super) const fn is_client_report(&self) -> bool {
        matches!(
            self,
            Self::Bootstrapped(_)
                | Self::BootstrapFailed(_)
                | Self::SessionsListed(_)
                | Self::SessionsListFailed(_)
                | Self::SessionSnapshotLoaded(_)
                | Self::SessionSnapshotFailed(_)
                | Self::SessionCreated(_)
                | Self::SessionCreateFailed(_)
                | Self::SettingsReceived(_)
                | Self::SettingsFailed(_)
                | Self::RunStreamOpened(_)
                | Self::RunStreamFailed(_)
                | Self::RunStreamEnded
                | Self::FrameReceived(_)
                | Self::ElapsedReported { .. }
                | Self::TurnAccepted(_)
                | Self::TurnFailed(_)
                | Self::InterruptAccepted
                | Self::InterruptFailed(_)
        )
    }
}

/// One client operation the application core asks the front end to perform.
///
/// Effects carry no client handle: a front end decides how to run them, so the
/// application core stays constructible without a daemon.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    /// Bootstrap or re-bootstrap the shared local client.
    Connect,
    /// Read the daemon's effective terminal settings.
    ///
    /// The settings are read once the connection is ready and whenever a
    /// reconnect re-establishes it, so the committed theme starts from the
    /// daemon's own value instead of a terminal guess.
    LoadSettings,
    /// Persist one selected theme through the daemon.
    ///
    /// The daemon stores the override and answers with the accepted theme;
    /// only that reply moves the committed theme.
    PersistTheme(Theme),
    /// Read the bounded current session list.
    ListSessions,
    /// Read the current snapshot of one session.
    OpenSession(SessionId),
    /// Create a session from the front end's workspace and mode.
    CreateSession,
    /// Send one user turn with this content.
    SendTurn {
        /// The owning session.
        session_id: SessionId,
        /// The non-blank turn content.
        content: String,
    },
    /// Subscribe to the committed stream of one run.
    Subscribe {
        /// The owning session.
        session_id: SessionId,
        /// The subscribed run.
        run_id: RunId,
    },
    /// Drop the live subscription the front end holds, if any.
    ///
    /// A front end owns its one subscription as a resource, so it is the front
    /// end that closes it; no client call is involved.
    CloseStream,
    /// Request interruption of one run's current operation.
    Interrupt {
        /// The owning session.
        session_id: SessionId,
        /// The interrupted run.
        run_id: RunId,
    },
}
