//! The input transitions: the input line, its cursor, its history, the command
//! hint menu, and submission.

use super::commands::{self, CommandMenu, MenuRow};
use super::{
    AppState, Effect, InputCursorMove, InputHistoryMove, MenuMove, Theme, TranscriptScroll,
};

/// How many transcript display rows one mouse-wheel notch moves.
const TRANSCRIPT_WHEEL_ROWS: u16 = 3;

/// How many transcript display rows one fast drag step moves.
///
/// Five wheel notches: a held pointer that slides past the transcript's edge -
/// and a wheel notch that arrives while the button is held - reaches rows far
/// beyond the window quickly. The step is public because the view computes
/// where the window lands to keep a dragged selection's leading edge on the row
/// the pointer is over.
pub const TRANSCRIPT_DRAG_ROWS: u16 = TRANSCRIPT_WHEEL_ROWS * 5;

impl AppState {
    /// Inserts one typed character at the cursor.
    pub(super) fn apply_input_char(&mut self, character: char) -> Vec<Effect> {
        let offset = self.input_byte_offset(self.cursor);
        self.input.insert(offset, character);
        self.cursor += 1;
        self.refresh_menu();
        Vec::new()
    }

    /// Removes the character before the cursor.
    pub(super) fn apply_input_backspace(&mut self) -> Vec<Effect> {
        if self.cursor == 0 {
            return Vec::new();
        }
        let removed = self.input_byte_offset(self.cursor - 1)..self.input_byte_offset(self.cursor);
        self.input.replace_range(removed, "");
        self.cursor -= 1;
        self.refresh_menu();
        Vec::new()
    }

    /// Moves the cursor one character, one line boundary, or one line end.
    ///
    /// The cursor counts characters of the whole buffer, so `Left` and `Right`
    /// step across a line break like any other character. `Home` and `End` stay
    /// inside the line the cursor is on: `Home` reaches its first character and
    /// `End` its last, whether the line ends at a break or at the buffer's end.
    pub(super) fn apply_input_cursor_move(&mut self, movement: InputCursorMove) {
        let characters: Vec<char> = self.input.chars().collect();
        self.cursor = self.cursor.min(characters.len());
        self.cursor = match movement {
            InputCursorMove::Left => self.cursor.saturating_sub(1),
            InputCursorMove::Right => self.cursor.saturating_add(1).min(characters.len()),
            InputCursorMove::Home => line_start(&characters, self.cursor),
            InputCursorMove::End => line_end(&characters, self.cursor),
        };
        self.refresh_menu();
    }

    /// Recomputes the input's command hint band from the line and the caret.
    ///
    /// The open rule is one function of the two: the line's first character is
    /// `/`, the caret sits in or immediately after a word of that line, and the
    /// word's filter selects at least one row - the registry's commands for the
    /// command word, and a declared argument's values for an argument word.
    /// Anything else - the slash removed, the caret past the word, no match
    /// left - closes the band, and the same refresh reopens it as soon as the
    /// line matches again. A band whose word did not change keeps the row the
    /// user highlighted, so moving the caret inside the word it is already on
    /// never moves the highlight; another character starts the highlight at the
    /// best match.
    pub(super) fn refresh_menu(&mut self) {
        let opened = commands::command_word(&self.input, self.cursor)
            .and_then(|word| CommandMenu::opened(&word));
        self.command_menu = match (self.command_menu.take(), opened) {
            (Some(previous), Some(next)) if previous.ranks_the_same_word(&next) => Some(previous),
            (_, opened) => opened,
        };
    }

    /// Moves the hint menu's highlight one row towards `direction`.
    pub(super) fn move_menu(&mut self, direction: MenuMove) {
        if let Some(menu) = self.command_menu.as_mut() {
            menu.move_highlight(direction);
        }
    }

    /// Commits the band's highlighted row into the input line.
    ///
    /// The word the band was opened for - the command word's leading slash
    /// through its last character, or an argument word's first through its
    /// last - is replaced by the row's completion, which is its label and one
    /// trailing space, so the caret lands ready for the argument after a
    /// command or for the submission after a value. The band then follows the
    /// same open rule: a command that declares an argument reopens it on the
    /// argument word, and a complete command or value closes it.
    pub(super) fn accept_menu(&mut self) {
        let Some(completed) = self
            .command_menu
            .as_ref()
            .and_then(CommandMenu::highlighted)
            .map(MenuRow::completion)
        else {
            return;
        };
        let Some(word) = commands::command_word(&self.input, self.cursor) else {
            self.command_menu = None;
            return;
        };
        let before = self.input[..word.start].chars().count();
        self.input.replace_range(word.start..word.end, &completed);
        self.cursor = before + completed.chars().count();
        self.refresh_menu();
    }

