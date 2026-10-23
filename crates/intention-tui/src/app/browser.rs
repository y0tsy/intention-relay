//! The sessions-browser transitions: the browser card, its tabs, its filter,
//! and its cursor.
//!
//! The browser is one surface of the same render-free core the chat uses, drawn
//! as a card over the chat inside the one window. It computes the rows a view
//! renders - filtered, recency-ordered, and clamped to a valid cursor row - so
//! the view stays a pure function of them. A tab exists only for a fact the
//! session summary carries: the workspace binding and the whole list.

use intention_proto::{RunModeDto, SessionId, SessionSummaryDto, WorkspaceRootDto};

use super::{AppState, BrowserCursorMove, Effect, Screen, short_identifier};

/// The empty state of a filter that selects no row.
const NO_MATCHES_REASON: &str = "no session matches the filter";

/// The empty state while no session list has been read yet.
const READING_SESSIONS_REASON: &str = "reading sessions";

/// The notice the selection key shows when the result has no row.
const NO_SELECTION_NOTICE: &str = "no session to select";

/// The tabs the sessions browser offers, in the order the radio row shows them.
///
/// Each tab selects by a fact the session summary carries, so every tab can
/// have rows: the workspace root a session is durably bound to, and the list
/// itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserTab {
    /// Sessions bound to the workspace root this front end runs in.
    CurrentFolder,
    /// Every listed session, most recently updated first.
    All,
}

impl BrowserTab {
    /// Every tab, in the order the radio row shows them.
    pub const ALL: [Self; 2] = [Self::CurrentFolder, Self::All];

    /// Returns the label the radio row shows for this tab.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::CurrentFolder => "Current Folder",
            Self::All => "All",
        }
    }

    /// Returns the next tab, wrapping from the last one to the first.
    const fn next(self) -> Self {
        match self {
            Self::CurrentFolder => Self::All,
            Self::All => Self::CurrentFolder,
        }
    }

    /// Returns the previous tab, wrapping from the first one to the last.
    const fn previous(self) -> Self {
        match self {
            Self::CurrentFolder => Self::All,
            Self::All => Self::CurrentFolder,
        }
    }
}

/// One session row the browser shows.
///
/// The row carries the durable summary facts the table renders - when the
/// session was created and last updated, how many committed rows it holds, and
/// the workspace root it is bound to - beside the placeholder title the core
/// has, so the filter matches exactly the text the view renders and no later
/// layer derives it again.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserRow {
    session_id: SessionId,
    title: String,
    mode: RunModeDto,
    created_at: i64,
    updated_at: i64,
    workspace_root: WorkspaceRootDto,
    message_count: u64,
}

impl BrowserRow {
    /// Returns the row of one listed session summary.
    fn from_summary(summary: &SessionSummaryDto) -> Self {
        Self {
            session_id: summary.session_id(),
            title: placeholder_title(summary.session_id()),
            mode: summary.mode(),
            created_at: summary.created_at().unix_seconds(),
            updated_at: summary.updated_at().unix_seconds(),
            workspace_root: summary.workspace_root().clone(),
            message_count: summary.message_count(),
        }
    }

    /// Returns the durable session identity behind this row.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the visible title of this row.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the run mode this row shows and matches.
    #[must_use]
    pub const fn mode(&self) -> RunModeDto {
        self.mode
    }

    /// Returns when the durable session was created, in whole Unix seconds.
    #[must_use]
    pub const fn created_at(&self) -> i64 {
        self.created_at
    }

    /// Returns the last durable update of the session, in whole Unix seconds.
    #[must_use]
    pub const fn updated_at(&self) -> i64 {
        self.updated_at
    }

    /// Returns the workspace root the session is durably bound to.
    #[must_use]
    pub fn workspace_root(&self) -> &str {
        self.workspace_root.as_str()
    }

