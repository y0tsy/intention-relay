//! The render-free state machine the terminal front ends drive.
//!
//! This module owns the state structure, its accessors, and the one `update`
//! dispatch. The transition bodies live one file per feature beside it:
//! `sessions.rs`, `browser.rs`, `run.rs`, `input.rs`, `transcript.rs`, and
//! `ctrl_c.rs`.

use intention_client::RunStreamState;
use intention_proto::{
    ErrorDto, MessageKindDto, MessageProjectionDto, RunModeDto, RunProjectionDto, RunStatusDto,
    SessionId, SessionSummaryDto,
};

use super::{
    Action, BrowserRow, BrowserTab, CommandMenu, ConnectionStatus, Effect, Screen, StreamStatus,
    Theme, TranscriptSelection, browser, ctrl_c, error_text, transcript,
};

/// Every value one terminal session holds, with no terminal attached.
///
/// The state owns the session list, the current session's committed transcript,
/// the live run mirror, the input line, and the status text; renderers read it
/// and the front end mutates it only through [`AppState::update`]. The fields
/// are visible to the transition modules beside this one and to nothing else.
pub struct AppState {
    pub(super) initial_session: Option<SessionId>,
    /// Whether the caller explicitly asked to continue the newest session.
    ///
    /// It is the command line's `--continue`, and it is the only launch
    /// request that opens a session the caller did not name. When the session
    /// list arrives, the newest session it carries is opened; without the
    /// request, a launch with no selected session opens nothing.
    pub(super) continue_session: bool,
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
    pub(super) transcript: Vec<MessageProjectionDto>,
    /// The committed transcript's version: every mutation moves it forward.
    pub(super) transcript_epoch: u64,
    /// The epoch a replacement left the transcript at.
    ///
    /// Only rows appended at or after this epoch keep the transcript a prefix
    /// of what it was, so a query before it can never be answered append-only.
    pub(super) transcript_append_base: u64,
    /// The epoch of every row appended since [`Self::transcript_append_base`],
    /// in append order.
    ///
    /// The epoch of a row is the value the transcript had right after the row
    /// joined it, so the count of rows appended after any epoch in the run is a
    /// partition point rather than a scan.
    pub(super) transcript_appends: Vec<u64>,
    pub(super) active_run: Option<RunProjectionDto>,
    /// The elapsed time the front end measured for the current or last turn.
    pub(super) elapsed_millis: Option<u64>,
    pub(super) run_stream: Option<RunStreamState>,
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
    /// How many committed `tool_result` rows the transcript carries.
    ///
    /// A tool block pairs a call with its result, so a committed result changes
    /// an earlier block: the layout cache compares this count to know whether
    /// its prefix still reflects the pairing.
    pub(super) tool_results: usize,
    pub(super) quit: bool,
}

impl AppState {
    /// Creates the state a front end starts from, pinned to one session to open.
    ///
    /// `initial_session` is the session the caller selected on the command line;
    /// `None` opens nothing, so a launch shows the chat's welcome state until
    /// the user asks for a session. A caller that wants the newest session
    /// opened asks for it explicitly with [`AppState::continuing`]. The state
    /// needs no terminal, daemon, or clock to exist.
    #[must_use]
    pub const fn new(initial_session: Option<SessionId>) -> Self {
        Self {
            initial_session,
            continue_session: false,
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
            transcript: Vec::new(),
            transcript_epoch: 0,
            transcript_append_base: 0,
            transcript_appends: Vec::new(),
            active_run: None,
            elapsed_millis: None,
            run_stream: None,
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
            tool_results: 0,
            quit: false,
        }
    }