    /// Walks the history of sent and abandoned lines, newest entry first.
    pub(super) fn apply_input_history_move(&mut self, movement: InputHistoryMove) {
        match movement {
            InputHistoryMove::Previous => self.recall_older_line(),
            InputHistoryMove::Next => self.recall_newer_line(),
        }
    }

    /// Replaces the input line with the next older history entry, if one exists.
    ///
    /// The first step saves the line the user was typing as the draft; walking
    /// to the oldest entry stops there instead of leaving the history.
    fn recall_older_line(&mut self) {
        let Some(newest) = self.history.len().checked_sub(1) else {
            return;
        };
        let position = match self.history_position {
            Some(position) => position.saturating_sub(1),
            None => {
                self.history_draft = Some(self.input.clone());
                newest
            }
        };
        self.history_position = Some(position);
        self.set_input(self.history[position].clone());
    }

    /// Steps towards the newest history entry, then back to the preserved draft.
    fn recall_newer_line(&mut self) {
        let Some(position) = self.history_position else {
            return;
        };
        if position + 1 < self.history.len() {
            self.history_position = Some(position + 1);
            self.set_input(self.history[position + 1].clone());
            return;
        }
        // Walking past the newest entry leaves the history for the draft.
        self.history_position = None;
        let draft = self.history_draft.take().unwrap_or_default();
        self.set_input(draft);
    }

    /// Sends the input buffer as a user turn, or runs the slash command it names.
    ///
    /// A backslash immediately before the cursor escapes the Enter press: this
    /// action removes it and inserts a line break in its place, so a multi-line
    /// prompt is typed without being submitted and the backslash itself never
    /// reaches the submitted buffer.
    ///
    /// An open band claims Enter for its completion, except for the empty
    /// argument word an omitted optional argument opens: that band has nothing
    /// to complete, so Enter runs the command itself.
    ///
    /// A prompt with no open session starts the session it needs instead of
    /// being rejected: the state asks for a session creation and remembers the
    /// prompt, and the snapshot that opens the created session sends it as the
    /// first turn.
    pub(super) fn submit_input(&mut self) -> Vec<Effect> {
        if self.insert_line_break() {
            return Vec::new();
        }
        // The band owns Enter while it is open: the press completes the word
        // the user is typing instead of running it, so a line that already
        // spells a complete command needs its second Enter to run, and the
        // caret always lands past the command with one space ready for its
        // arguments. The one band that has nothing to complete is the empty
        // argument word: the command is already complete and none of the
        // argument is typed, so Enter runs the command - `/theme` opens the
        // picker - while Tab still completes the value it highlights.
        if let Some(menu) = self.command_menu.as_ref()
            && !menu.completes_nothing()
        {
            self.accept_menu();
            return Vec::new();
        }
        let content = self.input.trim().to_owned();
        if content.is_empty() {
            self.notice = Some("type a message before sending it".to_owned());
            return Vec::new();
        }
        if let Some(command) = self.input.strip_prefix('/') {
            // One rule decides what a command is, for the menu and for
            // submission alike: the line's first character is the slash, so a
            // line whose first character is anything else - a blank included -
            // is a turn.
            let command = command.trim().to_owned();
            self.clear_input();
            return self.submit_command(&command);
        }
        let Some(session_id) = self.session_id else {
            return self.start_session_for_prompt(content);
        };
        self.history.push(content.clone());
        self.clear_input();
        self.error = None;
        // A new turn starts a new measurement: the previous turn's elapsed time
        // never survives into the next run's status line.
        self.elapsed_millis = None;
        self.note("sending turn".to_owned());
        vec![Effect::SendTurn {
            session_id,
            content,
        }]
    }

    /// Replaces the backslash before the cursor with a line break, if one is
    /// there.
    ///
    /// Returns whether the Enter press inserted a line break instead of
    /// submitting the buffer. The backslash is the escape for that one press:
    /// the same action removes it and inserts the break, so no backslash ever
    /// reaches the submitted prompt, while a backslash not followed by Enter
    /// stays literal. The replacement keeps the character count, so the cursor
    /// stays after the break it just inserted.
    fn insert_line_break(&mut self) -> bool {
        let Some(before) = self.cursor.checked_sub(1) else {
            return false;
        };
        let offset = self.input_byte_offset(before);
        if !self.input[offset..].starts_with('\\') {
            return false;
        }
        self.input.replace_range(offset..offset + 1, "\n");
        self.refresh_menu();
        true
    }

