//! The render-free state machine the terminal front ends drive.
//!
//! This module owns the state structure, its accessors, and the one `update`
//! dispatch. The transition bodies live one file per feature beside it:
//! `sessions.rs`, `browser.rs`, `run.rs`, `input.rs`, `transcript.rs`, and
//! `ctrl_c.rs`.

use intention_client::SessionStreamState;
use intention_proto::{
    DtoResult, ErrorDto, MessageId, MessageKindDto, MessageProjectionDto, RunModeDto,
    RunProjectionDto, RunStatusDto, SessionId, SessionSummaryDto, WorkspaceRootDto,
};

use super::{
    Action, BrowserRow, BrowserTab, CommandMenu, ConnectionStatus, Effect, RunPhase, Screen,
    StreamStatus, Theme, TranscriptSelection, browser, ctrl_c, transcript,
};

/// Every value one terminal session holds, with no terminal attached.
///
/// The state owns the session list, the current session's committed transcript,
/// the live run mirror, the input line, and the status text; renderers read it
/// and the front end mutates it only through [`AppState::update`]. The fields
/// are visible to the transition modules beside this one and to nothing else.
pub struct AppState {
    pub(super) initial_session: Option<SessionId>,
    /// The prompt a submission with no open session asked to send.
    ///
    /// A prompt is never rejected for the lack of a session: the session it
    /// needs is created and opened, and the snapshot that opens it sends the
    /// prompt as that session's first turn. A session creation or open that
    /// fails drops the prompt, so it can never fire against a session it did
    /// not ask for; its text stays recallable through the input history.
    pub(super) pending_prompt: Option<String>,
    pub(super) connection: ConnectionStatus,
    /// The screen a front end renders.
    pub(super) screen: Screen,
    /// The colour theme a front end renders the session through.
    ///
    /// The theme is a value of the render-free core like the screen: a front
    /// end resolves its palette from this one field and never tracks a theme
    /// of its own, so the whole view stays a pure function of the state.
    ///
    /// This is the committed theme - what the daemon accepted and stored. A
    /// selection or a picker row is not committed here: it lives in
    /// [`AppState::theme_preview`] until the accepted reply arrives.
    pub(super) theme: Theme,
    /// The theme the picker is previewing, while one is.
    ///
    /// The preview is local and never persisted: it is what makes the whole
    /// window repaint through a candidate before the daemon has accepted it,
    /// and dropping it is what restores the committed theme.
    pub(super) theme_preview: Option<Theme>,
    pub(super) sessions: Vec<SessionSummaryDto>,
    pub(super) sessions_omitted: u32,
    pub(super) sessions_loaded: bool,
    /// The sessions browser: its tab, its filter, its rows, and its cursor.
    pub(super) browser: browser::SessionsBrowserState,
    pub(super) session_id: Option<SessionId>,
    /// The run policy mode of the open session's projection.
    pub(super) session_mode: Option<RunModeDto>,
    /// The effective provider model the open session's projection names.
    ///
    /// The model is the daemon's resolved configuration selection: it fills the
    /// session projection, so this front end renders it instead of guessing.
    pub(super) session_model: Option<String>,
    /// The shared client's state of the open session, if any.
    ///
    /// The client owns the committed transcript - its rows, its one retention
    /// bound, its run projection, its supersession identities, and its
    /// append-only epoch - so this front end holds no second copy of any of it:
    /// the accessors below delegate, and the display state they read is what a
    /// pane needs and the client does not carry.
    pub(super) session_state: Option<SessionStreamState>,
    /// The live phase of the run the status row names.
    ///
    /// `None` means no live phase: no turn request has left the state since it
    /// was created, or the run it mirrors reached a terminal status. The
    /// terminal words come from the run projection's own status.
    pub(super) phase: Option<RunPhase>,
    /// How many open tool calls the run projection reports.
    pub(super) tools_in_flight: u32,
    /// The elapsed value the newest open tool call was first observed at.
    ///
    /// The value is the front end's own measurement, so the working threshold
    /// is a subtraction on reported time and never a clock read here. A run
    /// whose projection reports an open call before any report has no
    /// baseline, and the row never invents one.
    pub(super) tool_wait_from: Option<u64>,
    /// Whether an in-flight tool call is past the working threshold.
    pub(super) tool_working: bool,
    /// The elapsed time the front end measured for the current or last turn.
    pub(super) elapsed_millis: Option<u64>,
    /// How many reconnects this front end has asked for the current stream.
    ///
    /// The budget is one reconnect per lost stream: a stream that opens again
    /// resets it, and a second loss inside one recovery asks for nothing, so
    /// the front end reports its typed failure instead of retrying in a loop.
    pub(super) reconnects_asked: u32,
    pub(super) stream: StreamStatus,
    pub(super) input: String,
    /// The caret position of `input`, measured in characters from its start.
    pub(super) cursor: usize,
    /// The prompts the user sent or abandoned, oldest first.
    pub(super) history: Vec<String>,
    /// The history entry being recalled, while the user walks the history.
    pub(super) history_position: Option<usize>,
    /// The in-progress line saved when the user walked into the history.
    pub(super) history_draft: Option<String>,
    /// The input's command hint menu, while one is open.
    ///
    /// The menu is derived from the input line and the caret: the word the
    /// caret is in or immediately after, and the registered commands that word
    /// selects. It holds only what a frame cannot recompute cheaply - the
    /// matches and the highlighted row - so the input line stays the single
    /// source of the word.
    pub(super) command_menu: Option<CommandMenu>,
    /// The arm the last Ctrl+C press left for the next consecutive press.
    ///
    /// The arm names which press armed it: an interrupt arm fires only while
    /// its run is still live, and an exit arm fires only when the input line is
    /// still empty and no run is live, so a state the press no longer matches
    /// starts a fresh sequence instead of firing a stale one.
    pub(super) ctrl_c_arm: Option<ctrl_c::CtrlCArm>,
    /// Whether the last Esc press armed the clear for the next consecutive one.
    ///
    /// The clear is two consecutive Esc presses over a non-empty line and no
    /// live run: the first press arms it with a notice and the second abandons
    /// the line into the history, exactly as one Ctrl+C press does. Only its
    /// own key and client reports leave the arm standing, so any other user
    /// action starts a fresh sequence.
    pub(super) escape_arm: bool,
    pub(super) notice: Option<String>,
    pub(super) error: Option<String>,
    pub(super) scroll: u16,
    /// The transcript display rows the pointer selected, if any.
    pub(super) selection: Option<transcript::TranscriptSelection>,
    /// The expanded reasoning blocks, keyed by their committed transcript row.
    pub(super) reasoning_expansions: Vec<transcript::ReasoningExpansion>,
    /// The epoch of the reasoning expansion state.
    ///
    /// Every activation moves it forward: an expansion inserts display rows
    /// into an existing block, so a layout cache keyed on the transcript epoch
    /// alone would keep a prefix that is no longer a prefix.
    pub(super) reasoning_epoch: u64,
    pub(super) quit: bool,
}