    /// Returns the same state asking to continue the newest session, or not.
    ///
    /// The request is explicit: it is the command line's `--continue`, and it
    /// is resolved when the daemon-ordered session list arrives. Without it - a
    /// launch with no selected session - the state opens no session at all and
    /// the chat shows its welcome state until the user asks for one.
    #[must_use]
    pub const fn continuing(mut self, continue_session: bool) -> Self {
        self.continue_session = continue_session;
        self
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
    /// The one cross-cutting rule lives here: each layered arm - the Ctrl+C
    /// arm and the Esc clear arm - is consecutive over the user's own actions,
    /// so any other action the user asks for disarms it at its entry point, and
    /// `Action::is_client_report` leaves it standing.
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
        match action {
            Action::Bootstrapped(health) => self.apply_bootstrapped(health.readiness()),
            Action::BootstrapFailed(error) => self.apply_bootstrap_failed(&error),
            Action::SessionsListed(summaries) => self.apply_sessions_listed(&summaries),
            Action::SessionsListFailed(error) => self.apply_failure(&error),
            Action::SessionSnapshotLoaded(snapshot) => self.apply_session_snapshot(snapshot),
            Action::SessionSnapshotFailed(error) => self.apply_session_open_failure(&error),
            Action::SessionCreated(session_id) => self.apply_session_created(session_id),
            Action::SessionCreateFailed(error) => self.apply_session_open_failure(&error),
            Action::SettingsReceived(theme) => self.apply_settings_received(theme),
            Action::SettingsFailed(error) => self.apply_settings_failed(&error),
            Action::ThemeSelected(theme) => self.apply_theme_selected(theme),
            Action::ThemePreviewed(theme) => self.apply_theme_previewed(theme),
            Action::ThemePreviewCleared => self.apply_theme_preview_cleared(),
            Action::RunStreamOpened(state) => self.apply_run_stream_opened(state),
            Action::RunStreamFailed(error) => self.apply_run_stream_failed(&error),
            Action::RunStreamEnded => self.apply_run_stream_ended(),
            Action::FrameReceived(frame) => self.apply_frame(frame),
            Action::TurnAccepted(outcome) => self.apply_turn_accepted(outcome),
            Action::ElapsedReported { millis } => self.apply_elapsed_reported(millis),
            Action::TurnFailed(error) => self.apply_failure(&error),
            Action::InterruptAccepted => self.apply_interrupt_accepted(),
            Action::InterruptFailed(error) => self.apply_failure(&error),
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
        }
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

    /// Returns the elapsed time the front end measured for the current or last turn.
    #[must_use]
    pub const fn elapsed_millis(&self) -> Option<u64> {
        self.elapsed_millis
    }

    /// Returns the committed transcript rows of the current session.
    #[must_use]
    pub fn transcript(&self) -> &[MessageProjectionDto] {
        &self.transcript
    }

    /// Returns the version of the committed transcript.
    ///
    /// Every transcript mutation moves it forward: an append, a replacement,
    /// and a trim's front drain each leave a new value. A renderer caches what
    /// it laid out under the epoch it saw, and one comparison tells it whether
    /// the rows it holds are still current.
    #[must_use]
    pub const fn transcript_epoch(&self) -> u64 {
        self.transcript_epoch
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
    pub const fn tool_result_count(&self) -> usize {
        self.tool_results
    }

    /// Returns how many transcript rows were appended since `epoch`, if the
    /// transcript has only grown by appends since then.
    ///
    /// `None` means a replacement, a trim's front drain, a session switch, or a
    /// future epoch: the rows current at `epoch` are no longer a prefix of the
    /// transcript, so a caller holding them must lay the whole transcript out
    /// again. `Some(0)` means the transcript did not change at all.
    #[must_use]
    pub fn transcript_appended_since(&self, epoch: u64) -> Option<usize> {
        if epoch > self.transcript_epoch || epoch < self.transcript_append_base {
            return None;
        }
        let before = self
            .transcript_appends
            .partition_point(|mark| *mark <= epoch);
        Some(self.transcript_appends.len() - before)
    }

    /// Appends one committed row, recording the append.
    pub(super) fn push_transcript_row(&mut self, row: MessageProjectionDto) {
        self.tool_results += usize::from(row.kind() == MessageKindDto::ToolResult);
        self.transcript.push(row);
        self.transcript_epoch += 1;
        self.transcript_appends.push(self.transcript_epoch);
    }

    /// Appends committed rows, recording each append.
    pub(super) fn extend_transcript(
        &mut self,
        rows: impl IntoIterator<Item = MessageProjectionDto>,
    ) {
        for row in rows {
            self.push_transcript_row(row);
        }
    }

    /// Replaces every committed row, resetting the append-only marker.
    pub(super) fn replace_transcript(&mut self, rows: &[MessageProjectionDto]) {
        self.transcript.clear();
        self.transcript.extend_from_slice(rows);
        self.note_replacement();
    }

    /// Moves the epoch past a mutation that is not an append.
    ///
    /// A row that is dropped in front of the transcript - a trim or a session
    /// switch - changes every cached row offset, so the append-only run ends
    /// here and a stale cache always lays out whole. Every display-row offset
    /// may now point at another row, so the selection and every reasoning
    /// expansion are dropped with it.
    pub(super) fn note_replacement(&mut self) {
        self.transcript_epoch += 1;
        self.transcript_appends.clear();
        self.transcript_append_base = self.transcript_epoch;
        self.selection = None;
        self.reasoning_expansions.clear();
        self.reasoning_epoch += 1;
        self.tool_results = self
            .transcript
            .iter()
            .filter(|row| row.kind() == MessageKindDto::ToolResult)
            .count();
    }

    /// Returns the last known run projection.
    ///
    /// A status frame replaces it, so a finished run keeps reporting its terminal
    /// status until another run starts.
    #[must_use]
    pub const fn active_run(&self) -> Option<RunProjectionDto> {
        self.active_run
    }

    /// Returns the last known run lifecycle status.
    #[must_use]
    pub fn run_status(&self) -> Option<RunStatusDto> {
        self.active_run.map(|run| run.status())
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
        self.run_stream
            .as_ref()
            .map_or("", RunStreamState::provisional_text)
    }

    /// Returns the transient provisional reasoning of the run's current model
    /// step.
    ///
    /// The reasoning channel streams before the answer it informs and commits
    /// as that answer's reasoning attachment, so the live segment the pane
    /// renders from it is the reasoning block's own streaming form.
    #[must_use]
    pub fn provisional_reasoning(&self) -> &str {
        self.run_stream
            .as_ref()
            .map_or("", RunStreamState::provisional_reasoning)
    }

    /// Returns the committed row the live reasoning segment will become.
    ///
    /// A step's running reasoning belongs to the assistant row that step will
    /// commit, which is exactly the transcript's next row. Anchoring the live
    /// segment there makes its expansion state the committed block's: the row
    /// key is the same before and after the commit, so a reader who expanded
    /// the streaming reasoning still sees it expanded once it is committed.
    #[must_use]
    pub const fn live_reasoning_row(&self) -> usize {
        self.transcript.len()
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
    /// an error.
    pub(super) fn apply_failure(&mut self, error: &ErrorDto) -> Vec<Effect> {
        self.error = Some(error_text(error));
        Vec::new()
    }

    /// Records the single-line notice of one completed transition.
    pub(super) fn note(&mut self, text: String) {
        self.notice = Some(text);
    }
}