    /// Starts the session one submitted prompt needs.
    ///
    /// The prompt is kept as the state's pending prompt and the session is
    /// created and opened like `/new`; the snapshot that opens it sends the
    /// prompt as its first turn. A second prompt arriving while the first is
    /// still waiting for its session keeps its text on the input line instead
    /// of replacing the pending one.
    fn start_session_for_prompt(&mut self, content: String) -> Vec<Effect> {
        if self.pending_prompt.is_some() {
            self.notice = Some("a session is already being created".to_owned());
            return Vec::new();
        }
        self.history.push(content.clone());
        self.clear_input();
        self.error = None;
        // A new turn starts a new measurement: the previous turn's elapsed time
        // never survives into the next run's status line.
        self.elapsed_millis = None;
        self.note("creating a session".to_owned());
        self.pending_prompt = Some(content);
        vec![Effect::CreateSession]
    }

    /// Runs one slash command, spelled without its leading `/`.
    ///
    /// The name is resolved through the one registry, and the transition it
    /// runs is the one its entry names, so the submission path and the hint
    /// menu can never disagree about which commands exist. An unregistered name
    /// answers with the registry's own list.
    ///
    /// The line is resolved through the one registry, so the words after the
    /// name are matched against the values the command declares and a line that
    /// does not satisfy them answers the notice the declaration builds. The
    /// transition a resolved line runs is the one its entry names.
    fn submit_command(&mut self, command: &str) -> Vec<Effect> {
        let invocation = match commands::resolve_command(command).into_invocation() {
            Ok(invocation) => invocation,
            Err(notice) => {
                self.note(notice);
                return Vec::new();
            }
        };
        match invocation.spec().action {
            commands::CommandAction::NewSession => self.request_new_session(),
            commands::CommandAction::SessionsBrowser => self.request_sessions_browser(),
            commands::CommandAction::Theme => {
                // `/theme`'s one argument is the first the command declares: a
                // value applies that theme, and its absence opens the picker.
                // The declared values are the theme vocabulary itself, so the
                // assertion names the invariant a registry edit would break.
                let spelled = invocation.value(0);
                let selected = spelled.and_then(Theme::from_str);
                debug_assert!(
                    spelled.is_none() || selected.is_some(),
                    "every value /theme declares is a theme name"
                );
                match selected {
                    Some(theme) => self.apply_theme_selected(theme),
                    None => self.request_theme_picker(),
                }
            }
        }
    }

    /// Clears the input line, keeping its text recallable through the history.
    pub(super) fn clear_input_into_history(&mut self) -> Vec<Effect> {
        let content = self.input.trim().to_owned();
        if !content.is_empty() {
            self.history.push(content);
        }
        self.clear_input();
        Vec::new()
    }

    /// Clears the input line and leaves the history at the live line.
    fn clear_input(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.history_position = None;
        self.history_draft = None;
        self.refresh_menu();
    }

    /// Replaces the input line with `content` and puts the cursor at its end.
    fn set_input(&mut self, content: String) {
        self.cursor = content.chars().count();
        self.input = content;
        self.refresh_menu();
    }

    /// Returns the byte offset of character index `index` in the input line.
    ///
    /// The cursor is a character index, so every insertion and removal turns it
    /// into the byte offset `String` operates on; an index at or past the end
    /// maps to the line's end.
    fn input_byte_offset(&self, index: usize) -> usize {
        self.input
            .char_indices()
            .nth(index)
            .map_or(self.input.len(), |(offset, _)| offset)
    }

    /// Moves the transcript window by one wheel notch of display rows.
    pub(super) const fn apply_scroll(&mut self, direction: TranscriptScroll) {
        self.scroll_by(direction, TRANSCRIPT_WHEEL_ROWS);
    }

    /// Moves the transcript window by one fast drag step of display rows.
    pub(super) const fn apply_scroll_fast(&mut self, direction: TranscriptScroll) {
        self.scroll_by(direction, TRANSCRIPT_DRAG_ROWS);
    }

    /// Moves the transcript window `rows` display rows towards `direction`.
    const fn scroll_by(&mut self, direction: TranscriptScroll, rows: u16) {
        match direction {
            TranscriptScroll::Older => {
                self.scroll = self.scroll.saturating_add(rows);
            }
            TranscriptScroll::Newer => {
                self.scroll = self.scroll.saturating_sub(rows);
            }
        }
    }
}

/// Returns the index of the first character of the line holding `cursor`.
fn line_start(characters: &[char], cursor: usize) -> usize {
    characters[..cursor]
        .iter()
        .rposition(|character| *character == '\n')
        .map_or(0, |index| index + 1)
}

/// Returns the index of the line break ending the line holding `cursor`.
///
/// A cursor on the buffer's last line reaches the buffer's end instead, which
/// is that line's end.
fn line_end(characters: &[char], cursor: usize) -> usize {
    characters[cursor..]
        .iter()
        .position(|character| *character == '\n')
        .map_or(characters.len(), |index| cursor + index)
}