impl AppState {
    /// Creates the state a front end starts from, pinned to one session to open.
    ///
    /// `initial_session` is the session the caller selected on the command line,
    /// or the session the client resolved for an explicit continuation; `None`
    /// opens nothing, so a launch shows the chat's welcome state until the user
    /// asks for a session. The state needs no terminal, daemon, or clock to
    /// exist.
    #[must_use]
    pub const fn new(initial_session: Option<SessionId>) -> Self {
        Self {
            initial_session,
            pending_prompt: None,
            connection: ConnectionStatus::Connecting,
            screen: Screen::Chat,
            theme: Theme::Light,
            theme_preview: None,
            sessions: Vec::new(),
            sessions_omitted: 0,
            sessions_loaded: false,
            browser: browser::SessionsBrowserState::new(),
            session_id: None,
            session_mode: None,
            session_model: None,
            session_state: None,
            phase: None,
            tools_in_flight: 0,
            tool_wait_from: None,
            tool_working: false,
            elapsed_millis: None,
            reconnects_asked: 0,
            stream: StreamStatus::Inactive,
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_position: None,
            history_draft: None,
            command_menu: None,
            ctrl_c_arm: None,
            escape_arm: false,
            notice: None,
            error: None,
            scroll: 0,
            selection: None,
            reasoning_expansions: Vec::new(),
            reasoning_epoch: 0,
            quit: false,
        }
    }