    /// Returns how many committed transcript rows the session holds.
    #[must_use]
    pub const fn message_count(&self) -> u64 {
        self.message_count
    }
}

/// Everything the sessions browser carries while its screen is open.
///
/// The rows are the filtered result in recency order, so the fields a view
/// reads - the tab, the filter, the rows, and the cursor - are the same values
/// the transitions computed; a view never filters, sorts, or clamps.
pub(super) struct SessionsBrowserState {
    pub(super) tab: BrowserTab,
    pub(super) filter: String,
    /// The front end's own workspace root, when it declared one.
    ///
    /// `Current Folder` selects the sessions bound to exactly this root; a
    /// state that never received one selects no session there.
    pub(super) workspace_root: Option<WorkspaceRootDto>,
    pub(super) rows: Vec<BrowserRow>,
    pub(super) cursor: usize,
}

impl SessionsBrowserState {
    /// Creates the browser of a front end that has not opened it yet.
    pub(super) const fn new() -> Self {
        Self {
            // `All` is the deliberate deviation from the mock's `Current
            // Folder` default: the browser opens on the whole list, and a state
            // with no declared workspace root still shows sessions instead of
            // an empty current folder.
            tab: BrowserTab::All,
            filter: String::new(),
            workspace_root: None,
            rows: Vec::new(),
            cursor: 0,
        }
    }

    /// Opens the browser on its default tab over the current session list.
    pub(super) fn open(&mut self, sessions: &[SessionSummaryDto]) {
        self.tab = BrowserTab::All;
        self.filter.clear();
        self.cursor = 0;
        self.refresh(sessions);
    }

    /// Closes the browser, dropping the rows it showed.
    pub(super) fn close(&mut self) {
        self.rows.clear();
        self.cursor = 0;
    }

    /// Recomputes the rows the current tab and filter select.
    ///
    /// The rows keep the daemon's own order: a session list is a recency-ordered
    /// contract, so the browser shows the order it received instead of
    /// re-sorting a raw timestamp.
    pub(super) fn refresh(&mut self, sessions: &[SessionSummaryDto]) {
        self.rows = sessions
            .iter()
            .filter(|summary| self.matches(summary))
            .map(BrowserRow::from_summary)
            .collect();
        self.clamp_cursor();
    }

    /// Returns whether one summary survives the current tab and filter.
    fn matches(&self, summary: &SessionSummaryDto) -> bool {
        // `Current Folder` selects by the durable binding: a session belongs to
        // the front end's folder only when its own workspace root is that root.
        if self.tab == BrowserTab::CurrentFolder
            && self.workspace_root.as_ref() != Some(summary.workspace_root())
        {
            return false;
        }
        // @todo(core): the filter runs over the placeholder title and the mode;
        // durable session titles would let a user search by name.
        let candidate = format!(
            "{} {}",
            placeholder_title(summary.session_id()),
            summary.mode().as_str()
        );
        fuzzy_match(&self.filter, &candidate)
    }

    /// Keeps the cursor on a row of the current result, or at zero without rows.
    fn clamp_cursor(&mut self) {
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
    }

    /// Returns the empty-state text of the current tab and filter, if any.
    pub(super) const fn empty_reason(&self, sessions_loaded: bool) -> Option<&'static str> {
        if self.rows.is_empty() {
            if !self.filter.is_empty() {
                return Some(NO_MATCHES_REASON);
            }
            if !sessions_loaded {
                return Some(READING_SESSIONS_REASON);
            }
        }
        None
    }
}

impl AppState {
    /// Opens the sessions browser and refreshes the session list behind it.
    pub(super) fn request_sessions_browser(&mut self) -> Vec<Effect> {
        self.screen = Screen::Sessions;
        // A message that belonged to the chat screen does not outlive it: the
        // browser footer shows a notice of its own keys and nothing stale.
        self.notice = None;
        self.browser.open(&self.sessions);
        vec![Effect::ListSessions]
    }