    /// Returns the same state rendering through `theme`.
    ///
    /// The theme is the terminal's own value, so a caller selects it directly
    /// rather than through a wire action: the state carries it and a front end
    /// resolves its palette from it once per frame.
    #[must_use]
    pub const fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Returns the same state whose sessions browser selects the current folder
    /// by `root`.
    ///
    /// The root is the front end's own - the directory it was launched in - and
    /// the browser's `Current Folder` tab selects exactly the sessions durably
    /// bound to it. A caller that supplies no root leaves that tab empty; every
    /// front end resolves its root at startup.
    #[must_use]
    pub fn with_workspace_root(mut self, root: WorkspaceRootDto) -> Self {
        self.browser.workspace_root = Some(root);
        self
    }

    /// Returns the effects that connect a freshly created state.
    #[must_use]
    pub fn start(&self) -> Vec<Effect> {
        vec![Effect::Connect]
    }

    /// Applies one action and returns the client effects it asks for.
    ///
    /// The transition is total: every action leaves a coherent state, and a
    /// failure carried by an action becomes the error line instead of an error
    /// return, so a front end never has to recover from a state transition.
    ///
    /// Two cross-cutting rules live here. Each layered arm - the Ctrl+C arm
    /// and the Esc clear arm - is consecutive over the user's own actions, so
    /// any other action the user asks for disarms it at its entry point, and
    /// `Action::is_client_report` leaves it standing. And a produced
    /// `Effect::SendTurn` starts the waiting phase of the run it requests,
    /// whichever transition produced it, so the status row names that run from
    /// the moment its request leaves for the network.
    pub fn update(&mut self, action: Action) -> Vec<Effect> {
        // Each layered arm is consecutive over the user's own actions: any
        // other user action clears it - including the other key - while a
        // client report leaves it standing for the second press.
        if !action.is_client_report() {
            if !matches!(action, Action::CtrlCPressed) {
                self.ctrl_c_arm = None;
            }
            if !matches!(action, Action::EscapePressed) {
                self.escape_arm = false;
            }
        }
        let effects = match action {
            Action::Bootstrapped(health) => self.apply_bootstrapped(health.readiness()),
            Action::BootstrapFailed(error) => self.apply_bootstrap_failed(&error),
            Action::SessionsListed(summaries) => self.apply_sessions_listed(&summaries),
            Action::SessionsListFailed(error) => self.apply_failure(&error),
            Action::SessionSnapshotLoaded(snapshot) => {
                let effects = self.apply_session_snapshot(snapshot);
                // A snapshot re-reads committed state: the phase of the run the
                // previous state mirrored does not describe the session it
                // opens. The snapshot's own run, when it is still live, starts
                // as waiting for the first token this state will see.
                self.rebase_run_phase();
                effects
            }
            Action::SessionSnapshotFailed(error) => self.apply_session_open_failure(&error),
            Action::SessionCreated(session_id) => self.apply_session_created(session_id),
            Action::SessionCreateFailed(error) => self.apply_session_open_failure(&error),
            Action::SettingsReceived(theme) => self.apply_settings_received(theme),
            Action::SettingsFailed(error) => self.apply_settings_failed(&error),
            Action::ThemeSelected(theme) => self.apply_theme_selected(theme),
            Action::ThemePreviewed(theme) => self.apply_theme_previewed(theme),
            Action::ThemePreviewCleared => self.apply_theme_preview_cleared(),
            Action::SessionStreamOpened(state) => self.apply_session_stream_opened(state),
            Action::SessionStreamFailed(error) => self.apply_session_stream_failed(&error),
            Action::SessionStreamEnded => self.apply_session_stream_ended(),
            Action::FrameReceived(frame) => self.apply_frame(frame),
            Action::TurnAccepted(outcome) => self.apply_turn_accepted(outcome),
            Action::ElapsedReported { millis } => self.apply_elapsed_reported(millis),
            Action::TurnFailed(error) => self.apply_turn_send_failed(&error),
            Action::InterruptAccepted => self.apply_interrupt_accepted(),
            Action::InterruptFailed(error) => self.apply_failure(&error),
            Action::RunCancelAccepted => self.apply_run_cancel_accepted(),
            Action::RunCancelFailed(error) => self.apply_failure(&error),
            Action::InputChar(character) => self.apply_input_char(character),
            Action::InputBackspace => self.apply_input_backspace(),
            Action::MoveInputCursor(movement) => {
                self.apply_input_cursor_move(movement);
                Vec::new()
            }
            Action::NavigateInputHistory(movement) => {
                self.apply_input_history_move(movement);
                Vec::new()
            }
            Action::MenuMove(movement) => {
                self.move_menu(movement);
                Vec::new()
            }
            Action::MenuAccept => {
                self.accept_menu();
                Vec::new()
            }
            Action::InputSubmitted => self.submit_input(),
            Action::TurnSubmitted { content } => self.submit_turn(content),
            Action::ClearInput => self.clear_input_into_history(),
            Action::SessionsBrowserRequested => self.request_sessions_browser(),
            Action::SessionsBrowserClosed => self.close_sessions_browser(),
            Action::BrowserTabNext => self.browser_tab_next(),
            Action::BrowserTabPrevious => self.browser_tab_previous(),
            Action::BrowserFilterChar(character) => self.apply_browser_filter_char(character),
            Action::BrowserFilterBackspace => self.apply_browser_filter_backspace(),
            Action::BrowserCursorMove(movement) => self.apply_browser_cursor_move(movement),
            Action::BrowserSelectionRequested => self.select_browser_session(),
            Action::BrowserRenameRequested => self.request_session_rename(),
            Action::BrowserArchiveRequested => self.request_session_archive(),
            Action::BrowserTreeRequested => self.request_session_tree(),
            Action::NewSessionRequested => self.request_new_session(),
            Action::CtrlCPressed => self.apply_ctrl_c(),
            Action::EscapePressed => self.apply_escape(),
            Action::InterruptRequested => self.request_interrupt(),
            Action::CancelRunRequested => self.request_cancel(),
            Action::ReconnectRequested => self.request_reconnect(),
            Action::SelectTranscriptRows { anchor, extent } => self.apply_selection(anchor, extent),
            Action::ExpandReasoning { row } => self.apply_expand_reasoning(row),
            Action::ScrollTranscriptFast(direction) => {
                self.apply_scroll_fast(direction);
                Vec::new()
            }
            Action::ScrollTranscript(direction) => {
                self.apply_scroll(direction);
                Vec::new()
            }
            Action::Quit => {
                self.quit = true;
                Vec::new()
            }
        };
        // A turn request leaving for the network starts the run the row names:
        // the run that request begins is waiting for its first streamed token,
        // and a previous run's phase never describes it.
        if effects
            .iter()
            .any(|effect| matches!(effect, Effect::SendTurn { .. }))
        {
            self.apply_turn_dispatched();
        }
        effects
    }

    /// Returns the current connection status.
    #[must_use]
    pub const fn connection(&self) -> ConnectionStatus {
        self.connection
    }

    /// Returns the screen a front end renders.
    #[must_use]
    pub const fn screen(&self) -> Screen {
        self.screen
    }

    /// Returns the colour theme a front end renders the session through.
    ///
    /// This is the committed theme: the value the daemon accepted and stored.
    /// A front end paints [`AppState::effective_theme`] instead, which is the
    /// preview while one is active and this value at every other moment.
    #[must_use]
    pub const fn theme(&self) -> Theme {
        self.theme
    }

    /// Returns the theme a front end paints: the preview, else the committed
    /// theme.
    ///
    /// The whole view resolves its palette from this one value, so a picker
    /// preview repaints the window through the candidate - the transcript
    /// layout cache included - while the committed theme stays what the daemon
    /// carries.
    #[must_use]
    pub const fn effective_theme(&self) -> Theme {
        match self.theme_preview {
            Some(theme) => theme,
            None => self.theme,
        }
    }

    /// Returns the bounded session summaries the daemon reported.
    #[must_use]
    pub fn sessions(&self) -> &[SessionSummaryDto] {
        &self.sessions
    }

    /// Returns how many sessions exist beyond the reported window.
    #[must_use]
    pub const fn sessions_omitted(&self) -> u32 {
        self.sessions_omitted
    }

    /// Returns whether a session list read completed.
    #[must_use]
    pub const fn sessions_loaded(&self) -> bool {
        self.sessions_loaded
    }

    /// Returns the tab the sessions browser shows.
    #[must_use]
    pub const fn browser_tab(&self) -> BrowserTab {
        self.browser.tab
    }

    /// Returns the text the sessions browser filters its rows by.
    #[must_use]
    pub fn browser_filter(&self) -> &str {
        &self.browser.filter
    }