    /// Closes the sessions browser without touching the current session.
    pub(super) fn close_sessions_browser(&mut self) -> Vec<Effect> {
        self.screen = Screen::Chat;
        self.browser.close();
        Vec::new()
    }

    /// Switches the browser to the next tab and re-selects its rows.
    pub(super) fn browser_tab_next(&mut self) -> Vec<Effect> {
        self.set_browser_tab(self.browser.tab.next());
        Vec::new()
    }

    /// Switches the browser to the previous tab and re-selects its rows.
    pub(super) fn browser_tab_previous(&mut self) -> Vec<Effect> {
        self.set_browser_tab(self.browser.tab.previous());
        Vec::new()
    }

    /// Appends one typed character to the browser filter.
    pub(super) fn apply_browser_filter_char(&mut self, character: char) -> Vec<Effect> {
        self.browser.filter.push(character);
        self.browser.refresh(&self.sessions);
        Vec::new()
    }

    /// Removes the last character of the browser filter.
    pub(super) fn apply_browser_filter_backspace(&mut self) -> Vec<Effect> {
        if self.browser.filter.pop().is_some() {
            self.browser.refresh(&self.sessions);
        }
        Vec::new()
    }

    /// Moves the browser cursor one row, stopping at either end of the result.
    pub(super) fn apply_browser_cursor_move(&mut self, movement: BrowserCursorMove) -> Vec<Effect> {
        let last = self.browser.rows.len().saturating_sub(1);
        self.browser.cursor = match movement {
            BrowserCursorMove::Up => self.browser.cursor.saturating_sub(1),
            BrowserCursorMove::Down => self.browser.cursor.saturating_add(1).min(last),
        };
        Vec::new()
    }

    /// Opens the session the cursor points at and returns to the chat screen.
    pub(super) fn select_browser_session(&mut self) -> Vec<Effect> {
        let Some(row) = self.browser.rows.get(self.browser.cursor) else {
            self.notice = Some(NO_SELECTION_NOTICE.to_owned());
            return Vec::new();
        };
        let session_id = row.session_id();
        self.screen = Screen::Chat;
        self.browser.close();
        self.note(format!("opening session {}", short_identifier(session_id)));
        vec![Effect::OpenSession(session_id)]
    }

    /// Selects one browser tab and rebuilds the rows it shows.
    fn set_browser_tab(&mut self, tab: BrowserTab) {
        self.browser.tab = tab;
        self.browser.refresh(&self.sessions);
    }
}

/// Returns the placeholder title of one session.
// @todo(core): durable session titles; until the summary carries one, the title
// column and the filter see `session <short-id>`.
fn placeholder_title(session_id: SessionId) -> String {
    format!("session {}", short_identifier(session_id))
}

/// Returns whether `needle` is a case-insensitive subsequence of `haystack`.
///
/// Every filter character must appear in the candidate, in order, with anything
/// between them; an empty filter matches every candidate. Lowercasing is per
/// character, so a filter only has to match the case the user typed.
fn fuzzy_match(needle: &str, haystack: &str) -> bool {
    let mut candidates = haystack.chars().flat_map(char::to_lowercase);
    needle
        .chars()
        .flat_map(char::to_lowercase)
        .all(|wanted| candidates.any(|candidate| candidate == wanted))
}

#[cfg(test)]
mod tests {
    use super::fuzzy_match;

    #[test]
    fn fuzzy_match_follows_the_order_of_the_filter_characters() {
        assert!(fuzzy_match("", "session 11111111 build"));
        assert!(fuzzy_match("s111", "session 11111111 build"));
        assert!(fuzzy_match("BUIL", "session 11111111 build"));
        assert!(
            !fuzzy_match("111999", "session 11111111 build"),
            "a character the candidate lacks rejects it"
        );
        assert!(
            !fuzzy_match("dliub", "session 11111111 build"),
            "characters in the wrong order are not a subsequence"
        );
    }
}