    /// Returns the filtered rows the sessions browser shows, newest first.
    #[must_use]
    pub fn browser_rows(&self) -> &[BrowserRow] {
        &self.browser.rows
    }

    /// Returns the index of the row the sessions browser marks.
    ///
    /// The index is always a row of [`AppState::browser_rows`] when that slice
    /// is not empty, and zero when it is.
    #[must_use]
    pub const fn browser_cursor(&self) -> usize {
        self.browser.cursor
    }

    /// Returns the explanation the sessions browser shows in place of rows.
    ///
    /// A tab the core cannot fill, a filter that selects nothing, and a list
    /// that has not been read each carry their own text; `None` means the
    /// browser has rows, or that an empty result needs no explanation.
    #[must_use]
    pub const fn browser_empty_reason(&self) -> Option<&'static str> {
        self.browser.empty_reason(self.sessions_loaded)
    }

    /// Returns the session the front end currently shows.
    #[must_use]
    pub const fn session_id(&self) -> Option<SessionId> {
        self.session_id
    }

    /// Returns the run policy mode the open session's projection declared.
    ///
    /// `None` before a session snapshot arrived; the mode is durable session
    /// state, so the input badges show the core's value and never a guess.
    #[must_use]
    pub const fn session_mode(&self) -> Option<RunModeDto> {
        self.session_mode
    }

    /// Returns the effective provider model the open session's projection names.
    ///
    /// `None` before a session snapshot arrived; the model is the daemon's
    /// resolved configuration selection, so the input badge shows the core's
    /// value and never a guess.
    #[must_use]
    pub fn session_model(&self) -> Option<&str> {
        self.session_model.as_deref()
    }

    /// Returns the elapsed time the front end measured for the current or last turn.
    #[must_use]
    pub const fn elapsed_millis(&self) -> Option<u64> {
        self.elapsed_millis
    }

    /// Returns the committed transcript rows of the current session.
    ///
    /// The rows are the client's own reconciled list: this front end holds no
    /// second projection, so what a pane renders is exactly what the client
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

    /// Returns the version of the committed transcript.
    ///
    /// Every transcript mutation moves it forward: an append, a reconciliation,
    /// a read, and a front drain each leave a new value. A renderer caches what
    /// it laid out under the epoch it saw, and one comparison tells it whether
    /// the rows it holds are still current.
    #[must_use]
    pub fn transcript_epoch(&self) -> u64 {
        self.session_state
            .as_ref()
            .map_or(0, SessionStreamState::transcript_epoch)
    }

    /// Returns the transcript display rows the pointer selected, if any.
    ///
    /// The range is measured in display rows of the laid-out transcript and the
    /// view paints it; a press and release without a drag leaves the pressed
    /// row alone, and a replacement or a front trim drops the selection.
    #[must_use]
    pub const fn transcript_selection(&self) -> Option<TranscriptSelection> {
        self.selection
    }

    /// Returns how many extra display rows one reasoning block reveals.
    ///
    /// The row is the block's committed transcript row; `0` means the block is
    /// at its collapsed length. The state is keyed by that row, so it survives
    /// appends, and a replacement or a front trim clears it.
    #[must_use]
    pub fn reasoning_expansion(&self, row: usize) -> usize {
        self.reasoning_expansions
            .iter()
            .find(|expansion| expansion.row == row)
            .map_or(0, |expansion| expansion.extra_rows)
    }

    /// Returns the version of the reasoning expansion state.
    ///
    /// Every activation moves it forward, and a replacement or trim moves it
    /// too: the layout a cache holds stops being current, because an expansion
    /// inserts display rows into the middle of the transcript.
    #[must_use]
    pub const fn reasoning_epoch(&self) -> u64 {
        self.reasoning_epoch
    }

    /// Returns how many committed `tool_result` rows the transcript carries.
    ///
    /// A tool block pairs a call row with its result row, so a committed result
    /// changes a block the cached prefix holds; a layout cache compares this
    /// count to know whether its prefix still reflects the pairing.
    #[must_use]
    pub fn tool_result_count(&self) -> usize {
        self.transcript()
            .iter()
            .filter(|row| row.kind() == MessageKindDto::ToolResult)
            .count()
    }

    /// Returns how many transcript rows were appended since `epoch`, if the
    /// transcript has only grown by appends since then.
    ///
    /// `None` means a reconciliation, a read, a front drain, a session switch,
    /// or a future epoch: the rows current at `epoch` are no longer a prefix of
    /// the transcript, so a caller holding them must lay the whole transcript
    /// out again. `Some(0)` means the transcript did not change at all.
    #[must_use]
    pub fn transcript_appended_since(&self, epoch: u64) -> Option<usize> {
        self.session_state
            .as_ref()
            .and_then(|state| state.transcript_appended_since(epoch))
    }

    /// Moves the display state past a transcript mutation that is not an append.
    ///
    /// A reconciliation, a read, and a front drain each move every retained row,
    /// so every display-row offset may now point at another row: the selection
    /// and every reasoning expansion are dropped with it.
    pub(super) fn note_replacement(&mut self) {
        self.selection = None;
        self.reasoning_expansions.clear();
        self.reasoning_epoch += 1;
        self.refresh_tool_watch();
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

    /// Returns the last known run projection.
    ///
    /// The projection is the client's: a status frame replaces it, so a
    /// finished run keeps reporting its terminal status until another run
    /// starts, and a lost stream leaves the last projection the session
    /// reported in place.
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

    /// Returns the live phase of the run the status row names, if one is live.
    ///
    /// The phase follows the run's own transitions: it starts at
    /// [`RunPhase::Waiting`] the moment a turn request leaves the state, moves
    /// with the stream's reasoning and answer channels, and names
    /// [`RunPhase::Working`] while a committed tool call is in flight past the
    /// working threshold. A run whose projection reached a terminal status
    /// carries no phase; the status row shows that projection's terminal word.
    #[must_use]
    pub const fn run_phase(&self) -> Option<RunPhase> {
        if self.tool_working {
            return Some(RunPhase::Working);
        }
        self.phase
    }

    /// Returns the state of the live run-stream subscription.
    #[must_use]
    pub const fn stream(&self) -> StreamStatus {
        self.stream
    }

    /// Returns the input's command hint menu, while one is open.
    ///
    /// The menu is open exactly while the input's first character is `/` and
    /// the caret sits in or immediately after that word and at least one
    /// registered command matches it; every one of those conditions is a
    /// function of the input line, the caret, and the registry, so a front end
    /// only reads this value and never decides for itself.
    #[must_use]
    pub const fn command_menu(&self) -> Option<&CommandMenu> {
        self.command_menu.as_ref()
    }

    /// Returns the transient provisional answer text of the run's current model
    /// step.
    #[must_use]
    pub fn provisional_text(&self) -> &str {
        self.session_state
            .as_ref()
            .map_or("", SessionStreamState::provisional_text)
    }

    /// Returns the transient provisional reasoning of the run's current model
    /// step.
    ///
    /// The reasoning channel streams before the answer it informs and commits
    /// as that answer's reasoning attachment, so the live segment the pane
    /// renders from it is the reasoning block's own streaming form.
    #[must_use]
    pub fn provisional_reasoning(&self) -> &str {
        self.session_state
            .as_ref()
            .map_or("", SessionStreamState::provisional_reasoning)
    }

    /// Returns the committed row the live reasoning segment will become.
    ///
    /// A step's running reasoning belongs to the assistant row that step will
    /// commit, which is exactly the transcript's next row. Anchoring the live
    /// segment there makes its expansion state the committed block's: the row
    /// key is the same before and after the commit, so a reader who expanded
    /// the streaming reasoning still sees it expanded once it is committed.
    #[must_use]
    pub fn live_reasoning_row(&self) -> usize {
        self.transcript().len()
    }

    /// Returns the current input line.
    #[must_use]
    pub fn input(&self) -> &str {
        &self.input
    }

    /// Returns the input line's caret position, measured in characters.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Returns the single-line notice of the last completed transition.
    #[must_use]
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// Returns the single-line failure the last transition reported.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Returns how many transcript lines the window is scrolled towards older rows.
    #[must_use]
    pub const fn scroll(&self) -> u16 {
        self.scroll
    }

    /// Returns whether the user asked to leave the front end.
    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.quit
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

    /// Records the single-line notice of one completed transition.
    pub(super) fn note(&mut self, text: String) {
        self.notice = Some(text);
    }
}
